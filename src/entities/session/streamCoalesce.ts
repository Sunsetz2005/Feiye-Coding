import {
  applyStreamChunk,
  type ChatMessage,
  type StreamPayload,
} from "@/lib/session";

export type StreamApply = (sessionId: string, chunks: StreamPayload[]) => void;

export type StreamCoalescer = {
  enqueue: (chunk: StreamPayload) => void;
  flush: () => void;
  dispose: () => void;
};

const DRAFT_SESSION_KEY = "__draft__";

export function streamSessionKey(sessionId: string | null | undefined): string {
  return sessionId || DRAFT_SESSION_KEY;
}

export function applyStreamChunks(
  messages: ChatMessage[],
  chunks: readonly StreamPayload[],
): ChatMessage[] {
  let next = messages;
  for (const chunk of chunks) {
    next = applyStreamChunk(next, chunk);
  }
  return next;
}

/**
 * Batch Host stream events to one apply per animation frame, per session.
 * A `done` chunk flushes that session immediately so turn completion is not
 * delayed by a frame.
 */
export function createStreamCoalescer(apply: StreamApply): StreamCoalescer {
  const pending = new Map<string, StreamPayload[]>();
  let raf = 0;
  let disposed = false;

  const flushSession = (sessionId: string) => {
    const chunks = pending.get(sessionId);
    if (!chunks?.length) return;
    pending.delete(sessionId);
    apply(sessionId, chunks);
  };

  const flushAll = () => {
    raf = 0;
    if (disposed) return;
    const ids = [...pending.keys()];
    for (const id of ids) flushSession(id);
  };

  const schedule = () => {
    if (raf || disposed) return;
    const rafFn =
      typeof requestAnimationFrame === "function"
        ? requestAnimationFrame
        : (cb: FrameRequestCallback) =>
            setTimeout(() => cb(performance.now()), 16) as unknown as number;
    raf = rafFn(flushAll);
  };

  return {
    enqueue(chunk) {
      if (disposed) return;
      const id = streamSessionKey(chunk.sessionId);
      const list = pending.get(id);
      if (list) list.push(chunk);
      else pending.set(id, [chunk]);
      if (chunk.done) {
        flushSession(id);
        return;
      }
      schedule();
    },
    flush() {
      if (raf && typeof cancelAnimationFrame === "function") {
        cancelAnimationFrame(raf);
      }
      raf = 0;
      flushAll();
    },
    dispose() {
      disposed = true;
      if (raf && typeof cancelAnimationFrame === "function") {
        cancelAnimationFrame(raf);
      }
      raf = 0;
      pending.clear();
    },
  };
}

export { DRAFT_SESSION_KEY };
