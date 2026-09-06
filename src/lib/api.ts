/** Typed Tauri invoke helpers with browser fallback. */

import type { PlanArtifactV1 } from "./planArtifacts";
import type {
  AskUserPayload,
  InteractionSnapshotV1,
  SessionSnapshot,
} from "./session";
import { IDLE_SNAPSHOT } from "./session";
import {
  RUNTIME_SUBSCRIBE_URL,
  RUNTIME_USAGE_URL,
} from "./runtimeCompat";

export function isTauri(): boolean {
  return (
    typeof window !== "undefined" &&
    ("__TAURI_INTERNALS__" in window || "__TAURI__" in window)
  );
}

export type CapabilityState =
  | "available"
  | "unavailable"
  | "needs_permission"
  | "needs_install"
  | "unsupported_platform";

export interface HostCapability {
  state: CapabilityState;
  reason?: string;
  version?: string;
}

export interface HostCapabilities {
  version?: number;
  platform: "macos" | "windows" | "linux" | "other" | string;
  finderSelection: boolean;
  speechRecognition: boolean;
  skillDraftSave: boolean;
  capabilities?: Record<string, HostCapability>;
}

export type SandboxProfileV1 = "off" | "workspace_write" | "read_only";

export interface SandboxApplicationV1 {
  requested: SandboxProfileV1 | string;
  applied: SandboxProfileV1 | string;
  verified: boolean;
  state: string;
  platform: string;
  reason?: string | null;
}

export interface RuntimeFeatureV1 {
  state: string;
  source: string;
  reason?: string | null;
}

export interface RuntimeKernelV1 {
  stored: string;
  effective: string;
  overrideSource: string;
}

export interface RuntimeCapabilitiesV1 {
  version: 1;
  runtimeVersion?: string | null;
  protocolVersion: number;
  clientVersion: string;
  platform: string;
  sandbox: SandboxApplicationV1;
  memory: RuntimeFeatureV1;
  pluginCatalog: RuntimeFeatureV1;
  hooksInventory: RuntimeFeatureV1;
  mcp: RuntimeFeatureV1;
  kernel: RuntimeKernelV1;
}

export interface CapabilityDescriptorV1 {
  id: string;
  kind: "host_contract" | "runtime_contract" | "plugin_inventory" | string;
  state: string;
  version?: string | null;
  source: string;
  interfaceHash: string;
  implementationOmitted: boolean;
}

export interface CapabilityManifestV1 {
  schema: "sunsetz.capabilities.v1" | string;
  version: 1;
  producer: string;
  generatedAt: string;
  capabilities: CapabilityDescriptorV1[];
}

export interface CapabilityManifestValidationV1 {
  version: 1;
  valid: boolean;
  errors: string[];
}

export type EcosystemPackageKindV1 = "skill" | "plugin" | "mcp";
export type EcosystemPublisherVerificationStateV1 = "unverified" | "verified";
export type EcosystemSourceKindV1 =
  | "built_in"
  | "local"
  | "git"
  | "registry"
  | "remote";
export type EcosystemPackagePermissionV1 =
  | "filesystem_read"
  | "filesystem_write"
  | "network"
  | "process_spawn"
  | "mcp_invoke"
  | "secrets_use"
  | "user_interaction";
export type EcosystemCapabilityKindV1 =
  | "skill"
  | "tool"
  | "resource"
  | "mcp_server";

export interface EcosystemPublisherIdentityV1 {
  id: string;
  displayName: string;
  verificationState: EcosystemPublisherVerificationStateV1;
  verificationAuthority: string | null;
  verificationEvidenceHash: string | null;
}

export interface EcosystemSourceIdentityV1 {
  kind: EcosystemSourceKindV1;
  sourceId: string;
}

export interface EcosystemSourceProvenanceV1 {
  identity: EcosystemSourceIdentityV1;
  revision: string | null;
}

export interface EcosystemPackageDependencyV1 {
  packageId: string;
  source: EcosystemSourceIdentityV1;
  versionRequirement: string;
}

export interface EcosystemCapabilityExportV1 {
  capabilityId: string;
  kind: EcosystemCapabilityKindV1;
  version: string;
  interfaceHash: string;
}

export interface EcosystemPackageManifestV1 {
  schema: "sunsetz.ecosystem-package.v1" | string;
  schemaVersion: 1;
  kind: EcosystemPackageKindV1;
  packageId: string;
  displayName: string;
  version: string;
  publisher: EcosystemPublisherIdentityV1;
  license: string;
  source: EcosystemSourceProvenanceV1;
  interfaceHash: string;
  artifactHash: string;
  dependencies: EcosystemPackageDependencyV1[];
  permissions: EcosystemPackagePermissionV1[];
  capabilityExports: EcosystemCapabilityExportV1[];
  implementationOmitted: boolean;
}

export interface EcosystemPackageManifestValidationV1 {
  version: 1;
  valid: boolean;
  canonicalManifestHash: string | null;
  errors: string[];
}

export interface EcosystemPackageImportCandidateV1 {
  manifest: EcosystemPackageManifestV1;
  expectedManifestHash: string;
}

export interface EcosystemPackageImportPreviewRequestV1 {
  version: 1;
  candidates: EcosystemPackageImportCandidateV1[];
}

export interface EcosystemPackageImportPreviewItemV1 {
  packageRef: string;
  kind: EcosystemPackageKindV1;
  packageId: string;
  displayName: string;
  version: string;
  publisherId: string;
  publisherVerificationState: EcosystemPublisherVerificationStateV1;
  source: EcosystemSourceIdentityV1;
  manifestHash: string;
  artifactHash: string;
  dependencies: string[];
  permissions: EcosystemPackagePermissionV1[];
  capabilityExports: EcosystemCapabilityExportV1[];
  requiresExplicitReview: boolean;
}

export interface EcosystemPackageImportPreviewV1 {
  version: 1;
  previewHash: string;
  packages: EcosystemPackageImportPreviewItemV1[];
}

const BROWSER_HOST_CAPABILITIES: HostCapabilities = {
  platform: "other",
  finderSelection: false,
  speechRecognition: false,
  skillDraftSave: false,
  version: 2,
  capabilities: {
    finderSelection: {
      state: "unsupported_platform",
      reason: "Finder selection is available only on macOS",
    },
    speechRecognition: {
      state: "unavailable",
      reason: "No native speech adapter is registered",
    },
    skillDraftSave: {
      state: "unavailable",
      reason: "Skill drafts require the desktop Host",
    },
  },
};

function legacyCapabilityValue(
  host: HostCapabilities,
  id: string,
  legacyValue?: boolean,
): boolean | undefined {
  if (legacyValue !== undefined) return legacyValue;
  if (id === "finderSelection") return host.finderSelection;
  if (id === "speechRecognition") return host.speechRecognition;
  if (id === "skillDraftSave") return host.skillDraftSave;
  return undefined;
}

export function hostCapability(
  host: HostCapabilities,
  id: string,
  legacyValue?: boolean,
): HostCapability | undefined {
  const declared = host.capabilities?.[id];
  if (declared) return declared;
  if ((host.version ?? 1) >= 2) return undefined;
  const fallback = legacyCapabilityValue(host, id, legacyValue);
  if (fallback === undefined) return undefined;
  return { state: fallback ? "available" : "unavailable" };
}

export function capabilityState(
  host: HostCapabilities,
  id: string,
  legacyValue?: boolean,
): CapabilityState | undefined {
  return hostCapability(host, id, legacyValue)?.state;
}

export function capabilityReason(
  host: HostCapabilities,
  id: string,
  legacyValue?: boolean,
): string | undefined {
  return hostCapability(host, id, legacyValue)?.reason;
}

export function capabilityAvailable(
  host: HostCapabilities,
  id: string,
  legacyValue?: boolean,
): boolean {
  return capabilityState(host, id, legacyValue) === "available";
}

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) throw new Error(`Tauri required: ${cmd}`);
  const { invoke: inv } = await import("@tauri-apps/api/core");
  return inv<T>(cmd, args);
}

/** Capabilities backed by concrete host commands. Unknown capabilities stay hidden. */
export async function hostCapabilities(): Promise<HostCapabilities> {
  if (!isTauri()) return BROWSER_HOST_CAPABILITIES;
  return invoke<HostCapabilities>("host_capabilities");
}

export async function runtimeCapabilitiesV1(): Promise<RuntimeCapabilitiesV1> {
  if (!isTauri()) {
    return {
      version: 1,
      runtimeVersion: null,
      protocolVersion: 1,
      clientVersion: "browser",
      platform: "other",
      sandbox: {
        requested: "off",
        applied: "off",
        verified: true,
        state: "off",
        platform: "other",
      },
      memory: { state: "unavailable", source: "browser" },
      pluginCatalog: { state: "unavailable", source: "browser" },
      hooksInventory: { state: "unavailable", source: "browser" },
      mcp: { state: "unavailable", source: "browser" },
      kernel: {
        stored: "sunsetz",
        effective: "sunsetz",
        overrideSource: "none",
      },
    };
  }
  return invoke<RuntimeCapabilitiesV1>("runtime_capabilities_v1");
}

/** Export a metadata-only capability contract; no implementation or local paths. */
export async function capabilityManifestExportV1(): Promise<CapabilityManifestV1> {
  if (!isTauri()) {
    throw new Error("Capability manifests require the desktop Host");
  }
  return invoke<CapabilityManifestV1>("capability_manifest_export_v1");
}

/** Validate a clean-room capability manifest before another Agent consumes it. */
export async function capabilityManifestValidateV1(
  manifest: CapabilityManifestV1,
): Promise<CapabilityManifestValidationV1> {
  if (!isTauri()) {
    return { version: 1, valid: false, errors: ["Desktop Host required"] };
  }
  return invoke<CapabilityManifestValidationV1>(
    "capability_manifest_validate_v1",
    { manifest },
  );
}

/** Validate metadata only; this command never imports, installs, or executes. */
export async function ecosystemPackageManifestValidateV1(
  manifest: EcosystemPackageManifestV1,
): Promise<EcosystemPackageManifestValidationV1> {
  if (!isTauri()) {
    return {
      version: 1,
      valid: false,
      canonicalManifestHash: null,
      errors: ["Desktop Host required"],
    };
  }
  return invoke<EcosystemPackageManifestValidationV1>(
    "ecosystem_package_manifest_validate_v1",
    { manifest },
  );
}

/** Build a dependency-ordered review preview; no artifact bytes are touched. */
export async function ecosystemPackageImportPreviewV1(
  request: EcosystemPackageImportPreviewRequestV1,
): Promise<EcosystemPackageImportPreviewV1> {
  if (!isTauri()) {
    throw new Error("Ecosystem package preview requires the desktop Host");
  }
  return invoke<EcosystemPackageImportPreviewV1>(
    "ecosystem_package_import_preview_v1",
    { request },
  );
}

/** Current Finder selection. Only exposed when `hostCapabilities.finderSelection`. */
export async function finderSelectedPaths(): Promise<string[]> {
  if (!isTauri()) return [];
  return invoke<string[]>("finder_selected_paths");
}

/** Recover a pending Agent question for the focused or specified task. */
export async function sessionGetPendingAskUser(
  sessionId?: string | null,
): Promise<AskUserPayload | null> {
  if (!isTauri()) return null;
  return invoke("session_get_pending_ask_user", {
    sessionId: sessionId ?? null,
  });
}

/** Recover every task currently paused on an Agent question. */
export async function sessionPendingInteractions(): Promise<AskUserPayload[]> {
  if (!isTauri()) return [];
  return invoke("session_pending_interactions");
}

/** Versioned permission / ask-user / plan interaction query. */
export async function sessionInteractionsList(
  sessionId?: string | null,
): Promise<InteractionSnapshotV1[]> {
  if (!isTauri()) return [];
  return invoke("session_interactions_list", {
    sessionId: sessionId ?? null,
  });
}

export interface ResolveInteractionRequestV1 {
  interactionId: string;
  sessionId: string;
  decision: string;
  optionId?: string | null;
  scopeKey?: string | null;
  feedback?: string | null;
  answers?: Record<string, string> | null;
}

/** Resolve a live interaction by stable id; stale/process-dead requests fail closed. */
export async function sessionResolveInteractionV1(
  request: ResolveInteractionRequestV1,
): Promise<SessionSnapshot> {
  return invoke("session_resolve_interaction_v1", { request });
}

/** Recover durable Plan artifacts for a session. Dead RPCs are not revived. */
export async function sessionPlanArtifactsListV1(
  sessionId: string,
): Promise<PlanArtifactV1[]> {
  if (!isTauri()) return [];
  return invoke("session_plan_artifacts_list_v1", { sessionId });
}

export interface SkillDraftSaveRequest {
  name: string;
  description: string;
  skillMd: string;
  references: Array<{ path: string; content: string }>;
  scope: "project" | "user";
  projectPath?: string | null;
  overwrite?: boolean;
}

export interface SkillDraftSaveResult {
  path: string;
  slug: string;
  scope: "project" | "user" | string;
  overwritten: boolean;
}

export interface SkillCandidateV1 {
  version: 1;
  id: string;
  status: "pending" | "approved" | "rejected" | "cancelled";
  createdAt: string;
  updatedAt: string;
  contentHash: string;
  reviewContentHash?: string | null;
  source: {
    sessionId: string;
    sessionTitle: string;
    messageIds: string[];
  };
  owner: {
    kind: "host_candidate" | "host_generated" | string;
    namespace: string;
    mayOverwriteExternal: false;
  };
  draft: {
    name: string;
    description: string;
    skillMd: string;
    references: Array<{ path: string; content: string }>;
  };
  approvedPath?: string | null;
  approvedContentHash?: string | null;
  auditEvents?: Array<{
    version: 2;
    candidateId: string;
    status: SkillCandidateV1["status"];
    contentHash: string;
    occurredAt: string;
  }>;
}

/** Persist a reviewed skill through the Host's validated atomic writer. */
export async function skillDraftSave(
  request: SkillDraftSaveRequest,
): Promise<SkillDraftSaveResult> {
  return invoke("skill_draft_save", { request });
}

export async function skillCandidatesListV1() {
  return invoke<SkillCandidateV1[]>("skill_candidates_list_v1");
}

export async function skillCandidateApproveV1(request: {
  id: string;
  scope: "project" | "user";
  projectPath?: string | null;
  draft?: SkillCandidateV1["draft"] | null;
  overwrite?: boolean;
  userConfirmedOverwrite?: boolean;
}) {
  return invoke<SkillDraftSaveResult>("skill_candidate_approve_v1", { request });
}

export async function skillCandidateApproveV2(request: {
  id: string;
  expectedContentHash: string;
  finalContentHash?: string | null;
  scope: "project" | "user";
  projectPath?: string | null;
  draft?: SkillCandidateV1["draft"] | null;
  overwrite?: boolean;
  userConfirmedOverwrite?: boolean;
}) {
  return invoke<SkillDraftSaveResult>("skill_candidate_approve_v2", { request });
}

export async function skillCandidateRejectV1(id: string) {
  return invoke<SkillCandidateV1>("skill_candidate_reject_v1", { id });
}

export async function skillCandidateRejectV2(request: {
  id: string;
  expectedContentHash: string;
}) {
  return invoke<SkillCandidateV1>("skill_candidate_reject_v2", { request });
}

export async function skillCandidateCancelV2(request: {
  id: string;
  expectedContentHash: string;
}) {
  return invoke<SkillCandidateV1>("skill_candidate_cancel_v2", { request });
}

export type MemoryCandidateStatusV1 =
  | "pending"
  | "approved"
  | "rejected"
  | "superseded";
export type MemoryCandidateTypeV1 =
  | "user_preference"
  | "project_fact"
  | "workflow_hint";

export interface MemoryCandidateV1 {
  id: string;
  status: MemoryCandidateStatusV1;
  type: MemoryCandidateTypeV1;
  content: string;
  contentHash: string;
  source: { sessionId: string; messageId: string };
  ownership: "host_candidate";
  createdAt: string;
  updatedAt: string;
}

export interface MemoryCandidateMutationRequestV1 {
  id: string;
  expectedContentHash: string;
}

export interface MemoryContextPackItemV1 {
  candidateId: string;
  source: { sessionId: string; messageId: string };
  type: MemoryCandidateTypeV1;
  content: string;
  contentHash: string;
}

export interface MemoryContextPackV1 {
  version: 1;
  items: MemoryContextPackItemV1[];
}

export interface MemoryContextPackRequestV1 {
  version: 1;
  selections: MemoryCandidateMutationRequestV1[];
}

export type MemoryInjectionStatusV1 =
  | "prepared"
  | "dispatching"
  | "applied"
  | "failed"
  | "removed";
export type MemoryInjectionFailureCodeV1 =
  | "runtime_write_failed"
  | "interrupted"
  | "context_unavailable";
export type MemoryInjectionFeedbackV1 = "helpful" | "unhelpful";

export interface MemoryInjectionRecordV1 {
  version: 1;
  injectionId: string;
  sessionId: string;
  contextHash: string;
  selections: MemoryCandidateMutationRequestV1[];
  status: MemoryInjectionStatusV1;
  revision: number;
  attempt: number;
  failureCode?: MemoryInjectionFailureCodeV1 | null;
  feedback?: MemoryInjectionFeedbackV1 | null;
  createdAt: string;
  preparedAt: string;
  updatedAt: string;
  appliedAt?: string | null;
  removedAt?: string | null;
}

export interface MemoryInjectionDisclosureV1 {
  version: 1;
  injectionId: string;
  contextHash: string;
  reviewedMemory: true;
  contextOnlyNotInstructions: true;
  containsSessionEvidence: false;
  items: Array<{
    candidateId: string;
    contentHash: string;
    type: MemoryCandidateTypeV1;
    content: string;
    provenance: { sessionId: string; messageId: string };
  }>;
}

export interface MemoryRecallPreviewV1 {
  version: 1;
  memoryCandidates: Array<{
    candidateId: string;
    contentHash: string;
    type: MemoryCandidateTypeV1;
    content: string;
    provenance: { sessionId: string; messageId: string };
    relevanceScore: number;
  }>;
  contextPackSelections: MemoryCandidateMutationRequestV1[];
  sessionEvidence: Array<{
    sessionId: string;
    sessionTitle: string;
    messageId: string;
    role: string;
    snippet: string;
    rank: number;
    evidenceOnly: true;
    untrusted: true;
  }>;
}

export async function memoryCandidatesListV1() {
  return invoke<MemoryCandidateV1[]>("memory_candidates_list_v1");
}

export interface AgentMemoryEntryV1 {
  id: string;
  content: string;
  updatedAt: string;
}

export interface AgentMemoryStoreV1 {
  version: 1 | number;
  enabled: boolean;
  notes: AgentMemoryEntryV1[];
  userProfile: AgentMemoryEntryV1[];
}

export interface AgentMemoryMutationV1 {
  action: string;
  target: string;
  content?: string | null;
  oldText?: string | null;
}

export async function agentMemoryGetV1() {
  return invoke<AgentMemoryStoreV1>("agent_memory_get_v1");
}

export async function agentMemorySetEnabledV1(enabled: boolean) {
  return invoke<AgentMemoryStoreV1>("agent_memory_set_enabled_v1", { enabled });
}

export async function agentMemoryMutateV1(request: AgentMemoryMutationV1) {
  return invoke<AgentMemoryStoreV1>("agent_memory_mutate_v1", { request });
}

export async function agentMemoryClearV1() {
  return invoke<AgentMemoryStoreV1>("agent_memory_clear_v1");
}

export async function memoryContextPackBuildV1(
  selections: MemoryCandidateMutationRequestV1[],
) {
  return invoke<MemoryContextPackV1>("memory_context_pack_build_v1", {
    request: { version: 1, selections },
  });
}

export async function memoryRecallPreviewV1(
  query: string,
  currentSessionId?: string | null,
) {
  return invoke<MemoryRecallPreviewV1>("memory_recall_preview_v1", {
    request: {
      version: 1,
      query,
      currentSessionId: currentSessionId ?? null,
    },
  });
}

export async function memoryInjectionsListV1(sessionId: string) {
  return invoke<MemoryInjectionRecordV1[]>("memory_injections_list_v1", {
    sessionId,
  });
}

export async function memoryInjectionFeedbackV1(
  record: MemoryInjectionRecordV1,
  feedback: MemoryInjectionFeedbackV1,
) {
  return invoke<MemoryInjectionRecordV1>("memory_injection_feedback_v1", {
    request: {
      version: 1,
      sessionId: record.sessionId,
      injectionId: record.injectionId,
      expectedContextHash: record.contextHash,
      expectedRevision: record.revision,
      feedback,
    },
  });
}

export async function memoryInjectionRemoveV1(
  record: MemoryInjectionRecordV1,
) {
  return invoke<MemoryInjectionRecordV1>("memory_injection_remove_v1", {
    request: {
      version: 1,
      sessionId: record.sessionId,
      injectionId: record.injectionId,
      expectedContextHash: record.contextHash,
      expectedRevision: record.revision,
    },
  });
}

export interface MemoryPortableExportV1 {
  schema: string;
  version: 1;
  contentHash: string;
  candidates: Array<{
    candidateId: string;
    contentHash: string;
    category: MemoryCandidateTypeV1;
    content: string;
    provenance: {
      kind: "persisted_user_message";
      sessionId: string;
      messageId: string;
      live: true;
    };
    createdAt: string;
    updatedAt: string;
  }>;
  injectionAudit: Array<{
    sessionId: string;
    injectionId: string;
    contextHash: string;
    status: MemoryInjectionStatusV1;
    revision: number;
    attempt: number;
    failureCode?: MemoryInjectionFailureCodeV1 | null;
    feedback?: MemoryInjectionFeedbackV1 | null;
    selections: Array<{ candidateId: string; contentHash: string }>;
    createdAt: string;
    preparedAt: string;
    updatedAt: string;
    appliedAt?: string | null;
    removedAt?: string | null;
  }>;
  excludesSessionSearchIndex: true;
  excludesQueriesAndPromptFragments: true;
}

export interface MemoryClearScopeV1 {
  candidateSelections: MemoryCandidateMutationRequestV1[];
  sourceSessionIds: string[];
}

export interface MemoryClearPlanV1 {
  schema: string;
  version: 1;
  dryRun: true;
  requiresConfirmation: true;
  candidateStoreHash: string;
  sessionsIndexHash: string;
  scope: MemoryClearScopeV1;
  candidates: Array<{
    candidateId: string;
    expectedContentHash: string;
    category: MemoryCandidateTypeV1;
    provenance: {
      kind: "persisted_user_message";
      sessionId: string;
      messageId: string;
      live: boolean;
    };
  }>;
  injections: Array<{
    sessionId: string;
    injectionId: string;
    expectedContextHash: string;
    expectedRevision: number;
    matchedCandidateIds: string[];
  }>;
  skipped: Array<{
    kind: "candidate" | "source_session";
    id: string;
    reason: "candidate_not_found" | "no_candidates_for_provenance";
  }>;
  sessionSearchIndexIsRebuildableCache: true;
  planHash: string;
}

export interface MemoryClearResultV1 {
  version: 1;
  planHash: string;
  beforeCandidateStoreHash: string;
  afterCandidateStoreHash: string;
  deletedCandidates: Array<{ candidateId: string; contentHash: string }>;
  deletedInjections: Array<{
    sessionId: string;
    injectionId: string;
    contextHash: string;
    revision: number;
  }>;
  skipped: MemoryClearPlanV1["skipped"];
  sessionSearchIndexMutated: false;
  sessionSearchIndexIsRebuildableCache: true;
}

export async function memoryExportV1(includeInjectionAudit = true) {
  return invoke<MemoryPortableExportV1>("memory_export_v1", {
    request: { version: 1, includeInjectionAudit },
  });
}

export async function memoryClearPreviewV1(scope: MemoryClearScopeV1) {
  return invoke<MemoryClearPlanV1>("memory_clear_preview_v1", {
    request: { version: 1, scope },
  });
}

export async function memoryClearConfirmV1(plan: MemoryClearPlanV1) {
  return invoke<MemoryClearResultV1>("memory_clear_confirm_v1", {
    request: { version: 1, plan },
  });
}

export async function memoryCandidateCreateV1(request: {
  type: MemoryCandidateTypeV1;
  content: string;
  source: { sessionId: string; messageId: string };
}) {
  return invoke<MemoryCandidateV1>("memory_candidate_create_v1", { request });
}

export async function memoryCandidateApproveV1(
  request: MemoryCandidateMutationRequestV1,
) {
  return invoke<MemoryCandidateV1>("memory_candidate_approve_v1", { request });
}

export async function memoryCandidateRejectV1(
  request: MemoryCandidateMutationRequestV1,
) {
  return invoke<MemoryCandidateV1>("memory_candidate_reject_v1", { request });
}

export async function memoryCandidateSupersedeV1(
  request: MemoryCandidateMutationRequestV1,
) {
  return invoke<MemoryCandidateV1>("memory_candidate_supersede_v1", { request });
}

export async function memoryCandidateDeleteV1(
  request: MemoryCandidateMutationRequestV1,
) {
  return invoke<MemoryCandidateV1>("memory_candidate_delete_v1", { request });
}

export async function sessionGetState(): Promise<SessionSnapshot> {
  if (!isTauri()) return { ...IDLE_SNAPSHOT, backend: "browser" };
  return invoke("session_get_state");
}

export async function sessionConnect(opts?: {
  projectPath?: string;
  sessionId?: string;
  mode?: string;
}): Promise<SessionSnapshot> {
  if (!isTauri()) {
    return {
      ...IDLE_SNAPSHOT,
      sessionId: "browser",
      state: "ready",
      backend: "browser",
      title: "Browser preview",
    };
  }
  return invoke("session_connect", {
    projectPath: opts?.projectPath ?? null,
    sessionId: opts?.sessionId ?? null,
    mode: opts?.mode ?? null,
  });
}

/**
 * Send a turn to the agent.
 * @param text Agent prompt (skills as `/name`, attachments as `@path`, etc.)
 * @param displayText Optional user-bubble text for journal (e.g. `[[skill:name]]` chips).
 *                    When omitted, journal stores `text`.
 */
export async function sessionSend(
  text: string,
  displayText?: string | null,
  attachments?: Array<{
    path: string;
    name: string;
    isDir: boolean;
  }> | null,
): Promise<SessionSnapshot> {
  return invoke("session_send", {
    text,
    displayText: displayText ?? null,
    attachments: attachments ?? null,
  });
}

export interface SessionSendResultV2 {
  version: 2;
  snapshot: SessionSnapshot;
  memoryInjection?: MemoryInjectionRecordV1 | null;
  memoryDisclosure?: MemoryInjectionDisclosureV1 | null;
  skillUses?: SkillUseRecordV1[];
}

export type SkillUseSelectionV1 = "explicit" | "accepted_suggestion";
export type SkillUseStatusV1 =
  | "prepared"
  | "dispatching"
  | "applied"
  | "succeeded"
  | "failed"
  | "interrupted";
export type SkillUseFeedbackRatingV1 = "helpful" | "unhelpful";

export interface SkillIdentityV1 {
  id: string;
  name: string;
  treeHash: string;
  sourceCandidateId?: string | null;
}

export interface SkillSelectionRequestV1 {
  id: string;
  expectedTreeHash: string;
  selection: SkillUseSelectionV1;
}

export interface SkillUseRecordV1 {
  version: 1;
  id: string;
  revision: number;
  selection: SkillUseSelectionV1;
  sessionId: string;
  turnId: string;
  skill: SkillIdentityV1;
  status: SkillUseStatusV1;
  statusEvents: Array<{ status: SkillUseStatusV1; occurredAt: string }>;
  feedback?: { rating: SkillUseFeedbackRatingV1; occurredAt: string } | null;
  createdAt: string;
  updatedAt: string;
}

/**
 * Versioned Host send. When Memory selections are present, the Host rebuilds
 * the reviewed pack, writes a disclosure audit first, and owns the exact ACP
 * prompt fragment. JavaScript never receives or constructs that fragment.
 */
export async function sessionSendV2(request: {
  sessionId: string;
  text: string;
  displayText?: string | null;
  attachments?: Array<{
    path: string;
    name: string;
    isDir: boolean;
  }> | null;
  memoryContextPack?: MemoryContextPackRequestV1 | null;
  memoryRetry?: {
    version: 1;
    sessionId: string;
    injectionId: string;
    expectedContextHash: string;
    expectedRevision: number;
    contextPack: MemoryContextPackRequestV1;
  } | null;
  skillSelections?: SkillSelectionRequestV1[];
  connectorSelections?: Array<{ id: string; selection?: "explicit" }>;
}): Promise<SessionSendResultV2> {
  return invoke("session_send_v2", {
    request: {
      version: 2,
      sessionId: request.sessionId,
      text: request.text,
      displayText: request.displayText ?? null,
      attachments: request.attachments ?? null,
      memoryContextPack: request.memoryContextPack ?? null,
      memoryRetry: request.memoryRetry ?? null,
      skillSelections: request.skillSelections ?? [],
      connectorSelections: request.connectorSelections ?? [],
    },
  });
}

/** Drop last user turn (agent rewind + local journal) before edit-resend. */
export async function sessionRewindDropLastUser(): Promise<SessionSnapshot> {
  return invoke("session_rewind_drop_last_user");
}

/** One user-prompt checkpoint on the rewind timeline. */
export interface RewindPoint {
  promptIndex: number;
  messageId?: string | null;
  preview: string;
}

/** Result of rewinding to a prompt index (local journal always applies). */
export interface RewindExecuteResult {
  snapshot: SessionSnapshot;
  /** False when agent rewind failed / unsupported / disconnected. */
  agentOk: boolean;
  agentError?: string | null;
  localOk: boolean;
  keptCount: number;
}

/** List rewind points for a session journal (live session when `sessionId` omitted). */
export async function sessionRewindPoints(
  sessionId?: string | null,
): Promise<RewindPoint[]> {
  return invoke("session_rewind_points", {
    sessionId: sessionId ?? null,
  });
}

/**
 * Rewind to a 0-based user-prompt index (keep that turn, drop after).
 * Local journal is always truncated; agent extension is best-effort when live.
 */
export async function sessionRewindExecute(
  targetPromptIndex: number,
  opts?: { restoreFiles?: boolean; sessionId?: string | null },
): Promise<RewindExecuteResult> {
  return invoke("session_rewind_execute", {
    targetPromptIndex,
    restoreFiles: opts?.restoreFiles ?? false,
    sessionId: opts?.sessionId ?? null,
  });
}

/** Fork a session into a new chat (same project; optional cut through user turn). */
export async function sessionFork(
  sourceId: string,
  opts?: {
    throughUserPromptIndex?: number | null;
    title?: string | null;
  },
) {
  return invoke<{
    id: string;
    projectId: string | null;
    title: string;
    updatedAt: string;
    modelId: string | null;
    archived?: boolean;
    scheduled?: boolean;
  }>("session_fork", {
    sourceId,
    throughUserPromptIndex: opts?.throughUserPromptIndex ?? null,
    title: opts?.title ?? null,
  });
}

export async function sessionStop(): Promise<SessionSnapshot> {
  return invoke("session_stop");
}

export interface SessionSubagentView {
  id: string;
  parentSessionId: string;
  description: string;
  agentType: string;
  status: "queued" | "running" | "completed" | "failed" | "cancelled";
  background: boolean;
  summary: string;
  transcript: { kind: string; text: string }[];
}

export async function sessionSubagentGet(
  id: string,
): Promise<SessionSubagentView> {
  return invoke("session_subagent_get", { id });
}

export async function sessionDisconnect(): Promise<SessionSnapshot> {
  return invoke("session_disconnect");
}

export async function sessionReattach(): Promise<SessionSnapshot> {
  return invoke("session_reattach");
}

export async function sessionResolvePermission(args: {
  interactionId?: string | null;
  sessionId?: string | null;
  rpcId: number;
  decision: string;
  optionId?: string;
  scopeKey?: string;
}): Promise<SessionSnapshot> {
  return invoke("session_resolve_permission", {
    rpcId: args.rpcId,
    decision: args.decision,
    optionId: args.optionId ?? null,
    scopeKey: args.scopeKey ?? null,
    sessionId: args.sessionId ?? null,
    interactionId: args.interactionId ?? null,
  });
}

/** Approve / revise / abandon pending `_x.ai/exit_plan_mode`. */
export async function sessionResolvePlan(args: {
  interactionId?: string | null;
  sessionId?: string | null;
  decision: "approved" | "cancelled" | "abandoned" | string;
  feedback?: string | null;
  rpcId?: number | null;
}): Promise<SessionSnapshot> {
  return invoke("session_resolve_plan", {
    decision: args.decision,
    feedback: args.feedback ?? null,
    rpcId: args.rpcId ?? null,
    sessionId: args.sessionId ?? null,
    interactionId: args.interactionId ?? null,
  });
}

/** Answer or dismiss pending `_x.ai/ask_user_question`. */
export async function sessionResolveAskUser(args: {
  interactionId?: string | null;
  sessionId?: string | null;
  decision: "accepted" | "cancelled" | string;
  answers?: Record<string, string> | null;
  rpcId?: number | null;
}): Promise<SessionSnapshot> {
  return invoke("session_resolve_ask_user", {
    sessionId: args.sessionId ?? null,
    decision: args.decision,
    answers: args.answers ?? null,
    rpcId: args.rpcId ?? null,
    interactionId: args.interactionId ?? null,
  });
}

export async function probeCli(manualPath?: string) {
  return invoke<{
    found: boolean;
    path: string | null;
    version: string | null;
    source: string;
    cliAuthPresent?: boolean;
    candidatesTried?: string[];
  }>("probe_cli", { manualPath: manualPath ?? null });
}

export interface AcpProbeResult {
  ok: boolean;
  agentVersion?: string | null;
  model?: string | null;
  error?: string | null;
}

/** API mode: TCP-connect to an ACP server and run the initialize handshake. */
export async function acpTestConnection(addr: string) {
  return invoke<AcpProbeResult>("acp_test_connection", { addr });
}

export interface CliInstallProgress {
  phase: string;
  message: string;
  percent?: number | null;
  bytesDownloaded?: number | null;
  totalBytes?: number | null;
  mirror?: string | null;
  version?: string | null;
}

export interface CliInstallResult {
  ok: boolean;
  path: string | null;
  version: string | null;
  mirrorUsed: string | null;
  message: string;
}

export interface CliInstallCommands {
  primary: string;
  shell: string;
  docsUrl: string;
  mirrors: string[];
}

/** Download + install latest Grok Build (multi-mirror). Progress via setup://cli-install-progress. */
export async function cliInstallLatest() {
  return invoke<CliInstallResult>("cli_install_latest");
}

export async function cliInstallCommands() {
  return invoke<CliInstallCommands>("cli_install_commands");
}

export async function pickCliBinary() {
  return invoke<string | null>("pick_cli_binary");
}

export async function openExternalUrl(url: string) {
  return invoke<void>("open_external_url", { url });
}

export interface ProjectInstructionInspectV1 {
  version: 1;
  relativePath: string | null;
  truncated: boolean;
  characterCount: number;
}

export async function projectInstructionInspectV1(
  projectPath?: string | null,
): Promise<ProjectInstructionInspectV1> {
  if (!isTauri()) {
    return {
      version: 1,
      relativePath: null,
      truncated: false,
      characterCount: 0,
    };
  }
  return invoke<ProjectInstructionInspectV1>("project_instruction_inspect_v1", {
    projectPath: projectPath ?? null,
  });
}

export async function projectsList() {
  return invoke<
    Array<{
      id: string;
      name: string;
      path: string;
      trusted: boolean;
      pathOk: boolean;
      pinned?: boolean;
    }>
  >("projects_list");
}

export type ProjectGitSummarySourceV1 =
  | "filesystem_marker"
  | "git_status_porcelain_v2"
  | "cache"
  | "unavailable";

export type ProjectGitSummaryUnavailableReasonV1 =
  | "project_missing"
  | "invalid_git_metadata"
  | "git_unavailable"
  | "timeout"
  | "output_limit"
  | "git_failed"
  | "parse_failed";

export interface ProjectGitSummaryV1 {
  version: 1;
  projectId: string;
  available: boolean;
  isRepo: boolean;
  branch: string | null;
  ahead: number | null;
  behind: number | null;
  dirty: number;
  conflicts: number;
  countsCapped: boolean;
  head: string | null;
  observedAt: string;
  source: ProjectGitSummarySourceV1;
  unavailableReason: ProjectGitSummaryUnavailableReasonV1 | null;
}

/** Lightweight, metadata-only Git summary for an indexed project. */
export async function projectGitSummaryV1(
  projectId: string,
  projectPath: string,
): Promise<ProjectGitSummaryV1 | null> {
  if (!isTauri()) return null;
  return invoke<ProjectGitSummaryV1>("project_git_summary_v1", {
    request: { version: 1, projectId, projectPath },
  });
}

export async function projectAdd(path: string, trust: boolean) {
  return invoke("project_add", { path, trust });
}

/** One linked git worktree from `git worktree list --porcelain`. */
export interface GitWorktreeEntry {
  path: string;
  head?: string | null;
  branch?: string | null;
  detached: boolean;
  isMain: boolean;
  locked: boolean;
  prunable: boolean;
}

export interface GitWorktreesResult {
  available: boolean;
  worktrees: GitWorktreeEntry[];
  reason?: string | null;
}

/** List worktrees for a project folder. Soft-fails when git/repo missing. */
export async function gitWorktreesList(projectPath: string) {
  return invoke<GitWorktreesResult>("git_worktrees_list", { projectPath });
}

/** Native folder dialog → add project. Returns null if user cancels. */
export async function projectAddDialog(trust: boolean) {
  return invoke<{
    id: string;
    name: string;
    path: string;
    trusted: boolean;
    pathOk: boolean;
  } | null>("project_add_dialog", { trust });
}

export async function pickDirectory() {
  return invoke<string | null>("pick_directory");
}

/** Native multi-file picker for composer attachments (empty if cancelled). */
export async function pickAttachFiles() {
  return invoke<string[]>("pick_attach_files");
}

/** Native folder picker for attaching a directory. */
export async function pickAttachFolder() {
  return invoke<string | null>("pick_attach_folder");
}

/**
 * Persist clipboard/webview File bytes into the app attachments dir.
 * Returns a classified path entry for `@path` agent refs.
 */
export async function saveTempAttachment(
  bytesBase64: string,
  suggestedName?: string | null,
  mime?: string | null,
) {
  return invoke<PathEntry>("save_temp_attachment", {
    bytesBase64,
    suggestedName: suggestedName ?? null,
    mime: mime ?? null,
  });
}

/**
 * Read an image from the OS clipboard (native) and save under attachments/paste.
 * Fallback when the WebView paste event has no File objects (macOS screenshots).
 * Returns null when the clipboard has no image.
 */
export async function clipboardPasteImage() {
  if (!isTauri()) return null;
  return invoke<PathEntry | null>("clipboard_paste_image");
}

export interface PathEntry {
  path: string;
  name: string;
  isDir: boolean;
  exists: boolean;
}

/** Classify absolute paths as file/dir for drag-drop. */
export async function pathsClassify(paths: string[]) {
  return invoke<PathEntry[]>("paths_classify", { paths });
}

/** Open with OS default app. */
export async function pathOpen(path: string) {
  return invoke<void>("path_open", { path });
}

/** Reveal in Finder / Explorer. */
export async function pathReveal(path: string) {
  return invoke<void>("path_reveal", { path });
}

/** Optional git unified diff for a project file (session Changes panel). */
export interface GitFileDiffResult {
  available: boolean;
  diff?: string | null;
  relativePath?: string | null;
  reason?: string | null;
}

export async function gitFileDiff(projectPath: string, path: string) {
  return invoke<GitFileDiffResult>("git_file_diff", { projectPath, path });
}

/** One workspace file from `git status --porcelain` (Changes → Workspace). */
export interface GitStatusEntry {
  path: string;
  absolutePath: string;
  status: string;
  indexStatus: string;
  worktreeStatus: string;
  kind: string;
  name: string;
  originalPath?: string | null;
}

export interface GitStatusResult {
  available: boolean;
  files: GitStatusEntry[];
  branch?: string | null;
  reason?: string | null;
}

/** Soft-fail workspace git status for the project path. */
export async function gitStatus(projectPath: string) {
  return invoke<GitStatusResult>("git_status", { projectPath });
}

/** File content at HEAD (before snapshot for local unified diffs). */
export interface GitShowFileResult {
  available: boolean;
  content?: string | null;
  relativePath?: string | null;
  reason?: string | null;
}

export async function gitShowFile(projectPath: string, path: string) {
  return invoke<GitShowFileResult>("git_show_file", { projectPath, path });
}

export interface FsEntry {
  name: string;
  relativePath: string;
  isDir: boolean;
  size: number;
  ext: string;
}

export interface FsReadResult {
  relativePath: string;
  name: string;
  /** Absolute path for convertFileSrc streaming (video/audio/large images). */
  absolutePath: string;
  size: number;
  kind: string;
  mime: string;
  text: string | null;
  base64: string | null;
  /** Prefer asset-protocol stream instead of base64 embed. */
  stream: boolean;
  truncated: boolean;
  error: string | null;
  /** Last modified (ms since epoch) for edit conflict checks. */
  mtimeMs?: number;
  /** Opaque token for resource:// preview streaming. */
  resourceHandleId?: string;
}

export interface ResourceHandleV1 {
  version: 1;
  id: string;
  name: string;
  size: number;
  origin:
    | "trusted_project"
    | "app_attachment"
    | "session_artifact"
    | "message_attachment"
    | "user_selected";
  expiresAt: string;
}

export interface FsWriteResult {
  relativePath: string;
  absolutePath: string;
  size: number;
  mtimeMs: number;
}

/** List directory under a trusted project root (relative path, "" = root). */
export async function fsListDir(projectPath: string, relative = "") {
  return invoke<FsEntry[]>("fs_list_dir", {
    projectPath,
    relative: relative || null,
  });
}

/** Read file under project root for preview (text or base64). */
export async function fsReadFile(projectPath: string, relative: string) {
  return invoke<FsReadResult>("fs_read_file", {
    projectPath,
    relative,
  });
}

/** Save UTF-8 text under project root. Pass mtime from last read to detect conflicts. */
export async function fsWriteFile(
  projectPath: string,
  relative: string,
  content: string,
  expectedMtimeMs?: number | null,
) {
  return invoke<FsWriteResult>("fs_write_file", {
    projectPath,
    relative,
    content,
    expectedMtimeMs: expectedMtimeMs ?? null,
  });
}

/** Save UTF-8 text to an absolute path open in the resource pane. */
export async function fsWriteAbsolute(
  path: string,
  content: string,
  expectedMtimeMs?: number | null,
) {
  return invoke<FsWriteResult>("fs_write_absolute", {
    path,
    content,
    expectedMtimeMs: expectedMtimeMs ?? null,
  });
}

/** Read absolute filesystem path for chat → resource pane preview. */
export async function fsReadAbsolute(path: string) {
  return invoke<FsReadResult>("fs_read_absolute", { path });
}

export async function resourceOpenV1(path: string) {
  return invoke<ResourceHandleV1>("resource_open_v1", { path });
}

export async function resourceReadV1(handleId: string) {
  return invoke<FsReadResult>("resource_read_v1", { handleId });
}

/**
 * Smart open for chat file cards: absolute path, project-relative, or
 * suffix search under project (e.g. `05-handoff/next.md` in a subfolder).
 */
export async function fsOpenPath(path: string, projectPath?: string | null) {
  return invoke<FsReadResult>("fs_open_path", {
    path,
    projectPath: projectPath ?? null,
  });
}

/** Auto-title session from first user message (heuristic + optional low-effort CLI). */
export async function sessionAutoTitle(id: string, firstMessage: string) {
  return invoke<{
    id: string;
    title: string;
    projectId: string | null;
    updatedAt: string;
  }>("session_auto_title", { id, firstMessage });
}

export async function projectTrust(id: string) {
  return invoke("project_trust", { id });
}

/**
 * Set or clear a project-level permission tier (L10).
 * Pass `null` / `"inherit"` to fall back to the app default.
 * When the project is the live Host context, agent policy is synced.
 */
export async function projectSetPermissionPolicy(
  id: string,
  policy: string | null,
) {
  return invoke("project_set_permission_policy", {
    id,
    policy,
  });
}

/** Remove project from app list only (no disk / session wipe). */
export async function projectRemove(id: string) {
  return invoke("project_remove", { id });
}

export async function projectRename(id: string, name: string) {
  return invoke("project_rename", { id, name });
}

export async function projectSetPath(id: string, path: string) {
  return invoke("project_set_path", { id, path });
}

export async function projectSetPinned(id: string, pinned: boolean) {
  return invoke("project_set_pinned", { id, pinned });
}

export async function projectReveal(id: string) {
  return invoke("project_reveal", { id });
}

export async function projectArchiveSessions(id: string) {
  return invoke<number>("project_archive_sessions", { id });
}

export async function sessionsList() {
  return invoke<
    Array<{
      id: string;
      projectId: string | null;
      title: string;
      updatedAt: string;
      modelId: string | null;
      contextUsage?: import("./session").SessionTokenUsage | null;
      archived?: boolean;
      /** Shell automation run */
      scheduled?: boolean;
    }>
  >("sessions_list");
}

export interface SessionPreviewV1 {
  version: 1;
  sessionId: string;
  projectId: string | null;
  title: string;
  updatedAt: string;
  modelId: string | null;
  contextUsage?: import("./session").SessionTokenUsage | null;
  archived: boolean;
  scheduled: boolean;
  recentUserSummary: string | null;
  recentAssistantSummary: string | null;
}

/** Bounded, redacted sidebar preview; the browser fallback has no journal. */
export async function sessionPreview(
  id: string,
): Promise<SessionPreviewV1 | null> {
  if (!isTauri()) return null;
  return invoke<SessionPreviewV1>("session_preview", { id });
}

/** CLI sessions under GROK_HOME (shared-mode discovery). */
export type CliSessionSummary = {
  agentSessionId: string;
  title: string;
  cwd: string | null;
  updatedAt: string;
  dir: string;
  numMessages: number;
  alreadyLinked: boolean;
};

export async function cliSessionsList() {
  return invoke<CliSessionSummary[]>("cli_sessions_list");
}

export async function cliSessionImport(
  agentSessionId: string,
  opts?: { dir?: string | null; projectId?: string | null },
) {
  return invoke<{
    id: string;
    title: string;
    projectId: string | null;
    updatedAt: string;
  }>("cli_session_import", {
    agentSessionId,
    dir: opts?.dir ?? null,
    projectId: opts?.projectId ?? null,
  });
}

export async function cliSessionsImportAll(limit?: number) {
  return invoke<
    Array<{
      id: string;
      title: string;
      projectId: string | null;
      updatedAt: string;
    }>
  >("cli_sessions_import_all", { limit: limit ?? 50 });
}

export async function sessionCreate(
  projectId?: string,
  title?: string,
  opts?: { scheduled?: boolean },
) {
  return invoke("session_create", {
    projectId: projectId ?? null,
    title: title ?? null,
    scheduled: opts?.scheduled ?? false,
  });
}

export async function sessionSetScheduled(id: string, scheduled: boolean) {
  return invoke<{
    id: string;
    title: string;
    scheduled?: boolean;
  }>("session_set_scheduled", { id, scheduled });
}

export async function sessionRename(id: string, title: string) {
  return invoke("session_rename", { id, title });
}

export async function sessionSetArchived(id: string, archived: boolean) {
  return invoke("session_set_archived", { id, archived });
}

/** Bind session to a project, or clear (`projectId: null`) for orphan / 其他会话. */
export async function sessionSetProject(
  id: string,
  projectId: string | null,
) {
  return invoke<{
    id: string;
    projectId: string | null;
    title: string;
  }>("session_set_project", { id, projectId });
}

export async function sessionDelete(id: string) {
  return invoke("session_delete", { id });
}

export async function sessionMessages(id: string) {
  return invoke<
    Array<{
      id: string;
      role: string;
      content: string;
      thought?: string | null;
      createdAt: string;
      isError?: boolean;
      marker?: string | null;
      attachments?: Array<{
        path: string;
        name: string;
        isDir?: boolean;
      }> | null;
    }>
  >("session_messages", { id });
}

export interface SessionSearchResultV1 {
  version: 1;
  sessionId: string;
  sessionTitle: string;
  messageId: string;
  role: string;
  snippet: string;
  rank: number;
}

export interface SessionSearchRebuildResultV1 {
  version: 1;
  sessions: number;
  messages: number;
}

/** Search visible JSON-journal messages through the disposable FTS5 cache. */
export async function sessionSearchV1(query: string, limit = 40) {
  return invoke<SessionSearchResultV1[]>("session_search_v1", { query, limit });
}

export async function sessionSearchRebuildV1() {
  return invoke<SessionSearchRebuildResultV1>("session_search_rebuild_v1");
}

export async function sessionSearchDeleteIndexV1() {
  return invoke<void>("session_search_delete_index_v1");
}

/** Agent session folder under GROK_HOME (contains images/, etc.). */
export async function sessionMediaRoot(id: string) {
  return invoke<string | null>("session_media_root", { id });
}

/**
 * Resolve short session-relative paths (`images/1.jpg`) to absolute files
 * that exist under the agent session directory.
 */
export async function sessionResolveRelativeMedia(
  id: string,
  relatives: string[],
) {
  if (!relatives.length) return [];
  return invoke<
    Array<{ path: string; name: string; isDir?: boolean }>
  >("session_resolve_relative_media", { id, relatives });
}

export type ComposerPrefsScope = "global" | "project" | "session";

export interface AppSettings {
  theme: string;
  locale: string;
  sessionDataMode: string;
  manualCliPath: string | null;
  permissionPolicy: string;
  modelId: string | null;
  effort: string | null;
  mode: string;
  onboardingDone: boolean;
  setupSkipped: boolean;
  /** First-run wizard finished (CLI gate + optional auth). */
  setupWizardCompleted?: boolean;
  /** User skipped account/provider step during setup. */
  authSetupDeferred?: boolean;
  defaultOpenTarget?: string;
  /** global | project | session — where model/permission chips are remembered */
  composerPrefsScope?: ComposerPrefsScope | string;
  /** API mode: `host:port` of a remote ACP server. When set, sessions connect
   *  over TCP instead of spawning the local CLI. Empty/unset = local spawn. */
  acpServerAddr?: string | null;
  /** Max warm/live agent processes (default 3). */
  maxConcurrentAgents?: number;
  /** Recycle idle agent processes after N minutes (default 30). */
  agentIdleMinutes?: number;
  /** Pure stream silence before cancel prompt, seconds (default 120). */
  streamStallSeconds?: number;
  /** Runtime subprocess sandbox; defaults to off. */
  sandboxProfile?: SandboxProfileV1 | string;
  /**
   * When true, App API keys go in the OS keychain.
   * Default false: keys stay in secrets.json (0600). Official login uses auth.json.
   */
  storeApiKeysInKeychain?: boolean;
  /** Product kernel: `sunsetz` (default) or legacy `grok_acp`. */
  runtimeBackend?: string;
  /** Register login/interval OS job that starts Sunsetz with `--background`. */
  runScheduledTasksInBackground?: boolean;
}

export interface AvailableModel {
  id: string;
  label: string;
  source: string;
  isDefault?: boolean;
  capabilities?: {
    reasoningEfforts?: Array<"low" | "medium" | "high">;
  };
}

export interface AvailableModelsResult {
  models: AvailableModel[];
  defaultModelId: string;
  origin?: string | null;
  fetchedAt?: string | null;
}

export interface ComposerPrefs {
  modelId: string;
  effort: string;
  mode: string;
  permissionPolicy: string;
  scope: string;
  source: string;
}

export async function settingsGet() {
  return invoke<AppSettings>("settings_get");
}

export async function modelsListAvailable() {
  return invoke<AvailableModelsResult>("models_list_available");
}

export async function composerPrefsResolve(opts?: {
  projectId?: string | null;
  sessionId?: string | null;
}) {
  return invoke<ComposerPrefs>("composer_prefs_resolve", {
    projectId: opts?.projectId ?? null,
    sessionId: opts?.sessionId ?? null,
  });
}

export async function composerPrefsSet(body: {
  projectId?: string | null;
  sessionId?: string | null;
  modelId?: string | null;
  effort?: string | null;
  mode?: string | null;
  permissionPolicy?: string | null;
}) {
  return invoke<ComposerPrefs>("composer_prefs_set", {
    projectId: body.projectId ?? null,
    sessionId: body.sessionId ?? null,
    modelId: body.modelId ?? null,
    effort: body.effort ?? null,
    mode: body.mode ?? null,
    permissionPolicy: body.permissionPolicy ?? null,
  });
}

export async function settingsSet(settings: Record<string, unknown>) {
  return invoke("settings_set", { settings });
}

/** Atomically update explicit settings fields without replacing concurrent edits. */
export async function settingsPatchV1(patch: Partial<AppSettings>) {
  return invoke<AppSettings>("settings_patch_v1", { patch });
}

/** Update live Host permission policy + persist at configured prefs scope. */
export async function sessionSetPolicy(
  policy: string,
  opts?: { projectId?: string | null; sessionId?: string | null },
) {
  if (!isTauri()) return null;
  return invoke<ComposerPrefs>("session_set_policy", {
    policy,
    projectId: opts?.projectId ?? null,
    sessionId: opts?.sessionId ?? null,
  });
}

/** Switch live agent model + persist at configured prefs scope. */
export async function sessionSetModel(
  modelId: string,
  opts?: { projectId?: string | null; sessionId?: string | null },
) {
  if (!isTauri()) return null;
  return invoke<ComposerPrefs>("session_set_model", {
    modelId,
    projectId: opts?.projectId ?? null,
    sessionId: opts?.sessionId ?? null,
  });
}

export async function secretsGetMasked() {
  return invoke<{
    hasOfficialKey: boolean;
    hasRelayKey: boolean;
    relayBaseUrl: string | null;
    defaultModel: string | null;
  }>("secrets_get_masked");
}

export async function secretsSet(body: {
  officialApiKey?: string;
  relayBaseUrl?: string;
  relayApiKey?: string;
  defaultModel?: string;
}) {
  return invoke("secrets_set", {
    officialApiKey: body.officialApiKey ?? null,
    relayBaseUrl: body.relayBaseUrl ?? null,
    relayApiKey: body.relayApiKey ?? null,
    defaultModel: body.defaultModel ?? null,
  });
}

export async function providerPing() {
  return invoke<{ ok: boolean; class: string; message: string }>("provider_ping");
}

export async function importGrokCli() {
  return invoke("import_grok_cli_config");
}

export async function importLegacyProviderConfig() {
  return invoke("import_grok_go_config");
}

// ── Doctor / skills / MCP ───────────────────────────────────────────────────

export type DoctorLevel = "ok" | "warn" | "fail";

export interface DoctorCheck {
  id: string;
  level: DoctorLevel;
  title: string;
  detail: string;
  meta?: Record<string, unknown>;
}

export interface DoctorSummary {
  ok: number;
  warn: number;
  fail: number;
}

export interface DoctorReport {
  generatedAt: string;
  summary: DoctorSummary;
  checks: DoctorCheck[];
  /** Flat snapshot for copy/export (no secrets). */
  raw: Record<string, unknown>;
}

export interface SkillDto {
  name: string;
  description: string;
  /** Normalized source type (e.g. user, project, plugin). */
  source: string;
  path?: string | null;
  userInvocable: boolean;
  /** App Extensions enable flag (default true when omitted). */
  enabled?: boolean;
}

export interface McpDto {
  name: string;
  transport?: string | null;
  target?: string | null;
  vendor?: string | null;
  compatibilityStatus?: string | null;
  /** App Extensions enable flag (default true when omitted). */
  enabled?: boolean;
}

export interface SkillsListResult {
  skills: SkillDto[];
  error?: string;
}

export interface SkillMetadataInventoryItemV1 {
  id: string;
  name: string;
  description: string;
  whenToUse: string;
  source: "user" | "project" | "plugin";
  treeHash: string;
  sourceCandidateId?: string | null;
  userInvocable: boolean;
  enabled: boolean;
}

export interface SkillMetadataInventoryV1 {
  version: 1;
  items: SkillMetadataInventoryItemV1[];
}

export interface SkillMetadataRankingResultV1 {
  version: 1;
  disposition: "suggestion_only";
  requiresExplicitAcceptance: true;
  items: Array<{
    skill: SkillIdentityV1;
    score: number;
    matchedTerms: string[];
  }>;
}

export interface SkillImprovementProposalV1 {
  version: 1;
  id: string;
  status: "pending_review";
  owner: {
    kind: "host_candidate";
    namespace: string;
    mayOverwriteExternal: false;
  };
  skillId: string;
  skillName: string;
  lineage: {
    priorSkillTreeHash: string;
    sourceCandidateId?: string | null;
    evidenceUseIds: string[];
  };
  successfulUseCount: number;
  helpfulUseCount: number;
  repeatedEvidence: boolean;
  requiresUserReview: true;
  mayWriteSkill: false;
}

export interface InspectMcpResult {
  servers: McpDto[];
  error?: string;
}

/** App MCP/Skills enable prefs (`extensions.json`). Missing name = enabled. */
export interface ExtensionsPrefs {
  mcp: Record<string, boolean>;
  skills: Record<string, boolean>;
}

export async function extensionsGet() {
  return invoke<ExtensionsPrefs>("extensions_get");
}

/** Toggle one MCP server; Host persists + injects on next session + soft-respawns. */
export async function extensionsSetMcp(name: string, enabled: boolean) {
  return invoke<ExtensionsPrefs>("extensions_set_mcp", { name, enabled });
}

/** Toggle one skill (slash palette filter). */
export async function extensionsSetSkill(name: string, enabled: boolean) {
  return invoke<ExtensionsPrefs>("extensions_set_skill", { name, enabled });
}

/** Bulk-enable all listed MCP servers. */
export async function extensionsEnableAllMcp(names: string[]) {
  return invoke<ExtensionsPrefs>("extensions_enable_all_mcp", { names });
}

/** Bulk-enable all listed skills. */
export async function extensionsEnableAllSkills(names: string[]) {
  return invoke<ExtensionsPrefs>("extensions_enable_all_skills", { names });
}

export async function doctorReport() {
  return invoke<DoctorReport>("doctor_report");
}

export interface SupportBundleResult {
  ok: boolean;
  path: string;
}

/** Build a redacted support zip (Doctor + logs) and save via native dialog. */
export async function exportSupportBundle(doctorJson?: string | null) {
  return invoke<SupportBundleResult>("export_support_bundle", {
    doctorJson: doctorJson ?? null,
  });
}

/**
 * Full session diagnostic zip for bug reports: messages, meta, settings,
 * CLI probe, agent trail (events/history/terminal logs), optional runtime snapshot.
 * Secrets are redacted. Opens a native save dialog.
 */
export async function exportSessionBundle(sessionId: string) {
  return invoke<SupportBundleResult>("export_session_bundle", {
    sessionId,
  });
}

export interface ResetAppDataResult {
  ok: boolean;
  dataRoot: string;
  removed: string[];
  keptSecrets: boolean;
}

/**
 * Wipe App data under the data root.
 * Does not touch ~/.grok. Confirm twice in the UI before calling.
 */
export async function resetAppData(keepSecrets = true) {
  return invoke<ResetAppDataResult>("reset_app_data", {
    keepSecrets,
  });
}

/** List skills via `grok inspect --json` (optional project cwd). */
export async function skillsList(projectPath?: string | null) {
  return invoke<SkillsListResult>("skills_list", {
    projectPath: projectPath ?? null,
  });
}

export async function skillInventoryV1(projectPath?: string | null) {
  return invoke<SkillMetadataInventoryV1>("skill_inventory_v1", {
    projectPath: projectPath ?? null,
  });
}

export async function skillMetadataRankV1(
  query: string,
  projectPath?: string | null,
  maxResults = 4,
) {
  return invoke<SkillMetadataRankingResultV1>("skill_metadata_rank_v1", {
    projectPath: projectPath ?? null,
    query,
    maxResults,
  });
}

export async function skillUsesListV1(sessionId?: string | null) {
  return invoke<SkillUseRecordV1[]>("skill_uses_list_v1", {
    sessionId: sessionId ?? null,
  });
}

export async function skillUseFeedbackV1(
  record: SkillUseRecordV1,
  feedback: SkillUseFeedbackRatingV1,
) {
  return invoke<SkillUseRecordV1>("skill_use_feedback_v1", {
    request: {
      version: 1,
      id: record.id,
      expectedRevision: record.revision,
      expectedSkillTreeHash: record.skill.treeHash,
      feedback,
    },
  });
}

export async function skillImprovementProposalV1(
  skill: SkillIdentityV1,
  projectPath?: string | null,
) {
  return invoke<SkillImprovementProposalV1 | null>(
    "skill_improvement_proposal_v1",
    {
      projectPath: projectPath ?? null,
      request: {
        version: 1,
        skillId: skill.id,
        expectedSkillTreeHash: skill.treeHash,
      },
    },
  );
}

/** List MCP servers via `grok inspect --json` (optional project cwd). */
export async function inspectMcp(projectPath?: string | null) {
  return invoke<InspectMcpResult>("inspect_mcp", {
    projectPath: projectPath ?? null,
  });
}

// ── Plugins via `grok plugin …` ─────────────────────────────────────────────

/** Component counts from `grok inspect` plugins[].provides — Grok Build shape. */
export interface PluginProvidesDto {
  skills: number;
  agents: number;
  hooks: boolean;
  mcpServers: number;
}

export interface PluginDto {
  name: string;
  version?: string | null;
  source?: string | null;
  marketplace?: string | null;
  path?: string | null;
  /** Install status from `plugin list --json` (usually "installed"). */
  status: string;
  /** Load state from Grok Build config / enable|disable CLI. */
  enabled: boolean;
  repoKey?: string | null;
  /** Grok Build scope: user / project / cli / marketplace name. */
  scope?: string | null;
  provides?: PluginProvidesDto | null;
}

export interface PluginsListResult {
  plugins: PluginDto[];
  error?: string;
}

export interface RuntimePluginCatalogV1 {
  version: 1;
  source: "runtime_cli" | string;
  installActionAvailable: false;
  uninstallActionAvailable?: boolean;
  actionUnavailableReason?: string | null;
  plugins: PluginDto[];
  error?: string | null;
}

export interface RuntimeHookInventoryItemV1 {
  version: 1;
  pluginName: string;
  source: "runtime_inspect" | string;
  path?: string | null;
}

export interface PluginActionResult {
  ok: boolean;
  name: string;
  message?: string;
}

export interface PluginDetailsResult {
  name: string;
  details: string;
}

/** List installed plugins via `grok plugin list --json`. */
export async function pluginsList() {
  return invoke<PluginsListResult>("plugins_list");
}

export async function runtimePluginsCatalogV1(query?: string | null) {
  return invoke<RuntimePluginCatalogV1>("runtime_plugins_catalog_v1", {
    query: query?.trim() || null,
  });
}

export async function runtimeHooksInventoryV1() {
  return invoke<RuntimeHookInventoryItemV1[]>("runtime_hooks_inventory_v1");
}

/** Enable plugin (`grok plugin enable`) and soft-respawn agent. */
export async function pluginEnable(name: string) {
  return invoke<PluginActionResult>("plugin_enable", { name });
}

/** Disable plugin (`grok plugin disable`) and soft-respawn agent. */
export async function pluginDisable(name: string) {
  return invoke<PluginActionResult>("plugin_disable", { name });
}

/** Legacy compatibility route. Current Host fails closed until Runtime exposes a safe target contract. */
export async function pluginUninstall(name: string) {
  return invoke<PluginActionResult>("plugin_uninstall", { name });
}

/** Plugin component inventory text (`grok plugin details`). */
export async function pluginDetails(name: string) {
  return invoke<PluginDetailsResult>("plugin_details", { name });
}

// ── Official Grok Build account ─────────────────────────────────────────────

export interface AccountProfile {
  signedIn: boolean;
  authMode: string | null;
  email: string | null;
  displayName: string | null;
  userId: string | null;
  teamId: string | null;
  principalType: string | null;
  expiresAt: string | null;
  expired: boolean;
  hasRefresh: boolean;
  oidcIssuer: string | null;
}

export interface QuotaProduct {
  productId: number;
  label: string;
  usedPercent: number;
}

export interface BillingSnapshot {
  available: boolean;
  source: string;
  message: string | null;
  subscriptionTier: string | null;
  creditUsagePercent: number | null;
  remainingPercent: number | null;
  monthlyLimit: number | null;
  includedUsed: number | null;
  totalUsed: number | null;
  prepaidBalance: number | null;
  onDemandEnabled: boolean | null;
  onDemandCap: number | null;
  onDemandUsed: number | null;
  billingPeriodStart: string | null;
  billingPeriodEnd: string | null;
  resetsAt: string | null;
  isUnifiedBillingUser: boolean | null;
  products: QuotaProduct[];
  manageUrl: string;
  subscribeUrl: string;
  fetchedAt: string | null;
}

export interface HeatmapDay {
  date: string;
  requests: number;
  tokens: number;
  costUsd: number;
}

export interface CallLogEntry {
  id: string;
  title: string;
  model: string | null;
  projectPath: string | null;
  startedAt: string | null;
  durationSecs: number | null;
  turns: number;
  toolCalls: number;
  contextTokens: number;
  errors: number;
}

export interface AccountStatus {
  profile: AccountProfile;
  hasOfficialKey: boolean;
  hasRelayKey: boolean;
  relayBaseUrl: string | null;
  cliAuthPresent: boolean;
  cliFound: boolean;
  cliPath: string | null;
  channel: string;
  billing: BillingSnapshot;
  heatmap: HeatmapDay[];
  callLogs: CallLogEntry[];
  usageManageUrl: string;
  subscribeUrl: string;
}

export interface LoginResult {
  ok: boolean;
  method: string;
  message: string;
  deviceUrl: string | null;
  deviceCode: string | null;
  profile: AccountProfile | null;
}

export async function accountStatus(opts?: {
  refreshBilling?: boolean;
  manualCliPath?: string | null;
}) {
  if (!isTauri()) {
    return {
      profile: {
        signedIn: false,
        authMode: null,
        email: null,
        displayName: null,
        userId: null,
        teamId: null,
        principalType: null,
        expiresAt: null,
        expired: false,
        hasRefresh: false,
        oidcIssuer: null,
      },
      hasOfficialKey: false,
      hasRelayKey: false,
      relayBaseUrl: null,
      cliAuthPresent: false,
      cliFound: false,
      cliPath: null,
      channel: "none",
      billing: {
        available: false,
        source: "browser",
        message: "Account requires Tauri desktop runtime",
        subscriptionTier: null,
        creditUsagePercent: null,
        remainingPercent: null,
        monthlyLimit: null,
        includedUsed: null,
        totalUsed: null,
        prepaidBalance: null,
        onDemandEnabled: null,
        onDemandCap: null,
        onDemandUsed: null,
        billingPeriodStart: null,
        billingPeriodEnd: null,
        resetsAt: null,
        isUnifiedBillingUser: null,
        products: [],
        manageUrl: RUNTIME_USAGE_URL,
        subscribeUrl: RUNTIME_SUBSCRIBE_URL,
        fetchedAt: null,
      },
      heatmap: [],
      callLogs: [],
      usageManageUrl: RUNTIME_USAGE_URL,
      subscribeUrl: RUNTIME_SUBSCRIBE_URL,
    } satisfies AccountStatus;
  }
  return invoke<AccountStatus>("account_status", {
    refreshBilling: opts?.refreshBilling ?? true,
    manualCliPath: opts?.manualCliPath ?? null,
  });
}

export async function accountLogin(method: "oauth" | "device" = "oauth") {
  return invoke<LoginResult>("account_login", { method });
}

/** Abort a running `grok login` (OAuth / device-code). No-op if none is running. */
export async function accountLoginCancel() {
  return invoke<void>("account_login_cancel");
}

export async function accountLogout() {
  return invoke<AccountProfile>("account_logout");
}

export async function accountOpenUsage() {
  if (!isTauri()) {
    window.open(RUNTIME_USAGE_URL, "_blank");
    return;
  }
  return invoke<void>("account_open_usage");
}

export async function accountOpenSubscribe() {
  if (!isTauri()) {
    window.open(RUNTIME_SUBSCRIBE_URL, "_blank");
    return;
  }
  return invoke<void>("account_open_subscribe");
}

// ── Multi-account switcher ──────────────────────────────────────────────────

export interface SavedAccount {
  id: string;
  email?: string | null;
  displayName?: string | null;
  label: string;
  updatedAt: string;
}

export interface AccountsListResult {
  profiles: SavedAccount[];
  activeId?: string | null;
}

export async function accountsList() {
  return invoke<AccountsListResult>("accounts_list");
}

export async function accountSaveCurrent(label?: string | null) {
  return invoke<SavedAccount>("account_save_current", {
    label: label ?? null,
  });
}

export async function accountSwitch(id: string) {
  return invoke<AccountProfile>("account_switch", { id });
}

export async function accountRemove(id: string) {
  return invoke<void>("account_remove", { id });
}

export async function accountRename(id: string, label: string) {
  return invoke<SavedAccount>("account_rename", { id, label });
}

/** Import markdown/JSON transcript as a new local session. */
export async function sessionImportTranscript(
  text: string,
  title?: string | null,
  projectId?: string | null,
) {
  return invoke<{
    id: string;
    title: string;
    projectId?: string | null;
  }>("session_import_transcript", {
    text,
    title: title ?? null,
    projectId: projectId ?? null,
  });
}

/** Native file picker → import transcript. Returns null if cancelled. */
export async function sessionImportTranscriptFile(
  title?: string | null,
  projectId?: string | null,
) {
  return invoke<{
    id: string;
    title: string;
    projectId?: string | null;
  } | null>("session_import_transcript_file", {
    title: title ?? null,
    projectId: projectId ?? null,
  });
}

/** Rebuild system-tray / menu-bar menu (Recent list + Usage). */
export async function trayRefresh() {
  if (!isTauri()) return;
  return invoke<void>("tray_refresh");
}

// ── Custom providers (agent-home config.toml) ───────────────────────────────

export interface CustomProvider {
  id: string;
  model: string;
  baseUrl: string;
  name: string;
  hasApiKey: boolean;
  apiBackend: string;
  isDefault: boolean;
}

export interface ProvidersListResult {
  providers: CustomProvider[];
  defaultModel: string | null;
  /** `official` | `custom` */
  activeSource: string;
  activeProviderId: string | null;
  configPath: string;
  agentHome: string;
}

export async function providersList() {
  return invoke<ProvidersListResult>("providers_list");
}

export interface ConnectorStateV1 {
  id: string;
  slug: string;
  developer: string;
  version: string;
  enabled: boolean;
  connected: boolean;
  lastError: string | null;
  tools: string[];
}

export async function connectorsList() {
  return invoke<ConnectorStateV1[]>("connectors_list");
}

export async function connectorsConnect(id: string, credential?: string | null) {
  return invoke<ConnectorStateV1>("connectors_connect", {
    id,
    credential: credential ?? null,
  });
}

export async function connectorsDisconnect(id: string) {
  return invoke<ConnectorStateV1>("connectors_disconnect", { id });
}

/** Switch to official Grok Build or a custom provider (writes config.toml default). */
export async function providersActivate(
  source: "official" | "custom",
  providerId?: string | null,
) {
  return invoke<ProvidersListResult>("providers_activate", {
    source,
    providerId: providerId ?? null,
  });
}

export async function providersUpsert(body: {
  id: string;
  model: string;
  baseUrl: string;
  name?: string;
  apiKey?: string;
  apiBackend?: string;
  setAsDefault?: boolean;
  createOnly?: boolean;
}) {
  return invoke<ProvidersListResult>("providers_upsert", {
    id: body.id,
    model: body.model,
    baseUrl: body.baseUrl,
    name: body.name ?? null,
    apiKey: body.apiKey ?? null,
    apiBackend: body.apiBackend ?? null,
    setAsDefault: body.setAsDefault ?? null,
    createOnly: body.createOnly ?? null,
  });
}

export async function providersRemove(id: string) {
  return invoke<ProvidersListResult>("providers_remove", { id });
}

export async function providersSetDefault(modelId: string) {
  return invoke<ProvidersListResult>("providers_set_default", { modelId });
}

export async function providersPing(opts?: {
  baseUrl?: string;
  apiKey?: string;
  providerId?: string;
}) {
  return invoke<{
    ok: boolean;
    latencyMs: number;
    endpoint: string;
    status?: number;
    error?: string;
    diagnostic?: string;
  }>("providers_ping", {
    baseUrl: opts?.baseUrl ?? null,
    apiKey: opts?.apiKey ?? null,
    providerId: opts?.providerId ?? null,
  });
}

export async function providersListModels(opts: {
  baseUrl: string;
  apiKey?: string;
  providerId?: string;
}) {
  return invoke<{
    endpoint: string;
    models: Array<{ id: string; ownedBy?: string }>;
  }>("providers_list_models", {
    baseUrl: opts.baseUrl,
    apiKey: opts.apiKey ?? null,
    providerId: opts.providerId ?? null,
  });
}

// ── Editors ─────────────────────────────────────────────────────────────────

export interface DetectedEditor {
  id: string;
  label: string;
  command: string;
  available: boolean;
  /** `data:image/png;base64,...` from host-extracted app icon when available. */
  iconDataUrl?: string | null;
}

export interface EditorsListResult {
  editors: DetectedEditor[];
  finderIcon?: string | null;
  systemIcon?: string | null;
}

export async function editorsList() {
  return invoke<EditorsListResult>("editors_list");
}

export async function openInEditor(opts: {
  path: string;
  line?: number;
  editor?: string;
}) {
  return invoke<void>("open_in_editor", {
    path: opts.path,
    line: opts.line ?? null,
    editor: opts.editor ?? null,
  });
}

export async function listen<T>(
  event: string,
  handler: (payload: T) => void,
): Promise<() => void> {
  if (!isTauri()) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  const un = await listen<T>(event, (e) => handler(e.payload));
  return un;
}

// ── Automations (scheduled tasks) ───────────────────────────────────────────

export interface AutomationDto {
  id: string;
  title: string;
  prompt: string;
  enabled: boolean;
  projectId: string | null;
  modelId: string | null;
  effort: string | null;
  frequency: string;
  time: string;
  weekdays: number[];
  notify: string;
  missedRunPolicy: "skip" | "run_once" | string;
  createdAt: string;
  updatedAt: string;
  lastRunAt?: string | null;
  nextRunAt?: string | null;
  intervalMinutes?: number | null;
  skillIds?: Array<{ id: string; treeHash: string }>;
}

export interface AutomationClaimV1 {
  version: 1;
  claimId: string;
  scheduledFor: string;
  catchUp: boolean;
  sessionId?: string | null;
  automation: AutomationDto;
}

export interface AutomationInputDto {
  title: string;
  prompt: string;
  enabled?: boolean;
  projectId?: string | null;
  modelId?: string | null;
  effort?: string | null;
  frequency?: string;
  time?: string;
  weekdays?: number[];
  notify?: string;
  missedRunPolicy?: "skip" | "run_once";
  nextRunAt?: string | null;
  intervalMinutes?: number | null;
  skillIds?: Array<{ id: string; treeHash: string }>;
}

export async function automationsList(): Promise<AutomationDto[]> {
  if (!isTauri()) {
    const { loadAutomationsLocal } = await import("./automations");
    return loadAutomationsLocal() as AutomationDto[];
  }
  return invoke<AutomationDto[]>("automations_list");
}

export async function automationCreate(
  input: AutomationInputDto,
): Promise<AutomationDto> {
  if (!isTauri()) {
    const mod = await import("./automations");
    const list = mod.loadAutomationsLocal();
    const now = new Date().toISOString();
    const draft = {
      id: crypto.randomUUID(),
      title: input.title.trim(),
      prompt: input.prompt.trim(),
      enabled: input.enabled ?? true,
      projectId: input.projectId ?? null,
      modelId: input.modelId ?? null,
      effort: input.effort ?? null,
      frequency: input.frequency ?? "daily",
      time: input.time ?? "09:00",
      weekdays: input.weekdays ?? [],
      notify: input.notify ?? "all",
      missedRunPolicy: input.missedRunPolicy ?? "run_once",
      createdAt: now,
      updatedAt: now,
      lastRunAt: null as string | null,
      nextRunAt:
        input.nextRunAt ??
        mod.computeNextRunAt({
          frequency: input.frequency ?? "daily",
          time: input.time ?? "09:00",
          weekdays: input.weekdays ?? [],
          enabled: input.enabled ?? true,
        }),
    };
    list.unshift(draft);
    mod.saveAutomationsLocal(list);
    return draft as AutomationDto;
  }
  return invoke<AutomationDto>("automation_create", { input });
}

export async function automationUpdate(
  id: string,
  input: AutomationInputDto,
): Promise<AutomationDto> {
  if (!isTauri()) {
    const {
      loadAutomationsLocal,
      saveAutomationsLocal,
      computeNextRunAt,
    } = await import("./automations");
    const list = loadAutomationsLocal();
    const idx = list.findIndex((a) => a.id === id);
    if (idx < 0) throw new Error("automation not found");
    const prev = list[idx];
    const next = {
      ...prev,
      title: input.title.trim(),
      prompt: input.prompt.trim(),
      enabled: input.enabled ?? prev.enabled,
      projectId: input.projectId !== undefined ? input.projectId : prev.projectId,
      modelId: input.modelId !== undefined ? input.modelId : prev.modelId,
      effort: input.effort !== undefined ? input.effort : prev.effort,
      frequency: input.frequency ?? prev.frequency,
      time: input.time ?? prev.time,
      weekdays: input.weekdays ?? prev.weekdays,
      notify: input.notify ?? prev.notify,
      missedRunPolicy: input.missedRunPolicy ?? prev.missedRunPolicy ?? "run_once",
      updatedAt: new Date().toISOString(),
      nextRunAt:
        input.nextRunAt !== undefined
          ? input.nextRunAt
          : computeNextRunAt({
              frequency: input.frequency ?? prev.frequency,
              time: input.time ?? prev.time,
              weekdays: input.weekdays ?? prev.weekdays,
              enabled: input.enabled ?? prev.enabled,
            }),
    };
    list[idx] = next;
    saveAutomationsLocal(list);
    return next as AutomationDto;
  }
  return invoke<AutomationDto>("automation_update", { id, input });
}

export async function automationSetEnabled(
  id: string,
  enabled: boolean,
): Promise<AutomationDto> {
  if (!isTauri()) {
    const { loadAutomationsLocal, saveAutomationsLocal, computeNextRunAt } =
      await import("./automations");
    const list = loadAutomationsLocal();
    const idx = list.findIndex((a) => a.id === id);
    if (idx < 0) throw new Error("automation not found");
    const prev = list[idx];
    const next = {
      ...prev,
      enabled,
      updatedAt: new Date().toISOString(),
      nextRunAt: enabled
        ? computeNextRunAt({ ...prev, enabled: true })
        : null,
    };
    list[idx] = next;
    saveAutomationsLocal(list);
    return next as AutomationDto;
  }
  return invoke<AutomationDto>("automation_set_enabled", { id, enabled });
}

export async function automationMarkRun(
  id: string,
  lastRunAt: string,
  nextRunAt: string | null,
): Promise<AutomationDto> {
  if (!isTauri()) {
    const { loadAutomationsLocal, saveAutomationsLocal } =
      await import("./automations");
    const list = loadAutomationsLocal();
    const idx = list.findIndex((a) => a.id === id);
    if (idx < 0) throw new Error("automation not found");
    const next = {
      ...list[idx],
      lastRunAt,
      nextRunAt,
      updatedAt: new Date().toISOString(),
    };
    list[idx] = next;
    saveAutomationsLocal(list);
    return next as AutomationDto;
  }
  return invoke<AutomationDto>("automation_mark_run", {
    id,
    lastRunAt,
    nextRunAt,
  });
}

export async function automationDelete(id: string): Promise<void> {
  if (!isTauri()) {
    const { loadAutomationsLocal, saveAutomationsLocal } =
      await import("./automations");
    const list = loadAutomationsLocal().filter((a) => a.id !== id);
    saveAutomationsLocal(list);
    return;
  }
  return invoke<void>("automation_delete", { id });
}

export async function automationClaimCompleteV1(
  claimId: string,
  success: boolean,
  error?: string | null,
) {
  return invoke<void>("automation_claim_complete_v1", {
    claimId,
    success,
    error: error ?? null,
  });
}

export async function automationClaimBindV1(
  claimId: string,
  sessionId: string,
) {
  return invoke<void>("automation_claim_bind_v1", { claimId, sessionId });
}
