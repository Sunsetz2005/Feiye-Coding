// @vitest-environment jsdom

import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { SessionSnapshot, SessionState } from "@/lib/session";
import type { QueuedSend } from "@/lib/sendQueue";
import {
  useSendQueue,
  type ExecuteSendFromQueue,
} from "./useSendQueue";

function queued(id: string, storedDisplay = id): QueuedSend {
  return {
    id,
    storedDisplay,
    attachments: [
      { path: `/tmp/${id}`, name: `${id}.txt`, isDir: false },
    ],
    goalMode: false,
    createdAt: 1,
  };
}

function setup(options?: {
  queueKey?: string;
  persistQueuedState?: (
    key: string,
    queue: QueuedSend[],
  ) => Promise<boolean>;
  executeSend?: ExecuteSendFromQueue;
}) {
  const queueKey = options?.queueKey ?? "session-1";
  const liveHostRef = {
    current: {
      sessionId: queueKey === "__draft__" ? null : queueKey,
      state: "ready",
      lastError: null,
      streamingMessageId: null,
      backend: "codex",
    } satisfies SessionSnapshot,
  };
  const viewingSessionIdRef = {
    current: queueKey === "__draft__" ? null : queueKey,
  };
  const sendInFlightRef = { current: false };
  const executeSendRef = {
    current: options?.executeSend ?? vi.fn(async () => true),
  };
  const showToast = vi.fn();

  const hook = renderHook(
    ({
      activeKey,
      sessionState,
    }: {
      activeKey: string;
      sessionState?: SessionState;
    }) =>
      useSendQueue({
        queueKey: activeKey,
        sessionState: sessionState ?? "ready",
        connecting: false,
        liveHostRef,
        viewingSessionIdRef,
        sendInFlightRef,
        executeSendRef,
        persistQueuedState: options?.persistQueuedState,
        showToast,
        labels: {
          queued: "queued",
          sendFailed: "send failed",
          droppedOldest: (n, max) => `${n}/${max}`,
        },
      }),
    {
      initialProps: {
        activeKey: queueKey,
        sessionState: undefined as SessionState | undefined,
      },
    },
  );

  return {
    ...hook,
    executeSendRef,
    liveHostRef,
    viewingSessionIdRef,
    showToast,
  };
}

async function flushTimers() {
  await act(async () => {
    await vi.runAllTimersAsync();
  });
}

describe("useSendQueue recovery", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("hydrates exactly, returns deep snapshots, and holds restored sends", async () => {
    const { result, rerender, executeSendRef } = setup();
    const recovered = [queued("q1")];

    act(() => {
      result.current.hydrateKey("session-1", recovered, { hold: true });
      result.current.hydrateKey("session-1", recovered, { hold: true });
    });

    recovered[0]!.attachments[0]!.name = "mutated-source.txt";
    const snapshot = result.current.getSnapshot("session-1");
    snapshot[0]!.attachments[0]!.name = "mutated-snapshot.txt";

    rerender({ activeKey: "session-1", sessionState: "streaming" });
    rerender({ activeKey: "session-1", sessionState: "ready" });
    await flushTimers();

    expect(result.current.activeQueue.map((item) => item.id)).toEqual(["q1"]);
    expect(result.current.activeQueue[0]!.attachments[0]!.name).toBe("q1.txt");
    expect(result.current.flushHold).toBe(true);
    expect(executeSendRef.current).not.toHaveBeenCalled();
  });

  it.each([
    ["returns false", async () => false],
    [
      "throws",
      async () => {
        throw new Error("persistence unavailable");
      },
    ],
  ])("requeues and holds when the durable claim %s", async (_, rejectClaim) => {
    const persistQueuedState = vi.fn<
      (key: string, remaining: QueuedSend[]) => Promise<boolean>
    >(async () => rejectClaim());
    const executeSend = vi.fn(async () => true);
    const { result, executeSendRef } = setup({
      persistQueuedState,
      executeSend,
    });
    const first = queued("q1");
    const second = queued("q2");

    act(() => {
      result.current.hydrateKey("session-1", [first, second], { hold: false });
    });
    await flushTimers();

    expect(persistQueuedState).toHaveBeenCalledTimes(1);
    expect(persistQueuedState.mock.calls[0]?.[0]).toBe("session-1");
    expect(
      persistQueuedState.mock.calls[0]?.[1].map((item) => item.id),
    ).toEqual(["q2"]);
    expect(executeSendRef.current).not.toHaveBeenCalled();
    expect(result.current.activeQueue.map((item) => item.id)).toEqual([
      "q1",
      "q2",
    ]);
    expect(result.current.flushHold).toBe(true);
  });

  it("executes a durably claimed send only once", async () => {
    const persistQueuedState = vi.fn(async () => true);
    const executeSend = vi.fn(async () => true);
    const { result, executeSendRef } = setup({
      persistQueuedState,
      executeSend,
    });

    const boundToken = `[[skill-v1:review|${"a".repeat(64)}|${"b".repeat(64)}|accepted_suggestion]]`;
    act(() => {
      result.current.hydrateKey("session-1", [queued("q1", boundToken)], {
        hold: false,
      });
    });
    await flushTimers();
    await flushTimers();

    expect(persistQueuedState).toHaveBeenCalledTimes(1);
    expect(executeSendRef.current).toHaveBeenCalledTimes(1);
    expect(executeSendRef.current).toHaveBeenCalledWith(
      expect.objectContaining({ storedDisplay: boundToken }),
    );
    expect(result.current.activeQueue).toEqual([]);
  });

  it("serializes claims while the first queue CAS is pending", async () => {
    let finishPersist!: (ok: boolean) => void;
    const persistQueuedState = vi.fn<
      (key: string, queue: QueuedSend[]) => Promise<boolean>
    >(
      (_key, _queue) =>
        new Promise<boolean>((resolve) => {
          finishPersist = resolve;
        }),
    );
    const executeSend = vi.fn(async () => true);
    const { result, executeSendRef } = setup({
      persistQueuedState,
      executeSend,
    });

    act(() => {
      result.current.hydrateKey(
        "session-1",
        [queued("q1"), queued("q2")],
        { hold: false },
      );
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(200);
    });

    expect(persistQueuedState).toHaveBeenCalledTimes(1);
    expect(
      persistQueuedState.mock.calls[0]?.[1].map((item) => item.id),
    ).toEqual(["q2"]);
    expect(executeSendRef.current).not.toHaveBeenCalled();

    await act(async () => {
      finishPersist(true);
      await Promise.resolve();
    });
    await flushTimers();

    expect(persistQueuedState).toHaveBeenCalledTimes(2);
    expect(persistQueuedState.mock.calls[1]?.[1]).toEqual([]);
    expect(executeSendRef.current).toHaveBeenCalledTimes(1);
    expect(result.current.activeQueue).toEqual([]);
  });

  it("requeues a failed draft claim under its materialized session key", async () => {
    const events: string[] = [];
    const persistQueuedState = vi.fn(
      async (key: string, queue: QueuedSend[]) => {
        events.push(`persist:${key}:${queue.map((item) => item.id).join(",")}`);
        return queue.length === 0;
      },
    );
    let finishSend!: (ok: boolean) => void;
    const executeSend = vi.fn(
      () =>
        new Promise<boolean>((resolve) => {
          events.push("execute");
          finishSend = resolve;
        }),
    );
    const {
      result,
      rerender,
      executeSendRef,
      liveHostRef,
      viewingSessionIdRef,
      showToast,
    } = setup({
      queueKey: "__draft__",
      persistQueuedState,
      executeSend,
    });

    act(() => {
      result.current.hydrateKey("__draft__", [queued("draft-q")], {
        hold: false,
      });
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(40);
    });
    expect(executeSendRef.current).toHaveBeenCalledTimes(1);

    act(() => {
      result.current.migrateDraft("session-real");
      liveHostRef.current = { ...liveHostRef.current, sessionId: "session-real" };
      viewingSessionIdRef.current = "session-real";
      rerender({ activeKey: "session-real", sessionState: undefined });
      finishSend(false);
    });
    await act(async () => Promise.resolve());

    expect(executeSendRef.current).toHaveBeenCalledTimes(1);
    expect(result.current.getSnapshot("__draft__")).toEqual([]);
    expect(
      result.current.getSnapshot("session-real").map((item) => item.id),
    ).toEqual(["draft-q"]);
    expect(result.current.flushHold).toBe(true);
    expect(events).toEqual([
      "persist:__draft__:",
      "execute",
      "persist:session-real:draft-q",
    ]);
    expect(persistQueuedState).toHaveBeenCalledTimes(2);
    await flushTimers();
    expect(executeSendRef.current).toHaveBeenCalledTimes(1);
    expect(showToast).toHaveBeenCalledWith("send failed", 3500);
  });
});
