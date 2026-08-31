// @vitest-environment jsdom

import { createRef } from "react";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { WorkbenchTopbar } from "./WorkbenchTopbar";

afterEach(cleanup);

function renderTopbar(
  overrides: Partial<React.ComponentProps<typeof WorkbenchTopbar>> = {},
) {
  const props: React.ComponentProps<typeof WorkbenchTopbar> = {
    title: "Refactor workbench",
    scheduledLabel: "Scheduled",
    sessionMenuLabel: "Session menu",
    sidebarCollapsed: false,
    showSidebarLabel: "Show sidebar",
    onShowSidebar: vi.fn(),
    asideCollapsed: true,
    showAsideLabel: "Show files",
    hideAsideLabel: "Hide files",
    onToggleAside: vi.fn(),
    ...overrides,
  };
  render(<WorkbenchTopbar {...props} />);
  return props;
}

describe("WorkbenchTopbar", () => {
  it("renders the current task and exposes only real task actions", async () => {
    const user = userEvent.setup();
    const onOpenSessionMenu = vi.fn();
    renderTopbar({
      projectContext: true,
      sessionMenuOpen: true,
      onOpenSessionMenu,
    });

    expect(
      screen.getByRole("heading", { name: "Refactor workbench" }),
    ).toBeTruthy();
    expect(document.querySelector(".main__title-icon")).toBeTruthy();

    const menuButton = screen.getByRole("button", { name: "Session menu" });
    expect(menuButton.getAttribute("aria-haspopup")).toBe("menu");
    expect(menuButton.getAttribute("aria-expanded")).toBe("true");
    await user.click(menuButton);
    expect(onOpenSessionMenu).toHaveBeenCalledTimes(1);
  });

  it("keeps sidebar and resource panel controls stateful and accessible", async () => {
    const user = userEvent.setup();
    const onShowSidebar = vi.fn();
    const onToggleAside = vi.fn();
    const sidebarToggleRef = createRef<HTMLButtonElement>();
    const asideToggleRef = createRef<HTMLButtonElement>();
    renderTopbar({
      sidebarCollapsed: true,
      onShowSidebar,
      sidebarToggleRef,
      asideCollapsed: false,
      onToggleAside,
      asideToggleRef,
    });

    await user.click(screen.getByRole("button", { name: "Show sidebar" }));
    expect(onShowSidebar).toHaveBeenCalledTimes(1);
    expect(sidebarToggleRef.current).toBe(
      screen.getByRole("button", { name: "Show sidebar" }),
    );

    const asideButton = screen.getByRole("button", { name: "Hide files" });
    expect(asideToggleRef.current).toBe(asideButton);
    expect(asideButton.getAttribute("aria-pressed")).toBe("true");
    await user.click(asideButton);
    expect(onToggleAside).toHaveBeenCalledTimes(1);

    expect(
      document.querySelector(".main__top")?.hasAttribute("data-tauri-drag-region"),
    ).toBe(false);
    expect(
      document
        .querySelector(".main__title-row")
        ?.hasAttribute("data-tauri-drag-region"),
    ).toBe(false);
    expect(
      document
        .querySelector(".main__title-drag")
        ?.hasAttribute("data-tauri-drag-region"),
    ).toBe(true);
  });

  it("shows exceptional connection and retry state without inventing actions", () => {
    renderTopbar({
      connection: {
        pill: { tone: "err", labelKey: "conn.disconnected" },
        label: "Disconnected",
      },
      retry: {
        label: "Retrying 2/3: timeout",
        detail: "timeout",
      },
    });

    expect(screen.getByRole("status").textContent).toContain("Disconnected");
    expect(screen.getByText("Retrying 2/3: timeout")).toBeTruthy();
    expect(
      screen.queryByRole("button", { name: "Session menu" }),
    ).toBeNull();
  });
});
