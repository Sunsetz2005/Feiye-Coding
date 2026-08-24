// @vitest-environment jsdom

import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MemoryCandidateV1 } from "@/lib/api";
import { MemoryCandidatesPanel } from "./MemoryCandidatesPanel";

const apiMocks = vi.hoisted(() => ({
  list: vi.fn(),
  create: vi.fn(),
  approve: vi.fn(),
  reject: vi.fn(),
  supersede: vi.fn(),
  delete: vi.fn(),
}));

vi.mock("@/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api")>();
  return {
    ...actual,
    isTauri: () => true,
    memoryCandidatesListV1: apiMocks.list,
    memoryCandidateCreateV1: apiMocks.create,
    memoryCandidateApproveV1: apiMocks.approve,
    memoryCandidateRejectV1: apiMocks.reject,
    memoryCandidateSupersedeV1: apiMocks.supersede,
    memoryCandidateDeleteV1: apiMocks.delete,
  };
});

const source = { sessionId: "session-1", messageId: "message-1" };

function candidate(
  overrides: Partial<MemoryCandidateV1> = {},
): MemoryCandidateV1 {
  return {
    id: "candidate-pending",
    status: "pending",
    type: "user_preference",
    content: "Prefer concise verification notes.",
    contentHash: "a".repeat(64),
    source,
    ownership: "host_candidate",
    createdAt: "2026-08-24T00:00:00Z",
    updatedAt: "2026-08-24T00:00:00Z",
    ...overrides,
  };
}

beforeEach(() => {
  apiMocks.list.mockResolvedValue([]);
  apiMocks.create.mockResolvedValue(candidate());
  apiMocks.approve.mockResolvedValue(candidate({ status: "approved" }));
  apiMocks.reject.mockResolvedValue(candidate({ status: "rejected" }));
  apiMocks.supersede.mockResolvedValue(candidate({ status: "superseded" }));
  apiMocks.delete.mockResolvedValue(candidate());
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("MemoryCandidatesPanel", () => {
  it("creates only a pending candidate with the visible message source", async () => {
    const user = userEvent.setup();
    render(<MemoryCandidatesPanel locale="en" source={source} />);

    await waitFor(() => expect(apiMocks.list).toHaveBeenCalledTimes(1));
    await user.type(
      screen.getByRole("textbox", { name: "Memory content" }),
      "  Prefer short answers.  ",
    );
    await user.click(
      screen.getByRole("button", { name: "Create pending candidate" }),
    );

    await waitFor(() =>
      expect(apiMocks.create).toHaveBeenCalledWith({
        type: "user_preference",
        content: "Prefer short answers.",
        source,
      }),
    );
    await waitFor(() => expect(apiMocks.list).toHaveBeenCalledTimes(2));
    expect(apiMocks.approve).not.toHaveBeenCalled();
    expect(apiMocks.reject).not.toHaveBeenCalled();
    expect(apiMocks.supersede).not.toHaveBeenCalled();
  });

  it("passes each candidate content hash to every mutation route", async () => {
    const user = userEvent.setup();
    const pending = candidate();
    const approved = candidate({
      id: "candidate-approved",
      status: "approved",
      type: "project_fact",
      content: "The project uses Tauri.",
      contentHash: "b".repeat(64),
    });
    apiMocks.list.mockImplementation(async () => [pending, approved]);
    render(<MemoryCandidatesPanel locale="en" source={source} />);

    const pendingArticle = (await screen.findByText(pending.content)).closest(
      "article",
    );
    const approvedArticle = screen.getByText(approved.content).closest("article");
    if (!pendingArticle || !approvedArticle) throw new Error("candidate row missing");

    expect(within(pendingArticle).getByText(/User preference.*Pending/)).toBeTruthy();
    expect(within(approvedArticle).getByText(/Project fact.*Approved/)).toBeTruthy();

    await user.click(
      within(pendingArticle).getByRole("button", { name: "Approve" }),
    );
    await waitFor(() =>
      expect(apiMocks.approve).toHaveBeenCalledWith({
        id: pending.id,
        expectedContentHash: pending.contentHash,
      }),
    );

    await user.click(
      within(pendingArticle).getByRole("button", { name: "Reject" }),
    );
    await waitFor(() =>
      expect(apiMocks.reject).toHaveBeenCalledWith({
        id: pending.id,
        expectedContentHash: pending.contentHash,
      }),
    );

    await user.click(
      within(approvedArticle).getByRole("button", { name: "Supersede" }),
    );
    await waitFor(() =>
      expect(apiMocks.supersede).toHaveBeenCalledWith({
        id: approved.id,
        expectedContentHash: approved.contentHash,
      }),
    );

    await user.click(
      within(approvedArticle).getByRole("button", { name: "Delete" }),
    );
    await waitFor(() =>
      expect(apiMocks.delete).toHaveBeenCalledWith({
        id: approved.id,
        expectedContentHash: approved.contentHash,
      }),
    );
  });

  it("keeps creation disabled when no real message source is available", async () => {
    const user = userEvent.setup();
    render(<MemoryCandidatesPanel locale="en" source={null} />);

    await waitFor(() => expect(apiMocks.list).toHaveBeenCalledTimes(1));
    await user.type(
      screen.getByRole("textbox", { name: "Memory content" }),
      "Prefer concise answers.",
    );
    const createButton = screen.getByRole("button", {
      name: "Create pending candidate",
    });
    expect(createButton.hasAttribute("disabled")).toBe(true);

    await user.click(createButton);
    expect(apiMocks.create).not.toHaveBeenCalled();
  });

  it("shows stale mutation errors without optimistically changing the list", async () => {
    const user = userEvent.setup();
    const pending = candidate();
    apiMocks.list.mockResolvedValue([pending]);
    apiMocks.approve.mockRejectedValueOnce(
      new Error("STALE_MEMORY_CANDIDATE: content hash mismatch"),
    );
    render(<MemoryCandidatesPanel locale="en" source={source} />);

    const article = (await screen.findByText(pending.content)).closest("article");
    if (!article) throw new Error("candidate row missing");
    await user.click(within(article).getByRole("button", { name: "Approve" }));

    expect(
      await screen.findByText(/STALE_MEMORY_CANDIDATE: content hash mismatch/),
    ).toBeTruthy();
    expect(apiMocks.list).toHaveBeenCalledTimes(1);
    expect(screen.getByText(pending.content)).toBeTruthy();
    expect(within(article).getByText(/User preference.*Pending/)).toBeTruthy();
    expect(within(article).getByRole("button", { name: "Approve" })).toBeTruthy();
    expect(within(article).getByRole("button", { name: "Reject" })).toBeTruthy();
  });
});
