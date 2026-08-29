import { describe, expect, it } from "vitest";
import type { ChatMessage } from "@/lib/session";
import {
  CONVERSATION_WINDOW_THRESHOLD,
  conversationWindow,
  hiddenHistorySpacerPx,
} from "./conversationWindow";

function fill(n: number): ChatMessage[] {
  return Array.from({ length: n }, (_, i) => ({
    id: `m${i}`,
    role: i % 2 === 0 ? "user" : "assistant",
    content: `row ${i}`,
  }));
}

describe("conversationWindow", () => {
  it("does not window short transcripts", () => {
    const messages = fill(10);
    expect(conversationWindow(messages)).toEqual({
      windowed: false,
      start: 0,
      end: 10,
      hidden: 0,
    });
  });

  it("keeps the live turn from the last user message", () => {
    const messages = fill(CONVERSATION_WINDOW_THRESHOLD);
    messages.push({ id: "u-live", role: "user", content: "now" });
    messages.push({
      id: "a-live",
      role: "assistant",
      content: "stream",
      streaming: true,
    });
    const win = conversationWindow(messages);
    expect(win.windowed).toBe(true);
    expect(win.start).toBeLessThanOrEqual(messages.length - 2);
    expect(messages.slice(win.start).some((m) => m.id === "u-live")).toBe(true);
    expect(win.end).toBe(messages.length);
  });

  it("estimates spacer height for hidden rows", () => {
    expect(hiddenHistorySpacerPx(0)).toBe(0);
    expect(hiddenHistorySpacerPx(10, 80)).toBe(800);
  });
});
