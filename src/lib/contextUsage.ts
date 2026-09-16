/**
 * Context usage chip — pure token format + state for honest UX.
 *
 * Runtime telemetry is the authoritative source for current usage. Compact
 * events may provide an older exact count; otherwise the UI deliberately shows
 * an unknown state instead of estimating from visible characters.
 */

export type ContextUsageSource = "runtime" | "known" | "unknown";

/** Exact model usage persisted by the Tauri Runtime after a completed turn. */
export interface RuntimeContextUsage {
  usedTokens: number;
  inputTokens: number;
  outputTokens: number;
  cachedReadTokens: number;
  reasoningTokens: number;
  turnInputTokens: number;
  turnOutputTokens: number;
  modelCalls: number;
  modelId: string;
  contextWindowTokens?: number | null;
  updatedAt: string;
  source: "runtime" | string;
}

export interface LastCompactSummary {
  trigger: string;
  tokensBefore?: number;
  tokensAfter?: number;
  summaryPreview?: string;
  note?: string;
  messageId?: string;
}

export interface ContextUsageState {
  /** Absolute tokens from last agent compact event (`tokensAfter`). */
  knownTokens: number | null;
  /** Message id of the last compact marker (for post-compact delta). */
  lastCompactMessageId: string | null;
  lastCompact: LastCompactSummary | null;
  /** True between a `ContextCompactStart` signal and its terminal event. */
  compacting: boolean;
}

export const INITIAL_CONTEXT_USAGE: ContextUsageState = {
  knownTokens: null,
  lastCompactMessageId: null,
  lastCompact: null,
  compacting: false,
};

export type ContextUsageMessage = {
  id: string;
  role: string;
  content?: string;
  thought?: string;
  marker?: string;
  compactMeta?: {
    trigger?: string;
    tokensBefore?: number;
    tokensAfter?: number;
    summaryPreview?: string;
    note?: string;
  } | null;
};

export type ContextUsageAction =
  | { type: "reset" }
  | {
      type: "compact";
      tokensBefore?: number;
      tokensAfter?: number;
      trigger?: string;
      summaryPreview?: string;
      note?: string;
      messageId?: string;
    }
  | { type: "hydrate"; messages: ContextUsageMessage[] }
  | { type: "compact_start" }
  | { type: "compact_end" };

function finiteToken(n: number | undefined | null): number | undefined {
  if (n == null || !Number.isFinite(n) || n < 0) return undefined;
  return Math.floor(n);
}

export function reduceContextUsage(
  state: ContextUsageState,
  action: ContextUsageAction,
): ContextUsageState {
  switch (action.type) {
    case "reset":
      return { ...INITIAL_CONTEXT_USAGE };
    case "compact": {
      const tokensAfter = finiteToken(action.tokensAfter);
      const tokensBefore = finiteToken(action.tokensBefore);
      const trigger = (action.trigger || "auto").toLowerCase();
      // Only keep absolute known tokens when this event reports tokensAfter.
      // A compact without counts invalidates the previous absolute figure.
      return {
        knownTokens: tokensAfter ?? null,
        lastCompactMessageId:
          action.messageId ?? state.lastCompactMessageId,
        lastCompact: {
          trigger:
            trigger === "manual"
              ? "manual"
              : trigger === "auto"
                ? "auto"
                : trigger,
          tokensBefore,
          tokensAfter,
          summaryPreview: action.summaryPreview,
          note: action.note,
          messageId: action.messageId,
        },
        compacting: false,
      };
    }
    case "hydrate":
      return hydrateContextUsageFromMessages(action.messages);
    case "compact_start":
      return { ...state, compacting: true };
    case "compact_end":
      return { ...state, compacting: false };
    default:
      return state;
  }
}

/** Scan history for the latest compact marker (session open / switch). */
export function hydrateContextUsageFromMessages(
  messages: ContextUsageMessage[],
): ContextUsageState {
  for (let i = messages.length - 1; i >= 0; i--) {
    const m = messages[i]!;
    const isCompact =
      m.marker === "context_compact" ||
      (m.role === "tool" &&
        (m.content?.startsWith("context_compact") || !!m.compactMeta));
    if (!isCompact) continue;
    const meta = m.compactMeta;
    const tokensAfter = finiteToken(meta?.tokensAfter);
    const tokensBefore = finiteToken(meta?.tokensBefore);
    const trigger = (meta?.trigger || "auto").toLowerCase();
    return {
      knownTokens: tokensAfter ?? null,
      lastCompactMessageId: m.id,
      lastCompact: {
        trigger:
          trigger === "manual"
            ? "manual"
            : trigger === "auto"
              ? "auto"
              : trigger,
        tokensBefore,
        tokensAfter,
        summaryPreview: meta?.summaryPreview,
        note: meta?.note,
        messageId: m.id,
      },
      compacting: false,
    };
  }
  return { ...INITIAL_CONTEXT_USAGE };
}

/** Compact token display: 999 / 1.2k / 12k / 1.5M */
export function formatTokenCount(n: number): string {
  if (!Number.isFinite(n) || n < 0) return "—";
  if (n >= 1_000_000) {
    const v = n / 1_000_000;
    return `${v >= 10 ? Math.round(v) : v.toFixed(1).replace(/\.0$/, "")}M`;
  }
  if (n >= 10_000) return `${Math.round(n / 1000)}k`;
  if (n >= 1000) {
    const v = n / 1000;
    return `${v.toFixed(1).replace(/\.0$/, "")}k`;
  }
  return String(Math.round(n));
}

export function formatContextChipLabel(
  tokens: number | null,
  source: ContextUsageSource,
): string {
  if (tokens == null || source === "unknown") return "—";
  return formatTokenCount(tokens);
}

export interface ContextUsageDisplay {
  tokens: number | null;
  source: ContextUsageSource;
  /** Compact accessible label. */
  label: string;
  lastCompact: LastCompactSummary | null;
  contextWindowTokens: number | null;
  remainingTokens: number | null;
  percentUsed: number | null;
  runtime: RuntimeContextUsage | null;
  compacting: boolean;
}

/**
 * Resolve what the chip should show from reducer state + live messages.
 */
export function resolveContextUsageDisplay(
  state: ContextUsageState,
  _messages: ContextUsageMessage[],
  runtime: RuntimeContextUsage | null = null,
): ContextUsageDisplay {
  const lastCompact = state.lastCompact;

  if (
    runtime &&
    Number.isFinite(runtime.usedTokens) &&
    runtime.usedTokens >= 0
  ) {
    const window =
      runtime.contextWindowTokens != null &&
      Number.isFinite(runtime.contextWindowTokens) &&
      runtime.contextWindowTokens > 0
        ? Math.floor(runtime.contextWindowTokens)
        : null;
    const tokens = Math.floor(runtime.usedTokens);
    return {
      tokens,
      source: "runtime",
      label: formatContextChipLabel(tokens, "runtime"),
      lastCompact,
      contextWindowTokens: window,
      remainingTokens: window == null ? null : Math.max(0, window - tokens),
      percentUsed:
        window == null ? null : Math.min(100, Math.max(0, (tokens / window) * 100)),
      runtime,
      compacting: state.compacting,
    };
  }

  if (state.knownTokens != null) {
    const tokens = state.knownTokens;
    const source: ContextUsageSource = "known";
    return {
      tokens,
      source,
      label: formatContextChipLabel(tokens, source),
      lastCompact,
      contextWindowTokens: null,
      remainingTokens: null,
      percentUsed: null,
      runtime: null,
      compacting: state.compacting,
    };
  }

  // Compact happened without token counts — do not trust full UI history.
  if (lastCompact) {
    return {
      tokens: null,
      source: "unknown",
      label: formatContextChipLabel(null, "unknown"),
      lastCompact,
      contextWindowTokens: null,
      remainingTokens: null,
      percentUsed: null,
      runtime: null,
      compacting: state.compacting,
    };
  }

  // Do not invent context occupancy from visible characters. System prompts,
  // tools, cached history and provider tokenizers make that number misleading.
  return {
    tokens: null,
    source: "unknown",
    label: formatContextChipLabel(null, "unknown"),
    lastCompact: null,
    contextWindowTokens: null,
    remainingTokens: null,
    percentUsed: null,
    runtime: null,
    compacting: state.compacting,
  };
}
