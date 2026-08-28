// @vitest-environment jsdom

import { act, renderHook } from "@testing-library/react";
import { useCallback, useRef, useState } from "react";
import {
  afterEach,
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from "vitest";
import type { Attachment } from "@/lib/attachments";
import {
  COMPOSER_RECOVERY_DRAFT_KEY,
  type ComposerRecoverySnapshotV1,
  type ComposerRecoveryStateV1,
} from "@/lib/composerRecovery";
import {
  migrateDraftQueue,
  setQueueForKey,
  type QueuedSend,
} from "@/lib/sendQueue";

const recoveryHost = vi.hoisted(() => ({
  get: vi.fn(),
  put: vi.fn(),
  migrate: vi.fn(),
  delete: vi.fn(),
}));

vi.mock("@/lib/composerRecovery", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("@/lib/composerRecovery")>();
  return {
    ...actual,
    composerRecoveryGetV1: recoveryHost.get,
    composerRecoveryPutV1: recoveryHost.put,
    composerRecoveryMigrateV1: recoveryHost.migrate,
    composerRecoveryDeleteV1: recoveryHost.delete,
  };
});

import {
  COMPOSER_RECOVERY_MEMORY_CACHE_MAX,
  useComposerRecovery,
} from "./useComposerRecovery";

function queued(id: string): QueuedSend {
  return {
    id,
    storedDisplay: `send ${id}`,
    attachments: [],
    goalMode: false,
    createdAt: 100,
  };
}

function state(
  draft: string,
  queue: QueuedSend[] = [],
  attachments: Attachment[] = [],
): ComposerRecoveryStateV1 {
  return { draft, attachments, queue };
}

function snapshot(
  key: string,
  revision: number,
  value: ComposerRecoveryStateV1,
): ComposerRecoverySnapshotV1 {
  return {
    version: 1,
    key,
    revision,
    state: value,
    filteredAttachmentCount: 0,
    filteredQueueItemCount: 0,
  };
}

function useHarness() {
  const [recoveryKey, setRecoveryKey] = useState(
    COMPOSER_RECOVERY_DRAFT_KEY as string,
  );
  const [draft, setDraft] = useState("");
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  const [memoryPack, setMemoryPack] = useState<
    ComposerRecoveryStateV1["memoryPack"]
  >(null);
  const [queues, setQueues] = useState<Record<string, QueuedSend[]>>({});
  const [holds, setHolds] = useState<Record<string, boolean>>({});
  const queuesRef = useRef(queues);
  queuesRef.current = queues;

  const getSnapshot = useCallback(
    (key: string) =>
      (queuesRef.current[key] ?? []).map((item) => ({
        ...item,
        attachments: item.attachments.map((attachment) => ({ ...attachment })),
      })),
    [],
  );
  const hydrateKey = useCallback(
    (key: string, queue: QueuedSend[], options?: { hold?: boolean }) => {
      const next = setQueueForKey(queuesRef.current, key, queue);
      queuesRef.current = next;
      setQueues(next);
      if (options?.hold !== undefined) {
        setHolds((current) => ({ ...current, [key]: options.hold! }));
      }
    },
    [],
  );
  const migrateDraft = useCallback((newSessionId: string) => {
    const next = migrateDraftQueue(queuesRef.current, newSessionId);
    queuesRef.current = next;
    setQueues(next);
    setHolds((current) => ({
      ...current,
      [COMPOSER_RECOVERY_DRAFT_KEY]: false,
      [newSessionId]: Boolean(current[COMPOSER_RECOVERY_DRAFT_KEY]),
    }));
  }, []);
  const dropKeys = useCallback((keys: Iterable<string>) => {
    let next = queuesRef.current;
    for (const key of keys) next = setQueueForKey(next, key, []);
    queuesRef.current = next;
    setQueues(next);
  }, []);

  const recovery = useComposerRecovery({
    enabled: true,
    recoveryKey,
    setRecoveryKey,
    draft,
    attachments,
    activeQueue: queues[recoveryKey] ?? [],
    setDraft,
    setAttachments,
    memoryPack,
    setMemoryPack,
    queue: { getSnapshot, hydrateKey, migrateDraft, dropKeys },
    debounceMs: 25,
  });

  return {
    recovery,
    recoveryKey,
    draft,
    attachments,
    queues,
    holds,
    setDraft,
    hydrateKey,
    memoryPack,
    setMemoryPack,
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
}

describe("useComposerRecovery", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    recoveryHost.get.mockReset().mockResolvedValue(null);
    recoveryHost.put.mockReset().mockImplementation(
      async (key: string, _state: ComposerRecoveryStateV1, revision: number) => ({
        version: 1,
        key,
        revision: revision + 1,
      }),
    );
    recoveryHost.migrate.mockReset().mockImplementation(
      async (toKey: string, fromRevision: number, toRevision: number) => ({
        version: 1,
        fromKey: COMPOSER_RECOVERY_DRAFT_KEY,
        fromRevision: fromRevision + 1,
        toKey,
        toRevision: toRevision + 1,
      }),
    );
    recoveryHost.delete.mockReset().mockImplementation(
      async (key: string, revision: number) => ({
        version: 1,
        key,
        revision: revision + 1,
      }),
    );
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("restores a draft, attachment references, and held queue after reload", async () => {
    const restored = state(
      "restored draft",
      [queued("q1")],
      [{ path: "/tmp/report.pdf", name: "report.pdf", isDir: false }],
    );
    recoveryHost.get.mockResolvedValueOnce(
      snapshot(COMPOSER_RECOVERY_DRAFT_KEY, 4, restored),
    );
    const { result } = renderHook(() => useHarness());

    await act(async () => {
      await result.current.recovery.activate(COMPOSER_RECOVERY_DRAFT_KEY);
    });

    expect(result.current.draft).toBe("restored draft");
    expect(result.current.attachments).toEqual(restored.attachments);
    expect(result.current.queues[COMPOSER_RECOVERY_DRAFT_KEY]).toEqual(
      restored.queue,
    );
    expect(result.current.holds[COMPOSER_RECOVERY_DRAFT_KEY]).toBe(true);
    expect(result.current.recovery.ready).toBe(true);
    expect(recoveryHost.put).not.toHaveBeenCalled();
  });

  it("restores a reviewed Memory pack identity without dropping the draft", async () => {
    const pack = {
      version: 1 as const,
      selections: [
        {
          id: "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee",
          expectedContentHash: "ab".repeat(32),
        },
      ],
    };
    recoveryHost.get.mockResolvedValueOnce(
      snapshot("session-memory", 2, {
        ...state("keep draft"),
        memoryPack: pack,
      }),
    );
    const { result } = renderHook(() => useHarness());

    await act(async () => {
      await result.current.recovery.activate("session-memory");
    });

    expect(result.current.draft).toBe("keep draft");
    expect(result.current.memoryPack).toEqual(pack);
  });

  it("does not let a stale session get overwrite a later switch", async () => {
    const first = deferred<ComposerRecoverySnapshotV1 | null>();
    const second = deferred<ComposerRecoverySnapshotV1 | null>();
    recoveryHost.get
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);
    const { result } = renderHook(() => useHarness());

    let firstActivation!: Promise<void>;
    let secondActivation!: Promise<void>;
    act(() => {
      firstActivation = result.current.recovery.activate("session-1");
    });
    act(() => {
      secondActivation = result.current.recovery.activate("session-2");
    });
    await act(async () => {
      second.resolve(snapshot("session-2", 2, state("second")));
      await secondActivation;
    });
    await act(async () => {
      first.resolve(snapshot("session-1", 3, state("stale first")));
      await firstActivation;
    });

    expect(result.current.recoveryKey).toBe("session-2");
    expect(result.current.draft).toBe("second");
  });

  it("bounds clean per-session memory while retaining Host-backed restoration", async () => {
    recoveryHost.get.mockImplementation(async (key: string) =>
      snapshot(key, 1, state(`draft ${key}`)),
    );
    const { result } = renderHook(() => useHarness());

    for (let index = 0; index <= COMPOSER_RECOVERY_MEMORY_CACHE_MAX; index += 1) {
      await act(async () => {
        await result.current.recovery.activate(`session-${index}`);
      });
    }
    await act(async () => {
      await result.current.recovery.activate("session-0");
    });

    expect(
      recoveryHost.get.mock.calls.filter(([key]) => key === "session-0"),
    ).toHaveLength(2);
    expect(result.current.draft).toBe("draft session-0");
  });

  it("keeps edits made while a recovery read fails", async () => {
    const pending = deferred<ComposerRecoverySnapshotV1 | null>();
    recoveryHost.get.mockReturnValueOnce(pending.promise);
    const { result } = renderHook(() => useHarness());
    let activation!: Promise<void>;

    act(() => {
      activation = result.current.recovery.activate("session-read-fail");
    });
    act(() => result.current.setDraft("typed during reload"));
    await act(async () => {
      pending.reject(new Error("recovery read failed"));
      await activation;
    });

    expect(result.current.recoveryKey).toBe("session-read-fail");
    expect(result.current.draft).toBe("typed during reload");
    expect(result.current.recovery.ready).toBe(true);
  });

  it("keeps dirty memory after a failed debounce and writes the newer edit", async () => {
    recoveryHost.get.mockResolvedValueOnce(
      snapshot(COMPOSER_RECOVERY_DRAFT_KEY, 3, state("remote")),
    );
    recoveryHost.put
      .mockRejectedValueOnce(new Error("disk unavailable"))
      .mockResolvedValueOnce({
        version: 1,
        key: COMPOSER_RECOVERY_DRAFT_KEY,
        revision: 4,
      });
    const { result } = renderHook(() => useHarness());
    await act(async () => {
      await result.current.recovery.activate(COMPOSER_RECOVERY_DRAFT_KEY);
    });

    act(() => result.current.setDraft("local survives"));
    await act(async () => vi.advanceTimersByTimeAsync(25));
    expect(result.current.draft).toBe("local survives");

    act(() => result.current.setDraft("newer edit"));
    await act(async () => vi.advanceTimersByTimeAsync(25));

    expect(result.current.draft).toBe("newer edit");
    expect(recoveryHost.put).toHaveBeenCalledTimes(2);
    expect(recoveryHost.put.mock.calls[1]?.[1]).toMatchObject({
      draft: "newer edit",
    });
    expect(recoveryHost.put.mock.calls[1]?.[2]).toBe(3);
  });

  it("rebases an ordinary dirty CAS retry without replacing local state", async () => {
    recoveryHost.get
      .mockResolvedValueOnce(
        snapshot(COMPOSER_RECOVERY_DRAFT_KEY, 2, state("initial")),
      )
      .mockResolvedValueOnce(
        snapshot(COMPOSER_RECOVERY_DRAFT_KEY, 7, state("remote winner")),
      );
    recoveryHost.put
      .mockRejectedValueOnce(new Error("COMPOSER_RECOVERY_STALE"))
      .mockResolvedValueOnce({
        version: 1,
        key: COMPOSER_RECOVERY_DRAFT_KEY,
        revision: 8,
      });
    const { result } = renderHook(() => useHarness());
    await act(async () => {
      await result.current.recovery.activate(COMPOSER_RECOVERY_DRAFT_KEY);
    });
    act(() => result.current.setDraft("local winner"));
    await act(async () => vi.advanceTimersByTimeAsync(25));

    expect(recoveryHost.put.mock.calls.map((call) => call[2])).toEqual([2, 7]);
    expect(result.current.draft).toBe("local winner");
  });

  it("atomically migrates draft cache and queue onto a materialized session", async () => {
    recoveryHost.get
      .mockResolvedValueOnce(
        snapshot(
          COMPOSER_RECOVERY_DRAFT_KEY,
          5,
          state("draft", [queued("q1")]),
        ),
      )
      .mockResolvedValueOnce(null);
    const { result } = renderHook(() => useHarness());
    await act(async () => {
      await result.current.recovery.activate(COMPOSER_RECOVERY_DRAFT_KEY);
    });

    let migrated = false;
    await act(async () => {
      migrated = await result.current.recovery.migrateDraft("session-real");
    });

    expect(migrated).toBe(true);
    expect(recoveryHost.migrate).toHaveBeenCalledWith("session-real", 5, 0);
    expect(result.current.recoveryKey).toBe("session-real");
    expect(result.current.queues[COMPOSER_RECOVERY_DRAFT_KEY]).toBeUndefined();
    expect(result.current.queues["session-real"]?.map((item) => item.id)).toEqual([
      "q1",
    ]);
  });

  it("fails a conflicting dual-CAS migration closed and keeps the draft", async () => {
    recoveryHost.get
      .mockResolvedValueOnce(
        snapshot(COMPOSER_RECOVERY_DRAFT_KEY, 5, state("keep me")),
      )
      .mockResolvedValueOnce(null);
    recoveryHost.migrate.mockRejectedValueOnce(
      new Error("COMPOSER_RECOVERY_MIGRATION_CONFLICT"),
    );
    const { result } = renderHook(() => useHarness());
    await act(async () => {
      await result.current.recovery.activate(COMPOSER_RECOVERY_DRAFT_KEY);
    });

    let migrated = true;
    await act(async () => {
      migrated = await result.current.recovery.migrateDraft("session-conflict");
    });

    expect(migrated).toBe(false);
    expect(result.current.recoveryKey).toBe(COMPOSER_RECOVERY_DRAFT_KEY);
    expect(result.current.draft).toBe("keep me");
  });

  it("fails a queue CAS closed without fetching and overwriting remote state", async () => {
    recoveryHost.get.mockResolvedValueOnce(
      snapshot("session-1", 9, state("", [queued("q1")])),
    );
    recoveryHost.put.mockRejectedValueOnce(
      new Error("COMPOSER_RECOVERY_STALE"),
    );
    const { result } = renderHook(() => useHarness());
    await act(async () => {
      await result.current.recovery.activate("session-1");
    });

    let persisted = true;
    await act(async () => {
      persisted = await result.current.recovery.persistQueuedState(
        "session-1",
        [],
      );
    });

    expect(persisted).toBe(false);
    expect(recoveryHost.get).toHaveBeenCalledTimes(1);
    expect(recoveryHost.put).toHaveBeenCalledWith(
      "session-1",
      expect.objectContaining({ queue: [] }),
      9,
    );
  });

  it("deletes recovery before dropping a session queue", async () => {
    recoveryHost.get.mockResolvedValueOnce(
      snapshot("session-delete", 6, state("discard", [queued("q-delete")])),
    );
    const { result } = renderHook(() => useHarness());
    await act(async () => {
      await result.current.recovery.activate("session-delete");
    });

    let deleted = false;
    await act(async () => {
      deleted = await result.current.recovery.deleteKey("session-delete");
    });

    expect(deleted).toBe(true);
    expect(recoveryHost.delete).toHaveBeenCalledWith("session-delete", 6);
    expect(result.current.queues["session-delete"]).toBeUndefined();
  });

  it("refreshes revision once when a pre-session-delete CAS is stale", async () => {
    recoveryHost.get
      .mockResolvedValueOnce(snapshot("session-stale-delete", 2, state("old")))
      .mockResolvedValueOnce(snapshot("session-stale-delete", 8, state("new")));
    recoveryHost.delete
      .mockRejectedValueOnce(new Error("COMPOSER_RECOVERY_STALE"))
      .mockResolvedValueOnce({
        version: 1,
        key: "session-stale-delete",
        revision: 9,
      });
    const { result } = renderHook(() => useHarness());
    await act(async () => {
      await result.current.recovery.activate("session-stale-delete");
    });

    let deleted = false;
    await act(async () => {
      deleted = await result.current.recovery.deleteKey("session-stale-delete");
    });

    expect(deleted).toBe(true);
    expect(recoveryHost.delete.mock.calls.map((call) => call[1])).toEqual([2, 8]);
  });

  it("explicitly resets the draft through delete then put with the new revision", async () => {
    recoveryHost.get.mockResolvedValueOnce(
      snapshot(COMPOSER_RECOVERY_DRAFT_KEY, 3, state("old draft")),
    );
    const { result } = renderHook(() => useHarness());
    await act(async () => {
      await result.current.recovery.activate(COMPOSER_RECOVERY_DRAFT_KEY);
    });

    let reset = false;
    await act(async () => {
      reset = await result.current.recovery.resetDraft("seed draft");
    });

    expect(reset).toBe(true);
    expect(recoveryHost.delete).toHaveBeenCalledWith(
      COMPOSER_RECOVERY_DRAFT_KEY,
      3,
    );
    expect(recoveryHost.put).toHaveBeenCalledWith(
      COMPOSER_RECOVERY_DRAFT_KEY,
      expect.objectContaining({ draft: "seed draft", attachments: [], queue: [] }),
      4,
    );
    expect(result.current.draft).toBe("seed draft");
  });

  it("retries an explicit draft reset delete once with the latest revision", async () => {
    recoveryHost.get
      .mockResolvedValueOnce(
        snapshot(COMPOSER_RECOVERY_DRAFT_KEY, 3, state("old draft")),
      )
      .mockResolvedValueOnce(
        snapshot(COMPOSER_RECOVERY_DRAFT_KEY, 7, state("stale writer")),
      );
    recoveryHost.delete
      .mockRejectedValueOnce(new Error("COMPOSER_RECOVERY_STALE"))
      .mockResolvedValueOnce({
        version: 1,
        key: COMPOSER_RECOVERY_DRAFT_KEY,
        revision: 8,
      });
    const { result } = renderHook(() => useHarness());
    await act(async () => {
      await result.current.recovery.activate(COMPOSER_RECOVERY_DRAFT_KEY);
    });

    await act(async () => {
      await result.current.recovery.resetDraft("fresh draft");
    });

    expect(recoveryHost.delete.mock.calls.map((call) => call[1])).toEqual([3, 7]);
    expect(recoveryHost.put).toHaveBeenCalledWith(
      COMPOSER_RECOVERY_DRAFT_KEY,
      expect.objectContaining({ draft: "fresh draft" }),
      8,
    );
  });
});
