// @vitest-environment jsdom

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { PlanArtifactCard } from "./PlanArtifactCard";

afterEach(cleanup);

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
        labels={{
          plan: "Plan",
          empty: "No plan",
          open: "Open",
        }}
        onOpen={onOpen}
      />,
    );

    expect(screen.getByText("Plan")).toBeTruthy();
    expect(screen.queryByText(/ready for review/i)).toBeNull();
    screen.getByRole("button", { name: "Open" }).click();
    expect(onOpen).toHaveBeenCalledOnce();
  });
});
