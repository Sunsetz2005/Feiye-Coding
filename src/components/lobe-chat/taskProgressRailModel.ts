import { parsePlanEntries } from "@/lib/planStatus";
import { normalizePath } from "@/lib/sessionChanges";

export interface TaskProgressChange {
  path: string;
  /** Authoritative line stats only; omit when the backend did not provide them. */
  additions?: number | null;
  deletions?: number | null;
}

export interface TaskProgressChangeTotals {
  additions?: number | null;
  deletions?: number | null;
}

export interface TaskProgressMetrics {
  hasSteps: boolean;
  currentStep: number | null;
  totalSteps: number;
  changedFiles: number | null;
  additions: number | null;
  deletions: number | null;
}

function knownNonNegativeInt(value: number | null | undefined): number | null {
  if (value == null || !Number.isFinite(value) || value < 0) return null;
  return Math.floor(value);
}

function currentPlanStep(entries: ReturnType<typeof parsePlanEntries>): number {
  const active = entries.findIndex((entry) => entry.status === "in_progress");
  if (active >= 0) return active + 1;
  const next = entries.findIndex(
    (entry) => entry.status === "pending" || entry.status === "unknown",
  );
  if (next >= 0) return next + 1;
  return entries.length;
}

function uniqueChanges(
  changes: readonly TaskProgressChange[],
): TaskProgressChange[] {
  const byPath = new Map<string, TaskProgressChange>();
  changes.forEach((change) => {
    const path = normalizePath(change.path);
    if (!path) return;
    // Session change collections are newest-first; preserve the first
    // authoritative record if a less-normalized caller supplies duplicates.
    if (!byPath.has(path)) byPath.set(path, change);
  });
  return [...byPath.values()];
}

function completePerFileTotal(
  changes: readonly TaskProgressChange[],
  field: "additions" | "deletions",
): number | null {
  if (!changes.length) return null;
  let total = 0;
  for (const change of changes) {
    const value = knownNonNegativeInt(change[field]);
    if (value == null) return null;
    total += value;
  }
  return total;
}

/**
 * Build progress facts from persisted plan/change data.
 * Missing line stats stay missing; partial totals are never extrapolated.
 */
export function buildTaskProgressMetrics(
  rawEntries: unknown,
  rawChanges: readonly TaskProgressChange[],
  authoritativeTotals?: TaskProgressChangeTotals | null,
): TaskProgressMetrics {
  const entries = parsePlanEntries(rawEntries);
  const changes = uniqueChanges(rawChanges);
  const providedAdds = knownNonNegativeInt(authoritativeTotals?.additions);
  const providedDeletes = knownNonNegativeInt(authoritativeTotals?.deletions);

  return {
    hasSteps: entries.length > 0,
    currentStep: entries.length ? currentPlanStep(entries) : null,
    totalSteps: entries.length,
    changedFiles: changes.length ? changes.length : null,
    additions:
      providedAdds ??
      completePerFileTotal(changes, "additions"),
    deletions:
      providedDeletes ??
      completePerFileTotal(changes, "deletions"),
  };
}

export function formatTaskElapsed(elapsedMs: number | null | undefined): string {
  if (
    elapsedMs == null ||
    !Number.isFinite(elapsedMs) ||
    elapsedMs < 0
  ) {
    return "";
  }
  const totalSeconds = Math.floor(elapsedMs / 1000);
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  if (hours > 0) return `${hours}h ${minutes}m ${seconds}s`;
  if (minutes > 0) return `${minutes}m ${seconds}s`;
  return `${seconds}s`;
}
