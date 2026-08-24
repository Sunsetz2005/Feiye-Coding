import type { Attachment } from "@/lib/attachments";
import { isTauri } from "@/lib/api";
import {
  SEND_QUEUE_MAX,
  type QueuedSend,
} from "@/lib/sendQueue";

export const COMPOSER_RECOVERY_VERSION = 1 as const;
export const COMPOSER_RECOVERY_DRAFT_KEY = "__draft__" as const;

export const COMPOSER_RECOVERY_COMMANDS = {
  get: "composer_recovery_get_v1",
  put: "composer_recovery_put_v1",
  migrate: "composer_recovery_migrate_v1",
  delete: "composer_recovery_delete_v1",
} as const;

/** Hard limits for data accepted from the recovery journal. */
export const COMPOSER_RECOVERY_LIMITS = {
  keyChars: 512,
  draftChars: 1_000_000,
  attachmentPathChars: 4_096,
  attachmentNameChars: 1_024,
  attachmentsPerSend: 128,
  totalAttachments: 1_024,
  queueItems: SEND_QUEUE_MAX,
  queueIdChars: 256,
  queuedDisplayChars: 1_000_000,
  totalStateChars: 4_000_000,
} as const;

export type ComposerRecoveryRevisionV1 = number;

/**
 * Persisted paths are untrusted references only. Restoring this state does not
 * authorize a path; callers must run the existing classify/authorize path
 * before a recovered attachment is sent.
 */
export interface ComposerRecoveryStateV1 {
  draft: string;
  attachments: Attachment[];
  queue: QueuedSend[];
}

export interface ComposerRecoverySnapshotV1 {
  version: typeof COMPOSER_RECOVERY_VERSION;
  key: string;
  revision: ComposerRecoveryRevisionV1;
  state: ComposerRecoveryStateV1;
  filteredAttachmentCount: number;
  filteredQueueItemCount: number;
}

export interface ComposerRecoveryMutationResultV1 {
  version: typeof COMPOSER_RECOVERY_VERSION;
  key: string;
  revision: ComposerRecoveryRevisionV1;
}

export interface ComposerRecoveryMigrateResultV1 {
  version: typeof COMPOSER_RECOVERY_VERSION;
  fromKey: typeof COMPOSER_RECOVERY_DRAFT_KEY;
  fromRevision: ComposerRecoveryRevisionV1;
  toKey: string;
  toRevision: ComposerRecoveryRevisionV1;
}

export interface ComposerRecoveryGetInvokeArgsV1 {
  request: {
    version: typeof COMPOSER_RECOVERY_VERSION;
    key: string;
  };
}

export interface ComposerRecoveryPutInvokeArgsV1 {
  request: {
    version: typeof COMPOSER_RECOVERY_VERSION;
    key: string;
    expectedRevision: ComposerRecoveryRevisionV1;
    state: ComposerRecoveryStateV1;
  };
}

export interface ComposerRecoveryMigrateInvokeArgsV1 {
  request: {
    version: typeof COMPOSER_RECOVERY_VERSION;
    fromKey: typeof COMPOSER_RECOVERY_DRAFT_KEY;
    toKey: string;
    expectedFromRevision: ComposerRecoveryRevisionV1;
    expectedToRevision: ComposerRecoveryRevisionV1;
  };
}

export interface ComposerRecoveryDeleteInvokeArgsV1 {
  request: {
    version: typeof COMPOSER_RECOVERY_VERSION;
    key: string;
    expectedRevision: ComposerRecoveryRevisionV1;
  };
}

type UnknownRecord = Record<string, unknown>;

function isRecord(value: unknown): value is UnknownRecord {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function hasControlCharacters(value: string): boolean {
  return /[\u0000-\u001f\u007f]/.test(value);
}

function isBoundedString(value: unknown, max: number): value is string {
  return typeof value === "string" && value.length <= max;
}

function normalizeRevision(value: unknown): ComposerRecoveryRevisionV1 | null {
  return typeof value === "number" &&
    Number.isSafeInteger(value) &&
    value >= 0
    ? value
    : null;
}

function normalizeCount(value: unknown): number | null {
  return normalizeRevision(value);
}

export function normalizeComposerRecoveryKey(value: unknown): string | null {
  if (!isBoundedString(value, COMPOSER_RECOVERY_LIMITS.keyChars)) return null;
  if (!value || value.trim() !== value || hasControlCharacters(value)) {
    return null;
  }
  return value;
}

function isAbsoluteRecoveryPath(value: string): boolean {
  if (value.startsWith("/")) return true;
  if (/^[A-Za-z]:[\\/]/.test(value)) return true;
  return /^\\\\[^\\/]+[\\/][^\\/]+/.test(value);
}

function normalizeRecoveryPath(value: unknown): string | null {
  if (
    !isBoundedString(value, COMPOSER_RECOVERY_LIMITS.attachmentPathChars) ||
    !value ||
    hasControlCharacters(value) ||
    !isAbsoluteRecoveryPath(value)
  ) {
    return null;
  }
  return value.replace(/\\/g, "/").split("/").includes("..") ? null : value;
}

function normalizeAttachment(value: unknown): Attachment | null {
  if (!isRecord(value)) return null;
  const path = normalizeRecoveryPath(value.path);
  if (
    !path ||
    !isBoundedString(
      value.name,
      COMPOSER_RECOVERY_LIMITS.attachmentNameChars,
    ) ||
    !value.name ||
    hasControlCharacters(value.name) ||
    typeof value.isDir !== "boolean"
  ) {
    return null;
  }
  // Only plain Attachment fields cross this boundary. Persisted handles or
  // authorization flags are data, never capabilities.
  return { path, name: value.name, isDir: value.isDir };
}

function normalizeAttachments(value: unknown): Attachment[] | null {
  if (
    !Array.isArray(value) ||
    value.length > COMPOSER_RECOVERY_LIMITS.attachmentsPerSend
  ) {
    return null;
  }
  const attachments: Attachment[] = [];
  for (const candidate of value) {
    const attachment = normalizeAttachment(candidate);
    if (!attachment) return null;
    attachments.push(attachment);
  }
  return attachments;
}

function normalizeQueue(value: unknown): QueuedSend[] | null {
  if (
    !Array.isArray(value) ||
    value.length > COMPOSER_RECOVERY_LIMITS.queueItems
  ) {
    return null;
  }
  const ids = new Set<string>();
  const queue: QueuedSend[] = [];
  for (const candidate of value) {
    if (!isRecord(candidate)) return null;
    if (
      !isBoundedString(candidate.id, COMPOSER_RECOVERY_LIMITS.queueIdChars) ||
      !candidate.id ||
      hasControlCharacters(candidate.id) ||
      ids.has(candidate.id) ||
      !isBoundedString(
        candidate.storedDisplay,
        COMPOSER_RECOVERY_LIMITS.queuedDisplayChars,
      ) ||
      typeof candidate.goalMode !== "boolean" ||
      normalizeCount(candidate.createdAt) === null
    ) {
      return null;
    }
    const attachments = normalizeAttachments(candidate.attachments);
    if (!attachments) return null;
    ids.add(candidate.id);
    queue.push({
      id: candidate.id,
      storedDisplay: candidate.storedDisplay,
      attachments,
      goalMode: candidate.goalMode,
      createdAt: candidate.createdAt as number,
    });
  }
  return queue;
}

/** Validate untrusted journal state and return a deep, field-filtered copy. */
export function normalizeComposerRecoveryStateV1(
  value: unknown,
): ComposerRecoveryStateV1 | null {
  if (!isRecord(value)) return null;
  if (!isBoundedString(value.draft, COMPOSER_RECOVERY_LIMITS.draftChars)) {
    return null;
  }
  const attachments = normalizeAttachments(value.attachments);
  const queue = normalizeQueue(value.queue);
  if (!attachments || !queue) return null;

  const queuedAttachments = queue.reduce(
    (total, item) => total + item.attachments.length,
    0,
  );
  if (
    attachments.length + queuedAttachments >
    COMPOSER_RECOVERY_LIMITS.totalAttachments
  ) {
    return null;
  }
  const totalChars =
    value.draft.length +
    attachments.reduce((n, item) => n + item.path.length + item.name.length, 0) +
    queue.reduce(
      (n, item) =>
        n +
        item.id.length +
        item.storedDisplay.length +
        item.attachments.reduce(
          (sum, attachment) =>
            sum + attachment.path.length + attachment.name.length,
          0,
        ),
      0,
    );
  if (totalChars > COMPOSER_RECOVERY_LIMITS.totalStateChars) return null;

  return { draft: value.draft, attachments, queue };
}

export function cloneComposerRecoveryStateV1(
  state: ComposerRecoveryStateV1,
): ComposerRecoveryStateV1 {
  return {
    draft: state.draft,
    attachments: state.attachments.map((attachment) => ({ ...attachment })),
    queue: state.queue.map((item) => ({
      ...item,
      attachments: item.attachments.map((attachment) => ({ ...attachment })),
    })),
  };
}

export function normalizeComposerRecoverySnapshotV1(
  value: unknown,
  expectedKey?: string,
): ComposerRecoverySnapshotV1 | null {
  if (!isRecord(value) || value.version !== COMPOSER_RECOVERY_VERSION) {
    return null;
  }
  const key = normalizeComposerRecoveryKey(value.key);
  const revision = normalizeRevision(value.revision);
  const state = normalizeComposerRecoveryStateV1(value.state);
  const filteredAttachmentCount = normalizeCount(value.filteredAttachmentCount);
  const filteredQueueItemCount = normalizeCount(value.filteredQueueItemCount);
  if (
    !key ||
    revision === null ||
    !state ||
    filteredAttachmentCount === null ||
    filteredQueueItemCount === null ||
    (expectedKey !== undefined && key !== expectedKey)
  ) {
    return null;
  }
  return {
    version: COMPOSER_RECOVERY_VERSION,
    key,
    revision,
    state,
    filteredAttachmentCount,
    filteredQueueItemCount,
  };
}

function normalizeMutationResult(
  value: unknown,
  expectedKey: string,
): ComposerRecoveryMutationResultV1 | null {
  if (!isRecord(value) || value.version !== COMPOSER_RECOVERY_VERSION) {
    return null;
  }
  const key = normalizeComposerRecoveryKey(value.key);
  const revision = normalizeRevision(value.revision);
  if (!key || key !== expectedKey || revision === null) return null;
  return { version: COMPOSER_RECOVERY_VERSION, key, revision };
}

function normalizeMigrateResult(
  value: unknown,
  expectedToKey: string,
): ComposerRecoveryMigrateResultV1 | null {
  if (!isRecord(value) || value.version !== COMPOSER_RECOVERY_VERSION) {
    return null;
  }
  const fromRevision = normalizeRevision(value.fromRevision);
  const toKey = normalizeComposerRecoveryKey(value.toKey);
  const toRevision = normalizeRevision(value.toRevision);
  if (
    value.fromKey !== COMPOSER_RECOVERY_DRAFT_KEY ||
    fromRevision === null ||
    !toKey ||
    toKey !== expectedToKey ||
    toRevision === null
  ) {
    return null;
  }
  return {
    version: COMPOSER_RECOVERY_VERSION,
    fromKey: COMPOSER_RECOVERY_DRAFT_KEY,
    fromRevision,
    toKey,
    toRevision,
  };
}

function requireKey(value: unknown): string {
  const key = normalizeComposerRecoveryKey(value);
  if (!key) throw new TypeError("Invalid composer recovery key");
  return key;
}

function requireExpectedRevision(
  value: ComposerRecoveryRevisionV1,
): ComposerRecoveryRevisionV1 {
  const revision = normalizeRevision(value);
  if (revision === null) {
    throw new TypeError("Invalid composer recovery revision");
  }
  return revision;
}

async function hostInvoke<T>(
  command: string,
  args: object,
): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(command, args as Record<string, unknown>);
}

/** Host wire mapping is deliberately confined to this function. */
export async function composerRecoveryGetV1(
  recoveryKey: string,
): Promise<ComposerRecoverySnapshotV1 | null> {
  if (!isTauri()) return null;
  const key = requireKey(recoveryKey);
  const args: ComposerRecoveryGetInvokeArgsV1 = {
    request: {
      version: COMPOSER_RECOVERY_VERSION,
      key,
    },
  };
  const raw = await hostInvoke<unknown>(COMPOSER_RECOVERY_COMMANDS.get, args);
  if (raw === null) return null;
  const snapshot = normalizeComposerRecoverySnapshotV1(raw, key);
  if (!snapshot) throw new Error("Invalid composer recovery get response");
  return snapshot;
}

/** Host wire mapping is deliberately confined to this function. */
export async function composerRecoveryPutV1(
  recoveryKey: string,
  state: ComposerRecoveryStateV1,
  expectedRevision: ComposerRecoveryRevisionV1,
): Promise<ComposerRecoveryMutationResultV1 | null> {
  if (!isTauri()) return null;
  const key = requireKey(recoveryKey);
  const normalizedState = normalizeComposerRecoveryStateV1(state);
  if (!normalizedState) throw new TypeError("Invalid composer recovery state");
  const args: ComposerRecoveryPutInvokeArgsV1 = {
    request: {
      version: COMPOSER_RECOVERY_VERSION,
      key,
      expectedRevision: requireExpectedRevision(expectedRevision),
      state: normalizedState,
    },
  };
  const raw = await hostInvoke<unknown>(COMPOSER_RECOVERY_COMMANDS.put, args);
  const result = normalizeMutationResult(raw, key);
  if (!result) throw new Error("Invalid composer recovery put response");
  return result;
}

/** Atomically move `__draft__` state onto a newly materialized session. */
export async function composerRecoveryMigrateV1(
  toRecoveryKey: string,
  expectedFromRevision: ComposerRecoveryRevisionV1,
  expectedToRevision: ComposerRecoveryRevisionV1,
): Promise<ComposerRecoveryMigrateResultV1 | null> {
  if (!isTauri()) return null;
  const toKey = requireKey(toRecoveryKey);
  if (toKey === COMPOSER_RECOVERY_DRAFT_KEY) {
    throw new TypeError("Composer recovery target must be a real session key");
  }
  const args: ComposerRecoveryMigrateInvokeArgsV1 = {
    request: {
      version: COMPOSER_RECOVERY_VERSION,
      fromKey: COMPOSER_RECOVERY_DRAFT_KEY,
      toKey,
      expectedFromRevision: requireExpectedRevision(expectedFromRevision),
      expectedToRevision: requireExpectedRevision(expectedToRevision),
    },
  };
  const raw = await hostInvoke<unknown>(COMPOSER_RECOVERY_COMMANDS.migrate, args);
  const result = normalizeMigrateResult(raw, toKey);
  if (!result) throw new Error("Invalid composer recovery migrate response");
  return result;
}

/** Host wire mapping is deliberately confined to this function. */
export async function composerRecoveryDeleteV1(
  recoveryKey: string,
  expectedRevision: ComposerRecoveryRevisionV1,
): Promise<ComposerRecoveryMutationResultV1 | null> {
  if (!isTauri()) return null;
  const key = requireKey(recoveryKey);
  const args: ComposerRecoveryDeleteInvokeArgsV1 = {
    request: {
      version: COMPOSER_RECOVERY_VERSION,
      key,
      expectedRevision: requireExpectedRevision(expectedRevision),
    },
  };
  const raw = await hostInvoke<unknown>(COMPOSER_RECOVERY_COMMANDS.delete, args);
  const result = normalizeMutationResult(raw, key);
  if (!result) throw new Error("Invalid composer recovery delete response");
  return result;
}
