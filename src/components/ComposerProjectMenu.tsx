/**
 * Composer project chip — pick / clear / add folder; switch git worktrees.
 */

import { useEffect, useRef, useState, type CSSProperties } from "react";
import { createPortal } from "react-dom";
import {
  IconCheck,
  IconChevronDown,
  IconClose,
  IconFolder,
  IconFork,
  IconPlus,
  IconSearch,
} from "@/components/icons";
import { Tip } from "@/components/ui/tooltip";
import { useFloatingMenu } from "@/lib/floatingMenu";
import { pathsEqual, worktreeLabel } from "@/lib/gitWorktree";
import type { GitWorktreeEntry } from "@/lib/api";

export type ProjectOption = {
  id: string;
  name: string;
  path: string;
  trusted: boolean;
  pathOk: boolean;
  pinned?: boolean;
};

type Props = {
  activeProject: ProjectOption | null;
  projects: ProjectOption[];
  labels: {
    noProject: string;
    pickProject: string;
    addProject: string;
    searchProjects?: string;
    workNotInProject?: string;
    searchWorktrees?: string;
    uncommittedCount?: string;
    worktrees: string;
    worktreesEmpty: string;
    worktreesUnavailable: string;
    worktreesLoading?: string;
    worktreeCurrent: string;
    worktreeSwitch: string;
    worktreeMain: string;
    worktreeDetached: string;
  };
  /** Linked worktrees for the active project (loaded by parent). */
  worktrees?: GitWorktreeEntry[];
  /**
   * `true` only after host confirmed a git work tree.
   * `false` = not a git repo / git missing — section hidden.
   * `null` / omitted = unknown (loading or no project) — section hidden.
   */
  worktreesAvailable?: boolean | null;
  worktreesLoading?: boolean;
  worktreesReason?: string | null;
  disabled?: boolean;
  /** Quiet project-instruction hint; shown in the chip tooltip only. */
  instructionTip?: string | null;
  /** External, monotonic request used by the + menu to open this picker. */
  openRequestKey?: number;
  onSelect: (project: ProjectOption | null) => void;
  onAdd: () => void;
  /** Switch agent cwd to this worktree path (add project if needed + bind). */
  onSwitchWorktree?: (wt: GitWorktreeEntry) => void;
  onOpen?: () => void;
};

const LIST_MAX_H = 220;

export function ComposerProjectMenu({
  activeProject,
  projects,
  labels,
  worktrees: _worktrees = [],
  worktreesAvailable: _worktreesAvailable = null,
  worktreesLoading: _worktreesLoading = false,
  worktreesReason: _worktreesReason = null,
  disabled,
  instructionTip = null,
  openRequestKey = 0,
  onSelect,
  onAdd,
  onSwitchWorktree: _onSwitchWorktree,
  onOpen,
}: Props) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const popRef = useRef<HTMLDivElement>(null);
  /** Avoid putting unstable parent callbacks in the open-effect deps (re-fetch loop). */
  const onOpenRef = useRef(onOpen);
  onOpenRef.current = onOpen;

  const needle = query.trim().toLowerCase();
  const visibleProjects = needle
    ? projects.filter(
        (project) =>
          project.name.toLowerCase().includes(needle) ||
          project.path.toLowerCase().includes(needle),
      )
    : projects;

  const estHeight = Math.min(
    400,
    88 + Math.min(LIST_MAX_H, visibleProjects.length * 40 + 8),
  );
  const { pos, style: popStyle } = useFloatingMenu({
    open,
    surfaceId: "composer-project",
    triggerRef,
    panelRef: popRef,
    roots: [rootRef],
    onClose: () => setOpen(false),
    placement: "up",
    fitContent: true,
    minWidth: 260,
    estHeight,
    gap: 8,
    deps: [visibleProjects.length, query],
  });

  // Refresh only when the menu opens — not when parent re-renders with a new onOpen.
  useEffect(() => {
    if (!open) return;
    onOpenRef.current?.();
  }, [open]);
  useEffect(() => {
    if (!open) setQuery("");
  }, [open]);

  const lastOpenRequestRef = useRef(openRequestKey);
  useEffect(() => {
    if (lastOpenRequestRef.current === openRequestKey) return;
    lastOpenRequestRef.current = openRequestKey;
    if (disabled) return;
    setOpen(true);
    requestAnimationFrame(() => triggerRef.current?.focus());
  }, [disabled, openRequestKey]);

  const label = activeProject?.name ?? labels.noProject;
  const tip = instructionTip?.trim() || activeProject?.path || labels.pickProject;

  return (
    <div ref={rootRef} className={`cpm${open ? " is-open" : ""}`}>
      <Tip label={tip} disabled={open}>
        <button
          ref={triggerRef}
          type="button"
          className={
            "chip chip--project" +
            (open ? " is-open" : "") +
            (!activeProject ? " chip--muted" : "")
          }
          disabled={disabled}
          aria-haspopup="menu"
          aria-expanded={open}
          data-testid={instructionTip ? "project-instruction-chip" : undefined}
          title={instructionTip ?? undefined}
          onClick={() => setOpen((v) => !v)}
        >
          <IconFolder size={14} />
          <span className="chip__label">{label}</span>
          <IconChevronDown size={12} />
        </button>
      </Tip>
      {open &&
        pos &&
        typeof document !== "undefined" &&
        createPortal(
          <div
            ref={popRef}
            className="cmm__pop cmm__pop--portal cpm__pop"
            role="menu"
            aria-label={labels.pickProject}
            style={popStyle as CSSProperties}
          >
            <div className="cpm__search">
              <IconSearch size={14} aria-hidden />
              <input
                className="cpm__search-input"
                value={query}
                placeholder={labels.searchProjects ?? labels.pickProject}
                onChange={(event) => setQuery(event.target.value)}
                aria-label={labels.searchProjects ?? labels.pickProject}
              />
            </div>
            {visibleProjects.length > 0 ? (
              <div
                className="cpm__list"
                style={{ maxHeight: LIST_MAX_H }}
                role="group"
                aria-label={labels.pickProject}
              >
                {visibleProjects.map((p) => {
                  const active = activeProject?.id === p.id;
                  return (
                    <button
                      key={p.id}
                      type="button"
                      role="menuitem"
                      className={
                        "cmm__opt cpm__item" + (active ? " is-active" : "")
                      }
                      title={p.path}
                      onClick={() => {
                        onSelect(p);
                        setOpen(false);
                      }}
                    >
                      <IconFolder size={14} aria-hidden />
                      <span className="cmm__opt-main">
                        <span className="cmm__opt-title">{p.name}</span>
                      </span>
                      {active ? (
                        <span className="cmm__opt-check" aria-hidden>
                          <IconCheck size={16} />
                        </span>
                      ) : null}
                    </button>
                  );
                })}
              </div>
            ) : null}
            <div className="cpm__actions">
              <button
                type="button"
                role="menuitem"
                className="cpm__action cpm__action--add"
                onClick={() => {
                  setOpen(false);
                  onAdd();
                }}
              >
                <IconPlus size={14} aria-hidden />
                <span>{labels.addProject}</span>
              </button>
              <button
                type="button"
                role="menuitem"
                className={
                  "cpm__action" + (!activeProject ? " is-active" : "")
                }
                onClick={() => {
                  onSelect(null);
                  setOpen(false);
                }}
              >
                <IconClose size={14} aria-hidden />
                <span>
                  {labels.workNotInProject ?? labels.noProject}
                </span>
              </button>
            </div>
          </div>,
          document.body,
        )}
    </div>
  );
}

type WorktreeMenuProps = {
  activeProject: ProjectOption | null;
  worktrees: GitWorktreeEntry[];
  worktreesAvailable: boolean | null;
  worktreesLoading?: boolean;
  worktreesReason?: string | null;
  labels: Props["labels"];
  disabled?: boolean;
  onSwitchWorktree?: (wt: GitWorktreeEntry) => void;
  onOpen?: () => void;
};

export function ComposerWorktreeMenu({
  activeProject,
  worktrees,
  worktreesAvailable,
  worktreesLoading = false,
  worktreesReason = null,
  labels,
  disabled,
  onSwitchWorktree,
  onOpen,
}: WorktreeMenuProps) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const popRef = useRef<HTMLDivElement>(null);
  const onOpenRef = useRef(onOpen);
  onOpenRef.current = onOpen;

  const show = !!activeProject && worktreesAvailable === true;
  const current = worktrees.find((wt) =>
    pathsEqual(wt.path, activeProject?.path),
  );
  const needle = query.trim().toLowerCase();
  const visible = needle
    ? worktrees.filter((wt) => {
        const name = worktreeLabel(wt).toLowerCase();
        return name.includes(needle) || wt.path.toLowerCase().includes(needle);
      })
    : worktrees;
  const { pos, style: popStyle } = useFloatingMenu({
    open,
    surfaceId: "composer-worktree",
    triggerRef,
    panelRef: popRef,
    roots: [rootRef],
    onClose: () => setOpen(false),
    placement: "up",
    fitContent: true,
    minWidth: 260,
    estHeight: Math.min(360, 72 + Math.max(visible.length, 1) * 44),
    gap: 8,
    deps: [visible.length, query],
  });

  useEffect(() => {
    if (!open) return;
    onOpenRef.current?.();
  }, [open]);
  useEffect(() => {
    if (!open) setQuery("");
  }, [open]);

  if (!show) return null;
  const label = current ? worktreeLabel(current) : labels.worktreeMain;

  return (
    <div ref={rootRef} className={`cpm cpm--worktree${open ? " is-open" : ""}`}>
      <Tip label={current?.path || labels.worktrees} disabled={open}>
        <button
          ref={triggerRef}
          type="button"
          className={"chip chip--worktree" + (open ? " is-open" : "")}
          disabled={disabled}
          aria-haspopup="menu"
          aria-expanded={open}
          onClick={() => setOpen((value) => !value)}
        >
          <IconFork size={14} />
          <span className="chip__label">{label}</span>
          <IconChevronDown size={12} />
        </button>
      </Tip>
      {open &&
        pos &&
        typeof document !== "undefined" &&
        createPortal(
          <div
            ref={popRef}
            className="cmm__pop cmm__pop--portal cpm__pop"
            role="menu"
            aria-label={labels.worktrees}
            style={popStyle as CSSProperties}
          >
            <div className="cpm__search">
              <IconSearch size={14} aria-hidden />
              <input
                className="cpm__search-input"
                value={query}
                placeholder={labels.searchWorktrees ?? labels.worktrees}
                onChange={(event) => setQuery(event.target.value)}
                aria-label={labels.searchWorktrees ?? labels.worktrees}
              />
            </div>
            {visible.length > 0 ? (
              <ul className="cpm__worktrees-list">
                {visible.map((wt) => {
                  const isCurrent = pathsEqual(wt.path, activeProject?.path);
                  const name = worktreeLabel(wt);
                  return (
                    <li key={wt.path}>
                      <button
                        type="button"
                        role="menuitem"
                        className={
                          "cmm__opt cpm__item cpm__worktree" +
                          (isCurrent ? " is-active" : "")
                        }
                        title={wt.path}
                        disabled={isCurrent || !onSwitchWorktree}
                        onClick={() => {
                          if (isCurrent || !onSwitchWorktree) return;
                          setOpen(false);
                          onSwitchWorktree(wt);
                        }}
                      >
                        <span className="cpm__worktree-row">
                          <span className="cpm__worktree-name">{name}</span>
                          {isCurrent && worktreesReason?.trim() ? (
                            <span className="cpm__worktree-meta">
                              {worktreesReason}
                            </span>
                          ) : null}
                        </span>
                        {isCurrent ? (
                          <span className="cmm__opt-check" aria-hidden>
                            <IconCheck size={16} />
                          </span>
                        ) : null}
                      </button>
                    </li>
                  );
                })}
              </ul>
            ) : (
              <p className="cpm__worktrees-empty">
                {worktreesLoading
                  ? labels.worktreesLoading
                  : worktreesReason?.trim()
                    ? labels.worktreesUnavailable
                    : labels.worktreesEmpty}
              </p>
            )}
          </div>,
          document.body,
        )}
    </div>
  );
}
