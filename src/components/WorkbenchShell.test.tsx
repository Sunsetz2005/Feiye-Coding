// @vitest-environment jsdom

import { createRef } from "react";
import { render, screen, waitFor } from "@testing-library/react";
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

  it("restores focus to the sidebar control after the pane closes", async () => {
    const sidebarToggleRef = createRef<HTMLButtonElement>();
    const { rerender } = render(
      <WorkbenchShell
        sidebarCollapsed={false}
        sidebarToggleRef={sidebarToggleRef}
      >
        <button ref={sidebarToggleRef}>Show sidebar</button>
        <main />
      </WorkbenchShell>,
    );

    rerender(
      <WorkbenchShell
        sidebarCollapsed
        sidebarToggleRef={sidebarToggleRef}
      >
        <button ref={sidebarToggleRef}>Show sidebar</button>
        <main />
      </WorkbenchShell>,
    );

    await waitFor(() => {
      expect(document.activeElement).toBe(
        screen.getByRole("button", { name: "Show sidebar" }),
      );
    });
  });

  it("restores focus to the resource control after the pane closes", async () => {
    const asideToggleRef = createRef<HTMLButtonElement>();
    const { rerender } = render(
      <WorkbenchShell asideCollapsed={false} asideToggleRef={asideToggleRef}>
        <main />
        <button ref={asideToggleRef}>Show resources</button>
      </WorkbenchShell>,
    );

    rerender(
      <WorkbenchShell asideCollapsed asideToggleRef={asideToggleRef}>
        <main />
        <button ref={asideToggleRef}>Show resources</button>
      </WorkbenchShell>,
    );

    await waitFor(() => {
      expect(document.activeElement).toBe(
        screen.getByRole("button", { name: "Show resources" }),
      );
    });
  });

  it("keeps conversation scroll position when pane state changes", () => {
    const { container, rerender } = render(
      <WorkbenchShell asideCollapsed={false}>
        <main>
          <div className="lobe-chat__scroll" />
        </main>
      </WorkbenchShell>,
    );
    const scroll = container.querySelector<HTMLElement>(".lobe-chat__scroll");
    expect(scroll).not.toBeNull();
    if (!scroll) return;
    scroll.scrollTop = 148;

    rerender(
      <WorkbenchShell asideCollapsed>
        <main>
          <div className="lobe-chat__scroll" />
        </main>
      </WorkbenchShell>,
    );

    const scrollAfterRerender =
      container.querySelector<HTMLElement>(".lobe-chat__scroll");
    expect(scrollAfterRerender).toBe(scroll);
    expect(scrollAfterRerender?.isConnected).toBe(true);
    expect(scrollAfterRerender?.scrollTop).toBe(148);
  });
});
