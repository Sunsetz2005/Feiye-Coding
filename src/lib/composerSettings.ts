import { isSessionBusy, type SessionState } from "./session";

/** Runtime-owned states in which composer preferences are inspectable only. */
export function shouldLockComposerSettings(input: {
  state: SessionState;
  hasPermissionPrompt: boolean;
  hasAskUserPrompt: boolean;
  hasPlanReview: boolean;
}): boolean {
  return (
    isSessionBusy(input.state) ||
    input.hasPermissionPrompt ||
    input.hasAskUserPrompt ||
    input.hasPlanReview
  );
}

/** Avoid clobbering a newer choice when an older optimistic write fails. */
export function rollbackOptimisticSetting<T>(
  current: T,
  attempted: T,
  previous: T,
): T {
  return Object.is(current, attempted) ? previous : current;
}

/** + menu actions that mutate Runtime/session preferences. */
export function isComposerSettingAction(action: string): boolean {
  return (
    action === "project" ||
    action === "goal" ||
    action === "plan" ||
    action === "record-skill"
  );
}
