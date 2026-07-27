/**
 * Catalogs aligned with Grok Build CLI (`grok models`, reasoning effort, permission).
 * Live selectable models come from `models_list_available` (CLI cache + custom providers).
 * Update docs/llm-wiki/catalog.md when defaults change.
 */

export interface ModelOption {
  id: string;
  /** Display name (language-neutral product name) */
  label: string;
  /** True if CLI lists as default */
  isDefault?: boolean;
  /** Catalog source; composer only shows official model IDs (not providers). */
  source?: string;
  /** Optional host/runtime-declared controls for this exact model. */
  capabilities?: {
    reasoningEfforts?: Array<"low" | "medium" | "high">;
  };
}

export interface EffortOption {
  id: "high" | "medium" | "low";
}

export interface SessionModeOption {
  id: "agent" | "plan" | "ask";
}

/**
 * Permission policies (composer + settings), aligned with Grok Build modes:
 * | Build mode           | App id            |
 * | default              | ask               |
 * | acceptEdits          | accept_edits      |
 * | (session grant UX)   | allow_for_session |
 * | dontAsk              | dont_ask          |
 * | bypassPermissions    | always_approve    |
 */
export type PermissionPolicyId =
  | "ask"
  | "accept_edits"
  | "allow_for_session"
  | "dont_ask"
  | "always_approve";

/** Where composer model / permission choices are remembered. */
export type ComposerPrefsScope = "global" | "project" | "session";

export const COMPOSER_PREFS_SCOPES: ComposerPrefsScope[] = [
  "global",
  "project",
  "session",
];

/**
 * Fallback catalog when Host has not returned live models yet.
 * Official OAuth currently exposes grok-4.5 only (2026-07 probe).
 * `grok-build` is NOT listed — CLI rejects it as unknown model id.
 */
export const GROK_BUILD_MODELS: ModelOption[] = [
  {
    id: "grok-4.5",
    label: "Sunsetz 4.5",
    isDefault: true,
    source: "official",
    capabilities: {
      reasoningEfforts: ["low", "medium", "high"],
    },
  },
];

export const DEFAULT_MODEL_ID =
  GROK_BUILD_MODELS.find((m) => m.isDefault)?.id ?? "grok-4.5";

/** Reasoning effort flags: `--reasoning-effort` / `--effort` on grok agent. */
export const GROK_BUILD_EFFORTS: EffortOption[] = [
  { id: "medium" },
  { id: "low" },
  { id: "high" },
];

/**
 * Default reasoning depth. `medium` balances speed vs quality for agentic use;
 * users can lower (faster) or raise (deeper) via the composer chip.
 */
export const DEFAULT_EFFORT: EffortOption["id"] = "medium";

/** Product session modes (desktop shell). */
export const SESSION_MODES: SessionModeOption[] = [
  { id: "agent" },
  { id: "plan" },
  { id: "ask" },
];

/**
 * Permission policies (composer + settings).
 * `always_approve` = YOLO / unrestricted (CLI `--always-approve`, config yolo).
 */
export const PERMISSION_POLICIES: {
  id: PermissionPolicyId;
  dangerous?: boolean;
}[] = [
  { id: "ask" },
  { id: "accept_edits" },
  { id: "allow_for_session" },
  { id: "dont_ask" },
  { id: "always_approve", dangerous: true },
];

export function isValidModelId(
  id: string,
  catalog: ModelOption[] = GROK_BUILD_MODELS,
): boolean {
  return catalog.some((m) => m.id === id);
}

export function isValidEffort(id: string): id is EffortOption["id"] {
  return GROK_BUILD_EFFORTS.some((e) => e.id === id);
}

export function isValidPolicy(id: string): id is PermissionPolicyId {
  return PERMISSION_POLICIES.some((p) => p.id === id);
}

export function isValidPrefsScope(id: string): id is ComposerPrefsScope {
  return COMPOSER_PREFS_SCOPES.includes(id as ComposerPrefsScope);
}

export function pickDefaultModelId(catalog: ModelOption[]): string {
  return (
    catalog.find((m) => m.isDefault)?.id ??
    catalog[0]?.id ??
    DEFAULT_MODEL_ID
  );
}
