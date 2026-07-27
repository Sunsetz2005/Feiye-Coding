// @vitest-environment jsdom

import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { WorkbenchShell } from "@/components/WorkbenchShell";

describe("WorkbenchShell", () => {
  it("preserves the ordered workbench regions", () => {
    render(
      <WorkbenchShell>
        <aside aria-label="Projects" />
        <main aria-label="Conversation" />
        <aside aria-label="Resources" />
      </WorkbenchShell>,
    );

    const shell = screen.getByTestId("workbench-shell");
    expect(shell.className).toBe("workbench");
    expect(Array.from(shell.children).map((node) => node.tagName)).toEqual([
      "ASIDE",
      "MAIN",
      "ASIDE",
    ]);
  });
});
