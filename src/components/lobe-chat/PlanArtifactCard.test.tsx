// @vitest-environment jsdom

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { PlanArtifactCard } from "./PlanArtifactCard";

afterEach(cleanup);

const labels = {
  plan: "Plan",
  empty: "No plan",
  open: "Open",
  approved: "Approved",
  executing: "In progress",
  done: "Done",
};

describe("PlanArtifactCard", () => {
  it("uses a neutral plan label while review decisions stay in the dock", () => {
    const onOpen = vi.fn();
    render(
      <PlanArtifactCard
        artifact={{
          visible: true,
          waiting: false,
          title: "Implementation plan",
          body: "One visible plan.",
        }}
        labels={labels}
        onOpen={onOpen}
      />,
    );

    expect(screen.getByText("Plan")).toBeTruthy();
    expect(screen.queryByText(/ready for review/i)).toBeNull();
    expect(screen.queryByText(/approve|dismiss/i)).toBeNull();
    screen.getByRole("button", { name: "Open" }).click();
    expect(onOpen).toHaveBeenCalledOnce();
  });

  it.each([
    ["approved", "Approved"],
    ["executing", "In progress"],
    ["completed", "Done"],
  ] as const)("shows %s product copy without decision buttons", (status, copy) => {
    render(
      <PlanArtifactCard
        artifact={{
          visible: true,
          title: "Implementation plan",
          body: "Keep the card read-only.",
          artifactStatus: status,
        }}
        labels={labels}
      />,
    );
    expect(screen.getByText(copy)).toBeTruthy();
    expect(
      screen.queryByRole("button", { name: /approve|dismiss/i }),
    ).toBeNull();
  });
});
