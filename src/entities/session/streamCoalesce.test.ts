import { describe, expect, it, vi } from "vitest";
import type { ChatMessage, StreamPayload } from "@/lib/session";
import {
  applyStreamChunks,
  createStreamCoalescer,
  streamSessionKey,
} from "./streamCoalesce";

function chunk(
  partial: Partial<StreamPayload> & Pick<StreamPayload, "text">,
): StreamPayload {
  return {
    sessionId: "s1",
    messageId: "a1",
    done: false,
    ...partial,
  };
}

describe("applyStreamChunks", () => {
  it("folds multiple tokens onto one assistant", () => {
    const start: ChatMessage[] = [];
    const next = applyStreamChunks(start, [
      chunk({ text: "Hel" }),
      chunk({ text: "lo" }),
      chunk({ text: "", done: true }),
    ]);
    expect(next).toHaveLength(1);
    expect(next[0]?.content).toBe("Hello");
    expect(next[0]?.streaming).toBe(false);
  });
});

describe("createStreamCoalescer", () => {
  it("batches tokens until rAF, then applies once", () => {
    vi.useFakeTimers();
    const frames: Array<FrameRequestCallback> = [];
    const previousRaf = globalThis.requestAnimationFrame;
    globalThis.requestAnimationFrame = ((cb: FrameRequestCallback) => {
      frames.push(cb);
      return frames.length;
    }) as typeof requestAnimationFrame;
    const applied: StreamPayload[][] = [];
    const coalescer = createStreamCoalescer((_id, chunks) => {
      applied.push(chunks);
    });
    coalescer.enqueue(chunk({ text: "a" }));
    coalescer.enqueue(chunk({ text: "b" }));
    expect(applied).toEqual([]);
    expect(frames).toHaveLength(1);
    frames[0]!(0);
    expect(applied).toEqual([[chunk({ text: "a" }), chunk({ text: "b" })]]);
    coalescer.dispose();
    globalThis.requestAnimationFrame = previousRaf;
    vi.useRealTimers();
  });

  it("flushes a session immediately on done", () => {
    const applied: Array<{ id: string; n: number }> = [];
    const coalescer = createStreamCoalescer((id, chunks) => {
      applied.push({ id, n: chunks.length });
    });
    coalescer.enqueue(chunk({ text: "a" }));
    coalescer.enqueue(chunk({ text: "b", done: true }));
    expect(applied).toEqual([{ id: "s1", n: 2 }]);
    coalescer.dispose();
  });

  it("flush drains pending tokens and dispose drops later ones", () => {
    const applied: StreamPayload[][] = [];
    const coalescer = createStreamCoalescer((_id, chunks) => {
      applied.push(chunks);
    });
    coalescer.enqueue(chunk({ text: "keep" }));
    coalescer.flush();
    expect(applied).toHaveLength(1);
    coalescer.dispose();
    coalescer.enqueue(chunk({ text: "late" }));
    expect(applied).toHaveLength(1);
  });

  it("uses draft key when session id is empty", () => {
    expect(streamSessionKey("")).toBe("__draft__");
    expect(streamSessionKey(null)).toBe("__draft__");
  });
});
