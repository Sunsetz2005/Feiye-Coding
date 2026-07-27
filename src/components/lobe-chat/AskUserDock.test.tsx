// @vitest-environment jsdom

import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { AskUserPayload } from "@/lib/session";
import { AskUserDock, type AskUserDockLabels } from "./AskUserDock";

const labels: AskUserDockLabels = {
  title: "Agent question",
  submit: "Submit",
  cancel: "Cancel",
  otherPlaceholder: "Write an answer",
  freeTextHint: "Or write an answer",
  multiHint: "Choose one or more",
  close: "Close",
  previous: "Previous",
  next: "Next",
  skip: "Skip",
  progress: "{current} of {total}",
  recommended: "Recommended",
  submitFailed: "Could not submit",
};

afterEach(cleanup);

function payload(
  questions: AskUserPayload["questions"],
): AskUserPayload {
  return {
    rpcId: 9,
    sessionId: "session-a",
    questions,
  };
}

describe("AskUserDock interaction", () => {
  it("uses compact chrome for one question and only marks explicit recommendations", () => {
    render(
      <AskUserDock
        payload={payload([
          {
            id: "q1",
            question: "Choose",
            options: [
              { id: "a", label: "Alpha" },
              { id: "b", label: "Beta (Recommended)" },
            ],
          },
        ])}
        labels={labels}
        onSubmit={() => undefined}
        onCancel={() => undefined}
      />,
    );

    expect(screen.queryByLabelText("Previous")).toBeNull();
    expect(screen.queryByLabelText("Next")).toBeNull();
    expect(screen.getAllByText("Recommended")).toHaveLength(1);
    expect(screen.getByRole("button", { name: /Alpha/ })).toBeTruthy();
  });

  it("submits a single choice and advances multi-question flows one at a time", async () => {
    const onSubmit = vi.fn();
    const user = userEvent.setup();
    render(
      <AskUserDock
        payload={payload([
          {
            id: "q1",
            question: "First?",
            options: [{ id: "yes", label: "Yes" }],
          },
          {
            id: "q2",
            question: "Second?",
            options: [{ id: "done", label: "Done" }],
          },
        ])}
        labels={labels}
        onSubmit={onSubmit}
        onCancel={() => undefined}
      />,
    );

    await user.click(screen.getByRole("button", { name: /Yes/ }));
    expect(screen.getByText("Second?")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: /Done/ }));

    await waitFor(() =>
      expect(onSubmit).toHaveBeenCalledWith(
        { "First?": "Yes", "Second?": "Done" },
        { skippedQuestionIds: [] },
      ),
    );
  });

  it("submits accepted empty answers when every question is skipped", async () => {
    const onSubmit = vi.fn();
    const user = userEvent.setup();
    render(
      <AskUserDock
        payload={payload([
          { id: "q1", question: "First?", options: [] },
          { id: "q2", question: "Second?", options: [] },
        ])}
        labels={labels}
        onSubmit={onSubmit}
        onCancel={() => undefined}
      />,
    );

    await user.click(screen.getByRole("button", { name: "Skip" }));
    await user.click(screen.getByRole("button", { name: "Skip" }));

    await waitFor(() =>
      expect(onSubmit).toHaveBeenCalledWith(
        {},
        { skippedQuestionIds: ["q1", "q2"] },
      ),
    );
  });

  it("does not silently treat a browsed unanswered question as skipped", async () => {
    const onSubmit = vi.fn();
    const user = userEvent.setup();
    render(
      <AskUserDock
        payload={payload([
          {
            id: "q1",
            question: "First?",
            options: [{ id: "a", label: "Alpha" }],
          },
          {
            id: "q2",
            question: "Second?",
            options: [{ id: "b", label: "Beta" }],
          },
        ])}
        labels={labels}
        onSubmit={onSubmit}
        onCancel={() => undefined}
      />,
    );

    await user.click(screen.getByTitle("Next"));
    await user.click(screen.getByRole("button", { name: /Beta/ }));

    expect(onSubmit).not.toHaveBeenCalled();
    expect(screen.getByText("First?")).toBeTruthy();
  });

  it("keeps the dock and answer state when submission fails", async () => {
    const onSubmit = vi.fn().mockRejectedValue(new Error("offline"));
    const user = userEvent.setup();
    render(
      <AskUserDock
        payload={payload([
          {
            id: "q1",
            question: "Proceed?",
            options: [{ id: "yes", label: "Yes" }],
          },
        ])}
        labels={labels}
        onSubmit={onSubmit}
        onCancel={() => undefined}
      />,
    );

    await user.click(screen.getByRole("button", { name: /Yes/ }));
    expect((await screen.findByRole("alert")).textContent).toContain(
      "Could not submit",
    );
    expect(
      screen.getByRole("button", { name: /Yes/ }).getAttribute("aria-pressed"),
    ).toBe("true");
  });
});
