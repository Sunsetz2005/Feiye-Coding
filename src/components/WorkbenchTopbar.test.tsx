// @vitest-environment jsdom

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
    renderTopbar({ scheduled: true, onOpenSessionMenu });

    expect(
      screen.getByRole("heading", { name: "Refactor workbench" }),
    ).toBeTruthy();
    expect(screen.getByLabelText("Scheduled")).toBeTruthy();

    await user.click(screen.getByRole("button", { name: "Session menu" }));
    expect(onOpenSessionMenu).toHaveBeenCalledTimes(1);
  });

  it("keeps sidebar and resource panel controls stateful and accessible", async () => {
    const user = userEvent.setup();
    const onShowSidebar = vi.fn();
    const onToggleAside = vi.fn();
    renderTopbar({
      sidebarCollapsed: true,
      onShowSidebar,
      asideCollapsed: false,
      onToggleAside,
    });

    await user.click(screen.getByRole("button", { name: "Show sidebar" }));
    expect(onShowSidebar).toHaveBeenCalledTimes(1);

    const asideButton = screen.getByRole("button", { name: "Hide files" });
    expect(asideButton.getAttribute("aria-pressed")).toBe("true");
    await user.click(asideButton);
    expect(onToggleAside).toHaveBeenCalledTimes(1);
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
