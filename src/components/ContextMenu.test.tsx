// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  ContextMenu,
  contextMenuPosForAnchor,
  type ContextMenuItem,
} from "./ContextMenu";

describe("ContextMenu", () => {
  afterEach(() => cleanup());

  beforeEach(() => {
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) =>
      window.setTimeout(() => callback(0), 0),
    );
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("focuses the first item and skips disabled items with arrow keys", async () => {
    const items: ContextMenuItem[] = [
      { id: "one", label: "One", onClick: vi.fn() },
      { id: "disabled", label: "Disabled", disabled: true, onClick: vi.fn() },
      { id: "three", label: "Three", onClick: vi.fn() },
    ];
    render(
      <ContextMenu
        open
        x={20}
        y={20}
        items={items}
        onClose={vi.fn()}
      />,
    );

    const first = screen.getByRole("menuitem", { name: "One" });
    const third = screen.getByRole("menuitem", { name: "Three" });
    await waitFor(() => expect(document.activeElement).toBe(first));
    fireEvent.keyDown(screen.getByRole("menu"), { key: "ArrowDown" });
    expect(document.activeElement).toBe(third);
    fireEvent.keyDown(screen.getByRole("menu"), { key: "Home" });
    expect(document.activeElement).toBe(first);
  });

  it("restores the trigger focus after Escape", async () => {
    const trigger = document.createElement("button");
    document.body.appendChild(trigger);
    trigger.focus();
    const onClose = vi.fn();
    render(
      <ContextMenu
        open
        x={20}
        y={20}
        restoreFocusTo={trigger}
        items={[{ label: "Action", onClick: vi.fn() }]}
        onClose={onClose}
      />,
    );
    const action = screen.getByRole("menuitem", { name: "Action" });
    await waitFor(() => expect(document.activeElement).toBe(action));
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(document.activeElement).toBe(trigger));
    trigger.remove();
  });

  it("opens a cascade submenu with ArrowRight and activates a child", async () => {
    const onMove = vi.fn();
    const onClose = vi.fn();
    render(
      <ContextMenu
        open
        x={20}
        y={20}
        onClose={onClose}
        items={[
          { id: "rename", label: "Rename", onClick: vi.fn() },
          {
            id: "move",
            label: "Move to project",
            submenu: [
              { id: "alpha", label: "Alpha", onClick: onMove },
              { id: "none", label: "No project", onClick: vi.fn() },
            ],
          },
        ]}
      />,
    );
    const parent = screen.getByRole("menuitem", { name: "Move to project" });
    await waitFor(() =>
      expect(screen.getByRole("menuitem", { name: "Rename" })).toBeTruthy(),
    );
    fireEvent.mouseEnter(parent);
    const child = await screen.findByRole("menuitem", { name: "Alpha" });
    fireEvent.click(child);
    expect(onMove).toHaveBeenCalledTimes(1);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("opens a cascade with ArrowRight and closes it with ArrowLeft", async () => {
    render(
      <ContextMenu
        open
        x={20}
        y={20}
        onClose={vi.fn()}
        items={[
          {
            id: "move",
            label: "Move to project",
            submenu: [
              { id: "alpha", label: "Alpha", onClick: vi.fn() },
              { id: "beta", label: "Beta", onClick: vi.fn() },
            ],
          },
        ]}
      />,
    );
    const parent = screen.getByRole("menuitem", { name: "Move to project" });
    await waitFor(() => expect(document.activeElement).toBe(parent));
    fireEvent.keyDown(screen.getAllByRole("menu")[0], { key: "ArrowRight" });
    const alpha = await screen.findByRole("menuitem", { name: "Alpha" });
    await waitFor(() => expect(document.activeElement).toBe(alpha));
    fireEvent.keyDown(screen.getAllByRole("menu")[0], { key: "ArrowDown" });
    expect(document.activeElement).toBe(
      screen.getByRole("menuitem", { name: "Beta" }),
    );
    fireEvent.keyDown(screen.getAllByRole("menu")[0], { key: "ArrowLeft" });
    await waitFor(() =>
      expect(screen.queryByRole("menuitem", { name: "Alpha" })).toBeNull(),
    );
    expect(document.activeElement).toBe(parent);
  });

  it("keeps the parent open when the cascade trigger is clicked", async () => {
    const onClose = vi.fn();
    const onMove = vi.fn();
    render(
      <ContextMenu
        open
        x={20}
        y={20}
        onClose={onClose}
        items={[
          {
            id: "move",
            label: "Move to project",
            submenu: [{ id: "alpha", label: "Alpha", onClick: onMove }],
          },
        ]}
      />,
    );
    fireEvent.click(screen.getByRole("menuitem", { name: "Move to project" }));
    expect(onClose).not.toHaveBeenCalled();
    expect(await screen.findByRole("menuitem", { name: "Alpha" })).toBeTruthy();
    expect(onMove).not.toHaveBeenCalled();
  });

  it("closes a cascade with Escape before dismissing the parent menu", async () => {
    const onClose = vi.fn();
    render(
      <ContextMenu
        open
        x={20}
        y={20}
        onClose={onClose}
        items={[
          {
            id: "move",
            label: "Move to project",
            submenu: [{ id: "alpha", label: "Alpha", onClick: vi.fn() }],
          },
        ]}
      />,
    );
    fireEvent.mouseEnter(screen.getByRole("menuitem", { name: "Move to project" }));
    expect(await screen.findByRole("menuitem", { name: "Alpha" })).toBeTruthy();
    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() =>
      expect(screen.queryByRole("menuitem", { name: "Alpha" })).toBeNull(),
    );
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("flips an anchored menu above when there is not enough room below", () => {
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: 800,
    });
    Object.defineProperty(window, "innerHeight", {
      configurable: true,
      value: 600,
    });
    const pos = contextMenuPosForAnchor(
      {
        left: 700,
        right: 740,
        top: 560,
        bottom: 590,
        width: 40,
        height: 30,
      },
      200,
      240,
    );
    expect(pos.left).toBe(592);
    expect(pos.top).toBe(314);
  });
});
