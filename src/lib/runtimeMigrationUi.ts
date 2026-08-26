import type { PlanArtifactV1 } from "./planArtifacts";
import { currentPlanRevision } from "./planArtifacts";
import type {
  AskUserPayload,
  InteractionSnapshotV1,
  PermissionPayload,
} from "./session";
import type { PlanReviewState } from "./planBody";
import type {
  AutomationClaimV1,
  SandboxProfileV1,
  SessionSearchResultV1,
  SkillCandidateV1,
} from "./api";

export function isActiveInteraction(interaction: InteractionSnapshotV1): boolean {
  return interaction.status === "pending" || interaction.status === "resolving";
}

export function updateActiveInteractions(
  current: ReadonlyMap<string, InteractionSnapshotV1>,
  interaction: InteractionSnapshotV1,
): Map<string, InteractionSnapshotV1> {
  const next = new Map(current);
  if (isActiveInteraction(interaction)) {
    next.set(interaction.interactionId, interaction);
  } else {
    next.delete(interaction.interactionId);
  }
  return next;
}

export function pendingInteractionSessionIds(
  interactions: ReadonlyMap<string, InteractionSnapshotV1>,
  focusedSessionId: string | null,
): Set<string> {
  const sessionIds = new Set<string>();
  for (const interaction of interactions.values()) {
    if (interaction.sessionId !== focusedSessionId) {
      sessionIds.add(interaction.sessionId);
    }
  }
  return sessionIds;
}

export function permissionFromInteraction(
  interaction: InteractionSnapshotV1,
): PermissionPayload | null {
  const payload = interaction.payload;
  if (payload.kind !== "permission") return null;
  return {
    interactionId: interaction.interactionId,
    rpcId: interaction.rpcId,
    sessionId: interaction.sessionId,
    toolCallId: interaction.toolCallId ?? "",
    toolName: payload.toolName,
    title: payload.title,
    preview: payload.preview,
    scopeKey: payload.scopeKey,
    options: payload.options,
  };
}

export function askUserFromInteraction(
  interaction: InteractionSnapshotV1,
): AskUserPayload | null {
  const payload = interaction.payload;
  if (payload.kind !== "ask_user") return null;
  return {
    interactionId: interaction.interactionId,
    rpcId: interaction.rpcId,
    sessionId: interaction.sessionId,
    toolCallId: interaction.toolCallId,
    questions: payload.questions,
    partialAnswers: payload.partialAnswers,
  };
}

export function normalizeSandboxProfile(value: unknown): SandboxProfileV1 {
  return value === "workspace_write" || value === "read_only" ? value : "off";
}

export function sandboxStateMessageKey(state: string): string {
  return (
    {
      off: "settings.sandboxState.off",
      available: "settings.sandboxState.available",
      needs_install: "settings.sandboxState.needsInstall",
      unsupported_platform: "settings.sandboxState.unsupportedPlatform",
      applied: "settings.sandboxState.applied",
    }[state] ?? "settings.sandboxState.unknown"
  );
}

export function acpLoopbackSetupCommand(address: string): string {
  const port = address.trim().match(/:(\d+)$/)?.[1] || "8799";
  return `socat TCP-LISTEN:${port},bind=127.0.0.1,reuseaddr,fork EXEC:'grok agent --no-leader stdio'`;
}

export function skillCandidateAtBootstrap(
  candidates: SkillCandidateV1[],
): SkillCandidateV1 | null {
  return candidates.find((candidate) => candidate.status === "pending") ?? null;
}

export function unseenSkillCandidateForSession(
  candidates: SkillCandidateV1[],
  sessionId: string | null,
  seenIds: ReadonlySet<string>,
): SkillCandidateV1 | null {
  return (
    candidates.find(
      (candidate) =>
        candidate.status === "pending" &&
        candidate.source.sessionId === sessionId &&
        !seenIds.has(candidate.id),
    ) ?? null
  );
}

export function rememberSkillCandidateIds(
  target: Set<string>,
  candidates: SkillCandidateV1[],
): void {
  for (const candidate of candidates) target.add(candidate.id);
}

type SearchSession = {
  id: string;
  title: string;
  projectId?: string | null;
  archived?: boolean;
};

export function mergeContentSearchSessions<T extends SearchSession>(
  titleMatches: T[],
  allSessions: T[],
  contentHits: SessionSearchResultV1[],
  limit = 20,
): T[] {
  const seen = new Set(titleMatches.map((session) => session.id));
  const merged = [...titleMatches];
  for (const hit of contentHits) {
    const session = allSessions.find(
      (candidate) => candidate.id === hit.sessionId && !candidate.archived,
    );
    if (!session || seen.has(session.id)) continue;
    seen.add(session.id);
    merged.push(session);
    if (merged.length >= limit) break;
  }
  return merged;
}

export type AutomationClaimIntake = "ignore" | "bound" | "queue";

export function automationClaimIntake(
  claim: AutomationClaimV1,
  handledIds: ReadonlySet<string>,
  disposed: boolean,
): AutomationClaimIntake {
  if (disposed || handledIds.has(claim.claimId)) return "ignore";
  return claim.sessionId ? "bound" : "queue";
}

export function canStartAutomationClaim(args: {
  claim: AutomationClaimV1 | null;
  connecting: boolean;
  sessionState: string;
  runLocked: boolean;
  handledIds: ReadonlySet<string>;
}): boolean {
  return !!(
    args.claim &&
    !args.connecting &&
    args.sessionState !== "streaming" &&
    args.sessionState !== "awaiting_permission" &&
    !args.runLocked &&
    !args.handledIds.has(args.claim.claimId)
  );
}

export function interactionMatches(
  currentInteractionId: string | null | undefined,
  interaction: InteractionSnapshotV1,
): boolean {
  return currentInteractionId === interaction.interactionId;
}

export function planResolutionContext(
  interactionId: string | null | undefined,
  sessionId: string | null | undefined,
): { interactionId?: string; sessionId?: string } {
  return {
    ...(interactionId ? { interactionId } : {}),
    ...(sessionId ? { sessionId } : {}),
  };
}

export function isReviewablePlanInteraction(
  interaction: InteractionSnapshotV1,
): boolean {
  return interaction.payload.kind === "plan" && isActiveInteraction(interaction);
}

export function livePlanInteraction(
  interactions: ReadonlyMap<string, InteractionSnapshotV1>,
  sessionId: string | null | undefined,
): InteractionSnapshotV1 | null {
  if (!sessionId) return null;
  for (const interaction of interactions.values()) {
    if (
      interaction.sessionId === sessionId &&
      isReviewablePlanInteraction(interaction)
    ) {
      return interaction;
    }
  }
  return null;
}

export function planFromArtifact(
  artifact: PlanArtifactV1,
): Pick<
  PlanReviewState,
  | "visible"
  | "waiting"
  | "body"
  | "entries"
  | "rpcId"
  | "interactionId"
  | "toolCallId"
  | "artifactId"
  | "artifactStatus"
  | "currentRevision"
  | "liveReview"
> {
  const revision = currentPlanRevision(artifact);
  const entries = Array.isArray(revision?.entries) ? revision.entries : [];
  return {
    visible: artifact.status !== "abandoned",
    waiting: artifact.status === "proposed",
    body: (revision?.body ?? "").trim(),
    entries,
    rpcId: null,
    interactionId: artifact.interactionId ?? null,
    toolCallId: artifact.toolCallId ?? null,
    artifactId: artifact.id,
    artifactStatus: artifact.status,
    currentRevision: artifact.currentRevision,
    liveReview: false,
  };
}
