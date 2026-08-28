import {
  useMemo,
  type CSSProperties,
  type Dispatch,
  type ReactNode,
  type RefObject,
  type SetStateAction,
} from "react";
import { createPortal } from "react-dom";
import { createT, type Locale } from "@/i18n";
import type { Attachment } from "@/lib/attachments";
import { isImagePath } from "@/lib/attachments";
import type { MemoryContextPackV1 } from "@/lib/api";
import {
  isDraftEmpty,
  parseStoredContent,
} from "@/lib/draftDoc";
import type { GitWorktreeEntry } from "@/lib/gitWorktree";
import {
  canSend,
  canStop,
  canType,
  type SessionState,
} from "@/lib/session";
import {
  queuePreviewText,
  shouldEnqueueSend,
  type QueuedSend,
  type QueuePreviewLabels,
} from "@/lib/sendQueue";
import type {
  EffortOption,
  ModelOption,
  PermissionPolicyId,
} from "@/lib/grokCatalog";
import type { ContextUsageDisplay } from "@/lib/contextUsage";
import type { SlashItem } from "@/lib/slashCatalog";
import {
  AttachmentCard,
  type AttachmentCardLabels,
} from "@/components/AttachmentCard";
import { ComposerEditor } from "@/components/ComposerEditor";
import {
  ComposerAccessMenu,
  ComposerModelMenu,
} from "@/components/ComposerModelMenu";
import { ComposerPlanModeButton } from "@/components/ComposerPlanModeButton";
import {
  ComposerPlusPanel,
  type ComposerPlusEntry,
} from "@/components/ComposerPlusPanel";
import {
  ComposerProjectMenu,
  type ProjectOption,
} from "@/components/ComposerProjectMenu";
import { ContextUsageChip } from "@/components/ContextUsageChip";
import {
  MemoryContextBadge,
  type MemoryContextBadgeLabels,
} from "@/components/MemoryContextBadge";
import {
  TaskProgressRail,
  type TaskProgressRailProps,
} from "@/components/lobe-chat/TaskProgressRail";
import {
  IconClock,
  IconClose,
  IconImagine,
  IconPlus,
  IconSend,
  IconStop,
} from "@/components/icons";
import { Tip } from "@/components/ui/tooltip";

type ComposerActionEntry = Extract<
  ComposerPlusEntry,
  { kind: "action" }
>;

type SlashQuery = {
  start: number;
  query: string;
  end: number;
};

export interface ComposerDockProps {
  locale: Locale;
  welcomeSession?: boolean;
  goalMode: boolean;
  settingsLocked: boolean;
  sessionState: SessionState;
  connecting: boolean;
  dropReady: boolean;
  draft: string;
  attachments: Attachment[];
  attachmentLabels: AttachmentCardLabels;
  contextUsage: ContextUsageDisplay;
  /** Compact running-plan status rail. Replaces the project/goal rail. */
  progress?: TaskProgressRailProps | null;
  /** Permission prompt; rendered above the composer, never as a second decision dock. */
  permission?: ReactNode;
  /** Ask-user / plan-approval dock. Replaces the three-layer composer. */
  takeover?: ReactNode;
  project: {
    active: ProjectOption | null;
    options: ProjectOption[];
    openRequestKey: number;
    worktrees: GitWorktreeEntry[];
    worktreesAvailable: boolean | null;
    worktreesLoading: boolean;
    worktreesReason: string | null;
    onSelect: (project: ProjectOption | null) => void;
    onAdd: () => void;
    onSwitchWorktree: (worktree: GitWorktreeEntry) => void;
    onOpen: () => void;
  };
  queue: {
    items: QueuedSend[];
    flushHold: boolean;
    previewLabels: QueuePreviewLabels;
    onClear: () => void;
    onRemove: (id: string) => void;
    onRetry: () => void;
  };
  memory?: {
    pack: MemoryContextPackV1 | null;
    labels: MemoryContextBadgeLabels;
    onClear: () => void;
  };
  projectInstruction?: {
    path: string;
    truncated: boolean;
    labels: { attached: string; truncated: string };
  } | null;
  menu: {
    open: boolean;
    positioned: boolean;
    plusMode: boolean;
    showPlus: boolean;
    liveSlashPresent: boolean;
    slashFilterQuery: string;
    skillsLoading: boolean;
    activeIndex: number;
    entries: ComposerPlusEntry[];
    style?: CSSProperties;
    onActiveIndexChange: Dispatch<SetStateAction<number>>;
    onPickFiles: () => void | Promise<void>;
    onSelectAction: (
      entry: ComposerActionEntry,
    ) => void | Promise<void>;
    onSelectSlash: (item: SlashItem) => void;
    resolveTitle: (item: SlashItem) => string;
    resolveDescription: (item: SlashItem) => string;
    onClose: () => void;
    onTogglePlus: () => void;
  };
  preferences: {
    mode: string;
    policy: string;
    modelId: string;
    effort: string;
    models: ModelOption[];
    onMode: (value: string) => void;
    onPolicy: (value: PermissionPolicyId) => void;
    onDisablePlan: () => void;
    onClearGoal: () => void;
    onModel: (value: string) => void;
    onEffort: (value: EffortOption["id"]) => void;
    onReset: () => void;
  };
  refs: {
    wrap: RefObject<HTMLDivElement | null>;
    input: RefObject<HTMLDivElement | null>;
    shell: RefObject<HTMLDivElement | null>;
    plusTrigger: RefObject<HTMLButtonElement | null>;
    plusPanel: RefObject<HTMLDivElement | null>;
    menuEntries: RefObject<ComposerPlusEntry[]>;
  };
  onDraftChange: (value: string) => void;
  onRemoveAttachment: (attachment: Attachment) => void;
  onAddAttachment: (attachment: Attachment) => void;
  onPasteFiles: (files: File[]) => void | Promise<void>;
  onPasteMediaFallback: (options?: {
    expectMedia?: boolean;
  }) => void | Promise<void>;
  onSlashQueryChange: (query: SlashQuery | null) => void;
  onCompact: () => void;
  onSend: () => void | Promise<void>;
  onStop: () => void | Promise<void>;
}

/**
 * Bottom composer dock: floating wrap, three-layer inputer, and the compact
 * running-plan rail. Runtime/Host orchestration, permission resolution, and
 * ask-user/plan decisions stay with App; this component renders current values
 * and delegates user intent through explicit callbacks or slots.
 */
export function ComposerDock({
  locale,
  welcomeSession = false,
  goalMode,
  settingsLocked,
  sessionState,
  connecting,
  dropReady,
  draft,
  attachments,
  attachmentLabels,
  contextUsage,
  progress,
  permission,
  takeover,
  project,
  queue,
  memory,
  projectInstruction = null,
  menu,
  preferences,
  refs,
  onDraftChange,
  onRemoveAttachment,
  onAddAttachment,
  onPasteFiles,
  onPasteMediaFallback,
  onSlashQueryChange,
  onCompact,
  onSend,
  onStop,
}: ComposerDockProps) {
  const tr = useMemo(() => createT(locale), [locale]);
  const galleryPaths = useMemo(
    () =>
      attachments
        .filter((attachment) =>
          !attachment.isDir && isImagePath(attachment.path),
        )
        .map((attachment) => attachment.path),
    [attachments],
  );
  const queueRows = useMemo(
    () =>
      queue.items.map((item) => ({
        item,
        fullPreview: queuePreviewText(
          item.storedDisplay,
          item.attachments,
          200,
          queue.previewLabels,
        ),
        shortPreview: queuePreviewText(
          item.storedDisplay,
          item.attachments,
          72,
          queue.previewLabels,
        ),
      })),
    [queue.items, queue.previewLabels],
  );
  const hasBody = useMemo(
    () =>
      !isDraftEmpty(parseStoredContent(draft)) || attachments.length > 0,
    [attachments.length, draft],
  );

  const taskProgressVisible = progress != null;

  return (
    <div
      ref={refs.wrap}
      className={
        "composer-wrap composer-wrap--float" +
        (welcomeSession ? " composer-wrap--welcome" : "")
      }
    >
      {progress ? <TaskProgressRail {...progress} /> : null}
      {permission}
      {takeover ?? (
    <div className="composer-dock">
      {!taskProgressVisible ? (
        <div className="composer-context-rail">
          {goalMode ? (
            <button
              type="button"
              className="composer-context-rail__activity"
              onClick={() => {
                refs.input.current?.focus();
              }}
            >
              <IconImagine size={14} aria-hidden />
              <span className="composer-context-rail__label">
                {tr("composer.goal")}
              </span>
            </button>
          ) : (
            <ComposerProjectMenu
              activeProject={project.active}
              projects={project.options}
              openRequestKey={project.openRequestKey}
              labels={{
                noProject: tr("composer.noProject"),
                pickProject: tr("composer.pickProject"),
                addProject: tr("composer.addProject"),
                worktrees: tr("composer.worktrees"),
                worktreesEmpty: tr("composer.worktreesEmpty"),
                worktreesUnavailable: tr("composer.worktreesUnavailable"),
                worktreeCurrent: tr("composer.worktreeCurrent"),
                worktreeSwitch: tr("composer.worktreeSwitch"),
                worktreeMain: tr("composer.worktreeMain"),
                worktreeDetached: tr("composer.worktreeDetached"),
              }}
              worktrees={project.worktrees}
              worktreesAvailable={project.worktreesAvailable}
              worktreesLoading={project.worktreesLoading}
              worktreesReason={project.worktreesReason}
              disabled={settingsLocked}
              onSelect={project.onSelect}
              onAdd={project.onAdd}
              onSwitchWorktree={project.onSwitchWorktree}
              onOpen={project.onOpen}
            />
          )}
          {projectInstruction ? (
            <p
              className="composer-context-rail__instruction"
              data-testid="project-instruction-chip"
              title={
                projectInstruction.truncated
                  ? projectInstruction.labels.truncated
                  : projectInstruction.labels.attached
              }
            >
              {projectInstruction.truncated
                ? projectInstruction.labels.truncated
                : projectInstruction.labels.attached}
            </p>
          ) : null}
        </div>
      ) : null}
      <div
        ref={refs.shell}
        className={"composer" + (dropReady ? " composer--drop-ready" : "")}
      >
        {queueRows.length > 0 ? (
          <div
            className="composer__queue"
            aria-label={tr("composer.queueCount", {
              n: String(queueRows.length),
            })}
          >
            <div className="composer__queue-head">
              <IconClock size={14} aria-hidden />
              <span className="composer__queue-title">
                {tr("composer.queueCount", {
                  n: String(queueRows.length),
                })}
              </span>
              <button
                type="button"
                className="composer__queue-clear"
                onClick={queue.onClear}
              >
                {tr("composer.queueClear")}
              </button>
            </div>
            {queue.flushHold ? (
              <div className="composer__queue-hold" role="status">
                <span className="composer__queue-hold-text">
                  {tr("composer.queueHold")}
                </span>
                <button
                  type="button"
                  className="composer__queue-hold-retry"
                  onClick={queue.onRetry}
                >
                  {tr("composer.queueHoldRetry")}
                </button>
              </div>
            ) : null}
            <ul className="composer__queue-list">
              {queueRows.map(({ item, fullPreview, shortPreview }, index) => (
                <li key={item.id} className="composer__queue-item">
                  <span className="composer__queue-idx" aria-hidden>
                    {index + 1}
                  </span>
                  <span className="composer__queue-text" title={fullPreview}>
                    {shortPreview}
                  </span>
                  <button
                    type="button"
                    className="composer__queue-remove"
                    aria-label={tr("composer.queueRemove")}
                    onClick={() => queue.onRemove(item.id)}
                  >
                    <IconClose size={12} />
                  </button>
                </li>
              ))}
            </ul>
          </div>
        ) : null}
        {memory ? (
          <MemoryContextBadge
            pack={memory.pack}
            locale={locale}
            labels={memory.labels}
            disabled={connecting || !canType(sessionState)}
            onClear={memory.onClear}
          />
        ) : null}
        {attachments.length > 0 ? (
          <div
            className="composer__attachments"
            aria-label={tr("composer.attachCount", {
              n: String(attachments.length),
            })}
          >
            {attachments.map((attachment) => (
              <AttachmentCard
                key={attachment.path}
                attachment={attachment}
                variant="chip"
                labels={attachmentLabels}
                galleryPaths={galleryPaths}
                onRemove={onRemoveAttachment}
                onAddToComposer={onAddAttachment}
              />
            ))}
          </div>
        ) : null}
        {menu.open &&
          menu.positioned &&
          typeof document !== "undefined" &&
          createPortal(
            <ComposerPlusPanel
              open
              mode={menu.plusMode ? "plus" : "slash"}
              panelRef={refs.plusPanel}
              locale={locale}
              entries={menu.entries}
              filterQuery={
                menu.liveSlashPresent
                  ? menu.slashFilterQuery
                  : undefined
              }
              skillsLoading={menu.skillsLoading}
              activeIndex={menu.activeIndex}
              onActiveIndexChange={menu.onActiveIndexChange}
              onSelectUpload={() => {
                void menu.onPickFiles();
              }}
              onSelectAction={(entry) => {
                void menu.onSelectAction(entry);
              }}
              onSelectSlash={menu.onSelectSlash}
              resolveTitle={menu.resolveTitle}
              resolveDescription={menu.resolveDescription}
              style={{
                ...menu.style,
                zIndex: 10050,
              }}
            />,
            document.body,
          )}
        <ComposerEditor
          editorRef={refs.input}
          className="composer__input"
          value={draft}
          disabled={!canType(sessionState)}
          placeholder={
            goalMode
              ? tr("composer.goalPlaceholder")
              : tr("composer.placeholder")
          }
          onChange={onDraftChange}
          onPasteFiles={(files) => {
            void onPasteFiles(files);
          }}
          onPasteMediaFallback={(options) => {
            void onPasteMediaFallback(options);
          }}
          onSlashQueryChange={onSlashQueryChange}
          onKeyDown={(event) => {
            if (
              event.nativeEvent.isComposing ||
              (event.nativeEvent as KeyboardEvent).keyCode === 229
            ) {
              return;
            }
            if (menu.open) {
              const flat = refs.menuEntries.current;
              const count = flat.length;
              if (event.key === "ArrowDown") {
                event.preventDefault();
                if (!count) return;
                menu.onActiveIndexChange((index) => (index + 1) % count);
                return;
              }
              if (event.key === "ArrowUp") {
                event.preventDefault();
                if (!count) return;
                menu.onActiveIndexChange(
                  (index) => (index - 1 + count) % count,
                );
                return;
              }
              if (event.key === "Enter" && !event.shiftKey) {
                event.preventDefault();
                const entry =
                  flat[
                    Math.min(
                      Math.max(0, menu.activeIndex),
                      Math.max(0, count - 1),
                    )
                  ];
                if (!entry) return;
                if (entry.kind === "upload") void menu.onPickFiles();
                else if (entry.kind === "action") {
                  void menu.onSelectAction(entry);
                } else {
                  menu.onSelectSlash(entry.item);
                }
                return;
              }
              if (event.key === "Escape") {
                event.preventDefault();
                menu.onClose();
                return;
              }
              if (event.key === "Tab" && count > 0) {
                event.preventDefault();
                const entry =
                  flat[
                    Math.min(
                      Math.max(0, menu.activeIndex),
                      count - 1,
                    )
                  ]!;
                if (entry.kind === "upload") void menu.onPickFiles();
                else if (entry.kind === "action") {
                  void menu.onSelectAction(entry);
                } else {
                  menu.onSelectSlash(entry.item);
                }
                return;
              }
            }
            if (event.key === "Enter" && !event.shiftKey) {
              event.preventDefault();
              if (hasBody && sessionState !== "awaiting_permission") {
                void onSend();
              }
            }
            if (event.key === "Escape") menu.onClose();
          }}
        />
        <div className="composer__row">
          <Tip label={tr("composer.add")}>
            <button
              ref={refs.plusTrigger}
              type="button"
              className={
                "icon-btn icon-btn--plus" +
                (menu.showPlus ? " is-open" : "")
              }
              aria-label={tr("composer.add")}
              aria-haspopup="menu"
              aria-expanded={menu.showPlus}
              aria-controls={
                menu.showPlus ? "composer-plus-panel" : undefined
              }
              onClick={menu.onTogglePlus}
            >
              <IconPlus size={18} />
            </button>
          </Tip>
          <ComposerAccessMenu
            mode={preferences.mode}
            policy={preferences.policy}
            disabled={settingsLocked}
            labels={{
              access: tr("composer.access"),
              accessHint: tr("composer.accessHint"),
              mode: tr("composer.mode"),
              modeAgent: tr("mode.agent"),
              modePlan: tr("mode.plan"),
              modeAsk: tr("mode.ask"),
              modeAgentDesc: tr("mode.agentDesc"),
              modePlanDesc: tr("mode.planDesc"),
              modeAskDesc: tr("mode.askDesc"),
              permission: tr("composer.permission"),
              policyAsk: tr("policy.ask"),
              policyAcceptEdits: tr("policy.accept_edits"),
              policySession: tr("policy.allow_for_session"),
              policyDontAsk: tr("policy.dont_ask"),
              policyYolo: tr("policy.always_approve"),
              policyAskDesc: tr("policy.askDesc"),
              policyAcceptEditsDesc: tr("policy.accept_editsDesc"),
              policySessionDesc: tr("policy.allow_for_sessionDesc"),
              policyDontAskDesc: tr("policy.dont_askDesc"),
              policyYoloDesc: tr("policy.always_approveDesc"),
              policyShortAsk: tr("policy.short.ask"),
              policyShortAccept: tr("policy.short.accept_edits"),
              policyShortSession: tr("policy.short.allow_for_session"),
              policyShortDontAsk: tr("policy.short.dont_ask"),
              policyShortYolo: tr("policy.short.always_approve"),
            }}
            onMode={preferences.onMode}
            onPolicy={preferences.onPolicy}
          />
          {preferences.mode === "plan" ? (
            <ComposerPlanModeButton
              label={tr("composer.planMode")}
              disabled={settingsLocked}
              onDisable={preferences.onDisablePlan}
            />
          ) : null}
          {goalMode ? (
            <Tip label={tr("composer.goalHint")}>
              <button
                type="button"
                className="chip chip--goal"
                disabled={settingsLocked}
                onClick={preferences.onClearGoal}
                aria-label={tr("composer.goalClear")}
              >
                <IconImagine size={14} />
                <span className="chip__label">{tr("composer.goal")}</span>
                <IconClose size={12} />
              </button>
            </Tip>
          ) : null}
          <span className="composer__spacer" />
          <ContextUsageChip
            display={contextUsage}
            labels={{
              aria: tr("context.chipAria"),
              menuTitle: tr("context.menuTitle"),
              used: tr("context.used"),
              remaining: tr("context.remaining"),
              total: tr("context.total"),
              latestInput: tr("context.latestInput"),
              latestOutput: tr("context.latestOutput"),
              cacheRead: tr("context.cacheRead"),
              reasoning: tr("context.reasoning"),
              model: tr("context.model"),
              modelCalls: tr("context.modelCalls"),
              exactSource: tr("context.exactSource"),
              updatedAt: tr("context.updatedAt"),
              waiting: tr("context.waiting"),
              capacityUnknown: tr("context.capacityUnknown"),
              lastCompact: tr("context.lastCompact"),
              lastCompactNone: tr("context.lastCompactNone"),
              tokensRange: tr("compact.tokensRange"),
              compactAction: tr("context.compactAction"),
              auto: tr("context.triggerAuto"),
              manual: tr("context.triggerManual"),
            }}
            compactDisabled={settingsLocked}
            onCompact={onCompact}
          />
          <ComposerModelMenu
            modelId={preferences.modelId}
            effort={preferences.effort}
            models={preferences.models}
            disabled={settingsLocked}
            labels={{
              model: tr("composer.model"),
              effort: tr("composer.effort"),
              effortHigh: tr("effort.high"),
              effortMedium: tr("effort.medium"),
              effortLow: tr("effort.low"),
              resetDefaults: tr("composer.resetDefaults"),
              resetDefaultsHint: tr("composer.resetDefaultsHint"),
            }}
            onModel={preferences.onModel}
            onEffort={preferences.onEffort}
            onReset={preferences.onReset}
          />
          {canStop(sessionState) ? (
            <Tip label={tr("composer.stop")}>
              <button
                type="button"
                className="icon-btn icon-btn--primary icon-btn--stop"
                onClick={() => void onStop()}
                aria-label={tr("composer.stop")}
              >
                <IconStop size={14} />
              </button>
            </Tip>
          ) : (
            <Tip label={tr("composer.send")}>
              <button
                type="button"
                className="icon-btn icon-btn--primary"
                disabled={
                  (!canSend(sessionState) &&
                    !shouldEnqueueSend(sessionState, connecting)) ||
                  !hasBody ||
                  sessionState === "awaiting_permission"
                }
                onClick={() => void onSend()}
                aria-label={tr("composer.send")}
              >
                <IconSend size={16} />
              </button>
            </Tip>
          )}
        </div>
      </div>
    </div>
      )}
    </div>
  );
}
