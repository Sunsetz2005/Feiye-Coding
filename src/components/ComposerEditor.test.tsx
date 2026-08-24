// @vitest-environment jsdom

import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ComposerEditor } from "./ComposerEditor";

let nextFrameId = 1;
let animationFrames = new Map<number, FrameRequestCallback>();

beforeEach(() => {
  nextFrameId = 1;
  animationFrames = new Map();
  vi.stubGlobal(
    "requestAnimationFrame",
    vi.fn((callback: FrameRequestCallback) => {
      const id = nextFrameId++;
      animationFrames.set(id, callback);
      return id;
    }),
  );
  vi.stubGlobal(
    "cancelAnimationFrame",
    vi.fn((id: number) => animationFrames.delete(id)),
  );
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

function drainAnimationFrames(maxBatches = 20) {
  let batches = 0;
  while (animationFrames.size > 0) {
    if (batches >= maxBatches) {
      throw new Error("ComposerEditor kept scheduling animation frames");
    }
    batches += 1;
    const callbacks = [...animationFrames.values()];
    animationFrames.clear();
    callbacks.forEach((callback) => callback(batches * 16));
  }
  return batches;
}

describe("ComposerEditor slash query events", () => {
  it("reports the exact query from user input", () => {
    const onChange = vi.fn();
    const onSlashQueryChange = vi.fn();
    render(
      <ComposerEditor
        value=""
        onChange={onChange}
        onSlashQueryChange={onSlashQueryChange}
      />,
    );
    onSlashQueryChange.mockClear();

    const editor = screen.getByRole("textbox");
    editor.textContent = "/re";
    fireEvent.input(editor, { data: "e", inputType: "insertText" });

    expect(onChange).toHaveBeenCalledWith("/re");
    expect(onSlashQueryChange).toHaveBeenLastCalledWith({
      start: 0,
      query: "re",
      end: 3,
    });
  });

  it("reports a programmatic value while the editor is not focused", () => {
    const onSlashQueryChange = vi.fn();
    const { rerender } = render(
      <ComposerEditor
        value=""
        onChange={vi.fn()}
        onSlashQueryChange={onSlashQueryChange}
      />,
    );
    const editor = screen.getByRole("textbox");
    expect(document.activeElement).not.toBe(editor);
    onSlashQueryChange.mockClear();

    rerender(
      <ComposerEditor
        value="/plan"
        onChange={vi.fn()}
        onSlashQueryChange={onSlashQueryChange}
      />,
    );

    expect(document.activeElement).not.toBe(editor);
    expect(onSlashQueryChange).toHaveBeenLastCalledWith({
      start: 0,
      query: "plan",
      end: 5,
    });
  });

  it("keeps IME slash updates live and finishes its deferred flushes", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    const onChange = vi.fn();
    const onSlashQueryChange = vi.fn();
    render(
      <ComposerEditor
        value=""
        onChange={onChange}
        onSlashQueryChange={onSlashQueryChange}
      />,
    );
    onSlashQueryChange.mockClear();

    const editor = screen.getByRole("textbox");
    fireEvent.compositionStart(editor);
    editor.textContent = "/计";
    fireEvent.input(editor, {
      data: "ji",
      inputType: "insertCompositionText",
      isComposing: true,
    });
    expect(onChange).not.toHaveBeenCalled();
    expect(onSlashQueryChange).toHaveBeenLastCalledWith({
      start: 0,
      query: "计",
      end: 2,
    });

    editor.textContent = "/计划";
    fireEvent.compositionUpdate(editor, { data: "计划" });
    fireEvent.compositionEnd(editor, { data: "计划" });

    expect(onChange).toHaveBeenCalledWith("/计划");
    expect(onSlashQueryChange).toHaveBeenLastCalledWith({
      start: 0,
      query: "计划",
      end: 3,
    });

    await act(async () => {
      await Promise.resolve();
    });

    let frameBatches = 0;
    act(() => {
      vi.runAllTimers();
      frameBatches = drainAnimationFrames();
    });

    expect(frameBatches).toBeGreaterThan(0);
    expect(frameBatches).toBeLessThan(20);
    expect(animationFrames.size).toBe(0);
  });
});
