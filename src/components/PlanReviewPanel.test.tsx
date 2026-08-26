// @vitest-environment jsdom

import { cleanup, render, screen } from "@testing-library/react";
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { PlanReviewPanel } from "@/components/PlanReviewPanel";

afterEach(cleanup);
beforeAll(() => {
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
});
afterAll(() => vi.unstubAllGlobals());

const labels = {
  plan: "Plan",
  waiting: "Waiting for plan",
  progress: "In progress",
  done: "Done",
  empty: "No plan content",
  steps: "Steps",
  fraction: "{n}",
  expandDetails: "Expand details",
  collapseDetails: "Collapse details",
  current: "Current",
};

describe("PlanReviewPanel", () => {
  it("keeps an awaiting plan readable without exposing a second decision path", () => {
    render(
      <PlanReviewPanel
        plan={{
          visible: true,
          waiting: false,
          title: "Implementation plan",
          body: "## Scope\n\nKeep the resource pane read-only.",
          entries: [{ content: "Inspect", status: "completed" }],
          rpcId: 42,
          liveReview: true,
          artifactStatus: "proposed",
        }}
        labels={labels}
      />,
    );

    expect(screen.getByText("Plan")).toBeTruthy();
    expect(screen.getByRole("heading", { name: "Implementation plan" }))
      .toBeTruthy();
    expect(screen.getByText("Keep the resource pane read-only.")).toBeTruthy();
    expect(screen.queryByText(/approve|review|dismiss/i)).toBeNull();
  });

  it.each([
    ["approved", "In progress"],
    ["executing", "In progress"],
    ["completed", "Done"],
  ] as const)("renders %s artifacts without decision buttons", (status, copy) => {
    render(
      <PlanReviewPanel
        plan={{
          visible: true,
          waiting: false,
          title: "Implementation plan",
          body: "Read-only body",
          entries: [{ content: "Inspect", status: "completed" }],
          artifactStatus: status,
          liveReview: false,
        }}
        labels={labels}
      />,
    );
    expect(screen.getByText(copy)).toBeTruthy();
    expect(screen.queryByText(/approve|review|dismiss/i)).toBeNull();
  });

  it("shows real execution progress and remains expandable", () => {
    render(
      <PlanReviewPanel
        plan={{
          visible: true,
          waiting: false,
          title: "Implementation plan",
          body: "",
          entries: [
            { content: "Inspect", status: "completed" },
            { content: "Implement", status: "in_progress" },
          ],
          rpcId: null,
        }}
        labels={labels}
      />,
    );

    expect(screen.getByRole("progressbar").getAttribute("aria-valuenow"))
      .toBe("1");
    expect(
      screen
        .getByRole("button", { name: /Implementation plan/ })
        .getAttribute("aria-expanded"),
    ).toBe("false");
  });

  it("uses neutral fallback copy for an empty waiting plan", () => {
    render(
      <PlanReviewPanel
        plan={{
          visible: true,
          waiting: true,
          title: "",
          body: "",
          entries: [],
          rpcId: null,
        }}
        labels={labels}
      />,
    );

    expect(screen.getByText("Waiting for plan")).toBeTruthy();
    expect(screen.getByRole("heading", { name: "Plan" })).toBeTruthy();
    expect(
      screen.getByRole("button", { name: /Waiting for plan/ }).hasAttribute(
        "disabled",
      ),
    ).toBe(true);
    expect(screen.queryByText("Expand details")).toBeNull();
  });
});
