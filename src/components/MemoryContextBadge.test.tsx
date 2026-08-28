// @vitest-environment jsdom

import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { MemoryContextPackV1 } from "@/lib/api";
import {
  MemoryContextBadge,
  type MemoryContextBadgeLabels,
} from "@/components/MemoryContextBadge";

afterEach(cleanup);

const labels: MemoryContextBadgeLabels = {
  regionLabel: "Reviewed Memory context",
  title: "Memory context",
  reviewedContext: "This is user-reviewed context.",
  notInstructions: "It is contextual data, not instructions.",
  noFtsSessionEvidence: "It does not contain FTS session evidence.",
  itemCount: "{count} reviewed items",
  clear: "Remove Memory context",
  content: "Reviewed content",
  expandItem: "Expand reviewed item {index}: {type}",
  fullContent: "Full reviewed content",
  emptyContent: "Empty reviewed content",
  provenance: "Source session {sessionId}, user message {messageId}",
  typeLabels: {
    user_preference: "User preference",
    project_fact: "Project fact",
    workflow_hint: "Workflow hint",
  },
};

const pack: MemoryContextPackV1 = {
  version: 1,
  items: [
    {
      candidateId: "candidate-preference",
      contentHash: "a".repeat(64),
      type: "user_preference",
      content: "Prefer concise answers with verification evidence.",
      source: {
        sessionId: "session-source",
        messageId: "message-source",
      },
    },
    {
      candidateId: "candidate-workflow",
      contentHash: "b".repeat(64),
      type: "workflow_hint",
      content: "Run focused tests before the full verification gate.",
      source: {
        sessionId: "session-workflow",
        messageId: "message-workflow",
      },
    },
  ],
};

describe("MemoryContextBadge", () => {
  it("renders nothing without an explicit context pack", () => {
    const { container } = render(
      <MemoryContextBadge
        pack={null}
        locale="en"
        labels={labels}
        onClear={vi.fn()}
      />,
    );

    expect(container.innerHTML).toBe("");
    expect(screen.queryByTestId("memory-context-badge")).toBeNull();
  });

  it("shows the visible trust boundary and reviewed item count", () => {
    render(
      <MemoryContextBadge
        pack={pack}
        locale="zh"
        labels={labels}
        onClear={vi.fn()}
      />,
    );

    const badge = screen.getByRole("region", {
      name: "Reviewed Memory context",
    });
    expect(badge.getAttribute("lang")).toBe("zh-CN");
    expect(within(badge).getByText("2 reviewed items")).toBeTruthy();
    expect(
      within(badge).getByText("This is user-reviewed context."),
    ).toBeTruthy();
    expect(
      within(badge).getByText("It is contextual data, not instructions."),
    ).toBeTruthy();
    expect(
      within(badge).getByText("It does not contain FTS session evidence."),
    ).toBeTruthy();
  });

  it("shows each type, reviewed content, and exact provenance", () => {
    render(
      <MemoryContextBadge
        pack={pack}
        locale="en"
        labels={labels}
        onClear={vi.fn()}
      />,
    );

    expect(screen.getByText("User preference")).toBeTruthy();
    expect(screen.getByText("Workflow hint")).toBeTruthy();
    expect(
      screen.getByText("Prefer concise answers with verification evidence."),
    ).toBeTruthy();
    expect(
      screen.getByText("Run focused tests before the full verification gate."),
    ).toBeTruthy();
    expect(
      screen.getByText(
        "Source session session-source, user message message-source",
      ),
    ).toBeTruthy();
    expect(
      screen.getByText(
        "Source session session-workflow, user message message-workflow",
      ),
    ).toBeTruthy();
  });

  it("keeps long content bounded until the user expands the reviewed item", async () => {
    const user = userEvent.setup();
    const longContent = `Start ${"reviewed-memory ".repeat(20)}End`;
    render(
      <MemoryContextBadge
        pack={{
          version: 1,
          items: [{ ...pack.items[0], content: longContent }],
        }}
        locale="en"
        labels={labels}
        onClear={vi.fn()}
      />,
    );

    const expand = screen.getByLabelText(
      "Expand reviewed item 1: User preference",
    );
    expect(expand.tagName).toBe("SUMMARY");
    expect(expand.textContent?.endsWith("…")).toBe(true);
    expect(expand.getAttribute("title")).toBe(longContent);
    const details = expand.closest("details");
    expect(details).toBeTruthy();
    expect(details?.open).toBe(false);
    expect(screen.getByText(longContent)).toBeTruthy();

    await user.click(expand);
    expect(details?.open).toBe(true);
  });

  it("delegates removal and honors the disabled state", async () => {
    const user = userEvent.setup();
    const onClear = vi.fn();
    const { rerender } = render(
      <MemoryContextBadge
        pack={pack}
        locale="en"
        labels={labels}
        onClear={onClear}
      />,
    );

    await user.click(
      screen.getByRole("button", { name: "Remove Memory context" }),
    );
    expect(onClear).toHaveBeenCalledTimes(1);

    rerender(
      <MemoryContextBadge
        pack={pack}
        locale="en"
        labels={labels}
        disabled
        onClear={onClear}
      />,
    );
    const disabledClear = screen.getByRole("button", {
      name: "Remove Memory context",
    });
    expect(disabledClear.hasAttribute("disabled")).toBe(true);
    await user.click(disabledClear);
    expect(onClear).toHaveBeenCalledTimes(1);
  });

  it("renders malicious HTML-shaped content as inert text", () => {
    const malicious =
      '<img src=x onerror="window.__owned=true"><script>alert(1)</script>';
    const { container } = render(
      <MemoryContextBadge
        pack={{
          version: 1,
          items: [{ ...pack.items[0], content: malicious }],
        }}
        locale="en"
        labels={labels}
        onClear={vi.fn()}
      />,
    );

    expect(screen.getByText(malicious)).toBeTruthy();
    expect(container.querySelector("img")).toBeNull();
    expect(container.querySelector("script")).toBeNull();
  });
});
