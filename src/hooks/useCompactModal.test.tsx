// @vitest-environment jsdom

import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useCompactModal } from "./useCompactModal";

afterEach(() => {
  vi.useRealTimers();
  document.body.innerHTML = "";
});

describe("useCompactModal", () => {
  it("opens with a cleared note and closes with the note reset", () => {
    const { result } = renderHook(() => useCompactModal());

    act(() => result.current.setCompactNote("stale note"));
    act(() => result.current.openCompactModal());
    expect(result.current.showCompactModal).toBe(true);
    expect(result.current.compactNote).toBe("");

    act(() => result.current.setCompactNote("keep this"));
    act(() => result.current.closeCompactModal());
    expect(result.current.showCompactModal).toBe(false);
    expect(result.current.compactNote).toBe("");
  });

  it("focuses the note field after opening", () => {
    vi.useFakeTimers();
    const input = document.createElement("input");
    document.body.appendChild(input);
    const { result } = renderHook(() => useCompactModal());
    result.current.compactNoteRef.current = input;

    act(() => result.current.openCompactModal());
    act(() => vi.runOnlyPendingTimers());

    expect(document.activeElement).toBe(input);
  });

  it("dismisses on Escape and clears the note", () => {
    const { result } = renderHook(() => useCompactModal());
    act(() => result.current.openCompactModal());
    act(() => result.current.setCompactNote("temporary"));

    act(() => {
      document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    });

    expect(result.current.showCompactModal).toBe(false);
    expect(result.current.compactNote).toBe("");
  });
});
