// @vitest-environment jsdom

import { StrictMode, createRef } from "react";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { WorkbenchShell } from "@/components/WorkbenchShell";
import { WorkbenchTopbar } from "@/components/WorkbenchTopbar";

afterEach(() => {
  cleanup();
  document.documentElement.removeAttribute("data-kb-focus");
});

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

  it("restores sidebar focus after StrictMode replays the close effect", async () => {
    const sidebarToggleRef = createRef<HTMLButtonElement>();
    const { rerender } = render(
      <StrictMode>
        <WorkbenchShell
          sidebarCollapsed={false}
          sidebarToggleRef={sidebarToggleRef}
        >
          <button ref={sidebarToggleRef}>Show sidebar</button>
          <main />
        </WorkbenchShell>
      </StrictMode>,
    );

    rerender(
      <StrictMode>
        <WorkbenchShell sidebarCollapsed sidebarToggleRef={sidebarToggleRef}>
          <button ref={sidebarToggleRef}>Show sidebar</button>
          <main />
        </WorkbenchShell>
      </StrictMode>,
    );

    await waitFor(() => {
      expect(document.activeElement).toBe(sidebarToggleRef.current);
    });
    expect(document.documentElement.getAttribute("data-kb-focus")).toBe("true");
  });

  it("retries until the newly mounted sidebar trigger exists", async () => {
    const sidebarToggleRef = createRef<HTMLButtonElement>();
    const { rerender } = render(
      <WorkbenchShell
        sidebarCollapsed={false}
        sidebarToggleRef={sidebarToggleRef}
      >
        <main />
      </WorkbenchShell>,
    );

    rerender(
      <WorkbenchShell sidebarCollapsed sidebarToggleRef={sidebarToggleRef}>
        <main />
      </WorkbenchShell>,
    );

    rerender(
      <WorkbenchShell sidebarCollapsed sidebarToggleRef={sidebarToggleRef}>
        <button ref={sidebarToggleRef}>Show sidebar</button>
        <main />
      </WorkbenchShell>,
    );

    await waitFor(() => {
      expect(document.activeElement).toBe(sidebarToggleRef.current);
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

  it("restores focus to the Tip-wrapped Show sidebar control", async () => {
    const sidebarToggleRef = createRef<HTMLButtonElement>();
    const topbar = (collapsed: boolean) => (
      <WorkbenchTopbar
        title="Empty task"
        scheduledLabel="Scheduled"
        sessionMenuLabel="Session menu"
        sidebarCollapsed={collapsed}
        showSidebarLabel="Show sidebar"
        onShowSidebar={() => {}}
        sidebarToggleRef={sidebarToggleRef}
        asideCollapsed
        showAsideLabel="Show files"
        hideAsideLabel="Hide files"
        onToggleAside={() => {}}
      />
    );
    const { rerender } = render(
      <WorkbenchShell
        sidebarCollapsed={false}
        sidebarToggleRef={sidebarToggleRef}
      >
        {topbar(false)}
      </WorkbenchShell>,
    );

    rerender(
      <WorkbenchShell sidebarCollapsed sidebarToggleRef={sidebarToggleRef}>
        {topbar(true)}
      </WorkbenchShell>,
    );

    await waitFor(() => {
      expect(document.activeElement).toBe(sidebarToggleRef.current);
    });
  });
});
