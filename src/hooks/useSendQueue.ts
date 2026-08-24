import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type MutableRefObject,
  type RefObject,
} from "react";
import type { Attachment } from "@/lib/attachments";
import type { SessionSnapshot, SessionState } from "@/lib/session";
import {
  canShowQueueButton,
  claimQueueHead,
  dropQueuesForSessions,
  enqueueSend,
  getQueueForKey,
  makeQueuedSend,
  migrateDraftQueue,
  removeQueuedSend,
  requeueAfterFlushFail,
  SEND_QUEUE_MAX,
  setQueueForKey,
  shouldEnqueueSend,
  type QueuedSend,
} from "@/lib/sendQueue";

export type ExecuteSendFromQueue = (opts: {
  storedDisplay: string;
  att: Attachment[];
  goalMode: boolean;
  fromQueue: true;
  targetSessionId: string | null;
}) => Promise<boolean>;

export type UseSendQueueOptions = {
  queueKey: string;
  sessionState: SessionState;
  connecting: boolean;
  liveHostRef: RefObject<SessionSnapshot>;
  viewingSessionIdRef: MutableRefObject<string | null>;
  sendInFlightRef: MutableRefObject<boolean>;
  /** Always call via ref so flush sees the latest executeSend. */
  executeSendRef: MutableRefObject<ExecuteSendFromQueue>;
  /** CAS-persist each queue transition before it may advance. */
  persistQueuedState?: (
    key: string,
    queue: QueuedSend[],
  ) => Promise<boolean>;
  showToast: (msg: string, ms?: number) => void;
  labels: {
    queued: string;
    sendFailed: string;
    droppedOldest: (n: number, max: number) => string;
  };
};

function cloneQueue(queue: QueuedSend[]): QueuedSend[] {
  return queue.map((item) => ({
    ...item,
    attachments: item.attachments.map((attachment) => ({ ...attachment })),
  }));
}

function queuesEqual(left: QueuedSend[], right: QueuedSend[]): boolean {
  if (left.length !== right.length) return false;
  return left.every((item, index) => {
    const other = right[index];
    if (
      !other ||
      item.id !== other.id ||
      item.storedDisplay !== other.storedDisplay ||
      item.goalMode !== other.goalMode ||
      item.createdAt !== other.createdAt ||
      item.attachments.length !== other.attachments.length
    ) {
      return false;
    }
    return item.attachments.every((attachment, attachmentIndex) => {
      const otherAttachment = other.attachments[attachmentIndex];
      return (
        otherAttachment?.path === attachment.path &&
        otherAttachment.name === attachment.name &&
        otherAttachment.isDir === attachment.isDir
      );
    });
  });
}

type FlushHoldReason = "recovered" | "failed";

/**
 * Per-session follow-up send queue: enqueue while busy, auto-flush when idle,
 * claim/requeue on flush failure, hold after fail to avoid spin.
 */
export function useSendQueue({
  queueKey,
  sessionState,
  connecting,
  liveHostRef,
  viewingSessionIdRef,
  sendInFlightRef,
  executeSendRef,
  persistQueuedState,
  showToast,
  labels,
}: UseSendQueueOptions) {
  const [sendQueueByKey, setSendQueueByKey] = useState<
    Record<string, QueuedSend[]>
  >({});
  const sendQueueByKeyRef = useRef(sendQueueByKey);
  sendQueueByKeyRef.current = sendQueueByKey;

  const [flushHoldByKey, setFlushHoldByKey] = useState<
    Record<string, FlushHoldReason>
  >({});
  const flushHoldByKeyRef = useRef(flushHoldByKey);
  flushHoldByKeyRef.current = flushHoldByKey;
  /** Tracks a claimed item across `__draft__` materialization. */
  const pendingClaimKeyByIdRef = useRef(new Map<string, string | null>());
  const flushingKeysRef = useRef(new Set<string>());
  const flushQueueTimerRef = useRef<ReturnType<typeof setTimeout> | null>(
    null,
  );

  const activeQueue = useMemo(
    () => getQueueForKey(sendQueueByKey, queueKey),
    [sendQueueByKey, queueKey],
  );
  const flushHold = Boolean(flushHoldByKey[queueKey]);

  const setHold = useCallback((key: string, reason: FlushHoldReason | null) => {
    const current = flushHoldByKeyRef.current;
    if ((current[key] ?? null) === reason) return;
    const next = { ...current };
    if (reason) next[key] = reason;
    else delete next[key];
    flushHoldByKeyRef.current = next;
    setFlushHoldByKey(next);
  }, []);

  const releaseFlushHold = useCallback(() => {
    if (flushHoldByKeyRef.current[queueKey] === "failed") {
      setHold(queueKey, null);
    }
  }, [queueKey, setHold]);

  const cancelFlushTimer = useCallback(() => {
    if (flushQueueTimerRef.current) {
      clearTimeout(flushQueueTimerRef.current);
      flushQueueTimerRef.current = null;
    }
  }, []);

  const writeMap = useCallback((next: Record<string, QueuedSend[]>) => {
    sendQueueByKeyRef.current = next;
    setSendQueueByKey(next);
  }, []);

  const getSnapshot = useCallback(
    (key: string) => cloneQueue(getQueueForKey(sendQueueByKeyRef.current, key)),
    [],
  );

  const hydrateKey = useCallback(
    (key: string, queue: QueuedSend[], options?: { hold?: boolean }) => {
      const current = getQueueForKey(sendQueueByKeyRef.current, key);
      if (!queuesEqual(current, queue)) {
        writeMap(
          setQueueForKey(sendQueueByKeyRef.current, key, cloneQueue(queue)),
        );
      }
      if (options?.hold !== undefined) {
        setHold(key, options.hold ? "recovered" : null);
      }
    },
    [setHold, writeMap],
  );

  /** Enqueue a follow-up for the current session. Returns dropped count. */
  const enqueue = useCallback(
    (input: {
      storedDisplay: string;
      attachments: Attachment[];
      goalMode: boolean;
    }) => {
      const key = queueKey;
      const item = makeQueuedSend(input);
      const r = enqueueSend(getQueueForKey(sendQueueByKeyRef.current, key), item);
      writeMap(setQueueForKey(sendQueueByKeyRef.current, key, r.queue));
      if (r.dropped > 0) {
        showToast(labels.droppedOldest(r.dropped, SEND_QUEUE_MAX), 3200);
      } else {
        showToast(labels.queued, 2200);
      }
      return r.dropped;
    },
    [queueKey, showToast, labels, writeMap],
  );

  const removeItem = useCallback(
    (id: string) => {
      const key = queueKey;
      const next = setQueueForKey(
        sendQueueByKeyRef.current,
        key,
        removeQueuedSend(getQueueForKey(sendQueueByKeyRef.current, key), id),
      );
      writeMap(next);
      if (!getQueueForKey(next, key).length) cancelFlushTimer();
    },
    [queueKey, writeMap, cancelFlushTimer],
  );

  const clearQueue = useCallback(() => {
    cancelFlushTimer();
    writeMap(setQueueForKey(sendQueueByKeyRef.current, queueKey, []));
    setHold(queueKey, null);
  }, [queueKey, writeMap, cancelFlushTimer, setHold]);

  const clearDraftQueue = useCallback(() => {
    writeMap(setQueueForKey(sendQueueByKeyRef.current, "__draft__", []));
    setHold("__draft__", null);
  }, [writeMap, setHold]);

  const dropKeys = useCallback(
    (keys: Iterable<string>) => {
      const dropped = [...keys];
      const droppedSet = new Set(dropped);
      const next = dropQueuesForSessions(sendQueueByKeyRef.current, dropped);
      if (next !== sendQueueByKeyRef.current) writeMap(next);
      for (const key of dropped) setHold(key, null);
      for (const [id, key] of pendingClaimKeyByIdRef.current) {
        if (key !== null && droppedSet.has(key)) {
          pendingClaimKeyByIdRef.current.set(id, null);
        }
      }
      for (const key of dropped) flushingKeysRef.current.delete(key);
    },
    [setHold, writeMap],
  );

  const migrateDraft = useCallback(
    (newSessionId: string) => {
      const next = migrateDraftQueue(sendQueueByKeyRef.current, newSessionId);
      if (next !== sendQueueByKeyRef.current) writeMap(next);
      const draftHold = flushHoldByKeyRef.current["__draft__"];
      if (draftHold) {
        setHold("__draft__", null);
        setHold(newSessionId, draftHold);
      }
      for (const [id, key] of pendingClaimKeyByIdRef.current) {
        if (key === "__draft__") {
          pendingClaimKeyByIdRef.current.set(id, newSessionId);
        }
      }
      if (flushingKeysRef.current.delete("__draft__")) {
        flushingKeysRef.current.add(newSessionId);
      }
    },
    [setHold, writeMap],
  );

  const flush = useCallback(() => {
    if (sendInFlightRef.current) return;
    if (connecting) return;
    if (flushHoldByKeyRef.current[queueKey]) return;
    if (flushingKeysRef.current.has(queueKey)) return;
    const live = liveHostRef.current;
    const viewId = viewingSessionIdRef.current;
    if (live.sessionId && viewId && live.sessionId !== viewId) return;
    if (shouldEnqueueSend(live.state, false)) return;

    const claimKey = queueKey;
    const claimed = claimQueueHead(sendQueueByKeyRef.current, claimKey);
    if (!claimed) return;
    const { head } = claimed;
    pendingClaimKeyByIdRef.current.set(head.id, claimKey);
    flushingKeysRef.current.add(claimKey);
    writeMap(claimed.byKey);

    void (async () => {
      const restoreClaim = () => {
        const retryKey = pendingClaimKeyByIdRef.current.get(head.id);
        pendingClaimKeyByIdRef.current.delete(head.id);
        flushingKeysRef.current.delete(claimKey);
        if (retryKey === null) return null;
        const resolvedKey = retryKey ?? claimKey;
        flushingKeysRef.current.delete(resolvedKey);
        const restored = requeueAfterFlushFail(
          sendQueueByKeyRef.current,
          resolvedKey,
          head,
        );
        setHold(resolvedKey, "failed");
        writeMap(restored.byKey);
        return {
          key: resolvedKey,
          queue: cloneQueue(getQueueForKey(restored.byKey, resolvedKey)),
          dropped: restored.dropped,
        };
      };

      let claimDurable = true;
      try {
        if (persistQueuedState) {
          claimDurable = await persistQueuedState(
            claimKey,
            cloneQueue(getQueueForKey(claimed.byKey, claimKey)),
          );
        }
      } catch {
        claimDurable = false;
      }
      if (!claimDurable) {
        const restored = restoreClaim();
        if (!restored) return;
        showToast(labels.sendFailed, 3500);
        return;
      }

      let ok = false;
      const executeKey = pendingClaimKeyByIdRef.current.get(head.id);
      if (executeKey === null) {
        pendingClaimKeyByIdRef.current.delete(head.id);
        flushingKeysRef.current.delete(claimKey);
        return;
      }
      const resolvedExecuteKey = executeKey ?? claimKey;
      try {
        ok = await executeSendRef.current({
          storedDisplay: head.storedDisplay,
          att: head.attachments,
          goalMode: head.goalMode,
          fromQueue: true,
          targetSessionId:
            resolvedExecuteKey === "__draft__" ? null : resolvedExecuteKey,
        });
      } catch {
        ok = false;
      }
      if (ok) {
        pendingClaimKeyByIdRef.current.delete(head.id);
        flushingKeysRef.current.delete(claimKey);
        flushingKeysRef.current.delete(resolvedExecuteKey);
        return;
      }

      const restored = restoreClaim();
      if (!restored) return;
      let requeueDurable = true;
      if (persistQueuedState) {
        try {
          requeueDurable = await persistQueuedState(
            restored.key,
            restored.queue,
          );
        } catch {
          requeueDurable = false;
          // The in-memory queue remains held; the persistence port reports CAS
          // conflicts without refreshing over newer remote state.
        }
      }
      // A failed CAS deliberately has no retry here: retrying with refreshed
      // state could overwrite a newer remote queue. The held item stays visible.
      if (!requeueDurable) {
        showToast(labels.sendFailed, 3500);
        return;
      }
      if (restored.dropped > 0) {
        showToast(
          labels.droppedOldest(restored.dropped, SEND_QUEUE_MAX),
          3500,
        );
      } else {
        showToast(labels.sendFailed, 3500);
      }
    })();
  }, [
    connecting,
    liveHostRef,
    viewingSessionIdRef,
    sendInFlightRef,
    executeSendRef,
    persistQueuedState,
    showToast,
    labels,
    writeMap,
    setHold,
    queueKey,
  ]);

  // Clear flush hold once a real turn is in progress again.
  useEffect(() => {
    if (
      sessionState === "streaming" ||
      sessionState === "awaiting_permission"
    ) {
      if (flushHoldByKeyRef.current[queueKey] === "failed") {
        setHold(queueKey, null);
      }
    }
  }, [sessionState, queueKey, setHold]);

  // Auto-send next queued follow-up when the agent becomes idle.
  useEffect(() => {
    if (sessionState !== "ready" && sessionState !== "idle") return;
    if (
      connecting ||
      sendInFlightRef.current ||
      flushHoldByKeyRef.current[queueKey]
    ) {
      return;
    }
    if (!getQueueForKey(sendQueueByKeyRef.current, queueKey).length) return;
    cancelFlushTimer();
    flushQueueTimerRef.current = setTimeout(() => {
      flushQueueTimerRef.current = null;
      flush();
    }, 40);
    return () => cancelFlushTimer();
  }, [
    sessionState,
    queueKey,
    connecting,
    sendQueueByKey,
    flush,
    cancelFlushTimer,
    sendInFlightRef,
  ]);

  /** Clear hold and try flush immediately (user retry). */
  const resumeFlush = useCallback(() => {
    setHold(queueKey, null);
    // Defer so ref/state settle before claim.
    window.setTimeout(() => flush(), 0);
  }, [queueKey, setHold, flush]);

  return {
    activeQueue,
    flushHold,
    getSnapshot,
    hydrateKey,
    enqueue,
    removeItem,
    clearQueue,
    clearDraftQueue,
    dropKeys,
    migrateDraft,
    releaseFlushHold,
    resumeFlush,
    shouldEnqueue: (state: SessionState, conn: boolean) =>
      shouldEnqueueSend(state, conn),
    canShowQueueButton: (state: SessionState, conn: boolean, hasBody: boolean) =>
      canShowQueueButton(state, conn, hasBody),
  };
}
