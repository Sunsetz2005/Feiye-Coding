// @vitest-environment jsdom

import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { useAppDialog } from "./useAppDialog";

function fireKeydown(key: string) {
  const event = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true });
  document.dispatchEvent(event);
  return event;
}

describe("useAppDialog", () => {
  it("opens a prompt dialog and seeds dialogInput from initial", () => {
    const { result } = renderHook(() => useAppDialog());
    act(() => {
      result.current.setAppDialog({
        kind: "prompt",
        title: "Rename",
        initial: "old-name",
        onSubmit: vi.fn(),
      });
    });
    expect(result.current.appDialog?.kind).toBe("prompt");
    expect(result.current.dialogInput).toBe("old-name");
  });

  it("opens an edit-project dialog and seeds both dialogInput and dialogPath", () => {
    const { result } = renderHook(() => useAppDialog());
    act(() => {
      result.current.setAppDialog({
        kind: "edit-project",
        title: "Edit project",
        projectId: "p1",
        name: "My Project",
        path: "/tmp/my-project",
        onSubmit: vi.fn(),
      });
    });
    expect(result.current.dialogInput).toBe("My Project");
    expect(result.current.dialogPath).toBe("/tmp/my-project");
  });

  it("does not seed dialogInput/dialogPath for a confirm dialog", () => {
    const { result } = renderHook(() => useAppDialog());
    act(() => {
      result.current.setDialogInput("stale");
      result.current.setAppDialog({
        kind: "confirm",
        title: "Delete?",
        message: "Are you sure?",
        onConfirm: vi.fn(),
      });
    });
    expect(result.current.dialogInput).toBe("stale");
  });

  it("Escape clears the open dialog", () => {
    const { result } = renderHook(() => useAppDialog());
    act(() => {
      result.current.setAppDialog({
        kind: "confirm",
        title: "Delete?",
        message: "Are you sure?",
        onConfirm: vi.fn(),
      });
    });
    expect(result.current.appDialog).not.toBeNull();
    act(() => {
      fireKeydown("Escape");
    });
    expect(result.current.appDialog).toBeNull();
  });

  it("Enter on a confirm dialog invokes onConfirm and clears the dialog", () => {
    const { result } = renderHook(() => useAppDialog());
    const onConfirm = vi.fn();
    act(() => {
      result.current.setAppDialog({
        kind: "confirm",
        title: "Delete?",
        message: "Are you sure?",
        onConfirm,
      });
    });
    act(() => {
      fireKeydown("Enter");
    });
    expect(onConfirm).toHaveBeenCalledTimes(1);
    expect(result.current.appDialog).toBeNull();
  });

  it("Enter is a no-op when no dialog is open, or the open dialog is not a confirm", () => {
    const { result } = renderHook(() => useAppDialog());
    // No dialog open at all.
    expect(() => fireKeydown("Enter")).not.toThrow();

    const onSubmit = vi.fn();
    act(() => {
      result.current.setAppDialog({
        kind: "prompt",
        title: "Rename",
        initial: "",
        onSubmit,
      });
    });
    act(() => {
      fireKeydown("Enter");
    });
    // Prompt dialogs are submitted via their own form, not the global Enter handler.
    expect(onSubmit).not.toHaveBeenCalled();
    expect(result.current.appDialog?.kind).toBe("prompt");
  });

  it("resolves chained confirm dialogs via the ref, not a stale closure", () => {
    const { result } = renderHook(() => useAppDialog());
    const onConfirmStep1 = vi.fn(() => {
      act(() => {
        result.current.setAppDialog({
          kind: "confirm",
          title: "Step 2",
          message: "Really really sure?",
          onConfirm: onConfirmStep2,
        });
      });
    });
    const onConfirmStep2 = vi.fn();
    act(() => {
      result.current.setAppDialog({
        kind: "confirm",
        title: "Step 1",
        message: "Are you sure?",
        onConfirm: onConfirmStep1,
      });
    });
    act(() => {
      fireKeydown("Enter");
    });
    expect(onConfirmStep1).toHaveBeenCalledTimes(1);
    expect(result.current.appDialog?.kind).toBe("confirm");
    expect((result.current.appDialog as { title: string }).title).toBe("Step 2");

    act(() => {
      fireKeydown("Enter");
    });
    expect(onConfirmStep2).toHaveBeenCalledTimes(1);
    expect(result.current.appDialog).toBeNull();
  });
});
