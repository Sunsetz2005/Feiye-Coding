/**
 * Exact model context meter in the composer row.
 *
 * The compact ring opens only on deliberate click. Values come from
 * Runtime/provider telemetry; this component never estimates tokens.
 */

import {
  useMemo,
  useRef,
  useState,
  type CSSProperties,
} from "react";
import { createPortal } from "react-dom";
import { IconArrowsMinimize } from "@/components/icons";
import { useFloatingMenu } from "@/lib/floatingMenu";
import {
  formatTokenCount,
  type ContextUsageDisplay,
  type LastCompactSummary,
} from "@/lib/contextUsage";

export type ContextUsageChipLabels = {
  aria: string;
  menuTitle: string;
  used: string;
  remaining: string;
  total: string;
  latestInput: string;
  latestOutput: string;
  cacheRead: string;
  reasoning: string;
  model: string;
  modelCalls: string;
  exactSource: string;
  updatedAt: string;
  waiting: string;
  capacityUnknown: string;
  lastCompact: string;
  lastCompactNone: string;
  tokensRange: string;
  compactAction: string;
  auto: string;
  manual: string;
};

type Props = {
  display: ContextUsageDisplay;
  labels: ContextUsageChipLabels;
  /**
   * The meter remains inspectable while a turn or another interaction owns
   * the Runtime. Only the mutating `/compact` action is locked.
   */
  compactDisabled?: boolean;
  onCompact: () => void;
};

function formatLastCompactDetail(
  last: LastCompactSummary,
  labels: ContextUsageChipLabels,
): string {
  if (
    last.tokensBefore != null &&
    last.tokensAfter != null &&
    Number.isFinite(last.tokensBefore) &&
    Number.isFinite(last.tokensAfter)
  ) {
    return labels.tokensRange
      .replace("{before}", formatTokenCount(last.tokensBefore))
      .replace("{after}", formatTokenCount(last.tokensAfter));
  }
  if (last.note?.trim()) return last.note.trim();
  return last.trigger === "manual" ? labels.manual : labels.auto;
}

function formatPercent(percent: number | null): string {
  if (percent == null || !Number.isFinite(percent)) return "—";
  if (percent > 0 && percent < 1) return "<1%";
  return `${Math.round(percent)}%`;
}

function formatUpdatedAt(value: string | undefined): string {
  if (!value) return "—";
  const date = new Date(value);
  if (!Number.isFinite(date.getTime())) return "—";
  return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

function ContextRing({
  percent,
  size,
  strokeWidth,
}: {
  percent: number | null;
  size: number;
  strokeWidth: number;
}) {
  const normalized =
    percent == null || !Number.isFinite(percent)
      ? 0
      : Math.min(100, Math.max(0, percent));
  const radius = 10 - strokeWidth / 2;
  const circumference = 2 * Math.PI * radius;
  const offset = circumference * (1 - normalized / 100);
  return (
    <svg
      className={`ctx-ring${percent == null ? " is-unknown" : ""}`}
      width={size}
      height={size}
      viewBox="0 0 20 20"
      aria-hidden
    >
      <circle
        className="ctx-ring__track"
        cx="10"
        cy="10"
        r={radius}
        fill="none"
        strokeWidth={strokeWidth}
      />
      <circle
        className="ctx-ring__value"
        cx="10"
        cy="10"
        r={radius}
        fill="none"
        strokeWidth={strokeWidth}
        strokeDasharray={circumference}
        strokeDashoffset={offset}
      />
    </svg>
  );
}

export function ContextUsageChip({
  display,
  labels,
  compactDisabled = false,
  onCompact,
}: Props) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const popRef = useRef<HTMLDivElement>(null);

  const closeNow = () => {
    setOpen(false);
  };

  const { pos, style: popStyle } = useFloatingMenu({
    open,
    surfaceId: "composer-context",
    triggerRef,
    panelRef: popRef,
    roots: [rootRef],
    onClose: closeNow,
    placement: "up",
    fitContent: true,
    minWidth: 236,
    estHeight: 238,
    gap: 8,
    deps: [
      display.tokens,
      display.contextWindowTokens,
      display.runtime?.updatedAt,
    ],
  });

  const usedPercent = display.percentUsed;
  const percentLabel = formatPercent(usedPercent);
  const accessibleLabel = useMemo(() => {
    if (display.source !== "runtime") return `${labels.aria}: ${labels.waiting}`;
    if (display.contextWindowTokens == null) {
      return `${labels.aria}: ${formatTokenCount(display.tokens ?? 0)}`;
    }
    return `${labels.aria}: ${percentLabel}`;
  }, [display, labels, percentLabel]);
  const lastDetail = display.lastCompact
    ? formatLastCompactDetail(display.lastCompact, labels)
    : labels.lastCompactNone;
  const runtime = display.runtime;

  return (
    <div ref={rootRef} className={`ctx-chip${open ? " is-open" : ""}`}>
      <button
        ref={triggerRef}
        type="button"
        className="ctx-meter"
        data-readonly={compactDisabled || undefined}
        aria-haspopup="dialog"
        aria-expanded={open}
        aria-label={accessibleLabel}
        onClick={() => setOpen((value) => !value)}
      >
        <ContextRing percent={usedPercent} size={20} strokeWidth={2.4} />
      </button>
      {open &&
        pos &&
        typeof document !== "undefined" &&
        createPortal(
          <section
            ref={popRef}
            className="cmm__pop cmm__pop--portal ctx-chip__pop"
            role="dialog"
            aria-label={labels.menuTitle}
            style={popStyle as CSSProperties}
          >
            <header className="ctx-chip__head">
              <div className="ctx-chip__hero-ring">
                <ContextRing percent={usedPercent} size={34} strokeWidth={2} />
              </div>
              <div className="ctx-chip__hero-copy">
                <h3>{labels.menuTitle}</h3>
                {display.source === "runtime" ? (
                  <p>
                    <strong>{formatTokenCount(display.tokens ?? 0)}</strong>
                    {display.contextWindowTokens == null
                      ? ` ${labels.used}`
                      : ` / ${formatTokenCount(display.contextWindowTokens)} · ${percentLabel}`}
                  </p>
                ) : (
                  <p>{labels.waiting}</p>
                )}
              </div>
              {runtime ? (
                <span className="ctx-chip__source">{labels.exactSource}</span>
              ) : null}
            </header>

            <dl className="ctx-chip__details">
              <div>
                <dt>{labels.used}</dt>
                <dd>
                  {display.tokens == null
                    || display.source !== "runtime"
                    ? "—"
                    : formatTokenCount(display.tokens)}
                </dd>
              </div>
              <div>
                <dt>{labels.remaining}</dt>
                <dd>
                  {display.remainingTokens == null
                    ? "—"
                    : formatTokenCount(display.remainingTokens)}
                </dd>
              </div>
              <div>
                <dt>{labels.total}</dt>
                <dd>
                  {display.source !== "runtime" ||
                  display.contextWindowTokens == null
                    ? labels.capacityUnknown
                    : formatTokenCount(display.contextWindowTokens)}
                </dd>
              </div>
              {runtime ? (
                <>
                  <div>
                    <dt>{labels.model}</dt>
                    <dd title={runtime.modelId}>{runtime.modelId}</dd>
                  </div>
                  <div>
                    <dt>{labels.updatedAt}</dt>
                    <dd>{formatUpdatedAt(runtime.updatedAt)}</dd>
                  </div>
                </>
              ) : null}
              {display.lastCompact ? (
                <div>
                  <dt>{labels.lastCompact}</dt>
                  <dd title={lastDetail}>{lastDetail}</dd>
                </div>
              ) : null}
            </dl>

            <button
              type="button"
              className="ctx-chip__action"
              disabled={compactDisabled}
              aria-disabled={compactDisabled || undefined}
              onClick={() => {
                if (compactDisabled) return;
                closeNow();
                onCompact();
              }}
            >
              <IconArrowsMinimize size={14} aria-hidden />
              <span>{labels.compactAction}</span>
            </button>
          </section>,
          document.body,
        )}
    </div>
  );
}
