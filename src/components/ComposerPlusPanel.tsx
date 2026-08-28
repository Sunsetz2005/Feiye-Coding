/**
 * Unified composer command panel (+ button and `/` slash).
 *
 * IMPORTANT: Render and keyboard nav share one `entries` array so they can
 * never desync (which caused “see many rows but only 2 keyboard targets”).
 */

import {
  useEffect,
  useRef,
  type CSSProperties,
  type ReactNode,
  type Ref,
} from "react";
import type { Locale } from "@/i18n";
import { createT } from "@/i18n";
import type { SlashItem } from "@/lib/slashCatalog";
import {
  IconActivity,
  IconArrowsMinimize,
  IconAttach,
  IconAutomations,
  IconBox,
  IconCircleDashed,
  IconClipboardList,
  IconDoctor,
  IconNewChat,
  IconPlug,
  IconPuzzle,
  IconSettings,
  IconShieldCheck,
  IconSkills,
  IconTarget,
  IconFolder,
  IconFolderPlus,
} from "@/components/icons";

const ICON_SIZE = 16;

/** Selectable row (keyboard + click). */
export type ComposerPlusEntry =
  | { id: "upload"; kind: "upload" }
  | {
      id: string;
      kind: "action";
      action:
        | "folder"
        | "finder"
        | "project"
        | "goal"
        | "plan"
        | "record-skill";
      title: string;
      description?: string;
      /** Inline feedback produced by this row's latest action. */
      feedback?: boolean;
      disabled?: boolean;
    }
  | { id: string; kind: "slash"; item: SlashItem };

/** Visual row including section headers (headers are not in keyboard nav). */
export type ComposerPlusRow =
  | { type: "section"; id: string; label: string }
  | { type: "entry"; entry: ComposerPlusEntry; navIndex: number };

function slashItemIcon(item: SlashItem): ReactNode {
  if (item.kind === "skill") {
    return <IconPuzzle size={ICON_SIZE} />;
  }
  const key = item.action ?? item.mode ?? item.name;
  switch (key) {
    case "goal":
      return <IconTarget size={ICON_SIZE} />;
    case "plan":
      return <IconClipboardList size={ICON_SIZE} />;
    case "compact":
      return <IconArrowsMinimize size={ICON_SIZE} />;
    case "status":
      return <IconActivity size={ICON_SIZE} />;
    case "mcp":
      return <IconPlug size={ICON_SIZE} />;
    case "doctor":
      return <IconDoctor size={ICON_SIZE} />;
    case "settings":
      return <IconSettings size={ICON_SIZE} />;
    case "automations":
      return <IconAutomations size={ICON_SIZE} />;
    case "newChat":
    case "new":
      return <IconNewChat size={ICON_SIZE} />;
    case "yolo":
    case "always-approve":
      return <IconShieldCheck size={ICON_SIZE} />;
    default:
      if (item.kind === "mode") return <IconCircleDashed size={ICON_SIZE} />;
      if (item.kind === "action") return <IconBox size={ICON_SIZE} />;
      return <IconSkills size={ICON_SIZE} />;
  }
}

/** Build keyboard-nav flat list: optional upload + commands + skills. */
export function buildComposerPlusEntries(opts: {
  showUpload: boolean;
  commands: SlashItem[];
  skills: SlashItem[];
  /** Codex-style + menu actions. Omit for the editor `/` palette. */
  actions?: Extract<ComposerPlusEntry, { kind: "action" }>[];
  /** When true, skills are presented as installed plugin actions. */
  plusMenu?: boolean;
}): ComposerPlusEntry[] {
  const out: ComposerPlusEntry[] = [];
  if (opts.showUpload) out.push({ id: "upload", kind: "upload" });
  for (const action of opts.actions ?? []) out.push(action);
  for (const item of opts.commands) {
    out.push({ id: item.id, kind: "slash", item });
  }
  for (const item of opts.skills) {
    out.push({ id: item.id, kind: "slash", item });
  }
  return out;
}

/**
 * Rows for rendering: section headers + the same entries used for keyboard.
 * Order always: 添加 → 命令 (builtins like 目标/计划) → 技能.
 * Built-in commands must never sit under the skills section.
 */
export function buildComposerPlusRows(
  entries: ComposerPlusEntry[],
  labels: {
    add: string;
    commands: string;
    skills: string;
    plugins?: string;
  },
  plusMenu = false,
): ComposerPlusRow[] {
  const rows: ComposerPlusRow[] = [];
  let navIndex = 0;
  let addedAddSection = false;
  let addedCmdSection = false;
  let addedSkillSection = false;

  for (const entry of entries) {
    if (entry.kind === "upload" || entry.kind === "action") {
      if (!addedAddSection) {
        rows.push({ type: "section", id: "sec-add", label: labels.add });
        addedAddSection = true;
      }
      rows.push({ type: "entry", entry, navIndex: navIndex++ });
      continue;
    }

    if (entry.item.kind === "skill") {
      if (!addedSkillSection) {
        rows.push({
          type: "section",
          id: "sec-skills",
          label: plusMenu ? labels.plugins ?? labels.skills : labels.skills,
        });
        addedSkillSection = true;
      }
      rows.push({ type: "entry", entry, navIndex: navIndex++ });
      continue;
    }

    // mode / action / prompt → built-in commands (目标, 计划, …)
    if (!addedCmdSection) {
      rows.push({ type: "section", id: "sec-cmd", label: labels.commands });
      addedCmdSection = true;
    }
    rows.push({ type: "entry", entry, navIndex: navIndex++ });
  }
  return rows;
}

function composerActionIcon(
  action: Extract<ComposerPlusEntry, { kind: "action" }>["action"],
): ReactNode {
  switch (action) {
    case "folder":
      return <IconFolderPlus size={ICON_SIZE} />;
    case "finder":
      return <IconFolder size={ICON_SIZE} />;
    case "project":
      return <IconFolderPlus size={ICON_SIZE} />;
    case "goal":
      return <IconTarget size={ICON_SIZE} />;
    case "plan":
      return <IconClipboardList size={ICON_SIZE} />;
    case "record-skill":
      return <IconActivity size={ICON_SIZE} />;
  }
}

/** Whether the upload row matches a slash filter query. */
export function uploadMatchesQuery(
  query: string,
  labels: { title: string; hint: string },
): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  const hay = [
    labels.title,
    labels.hint,
    "upload",
    "file",
    "files",
    "attach",
    "folder",
    "上传",
    "文件",
    "附件",
  ]
    .join(" ")
    .toLowerCase();
  return hay.includes(q);
}

export function ComposerPlusPanel({
  open,
  mode,
  locale,
  style,
  panelRef,
  entries,
  filterQuery,
  skillsLoading,
  activeIndex,
  onActiveIndexChange,
  onSelectUpload,
  onSelectAction,
  onSelectSlash,
  resolveTitle,
  resolveDescription,
}: {
  open: boolean;
  /**
   * The `+` surface is an action menu; the inline `/` surface is a listbox.
   * Keep this explicit because an empty slash query is still a slash listbox.
   */
  mode: "plus" | "slash";
  locale: Locale;
  style?: CSSProperties;
  panelRef?: Ref<HTMLDivElement | null>;
  /** Sole list of selectable items — same array the host uses for keyboard. */
  entries: ComposerPlusEntry[];
  /** Live filter string (shown in header when non-empty). */
  filterQuery?: string;
  skillsLoading?: boolean;
  activeIndex: number;
  onActiveIndexChange: (i: number) => void;
  onSelectUpload: () => void;
  onSelectAction?: (
    entry: Extract<ComposerPlusEntry, { kind: "action" }>,
  ) => void;
  onSelectSlash: (item: SlashItem) => void;
  resolveTitle: (item: SlashItem) => string;
  resolveDescription: (item: SlashItem) => string;
}) {
  const tr = createT(locale);
  const listRef = useRef<HTMLDivElement | null>(null);

  const setRefs = (node: HTMLDivElement | null) => {
    listRef.current = node;
    if (typeof panelRef === "function") panelRef(node);
    else if (panelRef && "current" in panelRef) {
      (panelRef as { current: HTMLDivElement | null }).current = node;
    }
  };

  const rows = buildComposerPlusRows(
    entries,
    {
      add: tr("composer.add"),
      commands: tr("slash.section.commands"),
      skills: tr("composer.skills"),
      plugins: tr("composer.plugins"),
    },
    mode === "plus",
  );

  useEffect(() => {
    if (!open) return;
    const panel = listRef.current;
    if (!panel) return;
    const el = panel.querySelector<HTMLElement>(
      `[data-plus-idx="${activeIndex}"]`,
    );
    if (!el) return;
    const pRect = panel.getBoundingClientRect();
    const eRect = el.getBoundingClientRect();
    if (eRect.top < pRect.top) {
      panel.scrollTop -= pRect.top - eRect.top;
    } else if (eRect.bottom > pRect.bottom) {
      panel.scrollTop += eRect.bottom - pRect.bottom;
    }
  }, [activeIndex, open, entries.length]);

  const prevLen = useRef(entries.length);
  useEffect(() => {
    if (!open) return;
    if (prevLen.current === entries.length) return;
    prevLen.current = entries.length;
    const panel = listRef.current;
    if (panel) panel.scrollTop = 0;
  }, [entries.length, open]);

  if (!open) return null;

  const q = (filterQuery ?? "").trim();
  const empty = entries.length === 0 && !skillsLoading;

  return (
    <div
      id="composer-plus-panel"
      ref={setRefs}
      className="menu-panel composer-plus composer-plus--portal"
      role={mode === "slash" ? "listbox" : "menu"}
      aria-activedescendant={
        entries[activeIndex] ? `plus-opt-${activeIndex}` : undefined
      }
      aria-orientation="vertical"
      data-filter-query={q}
      data-mode={mode}
      style={style}
    >
      {mode === "slash" && q ? (
        <div className="composer-plus__filter" aria-live="polite">
          <span className="composer-plus__filter-label">/</span>
          <span className="composer-plus__filter-q">{q}</span>
          <span className="composer-plus__filter-count">
            {entries.length}
          </span>
        </div>
      ) : null}

      {skillsLoading && entries.length === 0 && (
        <div
          className="composer-plus__item composer-plus__item--muted"
          aria-busy
        >
          <span className="composer-plus__ico" aria-hidden>
            <IconSkills size={ICON_SIZE} />
          </span>
          <span className="composer-plus__title">
            {tr("composer.skillsLoading")}
          </span>
        </div>
      )}

      {rows.map((row) => {
        if (row.type === "section") {
          return (
            <div key={row.id} className="composer-plus__section">
              {row.label}
            </div>
          );
        }
        const { entry, navIndex } = row;
        const active = navIndex === activeIndex;

        if (entry.kind === "upload") {
          return (
            <button
              key={`upload-${navIndex}`}
              id={`plus-opt-${navIndex}`}
              type="button"
              role={mode === "slash" ? "option" : "menuitem"}
              aria-selected={mode === "slash" ? active : undefined}
              data-plus-idx={navIndex}
              className={
                "composer-plus__item" + (active ? " is-active" : "")
              }
              onMouseEnter={() => onActiveIndexChange(navIndex)}
              onClick={onSelectUpload}
            >
              <span className="composer-plus__ico" aria-hidden>
                <IconAttach size={ICON_SIZE} />
              </span>
              <span className="composer-plus__title">
                {tr("composer.addFiles")}
              </span>
              <span className="composer-plus__desc">
                {tr("composer.addFilesHint")}
              </span>
            </button>
          );
        }

        if (entry.kind === "action") {
          return (
            <button
              key={`${entry.id}-${navIndex}`}
              id={`plus-opt-${navIndex}`}
              type="button"
              role={mode === "slash" ? "option" : "menuitem"}
              aria-selected={mode === "slash" ? active : undefined}
              aria-disabled={entry.disabled || undefined}
              disabled={entry.disabled}
              data-plus-idx={navIndex}
              className={
                "composer-plus__item" +
                (active ? " is-active" : "") +
                (entry.disabled ? " is-disabled" : "")
              }
              onMouseEnter={() => onActiveIndexChange(navIndex)}
              onClick={() => onSelectAction?.(entry)}
            >
              <span className="composer-plus__ico" aria-hidden>
                {composerActionIcon(entry.action)}
              </span>
              <span className="composer-plus__title">{entry.title}</span>
              {entry.description ? (
                <span
                  className={
                    "composer-plus__desc" +
                    (entry.feedback ? " is-feedback" : "")
                  }
                  role={entry.feedback ? "status" : undefined}
                  aria-live={entry.feedback ? "polite" : undefined}
                >
                  {entry.description}
                </span>
              ) : null}
            </button>
          );
        }

        const item = entry.item;
        const title = resolveTitle(item);
        const desc = resolveDescription(item);
        const right =
          desc.trim() ||
          (item.kind === "skill" && item.source ? item.source : "") ||
          `/${item.name}`;

        return (
          <button
            key={`${entry.id}#${navIndex}`}
            id={`plus-opt-${navIndex}`}
            type="button"
            role={mode === "slash" ? "option" : "menuitem"}
            aria-selected={mode === "slash" ? active : undefined}
            data-plus-idx={navIndex}
            className={
              "composer-plus__item" + (active ? " is-active" : "")
            }
            onMouseEnter={() => onActiveIndexChange(navIndex)}
            onClick={() => onSelectSlash(item)}
          >
            <span className="composer-plus__ico" aria-hidden>
              {slashItemIcon(item)}
            </span>
            <span className="composer-plus__title">
              {title}
              {item.kind === "skill" && item.suggested ? (
                <span className="composer-plus__suggested">
                  {tr("composer.skillSuggested")}
                </span>
              ) : null}
            </span>
            {right ? (
              <span className="composer-plus__desc">{right}</span>
            ) : null}
          </button>
        );
      })}

      {empty && (
        <div className="composer-plus__item composer-plus__item--muted">
          <span className="composer-plus__title">
            {mode === "slash"
              ? tr("slash.empty")
              : tr("composer.skillsEmpty")}
          </span>
        </div>
      )}
    </div>
  );
}
