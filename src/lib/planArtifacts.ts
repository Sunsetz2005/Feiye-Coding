/** Versioned Plan artifact sidecar. Independent of InteractionSnapshotV1 RPC status. */

export type PlanArtifactStatusV1 =
  | "proposed"
  | "approved"
  | "revision_requested"
  | "executing"
  | "completed"
  | "abandoned"
  | "interrupted";

export interface PlanArtifactRevisionV1 {
  revision: number;
  contentHash: string;
  body?: string | null;
  entries: unknown;
  createdAt: string;
}

export interface PlanArtifactTransitionV1 {
  status: PlanArtifactStatusV1;
  occurredAt: string;
  feedback?: string | null;
}

export interface PlanArtifactV1 {
  version: number;
  id: string;
  sessionId: string;
  processId: string;
  interactionId?: string | null;
  toolCallId?: string | null;
  status: PlanArtifactStatusV1;
  currentRevision: number;
  revisions: PlanArtifactRevisionV1[];
  transitions: PlanArtifactTransitionV1[];
  createdAt: string;
  updatedAt: string;
}

export function currentPlanRevision(
  artifact: PlanArtifactV1,
): PlanArtifactRevisionV1 | null {
  return (
    artifact.revisions.find((row) => row.revision === artifact.currentRevision) ??
    artifact.revisions[artifact.revisions.length - 1] ??
    null
  );
}

/**
 * Pick the artifact the workbench should restore.
 * Live proposed reviews outrank older completed rows; abandoned rows never display.
 */
export function selectDisplayPlanArtifact(
  artifacts: readonly PlanArtifactV1[],
): PlanArtifactV1 | null {
  const visible = artifacts.filter(
    (artifact) => artifact.status !== "abandoned",
  );
  if (!visible.length) return null;
  const proposed = visible.filter((artifact) => artifact.status === "proposed");
  const pool = proposed.length ? proposed : visible;
  return (
    [...pool].sort((left, right) => {
      const byTime = right.updatedAt.localeCompare(left.updatedAt);
      return byTime !== 0 ? byTime : right.id.localeCompare(left.id);
    })[0] ?? null
  );
}

/** Product copy uses done for schema `completed`. */
export function planArtifactProductStatus(
  status: PlanArtifactStatusV1 | null | undefined,
): "approved" | "executing" | "done" | null {
  if (status === "approved") return "approved";
  if (status === "executing") return "executing";
  if (status === "completed") return "done";
  return null;
}
