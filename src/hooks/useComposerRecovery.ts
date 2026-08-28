import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type Dispatch,
  type SetStateAction,
} from "react";
import type { Attachment } from "@/lib/attachments";
import {
  COMPOSER_RECOVERY_DRAFT_KEY,
  cloneComposerRecoveryStateV1,
  cloneMemoryPack,
  composerRecoveryDeleteV1,
  composerRecoveryGetV1,
  composerRecoveryMigrateV1,
  composerRecoveryPutV1,
  memoryPacksEqual,
  type ComposerMemoryPackRefV1,
  type ComposerRecoveryRevisionV1,
  type ComposerRecoveryStateV1,
} from "@/lib/composerRecovery";
import type { QueuedSend } from "@/lib/sendQueue";

export const COMPOSER_RECOVERY_DEBOUNCE_MS = 300;
/** Bounds clean WebView cache; dirty/in-flight state is never evicted. */
export const COMPOSER_RECOVERY_MEMORY_CACHE_MAX = 32;

export interface ComposerRecoveryQueuePort {
  getSnapshot: (key: string) => QueuedSend[];
  hydrateKey: (
    key: string,
    queue: QueuedSend[],
    options?: { hold?: boolean },
  ) => void;
  migrateDraft: (newSessionId: string) => void;
  dropKeys: (keys: Iterable<string>) => void;
}

export interface UseComposerRecoveryOptions {
  enabled: boolean;
  recoveryKey: string;
  setRecoveryKey: Dispatch<SetStateAction<string>>;
  draft: string;
  attachments: Attachment[];
  activeQueue: QueuedSend[];
  setDraft: Dispatch<SetStateAction<string>>;
  setAttachments: Dispatch<SetStateAction<Attachment[]>>;
  memoryPack?: ComposerMemoryPackRefV1 | null;
  setMemoryPack?: (value: ComposerMemoryPackRefV1 | null) => void;
  queue: ComposerRecoveryQueuePort;
  onError?: (error: unknown) => void;
  debounceMs?: number;
}

interface RecoveryEntry {
  revision: ComposerRecoveryRevisionV1;
  state: ComposerRecoveryStateV1;
  loaded: boolean;
  dirty: boolean;
  sequence: number;
  timer: ReturnType<typeof setTimeout> | null;
  drainPromise: Promise<boolean> | null;
  loadGeneration: number;
  /** Queue claim/requeue CAS must never refresh and overwrite newer remote state. */
  forbidConflictRefresh: boolean;
}

const EMPTY_STATE: ComposerRecoveryStateV1 = {
  draft: "",
  attachments: [],
  queue: [],
  memoryPack: null,
};

function cloneAttachments(attachments: Attachment[]): Attachment[] {
  return attachments.map((attachment) => ({ ...attachment }));
}

function cloneQueue(queue: QueuedSend[]): QueuedSend[] {
  return queue.map((item) => ({
    ...item,
    attachments: cloneAttachments(item.attachments),
  }));
}

function attachmentsEqual(left: Attachment[], right: Attachment[]): boolean {
  return (
    left.length === right.length &&
    left.every(
      (item, index) =>
        item.path === right[index]?.path &&
        item.name === right[index]?.name &&
        item.isDir === right[index]?.isDir,
    )
  );
}

function queueEqual(left: QueuedSend[], right: QueuedSend[]): boolean {
  return (
    left.length === right.length &&
    left.every((item, index) => {
      const candidate = right[index];
      return (
        !!candidate &&
        item.id === candidate.id &&
        item.storedDisplay === candidate.storedDisplay &&
        item.goalMode === candidate.goalMode &&
        item.createdAt === candidate.createdAt &&
        attachmentsEqual(item.attachments, candidate.attachments)
      );
    })
  );
}

function recoveryStateEqual(
  left: ComposerRecoveryStateV1,
  right: ComposerRecoveryStateV1,
): boolean {
  return (
    left.draft === right.draft &&
    attachmentsEqual(left.attachments, right.attachments) &&
    queueEqual(left.queue, right.queue) &&
    memoryPacksEqual(left.memoryPack, right.memoryPack)
  );
}

function isCasConflict(error: unknown): boolean {
  const text = String(error).toLowerCase();
  return (
    text.includes("composer_recovery_stale") ||
    text.includes("migration_conflict") ||
    text.includes("cas") ||
    text.includes("revision conflict")
  );
}

function makeEntry(
  state: ComposerRecoveryStateV1 = EMPTY_STATE,
  options?: {
    revision?: ComposerRecoveryRevisionV1;
    loaded?: boolean;
    dirty?: boolean;
  },
): RecoveryEntry {
  return {
    revision: options?.revision ?? 0,
    state: cloneComposerRecoveryStateV1(state),
    loaded: options?.loaded ?? false,
    dirty: options?.dirty ?? false,
    sequence: 0,
    timer: null,
    drainPromise: null,
    loadGeneration: 0,
    forbidConflictRefresh: false,
  };
}

/**
 * Coordinates per-session composer state with the Host CAS journal.
 *
 * React owns the visible draft/attachments and useSendQueue owns queues. This
 * hook owns only recovery identity, revisions and an in-memory write-back cache,
 * keeping a failed Host write from erasing the user's current WebView state.
 */
export function useComposerRecovery({
  enabled,
  recoveryKey,
  setRecoveryKey,
  draft,
  attachments,
  activeQueue,
  setDraft,
  setAttachments,
  memoryPack = null,
  setMemoryPack,
  queue,
  onError,
  debounceMs = COMPOSER_RECOVERY_DEBOUNCE_MS,
}: UseComposerRecoveryOptions) {
  const entriesRef = useRef(new Map<string, RecoveryEntry>());
  const activeKeyRef = useRef(recoveryKey);
  const activationEpochRef = useRef(0);
  const mountedRef = useRef(true);
  const enabledRef = useRef(enabled);
  const draftRef = useRef(draft);
  const attachmentsRef = useRef(attachments);
  const memoryPackRef = useRef<ComposerMemoryPackRefV1 | null>(memoryPack);
  const setMemoryPackRef = useRef(setMemoryPack);
  const queueRef = useRef(queue);
  const onErrorRef = useRef(onError);
  const debounceMsRef = useRef(debounceMs);
  const [readyKey, setReadyKey] = useState<string | null>(null);

  enabledRef.current = enabled;
  draftRef.current = draft;
  attachmentsRef.current = attachments;
  setMemoryPackRef.current = setMemoryPack;
  queueRef.current = queue;
  onErrorRef.current = onError;
  debounceMsRef.current = debounceMs;
  const memoryPackPropRef = useRef(memoryPack);
  const memoryPackPropChanged = !memoryPacksEqual(
    memoryPackPropRef.current,
    memoryPack,
  );
  if (memoryPackPropChanged) {
    memoryPackPropRef.current = memoryPack;
    memoryPackRef.current = cloneMemoryPack(memoryPack);
  }

  const reportError = useCallback((error: unknown) => {
    onErrorRef.current?.(error);
  }, []);

  const pruneCleanEntries = useCallback(() => {
    const entries = entriesRef.current;
    if (entries.size <= COMPOSER_RECOVERY_MEMORY_CACHE_MAX) return;
    for (const [key, entry] of entries) {
      if (entries.size <= COMPOSER_RECOVERY_MEMORY_CACHE_MAX) break;
      if (
        key === activeKeyRef.current ||
        !entry.loaded ||
        entry.dirty ||
        entry.timer ||
        entry.drainPromise
      ) {
        continue;
      }
      entries.delete(key);
      queueRef.current.dropKeys([key]);
    }
  }, []);

  const readVisibleState = useCallback(
    (key: string): ComposerRecoveryStateV1 => ({
      draft: draftRef.current,
      attachments: cloneAttachments(attachmentsRef.current),
      queue: cloneQueue(queueRef.current.getSnapshot(key)),
      memoryPack: cloneMemoryPack(memoryPackRef.current),
    }),
    [],
  );

  const applyVisibleState = useCallback(
    (
      key: string,
      state: ComposerRecoveryStateV1,
      options?: { holdQueue?: boolean; preserveQueueHold?: boolean },
    ) => {
      const next = cloneComposerRecoveryStateV1(state);
      activeKeyRef.current = key;
      draftRef.current = next.draft;
      attachmentsRef.current = next.attachments;
      queueRef.current.hydrateKey(
        key,
        next.queue,
        options?.preserveQueueHold
          ? undefined
          : { hold: options?.holdQueue ?? next.queue.length > 0 },
      );
      setRecoveryKey(key);
      setDraft(next.draft);
      setAttachments(next.attachments);
      memoryPackRef.current = cloneMemoryPack(next.memoryPack);
      setMemoryPackRef.current?.(cloneMemoryPack(next.memoryPack));
    },
    [setAttachments, setDraft, setRecoveryKey],
  );

  const clearTimer = useCallback((entry: RecoveryEntry) => {
    if (!entry.timer) return;
    clearTimeout(entry.timer);
    entry.timer = null;
  }, []);

  const drainKeyRef = useRef<
    (key: string, allowConflictRefresh?: boolean) => Promise<boolean>
  >(async () => false);

  const scheduleWrite = useCallback(
    (key: string, immediate = false) => {
      const entry = entriesRef.current.get(key);
      if (!entry || !entry.dirty) return;
      clearTimer(entry);
      entry.timer = setTimeout(
        () => {
          entry.timer = null;
          void drainKeyRef.current(key).catch(reportError);
        },
        immediate ? 0 : debounceMsRef.current,
      );
    },
    [clearTimer, reportError],
  );

  const updateEntry = useCallback(
    (
      key: string,
      state: ComposerRecoveryStateV1,
      options?: { forceDirty?: boolean; immediate?: boolean },
    ): RecoveryEntry => {
      let entry = entriesRef.current.get(key);
      if (!entry) {
        entry = makeEntry(state, { loaded: true, dirty: true });
        entriesRef.current.set(key, entry);
        scheduleWrite(key, options?.immediate);
        return entry;
      }
      if (!options?.forceDirty && recoveryStateEqual(entry.state, state)) {
        if (entry.dirty) scheduleWrite(key, options?.immediate);
        return entry;
      }
      entry.state = cloneComposerRecoveryStateV1(state);
      entry.loaded = true;
      entry.dirty = true;
      entry.sequence += 1;
      scheduleWrite(key, options?.immediate);
      return entry;
    },
    [scheduleWrite],
  );

  const refreshRevision = useCallback(
    async (key: string, entry: RecoveryEntry): Promise<boolean> => {
      try {
        const snapshot = await composerRecoveryGetV1(key);
        entry.revision = snapshot?.revision ?? 0;
        entry.loaded = true;
        return true;
      } catch (error) {
        reportError(error);
        return false;
      }
    },
    [reportError],
  );

  const drainKey = useCallback(
    async (key: string, allowConflictRefresh = true): Promise<boolean> => {
      const entry = entriesRef.current.get(key);
      if (!entry) return true;
      clearTimer(entry);
      if (!enabledRef.current) {
        entry.dirty = false;
        return true;
      }
      if (entry.drainPromise) return entry.drainPromise;

      const run = async (): Promise<boolean> => {
        let canRefreshConflict = allowConflictRefresh;
        while (entry.dirty) {
          const sequence = entry.sequence;
          const state = cloneComposerRecoveryStateV1(entry.state);
          try {
            const result = await composerRecoveryPutV1(
              key,
              state,
              entry.revision,
            );
            if (result) entry.revision = result.revision;
            if (
              entry.sequence === sequence &&
              recoveryStateEqual(entry.state, state)
            ) {
              entry.dirty = false;
            }
          } catch (error) {
            if (
              canRefreshConflict &&
              !entry.forbidConflictRefresh &&
              isCasConflict(error)
            ) {
              canRefreshConflict = false;
              if (await refreshRevision(key, entry)) continue;
            }
            reportError(error);
            return false;
          }
        }
        pruneCleanEntries();
        return true;
      };

      const promise = run();
      entry.drainPromise = promise;
      try {
        return await promise;
      } finally {
        if (entry.drainPromise === promise) entry.drainPromise = null;
      }
    },
    [clearTimer, pruneCleanEntries, refreshRevision, reportError],
  );
  drainKeyRef.current = drainKey;

  const captureActive = useCallback(
    (options?: { immediate?: boolean }) => {
      const key = activeKeyRef.current;
      const entry = entriesRef.current.get(key);
      if (!entry) return;
      updateEntry(key, readVisibleState(key), {
        immediate: options?.immediate,
      });
    },
    [readVisibleState, updateEntry],
  );

  const activate = useCallback(
    async (key: string): Promise<void> => {
      const outgoingKey = activeKeyRef.current;
      if (entriesRef.current.has(outgoingKey)) {
        captureActive({ immediate: true });
      }

      const epoch = ++activationEpochRef.current;
      setReadyKey(null);
      let entry = entriesRef.current.get(key);
      if (entry?.loaded || entry?.dirty) {
        applyVisibleState(key, entry.state, {
          holdQueue: entry.state.queue.length > 0,
        });
        if (entry.dirty) scheduleWrite(key);
        setReadyKey(key);
        return;
      }

      if (!entry) {
        entry = makeEntry();
        entriesRef.current.set(key, entry);
      }
      const loadGeneration = ++entry.loadGeneration;
      applyVisibleState(key, entry.state, { holdQueue: true });

      if (!enabledRef.current) {
        entry.loaded = true;
        if (mountedRef.current && activationEpochRef.current === epoch) {
          queueRef.current.hydrateKey(key, entry.state.queue, { hold: false });
          setReadyKey(key);
        }
        return;
      }

      try {
        const snapshot = await composerRecoveryGetV1(key);
        const current = entriesRef.current.get(key);
        if (!current || current.loadGeneration !== loadGeneration) return;
        current.revision = snapshot?.revision ?? 0;
        current.loaded = true;
        // Local edits made while get was in flight always win over the journal.
        if (!current.dirty && snapshot) {
          current.state = cloneComposerRecoveryStateV1(snapshot.state);
        }
        if (current.dirty) scheduleWrite(key);
        else pruneCleanEntries();
        if (
          mountedRef.current &&
          activationEpochRef.current === epoch &&
          activeKeyRef.current === key
        ) {
          applyVisibleState(key, current.state, {
            holdQueue: current.state.queue.length > 0,
          });
          setReadyKey(key);
        }
      } catch (error) {
        if (entry.loadGeneration !== loadGeneration) return;
        entry.loaded = true;
        reportError(error);
        if (
          mountedRef.current &&
          activationEpochRef.current === epoch &&
          activeKeyRef.current === key
        ) {
          // A failed read must not clear whatever is already in WebView memory.
          applyVisibleState(key, entry.state, {
            holdQueue: entry.state.queue.length > 0,
          });
          setReadyKey(key);
        }
      }
    },
    [
      applyVisibleState,
      captureActive,
      pruneCleanEntries,
      reportError,
      scheduleWrite,
    ],
  );

  /** Update cache synchronously when submit clears React state before re-render. */
  const captureComposer = useCallback(
    (nextDraft: string, nextAttachments: Attachment[]) => {
      const key = activeKeyRef.current;
      draftRef.current = nextDraft;
      attachmentsRef.current = cloneAttachments(nextAttachments);
      const state: ComposerRecoveryStateV1 = {
        draft: nextDraft,
        attachments: cloneAttachments(nextAttachments),
        queue: cloneQueue(queueRef.current.getSnapshot(key)),
        memoryPack: cloneMemoryPack(memoryPackRef.current),
      };
      updateEntry(key, state);
    },
    [updateEntry],
  );

  /**
   * Persist an optimistic dequeue before the queued item can reach sessionSend.
   * This closes the reload window that would otherwise resend a recovered head.
   */
  const persistQueuedState = useCallback(
    async (key: string, remaining: QueuedSend[]): Promise<boolean> => {
      const entry = entriesRef.current.get(key);
      const base =
        key === activeKeyRef.current
          ? readVisibleState(key)
          : cloneComposerRecoveryStateV1(entry?.state ?? EMPTY_STATE);
      base.queue = cloneQueue(remaining);
      const updated = updateEntry(key, base, {
        forceDirty: true,
        immediate: true,
      });
      updated.forbidConflictRefresh = true;
      const persisted = await drainKey(key, false);
      if (persisted) updated.forbidConflictRefresh = false;
      return persisted;
    },
    [drainKey, readVisibleState, updateEntry],
  );

  const loadRevisionOnly = useCallback(
    async (key: string): Promise<RecoveryEntry | null> => {
      let entry = entriesRef.current.get(key);
      if (entry?.loaded) return entry;
      if (!entry) {
        entry = makeEntry();
        entriesRef.current.set(key, entry);
      }
      if (!enabledRef.current) {
        entry.loaded = true;
        return entry;
      }
      try {
        const snapshot = await composerRecoveryGetV1(key);
        entry.revision = snapshot?.revision ?? 0;
        entry.loaded = true;
        if (!entry.dirty && snapshot) {
          entry.state = cloneComposerRecoveryStateV1(snapshot.state);
        }
        return entry;
      } catch (error) {
        reportError(error);
        return null;
      }
    },
    [reportError],
  );

  const migrateDraft = useCallback(
    async (toKey: string): Promise<boolean> => {
      if (toKey === COMPOSER_RECOVERY_DRAFT_KEY) return false;
      if (activeKeyRef.current === COMPOSER_RECOVERY_DRAFT_KEY) {
        captureActive({ immediate: true });
      }
      const draftEntry = await loadRevisionOnly(COMPOSER_RECOVERY_DRAFT_KEY);
      const targetEntry = await loadRevisionOnly(toKey);
      if (!draftEntry || !targetEntry) return false;
      if (!(await drainKey(COMPOSER_RECOVERY_DRAFT_KEY))) return false;

      try {
        const result = await composerRecoveryMigrateV1(
          toKey,
          draftEntry.revision,
          targetEntry.revision,
        );
        queueRef.current.migrateDraft(toKey);
        const migratedQueue = queueRef.current.getSnapshot(toKey);
        const migratedState = cloneComposerRecoveryStateV1(draftEntry.state);
        migratedState.queue = cloneQueue(migratedQueue);
        draftEntry.revision = result?.fromRevision ?? draftEntry.revision;
        draftEntry.state = cloneComposerRecoveryStateV1(EMPTY_STATE);
        draftEntry.loaded = true;
        draftEntry.dirty = false;
        targetEntry.revision = result?.toRevision ?? targetEntry.revision;
        targetEntry.state = migratedState;
        targetEntry.loaded = true;
        targetEntry.dirty = false;
        if (activeKeyRef.current === COMPOSER_RECOVERY_DRAFT_KEY) {
          ++activationEpochRef.current;
          applyVisibleState(toKey, migratedState, {
            preserveQueueHold: true,
          });
          setReadyKey(toKey);
        }
        pruneCleanEntries();
        return true;
      } catch (error) {
        // A dual-CAS conflict means another writer changed one side. Keep the
        // draft intact and let ensureConnected roll back the new empty row.
        reportError(error);
        return false;
      }
    },
    [
      applyVisibleState,
      captureActive,
      drainKey,
      loadRevisionOnly,
      pruneCleanEntries,
      reportError,
    ],
  );

  const deleteKey = useCallback(
    async (key: string): Promise<boolean> => {
      const entry = await loadRevisionOnly(key);
      if (!entry) return false;
      clearTimer(entry);
      if (entry.drainPromise) await entry.drainPromise;
      if (!enabledRef.current) {
        entriesRef.current.delete(key);
        queueRef.current.dropKeys([key]);
        return true;
      }

      let mayRefreshConflict = true;
      for (;;) {
        try {
          await composerRecoveryDeleteV1(key, entry.revision);
          entriesRef.current.delete(key);
          queueRef.current.dropKeys([key]);
          return true;
        } catch (error) {
          if (mayRefreshConflict && isCasConflict(error)) {
            mayRefreshConflict = false;
            if (await refreshRevision(key, entry)) continue;
          }
          reportError(error);
          return false;
        }
      }
    },
    [clearTimer, loadRevisionOnly, refreshRevision, reportError],
  );

  const resetDraft = useCallback(
    async (seedDraft = ""): Promise<boolean> => {
      const outgoingKey = activeKeyRef.current;
      if (entriesRef.current.has(outgoingKey)) {
        captureActive({ immediate: true });
      }
      const previousDraftEntry = entriesRef.current.get(
        COMPOSER_RECOVERY_DRAFT_KEY,
      );
      if (previousDraftEntry) clearTimer(previousDraftEntry);

      const nextEntry = makeEntry(
        { draft: seedDraft, attachments: [], queue: [], memoryPack: null },
        { loaded: true, dirty: true },
      );
      entriesRef.current.set(COMPOSER_RECOVERY_DRAFT_KEY, nextEntry);
      ++activationEpochRef.current;
      // Explicit New Chat also cancels any in-flight draft queue claim.
      queueRef.current.dropKeys([COMPOSER_RECOVERY_DRAFT_KEY]);
      applyVisibleState(COMPOSER_RECOVERY_DRAFT_KEY, nextEntry.state, {
        holdQueue: false,
      });
      setReadyKey(COMPOSER_RECOVERY_DRAFT_KEY);

      if (previousDraftEntry?.drainPromise) {
        await previousDraftEntry.drainPromise;
      }
      if (!enabledRef.current) {
        nextEntry.dirty = false;
        return true;
      }

      let revision = previousDraftEntry?.revision ?? 0;
      if (!previousDraftEntry?.loaded) {
        const snapshot = await composerRecoveryGetV1(
          COMPOSER_RECOVERY_DRAFT_KEY,
        ).catch((error) => {
          reportError(error);
          return null;
        });
        revision = snapshot?.revision ?? 0;
      }
      try {
        const result = await composerRecoveryDeleteV1(
          COMPOSER_RECOVERY_DRAFT_KEY,
          revision,
        );
        nextEntry.revision = result?.revision ?? revision;
      } catch (error) {
        if (isCasConflict(error)) {
          const snapshot = await composerRecoveryGetV1(
            COMPOSER_RECOVERY_DRAFT_KEY,
          ).catch(() => null);
          try {
            const result = await composerRecoveryDeleteV1(
              COMPOSER_RECOVERY_DRAFT_KEY,
              snapshot?.revision ?? 0,
            );
            nextEntry.revision = result?.revision ?? snapshot?.revision ?? 0;
          } catch (retryError) {
            reportError(retryError);
            scheduleWrite(COMPOSER_RECOVERY_DRAFT_KEY);
            return false;
          }
        } else {
          reportError(error);
          scheduleWrite(COMPOSER_RECOVERY_DRAFT_KEY);
          return false;
        }
      }
      return drainKey(COMPOSER_RECOVERY_DRAFT_KEY);
    },
    [
      applyVisibleState,
      captureActive,
      clearTimer,
      drainKey,
      reportError,
      scheduleWrite,
    ],
  );

  // React state changes are write-backed only after an entry has been activated.
  useEffect(() => {
    if (activeKeyRef.current !== recoveryKey) return;
    const entry = entriesRef.current.get(recoveryKey);
    if (!entry) return;
    updateEntry(recoveryKey, {
      draft,
      attachments: cloneAttachments(attachments),
      queue: cloneQueue(activeQueue),
      memoryPack: cloneMemoryPack(memoryPackRef.current),
    });
  }, [activeQueue, attachments, draft, memoryPack, recoveryKey, updateEntry]);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      for (const entry of entriesRef.current.values()) clearTimer(entry);
    };
  }, [clearTimer]);

  return {
    ready: readyKey === recoveryKey,
    activate,
    captureActive,
    captureComposer,
    persistQueuedState,
    migrateDraft,
    deleteKey,
    resetDraft,
    flush: (key = activeKeyRef.current) => drainKey(key),
  };
}
