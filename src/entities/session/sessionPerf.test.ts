/**
 * Hot-path budgets: stream fold, markdown split, and windowing must stay
 * cheap on a long turn so the workbench stays responsive.
 */
import { describe, expect, it } from "vitest";
import type { ChatMessage, StreamPayload } from "@/lib/session";
import {
  SESSION_ENTITY_MODULE,
  applyStreamChunks,
  conversationWindow,
  splitStreamingMarkdown,
} from "./index";

function elapsed(run: () => void): number {
  const start = performance.now();
  run();
  return performance.now() - start;
}

describe("session hot-path performance", () => {
  it("exports the session entity barrel", () => {
    expect(SESSION_ENTITY_MODULE).toBe("session");
  });
  it("folds 400 stream tokens well under a frame", () => {
    const chunks: StreamPayload[] = Array.from({ length: 400 }, (_, i) => ({
      sessionId: "s1",
      messageId: "a1",
      text: i % 20 === 0 ? "word\n\n" : "x",
      done: i === 399,
    }));
    const ms = elapsed(() => {
      const next = applyStreamChunks([], chunks);
      expect(next).toHaveLength(1);
      expect(next[0]?.streaming).toBe(false);
    });
    expect(ms).toBeLessThan(16);
  });

  it("splits a 20k-character streaming document cheaply", () => {
    const body = `${"para\n\n".repeat(200)}` + "```ts\n" + "x".repeat(8000) + "\n";
    const ms = elapsed(() => {
      const parts = splitStreamingMarkdown(body);
      expect(parts.frozen.length).toBeGreaterThan(0);
      expect(parts.tail.startsWith("```")).toBe(true);
    });
    expect(ms).toBeLessThan(8);
  });

  it("windows 300 transcript rows without scanning more than once", () => {
    const messages: ChatMessage[] = Array.from({ length: 300 }, (_, i) => ({
      id: `m${i}`,
      role: i % 2 === 0 ? "user" : "assistant",
      content: `row ${i}`,
    }));
    const ms = elapsed(() => {
      const win = conversationWindow(messages);
      expect(win.windowed).toBe(true);
      expect(win.end).toBe(300);
      expect(win.start).toBeGreaterThan(0);
    });
    expect(ms).toBeLessThan(4);
  });
});
