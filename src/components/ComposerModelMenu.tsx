/**
 * Composer chip menus (Codex-style):
 * - Model (+effort)
 * - Access: session mode + permission in one panel
 * Narrow composer widths compress triggers to icon (+ short label).
 */

import {
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import {
  GROK_BUILD_EFFORTS,
  GROK_BUILD_MODELS,
  PERMISSION_POLICIES,
  SESSION_MODES,
  type EffortOption,
  type ModelOption,
  type PermissionPolicyId,
} from "@/lib/grokCatalog";
import { Tip } from "@/components/ui/tooltip";
import {
  IconAlertTriangle,
  IconCheck,
  IconChevronDown,
  IconChevronRight,
  IconHandStop,
  IconList,
  IconRobot,
  IconRefresh,
  IconShield,
  IconShieldCheck,
} from "@/components/icons";
import {
  computeCascadePos,
  useFloatingMenu,
  type FloatingPos,
} from "@/lib/floatingMenu";

type EffortId = EffortOption["id"];
type Nested = "model" | "effort" | null;

function usePortalMenu(
  estHeight = 220,
  _width = 300,
  nestedKey?: string,
  extraRoots: Array<React.RefObject<HTMLElement | null>> = [],
  surfaceId?: string,
) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const popRef = useRef<HTMLDivElement>(null);
  const popId = useId();

  const { pos, style: popStyle } = useFloatingMenu({
    open,
    surfaceId,
    triggerRef,
    panelRef: popRef,
    roots: [rootRef, ...extraRoots],
    onClose: () => setOpen(false),
    placement: "auto",
    fitContent: true,
    minWidth: 200,
    estHeight,
    gap: 8,
    deps: [nestedKey],
  });

  return {
    open,
    setOpen,
    pos,
    popStyle: popStyle as CSSProperties | undefined,
    rootRef,
    triggerRef,
    popRef,
    popId,
  };
}

function MenuShell({
  open,
  setOpen,
  rootRef,
  triggerRef,
  popRef,
  popId,
  pos,
  popStyle,
  triggerIcon,
  triggerText,
  triggerShort,
  ariaLabel,
  title,
  danger,
  readOnly,
  children,
  onOpenChange,
  className = "",
}: {
  open: boolean;
  setOpen: (v: boolean | ((p: boolean) => boolean)) => void;
  rootRef: React.RefObject<HTMLDivElement | null>;
  triggerRef: React.RefObject<HTMLButtonElement | null>;
  popRef: React.RefObject<HTMLDivElement | null>;
  popId: string;
  pos: FloatingPos | null;
  popStyle: CSSProperties | undefined;
  triggerIcon?: ReactNode;
  /** Full label (wide layout) */
  triggerText: string;
  /** Short label (medium; icon-only when very narrow via CSS) */
  triggerShort?: string;
  ariaLabel: string;
  title?: string;
  danger?: boolean;
  readOnly?: boolean;
  children: ReactNode;
  onOpenChange?: (open: boolean) => void;
  className?: string;
}) {
  const panel =
    open && pos && typeof document !== "undefined"
      ? createPortal(
          <div
            ref={popRef}
            className="cmm__pop cmm__pop--portal"
            id={popId}
            role="dialog"
            aria-label={ariaLabel}
            style={popStyle}
          >
            {children}
          </div>,
          document.body,
        )
      : null;

  const tipLabel = title ?? ariaLabel;
  const trigger = (
    <button
      ref={triggerRef}
      type="button"
      className="cmm__trigger"
      aria-haspopup="dialog"
      aria-expanded={open}
      aria-controls={popId}
      aria-label={ariaLabel}
      data-readonly={readOnly || undefined}
      onClick={() => {
        setOpen((v) => {
          const next = !v;
          onOpenChange?.(next);
          return next;
        });
      }}
    >
      {triggerIcon ? (
        <span className="cmm__icon" aria-hidden>
          {triggerIcon}
        </span>
      ) : null}
      <span className="cmm__trigger-text cmm__trigger-text--full">
        {triggerText}
      </span>
      {triggerShort != null && (
        <span className="cmm__trigger-text cmm__trigger-text--short">
          {triggerShort}
        </span>
      )}
      <span className="cmm__chev" aria-hidden>
        <IconChevronDown size={12} />
      </span>
    </button>
  );

  return (
    <div
      ref={rootRef}
      className={`cmm ${open ? "is-open" : ""} ${danger ? "cmm--danger" : ""} ${className}`.trim()}
    >
      {tipLabel ? <Tip label={tipLabel}>{trigger}</Tip> : trigger}
      {panel}
    </div>
  );
}

/* ---------- Model + effort ---------- */

export interface ComposerModelMenuProps {
  modelId: string;
  effort: string;
  /** Live selectable models only (from Host catalog). */
  models?: readonly ModelOption[];
  /**
   * Runtime-confirmed reasoning levels. Pass `[]` when the active model does
   * not expose an effort control. Omission preserves the legacy catalog.
   */
  efforts?: readonly EffortOption[];
  /**
   * Read-only while a turn / permission / question is pending. The menu still
   * opens so the current settings remain inspectable.
   */
  disabled?: boolean;
  labels: {
    model: string;
    effort: string;
    effortHigh: string;
    effortMedium: string;
    effortLow: string;
    resetDefaults?: string;
    resetDefaultsHint?: string;
  };
  onModel: (id: string) => void;
  onEffort: (id: EffortId) => void;
  /** Omit until the host has a real defaults reset operation. */
  onReset?: () => void;
}

function effortLabel(
  id: string,
  labels: ComposerModelMenuProps["labels"],
): string {
  if (id === "high") return labels.effortHigh;
  if (id === "medium") return labels.effortMedium;
  return labels.effortLow;
}

function effortShort(id: string, labels: ComposerModelMenuProps["labels"]): string {
  // Compact: just effort word (icon carries model)
  return effortLabel(id, labels);
}

export function resolveComposerEfforts(opts: {
  modelId: string;
  models?: readonly ModelOption[];
  efforts?: readonly EffortOption[];
}): readonly EffortOption[] {
  if (opts.efforts !== undefined) return opts.efforts;
  if (opts.models === undefined) return GROK_BUILD_EFFORTS;
  const declared = opts.models.find(
    (model) => model.id === opts.modelId,
  )?.capabilities?.reasoningEfforts;
  return declared?.map((id) => ({ id }) satisfies EffortOption) ?? [];
}

export function ComposerModelMenu({
  modelId,
  effort,
  models,
  efforts,
  disabled = false,
  labels,
  onModel,
  onEffort,
  onReset,
}: ComposerModelMenuProps) {
  const [nested, setNested] = useState<Nested>(null);
  const [rootIndex, setRootIndex] = useState(0);
  const [cascadeStyle, setCascadeStyle] = useState<CSSProperties>();
  const cascadeRef = useRef<HTMLDivElement>(null);
  const modelRowRef = useRef<HTMLButtonElement>(null);
  const effortRowRef = useRef<HTMLButtonElement>(null);
  const resetRowRef = useRef<HTMLButtonElement>(null);
  const menu = usePortalMenu(
    240,
    280,
    nested ?? "root",
    [cascadeRef],
    "composer-model",
  );
  const modelList = models ?? GROK_BUILD_MODELS;
  const effortList = resolveComposerEfforts({ modelId, models, efforts });
  const hasEffort = effortList.length > 0;
  const hasReset = Boolean(onReset && labels.resetDefaults);

  useEffect(() => {
    if (!menu.open) {
      setNested(null);
      setCascadeStyle(undefined);
    }
  }, [menu.open]);

  useLayoutEffect(() => {
    if (!menu.open || !nested) return;
    const row =
      nested === "model" ? modelRowRef.current : effortRowRef.current;
    if (!row) return;

    const update = () => {
      const panel = cascadeRef.current;
      const panelRect = panel?.getBoundingClientRect();
      const pos = computeCascadePos(
        row.getBoundingClientRect(),
        {
          width: panel?.offsetWidth || panelRect?.width || 224,
          height: panel?.offsetHeight || panelRect?.height || 220,
        },
        { gap: 6, margin: 8 },
      );
      setCascadeStyle({
        position: "fixed",
        left: pos.left,
        top: pos.top,
        width: pos.width,
        maxHeight: pos.maxHeight,
        zIndex: 10001,
        "--popover-origin":
          pos.side === "right" ? "center left" : "center right",
      } as CSSProperties);
    };

    update();
    const frame = window.requestAnimationFrame(update);
    const onViewportChange = () => update();
    window.addEventListener("resize", onViewportChange);
    window.addEventListener("scroll", onViewportChange, true);
    const observer =
      typeof ResizeObserver === "undefined" || !cascadeRef.current
        ? null
        : new ResizeObserver(update);
    if (cascadeRef.current) observer?.observe(cascadeRef.current);
    return () => {
      window.cancelAnimationFrame(frame);
      window.removeEventListener("resize", onViewportChange);
      window.removeEventListener("scroll", onViewportChange, true);
      observer?.disconnect();
    };
  }, [menu.open, nested, modelList.length, effortList.length]);

  useEffect(() => {
    if (!menu.open || nested) return;
    const frame = window.requestAnimationFrame(() => {
      rootMenuItems()[rootIndex]?.focus();
    });
    return () => window.cancelAnimationFrame(frame);
    // `rootMenuItems` is intentionally read from refs after the portal mounts.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [menu.open]);

  useEffect(() => {
    if (!menu.open || !nested) return;
    const frame = window.requestAnimationFrame(() => {
      const panel = cascadeRef.current;
      const current = panel?.querySelector<HTMLElement>('[aria-checked="true"]');
      const first = panel?.querySelector<HTMLElement>('[role="menuitemradio"]');
      (current ?? first ?? panel)?.focus();
    });
    return () => window.cancelAnimationFrame(frame);
  }, [menu.open, nested]);

  const modelLabel =
    modelList.find((m) => m.id === modelId)?.label ?? modelId;
  const eLabel = effortLabel(effort, labels);
  const triggerText = hasEffort ? `${modelLabel} ${eLabel}` : modelLabel;
  const title = hasEffort
    ? `${labels.model}: ${modelLabel} · ${labels.effort}: ${eLabel}`
    : `${labels.model}: ${modelLabel}`;

  function rootMenuItems(): HTMLButtonElement[] {
    return [
      modelRowRef.current,
      hasEffort ? effortRowRef.current : null,
      hasReset ? resetRowRef.current : null,
    ].filter((item): item is HTMLButtonElement => item != null);
  }

  function focusRoot(delta: number) {
    const items = rootMenuItems();
    if (items.length === 0) return;
    const focused = document.activeElement;
    const current = Math.max(0, items.indexOf(focused as HTMLButtonElement));
    const next = (current + delta + items.length) % items.length;
    setRootIndex(next);
    items[next]?.focus();
  }

  function closeNested() {
    const parent =
      nested === "model" ? modelRowRef.current : effortRowRef.current;
    setNested(null);
    setCascadeStyle(undefined);
    window.requestAnimationFrame(() => parent?.focus());
  }

  function closeAll({ restoreFocus = true } = {}) {
    setNested(null);
    setCascadeStyle(undefined);
    menu.setOpen(false);
    if (restoreFocus) {
      window.requestAnimationFrame(() => menu.triggerRef.current?.focus());
    }
  }

  function openNested(next: Exclude<Nested, null>) {
    setCascadeStyle(undefined);
    setNested(next);
  }

  function handleRootKeyDown(event: React.KeyboardEvent<HTMLDivElement>) {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      focusRoot(1);
      return;
    }
    if (event.key === "ArrowUp") {
      event.preventDefault();
      focusRoot(-1);
      return;
    }
    if (event.key === "Home" || event.key === "End") {
      event.preventDefault();
      const items = rootMenuItems();
      const index = event.key === "Home" ? 0 : items.length - 1;
      setRootIndex(index);
      items[index]?.focus();
      return;
    }
    if (event.key === "Escape" || event.key === "ArrowLeft") {
      event.preventDefault();
      event.stopPropagation();
      closeAll();
      return;
    }
    if (event.key === "ArrowRight") {
      const focused = document.activeElement;
      if (focused === modelRowRef.current) {
        event.preventDefault();
        openNested("model");
      } else if (focused === effortRowRef.current) {
        event.preventDefault();
        openNested("effort");
      }
    }
  }

  function handleNestedKeyDown(event: React.KeyboardEvent<HTMLDivElement>) {
    if (event.key === "Escape" || event.key === "ArrowLeft") {
      event.preventDefault();
      event.stopPropagation();
      closeNested();
      return;
    }
    if (
      event.key !== "ArrowDown" &&
      event.key !== "ArrowUp" &&
      event.key !== "Home" &&
      event.key !== "End"
    ) {
      return;
    }
    event.preventDefault();
    const items = Array.from(
      cascadeRef.current?.querySelectorAll<HTMLButtonElement>(
        '[role="menuitemradio"]',
      ) ?? [],
    );
    if (items.length === 0) return;
    const current = Math.max(
      0,
      items.indexOf(document.activeElement as HTMLButtonElement),
    );
    const index =
      event.key === "Home"
        ? 0
        : event.key === "End"
          ? items.length - 1
          : (current + (event.key === "ArrowDown" ? 1 : -1) + items.length) %
            items.length;
    items[index]?.focus();
  }

  const panel =
    menu.open && menu.pos && typeof document !== "undefined"
      ? createPortal(
          <>
            <div
              ref={menu.popRef}
              className="cmm__pop cmm__pop--portal cmm__pop--model-root"
              id={menu.popId}
              role="menu"
              aria-label={labels.model}
              aria-orientation="vertical"
              data-readonly={disabled || undefined}
              style={menu.popStyle}
              onKeyDown={handleRootKeyDown}
            >
              <button
                ref={modelRowRef}
                type="button"
                role="menuitem"
                className={"cmm__row" + (nested === "model" ? " is-active" : "")}
                aria-haspopup="menu"
                aria-expanded={nested === "model"}
                onFocus={() => setRootIndex(0)}
                onClick={() => openNested("model")}
              >
                <span>{labels.model}</span>
                <span className="cmm__row-val">
                  {modelLabel}
                  <IconChevronRight size={14} />
                </span>
              </button>
              {hasEffort ? (
                <button
                  ref={effortRowRef}
                  type="button"
                  role="menuitem"
                  className={
                    "cmm__row" + (nested === "effort" ? " is-active" : "")
                  }
                  aria-haspopup="menu"
                  aria-expanded={nested === "effort"}
                  onFocus={() => setRootIndex(1)}
                  onClick={() => openNested("effort")}
                >
                  <span>{labels.effort}</span>
                  <span className="cmm__row-val">
                    {eLabel}
                    <IconChevronRight size={14} />
                  </span>
                </button>
              ) : null}
              {hasReset ? (
                <button
                  ref={resetRowRef}
                  type="button"
                  role="menuitem"
                  className="cmm__row cmm__row--reset"
                  aria-disabled={disabled || undefined}
                  onFocus={() => setRootIndex(hasEffort ? 2 : 1)}
                  onClick={() => {
                    if (disabled || !onReset) return;
                    onReset();
                    closeAll();
                  }}
                >
                  <span className="cmm__row-reset-label">
                    <IconRefresh size={15} />
                    <span>
                      <strong>{labels.resetDefaults}</strong>
                      {labels.resetDefaultsHint ? (
                        <small>{labels.resetDefaultsHint}</small>
                      ) : null}
                    </span>
                  </span>
                </button>
              ) : null}
            </div>

            {nested ? (
              <div
                ref={cascadeRef}
                className="cmm__pop cmm__pop--portal cmm__cascade"
                role="menu"
                tabIndex={-1}
                aria-label={nested === "model" ? labels.model : labels.effort}
                aria-orientation="vertical"
                data-readonly={disabled || undefined}
                style={
                  cascadeStyle ?? {
                    position: "fixed",
                    left: 0,
                    top: 0,
                    visibility: "hidden",
                    pointerEvents: "none",
                  }
                }
                onKeyDown={handleNestedKeyDown}
              >
                {nested === "model"
                  ? modelList.length === 0
                    ? (
                      <div className="cmm__opt cmm__opt--muted" role="status">
                        <span className="cmm__opt-main">
                          <span className="cmm__opt-title">
                            {modelId || "—"}
                          </span>
                        </span>
                      </div>
                    )
                    : modelList.map((model) => (
                      <button
                        key={model.id}
                        type="button"
                        role="menuitemradio"
                        aria-checked={model.id === modelId}
                        aria-disabled={disabled || undefined}
                        className={
                          "cmm__opt" +
                          (model.id === modelId ? " is-active" : "")
                        }
                        onClick={() => {
                          if (disabled) return;
                          onModel(model.id);
                          closeAll();
                        }}
                      >
                        <span className="cmm__opt-main">
                          <span className="cmm__opt-title">{model.label}</span>
                        </span>
                        {model.id === modelId ? (
                          <span className="cmm__opt-check" aria-hidden>
                            <IconCheck size={16} />
                          </span>
                        ) : null}
                      </button>
                      ))
                  : effortList.map((option) => (
                      <button
                        key={option.id}
                        type="button"
                        role="menuitemradio"
                        aria-checked={option.id === effort}
                        aria-disabled={disabled || undefined}
                        className={
                          "cmm__opt" +
                          (option.id === effort ? " is-active" : "")
                        }
                        onClick={() => {
                          if (disabled) return;
                          onEffort(option.id);
                          closeAll();
                        }}
                      >
                        <span className="cmm__opt-main">
                          <span className="cmm__opt-title">
                            {effortLabel(option.id, labels)}
                          </span>
                        </span>
                        {option.id === effort ? (
                          <span className="cmm__opt-check" aria-hidden>
                            <IconCheck size={16} />
                          </span>
                        ) : null}
                      </button>
                    ))}
              </div>
            ) : null}
          </>,
          document.body,
        )
      : null;

  const trigger = (
    <button
      ref={menu.triggerRef}
      type="button"
      className="cmm__trigger"
      aria-haspopup="menu"
      aria-expanded={menu.open}
      aria-controls={menu.popId}
      aria-label={title}
      data-readonly={disabled || undefined}
      onClick={() => {
        if (menu.open) closeAll({ restoreFocus: false });
        else {
          setRootIndex(0);
          menu.setOpen(true);
        }
      }}
    >
      <span className="cmm__trigger-text cmm__trigger-text--full">
        {triggerText}
      </span>
      <span className="cmm__trigger-text cmm__trigger-text--short">
        {hasEffort ? `${modelLabel} ${effortShort(effort, labels)}` : modelLabel}
      </span>
      <span className="cmm__chev" aria-hidden>
        <IconChevronDown size={12} />
      </span>
    </button>
  );

  return (
    <div
      ref={menu.rootRef}
      className={`cmm cmm--model${menu.open ? " is-open" : ""}`}
    >
      <Tip label={title}>{trigger}</Tip>
      {panel}
    </div>
  );
}

/* ---------- Access: mode + permission (Codex-style one entry) ---------- */

export interface ComposerAccessMenuProps {
  mode: string;
  policy: string;
  /**
   * Read-only while Runtime state is owned by a turn, permission, question,
   * or plan review. The menu stays openable so settings can be inspected.
   */
  disabled?: boolean;
  labels: {
    access: string;
    accessHint: string;
    mode: string;
    modeAgent: string;
    modePlan: string;
    modeAsk: string;
    modeAgentDesc: string;
    modePlanDesc: string;
    modeAskDesc: string;
    permission: string;
    policyAsk: string;
    policyAcceptEdits: string;
    policySession: string;
    policyDontAsk: string;
    policyYolo: string;
    policyAskDesc: string;
    policyAcceptEditsDesc: string;
    policySessionDesc: string;
    policyDontAskDesc: string;
    policyYoloDesc: string;
    policyShortAsk: string;
    policyShortAccept: string;
    policyShortSession: string;
    policyShortDontAsk: string;
    policyShortYolo: string;
  };
  onMode: (id: string) => void;
  onPolicy: (id: PermissionPolicyId) => void;
}

function modeLabel(id: string, labels: ComposerAccessMenuProps["labels"]): string {
  if (id === "plan") return labels.modePlan;
  if (id === "ask") return labels.modeAsk;
  return labels.modeAgent;
}

function modeDesc(id: string, labels: ComposerAccessMenuProps["labels"]): string {
  if (id === "plan") return labels.modePlanDesc;
  if (id === "ask") return labels.modeAskDesc;
  return labels.modeAgentDesc;
}

function policyLabel(
  id: string,
  labels: ComposerAccessMenuProps["labels"],
): string {
  switch (id) {
    case "accept_edits":
      return labels.policyAcceptEdits;
    case "allow_for_session":
      return labels.policySession;
    case "dont_ask":
      return labels.policyDontAsk;
    case "always_approve":
      return labels.policyYolo;
    default:
      return labels.policyAsk;
  }
}

function policyShort(
  id: string,
  labels: ComposerAccessMenuProps["labels"],
): string {
  switch (id) {
    case "accept_edits":
      return labels.policyShortAccept;
    case "allow_for_session":
      return labels.policyShortSession;
    case "dont_ask":
      return labels.policyShortDontAsk;
    case "always_approve":
      return labels.policyShortYolo;
    default:
      return labels.policyShortAsk;
  }
}

function policyDesc(
  id: string,
  labels: ComposerAccessMenuProps["labels"],
): string {
  switch (id) {
    case "accept_edits":
      return labels.policyAcceptEditsDesc;
    case "allow_for_session":
      return labels.policySessionDesc;
    case "dont_ask":
      return labels.policyDontAskDesc;
    case "always_approve":
      return labels.policyYoloDesc;
    default:
      return labels.policyAskDesc;
  }
}

function policyIcon(id: string) {
  switch (id) {
    case "accept_edits":
      return <IconShieldCheck size={18} />;
    case "allow_for_session":
      return <IconShield size={18} />;
    case "dont_ask":
      return <IconHandStop size={18} />;
    case "always_approve":
      return <IconAlertTriangle size={18} />;
    default:
      return <IconHandStop size={18} />;
  }
}

function modeIcon(id: string) {
  if (id === "plan") return <IconList size={18} />;
  if (id === "ask") return <IconHandStop size={18} />;
  return <IconRobot size={18} />;
}

export function ComposerAccessMenu({
  mode,
  policy,
  disabled = false,
  labels,
  onMode,
  onPolicy,
}: ComposerAccessMenuProps) {
  const menu = usePortalMenu(
    420,
    320,
    undefined,
    [],
    "composer-access",
  );
  const isDanger = policy === "always_approve";
  const full = policyLabel(policy, labels);
  const short = policyShort(policy, labels);
  const title = `${labels.mode}: ${modeLabel(mode, labels)} · ${labels.permission}: ${full}`;

  return (
    <MenuShell
      {...menu}
      className="cmm--access"
      triggerIcon={policyIcon(policy)}
      triggerText={full}
      triggerShort={short}
      ariaLabel={labels.access}
      title={title}
      danger={isDanger}
      readOnly={disabled}
    >
      <div className="cmm__header">
        <div className="cmm__header-title">{labels.accessHint}</div>
      </div>

      <div className="cmm__section">{labels.mode}</div>
      {SESSION_MODES.map((m) => (
        <button
          key={m.id}
          type="button"
          className={"cmm__opt cmm__opt--rich" + (m.id === mode ? " is-active" : "")}
          aria-disabled={disabled || undefined}
          onClick={() => {
            if (disabled) return;
            onMode(m.id);
          }}
        >
          <span className="cmm__opt-icon" aria-hidden>
            {modeIcon(m.id)}
          </span>
          <span className="cmm__opt-main">
            <span className="cmm__opt-title">{modeLabel(m.id, labels)}</span>
            <span className="cmm__opt-desc">{modeDesc(m.id, labels)}</span>
          </span>
          {m.id === mode && (
            <span className="cmm__opt-check" aria-hidden>
              <IconCheck size={16} />
            </span>
          )}
        </button>
      ))}

      <div className="cmm__section cmm__section--gap">{labels.permission}</div>
      {PERMISSION_POLICIES.map((p) => (
        <button
          key={p.id}
          type="button"
          className={
            "cmm__opt cmm__opt--rich" +
            (p.id === policy ? " is-active" : "") +
            (p.dangerous ? " is-danger" : "")
          }
          aria-disabled={disabled || undefined}
          onClick={() => {
            if (disabled) return;
            onPolicy(p.id);
            menu.setOpen(false);
          }}
        >
          <span className="cmm__opt-icon" aria-hidden>
            {policyIcon(p.id)}
          </span>
          <span className="cmm__opt-main">
            <span className="cmm__opt-title">{policyLabel(p.id, labels)}</span>
            <span className="cmm__opt-desc">{policyDesc(p.id, labels)}</span>
          </span>
          {p.id === policy && (
            <span className="cmm__opt-check" aria-hidden>
              <IconCheck size={16} />
            </span>
          )}
        </button>
      ))}
    </MenuShell>
  );
}
