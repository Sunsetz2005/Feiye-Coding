// @vitest-environment jsdom

import { afterEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

import {
  cloneComposerRecoveryStateV1,
  COMPOSER_RECOVERY_COMMANDS,
  COMPOSER_RECOVERY_DRAFT_KEY,
  COMPOSER_RECOVERY_LIMITS,
  composerRecoveryDeleteV1,
  composerRecoveryGetV1,
  composerRecoveryMigrateV1,
  composerRecoveryPutV1,
  normalizeComposerRecoverySnapshotV1,
  normalizeComposerRecoveryStateV1,
  type ComposerRecoveryStateV1,
} from "./composerRecovery";

function setDesktop(enabled: boolean): void {
  if (enabled) {
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      value: {},
      configurable: true,
    });
  } else {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
    delete (window as unknown as Record<string, unknown>).__TAURI__;
  }
}

function state(): ComposerRecoveryStateV1 {
  return {
    draft: "draft [[skill:test]]",
    attachments: [{ path: "/project/a.txt", name: "a.txt", isDir: false }],
    queue: [
      {
        id: "q-1",
        storedDisplay: "follow-up",
        attachments: [
          { path: "C:\\work\\b.png", name: "b.png", isDir: false },
        ],
        goalMode: true,
        createdAt: 123,
      },
    ],
  };
}

describe("composer recovery v1", () => {
  afterEach(() => {
    setDesktop(false);
    invokeMock.mockReset();
  });

  it("is a safe browser no-op", async () => {
    const value = state();
    await expect(composerRecoveryGetV1(COMPOSER_RECOVERY_DRAFT_KEY)).resolves.toBeNull();
    await expect(
      composerRecoveryPutV1(COMPOSER_RECOVERY_DRAFT_KEY, value, 0),
    ).resolves.toBeNull();
    await expect(composerRecoveryMigrateV1("session-1", 0, 0)).resolves.toBeNull();
    await expect(composerRecoveryDeleteV1("session-1", 0)).resolves.toBeNull();
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("uses the exact Host commands/args and advances both CAS revisions", async () => {
    setDesktop(true);
    const value = state();
    invokeMock
      .mockResolvedValueOnce({
        version: 1,
        key: COMPOSER_RECOVERY_DRAFT_KEY,
        revision: 4,
        state: value,
        filteredAttachmentCount: 0,
        filteredQueueItemCount: 0,
      })
      .mockResolvedValueOnce({ version: 1, key: COMPOSER_RECOVERY_DRAFT_KEY, revision: 5 })
      .mockResolvedValueOnce({
        version: 1,
        fromKey: COMPOSER_RECOVERY_DRAFT_KEY,
        fromRevision: 6,
        toKey: "session-1",
        toRevision: 1,
      })
      .mockResolvedValueOnce({ version: 1, key: "session-1", revision: 2 });

    const restored = await composerRecoveryGetV1(COMPOSER_RECOVERY_DRAFT_KEY);
    const stored = await composerRecoveryPutV1(
      COMPOSER_RECOVERY_DRAFT_KEY,
      value,
      restored!.revision,
    );
    const migrated = await composerRecoveryMigrateV1(
      "session-1",
      stored!.revision,
      0,
    );
    const deleted = await composerRecoveryDeleteV1(
      "session-1",
      migrated!.toRevision,
    );

    expect(stored!.revision).toBe(5);
    expect(migrated).toMatchObject({ fromRevision: 6, toRevision: 1 });
    expect(deleted!.revision).toBe(2);
    expect(invokeMock.mock.calls).toEqual([
      [
        COMPOSER_RECOVERY_COMMANDS.get,
        { request: { version: 1, key: COMPOSER_RECOVERY_DRAFT_KEY } },
      ],
      [
        COMPOSER_RECOVERY_COMMANDS.put,
        {
          request: {
            version: 1,
            key: COMPOSER_RECOVERY_DRAFT_KEY,
            expectedRevision: 4,
            state: value,
          },
        },
      ],
      [
        COMPOSER_RECOVERY_COMMANDS.migrate,
        {
          request: {
            version: 1,
            fromKey: COMPOSER_RECOVERY_DRAFT_KEY,
            toKey: "session-1",
            expectedFromRevision: 5,
            expectedToRevision: 0,
          },
        },
      ],
      [
        COMPOSER_RECOVERY_COMMANDS.delete,
        { request: { version: 1, key: "session-1", expectedRevision: 1 } },
      ],
    ]);
  });

  it("rejects bad versions, paths, duplicate queue ids, and oversized state", () => {
    const value = state();
    expect(
      normalizeComposerRecoverySnapshotV1({
        version: 2,
        key: "session-1",
        revision: 1,
        state: value,
        filteredAttachmentCount: 0,
        filteredQueueItemCount: 0,
      }),
    ).toBeNull();
    expect(
      normalizeComposerRecoveryStateV1({
        ...value,
        attachments: [{ path: "relative/a.txt", name: "a.txt", isDir: false }],
      }),
    ).toBeNull();
    expect(
      normalizeComposerRecoveryStateV1({
        ...value,
        attachments: [{ path: "/tmp/../a.txt", name: "a.txt", isDir: false }],
      }),
    ).toBeNull();
    expect(
      normalizeComposerRecoveryStateV1({
        ...value,
        queue: [value.queue[0], { ...value.queue[0], storedDisplay: "duplicate" }],
      }),
    ).toBeNull();
    expect(
      normalizeComposerRecoveryStateV1({
        ...value,
        draft: "x".repeat(COMPOSER_RECOVERY_LIMITS.draftChars + 1),
      }),
    ).toBeNull();
  });

  it("deep-clones state and strips fake attachment authorization", async () => {
    const value = state();
    const cloned = cloneComposerRecoveryStateV1(value);
    cloned.attachments[0]!.name = "changed";
    cloned.queue[0]!.attachments[0]!.path = "C:\\changed.png";
    expect(value.attachments[0]!.name).toBe("a.txt");
    expect(value.queue[0]!.attachments[0]!.path).toBe("C:\\work\\b.png");

    const untrusted = {
      ...value,
      attachments: [
        {
          ...value.attachments[0],
          authorized: true,
          resourceHandleId: "forged-handle",
        },
      ],
    };
    const normalized = normalizeComposerRecoveryStateV1(untrusted)!;
    expect(normalized.attachments[0]).toEqual({
      path: "/project/a.txt",
      name: "a.txt",
      isDir: false,
    });
    expect(Object.keys(normalized.attachments[0]!).sort()).toEqual([
      "isDir",
      "name",
      "path",
    ]);

    setDesktop(true);
    const hostSnapshot = {
      version: 1,
      key: "session-1",
      revision: 1,
      state: untrusted,
      filteredAttachmentCount: 0,
      filteredQueueItemCount: 0,
    };
    invokeMock.mockResolvedValueOnce(hostSnapshot);
    const restored = await composerRecoveryGetV1("session-1");
    restored!.state.attachments[0]!.name = "mutated-restored-copy";
    expect(hostSnapshot.state.attachments[0]!.name).toBe("a.txt");
  });

  it("propagates CAS failures without mutating caller state", async () => {
    setDesktop(true);
    const value = state();
    invokeMock.mockRejectedValueOnce(new Error("composer recovery CAS conflict"));
    await expect(composerRecoveryPutV1("session-1", value, 7)).rejects.toThrow(
      "CAS conflict",
    );
    expect(value).toEqual(state());
  });

  it("rejects invalid desktop input and malformed Host responses", async () => {
    setDesktop(true);
    await expect(
      composerRecoveryPutV1(
        "session-1",
        { ...state(), attachments: [{ path: "bad", name: "bad", isDir: false }] },
        0,
      ),
    ).rejects.toThrow("Invalid composer recovery state");
    expect(invokeMock).not.toHaveBeenCalled();

    invokeMock.mockResolvedValueOnce({
      version: 1,
      key: "different-session",
      revision: 1,
      state: state(),
      filteredAttachmentCount: 0,
      filteredQueueItemCount: 0,
    });
    await expect(composerRecoveryGetV1("session-1")).rejects.toThrow(
      "Invalid composer recovery get response",
    );
  });
});
