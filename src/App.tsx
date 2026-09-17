import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type MouseEvent as ReactMouseEvent,
} from "react";
import { useFloatingMenu } from "@/lib/floatingMenu";
import {
  applyNativeWindowTheme,
  applyThemeToDocument,
  loadTheme,
  saveTheme,
  toggleTheme,
  type Theme,
} from "@/lib/theme";
import {
  ASIDE_WIDTH_MAX,
  ASIDE_WIDTH_MIN,
  DEFAULT_LAYOUT,
  clampAsideWidth,
  loadLayout,
  saveLayout,
} from "@/lib/layout";
import {
  hitDragZoneFromRects,
  querySidebarEl,
  toClientDragPoint,
} from "@/lib/dragZone";
import {
  applyContextCompact,
  applyGeneratedImage,
  applyToolEvent,
  applyTurnError,
  applyTurnMarker,
  parseCompactContent,
  parseToolStepContent,
  canSend,
  clearPriorTurnStreaming,
  isSessionBusy,
  isSessionLiveStreaming,
  preferSessionMessages,
  presentErrorBanner,
  type ErrorBannerView,
  buildSegmentsFromLegacy,
  splitThoughtPhases,
  truncateBeforeLastUser,
  truncateThroughUserPrompt,
  canRewindToUserPrompt,
  userPromptIndexOf,
  localRewindPoints,
  IDLE_SNAPSHOT,
  isDeveloperMockBackend,
  type AskUserPayload,
  type ChatMessage,
  type SessionState,
  type GeneratedImagePayload,
  type InteractionSnapshotV1,
  type PermissionPayload,
  type SessionSnapshot,
  type SessionTokenUsage,
  type StreamPayload,
  type TurnErrorPayload,
} from "@/lib/session";
import {
  isComposerSettingAction,
  rollbackOptimisticSetting,
  shouldLockComposerSettings,
} from "@/lib/composerSettings";
import {
  INITIAL_CONTEXT_USAGE,
  reduceContextUsage,
  resolveContextUsageDisplay,
  type ContextUsageState,
} from "@/lib/contextUsage";
import * as api from "@/lib/api";
import {
  sessionMoveAvailable,
  sessionMoveDestinations,
} from "@/lib/sessionMove";
import { createT, resolveLocale, type Locale } from "@/i18n";
import {
  PERMISSION_POLICIES,
  isValidPrefsScope,
  type PermissionPolicyId,
} from "@/lib/grokCatalog";
import { useComposerCatalog } from "@/hooks/useComposerCatalog";
import { useAppDialog } from "@/hooks/useAppDialog";
import {
  formatPermissionSummary,
  mapPermissionButtons,
} from "@/lib/permissionOptions";
import { AskUserDock } from "@/components/lobe-chat/AskUserDock";
import { SkillRecorderSheet } from "@/components/SkillRecorderSheet";
import {
  parseSkillDraft,
  skillDraftContentHash,
  type SkillDraft,
  type SkillGenerationRequest,
} from "@/lib/skillDraft";
import { DoctorModal } from "@/components/DoctorModal";
import { filterSessionSearch } from "@/lib/sessionSearch";
import {
  sessionExportFilename,
  sessionToMarkdown,
} from "@/lib/sessionExport";
import { connPillForState } from "@/lib/connStatus";
import { shortcutsForPlatform } from "@/lib/shortcuts";
import {
  ensureNotifyPermission,
  showDesktopNotification,
} from "@/lib/desktopNotify";
import { GlassModal } from "@/components/GlassModal";
import {
  applyResolvedSessionMedia,
  buildAgentPrompt,
  collectSessionRelativeMediaRefs,
  mergeAttachments,
  mergeMessageAttachments,
  parseAttachmentsFromContent,
  type Attachment,
} from "@/lib/attachments";
import {
  applyConnectorAtMention,
  applySkillAtSlash,
  isDraftEmpty,
  hydrateDisplayContent,
  parseStoredContent,
  plainTextOf,
  serializeDisplayForJournal,
  serializeForAgent,
} from "@/lib/draftDoc";
import { CONNECTOR_CATALOG } from "@/lib/connectorCatalog";
import {
  shouldEnqueueSend,
  type QueuedSend,
} from "@/lib/sendQueue";
import {
  useSendQueue,
  type ExecuteSendFromQueue,
} from "@/hooks/useSendQueue";
import { useComposerRecovery } from "@/hooks/useComposerRecovery";
import { useHostedCommandJobs } from "@/hooks/useHostedCommandJobs";
import { useStatusModals } from "@/hooks/useStatusModals";
import { BackgroundApprovalBanner } from "@/components/BackgroundApprovalBanner";
import {
  COMPOSER_RECOVERY_DRAFT_KEY,
  memoryPackRefFromContext,
  type ComposerMemoryPackRefV1,
} from "@/lib/composerRecovery";
import {
  buildSlashCatalog,
  flattenFilteredCatalog,
  type SlashItem,
  type SkillInfo,
} from "@/lib/slashCatalog";
import type { MessageKey } from "@/i18n";
import { ImageViewerProvider } from "@/components/ImageViewer";
import { SunsetzLogo } from "@/components/SunsetzLogo";
import { SetupWizard, type SetupCliInfo } from "@/components/SetupWizard";
import { ComposerDock } from "@/components/ComposerDock";
import { pathsEqual, siblingWorktreePath } from "@/lib/gitWorktree";
import {
  buildComposerPlusEntries,
  uploadMatchesQuery,
  type ComposerPlusEntry,
} from "@/components/ComposerPlusPanel";
import { StatusModal } from "@/components/StatusModal";
import { AppDialogHost } from "@/components/AppDialogHost";
import { McpStatusModal } from "@/components/McpStatusModal";
import {
  IconSearch,
  IconAlertTriangle,
  IconImagine,
  IconNotes,
  IconAttach,
  IconFolder,
  IconClose,
  IconNewChat as IconSquarePen,
  IconArchive,
  IconPin,
  IconPinOff,
  IconRename,
  IconCopy,
  IconTrash,
  IconExternalLink,
  IconFork,
  IconRewind,
  IconShield,
  IconCheck,
} from "@/components/icons";
import { AutomationsPage } from "@/components/AutomationsPage";
import { PluginMarketplace } from "@/components/PluginMarketplace";
import { WorkbenchTopbar } from "@/components/WorkbenchTopbar";
import { WorkbenchShell } from "@/components/WorkbenchShell";
import {
  SidebarNavigator,
  type SidebarProjectItem,
  type SidebarSessionItem,
} from "@/components/SidebarNavigator";
import {
  ContextMenu,
  type ContextMenuAnchor,
  type ContextMenuItem,
} from "@/components/ContextMenu";
import {
  aiCreateSeedPrompt,
  computeNextRunAt,
  parseScheduledUserContent,
  type Automation,
} from "@/lib/automations";
import {
  extractAutomationPayload,
  looksLikeScheduleIntent,
  wrapAutomationSetupAgentText,
} from "@/lib/automationSetup";
import type {
  PlanArtifactStatusV1,
  PlanArtifactV1,
} from "@/lib/planArtifacts";
import { selectDisplayPlanArtifact } from "@/lib/planArtifacts";
import { planIsAwaitingReview } from "@/lib/planBody";
import {
  askUserFromInteraction,
  automationClaimIntake,
  canStartAutomationClaim,
  interactionMatches,
  isActiveInteraction,
  isReviewablePlanInteraction,
  livePlanInteraction,
  mergeContentSearchSessions,
  normalizeSandboxProfile,
  pendingInteractionSessionIds,
  permissionFromInteraction,
  planFromArtifact,
  planResolutionContext,
  rememberSkillCandidateIds,
  skillCandidateAtBootstrap,
  unseenSkillCandidateForSession,
  updateActiveInteractions,
} from "@/lib/runtimeMigrationUi";
import type { ResourceOpenTarget } from "@/components/ResourceViewer";
import {
  mergeSessionChange,
  sessionChangesFromMessages,
  type SessionFileChange,
} from "@/lib/sessionChanges";
import { ConversationSurface } from "@/features/workbench/ConversationSurface";
import { Banner } from "@/shared/ui";
import {
  applyStreamChunks,
  createStreamCoalescer,
  DRAFT_SESSION_KEY,
  transcriptStore,
  useTranscriptMeta,
} from "@/entities/session";
import {
  installKeyboardFocusMode,
  preferPermissionFocus,
  trapTabKey,
} from "@/lib/a11yFocus";
import {
  SettingsPage,
  type SettingsSectionId,
} from "@/components/SettingsPage";
import {
  isAccountConnected,
  loadCachedSunsetzProBrand,
  resolveWelcomeBrandKind,
  saveCachedSunsetzProBrand,
  sunsetzProBrandKind,
} from "@/lib/accountUi";
import type { SunsetzProBrandKind } from "@/components/SunsetzProMark";
import {
  WindowControls,
  toggleMaximizeFromTitlebar,
} from "@/components/WindowControls";

interface Project {
  id: string;
  name: string;
  path: string;
  trusted: boolean;
  pathOk: boolean;
  pinned?: boolean;
  /** Project-level permission tier (L10). Null/undefined → app default. */
  permissionPolicy?: string | null;
}

interface SessionRow {
  id: string;
  title: string;
  projectId: string | null;
  updatedAt: string;
  archived?: boolean;
  /** Shell scheduled-automation run */
  scheduled?: boolean;
  contextUsage?: SessionTokenUsage | null;
}

type ContextMenuPosition = {
  x: number;
  y: number;
  anchorRect?: ContextMenuAnchor;
  restoreFocusTo?: HTMLElement | null;
};

type ContextMenuState =
  | ({ kind: "project"; id: string } & ContextMenuPosition)
  | ({ kind: "project-policy"; id: string } & ContextMenuPosition)
  | ({ kind: "session"; id: string } & ContextMenuPosition)
  | null;

const ResourceViewer = lazy(async () => {
  const module = await import("@/components/ResourceViewer");
  return { default: module.ResourceViewer };
});

interface PlanState {
  title: string;
  body: string;
  entries: unknown[];
  waiting: boolean;
  /** Pending exit_plan_mode JSON-RPC id */
  rpcId?: number | null;
  interactionId?: string | null;
  toolCallId?: string | null;
  artifactId?: string | null;
  artifactStatus?: PlanArtifactStatusV1 | null;
  currentRevision?: number | null;
  liveReview?: boolean;
  /**
   * Soft-hide the top PlanStatusBar without clearing progress.
   * Cleared when new plan events arrive or review gate opens.
   */
  barDismissed?: boolean;
}

function emptyPlanState(title: string): PlanState & { visible: boolean } {
  return {
    title,
    body: "",
    entries: [],
    waiting: true,
    visible: false,
    rpcId: null,
    interactionId: null,
    toolCallId: null,
    artifactId: null,
    artifactStatus: null,
    currentRevision: null,
    liveReview: false,
    barDismissed: false,
  };
}

export default function App() {
  const [theme, setTheme] = useState<Theme>(() => loadTheme(localStorage));
  const [layout, setLayout] = useState(() => loadLayout(localStorage));
  useEffect(() => installKeyboardFocusMode(), []);
  const [session, setSession] = useState<SessionSnapshot>(IDLE_SNAPSHOT);
  /** Host live agent (may differ from the session currently viewed in the UI). */
  const [liveHost, setLiveHost] = useState<SessionSnapshot>(IDLE_SNAPSHOT);
  const transcriptMeta = useTranscriptMeta();
  /** Context usage chip — known tokens from compact events + estimate fallback. */
  const [contextUsage, setContextUsage] = useState<ContextUsageState>(
    INITIAL_CONTEXT_USAGE,
  );
  /**
   * Files written/edited by agent tools per session (Changes / diff panel).
   * Live tool events may enrich entries with before/after snippets.
   */
  const [sessionChangesById, setSessionChangesById] = useState<
    Record<string, SessionFileChange[]>
  >({});
  /** Composer stored form (may include [[skill:name]] tokens). */
  const [draft, setDraft] = useState("");
  /** Composer identity changes before the slower session journal finishes loading. */
  const [composerRecoveryKey, setComposerRecoveryKey] = useState<string>(
    COMPOSER_RECOVERY_DRAFT_KEY,
  );
  const [goalMode, setGoalMode] = useState(false);
  /** Prevent overlapping executeSend / queue auto-flush races. */
  const sendInFlightRef = useRef(false);
  const executeSendFromQueueRef = useRef<ExecuteSendFromQueue>(
    async () => false,
  );
  const persistQueuedStateRef = useRef<
    (key: string, queue: QueuedSend[]) => Promise<boolean>
  >(async () => false);
  const persistQueuedState = useCallback(
    (key: string, queue: QueuedSend[]) =>
      persistQueuedStateRef.current(key, queue),
    [],
  );
  const composerRecoveryActionsRef = useRef<
    ReturnType<typeof useComposerRecovery> | null
  >(null);
  const [skillInfos, setSkillInfos] = useState<SkillInfo[]>([]);
  const [skillsLoading, setSkillsLoading] = useState(false);
  const [rankedSkillIds, setRankedSkillIds] = useState<Set<string>>(
    () => new Set(),
  );
  const [slashQuery, setSlashQuery] = useState<{
    start: number;
    query: string;
    end: number;
  } | null>(null);
  /**
   * Live slash token from contenteditable.innerText (rAF poll).
   * Independent of React draft so IME / <br> / missed onChange cannot desync.
   * `present` is true for bare `/` as well as `/query`.
   */
  const [liveSlash, setLiveSlash] = useState<{
    present: boolean;
    query: string;
    start: number;
    end: number;
  }>({ present: false, query: "", start: 0, end: 0 });
  const liveSlashRef = useRef(liveSlash);
  liveSlashRef.current = liveSlash;
  /** After Escape, suppress re-open until the `/token` text changes. */
  const slashDismissedSigRef = useRef<string | null>(null);
  const [liveAt, setLiveAt] = useState<{
    present: boolean;
    query: string;
    start: number;
    end: number;
  }>({ present: false, query: "", start: 0, end: 0 });
  const liveAtRef = useRef(liveAt);
  liveAtRef.current = liveAt;
  const atDismissedSigRef = useRef<string | null>(null);
  const [connectorStates, setConnectorStates] = useState<api.ConnectorStateV1[]>(
    [],
  );
  const connectorStatesRef = useRef(connectorStates);
  connectorStatesRef.current = connectorStates;
  const [slashActiveIndex, setSlashActiveIndex] = useState(0);
  const {
    showStatusModal,
    showMcpModal,
    mcpServers,
    mcpError,
    mcpLoading,
    openStatusModal,
    closeStatusModal,
    openMcpModal,
    closeMcpModal,
  } = useStatusModals();
  const [showCompactModal, setShowCompactModal] = useState(false);
  const [compactNote, setCompactNote] = useState("");
  const compactNoteRef = useRef<HTMLInputElement>(null);
  /** Rewind timeline picker (session menu / status). */
  const [rewindTimeline, setRewindTimeline] = useState<{
    sessionId: string;
    points: Array<{ promptIndex: number; messageId?: string | null; preview: string }>;
  } | null>(null);
  const [rewindBusy, setRewindBusy] = useState(false);
  /** Last user message open in inline edit (not main composer). */
  const [editingUserMessageId, setEditingUserMessageId] = useState<
    string | null
  >(null);
  /** Attachments for the open inline edit (reloaded from the message, editable). */
  const [editAttachments, setEditAttachments] = useState<Attachment[]>([]);
  const editingUserMessageIdRef = useRef<string | null>(null);
  editingUserMessageIdRef.current = editingUserMessageId;
  const [editSubmitting, setEditSubmitting] = useState(false);
  const [projects, setProjects] = useState<Project[]>([]);
  const [sessions, setSessions] = useState<SessionRow[]>([]);
  const [activeProject, setActiveProject] = useState<Project | null>(null);
  const [projectInstruction, setProjectInstruction] =
    useState<api.ProjectInstructionInspectV1 | null>(null);
  const viewingSessionIdRef = useRef<string | null>(null);
  const liveHostRef = useRef<SessionSnapshot>(IDLE_SNAPSHOT);
  const hostStateBySessionRef = useRef<Map<string, SessionState>>(new Map());
  const messagesRef = useRef<ChatMessage[]>(transcriptStore.getViewed());
  const [expandedProjects, setExpandedProjects] = useState<Record<string, boolean>>({});
  const [projectsOpen, setProjectsOpen] = useState(true);
  const [historyOpen, setHistoryOpen] = useState(true);
  const [ctxMenu, setCtxMenu] = useState<ContextMenuState>(null);
  const {
    appDialog,
    setAppDialog,
    dialogInput,
    setDialogInput,
    dialogPath,
    setDialogPath,
    dialogInputRef,
    confirmBtnRef,
    appDialogRef,
  } = useAppDialog();
  const [showSearch, setShowSearch] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const [contentSearchHits, setContentSearchHits] = useState<
    api.SessionSearchResultV1[]
  >([]);
  const [showComposerPlus, setShowComposerPlus] = useState(false);
  const [finderSelectionFeedback, setFinderSelectionFeedback] = useState<
    string | null
  >(null);
  /** Incremented when the + menu delegates to the project picker in the rail. */
  const [projectMenuOpenKey, setProjectMenuOpenKey] = useState(0);
  /** Review sheet for creating a reusable skill from visible conversation data. */
  const [skillRecorderOpen, setSkillRecorderOpen] = useState(false);
  const [skillCandidate, setSkillCandidate] =
    useState<api.SkillCandidateV1 | null>(null);
  /** Explicitly reviewed Memory pack for one immediate, non-queued turn. */
  const [pendingMemoryContext, setPendingMemoryContext] =
    useState<api.MemoryContextPackV1 | null>(null);
  const memoryPackRestoreGenRef = useRef(0);
  const seenSkillCandidateIdsRef = useRef<Set<string>>(new Set());
  const skillCandidatesReadyRef = useRef(false);
  const pendingSkillGenerationRef = useRef<{
    startIndex: number;
    sessionId: string | null;
    resolve: (draft: SkillDraft) => void;
    reject: (error: Error) => void;
  } | null>(null);
  const composerPlusTriggerRef = useRef<HTMLButtonElement>(null);
  const composerPlusPanelRef = useRef<HTMLDivElement>(null);
  const composerInputRef = useRef<HTMLDivElement>(null);
  const sidebarToggleRef = useRef<HTMLButtonElement>(null);
  const asideToggleRef = useRef<HTMLButtonElement>(null);
  /** Actual input card (.composer) — command panel anchors here. */
  const composerShellRef = useRef<HTMLDivElement>(null);
  /** Floating composer shell — height drives chat bottom padding. */
  const composerWrapRef = useRef<HTMLDivElement>(null);
  const [composerFloatPad, setComposerFloatPad] = useState(168);
  /** Set by newChat; applied after chat pane + textarea mount. */
  const pendingComposerFocus = useRef(false);
  const [sessionDataMode, setSessionDataMode] = useState("independent");
  const [defaultOpenTarget, setDefaultOpenTarget] = useState("finder");
  const [showUserMenu, setShowUserMenu] = useState(false);
  /** Hash route: workbench | settings/:section | automations */
  const [appView, setAppView] = useState<"workbench" | "settings">("workbench");
  /** Inside workbench: chat thread vs scheduled tasks list. */
  const [mainPane, setMainPane] = useState<
    "chat" | "automations" | "plugins"
  >("chat");
  const [settingsSection, setSettingsSection] =
    useState<SettingsSectionId>("general");
  /** Prevent overlapping automation runs. */
  const automationRunLock = useRef(false);
  const [pendingAutomationClaim, setPendingAutomationClaim] =
    useState<api.AutomationClaimV1 | null>(null);
  const handledAutomationClaimIds = useRef<Set<string>>(new Set());
  /** Conversation is guiding the user to create a scheduled task. */
  const automationSetupDraftRef = useRef(false);
  const automationSetupSessionsRef = useRef<Set<string>>(new Set());
  const automationAppliedRef = useRef<Set<string>>(new Set());
  /** While openSession loads, do not let session.sessionId effect clobber viewing id. */
  const openingSessionIdRef = useRef<string | null>(null);

  // ContextMenu handles outside click + Escape for sidebar menus.

  // Compact context modal: focus note field on open; Escape dismisses.
  useEffect(() => {
    if (!showCompactModal) return;
    const t = window.setTimeout(() => {
      compactNoteRef.current?.focus();
    }, 0);
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setShowCompactModal(false);
        setCompactNote("");
      }
    };
    document.addEventListener("keydown", onKey);
    return () => {
      window.clearTimeout(t);
      document.removeEventListener("keydown", onKey);
    };
  }, [showCompactModal]);

  useEffect(() => {
    if (!showSearch) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setShowSearch(false);
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [showSearch]);

  // Global shortcuts: search, help, doctor, new chat, settings.
  // Handlers go through refs so we don't re-bind every render.
  const shortcutHandlersRef = useRef({
    newChat: () => {},
    openSettings: () => {},
  });
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.isComposing) return;
      const mod = e.metaKey || e.ctrlKey;
      if (!mod) return;
      const target = e.target as HTMLElement | null;
      const tag = target?.tagName?.toLowerCase();
      const typing =
        tag === "input" ||
        tag === "textarea" ||
        !!target?.isContentEditable;
      const key = e.key.toLowerCase();
      if (key === "k") {
        e.preventDefault();
        setShowSearch(true);
        return;
      }
      if (key === "/") {
        e.preventDefault();
        setShowShortcuts((v) => !v);
        return;
      }
      if (key === "," && !typing) {
        e.preventDefault();
        shortcutHandlersRef.current.openSettings();
        return;
      }
      if (key === "n" && !typing) {
        e.preventDefault();
        shortcutHandlersRef.current.newChat();
        return;
      }
      if (key === "d" && e.shiftKey) {
        e.preventDefault();
        setShowDoctor(true);
        return;
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, []);

  /** First-run gate: loading → setup wizard → ready (home). */
  const [appGate, setAppGate] = useState<"loading" | "setup" | "ready">(
    "loading",
  );
  // Ask once for notification permission after first ready.
  useEffect(() => {
    if (appGate !== "ready") return;
    void ensureNotifyPermission();
  }, [appGate]);
  const [setupCliSeed, setSetupCliSeed] = useState<SetupCliInfo | null>(null);
  const [showDoctor, setShowDoctor] = useState(false);
  const [showShortcuts, setShowShortcuts] = useState(false);
  const [savedAccounts, setSavedAccounts] = useState<api.SavedAccount[]>([]);
  const [activeAccountId, setActiveAccountId] = useState<string | null>(null);
  const [perm, setPerm] = useState<PermissionPayload | null>(null);
  const permBarRef = useRef<HTMLDivElement | null>(null);
  const [permPreviewExpanded, setPermPreviewExpanded] = useState(false);
  useEffect(() => {
    setPermPreviewExpanded(false);
  }, [perm?.interactionId, perm?.rpcId]);
  /** Safety net: force-clear the compacting flag if no terminal event arrives
   * (crashed/killed process) so the context ring never pulses forever. */
  const compactSafetyTimerRef = useRef<number | null>(null);
  const clearCompactSafetyTimer = useCallback(() => {
    if (compactSafetyTimerRef.current != null) {
      window.clearTimeout(compactSafetyTimerRef.current);
      compactSafetyTimerRef.current = null;
    }
  }, []);
  const [askUser, setAskUser] = useState<AskUserPayload | null>(null);
  /** Background tasks paused on permission, plan review, or Agent questions. */
  const [pendingAskSessionIds, setPendingAskSessionIds] = useState<Set<string>>(
    () => new Set(),
  );
  const activeInteractionsRef = useRef<Map<string, InteractionSnapshotV1>>(
    new Map(),
  );
  /** Polite SR announce for stream start/stop (not every token). */
  const [streamA11yNote, setStreamA11yNote] = useState("");
  const wasStreamingRef = useRef(false);
  const [plan, setPlan] = useState<PlanState & { visible: boolean }>(() =>
    emptyPlanState("Plan ready for review"),
  );
  const [locale, setLocale] = useState<Locale>("zh");
  const localeRef = useRef(locale);
  localeRef.current = locale;
  const tr = useMemo(() => createT(locale), [locale]);
  const trRef = useRef(tr);
  trRef.current = tr;
  const syncPendingInteractionSessions = useCallback(() => {
    setPendingAskSessionIds(
      pendingInteractionSessionIds(
        activeInteractionsRef.current,
        viewingSessionIdRef.current,
      ),
    );
  }, []);
  const applyInteractionSnapshot = useCallback(
    (interaction: InteractionSnapshotV1) => {
      const active = isActiveInteraction(interaction);
      const focused = interaction.sessionId === viewingSessionIdRef.current;
      activeInteractionsRef.current = updateActiveInteractions(
        activeInteractionsRef.current,
        interaction,
      );
      syncPendingInteractionSessions();
      if (!focused) return;

      if (!active) {
        if (interaction.payload.kind === "permission") {
          setPerm((current) =>
            interactionMatches(current?.interactionId, interaction) ? null : current,
          );
        } else if (interaction.payload.kind === "ask_user") {
          setAskUser((current) =>
            interactionMatches(current?.interactionId, interaction) ? null : current,
          );
        } else {
          setPlan((current) =>
            interactionMatches(current.interactionId, interaction)
              ? {
                  ...current,
                  rpcId: null,
                  liveReview: false,
                  waiting: false,
                }
              : current,
          );
        }
        return;
      }

      const payload = interaction.payload;
      if (payload.kind === "permission") {
        setPerm(permissionFromInteraction(interaction));
      } else if (payload.kind === "ask_user") {
        setAskUser(askUserFromInteraction(interaction));
      } else {
        const entries = Array.isArray(payload.entries) ? payload.entries : [];
        const body = (payload.body || "").trim();
        setPlan((current) => ({
          ...current,
          title: trRef.current("resources.plan"),
          body: body || current.body,
          entries: entries.length ? entries : current.entries,
          waiting: false,
          visible: true,
          rpcId: interaction.rpcId,
          interactionId: interaction.interactionId,
          toolCallId: interaction.toolCallId,
          liveReview: isReviewablePlanInteraction(interaction),
          barDismissed: false,
        }));
      }
    },
    [syncPendingInteractionSessions],
  );
  const applyPlanArtifact = useCallback((artifact: PlanArtifactV1) => {
    if (artifact.sessionId !== viewingSessionIdRef.current) return;
    const live = livePlanInteraction(
      activeInteractionsRef.current,
      artifact.sessionId,
    );
    if (
      live &&
      artifact.status !== "proposed" &&
      artifact.interactionId !== live.interactionId
    ) {
      return;
    }
    const projected = planFromArtifact(artifact);
    setPlan((current) => ({
      ...current,
      ...projected,
      title: trRef.current("resources.plan"),
      rpcId: live?.rpcId ?? null,
      interactionId: live?.interactionId ?? projected.interactionId,
      liveReview: !!live,
      waiting: live ? false : projected.waiting,
    }));
  }, []);
  const restorePlanArtifacts = useCallback(
    async (sessionId: string) => {
      try {
        const artifacts = await api.sessionPlanArtifactsListV1(sessionId);
        if (viewingSessionIdRef.current !== sessionId) return;
        const display = selectDisplayPlanArtifact(artifacts);
        if (display) applyPlanArtifact(display);
      } catch {
        // Live events can still restore the card.
      }
    },
    [applyPlanArtifact],
  );
  useEffect(() => {
    syncPendingInteractionSessions();
  }, [session.sessionId, syncPendingInteractionSessions]);
  /** Files/folders attached for next send (@path to agent). */
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  /** Chat file/url card → open in right resource pane. */
  const [resourceOpenTarget, setResourceOpenTarget] =
    useState<ResourceOpenTarget | null>(null);
  /** Bump to force ResourceViewer into Plan review mode (详情 / auto-open). */
  const [planFocusKey, setPlanFocusKey] = useState(0);
  /** Live drag-drop target for zone overlays (null = not dragging). */
  const [dragZone, setDragZone] = useState<"sidebar" | "main" | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  const [columnNotice, setColumnNotice] = useState<string | null>(null);
  const dragPathsRef = useRef<string[]>([]);
  const layoutRef = useRef(layout);
  layoutRef.current = layout;
  const [, setSetup] = useState({ cli: false, auth: false, project: false });
  const [localError, setLocalError] = useState<string | null>(null);
  /** Expand technical dump under the compact error banner. */
  const [errorDetailOpen, setErrorDetailOpen] = useState(false);
  const [cliInfo, setCliInfo] = useState<{
    found: boolean;
    path: string | null;
    version: string | null;
    source: string;
    cliAuthPresent: boolean;
  }>({ found: false, path: null, version: null, source: "", cliAuthPresent: false });
  const [manualCliPath, setManualCliPath] = useState("");
  const [runtimeBackend, setRuntimeBackend] = useState("sunsetz");
  const [acpServerAddr, setAcpServerAddr] = useState("");
  const [maxConcurrentAgents, setMaxConcurrentAgents] = useState(3);
  const [agentIdleMinutes, setAgentIdleMinutes] = useState(30);
  const [streamStallSeconds, setStreamStallSeconds] = useState(120);
  const [sandboxProfile, setSandboxProfile] =
    useState<api.SandboxProfileV1>("off");
  const [runScheduledTasksInBackground, setRunScheduledTasksInBackground] =
    useState(false);
  const [storeApiKeysInKeychain, setStoreApiKeysInKeychain] = useState(false);
  const [gitWorktrees, setGitWorktrees] = useState<api.GitWorktreeEntry[]>([]);
  /** null = unknown/loading; true = git work tree; false = not a git repo. */
  const [gitWorktreesAvailable, setGitWorktreesAvailable] = useState<
    boolean | null
  >(null);
  const [gitWorktreesLoading, setGitWorktreesLoading] = useState(false);
  const [gitWorktreesReason, setGitWorktreesReason] = useState<string | null>(
    null,
  );
  /** Host stream-stall prompt (I06); null when dismissed or not stalled. */
  const [streamStall, setStreamStall] = useState<{
    sessionId?: string;
    stallSeconds: number;
  } | null>(null);
  const [connecting, setConnecting] = useState(false);
  /** Live provider retry progress (session://retry); cleared on success/stop/error. */
  const [retryStatus, setRetryStatus] = useState<{
    attempt: number;
    maxRetries: number;
    reason: string;
  } | null>(null);
  /** Epoch ms when the current agent turn became busy (for elapsed UI). */
  const [turnStartedAt, setTurnStartedAt] = useState<number | null>(null);
  const [resizingAside, setResizingAside] = useState(false);
  const [account, setAccount] = useState<api.AccountStatus | null>(null);
  const [accountLoading, setAccountLoading] = useState(false);
  const [accountBusy, setAccountBusy] = useState(false);
  const [loginHint, setLoginHint] = useState<string | null>(null);
  const platform = useMemo(() => {
    const ua = navigator.userAgent.toLowerCase();
    if (ua.includes("mac")) return "mac" as const;
    if (ua.includes("win")) return "win" as const;
    return "other" as const;
  }, []);
  const [hostCapabilities, setHostCapabilities] =
    useState<api.HostCapabilities>({
      platform: "other",
      finderSelection: false,
      speechRecognition: false,
      skillDraftSave: false,
      version: 1,
      capabilities: {},
    });
  /** Self-drawn chrome when OS title bar is disabled (Windows release config). */
  const useCustomWindowChrome = platform === "win" || platform === "other";
  const [windowMaximized, setWindowMaximized] = useState(false);

  useEffect(() => {
    applyThemeToDocument(theme);
    void applyNativeWindowTheme(theme);
  }, [theme]);

  useEffect(() => {
    document.documentElement.classList.remove(
      "platform-mac",
      "platform-win",
      "platform-other",
    );
    if (platform === "mac") document.documentElement.classList.add("platform-mac");
    if (platform === "win") document.documentElement.classList.add("platform-win");
    if (platform === "other") document.documentElement.classList.add("platform-other");
  }, [platform]);

  useEffect(() => {
    if (appGate !== "ready" || !api.isTauri()) return;
    let cancelled = false;
    void api
      .hostCapabilities()
      .then((caps) => {
        if (!cancelled) setHostCapabilities(caps);
      })
      .catch(() => {
        // Capability discovery is additive. Keep unsupported controls hidden.
      });
    return () => {
      cancelled = true;
    };
  }, [appGate]);

  useEffect(() => {
    if (!useCustomWindowChrome || !api.isTauri()) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void (async () => {
      try {
        const { getCurrentWindow } = await import("@tauri-apps/api/window");
        const w = getCurrentWindow();
        const sync = async () => {
          try {
            setWindowMaximized(await w.isMaximized());
          } catch {
            /* ignore */
          }
        };
        await sync();
        unlisten = await w.onResized(() => {
          void sync();
        });
        if (cancelled) unlisten?.();
      } catch {
        /* ignore */
      }
    })();
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [useCustomWindowChrome]);

  const refreshLists = useCallback(async () => {
    if (!api.isTauri()) {
      // Browser/Vite-only preview: skip Host gate.
      setAppGate("ready");
      setSetupCliSeed({
        found: true,
        path: null,
        version: "browser",
        source: "browser",
        cliAuthPresent: false,
      });
      return;
    }
    try {
      const [p, s, settings, cli, modelsRes] = await Promise.all([
        api.projectsList(),
        api.sessionsList(),
        api.settingsGet(),
        api.probeCli(),
        api.modelsListAvailable().catch(() => null),
      ]);
      setProjects(
        (p as Project[]).map((x) => ({
          ...x,
          pinned: !!(x as Project).pinned,
        })),
      );
      setSessions(
        (
          s as Array<SessionRow & { archived?: boolean; scheduled?: boolean }>
        ).map((x) => ({
          id: x.id,
          title: x.title,
          projectId: x.projectId,
          updatedAt: x.updatedAt,
          archived: !!x.archived,
          scheduled: !!x.scheduled,
        })),
      );
      void api.trayRefresh();
      setLocale(resolveLocale(settings.locale));
      await applyBootstrap(modelsRes, settings);
      setSessionDataMode(settings.sessionDataMode || "independent");
      setDefaultOpenTarget(
        (settings as { defaultOpenTarget?: string }).defaultOpenTarget ||
          "finder",
      );
      setManualCliPath(settings.manualCliPath || cli.path || "");
      setRuntimeBackend(settings.runtimeBackend || "sunsetz");
      setAcpServerAddr(settings.acpServerAddr || "");
      setMaxConcurrentAgents(
        typeof settings.maxConcurrentAgents === "number" &&
          settings.maxConcurrentAgents >= 1
          ? Math.min(8, Math.round(settings.maxConcurrentAgents))
          : 3,
      );
      setAgentIdleMinutes(
        typeof settings.agentIdleMinutes === "number" &&
          settings.agentIdleMinutes >= 1
          ? Math.min(1440, Math.round(settings.agentIdleMinutes))
          : 30,
      );
      setStreamStallSeconds(
        typeof settings.streamStallSeconds === "number" &&
          settings.streamStallSeconds >= 15
          ? Math.min(900, Math.round(settings.streamStallSeconds))
          : 120,
      );
      setSandboxProfile(normalizeSandboxProfile(settings.sandboxProfile));
      setRunScheduledTasksInBackground(!!settings.runScheduledTasksInBackground);
      setStoreApiKeysInKeychain(!!settings.storeApiKeysInKeychain);
      setCliInfo({
        found: cli.found,
        path: cli.path,
        version: cli.version,
        source: cli.source || "",
        cliAuthPresent: !!cli.cliAuthPresent,
      });
      const masked = await api.secretsGetMasked();
      const authOk =
        !!cli.cliAuthPresent ||
        masked.hasOfficialKey ||
        masked.hasRelayKey;
      setSetup({
        cli: cli.found,
        auth: authOk,
        project: p.some((x) => (x as Project).trusted) || p.length > 0,
      });

      // ── Setup gate: wizard is optional account setup; Grok CLI is not required ──
      const cliSeed: SetupCliInfo = {
        found: cli.found,
        path: cli.path,
        version: cli.version,
        source: cli.source || "",
        cliAuthPresent: !!cli.cliAuthPresent,
      };
      setSetupCliSeed(cliSeed);

      const wizardCompleted = !!settings.setupWizardCompleted;
      const legacyDone =
        !!settings.onboardingDone || !!settings.setupSkipped;

      if (wizardCompleted || legacyDone) {
        if (!wizardCompleted && legacyDone) {
          try {
            await api.settingsPatchV1({
              setupWizardCompleted: true,
              authSetupDeferred: !!settings.setupSkipped && !authOk,
              runtimeBackend: settings.runtimeBackend || "sunsetz",
            });
          } catch {
            /* ignore */
          }
        }
        setAppGate("ready");
      } else {
        setAppGate("setup");
      }

      // Prefer first trusted project; keep selection if still present
      setActiveProject((prev) => {
        if (prev && (p as Project[]).some((x) => x.id === prev.id)) {
          return (p as Project[]).find((x) => x.id === prev.id) || prev;
        }
        return (
          (p as Project[]).find((x) => x.trusted) ||
          (p as Project[])[0] ||
          null
        );
      });
      setExpandedProjects((prev) => {
        const next = { ...prev };
        for (const proj of p as Project[]) {
          if (next[proj.id] === undefined) next[proj.id] = true;
        }
        return next;
      });
    } catch (e) {
      setLocalError(String(e));
      // Still surface setup if Tauri partially works
      setSetupCliSeed((prev) =>
        prev ?? {
          found: false,
          path: null,
          version: null,
          source: "error",
          cliAuthPresent: false,
        },
      );
      setAppGate((g) => (g === "loading" ? "setup" : g));
    }
  }, []);

  // Bootstrap lists once
  useEffect(() => {
    void refreshLists();
  }, [refreshLists]);

  // Keep refs aligned for event handlers — but not while openSession is loading
  // (otherwise an intermediate null sessionId wipes viewing id and skips UI update).
  useEffect(() => {
    if (openingSessionIdRef.current) return;
    viewingSessionIdRef.current = session.sessionId;
  }, [session.sessionId]);

  useEffect(() => {
    liveHostRef.current = liveHost;
  }, [liveHost]);

  useEffect(() => {
    messagesRef.current = transcriptStore.getViewed();
    return transcriptStore.subscribeViewed(() => {
      messagesRef.current = transcriptStore.getViewed();
    });
  }, []);

  /** Apply a message reducer to the viewed session or only to the cache. */
  const patchSessionMessages = useCallback(
    (
      targetSessionId: string | undefined | null,
      reduce: (prev: ChatMessage[]) => ChatMessage[],
    ) => {
      transcriptStore.patch(targetSessionId ?? DRAFT_SESSION_KEY, reduce);
    },
    [],
  );

  /**
   * After any turn, if the last assistant message contains a grok-automation
   * fence, strip it from the bubble and call automation_create.
   * Applies to all sessions (not only “用 AI 创建”), so normal chat can schedule.
   * Deduped per assistant message id.
   */
  const tryApplyAutomationFromSession = useCallback(
    async (sessionId: string) => {
      if (!sessionId) return;

      const msgs = transcriptStore.getCached(sessionId) ?? [];
      let lastAssistantIdx = -1;
      for (let i = msgs.length - 1; i >= 0; i--) {
        if (msgs[i]?.role === "assistant" && !msgs[i]?.isError) {
          lastAssistantIdx = i;
          break;
        }
      }
      if (lastAssistantIdx < 0) return;
      const assistant = msgs[lastAssistantIdx]!;
      if (assistant.streaming) return;

      const applyKey = assistant.id || `${sessionId}:last`;
      if (automationAppliedRef.current.has(applyKey)) return;

      const { cleanText, input, rawJson } = extractAutomationPayload(
        assistant.content || "",
      );
      // Always strip fence from UI when present (even if JSON incomplete).
      if (cleanText !== (assistant.content || "")) {
        const aid = assistant.id;
        patchSessionMessages(sessionId, (prev) =>
          prev.map((m) => (m.id === aid ? { ...m, content: cleanText } : m)),
        );
      }
      if (!input) return;

      // Also dedupe identical payloads in this session.
      const payloadKey = `${sessionId}:${rawJson ?? input.title}`;
      if (automationAppliedRef.current.has(payloadKey)) return;

      automationAppliedRef.current.add(applyKey);
      automationAppliedRef.current.add(payloadKey);
      try {
        const created = await api.automationCreate(input);
        automationSetupSessionsRef.current.delete(sessionId);
        setToast(
          tr("automations.createdToast", {
            title: created.title || input.title,
          }),
        );
        window.setTimeout(() => setToast(null), 4200);
      } catch {
        automationAppliedRef.current.delete(applyKey);
        automationAppliedRef.current.delete(payloadKey);
        setToast(tr("automations.createFailed"));
        window.setTimeout(() => setToast(null), 4200);
      }
    },
    [patchSessionMessages, tr],
  );

  // Event listeners: StrictMode-safe (cleanup cancels pending + live unsubs)
  useEffect(() => {
    if (!api.isTauri()) return;
    let cancelled = false;
    let recoveryActivated = false;
    const cleanups: Array<() => void> = [];
    const streamCoalescer = createStreamCoalescer((sessionId, chunks) => {
      if (cancelled) return;
      const sid = sessionId === DRAFT_SESSION_KEY ? null : sessionId;
      patchSessionMessages(sid, (prev) => applyStreamChunks(prev, chunks));
      const last = chunks[chunks.length - 1];
      if (last?.done && last.sessionId) {
        void tryApplyAutomationFromSession(last.sessionId);
      }
    });
    cleanups.push(() => streamCoalescer.dispose());

    const track = async (p: Promise<() => void>) => {
      const un = await p;
      if (cancelled) {
        un();
      } else {
        cleanups.push(un);
      }
    };

    void (async () => {
      try {
        const snap = await api.sessionGetState();
        if (!cancelled) {
          const recovery = composerRecoveryActionsRef.current;
          if (recovery) {
            recoveryActivated = true;
            void recovery.activate(
              snap.sessionId ?? COMPOSER_RECOVERY_DRAFT_KEY,
            );
          }
          setLiveHost(snap);
          liveHostRef.current = snap;
          // Only bind the viewed session when Host already has a live row.
          if (snap.sessionId) {
            hostStateBySessionRef.current.set(snap.sessionId, snap.state);
            setSession(snap);
            viewingSessionIdRef.current = snap.sessionId;
            transcriptStore.setViewing(snap.sessionId);
            transcriptStore.markHostState(snap.sessionId, snap.state);
          }
        }

        await track(
          api.listen<SessionSnapshot>("session://state", (s) => {
            if (cancelled) return;
            if (s.sessionId) {
              hostStateBySessionRef.current.set(s.sessionId, s.state);
              transcriptStore.markHostState(s.sessionId, s.state);
            }
            setLiveHost(s);
            liveHostRef.current = s;
            // Only update the workbench session when the user is viewing it.
            // Otherwise switching sessions would yank selection back to the live agent.
            if (
              s.sessionId &&
              s.sessionId === viewingSessionIdRef.current
            ) {
              setSession(s);
              // Clear retry chip / turn timer / stall banner when turn ends or errors out
              if (s.state !== "streaming" && s.state !== "awaiting_permission") {
                setRetryStatus(null);
                setStreamStall(null);
                setTurnStartedAt(null);
                // Ensure no assistant is left with streaming=true after the turn
                // (missed done chunk) — otherwise the next send can bind to it.
                patchSessionMessages(s.sessionId, (prev) => {
                  if (!prev.some((m) => m.streaming)) return prev;
                  return prev.map((m) =>
                    m.streaming ? { ...m, streaming: false } : m,
                  );
                });
                if (s.state === "ready") {
                  showDesktopNotification({
                    title: trRef.current("notify.turnDoneTitle"),
                    body: trRef.current("notify.turnDoneBody"),
                    tag: `turn-${s.sessionId || "x"}`,
                  });
                }
              } else if (
                (s.state === "streaming" || s.state === "awaiting_permission") &&
                s.sessionId === viewingSessionIdRef.current
              ) {
                setTurnStartedAt((prev) => prev ?? Date.now());
              }
              // After a turn, resolve `images/N.jpg` short paths into image cards
              if (s.state === "ready") {
                const sid = s.sessionId;
                patchSessionMessages(sid, (prev) => {
                  const rels = collectSessionRelativeMediaRefs(prev);
                  if (!rels.length) return prev;
                  void api
                    .sessionResolveRelativeMedia(sid, rels)
                    .then((list) => {
                      if (
                        cancelled ||
                        !list.length ||
                        viewingSessionIdRef.current !== sid
                      ) {
                        return;
                      }
                      const resolved = list.map((a) => ({
                        path: a.path,
                        name:
                          a.name ||
                          a.path.split(/[/\\]/).pop() ||
                          a.path,
                        isDir: !!a.isDir,
                      }));
                      patchSessionMessages(sid, (cur) =>
                        applyResolvedSessionMedia(cur, resolved),
                      );
                    })
                    .catch(() => {
                      /* ignore */
                    });
                  return prev;
                });
              }
            } else if (!isSessionBusy(s.state)) {
              if (viewingSessionIdRef.current === s.sessionId) {
                setRetryStatus(null);
              }
              // Backup apply path if stream `done` chunk was missed.
              if (s.sessionId) {
                void tryApplyAutomationFromSession(s.sessionId);
              }
            }
          }),
        );
        await track(
          api.listen<StreamPayload>("session://stream", (chunk) => {
            if (cancelled) return;
            // Ignore empty terminal ticks that only flip done
            if (!chunk.text && !chunk.done) return;
            // Defense-in-depth: drop stream chunks that arrive while that
            // session is not producing a turn. Gate on the chunk's session,
            // not the globally focused host — background Sunsetz turns keep
            // streaming after the user switches chats.
            const chunkState = chunk.sessionId
              ? hostStateBySessionRef.current.get(chunk.sessionId)
              : undefined;
            if (
              chunk.text &&
              !isSessionLiveStreaming(
                chunkState ?? liveHostRef.current.state,
              )
            ) {
              return;
            }
            if (
              chunk.text &&
              chunk.sessionId === viewingSessionIdRef.current
            ) {
              setRetryStatus(null);
              // Progress clears stall banner (I06).
              setStreamStall(null);
            }
            streamCoalescer.enqueue(chunk);
          }),
        );
        await track(
          api.listen<GeneratedImagePayload>(
            "session://generated_image",
            (p) => {
              if (cancelled || !p?.path) return;
              patchSessionMessages(p.sessionId, (prev) =>
                applyGeneratedImage(prev, p),
              );
            },
          ),
        );
        await track(
          api.listen<{
            sessionId?: string;
            messageId?: string;
            trigger?: string;
            tokensBefore?: number;
            tokensAfter?: number;
            summaryPreview?: string;
            note?: string;
            content?: string;
          }>("session://context_compact", (p) => {
            if (cancelled || !p) return;
            const sid = p.sessionId;
            if (!sid) return;
            patchSessionMessages(sid, (prev) => applyContextCompact(prev, p));
            if (sid === viewingSessionIdRef.current) {
              clearCompactSafetyTimer();
              setContextUsage((prev) =>
                reduceContextUsage(prev, {
                  type: "compact",
                  trigger: p.trigger,
                  tokensBefore: p.tokensBefore,
                  tokensAfter: p.tokensAfter,
                  summaryPreview: p.summaryPreview,
                  note: p.note,
                  messageId: p.messageId,
                }),
              );
              const auto = (p.trigger || "auto").toLowerCase() !== "manual";
              setToast(
                auto
                  ? tr("compact.toastAuto")
                  : tr("compact.toastManual"),
              );
              window.setTimeout(() => setToast(null), 3200);
            }
          }),
        );
        await track(
          api.listen<{ sessionId?: string; trigger?: string }>(
            "session://context_compact_start",
            (p) => {
              if (cancelled || !p?.sessionId) return;
              if (p.sessionId !== viewingSessionIdRef.current) return;
              clearCompactSafetyTimer();
              setContextUsage((prev) =>
                reduceContextUsage(prev, { type: "compact_start" }),
              );
              compactSafetyTimerRef.current = window.setTimeout(() => {
                compactSafetyTimerRef.current = null;
                setContextUsage((prev) =>
                  reduceContextUsage(prev, { type: "compact_end" }),
                );
              }, 20_000);
            },
          ),
        );
        await track(
          api.listen<{ sessionId?: string; trigger?: string; outcome?: string }>(
            "session://context_compact_end",
            (p) => {
              if (cancelled || !p?.sessionId) return;
              if (p.sessionId !== viewingSessionIdRef.current) return;
              clearCompactSafetyTimer();
              setContextUsage((prev) =>
                reduceContextUsage(prev, { type: "compact_end" }),
              );
            },
          ),
        );
        await track(
          api.listen<{
            sessionId?: string;
            usage?: SessionTokenUsage;
          }>("session://context_usage", (p) => {
            if (cancelled || !p?.sessionId || !p.usage) return;
            setSessions((list) =>
              list.map((row) =>
                row.id === p.sessionId
                  ? { ...row, contextUsage: p.usage }
                  : row,
              ),
            );
            if (p.sessionId === viewingSessionIdRef.current) {
              setSession((prev) => ({
                ...prev,
                contextUsage: p.usage,
              }));
            }
          }),
        );
        await track(
          api.listen<{
            sessionId?: string;
            toolCallId?: string;
            title?: string;
            kind?: string;
            status?: string;
            path?: string | null;
            detail?: string | null;
            before?: string | null;
            after?: string | null;
          }>("session://tool", (p) => {
            if (cancelled || !p?.toolCallId) return;
            const sid = p.sessionId || viewingSessionIdRef.current;
            if (!sid) return;
            patchSessionMessages(sid, (prev) => applyToolEvent(prev, p));
            // Track write/edit tools for the session Changes panel.
            setSessionChangesById((prev) => {
              const list = prev[sid] ?? [];
              const next = mergeSessionChange(list, {
                toolCallId: p.toolCallId,
                title: p.title,
                kind: p.kind,
                status: p.status,
                path: p.path,
                detail: p.detail,
                before: p.before,
                after: p.after,
              });
              if (next === list) return prev;
              return { ...prev, [sid]: next };
            });
            if (sid === viewingSessionIdRef.current) {
              setTurnStartedAt((t) => t ?? Date.now());
              // Tool activity counts as progress — clear stall banner (I06).
              setStreamStall(null);
            }
          }),
        );
        await track(
          api.listen<{
            sessionId?: string;
            messageId?: string;
            marker?: string;
            reason?: string;
            content?: string;
          }>("session://turn_marker", (p) => {
            if (cancelled || !p) return;
            const sid = p.sessionId;
            if (!sid) return;
            patchSessionMessages(sid, (prev) => applyTurnMarker(prev, p));
            if (sid === viewingSessionIdRef.current) {
              setTurnStartedAt(null);
              setStreamStall(null);
              if (p.marker === "turn_cancelled") {
                setToast(tr("activity.cancelledToast"));
                window.setTimeout(() => setToast(null), 2800);
              }
            }
          }),
        );
        await track(
          api.listen<{ sessionId?: string; reason?: string }>(
            "session://idle_recycled",
            (p) => {
              if (cancelled || !p) return;
              if (p.reason === "capacity") {
                setToast(tr("agent.processLimitToast"));
                window.setTimeout(() => setToast(null), 5200);
                return;
              }
              // Toast when the focused (or unknown) session was idle-recycled.
              if (
                !p.sessionId ||
                p.sessionId === viewingSessionIdRef.current
              ) {
                setToast(tr("agent.idleRecycledToast"));
                window.setTimeout(() => setToast(null), 4200);
              }
            },
          ),
        );
        await track(
          api.listen<{
            sessionId?: string;
            stopReason?: string;
            toolCount?: number;
          }>("session://turn_empty_run", (p) => {
            if (cancelled || !p) return;
            if (
              p.sessionId &&
              p.sessionId !== viewingSessionIdRef.current
            ) {
              return;
            }
            setToast(tr("session.emptyRunToast"));
            window.setTimeout(() => setToast(null), 7200);
          }),
        );
        await track(
          api.listen<{
            sessionId?: string;
            code?: string;
            message?: string;
            maxConcurrentAgents?: number;
          }>("session://process_limit", (p) => {
            if (cancelled || !p) return;
            setToast(tr("agent.processLimitToast"));
            window.setTimeout(() => setToast(null), 5200);
            if (
              !p.sessionId ||
              p.sessionId === viewingSessionIdRef.current
            ) {
              setLocalError(
                p.message
                  ? `PROCESS_LIMIT: ${p.message}`
                  : "PROCESS_LIMIT",
              );
            }
          }),
        );
        await track(
          api.listen<{
            sessionId?: string;
            stallSeconds?: number;
            code?: string;
            message?: string;
          }>("session://stream_stall", (p) => {
            if (cancelled || !p) return;
            // Only prompt for the viewed session (or unknown id).
            if (
              p.sessionId &&
              p.sessionId !== viewingSessionIdRef.current
            ) {
              return;
            }
            const secs =
              typeof p.stallSeconds === "number" && p.stallSeconds > 0
                ? Math.round(p.stallSeconds)
                : 120;
            setStreamStall({
              sessionId: p.sessionId,
              stallSeconds: secs,
            });
          }),
        );
        await track(
          api.listen<{
            attempt?: number;
            maxRetries?: number;
            reason?: string;
            aborting?: boolean;
            sessionId?: string;
          }>("session://retry", (p) => {
            if (cancelled) return;
            // Retry chip is only meaningful on the viewed live session.
            if (
              p.sessionId &&
              p.sessionId !== viewingSessionIdRef.current
            ) {
              return;
            }
            if (
              liveHostRef.current.sessionId &&
              liveHostRef.current.sessionId !== viewingSessionIdRef.current
            ) {
              return;
            }
            const attempt = p.attempt ?? 0;
            const maxRetries = p.maxRetries ?? 5;
            const reason = (p.reason || "").trim();
            setRetryStatus({ attempt, maxRetries, reason });
          }),
        );
        await track(
          api.listen<TurnErrorPayload>("session://turn_error", (p) => {
            if (cancelled) return;
            if (p.sessionId === viewingSessionIdRef.current) {
              setRetryStatus(null);
            }
            patchSessionMessages(p.sessionId, (prev) =>
              applyTurnError(prev, p, localeRef.current),
            );
          }),
        );
        await track(
          api.listen<InteractionSnapshotV1>("session://interaction", (payload) => {
            if (cancelled) return;
            applyInteractionSnapshot(payload);
          }),
        );
        await track(
          api.listen<PermissionPayload>("session://permission", (p) => {
            if (cancelled) return;
            // Only surface the bar when viewing the session that needs it.
            if (
              p.sessionId &&
              p.sessionId !== viewingSessionIdRef.current
            ) {
              // Multi-session stream: another chat needs approval — nudge user.
              setToast(trRef.current("session.backgroundPermission"));
              window.setTimeout(() => setToast(null), 4200);
              showDesktopNotification({
                title: trRef.current("notify.permissionTitle"),
                body: trRef.current("session.backgroundPermission"),
                tag: `perm-bg-${p.rpcId}`,
                force: true,
              });
              return;
            }
            setPerm(p);
            showDesktopNotification({
              title: trRef.current("notify.permissionTitle"),
              body: trRef.current("notify.permissionBody"),
              tag: `perm-${p.rpcId}`,
              force: true,
            });
          }),
        );
        await track(
          api.listen<AskUserPayload>("session://ask_user", (p) => {
            if (cancelled) return;
            if (
              p.sessionId &&
              p.sessionId !== viewingSessionIdRef.current
            ) {
              setPendingAskSessionIds((current) => {
                const next = new Set(current);
                next.add(p.sessionId);
                return next;
              });
              return;
            }
            if (!p?.rpcId || !Array.isArray(p.questions) || !p.questions.length) {
              return;
            }
            setPendingAskSessionIds((current) => {
              const next = new Set(current);
              next.delete(p.sessionId);
              return next;
            });
            setAskUser(p);
          }),
        );
        await track(
          api.listen<{ sessionId?: string }>(
            "session://background_ask_user",
            (p) => {
              if (cancelled || !p.sessionId) return;
              setPendingAskSessionIds((current) => {
                const next = new Set(current);
                next.add(p.sessionId!);
                return next;
              });
            },
          ),
        );
        await track(
          api.listen<{
            entries?: unknown[];
            body?: string | null;
            sessionId?: string;
            rpcId?: number | null;
            toolCallId?: string | null;
            interactionId?: string | null;
            waiting?: boolean;
          }>("session://plan", (p) => {
            if (cancelled) return;
            if (
              p.sessionId &&
              p.sessionId !== viewingSessionIdRef.current
            ) {
              return;
            }
            const body = (p.body || "").trim();
            const entries = Array.isArray(p.entries) ? p.entries : [];
            // Prefer markdown planContent; fall back to readable entries list
            let displayBody = body;
            if (!displayBody && entries.length) {
              displayBody = entries
                .map((e, i) => {
                  if (e && typeof e === "object") {
                    const o = e as Record<string, unknown>;
                    const content = String(o.content ?? o.title ?? o.text ?? "");
                    const st = o.status ? ` [${o.status}]` : "";
                    const pr = o.priority ? ` (${o.priority})` : "";
                    return `${i + 1}. ${content}${pr}${st}`;
                  }
                  return `${i + 1}. ${String(e)}`;
                })
                .join("\n");
            }
            // Preserve exit_plan_mode rpcId across later sessionUpdate plan
            // notifications (those arrive with rpcId=null and would otherwise
            // disable Approve / Request changes — see #17).
            setPlan((prev) => {
              const rpcId =
                p.rpcId != null
                  ? p.rpcId
                  : prev.visible
                    ? (prev.rpcId ?? null)
                    : null;
              return {
                title: tr("resources.plan"),
                body: displayBody || (prev.visible ? prev.body : ""),
                entries: entries.length
                  ? entries
                  : prev.visible
                    ? prev.entries
                    : [],
                waiting: rpcId == null,
                visible: true,
                rpcId,
                interactionId:
                  p.interactionId != null
                    ? p.interactionId
                    : (prev.interactionId ?? null),
                toolCallId:
                  p.toolCallId != null
                    ? p.toolCallId
                    : prev.visible
                      ? (prev.toolCallId ?? null)
                      : null,
                liveReview: p.rpcId != null ? true : prev.liveReview,
                // New plan activity always resurfaces the top progress bar.
                barDismissed: false,
              };
            });
          }),
        );
        await track(
          api.listen<PlanArtifactV1>("session://plan_artifact", (artifact) => {
            if (cancelled) return;
            applyPlanArtifact(artifact);
          }),
        );
        await track(
          api.listen<{ sessionId?: string; title?: string }>(
            "session://title",
            (p) => {
              if (cancelled || !p.sessionId || !p.title) return;
              setSessions((list) =>
                list.map((s) =>
                  s.id === p.sessionId ? { ...s, title: p.title! } : s,
                ),
              );
              setSession((prev) =>
                prev.sessionId === p.sessionId
                  ? { ...prev, title: p.title! }
                  : prev,
              );
              setLiveHost((prev) =>
                prev.sessionId === p.sessionId
                  ? { ...prev, title: p.title! }
                  : prev,
              );
            },
          ),
        );

        // Restore every foreground/background interaction after a WebView reload.
        // Live events remain the source of truth after this bootstrap.
        const pendingInteractions = await api.sessionInteractionsList();
        if (!cancelled) {
          pendingInteractions.forEach(applyInteractionSnapshot);
        }
        const focusedSessionId = viewingSessionIdRef.current;
        if (!cancelled && focusedSessionId) {
          await restorePlanArtifacts(focusedSessionId);
        }
      } catch (e) {
        if (!cancelled) {
          if (!recoveryActivated) {
            void composerRecoveryActionsRef.current?.activate(
              COMPOSER_RECOVERY_DRAFT_KEY,
            );
          }
          setLocalError(String(e));
        }
      }
    })();

    return () => {
      cancelled = true;
      cleanups.forEach((u) => u());
    };
  }, [
    applyInteractionSnapshot,
    applyPlanArtifact,
    restorePlanArtifacts,
    patchSessionMessages,
    tryApplyAutomationFromSession,
  ]);

  const toggleThemeBtn = () => {
    setTheme((t) => {
      const n = toggleTheme(t);
      saveTheme(localStorage, n);
      applyThemeToDocument(n);
      void applyNativeWindowTheme(n);
      return n;
    });
  };

  const applyThemeChoice = (next: Theme) => {
    saveTheme(localStorage, next);
    applyThemeToDocument(next);
    void applyNativeWindowTheme(next);
    setTheme(next);
  };

  const navigateWorkbench = useCallback(() => {
    setAppView("workbench");
    setMainPane("chat");
    if (typeof window !== "undefined" && window.location.hash) {
      window.history.replaceState(null, "", window.location.pathname + window.location.search);
    }
  }, []);

  const navigateAutomations = useCallback(() => {
    setAppView("workbench");
    setMainPane("automations");
    setShowUserMenu(false);
    if (typeof window !== "undefined") {
      window.location.hash = "#/automations";
    }
  }, []);

  const navigatePlugins = useCallback(() => {
    setAppView("workbench");
    setMainPane("plugins");
    setShowUserMenu(false);
    if (typeof window !== "undefined") {
      window.location.hash = "#/plugins";
    }
  }, []);

  const navigateSettings = useCallback((section: SettingsSectionId = "general") => {
    setSettingsSection(section);
    setAppView("settings");
    setShowUserMenu(false);
    if (typeof window !== "undefined") {
      window.location.hash = `#/settings/${section}`;
    }
  }, []);

  // Hash route: #/settings[/section] | #/automations | #/workbench
  useEffect(() => {
    const syncFromHash = () => {
      const raw = (window.location.hash || "").replace(/^#\/?/, "");
      if (raw.startsWith("settings")) {
        const part = raw.split("/")[1] as SettingsSectionId | undefined;
        const allowed: SettingsSectionId[] = [
          "general",
          "appearance",
          "account",
          "models",
          "archived",
          "extensions",
          "runtime",
          "about",
        ];
        setSettingsSection(
          part && allowed.includes(part) ? part : "general",
        );
        setAppView("settings");
      } else if (raw === "automations" || raw.startsWith("automations")) {
        setAppView("workbench");
        setMainPane("automations");
      } else if (raw === "plugins" || raw.startsWith("plugins")) {
        setAppView("workbench");
        setMainPane("plugins");
      } else if (raw === "" || raw === "workbench" || raw === "home") {
        setAppView("workbench");
        setMainPane("chat");
      }
    };
    syncFromHash();
    window.addEventListener("hashchange", syncFromHash);
    return () => window.removeEventListener("hashchange", syncFromHash);
  }, []);

  /**
   * Open a stored session. Loads journal immediately; warms the ACP agent in
   * the background so the first send skips cold process spawn when possible.
   */
  /** Look up a session by id and switch to it — shared by the sidebar row
   * click and the background-approval banner so both use the same path. */
  const openSessionById = (sessionId: string) => {
    const row = sessions.find((item) => item.id === sessionId);
    if (!row) return;
    const project = row.projectId
      ? projects.find((item) => item.id === row.projectId)
      : undefined;
    void openSession(row, project);
  };

  const openSession = async (s: SessionRow, project?: Project | null) => {
    const proj =
      project ||
      projects.find((p) => p.id === s.projectId) ||
      null;
    setMainPane("chat");
    setAppView("workbench");

    // Snapshot the outgoing thread so a mid-turn switch does not lose the user bubble.
    const leavingId = viewingSessionIdRef.current;
    if (leavingId) {
      transcriptStore.write(leavingId, messagesRef.current);
    }

    // Composer state switches immediately; journal loading may take much longer.
    void composerRecoveryActionsRef.current?.activate(s.id);
    // Point viewing id immediately so late stream chunks land in the right cache.
    openingSessionIdRef.current = s.id;
    viewingSessionIdRef.current = s.id;
    clearCompactSafetyTimer();
    transcriptStore.setViewing(s.id);
    setEditingUserMessageId(null);
    setEditAttachments([]);
    setPlan(emptyPlanState(tr("resources.plan")));

    try {
      const stored = await api.sessionMessages(s.id);
      let mapped: ChatMessage[] = stored.map((m) => {
        const parsed = parseAttachmentsFromContent(m.content);
        const storedAtts: Attachment[] = (m.attachments ?? []).map((a) => ({
          path: a.path,
          name: a.name || a.path.split(/[/\\]/).pop() || a.path,
          isDir: !!a.isDir,
        }));
        // @path lines (user) + persisted image_gen cards + absolute paths in text
        const attachments = mergeMessageAttachments(
          mergeAttachments(parsed.attachments, storedAtts),
          m.content,
        );
        const rawContent =
          parsed.text || (parsed.attachments.length ? "" : m.content);
        // User turns: restore [[skill:]] chips from agent-form `/name` history.
        const content =
          m.role === "user" ? hydrateDisplayContent(rawContent) : rawContent;
        const rawMarker = (m as { marker?: string }).marker || undefined;
        const marker =
          rawMarker ||
          (m.role === "tool" && content.startsWith("context_compact")
            ? "context_compact"
            : m.role === "tool" && content.startsWith("tool_step|")
              ? "tool_step"
              : m.role === "tool" && content.startsWith("turn_cancelled")
                ? "turn_cancelled"
                : undefined);
        const compactMeta =
          marker === "context_compact"
            ? parseCompactContent(content) || undefined
            : undefined;
        const toolParsed =
          marker === "tool_step" ? parseToolStepContent(content) : null;
        const role = m.role as "user" | "assistant" | "tool";
        let displayContent = toolParsed?.title || content;
        // Never show silent automation fence to the user on reload.
        if (role === "assistant" && displayContent) {
          displayContent = extractAutomationPayload(displayContent).cleanText;
        }
        const thoughtPhases = splitThoughtPhases(m.thought);
        return {
          id: m.id,
          role,
          content: displayContent,
          thought: m.thought ?? undefined,
          thoughtPhases,
          // Reconstruct interleaved timeline for reload (first phase → body → rest).
          segments:
            role === "assistant"
              ? buildSegmentsFromLegacy(
                  displayContent,
                  m.thought,
                  thoughtPhases,
                )
              : undefined,
          isError: m.isError || undefined,
          attachments,
          createdAt: m.createdAt || undefined,
          marker,
          compactMeta: compactMeta ?? undefined,
          toolCallId: m.id.startsWith("tool-") ? m.id.slice(5) : undefined,
          toolKind: toolParsed?.kind,
          toolStatus: toolParsed?.status,
          toolDetail: toolParsed?.detail,
          toolPath: toolParsed?.path,
          streaming: false,
        };
      });
      // Short paths like `images/1.jpg` → agent session dir → image cards
      if (api.isTauri()) {
        const rels = collectSessionRelativeMediaRefs(mapped);
        if (rels.length) {
          try {
            const list = await api.sessionResolveRelativeMedia(s.id, rels);
            if (list.length) {
              mapped = applyResolvedSessionMedia(
                mapped,
                list.map((a) => ({
                  path: a.path,
                  name:
                    a.name || a.path.split(/[/\\]/).pop() || a.path,
                  isDir: !!a.isDir,
                })),
              );
            }
          } catch {
            /* ignore */
          }
        }
      }
      // Prefer in-memory cache (optimistic user msg + partial stream) over disk.
      const chosen = preferSessionMessages(
        transcriptStore.getCached(s.id),
        mapped,
      );
      if (viewingSessionIdRef.current !== s.id) {
        // User switched again while we were loading — keep cache warm, skip UI write.
        transcriptStore.write(s.id, chosen);
        if (openingSessionIdRef.current === s.id) {
          openingSessionIdRef.current = null;
        }
        return;
      }
      // Cache raw journal (may include fences) so apply can read them.
      transcriptStore.write(s.id, chosen);
      // Rebuild Changes list from tool_step history; preserve live before/after.
      {
        const fromHist = sessionChangesFromMessages(chosen);
        setSessionChangesById((prev) => {
          const existing = prev[s.id] ?? [];
          let list = fromHist;
          for (const e of existing) {
            if (e.before != null || e.after != null) {
              list = mergeSessionChange(list, {
                toolCallId: e.toolCallId,
                title: e.title,
                kind: e.toolKind,
                status: e.status,
                path: e.path,
                before: e.before,
                after: e.after,
                updatedAt: e.updatedAt,
              });
            }
          }
          return { ...prev, [s.id]: list };
        });
      }
      const stripped = chosen.map((m) => {
        if (m.role !== "assistant" || !m.content) return m;
        const { cleanText } = extractAutomationPayload(m.content);
        return cleanText === m.content ? m : { ...m, content: cleanText };
      });
      transcriptStore.setViewing(s.id, stripped);
      setContextUsage(
        reduceContextUsage(INITIAL_CONTEXT_USAGE, {
          type: "hydrate",
          messages: stripped,
        }),
      );
      // Backfill create if assistant still has a fence in journal (failed chat-create).
      void tryApplyAutomationFromSession(s.id);
      // Backfill scheduled flag from journal (older automation sessions).
      if (
        !s.scheduled &&
        chosen.some(
          (m) =>
            m.role === "user" && !!parseScheduledUserContent(m.content || ""),
        )
      ) {
        setSessions((list) =>
          list.map((row) =>
            row.id === s.id ? { ...row, scheduled: true } : row,
          ),
        );
        if (api.isTauri()) {
          void api.sessionSetScheduled(s.id, true).catch(() => {});
        }
      }
      // Refine isDir via classify when possible
      const allPaths = chosen.flatMap((m) => m.attachments?.map((a) => a.path) ?? []);
      if (allPaths.length && api.isTauri()) {
        void api.pathsClassify(allPaths).then((list) => {
          if (viewingSessionIdRef.current !== s.id) return;
          const byPath = new Map(list.map((c) => [c.path, c]));
          patchSessionMessages(s.id, (prev) =>
            prev.map((msg) => {
              if (!msg.attachments?.length) return msg;
              return {
                ...msg,
                attachments: msg.attachments.map((a) => {
                  const c = byPath.get(a.path);
                  return c
                    ? { path: c.path, name: c.name, isDir: c.isDir }
                    : a;
                }),
              };
            }),
          );
        });
      }
    } catch {
      if (viewingSessionIdRef.current !== s.id) {
        if (openingSessionIdRef.current === s.id) {
          openingSessionIdRef.current = null;
        }
        return;
      }
      const cached = transcriptStore.getCached(s.id);
      transcriptStore.setViewing(s.id, cached ?? []);
      setContextUsage(
        reduceContextUsage(INITIAL_CONTEXT_USAGE, {
          type: "hydrate",
          messages: cached ?? [],
        }),
      );
    }
    if (viewingSessionIdRef.current !== s.id) {
      if (openingSessionIdRef.current === s.id) {
        openingSessionIdRef.current = null;
      }
      return;
    }
    // Orphan sessions clear project context; project sessions select their folder.
    setActiveProject(proj);
    // Reattach live host snapshot when reopening the session that is still running.
    const live = liveHostRef.current;
    if (live.sessionId === s.id) {
      setSession({
        ...live,
        title: s.title || live.title || tr("session.untitled"),
      });
    } else {
      setSession({
        ...IDLE_SNAPSHOT,
        sessionId: s.id,
        title: s.title || tr("session.untitled"),
        state: "idle",
        backend: "sunsetz",
        contextUsage: s.contextUsage ?? null,
      });
    }
    if (openingSessionIdRef.current === s.id) {
      openingSessionIdRef.current = null;
    }
    setLocalError(null);
    // Permission / retry / ask-user chrome only apply to the live viewed session.
    if (live.sessionId !== s.id) {
      setPerm(null);
      setAskUser(null);
      setRetryStatus(null);
    }

    // Recover any interaction that belongs to this task, including one raised
    // while it was in the background or while the WebView was reloading.
    if (api.isTauri()) {
      void restorePlanArtifacts(s.id);
      void api
        .sessionInteractionsList(s.id)
        .then((interactions) => {
          if (viewingSessionIdRef.current !== s.id) return;
          interactions
            .filter(
              (interaction) =>
                interaction.status === "pending" ||
                interaction.status === "resolving",
            )
            .forEach(applyInteractionSnapshot);
        })
        .catch(() => {
          // Keep the transcript usable; a later live event can still restore it.
        });
    }

    // Warm ACP: connect while the user reads history (trusted project or orphan).
    // Host serializes connect; first send no-ops if already ready, or waits if
    // still handshaking. Process is reused across sessions when cwd/effort match.
    if (
      api.isTauri() &&
      (!proj || proj.trusted) &&
      !(live.sessionId === s.id && live.state === "ready")
    ) {
      const warmId = s.id;
      void (async () => {
        if (viewingSessionIdRef.current !== warmId) return;
        try {
          const snap = await api.sessionConnect({
            projectPath: proj?.path,
            sessionId: warmId,
          });
          if (viewingSessionIdRef.current !== warmId) return;
          setLiveHost(snap);
          liveHostRef.current = snap;
          if (snap.sessionId === warmId) {
            setSession((prev) => ({
              ...snap,
              title:
                prev.title ||
                s.title ||
                snap.title ||
                tr("session.untitled"),
            }));
          }
          if (snap.lastError && snap.state !== "ready") {
            // Soft: keep chat readable; send will retry via ensureConnected.
            console.warn(
              "warm connect:",
              snap.lastError.code,
              snap.lastError.message,
            );
          }
        } catch (e) {
          console.warn("warm connect failed", e);
        }
      })();
    }
  };

  /**
   * Focus composer after React commit. Retries until the textarea is mounted
   * (e.g. switching from automations → chat) or attempts run out.
   * Must be called after any await so state updates have been scheduled.
   */
  const requestComposerFocus = useCallback(() => {
    pendingComposerFocus.current = true;
    const tryFocus = (attemptsLeft: number) => {
      const el = composerInputRef.current;
      if (el && el.getAttribute("contenteditable") !== "false") {
        el.focus({ preventScroll: true });
        resizeComposer(el);
        try {
          const sel = window.getSelection();
          if (sel) {
            const range = document.createRange();
            range.selectNodeContents(el);
            range.collapse(false);
            sel.removeAllRanges();
            sel.addRange(range);
          }
        } catch {
          /* ignore */
        }
        if (document.activeElement === el) {
          pendingComposerFocus.current = false;
          return;
        }
      }
      if (attemptsLeft <= 0) {
        pendingComposerFocus.current = false;
        return;
      }
      requestAnimationFrame(() => tryFocus(attemptsLeft - 1));
    };
    // macOS: button click keeps focus on the button until the next tick.
    window.setTimeout(() => tryFocus(12), 0);
  }, []);

  /**
   * Draft new chat (Codex-style): clear UI only.
   * No store row / CLI until first successful send via ensureConnected.
   * Pass `null` for a project-less session (listed under “其他会话”).
   * Omit / pass undefined to use the active project (requires one).
   */
  const newChat = async (
    project?: Project | null,
    opts?: {
      seedDraft?: string;
      switchToChat?: boolean;
      /** Enter conversation-driven scheduled-task setup mode. */
      automationSetup?: boolean;
    },
  ) => {
    // Explicit null → orphan; undefined → fall back to active project.
    const wantOrphan = project === null;
    const proj = wantOrphan ? null : project || activeProject;
    if (!wantOrphan && !proj) {
      setLocalError(tr("project.addSelectFirst"));
      return;
    }
    if (proj && !proj.trusted) {
      setLocalError(tr("project.trustFirst", { name: proj.name }));
      return;
    }
    automationSetupDraftRef.current = !!opts?.automationSetup;
    if (opts?.switchToChat !== false) {
      setMainPane("chat");
      setAppView("workbench");
    }
    setActiveProject(proj);
    if (proj) {
      setExpandedProjects((e) => ({ ...e, [proj.id]: true }));
    } else {
      setHistoryOpen(true);
    }
    // Preserve outgoing thread in cache before clearing the draft UI.
    const leavingId = viewingSessionIdRef.current;
    if (leavingId) {
      const cachedLeaving = transcriptStore.getCached(leavingId);
      if (cachedLeaving) {
        transcriptStore.write(leavingId, cachedLeaving);
      }
    }
    const seedDraft = opts?.seedDraft ?? "";
    const recovery = composerRecoveryActionsRef.current;
    const recoveryReset = recovery?.resetDraft(seedDraft);
    if (!recovery) {
      setComposerRecoveryKey(COMPOSER_RECOVERY_DRAFT_KEY);
      setDraft(seedDraft);
      setAttachments([]);
      sendQueue.clearDraftQueue();
    }
    viewingSessionIdRef.current = null;
    transcriptStore.setViewing(null);
    transcriptStore.clearViewed();
    clearCompactSafetyTimer();
    setContextUsage(INITIAL_CONTEXT_USAGE);
    setPlan(emptyPlanState(tr("resources.plan")));
    setPerm(null);
    setAskUser(null);
    setRetryStatus(null);
    setSession({
      ...IDLE_SNAPSHOT,
      sessionId: null,
      title: tr("session.new"),
      state: "idle",
      backend: "sunsetz",
    });
    setLocalError(null);
    await recoveryReset;
    // Disconnect any live agent for previous session (best-effort).
    if (api.isTauri()) {
      try {
        await api.sessionDisconnect();
        const idle = { ...IDLE_SNAPSHOT };
        setLiveHost(idle);
        liveHostRef.current = idle;
      } catch {
        /* ignore */
      }
    }
    // Focus explicitly — do not rely only on useEffect: after await, effects may
    // already have run, and identical draft/sessionId can skip a re-render.
    requestComposerFocus();
  };

  const sidebarProjects = useMemo<SidebarProjectItem[]>(
    () =>
      projects.map((project) => ({
        id: project.id,
        name: project.name,
        path: project.path,
        trusted: project.trusted,
        pinned: !!project.pinned,
        open: expandedProjects[project.id] !== false,
        sessions: sessions
          .filter(
            (item) => item.projectId === project.id && !item.archived,
          )
          .map((item) => ({
            id: item.id,
            title: item.title,
            updatedAt: item.updatedAt,
            archived: !!item.archived,
            scheduled: !!item.scheduled,
          })),
      })),
    [expandedProjects, projects, sessions],
  );

  const sidebarOrphanSessions = useMemo<SidebarSessionItem[]>(
    () =>
      sessions
        .filter(
          (item) =>
            (!item.projectId ||
              !projects.some((project) => project.id === item.projectId)) &&
            !item.archived,
        )
        .map((item) => ({
          id: item.id,
          title: item.title,
          updatedAt: item.updatedAt,
          archived: !!item.archived,
          scheduled: !!item.scheduled,
        })),
    [projects, sessions],
  );

  /** Archived chats grouped by project for Settings → Archived. */
  const archivedGroups = useMemo(() => {
    const archived = sessions
      .filter((s) => s.archived)
      .slice()
      .sort(
        (a, b) =>
          new Date(b.updatedAt).getTime() - new Date(a.updatedAt).getTime(),
      );
    const byProject = new Map<string | null, SessionRow[]>();
    for (const s of archived) {
      const key =
        s.projectId && projects.some((p) => p.id === s.projectId)
          ? s.projectId
          : null;
      const list = byProject.get(key) ?? [];
      list.push(s);
      byProject.set(key, list);
    }
    const groups: Array<{
      id: string | null;
      name: string;
      sessions: SessionRow[];
    }> = [];
    // Stable order: pin projects list order, then orphan bucket.
    for (const p of projects) {
      const list = byProject.get(p.id);
      if (list?.length) {
        groups.push({ id: p.id, name: p.name, sessions: list });
      }
    }
    const orphan = byProject.get(null);
    if (orphan?.length) {
      groups.push({
        id: null,
        name: tr("settings.archived.orphan"),
        sessions: orphan,
      });
    }
    return groups;
  }, [sessions, projects, tr]);

  /** Session id currently running on the Host (for sidebar spinner). */
  const busySessionIds = useMemo(() => {
    const ids = new Set(liveHost.busySessionIds ?? []);
    if (liveHost.sessionId && isSessionBusy(liveHost.state)) {
      ids.add(liveHost.sessionId);
    }
    return ids;
  }, [liveHost.busySessionIds, liveHost.sessionId, liveHost.state]);

  const refreshSessions = async () => {
    try {
      const list = await api.sessionsList();
      setSessions(
        list.map((s) => ({
          id: s.id,
          title: s.title,
          projectId: s.projectId,
          updatedAt: s.updatedAt,
          archived: !!s.archived,
          scheduled: !!s.scheduled,
        })),
      );
      void api.trayRefresh();
    } catch {
      /* ignore */
    }
  };

  /**
   * Run a scheduled automation now: open chat under its project (or orphan),
   * connect, and send the stored prompt.
   * @returns true if the prompt was handed to the agent (mark_run applied).
   */
  const runAutomation = useCallback(
    async (
      auto: Automation,
      opts?: { fromScheduler?: boolean; claimId?: string },
    ): Promise<boolean> => {
      if (automationRunLock.current) return false;
      if (opts?.fromScheduler && (session.state === "streaming" || connecting)) {
        return false;
      }
      automationRunLock.current = true;
      let createdSessionId: string | null = null;
      try {
        const proj = auto.projectId
          ? projects.find((p) => p.id === auto.projectId) ?? null
          : null;
        if (proj && !proj.trusted) {
          setLocalError(tr("project.trustFirst", { name: proj.name }));
          return false;
        }
        setMainPane("chat");
        setAppView("workbench");
        setActiveProject(proj);
        if (proj) {
          setExpandedProjects((e) => ({ ...e, [proj.id]: true }));
        } else {
          setHistoryOpen(true);
        }
        openingSessionIdRef.current = null;
        viewingSessionIdRef.current = null;
        transcriptStore.setViewing(null);
        transcriptStore.clearViewed();
        setAttachments([]);
        setPerm(null);
        setAskUser(null);
        setRetryStatus(null);
        setLocalError(null);
        setDraft("");
        if (api.isTauri()) {
          try {
            await api.sessionDisconnect();
          } catch {
            /* ignore */
          }
        }
        setSession({
          ...IDLE_SNAPSHOT,
          sessionId: null,
          title: auto.title || tr("session.new"),
          state: "idle",
          backend: "sunsetz",
        });
        {
          const idle = { ...IDLE_SNAPSHOT };
          setLiveHost(idle);
          liveHostRef.current = idle;
        }

        let sessionId: string | null = null;
        if (api.isTauri()) {
          const meta = (await api.sessionCreate(
            proj?.id,
            auto.title || tr("session.new"),
            { scheduled: true },
          )) as { id: string; title?: string; scheduled?: boolean };
          sessionId = meta.id;
          createdSessionId = meta.id;
          if (opts?.claimId) {
            await api.automationClaimBindV1(opts.claimId, meta.id);
          }
          viewingSessionIdRef.current = meta.id;
          setSession((prev) => ({
            ...prev,
            sessionId: meta.id,
            title: meta.title || auto.title,
          }));
          await refreshSessions();
        }

        // Persist model/effort for this session before connect when possible.
        if (sessionId && api.isTauri() && (auto.modelId || auto.effort)) {
          try {
            await api.composerPrefsSet({
              sessionId,
              projectId: proj?.id ?? null,
              modelId: auto.modelId,
              effort: auto.effort,
            });
          } catch {
            /* soft-fail */
          }
        }

        const snap = await api.sessionConnect({
          projectPath: proj?.path,
          sessionId: sessionId ?? undefined,
          mode: "agent",
        });
        setLiveHost(snap);
        liveHostRef.current = snap;
        if (snap.sessionId) {
          viewingSessionIdRef.current = snap.sessionId;
          sessionId = snap.sessionId;
        }
        setSession({
          ...snap,
          title: snap.title || auto.title || snap.title,
        });
        if (snap.lastError || snap.state !== "ready") {
          const code = snap.lastError?.code ?? "AGENT_CRASHED";
          const msg = snap.lastError?.message ?? "connect failed";
          const detail = `${code}: ${msg}`;
          setLocalError(
            tr("automations.connectFailed", { detail }),
          );
          // Drop empty shell sessions so sidebar does not show Sunsetz Pro ghosts.
          if (createdSessionId && api.isTauri()) {
            try {
              await api.sessionDelete(createdSessionId);
              await refreshSessions();
            } catch {
              /* ignore */
            }
            if (viewingSessionIdRef.current === createdSessionId) {
              viewingSessionIdRef.current = null;
              transcriptStore.setViewing(null);
              transcriptStore.clearViewed();
              setSession({ ...IDLE_SNAPSHOT, state: "idle" });
            }
          }
          return false;
        }

        if (sessionId && auto.modelId && api.isTauri()) {
          try {
            await api.sessionSetModel(auto.modelId, {
              sessionId,
              projectId: proj?.id ?? null,
            });
          } catch {
            /* soft-fail */
          }
        }

        const header = `[Scheduled: ${auto.title}]\n\n`;
        const promptBody = header + auto.prompt;
        const autoMsgs: ChatMessage[] = [
          {
            id: `u-auto-${Date.now()}`,
            role: "user",
            content: promptBody,
            createdAt: new Date().toISOString(),
          },
        ];
        if (sessionId) {
          transcriptStore.write(sessionId, autoMsgs);
        }
        transcriptStore.replaceViewed(autoMsgs);
        setSession((prev) => ({
          ...prev,
          state: "streaming",
          lastError: null,
          title: auto.title || prev.title,
        }));

        try {
          await api.sessionSend(promptBody);
        } catch (sendErr) {
          const errText = String(sendErr);
          const failed: ChatMessage[] = [
            ...autoMsgs,
            {
              id: `err-auto-${Date.now()}`,
              role: "assistant",
              content: errText,
              isError: true,
              createdAt: new Date().toISOString(),
            },
          ];
          if (sessionId) {
            transcriptStore.write(sessionId, failed);
          }
          transcriptStore.replaceViewed(failed);
          setLocalError(errText);
          setSession((prev) =>
            prev.sessionId === sessionId
              ? { ...prev, state: "ready" }
              : prev,
          );
          return false;
        }

        if (!opts?.fromScheduler) {
          const lastRunAt = new Date().toISOString();
          const nextRunAt =
            auto.frequency === "once"
              ? null
              : computeNextRunAt(
                  { ...auto, enabled: auto.frequency !== "once" },
                  new Date(Date.now() + 60_000),
                );
          await api.automationMarkRun(auto.id, lastRunAt, nextRunAt);
          if (auto.frequency === "once") {
            await api.automationSetEnabled(auto.id, false);
          }
        }
        setToast(tr("automations.runningToast", { title: auto.title }));
        window.setTimeout(() => setToast(null), 3200);
        return true;
      } catch (e) {
        setLocalError(String(e));
        return false;
      } finally {
        automationRunLock.current = false;
      }
    },
    [projects, session.state, connecting, tr],
  );

  // Rust Host owns due-time polling and atomic claims. The WebView only runs
  // an already-claimed prompt through the existing ACP session path.
  useEffect(() => {
    if (!api.isTauri()) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void api
      .listen<api.AutomationClaimV1>("automation://claim_v1", (claim) => {
        const intake = automationClaimIntake(
          claim,
          handledAutomationClaimIds.current,
          disposed,
        );
        if (intake === "ignore") return;
        // A bound claim is already executing in the Host-owned session. After
        // a WebView reload, wait for Host completion instead of sending the
        // scheduled prompt a second time.
        if (intake === "bound") {
          handledAutomationClaimIds.current.add(claim.claimId);
          void refreshSessions();
          return;
        }
        setPendingAutomationClaim((current) => current ?? claim);
      })
      .then((dispose) => {
        if (disposed) dispose();
        else unlisten = dispose;
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    const claim = pendingAutomationClaim;
    if (
      !claim ||
      !canStartAutomationClaim({
        claim,
        connecting,
        sessionState: session.state,
        runLocked: automationRunLock.current,
        handledIds: handledAutomationClaimIds.current,
      })
    ) {
      return;
    }
    handledAutomationClaimIds.current.add(claim.claimId);
    void runAutomation(claim.automation as Automation, {
      fromScheduler: true,
      claimId: claim.claimId,
    })
      .then(async (success) => {
        if (!success) {
          await api.automationClaimCompleteV1(
            claim.claimId,
            false,
            "automation execution did not start",
          );
        }
      })
      .catch(async (error) => {
        await api
          .automationClaimCompleteV1(claim.claimId, false, String(error))
          .catch(() => {});
      })
      .finally(() => {
        setPendingAutomationClaim(null);
      });
  }, [connecting, pendingAutomationClaim, runAutomation, session.state]);

  const refreshProjects = async () => {
    try {
      const list = await api.projectsList();
      setProjects(
        list.map((p) => ({
          ...p,
          pinned: !!p.pinned,
        })),
      );
    } catch {
      /* ignore */
    }
  };

  const applySessionTitle = useCallback(
    (sessionId: string, title: string) => {
      setSessions((list) =>
        list.map((s) => (s.id === sessionId ? { ...s, title } : s)),
      );
      setSession((prev) =>
        prev.sessionId === sessionId ? { ...prev, title } : prev,
      );
      void api.trayRefresh();
    },
    [],
  );

  const editProject = (proj: Project) => {
    setCtxMenu(null);
    setAppDialog({
      kind: "edit-project",
      title: tr("project.editTitle"),
      projectId: proj.id,
      name: proj.name,
      path: proj.path,
      onSubmit: async (name, path) => {
        const nextName = name.trim();
        const nextPath = path.trim();
        if (!nextName) return;
        try {
          if (nextName !== proj.name) {
            await api.projectRename(proj.id, nextName);
          }
          if (nextPath && nextPath !== proj.path) {
            await api.projectSetPath(proj.id, nextPath);
          }
          await refreshProjects();
          void api.trayRefresh();
          if (activeProject?.id === proj.id) {
            setActiveProject((p) =>
              p
                ? {
                    ...p,
                    name: nextName,
                    path: nextPath || p.path,
                  }
                : p,
            );
          }
        } catch (e) {
          setLocalError(String(e));
        }
      },
    });
  };

  /**
   * Apply a project-level permission tier (L10).
   * `null` clears the override so the app default is used again.
   * YOLO still requires the same two-step confirm as the composer chip.
   */
  const applyProjectPermissionPolicy = (
    proj: Project,
    next: PermissionPolicyId | null,
  ) => {
    setCtxMenu(null);

    const commit = async () => {
      try {
        const updated = (await api.projectSetPermissionPolicy(
          proj.id,
          next,
        )) as Project;
        await refreshProjects();
        if (activeProject?.id === proj.id) {
          setActiveProject((p) =>
            p
              ? {
                  ...p,
                  permissionPolicy: updated.permissionPolicy ?? null,
                }
              : p,
          );
          await reresolve();
        }
        const msg = next
          ? tr("project.permissionSet", {
              name: proj.name,
              policy: tr(
                (
                  {
                    ask: "policy.short.ask",
                    accept_edits: "policy.short.accept_edits",
                    allow_for_session: "policy.short.allow_for_session",
                    dont_ask: "policy.short.dont_ask",
                    always_approve: "policy.short.always_approve",
                  } as const
                )[next],
              ),
            })
          : tr("project.permissionCleared", { name: proj.name });
        setToast(msg);
        window.setTimeout(() => setToast((cur) => (cur === msg ? null : cur)), 2800);
      } catch (e) {
        setLocalError(String(e));
      }
    };

    if (next === "always_approve") {
      setAppDialog({
        kind: "confirm",
        title: tr("policy.always_approve"),
        message: tr("policy.yoloConfirm"),
        confirmLabel: tr("common.confirm"),
        danger: true,
        onConfirm: () => {
          setAppDialog({
            kind: "confirm",
            title: tr("policy.always_approve"),
            message: tr("policy.yoloConfirm2"),
            confirmLabel: tr("policy.short.always_approve"),
            danger: true,
            onConfirm: () => {
              void commit();
            },
          });
        },
      });
      return;
    }

    void commit();
  };

  /** Remove project from the app list and delete its chats. Disk folder kept. */
  const removeProjectFromApp = (proj: Project) => {
    setCtxMenu(null);
    setAppDialog({
      kind: "confirm",
      title: tr("project.removeTitle"),
      message: tr("project.removeConfirmDetail", { name: proj.name }),
      confirmLabel: tr("project.remove"),
      danger: true,
      onConfirm: async () => {
        try {
          if (!api.isTauri()) {
            setLocalError(tr("error.needTauri"));
            return;
          }
          const removed = sessions.filter((row) => row.projectId === proj.id);
          await api.projectRemove(proj.id);
          const recovery = composerRecoveryActionsRef.current;
          for (const row of removed) {
            if (recovery) {
              await recovery.deleteKey(row.id);
            }
            transcriptStore.evict(row.id);
          }
          if (!recovery) {
            sendQueue.dropKeys(removed.map((row) => row.id));
          }
          const openId =
            session.sessionId ?? viewingSessionIdRef.current ?? null;
          if (
            activeProject?.id === proj.id ||
            (openId && removed.some((row) => row.id === openId))
          ) {
            setActiveProject(null);
            setSession(IDLE_SNAPSHOT);
            transcriptStore.clearViewed();
          }
          await refreshProjects();
          await refreshSessions();
          setLocalError(null);
        } catch (e) {
          setLocalError(String(e));
        }
      },
    });
  };

  const renameSession = (s: SessionRow) => {
    setCtxMenu(null);
    setAppDialog({
      kind: "prompt",
      title: tr("session.renamePrompt"),
      initial: s.title || tr("session.untitled"),
      placeholder: tr("session.renamePlaceholder"),
      onSubmit: async (title) => {
        const next = title.trim();
        if (!next) return;
        try {
          await api.sessionRename(s.id, next);
          applySessionTitle(s.id, next);
          await refreshSessions();
        } catch (e) {
          setLocalError(String(e));
        }
      },
    });
  };

  /**
   * Archive / unarchive a session.
   * If the open conversation is archived, leave it for a fresh draft so the
   * main pane does not keep showing a chat that disappeared from the tree.
   */
  const archiveSession = async (s: SessionRow, archived = true) => {
    setCtxMenu(null);
    const wasViewing =
      archived &&
      (session.sessionId === s.id || viewingSessionIdRef.current === s.id);
    try {
      await api.sessionSetArchived(s.id, archived);
      await refreshSessions();
      if (wasViewing) {
        const proj = s.projectId
          ? projects.find((p) => p.id === s.projectId) ?? null
          : null;
        // Same project context when possible; orphan → “其他会话” draft.
        if (proj) await newChat(proj, { switchToChat: true });
        else await newChat(null, { switchToChat: true });
      } else if (!archived && s.projectId) {
        setExpandedProjects((e) => ({ ...e, [s.projectId!]: true }));
      }
    } catch (e) {
      setLocalError(String(e));
    }
  };

  /** Permanent delete — confirm first; leave workbench if viewing that chat. */
  const deleteSessionConfirm = (s: SessionRow) => {
    deleteSessionsConfirm([s]);
  };

  /** Bulk restore archived sessions. */
  const restoreSessions = async (rows: SessionRow[]) => {
    if (!rows.length) return;
    try {
      if (!api.isTauri()) {
        setLocalError(tr("error.needTauri"));
        return;
      }
      for (const s of rows) {
        await api.sessionSetArchived(s.id, false);
        if (s.projectId) {
          setExpandedProjects((e) => ({ ...e, [s.projectId!]: true }));
        }
      }
      await refreshSessions();
      setLocalError(null);
    } catch (e) {
      setLocalError(String(e));
    }
  };

  /** Bulk permanent delete with one confirm. */
  const deleteSessionsConfirm = (rows: SessionRow[]) => {
    setCtxMenu(null);
    if (!rows.length) return;
    const n = rows.length;
    const title =
      n === 1
        ? rows[0].title || tr("session.untitled")
        : tr("session.deleteManyTitle");
    const message =
      n === 1
        ? tr("session.deleteConfirm", {
            name: rows[0].title || tr("session.untitled"),
          })
        : tr("session.deleteManyConfirm", { n: String(n) });
    setAppDialog({
      kind: "confirm",
      title: n === 1 ? tr("session.deleteTitle") : title,
      message,
      confirmLabel: tr("session.delete"),
      danger: true,
      onConfirm: async () => {
        try {
          if (!api.isTauri()) {
            setLocalError(tr("error.needTauri"));
            return;
          }
          const openId =
            session.sessionId ?? viewingSessionIdRef.current ?? null;
          const wasViewing = !!openId && rows.some((s) => s.id === openId);
          const viewingRow = wasViewing
            ? rows.find((s) => s.id === openId)
            : null;
          for (const s of rows) {
            const recovery = composerRecoveryActionsRef.current;
            if (recovery && !(await recovery.deleteKey(s.id))) {
              throw new Error(`Failed to delete composer recovery for ${s.id}`);
            }
            await api.sessionDelete(s.id);
            transcriptStore.evict(s.id);
            if (!recovery) sendQueue.dropKeys([s.id]);
          }
          await refreshSessions();
          if (wasViewing && viewingRow) {
            const proj = viewingRow.projectId
              ? projects.find((p) => p.id === viewingRow.projectId) ?? null
              : null;
            if (proj) await newChat(proj, { switchToChat: true });
            else await newChat(null, { switchToChat: true });
          }
          setLocalError(null);
        } catch (e) {
          setLocalError(String(e));
        }
      },
    });
  };

  /** Archive all chats under a project; exit mid-pane if current chat is among them. */
  const archiveProjectSessions = async (proj: Project) => {
    setCtxMenu(null);
    const openId = session.sessionId ?? viewingSessionIdRef.current;
    const openBelongs =
      !!openId &&
      sessions.some((s) => s.id === openId && s.projectId === proj.id);
    try {
      await api.projectArchiveSessions(proj.id);
      await refreshSessions();
      if (openBelongs) {
        await newChat(proj, { switchToChat: true });
      }
    } catch (e) {
      setLocalError(String(e));
    }
  };

  const copySessionId = async (s: SessionRow) => {
    setCtxMenu(null);
    try {
      await navigator.clipboard.writeText(s.id);
    } catch {
      setLocalError(s.id);
    }
  };

  const contextMenuPosition = (
    e: ReactMouseEvent,
  ): ContextMenuPosition => {
    const trigger = e.currentTarget as HTMLElement;
    if (e.type === "contextmenu") {
      return {
        x: e.clientX,
        y: e.clientY,
        restoreFocusTo: trigger,
      };
    }
    const rect = trigger.getBoundingClientRect();
    return {
      x: rect.left,
      y: rect.bottom + 6,
      anchorRect: {
        left: rect.left,
        right: rect.right,
        top: rect.top,
        bottom: rect.bottom,
        width: rect.width,
        height: rect.height,
      },
      restoreFocusTo: trigger,
    };
  };

  const openSessionMenu = (e: ReactMouseEvent, s: SessionRow) => {
    e.preventDefault();
    e.stopPropagation();
    setCtxMenu({
      kind: "session",
      id: s.id,
      ...contextMenuPosition(e),
    });
  };

  const openProjectMenu = (e: ReactMouseEvent, proj: Project) => {
    e.preventDefault();
    e.stopPropagation();
    setCtxMenu({
      kind: "project",
      id: proj.id,
      ...contextMenuPosition(e),
    });
  };

  useEffect(() => {
    const query = searchQuery.trim();
    if (!showSearch || !query || !api.isTauri()) {
      setContentSearchHits([]);
      return;
    }
    let cancelled = false;
    const timer = window.setTimeout(() => {
      void api
        .sessionSearchV1(query, 40)
        .then((hits) => {
          if (!cancelled) setContentSearchHits(hits);
        })
        .catch(() => {
          if (!cancelled) setContentSearchHits([]);
        });
    }, 180);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [searchQuery, showSearch]);

  const searchHits = useMemo(() => {
    const base = filterSessionSearch(
        searchQuery,
        sessions.map((s) => ({
          id: s.id,
          title: s.title,
          projectId: s.projectId,
          archived: s.archived,
        })),
        projects.map((p) => ({ id: p.id, name: p.name, path: p.path })),
      );
    const merged = mergeContentSearchSessions(
      base.matchedSessions,
      sessions,
      contentSearchHits,
    );
    return { ...base, matchedSessions: merged };
  }, [contentSearchHits, searchQuery, sessions, projects]);

  const connPill = useMemo(
    () => connPillForState(session.state, connecting),
    [session.state, connecting],
  );

  const isPlaceholderTitle = useCallback(
    (title: string | undefined | null) => {
      const t = (title || "").trim();
      if (!t) return true;
      const placeholders = [
        tr("session.new"),
        tr("session.placeholderTitle"),
        tr("session.untitled"),
        "New chat",
        "新会话",
        "Untitled",
        "未命名",
      ];
      return placeholders.some((p) => p.toLowerCase() === t.toLowerCase());
    },
    [tr],
  );

  /**
   * Ensure app session row + silent CLI connect.
   * Creates store session only on first send (draft → real).
   * Reconnects when disconnected / crashed. Pass force to tear down a "ready"
   * session that may be wedged (e.g. after a timeout).
   * Returns the live session id when ready, else null.
   *
   * Prefer `opts.sessionId` (e.g. queue flush target) over the render-time
   * `session` closure so connect never binds the wrong chat after a switch.
   *
   * Does not yank the UI if the user already switched to another session while
   * connect is in flight; still updates liveHost so the sidebar spinner tracks work.
   */
  const ensureConnected = async (
    forceOrOpts:
      | boolean
      | { force?: boolean; sessionId?: string | null } = false,
  ): Promise<string | null> => {
    const opts =
      typeof forceOrOpts === "boolean"
        ? { force: forceOrOpts, sessionId: undefined as string | null | undefined }
        : forceOrOpts;
    const force = !!opts.force;
    // Explicit target wins; else the session this render is bound to.
    const preferredId =
      opts.sessionId !== undefined ? opts.sessionId : session.sessionId;

    // Project-less (orphan) sessions are allowed: cwd falls back on Host.
    if (activeProject && !activeProject.trusted) {
      setLocalError(tr("project.trustFirst", { name: activeProject.name }));
      return null;
    }
    // Fast path: already ready on the *preferred* session (not merely "any" ready).
    if (
      !force &&
      preferredId &&
      session.sessionId === preferredId &&
      session.state === "ready" &&
      !session.lastError
    ) {
      return preferredId;
    }
    // Live host may already be on the target even if viewed session differs.
    if (!force && preferredId) {
      const live = liveHostRef.current;
      if (
        live.sessionId === preferredId &&
        live.state === "ready" &&
        !live.lastError
      ) {
        return preferredId;
      }
    }
    if (connecting) return null;
    setConnecting(true);
    // Capture draft identity before awaits (may still be null).
    const viewedBefore = viewingSessionIdRef.current;
    try {
      let sessionId = preferredId ?? null;
      // First send: materialize draft into a real session (project or orphan).
      if (!sessionId && api.isTauri()) {
        const meta = (await api.sessionCreate(
          activeProject?.id,
          tr("session.new"),
        )) as { id: string; title?: string };
        sessionId = meta.id;
        const recovery = composerRecoveryActionsRef.current;
        const recoveryMigrated = recovery
          ? await recovery.migrateDraft(meta.id)
          : true;
        if (!recoveryMigrated) {
          // The row is still empty; roll it back so retry keeps the same draft.
          await api.sessionDelete(meta.id).catch(() => {});
          setLocalError("COMPOSER_RECOVERY_MIGRATION_FAILED");
          return null;
        }
        if (!recovery) {
          sendQueue.migrateDraft(meta.id);
          setComposerRecoveryKey(meta.id);
        }
        // Bind draft messages cache to the new id without wiping the viewed thread.
        const draftMsgs = transcriptStore.getCached("__draft__");
        if (draftMsgs?.length) {
          const viewingDraft =
            transcriptStore.getViewingId() == null ||
            transcriptStore.getViewingId() === DRAFT_SESSION_KEY;
          if (
            viewingDraft &&
            (viewingSessionIdRef.current === viewedBefore ||
              viewingSessionIdRef.current === null ||
              viewingSessionIdRef.current === meta.id)
          ) {
            transcriptStore.adoptViewing(null, meta.id);
          } else {
            transcriptStore.write(meta.id, draftMsgs);
            transcriptStore.evict("__draft__");
          }
        }
        // Only take over the workbench if still on this draft / same session.
        if (
          viewingSessionIdRef.current === viewedBefore ||
          viewingSessionIdRef.current === null ||
          viewingSessionIdRef.current === meta.id
        ) {
          viewingSessionIdRef.current = meta.id;
          setSession((prev) => ({
            ...prev,
            sessionId: meta.id,
            title: meta.title || tr("session.new"),
          }));
        }
        if (activeProject) {
          setExpandedProjects((e) => ({ ...e, [activeProject.id]: true }));
        } else {
          setHistoryOpen(true);
        }
        await refreshSessions();
      }
      const snap = await api.sessionConnect({
        projectPath: activeProject?.path,
        sessionId: sessionId ?? undefined,
        mode,
      });
      setLiveHost(snap);
      liveHostRef.current = snap;
      // Only rebind viewed session when the user is still on it (or its draft).
      if (
        snap.sessionId &&
        (viewingSessionIdRef.current === snap.sessionId ||
          viewingSessionIdRef.current === viewedBefore ||
          (viewedBefore === null &&
            viewingSessionIdRef.current === snap.sessionId))
      ) {
        viewingSessionIdRef.current = snap.sessionId;
        setSession(snap);
      }
      if (snap.lastError || snap.state !== "ready") {
        const code = snap.lastError?.code ?? "AGENT_CRASHED";
        const msg = snap.lastError?.message ?? "connect failed";
        if (viewingSessionIdRef.current === (snap.sessionId || sessionId)) {
          setLocalError(`${code}: ${msg}`);
        }
        return null;
      }
      if (viewingSessionIdRef.current === (snap.sessionId || sessionId)) {
        setLocalError(null);
      }
      return snap.sessionId || sessionId || null;
    } catch (e) {
      if (
        viewingSessionIdRef.current === viewedBefore ||
        viewingSessionIdRef.current === preferredId ||
        viewingSessionIdRef.current === session.sessionId
      ) {
        setLocalError(String(e));
      }
      return null;
    } finally {
      setConnecting(false);
    }
  };

  const attachLabels = useMemo(
    () => ({
      open: tr("attach.open"),
      reveal: tr("attach.reveal"),
      copyPath: tr("attach.copyPath"),
      copyImage: tr("attach.copyImage"),
      addToComposer: tr("attach.addToComposer"),
      remove: tr("composer.attachRemove"),
      viewImage: tr("image.view"),
    }),
    [tr],
  );

  const lastUserMessageId = transcriptMeta.lastUserId;

  const canEditLastUser =
    !!lastUserMessageId &&
    canSend(session.state) &&
    !connecting &&
    session.state !== "streaming" &&
    session.state !== "awaiting_permission";

  /** Idle-ish: allow fork / rewind from transcript (not mid-turn). */
  const canRewindSession =
    canSend(session.state) &&
    !connecting &&
    !editSubmitting &&
    !rewindBusy;

  /**
   * Recovery stores attachment references, never capabilities. Re-run the
   * normal Host classification immediately before every send and use only the
   * fresh canonical metadata; missing/filtered paths abort the turn.
   */
  const classifyAttachmentsForSend = async (
    input: Attachment[],
  ): Promise<Attachment[] | null> => {
    if (!input.length || !api.isTauri()) {
      return input.map((attachment) => ({ ...attachment }));
    }
    try {
      const classified = await api.pathsClassify(
        input.map((attachment) => attachment.path),
      );
      if (
        classified.length !== input.length ||
        classified.some((entry) => !entry.exists)
      ) {
        setLocalError(tr("attach.droppedNone"));
        return null;
      }
      return classified.map((entry) => ({
        path: entry.path,
        name: entry.name,
        isDir: entry.isDir,
      }));
    } catch (error) {
      setLocalError(String(error) || tr("attach.droppedNone"));
      return null;
    }
  };

  /**
   * Dispatch one user turn (optimistic UI + connect + session_send).
   * @param targetSessionId When set (queue flush), bind optimistic UI to this id.
   * @param fromQueue Drop user+assistant on failure so requeue does not duplicate.
   */
  const executeSend = async (opts: {
    storedDisplay: string;
    att: Attachment[];
    goalMode: boolean;
    /** Optional private Agent form; the journal still stores storedDisplay. */
    agentTextOverride?: string;
    /** Direct composer send already classified immediately before dispatch. */
    attachmentsClassified?: boolean;
    /** Explicitly reviewed Memory for this immediate turn only. */
    memoryContextPack?: api.MemoryContextPackV1;
    fromQueue?: boolean;
    targetSessionId?: string | null;
  }): Promise<boolean> => {
    if (sendInFlightRef.current) return false;
    sendInFlightRef.current = true;
    const {
      storedDisplay,
      att: requestedAttachments,
      goalMode: useGoal,
      fromQueue,
    } = opts;
    const segments = parseStoredContent(storedDisplay);
    if (isDraftEmpty(segments) && !requestedAttachments.length) {
      sendInFlightRef.current = false;
      return false;
    }
    const boundSkills = new Map<
      string,
      { name: string; binding: api.SkillSelectionRequestV1 }
    >();
    const boundSkillIds = new Map<string, string>();
    for (const segment of segments) {
      if (segment.type !== "skill") continue;
      const name = segment.name.toLowerCase();
      if (!segment.binding) {
        setLocalError(`SKILL_USE_STALE: ${segment.name}`);
        sendInFlightRef.current = false;
        return false;
      }
      const binding: api.SkillSelectionRequestV1 = {
        id: segment.binding.id,
        expectedTreeHash: segment.binding.expectedTreeHash,
        selection: segment.binding.selection,
      };
      const existing = boundSkills.get(name);
      if (
        existing &&
        (existing.binding.id !== binding.id ||
          existing.binding.expectedTreeHash !== binding.expectedTreeHash ||
          existing.binding.selection !== binding.selection)
      ) {
        setLocalError(`SKILL_USE_CONFLICT: ${segment.name}`);
        sendInFlightRef.current = false;
        return false;
      }
      const priorName = boundSkillIds.get(binding.id);
      if (priorName && priorName !== name) {
        setLocalError(`SKILL_USE_CONFLICT: ${segment.name}`);
        sendInFlightRef.current = false;
        return false;
      }
      const fresh = skillInfos.find(
        (candidate) =>
          candidate.id === binding.id &&
          candidate.treeHash === binding.expectedTreeHash &&
          candidate.name.toLowerCase() === name,
      );
      if (!fresh) {
        setLocalError(`SKILL_USE_STALE: ${segment.name}`);
        sendInFlightRef.current = false;
        return false;
      }
      boundSkills.set(name, { name: segment.name, binding });
      boundSkillIds.set(binding.id, name);
    }
    const skillSelections = Array.from(boundSkills.values(), (item) =>
      item.binding,
    );
    const connectorSelections: Array<{ id: string; selection: "explicit" }> =
      [];
    for (const segment of segments) {
      if (segment.type !== "connector") continue;
      const connected = connectorStatesRef.current.some(
        (row) => row.id === segment.id && row.connected,
      );
      if (!connected) {
        const entry = CONNECTOR_CATALOG.find((item) => item.id === segment.id);
        setLocalError(
          tr("plugin.notConnected", {
            name: entry ? tr(entry.nameKey as MessageKey) : segment.id,
          }),
        );
        sendInFlightRef.current = false;
        return false;
      }
      if (!connectorSelections.some((row) => row.id === segment.id)) {
        connectorSelections.push({ id: segment.id, selection: "explicit" });
      }
    }
    const journalDisplay = serializeDisplayForJournal(segments);
    const classifiedAttachments = opts.attachmentsClassified
      ? requestedAttachments.map((attachment) => ({ ...attachment }))
      : await classifyAttachmentsForSend(requestedAttachments);
    if (!classifiedAttachments) {
      sendInFlightRef.current = false;
      return false;
    }
    const att = classifiedAttachments;
    const sendTargetId =
      opts.targetSessionId !== undefined
        ? opts.targetSessionId
        : session.sessionId;
    const cacheKey = sendTargetId ?? "__draft__";
    const viewingTarget = () =>
      viewingSessionIdRef.current === sendTargetId ||
      (sendTargetId == null && viewingSessionIdRef.current == null);

    const agentBody = serializeForAgent(segments, { goalMode: useGoal });
    const agentOverride = opts.agentTextOverride?.trim() || null;
    let agentText = agentOverride || buildAgentPrompt(agentBody, att);
    const scheduleIntent = !agentOverride && looksLikeScheduleIntent(agentText);
    const inAutomationSetup =
      !agentOverride &&
      (automationSetupDraftRef.current ||
        scheduleIntent ||
        (!!sendTargetId &&
          automationSetupSessionsRef.current.has(sendTargetId)));
    if (inAutomationSetup) {
      agentText = wrapAutomationSetupAgentText(agentText);
    }
    const titleSeed =
      serializeForAgent(segments).replace(/\n/g, " ").trim() ||
      att.map((a) => a.name).join(", ");
    const shouldAutoTitle =
      isPlaceholderTitle(session.title) || !sendTargetId;
    const ts = Date.now();
    const userMessageId = `u-${ts}`;
    const pendingAssistantId = `a-pending-${ts}`;
    const dropIds = fromQueue
      ? new Set([userMessageId, pendingAssistantId])
      : new Set([pendingAssistantId]);
    const stripOptimistic = (m: ChatMessage[]) =>
      m.filter((x) => !dropIds.has(x.id));

    if (editingUserMessageId) {
      setEditingUserMessageId(null);
      setEditAttachments([]);
    }

    if (viewingTarget()) setRetryStatus(null);
    const nowIso = new Date().toISOString();
    const appendOptimistic = (m: ChatMessage[]): ChatMessage[] => {
      const cleaned = clearPriorTurnStreaming(m);
      return [
        ...cleaned,
        {
          id: userMessageId,
          role: "user",
          content: journalDisplay,
          attachments: att.length ? att : undefined,
          createdAt: nowIso,
        },
        {
          id: pendingAssistantId,
          role: "assistant",
          content: "",
          streaming: true,
        },
      ];
    };
    if (sendTargetId) {
      patchSessionMessages(sendTargetId, appendOptimistic);
    } else if (viewingTarget()) {
      patchSessionMessages(cacheKey, appendOptimistic);
    } else {
      const prev = transcriptStore.getCached(cacheKey) ?? [];
      transcriptStore.write(cacheKey, appendOptimistic(prev));
    }
    if (viewingTarget()) {
      transcriptStore.rebindViewing(sendTargetId ?? null);
    }
    if (viewingTarget()) {
      setSession((prev) =>
        prev.state === "streaming" || prev.state === "awaiting_permission"
          ? prev
          : { ...prev, state: "streaming", lastError: null },
      );
      setTurnStartedAt(Date.now());
    }
    setLiveHost((prev) => {
      if (sendTargetId && prev.sessionId && prev.sessionId !== sendTargetId) {
        return prev;
      }
      const next = {
        ...prev,
        sessionId: sendTargetId ?? prev.sessionId,
        state: "streaming" as const,
        lastError: null,
      };
      liveHostRef.current = next;
      return next;
    });

    const failStrip = () => {
      if (sendTargetId) {
        patchSessionMessages(sendTargetId, stripOptimistic);
      } else {
        const draftMsgs = transcriptStore.getCached("__draft__");
        if (draftMsgs) {
          transcriptStore.write(
            "__draft__",
            stripOptimistic(draftMsgs),
          );
        }
        if (viewingTarget()) patchSessionMessages(cacheKey, stripOptimistic);
      }
      if (viewingTarget()) {
        setSession((prev) =>
          prev.state === "streaming"
            ? { ...prev, state: prev.sessionId ? "ready" : prev.state }
            : prev,
        );
      }
      // Symmetric rollback of optimistic liveHost streaming — otherwise
      // useSendQueue.flush sees streaming forever and auto-flush starves.
      setLiveHost((prev) => {
        if (
          sendTargetId &&
          prev.sessionId &&
          prev.sessionId !== sendTargetId
        ) {
          return prev;
        }
        if (prev.state !== "streaming") return prev;
        const next = {
          ...prev,
          state: (prev.sessionId ? "ready" : "idle") as SessionSnapshot["state"],
        };
        liveHostRef.current = next;
        return next;
      });
    };

    try {
      let sessionId: string | null = null;
      const live = liveHostRef.current;
      if (
        sendTargetId &&
        live.sessionId === sendTargetId &&
        live.state === "ready" &&
        !live.lastError
      ) {
        sessionId = sendTargetId;
      } else if (
        fromQueue &&
        sendTargetId &&
        viewingSessionIdRef.current !== sendTargetId
      ) {
        failStrip();
        return false;
      } else {
        sessionId = await ensureConnected({ sessionId: sendTargetId });
      }
      if (!sessionId) {
        failStrip();
        return false;
      }
      if (fromQueue && sendTargetId && sessionId !== sendTargetId) {
        failStrip();
        return false;
      }
      // Bind draft message cache to the real id early (Host already materialized).
      // Queue migrate waits until sessionSend succeeds so a failed flush can
      // requeue under the original claim key (`__draft__`) without splitting.
      if (!sendTargetId) {
        const draftMsgs = transcriptStore.getCached("__draft__");
        if (draftMsgs?.length) {
          if (
            transcriptStore.getViewingId() == null ||
            transcriptStore.getViewingId() === DRAFT_SESSION_KEY
          ) {
            transcriptStore.adoptViewing(null, sessionId);
          } else {
            transcriptStore.write(sessionId, draftMsgs);
            transcriptStore.evict("__draft__");
          }
        } else if (viewingTarget()) {
          transcriptStore.rebindViewing(sessionId);
        }
      }
      if (automationSetupDraftRef.current || inAutomationSetup) {
        automationSetupSessionsRef.current.add(sessionId);
        automationSetupDraftRef.current = false;
      }
      if (
        fromQueue &&
        sendTargetId &&
        liveHostRef.current.sessionId &&
        liveHostRef.current.sessionId !== sendTargetId
      ) {
        failStrip();
        return false;
      }
      const storedAttachments = att.map(({ path, name, isDir }) => ({
        path,
        name,
        isDir,
      }));
      if (
        opts.memoryContextPack ||
        skillSelections.length > 0 ||
        connectorSelections.length > 0
      ) {
        await api.sessionSendV2({
          sessionId,
          text: agentText,
          displayText: journalDisplay,
          attachments: storedAttachments,
          memoryContextPack: opts.memoryContextPack
            ? {
                version: 1,
                selections: opts.memoryContextPack.items.map((item) => ({
                id: item.candidateId,
                expectedContentHash: item.contentHash,
                })),
              }
            : null,
          skillSelections,
          connectorSelections,
        });
      } else {
        await api.sessionSend(agentText, journalDisplay, storedAttachments);
      }
      if (shouldAutoTitle && api.isTauri()) {
        void api
          .sessionAutoTitle(sessionId, titleSeed)
          .then((meta) => {
            if (meta?.title) applySessionTitle(sessionId, meta.title);
          })
          .catch(() => {
            /* ignore */
          });
      }
      return true;
    } catch (e) {
      failStrip();
      if (viewingTarget()) setLocalError(String(e));
      return false;
    } finally {
      sendInFlightRef.current = false;
    }
  };

  const generateSkillDraft = (
    request: SkillGenerationRequest,
  ): Promise<SkillDraft> => {
    setSkillCandidate(null);
    if (pendingSkillGenerationRef.current) {
      return Promise.reject(
        new Error(tr("skillRecorder.generationAlreadyRunning")),
      );
    }
    if (isSessionBusy(session.state) || connecting || perm || askUser) {
      return Promise.reject(new Error(tr("skillRecorder.waitForTurn")));
    }

    return new Promise<SkillDraft>((resolve, reject) => {
      const pending = {
        startIndex: messagesRef.current.length,
        sessionId: session.sessionId,
        resolve,
        reject,
      };
      pendingSkillGenerationRef.current = pending;
      void executeSend({
        storedDisplay: request.visibleRequest,
        agentTextOverride: request.prompt,
        att: [],
        goalMode: false,
        targetSessionId: session.sessionId,
      }).then((sent) => {
        if (pendingSkillGenerationRef.current !== pending) return;
        if (!sent) {
          pendingSkillGenerationRef.current = null;
          reject(new Error(trRef.current("skillRecorder.generateFailed")));
          return;
        }
        pending.sessionId = viewingSessionIdRef.current;
      });
    });
  };

  useEffect(() => {
    if (!api.isTauri()) {
      skillCandidatesReadyRef.current = true;
      return;
    }
    let cancelled = false;
    void api
      .skillCandidatesListV1()
      .then((candidates) => {
        if (cancelled) return;
        rememberSkillCandidateIds(seenSkillCandidateIdsRef.current, candidates);
        setSkillCandidate(skillCandidateAtBootstrap(candidates));
        skillCandidatesReadyRef.current = true;
      })
      .catch(() => {
        skillCandidatesReadyRef.current = true;
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (
      !skillCandidatesReadyRef.current ||
      !api.isTauri() ||
      session.state !== "ready" ||
      !session.sessionId
    ) {
      return;
    }
    let cancelled = false;
    void api
      .skillCandidatesListV1()
      .then((candidates) => {
        if (cancelled) return;
        const candidate = unseenSkillCandidateForSession(
          candidates,
          session.sessionId,
          seenSkillCandidateIdsRef.current,
        );
        rememberSkillCandidateIds(seenSkillCandidateIdsRef.current, candidates);
        if (!candidate) return;
        setSkillCandidate(candidate);
        setSkillRecorderOpen(true);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [session.sessionId, session.state]);

  useEffect(() => {
    const pending = pendingSkillGenerationRef.current;
    if (!pending) return;
    if (
      connecting ||
      session.state === "streaming" ||
      session.state === "awaiting_permission" ||
      session.state === "connecting"
    ) {
      return;
    }
    if (
      pending.sessionId &&
      session.sessionId &&
      pending.sessionId !== session.sessionId
    ) {
      pendingSkillGenerationRef.current = null;
      pending.reject(new Error(trRef.current("skillRecorder.taskChanged")));
      return;
    }

    const replies = transcriptStore
      .getViewed()
      .slice(pending.startIndex)
      .filter(
        (message) =>
          message.role === "assistant" &&
          !message.streaming &&
          Boolean(message.content?.trim()),
      );
    const latest = replies.at(-1);
    if (!latest) {
      if (session.state === "disconnected" || session.lastError) {
        pendingSkillGenerationRef.current = null;
        pending.reject(
          new Error(
            session.lastError?.message ||
              trRef.current("skillRecorder.generateFailed"),
          ),
        );
      }
      return;
    }

    try {
      const draft = parseSkillDraft(latest.content);
      pendingSkillGenerationRef.current = null;
      pending.resolve(draft);
    } catch (error) {
      pendingSkillGenerationRef.current = null;
      pending.reject(
        error instanceof Error
          ? error
          : new Error(trRef.current("skillRecorder.generateFailed")),
      );
    }
  }, [
    connecting,
    transcriptMeta.streaming,
    transcriptMeta.length,
    session.lastError,
    session.sessionId,
    session.state,
  ]);

  const clearComposerAfterSubmit = () => {
    composerRecoveryActionsRef.current?.captureComposer("", []);
    setDraft("");
    setSlashQuery(null);
    setAttachments([]);
    requestAnimationFrame(() => {
      const el = document.querySelector<HTMLElement>(".composer__input");
      if (el) el.style.height = "auto";
    });
  };

  /** Enqueue when agent is busy; otherwise send immediately. */
  const send = async () => {
    const segments = parseStoredContent(draft);
    const storedDisplay = draft;
    const att = attachments;
    if (isDraftEmpty(segments) && !att.length) return;
    if (session.state === "awaiting_permission") {
      showToast(tr("composer.queueBlockedPermission"), 2800);
      return;
    }
    // #52: orphan chats without a folder often stop after planning text —
    // tools can't land in a workspace until a project is bound.
    if (
      !activeProject &&
      (mode === "agent" || goalMode) &&
      !shouldEnqueueSend(session.state, connecting)
    ) {
      showColumnNotice(tr("composer.noProjectWriteHint"), 4500);
    }
    sendQueue.releaseFlushHold();

    if (shouldEnqueueSend(session.state, connecting)) {
      if (pendingMemoryContext) {
        showToast(tr("composer.memory.queueBlocked"), 4200);
        return;
      }
      sendQueue.enqueue({
        storedDisplay,
        attachments: att,
        goalMode,
      });
      clearComposerAfterSubmit();
      return;
    }

    // Validate before clearing the visible composer. A stale restored path must
    // leave the draft intact so the user can remove/replace the attachment.
    if (sendInFlightRef.current) return;
    sendInFlightRef.current = true;
    const classifiedAttachments = await classifyAttachmentsForSend(att);
    sendInFlightRef.current = false;
    if (!classifiedAttachments) return;
    clearComposerAfterSubmit();
    const memoryContext = pendingMemoryContext;
    const sent = await executeSend({
      storedDisplay,
      att: classifiedAttachments,
      goalMode,
      attachmentsClassified: true,
      memoryContextPack: memoryContext ?? undefined,
      targetSessionId: session.sessionId,
    });
    if (!sent) {
      setDraft((current) => (current ? current : storedDisplay));
      setAttachments((current) =>
        current.length > 0
          ? current
          : classifiedAttachments.map((attachment) => ({ ...attachment })),
      );
    }
    if (sent && memoryContext) {
      memoryPackRestoreGenRef.current += 1;
      setPendingMemoryContext((current) =>
        current === memoryContext ? null : current,
      );
    }
  };

  executeSendFromQueueRef.current = (opts) => executeSend(opts);

  const queuePreviewLabels = useMemo(
    () => ({
      filesCount: (n: number) =>
        tr("composer.queueFilesCount", { n: String(n) }),
      empty: tr("composer.queueEmptyPreview"),
    }),
    [tr],
  );

  const addAttachmentsFromPaths = useCallback(

    async (paths: string[]) => {
      if (!paths.length) {
        setLocalError(tr("attach.droppedNone"));
        return;
      }
      // While inline-editing a sent message, drops target the edit form — not the composer.
      const intoEdit = !!editingUserMessageIdRef.current;
      const mergeInto = intoEdit ? setEditAttachments : setAttachments;
      try {
        if (!api.isTauri()) {
          mergeInto((prev) =>
            mergeAttachments(
              prev,
              paths.map((p) => ({
                path: p,
                name: p.split(/[/\\]/).pop() || p,
                isDir: false,
              })),
            ),
          );
          return;
        }
        const classified = await api.pathsClassify(paths);
        // Accept all formats (images, docs, …). Keep entries even if exists is false
        // so transient sandbox / iCloud paths still show; open may fail later.
        const next = classified.map((c) => ({
          path: c.path,
          name: c.name,
          isDir: c.isDir,
        }));
        if (!next.length) {
          setLocalError(tr("attach.droppedNone"));
          return;
        }
        mergeInto((prev) => mergeAttachments(prev, next));
        setLocalError(null);
      } catch (e) {
        setLocalError(String(e));
      }
    },
    [tr],
  );

  /** Web File list (paste / HTML5 drop) → absolute paths for agent `@path`. */
  const addAttachmentsFromFiles = useCallback(
    async (files: File[]) => {
      if (!files.length) return;
      const withPath: string[] = [];
      const withoutPath: File[] = [];
      for (const f of files) {
        const anyF = f as File & { path?: string };
        if (anyF.path) withPath.push(anyF.path);
        else withoutPath.push(f);
      }
      if (withPath.length) {
        await addAttachmentsFromPaths(withPath);
      }
      if (!withoutPath.length) return;
      if (!api.isTauri()) {
        setLocalError(tr("composer.attachPasteFailed"));
        return;
      }
      const intoEdit = !!editingUserMessageIdRef.current;
      const mergeInto = intoEdit ? setEditAttachments : setAttachments;
      try {
        let lastName = "";
        for (const f of withoutPath) {
          const buf = await f.arrayBuffer();
          const bytes = new Uint8Array(buf);
          // Chunked base64 to avoid call-stack limits on large pastes
          let binary = "";
          const chunk = 0x8000;
          for (let i = 0; i < bytes.length; i += chunk) {
            binary += String.fromCharCode(
              ...bytes.subarray(i, Math.min(i + chunk, bytes.length)),
            );
          }
          const b64 = btoa(binary);
          const name =
            f.name && f.name !== "image.png" && f.name !== "blob"
              ? f.name
              : f.type?.startsWith("image/")
                ? `paste.${(f.type.split("/")[1] || "png").replace("jpeg", "jpg")}`
                : f.name || "paste.bin";
          const entry = await api.saveTempAttachment(b64, name, f.type || null);
          lastName = entry.name;
          mergeInto((prev) =>
            mergeAttachments(prev, [
              {
                path: entry.path,
                name: entry.name,
                isDir: entry.isDir,
              },
            ]),
          );
        }
        setLocalError(null);
        if (lastName) {
          const msg = tr("composer.attachSaved", { name: lastName });
          setToast(msg);
          window.setTimeout(
            () => setToast((cur) => (cur === msg ? null : cur)),
            2200,
          );
        }
      } catch (e) {
        setLocalError(String(e) || tr("composer.attachPasteFailed"));
      }
    },
    [addAttachmentsFromPaths, tr],
  );

  /**
   * Native OS clipboard image (arboard) when WebView paste has no File objects.
   * Used for macOS screenshots / system image clipboard.
   */
  const pasteMediaFromNativeClipboard = useCallback(
    async (opts?: { expectMedia?: boolean }) => {
      if (!api.isTauri()) {
        if (opts?.expectMedia) {
          setLocalError(tr("composer.attachPasteFailed"));
        }
        return;
      }
      try {
        const entry = await api.clipboardPasteImage();
        if (!entry?.path) {
          if (opts?.expectMedia) {
            setLocalError(tr("composer.attachPasteFailed"));
          }
          return;
        }
        await addAttachmentsFromPaths([entry.path]);
        setLocalError(null);
        const msg = tr("composer.attachSaved", { name: entry.name });
        setToast(msg);
        window.setTimeout(
          () => setToast((cur) => (cur === msg ? null : cur)),
          2200,
        );
      } catch (e) {
        setLocalError(String(e) || tr("composer.attachPasteFailed"));
      }
    },
    [addAttachmentsFromPaths, tr],
  );

  const closeComposerMenu = useCallback(() => {
    const live = liveSlashRef.current;
    if (live.present) {
      slashDismissedSigRef.current = `${live.start}:${live.query}`;
    }
    const at = liveAtRef.current;
    if (at.present) {
      atDismissedSigRef.current = `${at.start}:${at.query}`;
    }
    setShowComposerPlus(false);
    setSlashQuery(null);
    const cleared = { present: false, query: "", start: 0, end: 0 };
    setLiveSlash(cleared);
    liveSlashRef.current = cleared;
    setLiveAt(cleared);
    liveAtRef.current = cleared;
  }, []);

  /**
   * Event-driven slash state from ComposerEditor. Escape suppresses the current
   * token until it changes; no idle DOM polling is needed.
   */
  const onSlashQueryChange = useCallback(
    (q: { start: number; query: string; end: number } | null) => {
      let next = q
        ? { present: true, ...q }
        : { present: false, query: "", start: 0, end: 0 };

      if (next.present && slashDismissedSigRef.current != null) {
        const signature = `${next.start}:${next.query}`;
        if (signature === slashDismissedSigRef.current) {
          next = { present: false, query: "", start: 0, end: 0 };
        } else {
          slashDismissedSigRef.current = null;
        }
      } else if (!next.present) {
        slashDismissedSigRef.current = null;
      }

      const previous = liveSlashRef.current;
      if (
        previous.present === next.present &&
        previous.query === next.query &&
        previous.start === next.start &&
        previous.end === next.end
      ) {
        return;
      }

      liveSlashRef.current = next;
      setLiveSlash(next);
      setSlashQuery(next.present ? q : null);
      if (next.present) {
        const cleared = { present: false, query: "", start: 0, end: 0 };
        liveAtRef.current = cleared;
        setLiveAt(cleared);
      }
    },
    [],
  );

  const onAtQueryChange = useCallback(
    (q: { start: number; query: string; end: number } | null) => {
      if (liveSlashRef.current.present) {
        const cleared = { present: false, query: "", start: 0, end: 0 };
        liveAtRef.current = cleared;
        setLiveAt(cleared);
        return;
      }
      let next = q
        ? { present: true, ...q }
        : { present: false, query: "", start: 0, end: 0 };
      if (next.present && atDismissedSigRef.current != null) {
        const signature = `${next.start}:${next.query}`;
        if (signature === atDismissedSigRef.current) {
          next = { present: false, query: "", start: 0, end: 0 };
        } else {
          atDismissedSigRef.current = null;
        }
      } else if (!next.present) {
        atDismissedSigRef.current = null;
      }
      const previous = liveAtRef.current;
      if (
        previous.present === next.present &&
        previous.query === next.query &&
        previous.start === next.start &&
        previous.end === next.end
      ) {
        return;
      }
      liveAtRef.current = next;
      setLiveAt(next);
    },
    [],
  );

  const pickComposerFiles = useCallback(async () => {
    closeComposerMenu();
    if (!api.isTauri()) {
      setLocalError(tr("composer.attachPasteFailed"));
      return;
    }
    try {
      const paths = await api.pickAttachFiles();
      if (!paths.length) {
        // Cancelled — no error.
        return;
      }
      await addAttachmentsFromPaths(paths);
      setLocalError(null);
      const label =
        paths.length === 1
          ? paths[0]!.split(/[/\\]/).pop() || paths[0]!
          : tr("composer.attachCount", { n: String(paths.length) });
      const msg =
        paths.length === 1
          ? tr("composer.attachSaved", { name: label })
          : tr("composer.attachSaved", { name: label });
      setToast(msg);
      window.setTimeout(
        () => setToast((cur) => (cur === msg ? null : cur)),
        2200,
      );
    } catch (e) {
      setLocalError(String(e) || tr("composer.attachPasteFailed"));
    }
  }, [addAttachmentsFromPaths, closeComposerMenu, tr]);

  const pickComposerFolder = useCallback(async () => {
    closeComposerMenu();
    if (!api.isTauri()) {
      setLocalError(tr("composer.attachPasteFailed"));
      return;
    }
    try {
      const path = await api.pickAttachFolder();
      if (!path) return;
      await addAttachmentsFromPaths([path]);
      setLocalError(null);
      const name = path.split(/[/\\]/).pop() || path;
      const msg = tr("composer.attachSaved", { name });
      setToast(msg);
      window.setTimeout(
        () => setToast((cur) => (cur === msg ? null : cur)),
        2200,
      );
    } catch (error) {
      setLocalError(String(error) || tr("composer.attachPasteFailed"));
    }
  }, [addAttachmentsFromPaths, closeComposerMenu, tr]);

  const addProjectsFromPaths = useCallback(
    async (paths: string[]) => {
      if (!paths.length || !api.isTauri()) return;
      try {
        const classified = await api.pathsClassify(paths);
        const dirs = classified.filter((c) => c.exists && c.isDir);
        if (!dirs.length) {
          setLocalError(tr("composer.dropProjectFilesOnly"));
          return;
        }
        let last: Project | null = null;
        for (const d of dirs) {
          last = (await api.projectAdd(d.path, false)) as Project;
        }
        const list = (await api.projectsList()) as Project[];
        setProjects(list);
        if (last) {
          setActiveProject(list.find((p) => p.id === last!.id) ?? last);
          setExpandedProjects((e) => ({ ...e, [last!.id]: true }));
          setLocalError(null);
          setToast(tr("composer.projectAdded", { name: last.name }));
          window.setTimeout(() => setToast(null), 2500);
        }
      } catch (e) {
        setLocalError(String(e));
      }
    },
    [tr],
  );

  /**
   * Hit-test CSS client point against the live sidebar box.
   * Only the real left rail is "sidebar" (add project); rest of workbench is attach.
   */
  const hitDragZone = useCallback(
    (clientX: number, clientY: number): "sidebar" | "main" => {
      const collapsed = layoutRef.current.sidebarCollapsed;
      if (collapsed) return "main";
      const el = querySidebarEl();
      if (!el) return "main";
      return hitDragZoneFromRects(
        clientX,
        clientY,
        el.getBoundingClientRect(),
        false,
      );
    },
    [],
  );

  // Tauri OS file drag-drop (full absolute paths)
  useEffect(() => {
    if (!api.isTauri()) return;
    let cancelled = false;
    let unlisten: (() => void) | undefined;

    void (async () => {
      try {
        const { getCurrentWebview } = await import("@tauri-apps/api/webview");
        const { getCurrentWindow } = await import("@tauri-apps/api/window");
        const webview = getCurrentWebview();
        const win = getCurrentWindow();
        const factor = await win.scaleFactor();

        unlisten = await webview.onDragDropEvent((event) => {
          if (cancelled) return;
          const payload = event.payload;
          if (payload.type === "enter" || payload.type === "drop") {
            if ("paths" in payload && payload.paths?.length) {
              dragPathsRef.current = payload.paths;
            }
          }
          if (payload.type === "leave") {
            setDragZone(null);
            dragPathsRef.current = [];
            return;
          }
          if (payload.type === "enter" || payload.type === "over") {
            // macOS: coords are already view points; win: physical → / factor
            const { x, y } = toClientDragPoint(
              payload.position,
              factor,
              platform,
            );
            setDragZone(hitDragZone(x, y));
            return;
          }
          if (payload.type === "drop") {
            const { x, y } = toClientDragPoint(
              payload.position,
              factor,
              platform,
            );
            const zone = hitDragZone(x, y);
            const paths = payload.paths?.length
              ? payload.paths
              : dragPathsRef.current;
            setDragZone(null);
            dragPathsRef.current = [];
            if (!paths.length) {
              setLocalError(tr("attach.droppedNone"));
              return;
            }
            if (zone === "sidebar") {
              void addProjectsFromPaths(paths);
            } else {
              // All file types (images, pdf, …) attach in main zone
              void addAttachmentsFromPaths(paths);
            }
          }
        });
      } catch {
        /* webview API unavailable */
      }
    })();

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [
    addAttachmentsFromPaths,
    addProjectsFromPaths,
    hitDragZone,
    platform,
    tr,
  ]);

  // HTML5 fallback: some image drags only expose File list in the webview.
  // Prefer Tauri paths; use File.path when present (Tauri webview).
  useEffect(() => {
    const onDragOver = (e: DragEvent) => {
      if (!e.dataTransfer?.types?.includes("Files")) return;
      e.preventDefault();
      e.dataTransfer.dropEffect = "copy";
    };
    const onDrop = (e: DragEvent) => {
      if (!e.dataTransfer?.files?.length) return;
      // If Tauri already handled this OS drop, paths may be empty here.
      const files = Array.from(e.dataTransfer.files);
      const paths = files
        .map((f) => {
          const anyF = f as File & { path?: string };
          return anyF.path || "";
        })
        .filter(Boolean);
      const zone = hitDragZone(e.clientX, e.clientY);
      if (paths.length) {
        e.preventDefault();
        e.stopPropagation();
        if (zone === "sidebar") void addProjectsFromPaths(paths);
        else void addAttachmentsFromPaths(paths);
        return;
      }
      // Browser-only / path-less File list (e.g. image from another app)
      if (zone !== "sidebar" && files.length) {
        e.preventDefault();
        e.stopPropagation();
        void addAttachmentsFromFiles(files);
      }
    };
    window.addEventListener("dragover", onDragOver);
    window.addEventListener("drop", onDrop);
    return () => {
      window.removeEventListener("dragover", onDragOver);
      window.removeEventListener("drop", onDrop);
    };
  }, [
    addAttachmentsFromFiles,
    addAttachmentsFromPaths,
    addProjectsFromPaths,
    hitDragZone,
  ]);

  useEffect(() => {
    if (!api.isTauri()) return;
    void api
      .connectorsList()
      .then(setConnectorStates)
      .catch(() => undefined);
  }, []);

  // Drag-resize right resource pane
  useEffect(() => {
    if (!resizingAside) return;
    const onMove = (e: PointerEvent) => {
      const next = clampAsideWidth(window.innerWidth - e.clientX);
      setLayout((l) => {
        const n = { ...l, asideWidth: next, asideCollapsed: false };
        return n;
      });
    };
    const onUp = () => {
      setResizingAside(false);
      setLayout((l) => {
        saveLayout(localStorage, l);
        return l;
      });
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
    };
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    return () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
    };
  }, [resizingAside]);

  const resizeComposer = (el: HTMLElement) => {
    const line = 22; // ~line-height
    const min = line * 1;
    const max = line * 10;
    el.style.height = "auto";
    el.style.height = `${Math.min(Math.max(el.scrollHeight, min), max)}px`;
  };

  /** Programmatic draft / layout changes: recompute height after paint. */
  const syncComposerHeight = useCallback(() => {
    // Double rAF: wait for React commit + layout after mainPane switch.
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        const node = composerInputRef.current;
        if (node) resizeComposer(node);
      });
    });
  }, []);

  /** Bumped when Extensions skill toggles change so slash palette refilters. */
  const [skillsReloadToken, setSkillsReloadToken] = useState(0);

  // Load the Host-verified metadata inventory for slash selection and the
  // later send-time tree-hash comparison. The Host re-runs inspect before use.
  useEffect(() => {
    if (!api.isTauri()) return;
    let cancelled = false;
    setSkillsLoading(true);
    void api
      .skillInventoryV1(activeProject?.path ?? null)
      .then((res) => {
        if (cancelled) return;
        setSkillInfos(
          (res.items ?? [])
            .filter((s) => s.enabled && s.userInvocable)
            .map((s) => ({
              id: s.id,
              name: s.name,
              description: s.description ?? "",
              whenToUse: s.whenToUse,
              source: s.source,
              treeHash: s.treeHash,
              sourceCandidateId: s.sourceCandidateId,
              userInvocable: s.userInvocable,
            })),
        );
      })
      .catch(() => {
        if (!cancelled) setSkillInfos([]);
      })
      .finally(() => {
        if (!cancelled) setSkillsLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [activeProject?.path, skillsReloadToken]);

  useEffect(() => {
    if (!api.isTauri() || !activeProject?.trusted || !activeProject.path) {
      setProjectInstruction(null);
      return;
    }
    let cancelled = false;
    void api
      .projectInstructionInspectV1(activeProject.path)
      .then((inspect) => {
        if (!cancelled) setProjectInstruction(inspect);
      })
      .catch(() => {
        if (!cancelled) setProjectInstruction(null);
      });
    return () => {
      cancelled = true;
    };
  }, [activeProject?.path, activeProject?.trusted]);

  const rankedSkillInfos = useMemo(() => {
    const annotated = skillInfos.map((skill, index) => ({
      ...skill,
      suggested: Boolean(skill.id && rankedSkillIds.has(skill.id)),
      originalIndex: index,
    }));
    annotated.sort(
      (left, right) =>
        Number(right.suggested) - Number(left.suggested) ||
        left.originalIndex - right.originalIndex,
    );
    return annotated.map(({ originalIndex: _originalIndex, ...skill }) => skill);
  }, [rankedSkillIds, skillInfos]);
  const slashCatalog = useMemo(
    () => buildSlashCatalog(rankedSkillInfos),
    [rankedSkillInfos],
  );
  const resolveSlashTitle = useCallback(
    (item: SlashItem) => {
      if (item.titleKey) {
        try {
          return tr(item.titleKey as MessageKey);
        } catch {
          /* fall through */
        }
      }
      return item.displayTitle || item.name;
    },
    [tr],
  );
  const resolveSlashDescription = useCallback(
    (item: SlashItem) => {
      if (item.descriptionKey) {
        try {
          return tr(item.descriptionKey as MessageKey);
        } catch {
          /* fall through */
        }
      }
      return item.displayDescription || "";
    },
    [tr],
  );
  /** Filter query from live editor poll only. */
  const slashFilterQuery = liveSlash.present ? liveSlash.query : "";
  const atFilterQuery = liveAt.present ? liveAt.query : "";
  const plusMenuMode = showComposerPlus && !liveSlash.present && !liveAt.present;
  useEffect(() => {
    if (!plusMenuMode || !api.isTauri()) {
      setRankedSkillIds(new Set());
      return;
    }
    const query = plainTextOf(parseStoredContent(draft)).trim();
    if (query.length < 3 || skillInfos.length === 0) {
      setRankedSkillIds(new Set());
      return;
    }
    let cancelled = false;
    const timer = window.setTimeout(() => {
      void api
        .skillMetadataRankV1(query, activeProject?.path ?? null, 4)
        .then((result) => {
          if (!cancelled) {
            setRankedSkillIds(
              new Set(result.items.map((item) => item.skill.id)),
            );
          }
        })
        .catch(() => {
          if (!cancelled) setRankedSkillIds(new Set());
        });
    }, 250);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [activeProject?.path, draft, plusMenuMode, skillInfos.length]);
  const composerSettingsLocked = shouldLockComposerSettings({
    state: session.state,
    hasPermissionPrompt: !!perm,
    hasAskUserPrompt: !!askUser,
    hasPlanReview: plan.rpcId != null,
  });
  const canRecordSkill =
    transcriptMeta.hasUser && transcriptMeta.hasAssistant;
  const finderSelectionAvailable = api.capabilityAvailable(
    hostCapabilities,
    "finderSelection",
    hostCapabilities.finderSelection,
  );
  const skillDraftSaveAvailable = api.capabilityAvailable(
    hostCapabilities,
    "skillDraftSave",
    hostCapabilities.skillDraftSave,
  );

  const composerPlusActions = useMemo<
    Extract<ComposerPlusEntry, { kind: "action" }>[]
  >(
    () => [
      {
        id: "action:folder",
        kind: "action" as const,
        action: "folder" as const,
        title: tr("composer.addFolder"),
        description: tr("composer.addFolderHint"),
      },
      ...(finderSelectionAvailable
        ? [
            {
              id: "action:finder",
              kind: "action" as const,
              action: "finder" as const,
              title: tr("composer.finderSelection"),
              description:
                finderSelectionFeedback ??
                tr("composer.finderSelectionHint"),
              feedback: finderSelectionFeedback != null,
            },
          ]
        : []),
      {
        id: "action:project",
        kind: "action",
        action: "project",
        title: tr("composer.projectAction"),
        description: tr("composer.projectActionHint"),
        disabled: composerSettingsLocked,
      },
      {
        id: "action:goal",
        kind: "action",
        action: "goal",
        title: tr("composer.goal"),
        description: tr("composer.goalHint"),
        disabled: composerSettingsLocked,
      },
      {
        id: "action:plan",
        kind: "action",
        action: "plan",
        title: tr("composer.planMode"),
        description: tr("composer.planModeHint"),
        disabled: composerSettingsLocked,
      },
      {
        id: "action:ask",
        kind: "action",
        action: "ask",
        title: tr("composer.askMode"),
        description: tr("composer.askModeHint"),
        disabled: composerSettingsLocked,
      },
      {
        id: "action:record-skill",
        kind: "action",
        action: "record-skill",
        title: tr("composer.recordSkill"),
        description: canRecordSkill
          ? tr("composer.recordSkillHint")
          : tr("composer.recordSkillUnavailable"),
        disabled:
          !canRecordSkill ||
          !skillDraftSaveAvailable ||
          composerSettingsLocked,
      },
    ],
    [
      canRecordSkill,
      composerSettingsLocked,
      finderSelectionFeedback,
      finderSelectionAvailable,
      skillDraftSaveAvailable,
      tr,
    ],
  );

  /** `/` keeps the command palette; + uses the product-level Add/Plugins IA. */
  const slashFiltered = useMemo(
    () =>
      flattenFilteredCatalog(slashCatalog, slashFilterQuery, (item) => ({
        title: resolveSlashTitle(item),
        description: resolveSlashDescription(item),
      })),
    [
      slashCatalog,
      slashFilterQuery,
      resolveSlashTitle,
      resolveSlashDescription,
    ],
  );
  const showUploadInMenu = useMemo(
    () =>
      uploadMatchesQuery(slashFilterQuery, {
        title: tr("composer.addFiles"),
        hint: tr("composer.addFilesHint"),
      }),
    [slashFilterQuery, tr],
  );
  const composerMenuEntries = useMemo(
    () => {
      if (liveAt.present) {
        const q = liveAt.query.trim().toLowerCase();
        return CONNECTOR_CATALOG.filter((entry) => {
          if (!q) return true;
          const name = tr(entry.nameKey as MessageKey).toLowerCase();
          return entry.id.includes(q) || name.includes(q);
        }).map((entry) => {
          const connected = !!connectorStates.find((row) => row.id === entry.id)
            ?.connected;
          return {
            id: `connector:${entry.id}`,
            kind: "connector" as const,
            connectorId: entry.id,
            title: tr(entry.nameKey as MessageKey),
            description: connected
              ? tr("plugin.connectedHint", { id: entry.id })
              : tr("plugin.connectFirst"),
            connected,
          };
        });
      }
      if (plusMenuMode) {
        return buildComposerPlusEntries({
          showUpload: true,
          actions: composerPlusActions,
          commands: [],
          skills: slashCatalog.skills,
          plusMenu: true,
        });
      }
      return buildComposerPlusEntries({
        showUpload: showUploadInMenu,
        commands: slashFiltered.commands,
        skills: slashFiltered.skills,
      });
    },
    [
      composerPlusActions,
      connectorStates,
      liveAt.present,
      liveAt.query,
      plusMenuMode,
      showUploadInMenu,
      slashCatalog.skills,
      slashFiltered.commands,
      slashFiltered.skills,
      tr,
    ],
  );
  const composerMenuEntriesRef = useRef(composerMenuEntries);
  composerMenuEntriesRef.current = composerMenuEntries;

  /** + button and `/` open the same panel. */
  const composerMenuOpen = showComposerPlus || liveSlash.present || liveAt.present;

  /** Pin above input card; width matches composer shell.
   * Re-anchor when filter results change height (short list must sit on input). */
  const { pos: composerPlusPos, style: composerPlusStyle } = useFloatingMenu({
    open: composerMenuOpen,
    triggerRef: composerShellRef,
    panelRef: composerPlusPanelRef,
    roots: [composerPlusTriggerRef, composerShellRef, composerInputRef],
    onClose: closeComposerMenu,
    placement: "up",
    fitContent: false,
    matchTriggerWidth: true,
    minWidth: 280,
    estHeight: 220,
    gap: 8,
    deps: [slashFilterQuery, atFilterQuery, composerMenuEntries.length],
  });

  // Reset highlight only when the filter *string* changes.
  const prevFilterQueryRef = useRef(slashFilterQuery);
  useEffect(() => {
    if (prevFilterQueryRef.current === slashFilterQuery) return;
    prevFilterQueryRef.current = slashFilterQuery;
    setSlashActiveIndex(0);
  }, [slashFilterQuery]);

  // Keep highlight in range when the filtered list shrinks (no forced 0).
  useEffect(() => {
    setSlashActiveIndex((i) => {
      if (composerMenuEntries.length === 0) return 0;
      return i >= composerMenuEntries.length
        ? composerMenuEntries.length - 1
        : i;
    });
  }, [composerMenuEntries.length]);

  const showToast = useCallback((msg: string, ms = 3200) => {
    setToast(msg);
    window.setTimeout(() => {
      setToast((cur) => (cur === msg ? null : cur));
    }, ms);
  }, []);

  const showColumnNotice = useCallback((msg: string, ms = 4500) => {
    setColumnNotice(msg);
    window.setTimeout(() => {
      setColumnNotice((cur) => (cur === msg ? null : cur));
    }, ms);
  }, []);

  /** Open resource pane Plan review (replaces scroll-to-card “详情”). */
  const openPlanInResource = useCallback(() => {
    setLayout((l) => {
      if (!l.asideCollapsed) return l;
      const n = { ...l, asideCollapsed: false };
      saveLayout(localStorage, n);
      return n;
    });
    setPlanFocusKey((k) => k + 1);
  }, []);

  const sendQueueLabels = useMemo(
    () => ({
      queued: tr("composer.queued"),
      sendFailed: tr("composer.queueSendFailed"),
      droppedOldest: (n: number, max: number) =>
        tr("composer.queueDroppedOldest", {
          n: String(n),
          max: String(max),
        }),
    }),
    [tr],
  );
  const sendQueue = useSendQueue({
    queueKey: composerRecoveryKey,
    sessionState: session.state,
    connecting,
    liveHostRef,
    viewingSessionIdRef,
    sendInFlightRef,
    executeSendRef: executeSendFromQueueRef,
    persistQueuedState,
    showToast,
    labels: sendQueueLabels,
  });
  const hostedCommandJobs = useHostedCommandJobs(session.sessionId);
  const applyRecoveredMemoryPack = useCallback(
    (ref: ComposerMemoryPackRefV1 | null) => {
      const generation = ++memoryPackRestoreGenRef.current;
      if (!ref?.selections.length || !api.isTauri()) {
        setPendingMemoryContext(null);
        return;
      }
      void api
        .memoryContextPackBuildV1(ref.selections)
        .then((pack) => {
          if (memoryPackRestoreGenRef.current !== generation) return;
          setPendingMemoryContext(pack);
        })
        .catch((error) => {
          if (memoryPackRestoreGenRef.current !== generation) return;
          setPendingMemoryContext(null);
          showToast(tr("composer.memory.restoreFailed"), 4200);
          console.warn("memory pack restore:", error);
        });
    },
    [showToast, tr],
  );
  const composerRecovery = useComposerRecovery({
    enabled: api.isTauri(),
    recoveryKey: composerRecoveryKey,
    setRecoveryKey: setComposerRecoveryKey,
    draft,
    attachments,
    activeQueue: sendQueue.activeQueue,
    setDraft,
    setAttachments,
    memoryPack: memoryPackRefFromContext(pendingMemoryContext),
    setMemoryPack: applyRecoveredMemoryPack,
    queue: sendQueue,
    onError: (error) => console.warn("composer recovery:", error),
  });
  persistQueuedStateRef.current = composerRecovery.persistQueuedState;
  composerRecoveryActionsRef.current = composerRecovery;

  /**
   * Fork a session (full history or through a user-prompt index) and open it.
   */
  const runForkSession = useCallback(
    async (
      source: SessionRow,
      opts?: { throughUserPromptIndex?: number | null },
    ) => {
      if (!api.isTauri()) {
        showToast(tr("error.needTauri"));
        return;
      }
      try {
        const base = (source.title || tr("session.untitled")).trim();
        // Avoid double-prefix when forking a fork (any locale).
        const title = /^(fork of|分叉：|分叉:)\s*/i.test(base)
          ? base
          : tr("session.forkTitleOf", { name: base || "chat" });
        const meta = await api.sessionFork(source.id, {
          throughUserPromptIndex: opts?.throughUserPromptIndex ?? null,
          title,
        });
        await refreshSessions();
        const row: SessionRow = {
          id: meta.id,
          title: meta.title || title,
          projectId: meta.projectId ?? source.projectId,
          updatedAt: meta.updatedAt || new Date().toISOString(),
          archived: meta.archived,
          scheduled: meta.scheduled,
        };
        const proj = row.projectId
          ? projects.find((p) => p.id === row.projectId) ?? null
          : null;
        if (row.projectId) {
          setExpandedProjects((e) => ({ ...e, [row.projectId!]: true }));
        } else {
          setHistoryOpen(true);
        }
        await openSession(row, proj);
        showToast(tr("session.forkOk"), 2800);
      } catch (e) {
        showToast(tr("session.forkFailed") + ": " + String(e), 4500);
      }
    },
    // openSession / refreshSessions via closure
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [projects, showToast, tr],
  );

  const confirmForkSession = useCallback(
    (source: SessionRow, throughUserPromptIndex?: number | null) => {
      setCtxMenu(null);
      const partial =
        throughUserPromptIndex != null && throughUserPromptIndex !== undefined;
      setAppDialog({
        kind: "confirm",
        title: tr("session.forkTitle"),
        message: partial
          ? tr("session.forkConfirmPartial")
          : tr("session.forkConfirm"),
        confirmLabel: tr("session.fork"),
        onConfirm: () => {
          void runForkSession(source, {
            throughUserPromptIndex: throughUserPromptIndex ?? null,
          });
        },
      });
    },
    [runForkSession, tr],
  );

  /**
   * Apply rewind: truncate local journal (+ agent when live), refresh messages UI.
   */
  const runRewindToPrompt = useCallback(
    async (sessionId: string, targetPromptIndex: number) => {
      if (!api.isTauri()) {
        showToast(tr("error.needTauri"));
        return;
      }
      if (!canRewindSession) {
        showToast(tr("session.rewindBusy"));
        return;
      }
      setRewindBusy(true);
      try {
        // Prefer live connect so agent rewind can run; local truncate still works if not.
        if (
          (session.sessionId === sessionId ||
            viewingSessionIdRef.current === sessionId) &&
          session.state !== "ready"
        ) {
          try {
            await ensureConnected();
          } catch {
            /* local-only path */
          }
        }

        const result = await api.sessionRewindExecute(targetPromptIndex, {
          sessionId,
          restoreFiles: false,
        });

        // Refresh UI from truncated journal.
        if (viewingSessionIdRef.current === sessionId) {
          const stored = await api.sessionMessages(sessionId);
          const mapped: ChatMessage[] = stored.map((m) => ({
            id: m.id,
            role: m.role as "user" | "assistant" | "tool",
            content: m.content,
            thought: m.thought ?? undefined,
            thoughtPhases: splitThoughtPhases(m.thought),
            isError: m.isError || undefined,
            marker: m.marker || undefined,
            createdAt: m.createdAt || undefined,
            attachments: (m.attachments ?? []).map((a) => ({
              path: a.path,
              name: a.name || a.path.split(/[/\\]/).pop() || a.path,
              isDir: !!a.isDir,
            })),
            streaming: false,
          }));
          const kept = truncateThroughUserPrompt(mapped, targetPromptIndex);
          const finalMsgs =
            kept.length || mapped.length <= result.keptCount
              ? kept.length
                ? kept
                : mapped
              : mapped.slice(0, result.keptCount);
          transcriptStore.write(sessionId, finalMsgs);
          transcriptStore.replaceViewed(finalMsgs);
        } else {
          transcriptStore.evict(sessionId);
        }

        setRewindTimeline(null);
        if (result.agentOk) {
          showToast(tr("session.rewindOk"), 2600);
        } else {
          showToast(tr("session.rewindLocalOnly"), 4200);
        }
        await refreshSessions();
      } catch (e) {
        showToast(tr("session.rewindFailed") + ": " + String(e), 4500);
      } finally {
        setRewindBusy(false);
      }
    },
    // ensureConnected / refreshSessions via closure
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [canRewindSession, session.sessionId, session.state, showToast, tr],
  );

  const confirmRewindToPrompt = useCallback(
    (sessionId: string, targetPromptIndex: number, preview?: string) => {
      setCtxMenu(null);
      const msgPreview = preview?.trim()
        ? `\n\n“${preview.trim()}”`
        : "";
      setAppDialog({
        kind: "confirm",
        title: tr("session.rewindTitle"),
        message: tr("session.rewindConfirm") + msgPreview,
        confirmLabel: tr("session.rewindConfirmLabel"),
        danger: true,
        onConfirm: () => {
          void runRewindToPrompt(sessionId, targetPromptIndex);
        },
      });
    },
    [runRewindToPrompt, tr],
  );

  const openRewindTimeline = useCallback(
    async (sessionId: string) => {
      setCtxMenu(null);
      if (!api.isTauri()) {
        showToast(tr("error.needTauri"));
        return;
      }
      if (!canRewindSession) {
        showToast(tr("session.rewindBusy"));
        return;
      }
      try {
        let points = await api.sessionRewindPoints(sessionId);
        if (!points.length) {
          if (viewingSessionIdRef.current === sessionId) {
            points = localRewindPoints(messagesRef.current).map((p) => ({
              promptIndex: p.promptIndex,
              messageId: p.messageId,
              preview: p.preview,
            }));
          }
        }
        if (!points.length) {
          showToast(tr("session.rewindEmpty"));
          return;
        }
        setRewindTimeline({ sessionId, points });
      } catch (e) {
        if (viewingSessionIdRef.current === sessionId) {
          const points = localRewindPoints(messagesRef.current);
          if (points.length) {
            setRewindTimeline({
              sessionId,
              points: points.map((p) => ({
                promptIndex: p.promptIndex,
                messageId: p.messageId,
                preview: p.preview,
              })),
            });
            return;
          }
        }
        showToast(tr("session.rewindFailed") + ": " + String(e), 4500);
      }
    },
    [canRewindSession, showToast, tr],
  );

  const onRewindToUserMessage = useCallback(
    (msg: ChatMessage) => {
      const sid = session.sessionId ?? viewingSessionIdRef.current;
      if (!sid) {
        showToast(tr("session.rewindFailed"));
        return;
      }
      if (!canRewindSession) {
        showToast(tr("session.rewindBusy"));
        return;
      }
      const idx = userPromptIndexOf(transcriptStore.getViewed(), msg.id);
      if (idx < 0) return;
      if (!canRewindToUserPrompt(transcriptStore.getViewed(), idx)) {
        showToast(tr("session.rewindNoop"));
        return;
      }
      const preview = (msg.content || "")
        .replace(/\s+/g, " ")
        .trim()
        .slice(0, 80);
      confirmRewindToPrompt(sid, idx, preview);
    },
    [
      canRewindSession,
      confirmRewindToPrompt,
      session.sessionId,
      showToast,
      tr,
    ],
  );

  const onForkFromUserMessage = useCallback(
    (msg: ChatMessage) => {
      const sid = session.sessionId ?? viewingSessionIdRef.current;
      if (!sid) {
        showToast(tr("session.forkFailed"));
        return;
      }
      const row =
        sessions.find((s) => s.id === sid) ??
        ({
          id: sid,
          title: session.title || tr("session.untitled"),
          projectId: activeProject?.id ?? null,
          updatedAt: new Date().toISOString(),
        } satisfies SessionRow);
      const idx = userPromptIndexOf(transcriptStore.getViewed(), msg.id);
      if (idx < 0) return;
      confirmForkSession(row, idx);
    },
    [
      activeProject?.id,
      confirmForkSession,
      session.sessionId,
      session.title,
      sessions,
      showToast,
      tr,
    ],
  );

  /** Active inference channel: custom relay identity replaces official account chrome. */
  const [activeCustomProvider, setActiveCustomProvider] =
    useState<api.CustomProvider | null>(null);
  const customRouteActive = activeCustomProvider != null;
  const refreshProviderRoute = useCallback(async () => {
    if (!api.isTauri()) {
      setActiveCustomProvider(null);
      return;
    }
    try {
      const list = await api.providersList();
      const active =
        list.activeSource === "custom"
          ? list.providers.find((provider) => provider.id === list.activeProviderId) ?? null
          : null;
      setActiveCustomProvider(active);
    } catch {
      /* keep previous */
    }
  }, []);
  useEffect(() => {
    void refreshProviderRoute();
  }, [refreshProviderRoute]);

  /** Two-step in-app confirm for `always_approve` (YOLO). Never window.confirm in Tauri. */
  const confirmAlwaysApprove = useCallback(
    (onConfirmed: () => void) => {
      setAppDialog({
        kind: "confirm",
        title: tr("policy.always_approve"),
        message: tr("policy.yoloConfirm"),
        confirmLabel: tr("common.confirm"),
        danger: true,
        onConfirm: () => {
          setAppDialog({
            kind: "confirm",
            title: tr("policy.always_approve"),
            message: tr("policy.yoloConfirm2"),
            confirmLabel: tr("policy.short.always_approve"),
            danger: true,
            onConfirm: onConfirmed,
          });
        },
      });
    },
    [tr],
  );

  /**
   * Breaks a forward-reference cycle: `patchSettingsSafely` (defined later,
   * below `applyAuthoritativeSettingFields`) is needed by the catalog hook
   * here, but `applyAuthoritativeSettingFields` itself needs the catalog
   * hook's `setPrefsScopeRaw`. A stable ref indirection lets both sides
   * exist without moving the large, catalog-unrelated settings machinery.
   */
  const patchSettingsSafelyRef = useRef<
    (patch: Partial<api.AppSettings>) => Promise<unknown>
  >(async () => null);
  const patchSettingsSafelyForCatalog = useCallback(
    (patch: Partial<api.AppSettings>) => patchSettingsSafelyRef.current(patch),
    [],
  );

  const catalogPrefs = useComposerCatalog({
    activeProjectId: activeProject?.id ?? null,
    sessionId: session.sessionId ?? null,
    locked: composerSettingsLocked,
    showToast,
    tr,
    confirmAlwaysApprove,
    refreshProviderRoute,
    onModeApplied: (value) => {
      if (value === "plan") setGoalMode(false);
    },
    patchSettingsSafely: patchSettingsSafelyForCatalog,
  });
  const {
    modelId,
    effort,
    mode,
    policy,
    availableModels,
    prefsScope,
    onMode,
    onPolicy,
    onDisablePlan,
    onModel,
    onEffort,
    onReset,
    onPrefsScope,
    setModeRaw,
    setPrefsScopeRaw,
    applyBootstrap,
    reresolve,
  } = catalogPrefs;

  const applySlashItem = useCallback(
    (item: SlashItem) => {
      const live = liveSlashRef.current;
      const q =
        slashQuery ??
        (live.present
          ? { start: live.start, query: live.query, end: live.end }
          : null);
      setSlashQuery(null);
      setLiveSlash({ present: false, query: "", start: 0, end: 0 });
      liveSlashRef.current = { present: false, query: "", start: 0, end: 0 };
      setShowComposerPlus(false);

      if (item.kind === "skill") {
        const selected = skillInfos.find(
          (skill) => skill.name.toLowerCase() === item.name.toLowerCase(),
        );
        if (!selected?.id || !selected.treeHash) {
          setLocalError(`SKILL_USE_STALE: ${item.name}`);
          return;
        }
        const binding = {
          version: 1 as const,
          id: selected.id,
          expectedTreeHash: selected.treeHash,
          selection:
            plusMenuMode && rankedSkillIds.has(selected.id)
              ? ("accepted_suggestion" as const)
              : ("explicit" as const),
        };
        if (q) {
          setDraft((d) =>
            applySkillAtSlash(d, q.start, q.end, item.name, binding),
          );
        } else {
          setDraft((d) => {
            const needsSpace = d.length > 0 && !/\s$/.test(d);
            const base = `${d}${needsSpace ? " " : ""}`;
            return applySkillAtSlash(
              base,
              base.length,
              base.length,
              item.name,
              binding,
            );
          });
        }
        return;
      }

      // Remove the /query from draft for mode/action
      if (q) {
        setDraft((d) => d.slice(0, q.start) + d.slice(q.end));
      }

      if (item.kind === "mode") {
        if (composerSettingsLocked) return;
        if (item.mode === "goal") {
          setGoalMode(true);
          if (mode === "plan") setModeRaw("agent");
          return;
        }
        if (item.mode === "plan") {
          setGoalMode(false);
          setModeRaw("plan");
          void api
            .composerPrefsSet({
              projectId: activeProject?.id ?? null,
              sessionId: session.sessionId ?? null,
              mode: "plan",
            })
            .catch((e) => showToast(String(e), 4000));
          return;
        }
      }

      if (item.kind === "action") {
        switch (item.action) {
          case "doctor":
            openDoctor();
            return;
          case "status":
            openStatusModal();
            return;
          case "mcp":
            void openMcpModal(activeProject?.path ?? null);
            return;
          case "compact":
            if (composerSettingsLocked) return;
            setCompactNote("");
            setShowCompactModal(true);
            return;
          case "newChat":
            void newChat();
            return;
          case "automations":
            navigateAutomations();
            return;
          case "settings":
            navigateSettings("general");
            return;
          case "yolo": {
            if (composerSettingsLocked) return;
            const next: PermissionPolicyId =
              policy === "always_approve" ? "ask" : "always_approve";
            onPolicy(next, { toastYoloToggle: true });
            return;
          }
          default:
            return;
        }
      }
    },
    // many deps — intentionally broad for stable handlers used in render
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [
      slashQuery,
      mode,
      policy,
      activeProject?.id,
      activeProject?.path,
      session.sessionId,
      tr,
      openStatusModal,
      openMcpModal,
      onPolicy,
      setModeRaw,
      composerSettingsLocked,
      showToast,
      plusMenuMode,
      rankedSkillIds,
      skillInfos,
    ],
  );

  const applyConnectorMention = useCallback(
    (entry: Extract<ComposerPlusEntry, { kind: "connector" }>) => {
      if (!entry.connected) {
        setAppView("workbench");
        setMainPane("plugins");
        closeComposerMenu();
        return;
      }
      const live = liveAtRef.current;
      const q = live.present
        ? { start: live.start, end: live.end }
        : null;
      closeComposerMenu();
      if (q) {
        setDraft((d) =>
          applyConnectorAtMention(d, q.start, q.end, entry.connectorId),
        );
      } else {
        setDraft((d) => {
          const needsSpace = d.length > 0 && !/\s$/.test(d);
          const base = `${d}${needsSpace ? " " : ""}`;
          return applyConnectorAtMention(
            base,
            base.length,
            base.length,
            entry.connectorId,
          );
        });
      }
    },
    [closeComposerMenu],
  );

  // Seed draft / clear / pane switch: grow textarea. If a focus request is still
  // pending (e.g. textarea just remounted), retry focus here as a backstop.
  useEffect(() => {
    if (mainPane !== "chat") return;
    if (pendingComposerFocus.current) {
      requestComposerFocus();
      return;
    }
    syncComposerHeight();
  }, [draft, mainPane, session.sessionId, requestComposerFocus, syncComposerHeight]);

  /** Context usage chip label/state from exact Runtime or compact telemetry. */
  const contextUsageDisplay = useMemo(
    () =>
      resolveContextUsageDisplay(
        contextUsage,
        [],
        session.contextUsage ?? null,
      ),
    [contextUsage, session.contextUsage],
  );

  /**
   * New empty draft only: lift composer and Sunsetz Pro brand.
   * Existing sessions (even with empty journal) must not look like a fresh chat.
   */
  const welcomeSession =
    mainPane === "chat" &&
    !session.sessionId &&
    transcriptMeta.length === 0 &&
    session.state !== "streaming";
  const emptyExistingSession =
    mainPane === "chat" &&
    !!session.sessionId &&
    transcriptMeta.length === 0 &&
    session.state !== "streaming" &&
    session.state !== "connecting";
  const taskProgressVisible =
    plan.entries.length > 0 &&
    !plan.waiting &&
    plan.rpcId == null &&
    !askUser &&
    isSessionBusy(session.state);
  const planApprovalPayload = useMemo<AskUserPayload | null>(
    () =>
      !planIsAwaitingReview(plan) || plan.rpcId == null
        ? null
        : {
            rpcId: plan.rpcId,
            sessionId: session.sessionId || "",
            toolCallId: plan.toolCallId ?? null,
            questions: [
              {
                id: "plan-approval",
                question: tr("plan.confirmQuestion"),
                multiSelect: false,
                options: [
                  {
                    id: "approve",
                    label: tr("plan.confirmApprove"),
                  },
                ],
              },
            ],
          },
    [
      plan,
      plan.rpcId,
      plan.toolCallId,
      plan.liveReview,
      plan.visible,
      session.sessionId,
      tr,
    ],
  );
  const taskGoalSummary = useMemo(() => {
    const firstPlanLine = plan.body
      .split(/\r?\n/)
      .map((line) => line.replace(/^#{1,6}\s+/, "").trim())
      .find(Boolean);
    return (
      firstPlanLine ||
      session.title?.trim() ||
      activeProject?.name?.trim() ||
      null
    );
  }, [activeProject?.name, plan.body, session.title]);
  // Live billing can take seconds (quota network). Cache last mark so the
  // welcome logo paints immediately — the SVG itself is inline, not a fetch.
  const [cachedBrandKind, setCachedBrandKind] =
    useState<SunsetzProBrandKind | null>(() => loadCachedSunsetzProBrand());
  const liveBrandKind = useMemo(
    () =>
      sunsetzProBrandKind(
        account?.billing,
        !!account?.profile?.signedIn,
      ),
    [account?.billing, account?.profile?.signedIn],
  );
  useEffect(() => {
    // Do not cache Heavy while on a custom route — welcome mark is always Sunsetz Pro.
    if (customRouteActive) return;
    if (liveBrandKind) {
      saveCachedSunsetzProBrand(liveBrandKind);
      setCachedBrandKind(liveBrandKind);
      return;
    }
    if (account && !account.profile.signedIn) {
      saveCachedSunsetzProBrand(null);
      setCachedBrandKind(null);
    }
  }, [liveBrandKind, account, customRouteActive]);
  const welcomeBrandKind = useMemo(
    () =>
      resolveWelcomeBrandKind(liveBrandKind, cachedBrandKind, {
        accountReady: account != null,
        signedIn: !!account?.profile?.signedIn,
        customRoute: customRouteActive,
      }),
    [liveBrandKind, cachedBrandKind, account, customRouteActive],
  );

  // Floating composer height → chat bottom pad so messages can scroll under it.
  useEffect(() => {
    if (appView !== "workbench" || mainPane !== "chat") return;
    const el = composerWrapRef.current;
    if (!el) return;
    const measure = () => {
      const h = Math.ceil(el.getBoundingClientRect().height);
      if (h <= 0) return;
      // Ignore 1px subpixel flicker — pad thrash reflows chat scrollHeight
      // and looks like the transcript bouncing while you type/scroll.
      setComposerFloatPad((prev) => (Math.abs(prev - h) <= 1 ? prev : h));
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, [appView, mainPane]);

  const stop = async () => {
    try {
      await api.sessionStop();
      setRetryStatus(null);
      setStreamStall(null);
      setTurnStartedAt(null);
      setTurnStartedAt(null);
      const liveId = liveHostRef.current.sessionId;
      if (liveId) {
        patchSessionMessages(liveId, (m) =>
          m.map((x) => ({ ...x, streaming: false })),
        );
      } else {
        patchSessionMessages(DRAFT_SESSION_KEY, (m) =>
          m.map((x) => ({ ...x, streaming: false })),
        );
      }
    } catch (e) {
      setLocalError(String(e));
    }
  };

  /**
   * Bind (or clear) the open session's project. Draft chats only switch
   * workspace context. Untrusted projects refuse bind when a session exists.
   */
  const bindSessionProject = useCallback(
    async (proj: Project | null, opts?: { silent?: boolean }) => {
      const sid = session.sessionId;
      if (!sid || !api.isTauri()) {
        setActiveProject(proj);
        if (proj) {
          setExpandedProjects((e) => ({ ...e, [proj.id]: true }));
        } else {
          setHistoryOpen(true);
        }
        return;
      }
      if (proj && !proj.trusted) {
        setLocalError(tr("project.trustFirst", { name: proj.name }));
        return;
      }
      try {
        await api.sessionSetProject(sid, proj?.id ?? null);
        setActiveProject(proj);
        setSessions((list) =>
          list.map((s) =>
            s.id === sid ? { ...s, projectId: proj?.id ?? null } : s,
          ),
        );
        // Live agent used old cwd — force reconnect next send
        setSession((prev) =>
          prev.sessionId === sid
            ? {
                ...IDLE_SNAPSHOT,
                sessionId: sid,
                title: prev.title,
                state: "idle",
                backend: prev.backend || "sunsetz",
              }
            : prev,
        );
        setLiveHost((prev) =>
          prev.sessionId === sid ? { ...IDLE_SNAPSHOT } : prev,
        );
        if (proj) {
          setExpandedProjects((e) => ({ ...e, [proj.id]: true }));
          if (!opts?.silent) {
            showToast(tr("composer.projectBound", { name: proj.name }), 2500);
          }
        } else {
          setHistoryOpen(true);
          if (!opts?.silent) {
            showToast(tr("composer.projectCleared"), 2200);
          }
        }
        setLocalError(null);
      } catch (e) {
        showToast(String(e), 4500);
      }
    },
    [session.sessionId, showToast, tr],
  );

  const moveSessionToProject = useCallback(
    async (row: SessionRow, proj: Project | null) => {
      const currentId = row.projectId ?? null;
      const nextId = proj?.id ?? null;
      if (currentId === nextId) return;
      if (row.id === session.sessionId) {
        await bindSessionProject(proj);
        return;
      }
      if (proj && !proj.trusted) {
        setLocalError(tr("project.trustFirst", { name: proj.name }));
        return;
      }
      if (!api.isTauri()) {
        setSessions((list) =>
          list.map((item) =>
            item.id === row.id ? { ...item, projectId: nextId } : item,
          ),
        );
        if (viewingSessionIdRef.current === row.id) setActiveProject(proj);
        if (proj) {
          setExpandedProjects((open) => ({ ...open, [proj.id]: true }));
        } else {
          setHistoryOpen(true);
        }
        return;
      }
      try {
        await api.sessionSetProject(row.id, nextId);
        setSessions((list) =>
          list.map((item) =>
            item.id === row.id ? { ...item, projectId: nextId } : item,
          ),
        );
        if (viewingSessionIdRef.current === row.id) setActiveProject(proj);
        setLiveHost((prev) =>
          prev.sessionId === row.id ? { ...IDLE_SNAPSHOT } : prev,
        );
        if (proj) {
          setExpandedProjects((open) => ({ ...open, [proj.id]: true }));
          showToast(tr("composer.projectBound", { name: proj.name }), 2500);
        } else {
          setHistoryOpen(true);
          showToast(tr("composer.projectCleared"), 2200);
        }
        setLocalError(null);
      } catch (error) {
        showToast(String(error) || tr("session.moveFailed"), 4500);
      }
    },
    [bindSessionProject, session.sessionId, showToast, tr],
  );

  const selectComposerPlusAction = useCallback(
    async (entry: Extract<ComposerPlusEntry, { kind: "action" }>) => {
      if (entry.disabled) return;
      if (
        composerSettingsLocked &&
        isComposerSettingAction(entry.action)
      ) {
        return;
      }
      if (entry.action !== "finder") closeComposerMenu();
      switch (entry.action) {
        case "folder":
          await pickComposerFolder();
          return;
        case "finder": {
          setFinderSelectionFeedback(null);
          try {
            const paths = await api.finderSelectedPaths();
            if (!paths.length) {
              setFinderSelectionFeedback(
                tr("composer.finderSelectionEmpty"),
              );
              return;
            }
            closeComposerMenu();
            await addAttachmentsFromPaths(paths);
          } catch (error) {
            const detail = String(error).trim();
            setFinderSelectionFeedback(
              detail || tr("composer.finderSelectionFailed"),
            );
          }
          return;
        }
        case "project":
          setProjectMenuOpenKey((key) => key + 1);
          return;
        case "goal":
          setGoalMode(true);
          if (mode === "plan") setModeRaw("agent");
          return;
        case "plan": {
          setGoalMode(false);
          const previousMode = mode;
          setModeRaw("plan");
          void api
            .composerPrefsSet({
              projectId: activeProject?.id ?? null,
              sessionId: session.sessionId ?? null,
              mode: "plan",
            })
            .catch((error) => {
              setModeRaw((current) =>
                rollbackOptimisticSetting(current, "plan", previousMode),
              );
              showToast(String(error), 4000);
            });
          return;
        }
        case "ask": {
          setGoalMode(false);
          const previousMode = mode;
          setModeRaw("ask");
          void api
            .composerPrefsSet({
              projectId: activeProject?.id ?? null,
              sessionId: session.sessionId ?? null,
              mode: "ask",
            })
            .catch((error) => {
              setModeRaw((current) =>
                rollbackOptimisticSetting(current, "ask", previousMode),
              );
              showToast(String(error), 4000);
            });
          return;
        }
        case "record-skill":
          setSkillRecorderOpen(true);
          return;
      }
    },
    [
      activeProject?.id,
      addAttachmentsFromPaths,
      closeComposerMenu,
      composerSettingsLocked,
      mode,
      pickComposerFolder,
      session.sessionId,
      showToast,
      tr,
    ],
  );

  const gitWorktreesReqRef = useRef(0);
  const gitWorktreesPathRef = useRef<string | null>(null);
  const refreshGitWorktrees = useCallback(async () => {
    const path = activeProject?.path?.trim() || null;
    if (!path || !api.isTauri()) {
      gitWorktreesReqRef.current += 1;
      gitWorktreesPathRef.current = null;
      setGitWorktrees([]);
      setGitWorktreesAvailable(null);
      setGitWorktreesReason(null);
      setGitWorktreesLoading(false);
      return;
    }
    const reqId = ++gitWorktreesReqRef.current;
    // Drop stale rows when the active project path changes; soft-refresh keeps
    // the previous list for the same path so the menu does not flash empty.
    if (gitWorktreesPathRef.current !== path) {
      gitWorktreesPathRef.current = path;
      setGitWorktrees([]);
      setGitWorktreesAvailable(null);
      setGitWorktreesReason(null);
    }
    setGitWorktreesLoading(true);
    try {
      const res = await api.gitWorktreesList(path);
      if (reqId !== gitWorktreesReqRef.current) return;
      if (!res.available) {
        setGitWorktrees([]);
        setGitWorktreesAvailable(false);
        setGitWorktreesReason(res.reason?.trim() || "unavailable");
      } else {
        setGitWorktrees(res.worktrees ?? []);
        setGitWorktreesAvailable(true);
        setGitWorktreesReason(null);
      }
    } catch (e) {
      if (reqId !== gitWorktreesReqRef.current) return;
      setGitWorktrees([]);
      setGitWorktreesAvailable(false);
      setGitWorktreesReason(String(e));
    } finally {
      if (reqId === gitWorktreesReqRef.current) {
        setGitWorktreesLoading(false);
      }
    }
  }, [activeProject?.path]);

  useEffect(() => {
    void refreshGitWorktrees();
  }, [refreshGitWorktrees]);

  /**
   * After a project is created/updated: refresh list, expand, optionally trust
   * via in-app confirm, then set active (+ bind session when requested).
   */
  const finalizeAddedProject = useCallback(
    async (p: Project, opts: { bindSession: boolean }) => {
      const list = (await api.projectsList()) as Project[];
      setProjects(list);
      setSetup((s) => ({ ...s, project: true }));

      const apply = async (proj: Project) => {
        const fresh = (await api.projectsList()) as Project[];
        setProjects(fresh);
        const current = fresh.find((x) => x.id === proj.id) ?? proj;
        if (opts.bindSession) {
          await bindSessionProject(current);
        } else {
          setActiveProject(current);
          setExpandedProjects((e) => ({ ...e, [current.id]: true }));
          showToast(tr("composer.projectAdded", { name: current.name }), 2500);
        }
      };

      // Tauri WebView: never use window.confirm — offer in-app trust dialog.
      if (!p.trusted) {
        setAppDialog({
          kind: "confirm",
          title: tr("project.trustTitle"),
          message: tr("project.trustConfirm", {
            name: p.name,
            path: p.path,
          }),
          confirmLabel: tr("project.trustToSend", { name: p.name }),
          onConfirm: async () => {
            try {
              const trusted = (await api.projectTrust(p.id)) as Project;
              await apply(trusted);
            } catch (e) {
              setLocalError(String(e));
            }
          },
        });
        return;
      }
      await apply(p);
    },
    [bindSessionProject, showToast, tr],
  );

  /** Open a linked worktree as project cwd (reuse existing project if path matches). */
  const switchToWorktree = useCallback(
    async (wt: api.GitWorktreeEntry) => {
      if (!api.isTauri()) return;
      const path = wt.path?.trim();
      if (!path) return;
      try {
        const existing = projects.find((p) => pathsEqual(p.path, path));
        if (existing) {
          await bindSessionProject(existing, { silent: true });
          showToast(
            tr("composer.worktreeSwitched", {
              name: existing.name,
              branch: wt.branch || tr("composer.worktreeDetached"),
            }),
            2500,
          );
          return;
        }
        const trust = !!activeProject?.trusted;
        const added = (await api.projectAdd(path, trust)) as Project;
        const list = (await api.projectsList()) as Project[];
        setProjects(list);
        const proj = list.find((p) => p.id === added.id) ?? added;
        if (!proj.trusted) {
          await finalizeAddedProject(proj, { bindSession: true });
        } else {
          await bindSessionProject(proj, { silent: true });
          showToast(
            tr("composer.worktreeSwitched", {
              name: proj.name,
              branch: wt.branch || tr("composer.worktreeDetached"),
            }),
            2500,
          );
        }
      } catch (e) {
        showToast(String(e), 4500);
      }
    },
    [
      activeProject?.trusted,
      bindSessionProject,
      finalizeAddedProject,
      projects,
      showToast,
      tr,
    ],
  );

  /** Create a new worktree as a sibling of the active project, then switch to it. */
  const createWorktree = useCallback(
    async (branchName: string, createBranch: boolean) => {
      if (!api.isTauri() || !activeProject?.path) return;
      const branch = branchName.trim();
      if (!branch) return;
      const newPath = siblingWorktreePath(activeProject.path, branch);
      try {
        const res = await api.gitWorktreeAdd(
          activeProject.path,
          newPath,
          branch,
          createBranch,
        );
        setGitWorktrees(res.worktrees ?? []);
        showToast(tr("composer.worktreeCreated"), 2500);
        const created = res.worktrees.find((w) => pathsEqual(w.path, newPath));
        if (created) {
          await switchToWorktree(created);
        }
      } catch (e) {
        showToast(String(e), 4500);
      }
    },
    [activeProject?.path, showToast, switchToWorktree, tr],
  );

  /** Remove a linked worktree, confirming first and offering force on refusal. */
  const removeWorktree = useCallback(
    async (wt: api.GitWorktreeEntry) => {
      if (!api.isTauri() || !activeProject?.path) return;
      const attempt = async (force: boolean) => {
        try {
          const res = await api.gitWorktreeRemove(
            activeProject.path,
            wt.path,
            force,
          );
          setGitWorktrees(res.worktrees ?? []);
          showToast(tr("composer.worktreeRemoved"), 2500);
        } catch (e) {
          if (!force) {
            setAppDialog({
              kind: "confirm",
              title: tr("composer.worktreeRemoveTitle"),
              message: tr("composer.worktreeRemoveForceConfirm"),
              confirmLabel: tr("composer.worktreeRemove"),
              danger: true,
              onConfirm: () => attempt(true),
            });
          } else {
            showToast(String(e), 4500);
          }
        }
      };
      setAppDialog({
        kind: "confirm",
        title: tr("composer.worktreeRemoveTitle"),
        message: tr("composer.worktreeRemoveConfirm", { path: wt.path }),
        confirmLabel: tr("composer.worktreeRemove"),
        danger: true,
        onConfirm: () => attempt(false),
      });
    },
    [activeProject?.path, setAppDialog, showToast, tr],
  );

  /**
   * Pick folder → add project (name = folder basename; no rename prompt).
   * `bindSession` also attaches the open chat under the new project.
   */
  const addProjectFromPicker = useCallback(
    async (opts: { bindSession: boolean; autoTrust?: boolean }) => {
      setLocalError(null);
      try {
        if (!api.isTauri()) {
          setLocalError(tr("error.needTauri"));
          return;
        }
        const path = await api.pickDirectory();
        if (!path) return;
        const p = (await api.projectAdd(path, !!opts.autoTrust)) as Project;
        await finalizeAddedProject(p, { bindSession: opts.bindSession });
      } catch (e) {
        setLocalError(String(e));
      }
    },
    [finalizeAddedProject, tr],
  );

  const addProject = async (autoTrust = false) => {
    await addProjectFromPicker({ bindSession: false, autoTrust });
  };

  const trustProject = async (proj?: Project | null) => {
    const target = proj || activeProject;
    if (!target) return;
    try {
      const p = (await api.projectTrust(target.id)) as Project;
      setActiveProject(p);
      setProjects((await api.projectsList()) as Project[]);
      setLocalError(null);
      // CLI connects on first send only.
    } catch (e) {
      setLocalError(String(e));
    }
  };

  const openDoctor = () => {
    setShowDoctor(true);
  };

  // Keep tray menu actions on latest closures (listeners registered once).
  const trayHandlersRef = useRef({
    newChat: () => {},
    openSessionById: (_id: string) => {},
    openSettings: (_section: SettingsSectionId = "general") => {},
    openDoctor: () => {},
  });
  shortcutHandlersRef.current = {
    newChat: () => {
      void newChat();
    },
    openSettings: () => {
      setAppView("settings");
      setSettingsSection("general");
      window.location.hash = "#/settings/general";
    },
  };
  trayHandlersRef.current = {
    newChat: () => {
      void newChat();
    },
    openSessionById: (id: string) => {
      void (async () => {
        let row = sessions.find((s) => s.id === id) ?? null;
        if (!row) {
          try {
            const list = await api.sessionsList();
            const hit = list.find((s) => s.id === id);
            if (hit) {
              row = {
                id: hit.id,
                title: hit.title,
                projectId: hit.projectId,
                updatedAt: hit.updatedAt,
                archived: !!hit.archived,
                scheduled: !!hit.scheduled,
              };
              setSessions(
                list.map((s) => ({
                  id: s.id,
                  title: s.title,
                  projectId: s.projectId,
                  updatedAt: s.updatedAt,
                  archived: !!s.archived,
                  scheduled: !!s.scheduled,
                })),
              );
            }
          } catch {
            /* ignore */
          }
        }
        if (!row) return;
        const proj =
          projects.find((p) => p.id === row!.projectId) ?? null;
        await openSession(row, proj);
      })();
    },
    openSettings: (section: SettingsSectionId = "general") => {
      navigateSettings(section);
    },
    openDoctor: () => {
      void openDoctor();
    },
  };

  // System tray / menu-bar (Codex-style): Recent · More · Usage · New Chat · Open · Quit
  useEffect(() => {
    if (!api.isTauri()) return;
    let cancelled = false;
    const unsubs: Array<() => void> = [];
    void (async () => {
      try {
        const { listen } = await import("@tauri-apps/api/event");
        if (cancelled) return;
        unsubs.push(
          await listen("tray://new-chat", () => {
            trayHandlersRef.current.newChat();
          }),
        );
        unsubs.push(
          await listen<{ sessionId?: string }>("tray://open-session", (ev) => {
            const id = ev.payload?.sessionId;
            if (id) trayHandlersRef.current.openSessionById(id);
          }),
        );
        unsubs.push(
          await listen<{ section?: string }>("tray://open-settings", (ev) => {
            const raw = (ev.payload?.section || "general") as SettingsSectionId;
            const allowed: SettingsSectionId[] = [
              "general",
              "appearance",
              "account",
              "archived",
              "extensions",
              "runtime",
              "about",
            ];
            trayHandlersRef.current.openSettings(
              allowed.includes(raw) ? raw : "general",
            );
          }),
        );
        unsubs.push(
          await listen("tray://open-doctor", () => {
            trayHandlersRef.current.openDoctor();
          }),
        );
      } catch (e) {
        console.warn("tray listeners failed", e);
      }
    })();
    return () => {
      cancelled = true;
      for (const u of unsubs) u();
    };
  }, []);

  const error = session.lastError;
  const errorBanner = useMemo(
    () => presentErrorBanner(error, localError, locale),
    [error, localError, locale],
  );
  /** Prefer in-thread turn error; avoid stacking with the top error banner. */
  const hasChatTurnError = transcriptMeta.hasError;
  // Collapse technical dump whenever the visible error changes.
  useEffect(() => {
    setErrorDetailOpen(false);
  }, [errorBanner?.code, errorBanner?.summary, errorBanner?.detail]);

  // T15: announce stream start/end once (avoid token-level noise).
  useEffect(() => {
    const streaming =
      session.state === "streaming" || transcriptMeta.streaming;
    if (streaming && !wasStreamingRef.current) {
      setStreamA11yNote(tr("a11y.assistantStreaming"));
    } else if (!streaming && wasStreamingRef.current) {
      setStreamA11yNote(tr("a11y.assistantDone"));
      const t = window.setTimeout(() => setStreamA11yNote(""), 2500);
      wasStreamingRef.current = streaming;
      return () => window.clearTimeout(t);
    }
    wasStreamingRef.current = streaming;
  }, [session.state, transcriptMeta.streaming, tr]);

  // T15: permission bar — focus primary action, Tab trap, Escape → deny.
  useEffect(() => {
    if (!perm) return;
    const t = window.setTimeout(() => {
      preferPermissionFocus(permBarRef.current);
    }, 0);
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        const deny = mapPermissionButtons(perm.options, {
          allowOnce: tr("perm.allowOnce"),
          allowSession: tr("perm.allowSession"),
          deny: tr("perm.deny"),
        }).find((b) => b.decision === "deny");
        if (deny) {
          void api
            .sessionResolvePermission({
              interactionId: perm.interactionId,
              sessionId: perm.sessionId,
              rpcId: perm.rpcId,
              decision: deny.decision,
              optionId: deny.optionId,
              scopeKey: perm.scopeKey,
            })
            .then(() => setPerm(null));
        }
        return;
      }
      trapTabKey(e, permBarRef.current);
    };
    document.addEventListener("keydown", onKey, true);
    return () => {
      window.clearTimeout(t);
      document.removeEventListener("keydown", onKey, true);
    };
  }, [perm, tr]);

  /** T04 deck buttons: reconnect / Doctor / Settings sections / dismiss. */
  const runErrorBannerAction = useCallback(
    (action: NonNullable<ErrorBannerView["primary"]>) => {
      setErrorDetailOpen(false);
      switch (action.id) {
        case "reconnect":
          setLocalError(null);
          void ensureConnected(true).then((sid) => {
            if (sid) setLocalError(null);
          });
          break;
        case "open_doctor":
          setLocalError(null);
          openDoctor();
          break;
        case "open_runtime":
          setLocalError(null);
          navigateSettings("runtime");
          break;
        case "open_account":
          setLocalError(null);
          navigateSettings("account");
          break;
        case "open_providers":
          setLocalError(null);
          // Providers live under account / extensions path — account is the
          // login+key surface; extensions holds MCP. Prefer account for keys.
          navigateSettings("account");
          break;
        case "dismiss":
          setLocalError(null);
          break;
        default:
          break;
      }
    },
    [ensureConnected, navigateSettings, openDoctor],
  );

  const refreshAccount = useCallback(
    async (opts?: { refreshBilling?: boolean }) => {
      if (!api.isTauri()) return;
      setAccountLoading(true);
      try {
        const st = await api.accountStatus({
          refreshBilling: opts?.refreshBilling ?? true,
          manualCliPath: manualCliPath || null,
        });
        setAccount(st);
        setSetup((s) => ({
          ...s,
          auth: isAccountConnected(st),
          cli: st.cliFound || s.cli,
        }));
        try {
          const list = await api.accountsList();
          setSavedAccounts(list.profiles ?? []);
          setActiveAccountId(list.activeId ?? null);
        } catch {
          // multi-account list is best-effort
        }
        // Usage line on tray menu (Codex-style)
        void api.trayRefresh();
      } catch (e) {
        console.warn("account status failed", e);
      } finally {
        setAccountLoading(false);
      }
    },
    [manualCliPath],
  );

  const refreshSavedAccounts = useCallback(async () => {
    if (!api.isTauri()) return;
    try {
      const list = await api.accountsList();
      setSavedAccounts(list.profiles ?? []);
      setActiveAccountId(list.activeId ?? null);
    } catch {
      /* ignore */
    }
  }, []);

  /** Import markdown/JSON transcript as a new local session (from PR #24). */
  const importChatTranscript = useCallback(async () => {
    if (!api.isTauri()) {
      showToast(tr("error.needTauri"));
      return;
    }
    setAccountBusy(true);
    try {
      const created = await api.sessionImportTranscriptFile(
        null,
        activeProject?.id ?? null,
      );
      if (!created) return;
      await refreshSessions();
      showToast(tr("account.importChatOk", { title: created.title }), 3200);
      const list = (await api.sessionsList()) as SessionRow[];
      const hit = list.find((s) => s.id === created.id);
      if (hit) {
        const proj =
          projects.find((p) => p.id === (hit.projectId ?? undefined)) ?? null;
        void openSession(hit, proj ?? undefined);
      }
    } catch (e) {
      showToast(
        `${tr("account.importChatFailed")}: ${String(e)}`,
        5000,
      );
    } finally {
      setAccountBusy(false);
    }
  }, [activeProject?.id, projects, showToast, tr]);

  /** Export active (or given) session as Markdown (from PR #24). */
  const exportActiveSessionMd = useCallback(
    async (sessionMeta?: {
      id: string;
      title: string;
      projectId?: string | null;
    }) => {
      try {
        const id = sessionMeta?.id ?? session.sessionId;
        if (!id) {
          showToast(tr("session.exportFail"));
          return;
        }
        const title =
          sessionMeta?.title ||
          sessions.find((s) => s.id === id)?.title ||
          session.title ||
          tr("session.untitled");
        const projectId =
          sessionMeta?.projectId ??
          sessions.find((s) => s.id === id)?.projectId ??
          null;
        const proj =
          projects.find((p) => p.id === projectId) || activeProject || null;
        let msgs =
          id === session.sessionId
            ? transcriptStore.getViewed()
            : ((await api.sessionMessages(id)) as ChatMessage[]);
        const md = sessionToMarkdown({
          title,
          projectName: proj?.name,
          projectPath: proj?.path,
          sessionId: id,
          messages: msgs.map((m) => ({
            role: m.role,
            content: m.content,
            thought: m.thought,
            createdAt: m.createdAt,
          })),
        });
        const blob = new Blob([md], { type: "text/markdown;charset=utf-8" });
        const url = URL.createObjectURL(blob);
        const a = document.createElement("a");
        a.href = url;
        a.download = sessionExportFilename(title, id);
        a.click();
        URL.revokeObjectURL(url);
        showToast(tr("session.exportDone"));
      } catch (e) {
        showToast(`${tr("session.exportFail")}: ${String(e)}`);
      }
    },
    [
      session.sessionId,
      session.title,
      sessions,
      projects,
      activeProject,
      showToast,
      tr,
    ],
  );

  /** Full diagnostic zip (messages + agent trail + logs) for bug reports. */
  const exportSessionDiagnostic = useCallback(
    async (sessionId?: string | null) => {
      const id = sessionId || session.sessionId;
      if (!id) {
        showToast(tr("session.exportBundleFail"));
        return;
      }
      try {
        const res = await api.exportSessionBundle(id);
        if (res?.ok && res.path) {
          showToast(tr("session.exportBundleDone"), 4200);
        } else {
          showToast(tr("session.exportBundleFail"));
        }
      } catch (e) {
        showToast(`${tr("session.exportBundleFail")}: ${String(e)}`, 5000);
      }
    },
    [session.sessionId, showToast, tr],
  );

  const beginEditLastUser = useCallback(
    (msg: ChatMessage) => {
      if (msg.role !== "user") return;
      if (msg.id !== lastUserMessageId) {
        showToast(tr("message.editOnlyLast"));
        return;
      }
      if (!canEditLastUser) {
        showToast(tr("message.editBusy"));
        return;
      }
      // Inline only — do not move content into the main composer.
      // Reload original attachments into editable chips.
      setEditAttachments(
        (msg.attachments ?? []).map((a) => ({
          path: a.path,
          name: a.name,
          isDir: a.isDir,
        })),
      );
      setEditingUserMessageId(msg.id);
    },
    [lastUserMessageId, canEditLastUser, showToast, tr],
  );

  const cancelEditUser = useCallback(() => {
    if (editSubmitting) return;
    setEditingUserMessageId(null);
    setEditAttachments([]);
  }, [editSubmitting]);

  /**
   * Edit last user turn: commit UI immediately (edited bubble + thinking),
   * then connect / rewind / send while the thinking row is already visible.
   */
  const submitEditLastUser = useCallback(
    async (msg: ChatMessage, storedDisplay: string) => {
      if (msg.role !== "user" || msg.id !== lastUserMessageId) {
        showToast(tr("message.editOnlyLast"));
        return;
      }
      if (!canEditLastUser || editSubmitting) {
        showToast(tr("message.editBusy"));
        return;
      }
      const segments = parseStoredContent(storedDisplay);
      const unboundSkill = segments.find(
        (segment) => segment.type === "skill" && !segment.binding,
      );
      if (unboundSkill?.type === "skill") {
        setLocalError(
          `SKILL_USE_STALE: reselect ${unboundSkill.name} in Composer before resending`,
        );
        return;
      }
      // Live editable set is the source of truth (may have added/removed files).
      const att: Attachment[] = editAttachments.map((a) => ({
        path: a.path,
        name: a.name,
        isDir: a.isDir,
      }));
      if (isDraftEmpty(segments) && !att.length) return;

      const agentBody = serializeForAgent(segments, { goalMode });
      const agentText = buildAgentPrompt(agentBody, att);
      const titleSeed =
        serializeForAgent(segments).replace(/\n/g, " ").trim() ||
        att.map((a) => a.name).join(", ");
      const shouldAutoTitle =
        isPlaceholderTitle(session.title) || !session.sessionId;
      const pendingAssistantId = `a-pending-${Date.now()}`;
      // May still be a draft id; ensureConnected materializes it later.
      let sendTargetId = session.sessionId;
      let cacheKey = sendTargetId ?? "__draft__";
      const nowIso = new Date().toISOString();

      setEditSubmitting(true);

      // 1) Instant UI commit — same as normal send: user bubble + thinking.
      //    Connect/rewind wait happens under this thinking row, not the edit form.
      patchSessionMessages(cacheKey, (m) => {
        const kept = truncateBeforeLastUser(m);
        return [
          ...kept,
          {
            id: `u-${Date.now()}`,
            role: "user",
            content: storedDisplay,
            attachments: att.length ? att : undefined,
            createdAt: nowIso,
          },
          {
            id: pendingAssistantId,
            role: "assistant",
            content: "",
            streaming: true,
          },
        ];
      });
      setEditingUserMessageId(null);
      setEditAttachments([]);
      setRetryStatus(null);
      setSession((prev) =>
        prev.state === "streaming" || prev.state === "awaiting_permission"
          ? prev
          : { ...prev, state: "streaming", lastError: null },
      );
      setLiveHost((prev) => {
        if (sendTargetId && prev.sessionId && prev.sessionId !== sendTargetId) {
          return prev;
        }
        const next = {
          ...prev,
          sessionId: sendTargetId ?? prev.sessionId,
          state: "streaming" as const,
          lastError: null,
        };
        liveHostRef.current = next;
        return next;
      });

      const failPending = (errText?: string) => {
        const errTarget = sendTargetId ?? viewingSessionIdRef.current;
        patchSessionMessages(errTarget, (m) =>
          applyTurnError(
            m,
            {
              messageId: pendingAssistantId,
              content: errText || tr("message.editConnectFailed"),
            },
            localeRef.current,
          ),
        );
        if (
          viewingSessionIdRef.current === sendTargetId ||
          viewingSessionIdRef.current === errTarget ||
          (!sendTargetId && viewingSessionIdRef.current === null)
        ) {
          setSession((prev) =>
            prev.state === "streaming"
              ? { ...prev, state: prev.sessionId ? "ready" : prev.state }
              : prev,
          );
        }
      };

      // 2) Background: connect → rewind journal → send (thinking already shown).
      try {
        const sessionId = await ensureConnected();
        if (!sessionId) {
          failPending(tr("message.editConnectFailed"));
          return;
        }
        // Draft / id migrate after materialize.
        if (sessionId !== cacheKey) {
          const prevCache = transcriptStore.getCached(cacheKey);
          if (prevCache?.length) {
            transcriptStore.write(sessionId, prevCache);
            transcriptStore.evict(cacheKey);
          }
          sendTargetId = sessionId;
          cacheKey = sessionId;
        }

        if (api.isTauri()) {
          try {
            await api.sessionRewindDropLastUser();
          } catch (e) {
            console.warn("session rewind before edit failed", e);
            // Continue: UI already replaced the turn; resend still proceeds.
          }
        }

        await api.sessionSend(
          agentText,
          storedDisplay,
          att.map(({ path, name, isDir }) => ({ path, name, isDir })),
        );
        if (shouldAutoTitle && api.isTauri()) {
          void api
            .sessionAutoTitle(sessionId, titleSeed)
            .then((meta) => {
              if (meta?.title) applySessionTitle(sessionId, meta.title);
            })
            .catch(() => {
              /* ignore */
            });
        }
      } catch (e) {
        failPending(String(e));
        if (
          viewingSessionIdRef.current === sendTargetId ||
          viewingSessionIdRef.current === null
        ) {
          setLocalError(String(e));
        }
      } finally {
        setEditSubmitting(false);
      }
    },
    [
      lastUserMessageId,
      canEditLastUser,
      editSubmitting,
      editAttachments,
      showToast,
      tr,
      goalMode,
      session.title,
      session.sessionId,
      // ensureConnected / patchSessionMessages / applySessionTitle via closure
    ],
  );

  const runAccountLogin = useCallback(
    async (method: "oauth" | "device" = "oauth"): Promise<boolean> => {
      if (!api.isTauri()) {
        showToast(tr("error.needTauri"));
        return false;
      }
      setAccountBusy(true);
      setLoginHint(null);
      try {
        const res = await api.accountLogin(method);
        if (res.ok) {
          setLoginHint(null);
          showToast(tr("account.loginOk"), 2800);
        } else {
          const msg = res.message || tr("account.loginFailed");
          setLoginHint(msg);
          showToast(msg, 6000);
        }
        if (res.deviceUrl) {
          try {
            await api.openExternalUrl(res.deviceUrl);
          } catch {
            /* host may already open it */
          }
          showToast(
            [res.deviceUrl, res.deviceCode ? `code: ${res.deviceCode}` : ""]
              .filter(Boolean)
              .join(" · "),
            10000,
          );
        }
        await refreshAccount({ refreshBilling: true });
        await refreshSavedAccounts();
        // Drop live agent so next send re-spawns with synced auth.json in agent-home.
        if (res.ok && api.isTauri()) {
          try {
            await api.sessionDisconnect();
            setSession({ ...IDLE_SNAPSHOT });
          } catch {
            /* ignore */
          }
        }
        return !!res.ok;
      } catch (e) {
        const msg = String(e);
        setLoginHint(msg);
        showToast(msg, 4500);
        return false;
      } finally {
        setAccountBusy(false);
      }
    },
    [refreshAccount, refreshSavedAccounts, showToast, tr],
  );

  /** Abort a running login (OAuth/device) so the user can pick another method
   *  without restarting the app. The backend kills the `grok login` child. */
  const cancelAccountLogin = useCallback(async () => {
    try {
      await api.accountLoginCancel();
    } catch {
      /* ignore — still unlock UI */
    }
    setAccountBusy(false);
  }, []);

  const runSaveAccount = useCallback(async () => {
    if (!api.isTauri()) return;
    setAccountBusy(true);
    try {
      await api.accountSaveCurrent();
      await refreshSavedAccounts();
      showToast(tr("account.profileSaved"), 2500);
    } catch (e) {
      showToast(String(e), 4500);
    } finally {
      setAccountBusy(false);
    }
  }, [refreshSavedAccounts, showToast, tr]);

  /**
   * Save current login (if any), then start OAuth so the user can add another
   * account without losing the previous snapshot.
   */
  const runAddAccount = useCallback(async () => {
    if (!api.isTauri()) {
      showToast(tr("error.needTauri"));
      return;
    }
    // Snapshot current auth first so switcher keeps it.
    if (account?.profile?.signedIn) {
      setAccountBusy(true);
      try {
        await api.accountSaveCurrent();
        await refreshSavedAccounts();
        showToast(tr("account.profileSaved"), 1800);
      } catch (e) {
        // Still try login — user may want a fresh account even if save fails.
        showToast(String(e), 3500);
      } finally {
        setAccountBusy(false);
      }
    }
    await runAccountLogin("oauth");
  }, [
    account?.profile?.signedIn,
    refreshSavedAccounts,
    runAccountLogin,
    showToast,
    tr,
  ]);

  const runSwitchAccount = useCallback(
    async (id: string) => {
      if (!api.isTauri()) return;
      setAccountBusy(true);
      try {
        await api.accountSwitch(id);
        await refreshAccount({ refreshBilling: true });
        await refreshSavedAccounts();
        try {
          await api.sessionDisconnect();
        } catch {
          /* ignore */
        }
        setSession({ ...IDLE_SNAPSHOT });
        showToast(tr("account.profileSwitched"), 2500);
      } catch (e) {
        showToast(String(e), 4500);
      } finally {
        setAccountBusy(false);
      }
    },
    [refreshAccount, refreshSavedAccounts, showToast, tr],
  );

  const runRemoveAccount = useCallback(
    (id: string) => {
      if (!api.isTauri()) return;
      const label =
        savedAccounts.find((a) => a.id === id)?.label || id.slice(0, 8);
      setAppDialog({
        kind: "confirm",
        title: tr("account.profileRemove"),
        message: tr("account.profilesHint"),
        confirmLabel: tr("account.profileRemove"),
        danger: true,
        onConfirm: async () => {
          setAccountBusy(true);
          try {
            await api.accountRemove(id);
            await refreshSavedAccounts();
            showToast(tr("account.profileRemoved"), 2200);
          } catch (e) {
            showToast(String(e), 4500);
          } finally {
            setAccountBusy(false);
          }
        },
      });
      void label;
    },
    [refreshSavedAccounts, savedAccounts, showToast, tr],
  );

  const runAccountLogout = useCallback(async () => {
    if (!api.isTauri()) return;
    setAccountBusy(true);
    try {
      await api.accountLogout();
      await refreshAccount({ refreshBilling: false });
      await refreshSavedAccounts();
      try {
        await api.sessionDisconnect();
        setSession({ ...IDLE_SNAPSHOT });
      } catch {
        /* ignore */
      }
    } catch (e) {
      showToast(String(e), 4500);
    } finally {
      setAccountBusy(false);
    }
  }, [refreshAccount, refreshSavedAccounts, showToast]);

  // Account boot: paint fast from disk cache first, then refresh quota on network.
  // Welcome Sunsetz Pro logo depends on billing tier — waiting only on the slow
  // path made the mark look like a "slow image" even though it is inline SVG.
  useEffect(() => {
    if (!api.isTauri()) return;
    let cancelled = false;
    void (async () => {
      await refreshAccount({ refreshBilling: false });
      if (cancelled) return;
      await refreshAccount({ refreshBilling: true });
      if (cancelled) return;
      await refreshSavedAccounts();
    })();
    return () => {
      cancelled = true;
    };
  }, [refreshAccount, refreshSavedAccounts]);

  useEffect(() => {
    if (appView === "settings" && settingsSection === "account") {
      void refreshAccount({ refreshBilling: true });
      void refreshSavedAccounts();
    }
  }, [appView, settingsSection, refreshAccount, refreshSavedAccounts]);

  const [memoryCandidateSource, setMemoryCandidateSource] = useState<{
    sessionId: string;
    messageId: string;
  } | null>(null);
  useEffect(() => {
    const sessionId = session.sessionId;
    if (appView !== "settings" || !sessionId) {
      setMemoryCandidateSource(null);
      return;
    }
    let cancelled = false;
    void api
      .sessionMessages(sessionId)
      .then((stored) => {
        if (cancelled) return;
        const message = [...stored]
          .reverse()
          .find((item) => item.role === "user" && item.id);
        setMemoryCandidateSource(
          message ? { sessionId, messageId: message.id } : null,
        );
      })
      .catch(() => {
        if (!cancelled) setMemoryCandidateSource(null);
      });
    return () => {
      cancelled = true;
    };
  }, [appView, session.sessionId]);

  const settingsPatchSequenceRef = useRef(0);
  const settingsPatchOwnersRef = useRef(new Map<keyof api.AppSettings, number>());
  const applyAuthoritativeSettingFields = useCallback(
    (stored: api.AppSettings, keys: ReadonlySet<keyof api.AppSettings>) => {
      if (keys.has("locale")) setLocale(resolveLocale(stored.locale));
      if (keys.has("sessionDataMode")) {
        setSessionDataMode(stored.sessionDataMode || "independent");
      }
      if (keys.has("composerPrefsScope")) {
        const storedScope = stored.composerPrefsScope;
        setPrefsScopeRaw(
          storedScope && isValidPrefsScope(storedScope) ? storedScope : "global",
        );
      }
      if (keys.has("manualCliPath")) {
        setManualCliPath(stored.manualCliPath || "");
      }
      if (keys.has("runtimeBackend")) {
        setRuntimeBackend(stored.runtimeBackend || "sunsetz");
      }
      if (keys.has("acpServerAddr")) {
        setAcpServerAddr(stored.acpServerAddr || "");
      }
      if (keys.has("maxConcurrentAgents")) {
        setMaxConcurrentAgents(
          typeof stored.maxConcurrentAgents === "number"
            ? Math.max(1, Math.min(8, Math.round(stored.maxConcurrentAgents)))
            : 3,
        );
      }
      if (keys.has("agentIdleMinutes")) {
        setAgentIdleMinutes(
          typeof stored.agentIdleMinutes === "number"
            ? Math.max(1, Math.min(1440, Math.round(stored.agentIdleMinutes)))
            : 30,
        );
      }
      if (keys.has("streamStallSeconds")) {
        setStreamStallSeconds(
          typeof stored.streamStallSeconds === "number"
            ? Math.max(15, Math.min(900, Math.round(stored.streamStallSeconds)))
            : 120,
        );
      }
      if (keys.has("storeApiKeysInKeychain")) {
        setStoreApiKeysInKeychain(!!stored.storeApiKeysInKeychain);
      }
      if (keys.has("defaultOpenTarget")) {
        setDefaultOpenTarget(stored.defaultOpenTarget || "finder");
      }
      if (keys.has("sandboxProfile")) {
        setSandboxProfile(normalizeSandboxProfile(stored.sandboxProfile));
      }
      if (keys.has("runScheduledTasksInBackground")) {
        setRunScheduledTasksInBackground(!!stored.runScheduledTasksInBackground);
      }
    },
    [],
  );
  const patchSettingsSafely = useCallback(
    async (patch: Partial<api.AppSettings>) => {
      const sequence = ++settingsPatchSequenceRef.current;
      const keys = Object.keys(patch) as Array<keyof api.AppSettings>;
      for (const key of keys) settingsPatchOwnersRef.current.set(key, sequence);
      const currentKeys = () =>
        new Set(
          keys.filter(
            (key) => settingsPatchOwnersRef.current.get(key) === sequence,
          ),
        );
      try {
        const stored = await api.settingsPatchV1(patch);
        applyAuthoritativeSettingFields(stored, currentKeys());
        return stored;
      } catch (error) {
        const owned = currentKeys();
        if (owned.size > 0) {
          try {
            const stored = await api.settingsGet();
            applyAuthoritativeSettingFields(stored, owned);
          } catch {
            // Keep the explicit error visible even if authoritative reload fails.
          }
          showToast(String(error), 4500);
        }
        return null;
      }
    },
    [applyAuthoritativeSettingFields, showToast],
  );
  useEffect(() => {
    patchSettingsSafelyRef.current = patchSettingsSafely;
  }, [patchSettingsSafely]);

  const settingsLabels = useMemo(() => {
    const keys = [
      "settings.backToApp",
      "settings.searchPlaceholder",
      "settings.group.personal",
      "settings.group.system",
      "settings.nav.general",
      "settings.nav.appearance",
      "settings.nav.account",
      "settings.nav.models",
      "settings.nav.archived",
      "settings.nav.extensions",
      "settings.nav.runtime",
      "settings.nav.about",
      "settings.archived.desc",
      "settings.archived.empty",
      "settings.archived.restore",
      "settings.archived.delete",
      "settings.archived.orphan",
      "settings.archived.selectAll",
      "settings.archived.deselectAll",
      "settings.archived.selectedCount",
      "settings.archived.totalCount",
      "session.untitled",
      "settings.section.permissions",
      "settings.section.composer",
      "settings.section.general",
      "settings.language",
      "settings.languageDesc",
      "settings.sessionDataMode",
      "settings.sessionDataModeDesc",
      "settings.cliPath",
      "settings.cliPathDesc",
      "settings.cliNotFound",
      "settings.permissionDeep",
      "settings.permissionDeepDesc",
      "settings.prefsScope",
      "settings.prefsScopeDesc",
      "settings.prefsScope.global",
      "settings.prefsScope.project",
      "settings.prefsScope.session",
      "settings.availableModels",
      "settings.availableModelsDesc",
      "settings.availableModelsEmpty",
      "settings.theme",
      "settings.themeDesc",
      "settings.themeLight",
      "settings.themeDark",
      "settings.doctorDesc",
      "settings.runDoctor",
      "settings.aboutApp",
      "composer.permissionTitle",
      "policy.ask",
      "policy.accept_edits",
      "policy.allow_for_session",
      "policy.dont_ask",
      "policy.always_approve",
      "settings.modeIndependent",
      "settings.modeShared",
      "settings.tabOfficial",
      "settings.tabProviders",
      "settings.tabOfficialHint",
      "settings.tabProvidersHint",
      "settings.openTarget",
      "settings.openTargetDesc",
      "settings.openFinder",
      "settings.sharedConfirm",
      "doctor.title",
      "doctor.close",
      "doctor.rerun",
      "doctor.copy",
      "doctor.copied",
      "doctor.loading",
      "doctor.error",
      "doctor.empty",
      "doctor.summary",
      "doctor.generatedAt",
      "doctor.level.ok",
      "doctor.level.warn",
      "doctor.level.fail",
      "doctor.check.cli",
      "doctor.check.auth",
      "doctor.check.workspace",
      "doctor.check.backend",
      "doctor.check.logs",
      "common.local",
      "common.close",
      "common.cancel",
      "account.section.profile",
      "account.section.runtime",
      "account.signedIn",
      "account.signedOut",
      "account.loginOauth",
      "account.loginDevice",
      "account.loginBusy",
      "account.loginCancel",
      "account.logout",
      "account.refresh",
      "account.refreshing",
      "account.manageUsage",
      "account.subscribe",
      "account.channel",
      "account.channel.oauth",
      "account.channel.key",
      "account.channel.relay",
      "account.channel.none",
      "account.subscription",
      "account.weeklyTitle",
      "account.quota",
      "account.quotaRemaining",
      "account.quotaUsed",
      "account.quotaUnknown",
      "account.period",
      "account.prepaid",
      "account.onDemand",
      "account.resetsAt",
      "account.fetchedAt",
      "account.products",
      "account.heatmap",
      "account.heatmapHint",
      "account.heatmap.less",
      "account.heatmap.more",
      "account.heatmap.noData",
      "account.heatmap.aria",
      "account.heatmap.requests",
      "account.heatmap.tokens",
      "account.callLogs",
      "account.callLogsEmpty",
      "account.col.session",
      "account.col.model",
      "account.col.turns",
      "account.col.tokens",
      "account.col.duration",
      "account.col.when",
      "account.expired",
      "account.team",
      "account.billingUnavailable",
      "account.cliAuthOk",
      "account.cliAuthMissing",
      "account.loginHelpTitle",
      "account.loginHelpBody",
      "account.loginTryDevice",
      "account.profiles",
      "account.profilesHint",
      "account.profilesEmpty",
      "account.profileSave",
      "account.profileSwitch",
      "account.profileRemove",
      "account.profileActive",
      "account.manageAccounts",
      "account.addAccount",
      "account.profileSwitch",
      "account.profileRemove",
      "account.profileActive",
      "account.importChat",
      "account.importChatHint",
      "account.importChatBtn",
    ] as const;
    const out: Record<string, string> = {};
    for (const k of keys) out[k] = tr(k);
    return out;
  }, [tr]);

  return (
    <ImageViewerProvider locale={locale}>
    <div
      className={
        `app-shell platform-${platform}` +
        (windowMaximized ? " is-maximized" : "") +
        (useCustomWindowChrome ? " has-custom-chrome" : "")
      }
      data-testid="app-shell"
    >
      <WindowControls
        visible={useCustomWindowChrome}
        labels={{
          minimize: tr("window.minimize"),
          maximize: tr("window.maximize"),
          restore: tr("window.restore"),
          close: tr("window.close"),
        }}
      />

      {toast && (
        <div className="app-toast" role="status">
          {toast}
        </div>
      )}

      {appGate === "loading" && (
        <div className="setup-gate" data-testid="setup-booting">
          <div className="setup-gate__drag" data-tauri-drag-region />
          <div className="setup-gate__center">
            <div className="setup-hero">
              <div className="setup-logo setup-logo--spin">
                <SunsetzLogo size={44} />
              </div>
              <h1 className="setup-title">{tr("setup.title")}</h1>
              <p className="setup-subtitle">{tr("setup.detecting")}</p>
            </div>
          </div>
        </div>
      )}

      {appGate === "setup" && (
        <SetupWizard
          tr={tr}
          platform={platform}
          useCustomWindowChrome={useCustomWindowChrome}
          initialCli={
            setupCliSeed ?? {
              found: false,
              path: null,
              version: null,
              source: "",
              cliAuthPresent: false,
            }
          }
          onAccountLoginOauth={() => runAccountLogin("oauth")}
          onComplete={(cli) => {
            setCliInfo({
              found: cli.found,
              path: cli.path,
              version: cli.version,
              source: cli.source,
              cliAuthPresent: cli.cliAuthPresent,
            });
            if (cli.path) setManualCliPath(cli.path);
            setSetup((s) => ({
              ...s,
              cli: cli.found,
              auth: s.auth || cli.cliAuthPresent,
            }));
            setAppGate("ready");
            void refreshLists();
            void refreshAccount({ refreshBilling: false });
          }}
        />
      )}

      {appGate === "ready" && (appView === "settings" ? (
        <SettingsPage
          section={settingsSection}
          onSection={(id) => {
            setSettingsSection(id);
            window.location.hash = `#/settings/${id}`;
          }}
          onBack={navigateWorkbench}
          labels={settingsLabels}
          locale={locale}
          onLocale={(v) => {
            const next = resolveLocale(v);
            setLocale(next);
            void patchSettingsSafely({ locale: next });
          }}
          theme={theme}
          onTheme={applyThemeChoice}
          sessionDataMode={sessionDataMode}
          onCliSessionsImported={() => {
            void refreshSessions();
          }}
          onSessionDataMode={(v) => {
            const commit = () => {
              setSessionDataMode(v);
              void patchSettingsSafely({ sessionDataMode: v });
            };
            // Tauri WebView: window.confirm is unreliable (often always false).
            if (v === "shared") {
              setAppDialog({
                kind: "confirm",
                title: tr("settings.sessionDataMode"),
                message: tr("settings.sharedConfirm"),
                confirmLabel: tr("common.confirm"),
                onConfirm: commit,
              });
              return;
            }
            commit();
          }}
          policy={policy}
          onPolicy={onPolicy}
          prefsScope={prefsScope}
          onPrefsScope={onPrefsScope}
          availableModels={availableModels}
          manualCliPath={manualCliPath}
          onManualCliPath={setManualCliPath}
          onCliBlur={(v) => {
            void patchSettingsSafely({ manualCliPath: v || null });
            void api.probeCli(v || undefined).then((cli) => {
              setCliInfo({
                found: cli.found,
                path: cli.path,
                version: cli.version,
                source: cli.source || "",
                cliAuthPresent: !!cli.cliAuthPresent,
              });
              setSetup((prev) => ({
                ...prev,
                cli: cli.found,
                auth: prev.auth || !!cli.cliAuthPresent,
              }));
            });
          }}
          kernelBackend={
            isDeveloperMockBackend(liveHost.backend) ||
            isDeveloperMockBackend(session.backend)
              ? "mock_acp"
              : runtimeBackend
          }
          onKernelBackend={(v) => {
            setRuntimeBackend(v);
            void patchSettingsSafely({ runtimeBackend: v });
          }}
          acpServerAddr={acpServerAddr}
          onAcpServerAddr={(v) => {
            setAcpServerAddr(v);
            void patchSettingsSafely({ acpServerAddr: v.trim() || null });
          }}
          maxConcurrentAgents={maxConcurrentAgents}
          onMaxConcurrentAgents={(v) => {
            setMaxConcurrentAgents(v);
            void patchSettingsSafely({ maxConcurrentAgents: v });
          }}
          agentIdleMinutes={agentIdleMinutes}
          onAgentIdleMinutes={(v) => {
            setAgentIdleMinutes(v);
            void patchSettingsSafely({ agentIdleMinutes: v });
          }}
          streamStallSeconds={streamStallSeconds}
          onStreamStallSeconds={(v) => {
            setStreamStallSeconds(v);
            void patchSettingsSafely({ streamStallSeconds: v });
          }}
          storeApiKeysInKeychain={storeApiKeysInKeychain}
          onStoreApiKeysInKeychain={(v) => {
            setStoreApiKeysInKeychain(v);
            void patchSettingsSafely({ storeApiKeysInKeychain: v });
          }}
          cliInfo={cliInfo}
          onDoctor={() => void openDoctor()}
          versionFooter={tr("app.versionFooter")}
          account={account}
          accountLoading={accountLoading}
          accountBusy={accountBusy}
          loginHint={loginHint}
          savedAccounts={savedAccounts}
          activeAccountId={activeAccountId}
          onAccountLoginOauth={() => void runAccountLogin("oauth")}
          onAccountLoginDevice={() => void runAccountLogin("device")}
          onCancelLogin={() => void cancelAccountLogin()}
          onAccountLogout={() => void runAccountLogout()}
          onAccountRefresh={() => void refreshAccount({ refreshBilling: true })}
          onAccountManageUsage={() => void api.accountOpenUsage()}
          onAccountSubscribe={() => void api.accountOpenSubscribe()}
          onSaveAccount={() => void runSaveAccount()}
          onAddAccount={() => void runAddAccount()}
          onSwitchAccount={(id) => void runSwitchAccount(id)}
          onRemoveAccount={(id) => void runRemoveAccount(id)}
          onImportChat={() => void importChatTranscript()}
          defaultOpenTarget={defaultOpenTarget}
          onDefaultOpenTarget={(v) => {
            setDefaultOpenTarget(v);
            void patchSettingsSafely({ defaultOpenTarget: v });
          }}
          archivedGroups={archivedGroups}
          onRestoreArchivedSessions={(ids) => {
            const rows = ids
              .map((id) => sessions.find((x) => x.id === id))
              .filter((s): s is SessionRow => !!s);
            void restoreSessions(rows);
          }}
          onDeleteArchivedSessions={(ids) => {
            const rows = ids
              .map((id) => sessions.find((x) => x.id === id))
              .filter((s): s is SessionRow => !!s);
            deleteSessionsConfirm(rows);
          }}
          projectPath={activeProject?.path ?? null}
          activeSessionId={session.sessionId}
          onSkillsPrefsChanged={() =>
            setSkillsReloadToken((n) => n + 1)
          }
          memorySource={memoryCandidateSource}
          onUseMemoryContext={(pack) => {
            memoryPackRestoreGenRef.current += 1;
            setPendingMemoryContext(pack);
            setAppView("workbench");
            pendingComposerFocus.current = true;
          }}
          sandboxProfile={sandboxProfile}
          onSandboxProfile={(v) => {
            setSandboxProfile(v);
            void patchSettingsSafely({ sandboxProfile: v });
          }}
          runScheduledTasksInBackground={runScheduledTasksInBackground}
          onRunScheduledTasksInBackground={(v) => {
            setRunScheduledTasksInBackground(v);
            void patchSettingsSafely({ runScheduledTasksInBackground: v });
          }}
          onProviderActivated={() => {
            // Hot-reload Sunsetz Runtime: drop live ACP so next send re-spawns with new GROK_HOME config.
            void (async () => {
              try {
                if (api.isTauri()) {
                  await api.sessionDisconnect();
                  setSession({ ...IDLE_SNAPSHOT });
                }
                await refreshProviderRoute();
                await refreshLists();
                await refreshAccount({ refreshBilling: false });
                setToast(tr("prov.switchedHotReload"));
                window.setTimeout(() => setToast(null), 3200);
              } catch (e) {
                setToast(String(e));
              }
            })();
          }}
        />
      ) : (
      <WorkbenchShell
        sidebarCollapsed={layout.sidebarCollapsed}
        asideCollapsed={layout.asideCollapsed}
        sidebarToggleRef={sidebarToggleRef}
        asideToggleRef={asideToggleRef}
      >
        <SidebarNavigator
          collapsed={layout.sidebarCollapsed}
          dragZone={dragZone}
          labels={{
            chrome: {
              hide: tr("main.leftPaneHide"),
            },
            drag: {
              addProjectTitle: tr("composer.dropProjectTitle"),
              addProjectHint: tr("composer.dropProjectHint"),
            },
            navigation: {
              label: tr("sidebar.projects"),
              newSession: tr("sidebar.newSession"),
              search: tr("sidebar.search"),
              scheduled: tr("sidebar.scheduled"),
              plugins: tr("sidebar.plugins"),
            },
            tree: {
              projects: tr("sidebar.projects"),
              addProject: tr("sidebar.addProject"),
              noProjects: tr("sidebar.noProjects"),
              collapseProject: tr("sidebar.collapseProject"),
              expandProject: tr("sidebar.expandProject"),
              untrusted: tr("sidebar.untrusted"),
              menu: tr("sidebar.menu"),
              organize: tr("sidebar.organize"),
              groupByProject: tr("sidebar.groupByProject"),
              groupByList: tr("sidebar.groupByList"),
              chatSort: tr("sidebar.chatSort"),
              sortPriority: tr("sidebar.sortPriority"),
              sortRecent: tr("sidebar.sortRecent"),
              newConversation: tr("sidebar.newConversation"),
              editProject: tr("sidebar.editProject"),
              collapseProjects: tr("sidebar.collapseProjects"),
              expandProjects: tr("sidebar.expandProjects"),
              trustProject: tr("sidebar.trustProject"),
              noChats: tr("sidebar.noChats"),
              otherSessions: tr("sidebar.otherSessions"),
              untitled: tr("session.untitled"),
              scheduledTag: tr("automations.msgTag"),
              answerNeeded: tr("sidebar.answerNeeded"),
              sessionWorking: tr("sidebar.sessionWorking"),
              previewTasks: tr("sidebar.previewTasks"),
              previewActive: tr("sidebar.previewActive"),
              previewUpdated: tr("sidebar.previewUpdated"),
              previewPinned: tr("sidebar.previewPinned"),
              previewNoSummary: tr("sidebar.previewNoSummary"),
              previewGitRef: tr("sidebar.previewGitRef"),
              previewGitAhead: tr("sidebar.previewGitAhead"),
              previewGitBehind: tr("sidebar.previewGitBehind"),
              previewGitDirty: tr("sidebar.previewGitDirty"),
              previewGitConflicts: tr("sidebar.previewGitConflicts"),
              previewGitCountsCapped: tr(
                "sidebar.previewGitCountsCapped",
              ),
              unarchive: tr("sidebar.unarchive"),
              archive: tr("sidebar.archive"),
            },
            account: {
              trigger: tr("user.menu"),
              settings: tr("sidebar.settings"),
              theme: tr("user.theme"),
              themeLight: tr("user.themeLight"),
              themeDark: tr("user.themeDark"),
              local: tr("common.local"),
              signedIn: tr("account.signedIn"),
              signedOut: tr("account.signedOut"),
              login: tr("account.login"),
              logout: tr("account.logout"),
              remaining: tr("account.quotaRemaining"),
              usage: tr("account.usageSummary"),
              customProvider: tr("prov.customProvider"),
              resetsAt: tr("account.resetsAt"),
            },
          }}
          chrome={{
            useCustomWindowChrome,
            onHide: () =>
              setLayout((current) => {
                const next = { ...current, sidebarCollapsed: true };
                saveLayout(localStorage, next);
                return next;
              }),
            onToggleMaximize: () => void toggleMaximizeFromTitlebar(),
          }}
          navigation={{
            activePane: mainPane,
            onNewSession: () => void newChat(null),
            onSearch: () => {
              setShowSearch(true);
              setSearchQuery("");
            },
            onOpenAutomations: navigateAutomations,
            onOpenExtensions: navigatePlugins,
          }}
          tree={{
            projectsOpen,
            historyOpen,
            activeProjectId: activeProject?.id ?? null,
            activeSessionId: session.sessionId,
            busySessionIds,
            pendingAskSessionIds,
            projects: sidebarProjects,
            orphanSessions: sidebarOrphanSessions,
            groupBy: layout.sidebarGroupBy,
            sessionSort: layout.sidebarSessionSort,
            onToggleProjects: () => setProjectsOpen((open) => !open),
            onAddProject: () => void addProject(false),
            onToggleProject: (projectId, open) =>
              setExpandedProjects((current) => ({
                ...current,
                [projectId]: open,
              })),
            onSelectProject: (projectId) => {
              const project = projects.find((item) => item.id === projectId);
              if (project && !session.sessionId) setActiveProject(project);
            },
            onNewSessionInProject: (projectId) => {
              const project = projects.find((item) => item.id === projectId);
              if (project) void newChat(project);
            },
            onEditProject: (projectId) => {
              const project = projects.find((item) => item.id === projectId);
              if (project) editProject(project);
            },
            onOrganize: (next) => {
              setLayout((current) => {
                const updated = {
                  ...current,
                  sidebarGroupBy: next.groupBy,
                  sidebarSessionSort: next.sessionSort,
                };
                saveLayout(localStorage, updated);
                return updated;
              });
            },
            onTrustProject: (projectId) => {
              const project = projects.find((item) => item.id === projectId);
              if (project) void trustProject(project);
            },
            onProjectMenu: (event, projectId) => {
              const project = projects.find((item) => item.id === projectId);
              if (project) openProjectMenu(event, project);
            },
            onToggleHistory: () => setHistoryOpen((open) => !open),
            onOpenSession: (sessionId, projectId) => {
              const row = sessions.find((item) => item.id === sessionId);
              const project = projectId
                ? projects.find((item) => item.id === projectId)
                : undefined;
              if (row) void openSession(row, project);
            },
            onArchiveSession: (sessionId, archived) => {
              const row = sessions.find((item) => item.id === sessionId);
              if (row) void archiveSession(row, archived);
            },
            onSessionMenu: (event, sessionId) => {
              const row = sessions.find((item) => item.id === sessionId);
              if (row) openSessionMenu(event, row);
            },
            loadSessionPreview: api.sessionPreview,
          }}
          account={{
            open: showUserMenu,
            theme,
            account,
            activeProvider: activeCustomProvider,
            busy: accountBusy,
            customRouteActive,
            onClose: () => setShowUserMenu(false),
            onToggle: (open) => {
              setShowUserMenu(open);
              if (open) {
                void refreshAccount({
                  refreshBilling: !customRouteActive,
                });
              }
            },
            onSettings: () => navigateSettings("general"),
            onAccountSettings: () => navigateSettings("account"),
            onToggleTheme: toggleThemeBtn,
            onLogin: () => void runAccountLogin("oauth"),
            onLogout: () => void runAccountLogout(),
          }}
        />

        {/* CENTER — solid pane; top icons fully toggle L/R columns */}
        <main
          className={
            "main" +
            (layout.sidebarCollapsed ? " main--sidebar-hidden" : "") +
            (dragZone === "main" ? " is-drop-target" : "") +
            (dragZone === "sidebar" ? " is-drop-idle" : "")
          }
        >
          {dragZone === "main" && (
            <div className="drop-overlay drop-overlay--attach" aria-hidden>
              <div className="drop-overlay__card">
                <span className="drop-overlay__icon">
                  <IconAttach size={22} />
                </span>
                <strong>{tr("composer.dropAttachTitle")}</strong>
                <span>{tr("composer.dropAttachHint")}</span>
              </div>
            </div>
          )}
          {(() => {
            const currentSession = sessions.find(
              (candidate) => candidate.id === session.sessionId,
            );
            const automationTitle = mainPane === "automations";
            const pluginsTitle = mainPane === "plugins";
            const title = automationTitle
              ? tr("automations.title")
              : pluginsTitle
                ? tr("plugin.market.title")
              : currentSession?.title ||
                session.title ||
                activeProject?.name ||
                tr("session.new");
            const scheduled = !automationTitle && !!currentSession?.scheduled;
            const showConnection =
              mainPane === "chat" &&
              (connecting ||
                session.state === "connecting" ||
                session.state === "awaiting_permission" ||
                session.state === "disconnected");
            const retryReason = retryStatus?.reason || "";
            const retryLabel = retryStatus
              ? retryReason
                ? tr("main.retryingWithReason", {
                    attempt: String(retryStatus.attempt),
                    max: String(retryStatus.maxRetries),
                    reason:
                      retryReason.length > 72
                        ? `${retryReason.slice(0, 72)}…`
                        : retryReason,
                  })
                : tr("main.retrying", {
                    attempt: String(retryStatus.attempt),
                    max: String(retryStatus.maxRetries),
                  })
              : null;

            return (
              <WorkbenchTopbar
                title={title}
                automationTitle={automationTitle}
                scheduled={scheduled}
                scheduledLabel={tr("automations.msgTag")}
                sessionMenuLabel={tr("session.menu")}
                projectContext={!!currentSession?.projectId}
                sessionMenuOpen={
                  ctxMenu?.kind === "session" &&
                  ctxMenu.id === currentSession?.id
                }
                onOpenSessionMenu={
                  !automationTitle &&
                  !pluginsTitle &&
                  currentSession
                    ? (event) => openSessionMenu(event, currentSession)
                    : undefined
                }
                sidebarCollapsed={layout.sidebarCollapsed}
                showSidebarLabel={tr("main.leftPaneShow")}
                sidebarToggleRef={sidebarToggleRef}
                onShowSidebar={() =>
                  setLayout((current) => {
                    const next = { ...current, sidebarCollapsed: false };
                    saveLayout(localStorage, next);
                    return next;
                  })
                }
                asideCollapsed={layout.asideCollapsed}
                showAsideLabel={tr("main.rightPaneShow")}
                hideAsideLabel={tr("main.rightPaneHide")}
                asideToggleRef={asideToggleRef}
                onToggleAside={() =>
                  setLayout((current) => {
                    const next = {
                      ...current,
                      asideCollapsed: !current.asideCollapsed,
                    };
                    saveLayout(localStorage, next);
                    return next;
                  })
                }
                connection={
                  showConnection
                    ? {
                        pill: connPill,
                        label: tr(connPill.labelKey as MessageKey),
                      }
                    : null
                }
                retry={
                  retryLabel
                    ? {
                        label: retryLabel,
                        detail: retryReason || undefined,
                      }
                    : null
                }
                onTitlebarDoubleClick={() => {
                  if (useCustomWindowChrome) {
                    void toggleMaximizeFromTitlebar();
                  }
                }}
              />
            );
          })()}

          {mainPane === "plugins" ? (
            <PluginMarketplace
              locale={locale}
              skills={skillInfos.map((skill) => ({
                id: skill.id || skill.name,
                name: skill.name,
                description: skill.description,
              }))}
              onConnectorsChange={setConnectorStates}
              onUsePrompt={(text) => {
                setDraft(text);
                setAppView("workbench");
                setMainPane("chat");
                pendingComposerFocus.current = true;
                if (typeof window !== "undefined") {
                  window.history.replaceState(
                    null,
                    "",
                    window.location.pathname + window.location.search,
                  );
                }
              }}
            />
          ) : mainPane === "automations" ? (
            <AutomationsPage
              t={(k, vars) =>
                tr(k as Parameters<typeof tr>[0], vars as Record<string, string | number>)
              }
              projects={projects.map((p) => ({ id: p.id, name: p.name }))}
              defaultModelId={modelId}
              defaultEffort={effort}
              models={availableModels}
              onAiCreate={() => {
                void newChat(null, {
                  seedDraft: aiCreateSeedPrompt("Sunsetz"),
                  switchToChat: true,
                  automationSetup: true,
                });
                setToast(tr("automations.aiComposerHint"));
                window.setTimeout(() => setToast(null), 4200);
              }}
              onRunNow={(auto) => void runAutomation(auto)}
            />
          ) : (
          <>
          {columnNotice ? <Banner tone="warning">{columnNotice}</Banner> : null}
          {(isDeveloperMockBackend(session.backend) ||
            isDeveloperMockBackend(liveHost.backend)) && (
            <Banner tone="warning">{tr("runtime.mockBanner")}</Banner>
          )}
          <BackgroundApprovalBanner
            pendingSessionIds={pendingAskSessionIds}
            labels={{
              one: tr("banner.backgroundApprovalOne"),
              many: (n) => tr("banner.backgroundApprovalMany", { n: String(n) }),
              go: tr("banner.backgroundApprovalGo"),
            }}
            onGo={openSessionById}
          />
          {activeProject && !activeProject.trusted && (
            <div className="conn-bar">
              <button
                type="button"
                className="btn btn--primary"
                style={{ height: 24, fontSize: 11 }}
                onClick={() => void trustProject(activeProject)}
              >
                {tr("project.trustToSend", { name: activeProject.name })}
              </button>
            </div>
          )}

          {emptyExistingSession && (
            <div className="conn-bar" role="status">
              <span style={{ fontSize: 12, opacity: 0.85 }}>
                {tr("automations.emptySession")}
              </span>
            </div>
          )}

          {/* I06: pure stream silence — cancel or keep waiting */}
          {streamStall && mainPane === "chat" && (
            <div className="stall-banner" role="status">
              <div className="stall-banner__summary">
                {tr("agent.streamStallBanner", {
                  seconds: String(streamStall.stallSeconds),
                })}
              </div>
              <div className="stall-banner__actions">
                <button
                  type="button"
                  className="btn btn--ghost stall-banner__btn"
                  onClick={() => setStreamStall(null)}
                >
                  {tr("agent.streamStallKeepWaiting")}
                </button>
                <button
                  type="button"
                  className="btn btn--primary stall-banner__btn stall-banner__btn--danger"
                  onClick={() => {
                    setStreamStall(null);
                    void stop();
                  }}
                >
                  {tr("agent.streamStallCancel")}
                </button>
              </div>
            </div>
          )}

          {/* Pre-turn / host errors: T04 deck (problem · cause · primary · secondary) */}
          {errorBanner && !hasChatTurnError && (
            <div className="error-banner" role="alert">
              {errorBanner.code ? (
                <div className="error-banner__code">{errorBanner.code}</div>
              ) : null}
              <div className="error-banner__summary">{errorBanner.summary}</div>
              {errorBanner.cause ? (
                <div className="error-banner__cause">{errorBanner.cause}</div>
              ) : null}
              <div className="error-banner__actions">
                {errorBanner.primary ? (
                  <button
                    type="button"
                    className="btn btn--primary error-banner__primary"
                    disabled={
                      connecting && errorBanner.primary.id === "reconnect"
                    }
                    onClick={() => {
                      if (errorBanner.primary) {
                        runErrorBannerAction(errorBanner.primary);
                      }
                    }}
                  >
                    {errorBanner.primary.label}
                  </button>
                ) : null}
                {errorBanner.secondary ? (
                  <button
                    type="button"
                    className="btn btn--ghost error-banner__secondary"
                    disabled={
                      connecting && errorBanner.secondary.id === "reconnect"
                    }
                    onClick={() => {
                      if (errorBanner.secondary) {
                        runErrorBannerAction(errorBanner.secondary);
                      }
                    }}
                  >
                    {errorBanner.secondary.label}
                  </button>
                ) : null}
                {!errorBanner.primary &&
                  (errorBanner.reconnectHint ||
                    session.state === "disconnected") && (
                    <button
                      type="button"
                      className="btn btn--ghost error-banner__reconnect"
                      disabled={connecting}
                      onClick={() => {
                        setLocalError(null);
                        setErrorDetailOpen(false);
                        void ensureConnected(true).then((sid) => {
                          if (sid) setLocalError(null);
                        });
                      }}
                    >
                      {tr("main.reconnect")}
                    </button>
                  )}
                {errorBanner.detail ? (
                  <button
                    type="button"
                    className="error-banner__details-btn"
                    aria-expanded={errorDetailOpen}
                    onClick={() => setErrorDetailOpen((v) => !v)}
                  >
                    {errorDetailOpen
                      ? tr("error.hideDetails")
                      : tr("error.details")}
                  </button>
                ) : null}
              </div>
              {errorBanner.detail && errorDetailOpen && (
                <pre className="error-banner__detail">{errorBanner.detail}</pre>
              )}
            </div>
          )}

          <div
            className="main__stage"
            style={
              {
                ["--composer-float-pad"]: `${composerFloatPad}px`,
              } as CSSProperties
            }
          >
          <div className="sr-only" aria-live="polite" aria-atomic="true">
            {streamA11yNote}
          </div>
          <ConversationSurface
            locale={locale}
            sessionState={session.state}
            sessionKey={session.sessionId ?? `draft-${session.title ?? "new"}`}
            projectPath={activeProject?.path ?? null}
            suppressEmptyCopy={welcomeSession}
            canEditLastUser={canEditLastUser}
            lastUserMessageId={lastUserMessageId}
            editingUserMessageId={editingUserMessageId}
            editSubmitting={editSubmitting}
            editAttachments={editAttachments}
            onEditUserMessage={beginEditLastUser}
            onCancelEditUserMessage={cancelEditUser}
            onSubmitEditUserMessage={(msg, content) => {
              void submitEditLastUser(msg, content);
            }}
            onRemoveEditAttachment={(att) =>
              setEditAttachments((prev) =>
                prev.filter((x) => x.path !== att.path),
              )
            }
            canRewindSession={canRewindSession && !!session.sessionId}
            onRewindToUserMessage={onRewindToUserMessage}
            onForkFromUserMessage={onForkFromUserMessage}
            turnStartedAt={turnStartedAt}
            onOpenResource={(target) => {
              setLayout((l) => {
                if (l.asideCollapsed) {
                  const n = { ...l, asideCollapsed: false };
                  saveLayout(localStorage, n);
                  return n;
                }
                return l;
              });
              setResourceOpenTarget(target);
            }}
            onAddAttachmentToComposer={(att) =>
              setAttachments((prev) => mergeAttachments(prev, [att]))
            }
            planArtifact={
              plan.body || plan.entries.length
                ? {
                    visible: true,
                    title: plan.title,
                    body: plan.body,
                    entries: plan.entries,
                    waiting: plan.waiting,
                    artifactStatus: plan.artifactStatus,
                  }
                : null
            }
            onOpenPlanArtifact={openPlanInResource}
            sessionChanges={
              sessionChangesById[session.sessionId || ""] ?? []
            }
            onOpenTurnChanges={() => {
              setLayout((l) => {
                if (l.asideCollapsed) {
                  const n = { ...l, asideCollapsed: false };
                  saveLayout(localStorage, n);
                  return n;
                }
                return l;
              });
              setResourceOpenTarget({ type: "changes" });
            }}
            attachLabels={attachLabels}
          />

          {welcomeSession ? (
            <div
              className="composer-empty-hero"
              data-brand={welcomeBrandKind ?? "standard"}
            >
              <div className="composer-empty-hero__content">
                <SunsetzLogo size={38} />
                <h2 className="composer-empty-hero__title">
                  {activeProject?.name
                    ? tr("main.welcomeProject", {
                        project: activeProject.name,
                      })
                    : tr("main.welcomeGeneral")}
                </h2>
                <div className="composer-empty-hero__cards">
                  {(
                    [
                      {
                        id: "explore",
                        icon: <IconSearch size={18} />,
                        title: tr("welcome.explore"),
                        prompt: tr("welcome.explorePrompt"),
                      },
                      {
                        id: "build",
                        icon: <IconImagine size={18} />,
                        title: tr("welcome.build"),
                        prompt: tr("welcome.buildPrompt"),
                      },
                      {
                        id: "review",
                        icon: <IconNotes size={18} />,
                        title: tr("welcome.review"),
                        prompt: tr("welcome.reviewPrompt"),
                      },
                      {
                        id: "fix",
                        icon: <IconAlertTriangle size={18} />,
                        title: tr("welcome.fix"),
                        prompt: tr("welcome.fixPrompt"),
                      },
                    ] as const
                  ).map((card) => (
                    <button
                      key={card.id}
                      type="button"
                      className="composer-empty-hero__card"
                      onClick={() => {
                        setDraft(card.prompt);
                        requestComposerFocus();
                      }}
                    >
                      <span className="composer-empty-hero__card-icon" aria-hidden>
                        {card.icon}
                      </span>
                      <span>{card.title}</span>
                    </button>
                  ))}
                </div>
              </div>
            </div>
          ) : null}

          <ComposerDock
            locale={locale}
              welcomeSession={welcomeSession}
              goalMode={goalMode}
              settingsLocked={composerSettingsLocked}
              sessionState={session.state}
              connecting={connecting}
              dropReady={dragZone === "main"}
              draft={draft}
              attachments={attachments}
              attachmentLabels={attachLabels}
              contextUsage={contextUsageDisplay}
              progress={
                taskProgressVisible
                  ? {
                      entries: plan.entries,
                      changes:
                        sessionChangesById[session.sessionId || ""] ?? [],
                      goalSummary: taskGoalSummary,
                      elapsedMs:
                        turnStartedAt == null
                          ? null
                          : Math.max(0, Date.now() - turnStartedAt),
                      streaming: session.state === "streaming",
                      labels: {
                        step: tr("taskProgress.step"),
                        filesChanged: tr("taskProgress.filesChanged"),
                        activeGoal: tr("taskProgress.activeGoal"),
                        details: tr("taskProgress.details"),
                        edit: tr("taskProgress.edit"),
                        pause: tr("taskProgress.pause"),
                        delete: tr("taskProgress.delete"),
                      },
                      onOpenDetails: openPlanInResource,
                    }
                  : null
              }
              permission={
            perm ? (
              <div
                ref={permBarRef}
                className={
                  "perm-bar" + (perm.destructive ? " perm-bar--destructive" : "")
                }
                role="region"
                aria-labelledby="perm-bar-title"
                aria-describedby="perm-bar-summary"
              >
                <div className="sr-only" aria-live="assertive">
                  {tr("a11y.permissionNeeded")}
                </div>
                <div className="perm-bar__head">
                  <span className="perm-bar__badge" id="perm-bar-title">
                    {tr("perm.title")}
                  </span>
                  <span className="perm-bar__tool">
                    {perm.title || perm.toolName}
                  </span>
                </div>
                <p className="perm-bar__summary" id="perm-bar-summary">
                  {formatPermissionSummary({
                    toolName: perm.toolName,
                    title: perm.title,
                    command: perm.preview,
                  })}
                </p>
                {perm.destructive ? (
                  <p className="perm-bar__destructive-warning">
                    {tr("perm.destructiveWarning")}
                  </p>
                ) : null}
                {perm.preview?.trim() ? (
                  <div className="perm-bar__preview-wrap">
                    <pre
                      className={
                        "perm-bar__preview" +
                        (permPreviewExpanded ? " is-expanded" : "")
                      }
                    >
                      {perm.preview.trim()}
                    </pre>
                    {perm.preview.trim().split("\n").length > 3 ||
                    perm.preview.trim().length > 220 ? (
                      <button
                        type="button"
                        className="perm-bar__preview-toggle"
                        onClick={() =>
                          setPermPreviewExpanded((expanded) => !expanded)
                        }
                      >
                        {permPreviewExpanded
                          ? tr("perm.previewCollapse")
                          : tr("perm.previewExpand")}
                      </button>
                    ) : null}
                  </div>
                ) : null}
                <div className="perm-bar__actions" role="group">
                  {mapPermissionButtons(perm.options, {
                    allowOnce: tr("perm.allowOnce"),
                    allowSession: tr("perm.allowSession"),
                    deny: tr("perm.deny"),
                  }).map((btn) => (
                    <button
                      key={btn.decision + btn.optionId}
                      type="button"
                      className={
                        "perm-bar__btn" +
                        (btn.decision === "allow_once"
                          ? " perm-bar__btn--allow"
                          : btn.decision === "deny"
                            ? " perm-bar__btn--deny"
                            : " perm-bar__btn--session")
                      }
                      title={
                        btn.decision === "allow_once"
                          ? tr("perm.hintOnce")
                          : btn.decision === "allow_session"
                            ? tr("perm.hintSession")
                            : tr("perm.hintDeny")
                      }
                      onClick={() =>
                        void api
                          .sessionResolvePermission({
                            interactionId: perm.interactionId,
                            sessionId: perm.sessionId,
                            rpcId: perm.rpcId,
                            decision: btn.decision,
                            optionId: btn.optionId,
                            scopeKey: perm.scopeKey,
                          })
                          .then(() => setPerm(null))
                      }
                    >
                      {btn.label}
                    </button>
                  ))}
                </div>
              </div>
            ) : null}
            takeover={
            askUser ? (
              <AskUserDock
                payload={askUser}
                labels={{
                  title: tr("askUser.title"),
                  submit: tr("askUser.submit"),
                  cancel: tr("askUser.cancel"),
                  otherPlaceholder: tr("askUser.otherPlaceholder"),
                  freeTextHint: tr("askUser.freeTextHint"),
                  multiHint: tr("askUser.multiHint"),
                  close: tr("common.close"),
                  previous: tr("askUser.previous"),
                  next: tr("askUser.next"),
                  skip: tr("askUser.skip"),
                  progress: tr("askUser.progress"),
                  recommended: tr("askUser.recommended"),
                  submitFailed: tr("askUser.submitFailed"),
                }}
                onSubmit={async (answers) => {
                  await api.sessionResolveAskUser({
                    interactionId: askUser.interactionId,
                    sessionId: askUser.sessionId ?? session.sessionId,
                    decision: "accepted",
                    answers,
                    rpcId: askUser.rpcId,
                  });
                  setPendingAskSessionIds((current) => {
                    const next = new Set(current);
                    next.delete(askUser.sessionId);
                    return next;
                  });
                  setAskUser(null);
                }}
                onCancel={async () => {
                  await api.sessionResolveAskUser({
                    interactionId: askUser.interactionId,
                    sessionId: askUser.sessionId ?? session.sessionId,
                    decision: "cancelled",
                    rpcId: askUser.rpcId,
                  });
                  setPendingAskSessionIds((current) => {
                    const next = new Set(current);
                    next.delete(askUser.sessionId);
                    return next;
                  });
                  setAskUser(null);
                }}
              />
            ) : planApprovalPayload ? (
              <AskUserDock
                payload={planApprovalPayload}
                labels={{
                  title: tr("plan.ready"),
                  submit: tr("plan.changes"),
                  cancel: tr("plan.dismiss"),
                  otherPlaceholder: tr("plan.confirmChangesPlaceholder"),
                  freeTextHint: tr("plan.confirmChangesHint"),
                  multiHint: "",
                  close: tr("common.close"),
                  previous: tr("askUser.previous"),
                  next: tr("askUser.next"),
                  skip: tr("askUser.skip"),
                  progress: tr("askUser.progress"),
                  recommended: tr("askUser.recommended"),
                  submitFailed: tr("askUser.submitFailed"),
                }}
                onSubmit={async (answers) => {
                  const answer = Object.values(answers)[0]?.trim() || "";
                  if (answer === tr("plan.confirmApprove")) {
                    await api.sessionResolvePlan({
                      ...planResolutionContext(
                        plan.interactionId,
                        session.sessionId,
                      ),
                      decision: "approved",
                      rpcId: planApprovalPayload.rpcId,
                    });
                    setPlan((current) => ({
                      ...current,
                      visible: true,
                      waiting: false,
                      rpcId: null,
                      liveReview: false,
                      artifactStatus: "approved",
                    }));
                    showToast(tr("plan.approvedToast"), 2500);
                    return;
                  }
                  if (answer) {
                    await api.sessionResolvePlan({
                      ...planResolutionContext(
                        plan.interactionId,
                        session.sessionId,
                      ),
                      decision: "cancelled",
                      feedback: answer,
                      rpcId: planApprovalPayload.rpcId,
                    });
                    setPlan((current) => ({
                      ...current,
                      visible: true,
                      waiting: false,
                      rpcId: null,
                      liveReview: false,
                      artifactStatus: "revision_requested",
                    }));
                    showToast(tr("plan.reviseToast"), 2800);
                    return;
                  }
                  await api.sessionResolvePlan({
                    ...planResolutionContext(
                      plan.interactionId,
                      session.sessionId,
                    ),
                    decision: "abandoned",
                    rpcId: planApprovalPayload.rpcId,
                  });
                  setPlan({
                    ...emptyPlanState(tr("resources.plan")),
                    artifactStatus: "abandoned",
                  });
                }}
                onCancel={async () => {
                  await api.sessionResolvePlan({
                    ...planResolutionContext(
                      plan.interactionId,
                      session.sessionId,
                    ),
                    decision: "abandoned",
                    rpcId: planApprovalPayload.rpcId,
                  });
                  setPlan({
                    ...emptyPlanState(tr("resources.plan")),
                    artifactStatus: "abandoned",
                  });
                }}
              />
            ) : null
              }
              project={{
                active: activeProject,
                options: projects,
                openRequestKey: projectMenuOpenKey,
                worktrees: gitWorktrees,
                worktreesAvailable: gitWorktreesAvailable,
                worktreesLoading: gitWorktreesLoading,
                worktreesReason: gitWorktreesReason,
                onSelect: (project) => {
                  if (composerSettingsLocked) return;
                  void bindSessionProject(project);
                },
                onAdd: () => {
                  if (composerSettingsLocked) return;
                  void addProjectFromPicker({ bindSession: true });
                },
                onSwitchWorktree: (worktree) => {
                  if (composerSettingsLocked) return;
                  void switchToWorktree(worktree);
                },
                onCreateWorktree: (branchName, createBranch) => {
                  if (composerSettingsLocked) return;
                  void createWorktree(branchName, createBranch);
                },
                onRemoveWorktree: (worktree) => {
                  if (composerSettingsLocked) return;
                  void removeWorktree(worktree);
                },
                onOpen: refreshGitWorktrees,
              }}
              queue={{
                items: sendQueue.activeQueue,
                flushHold: sendQueue.flushHold,
                previewLabels: queuePreviewLabels,
                onClear: sendQueue.clearQueue,
                onRemove: sendQueue.removeItem,
                onRetry: sendQueue.resumeFlush,
                onSteer: () => {
                  sendQueue.releaseFlushHold();
                  void stop().then(() => sendQueue.resumeFlush());
                },
                onEdit: (id) => {
                  const item = sendQueue.activeQueue.find((row) => row.id === id);
                  if (!item) return;
                  sendQueue.removeItem(id);
                  setDraft(item.storedDisplay);
                  setAttachments(item.attachments);
                },
                onPause: sendQueue.pauseAutoSend,
              }}
              hostedJobs={{
                items: hostedCommandJobs.jobsForSession,
                labels: {
                  count: (n) => tr("composer.hostedJobsCount", { n: String(n) }),
                  running: tr("composer.hostedJobsRunning"),
                  completed: tr("composer.hostedJobsCompleted"),
                  failed: tr("composer.hostedJobsFailed"),
                  cancelled: tr("composer.hostedJobsCancelled"),
                },
              }}
              projectInstruction={
                projectInstruction?.relativePath
                  ? {
                      path: projectInstruction.relativePath,
                      truncated: projectInstruction.truncated,
                      labels: {
                        attached: tr("composer.projectInstruction.attached", {
                          path: projectInstruction.relativePath,
                        }),
                        truncated: tr(
                          "composer.projectInstruction.truncated",
                          { path: projectInstruction.relativePath },
                        ),
                      },
                    }
                  : null
              }
              connectors={{
                items: CONNECTOR_CATALOG.filter((entry) =>
                  connectorStates.some(
                    (row) => row.id === entry.id && row.connected,
                  ),
                ).map((entry) => ({
                  id: entry.id,
                  name: tr(entry.nameKey as MessageKey),
                })),
                openLabel: tr("plugin.market.title"),
                onOpen: () => {
                  setAppView("workbench");
                  setMainPane("plugins");
                },
              }}
              memory={{
                pack: pendingMemoryContext,
                labels: {
                  regionLabel: tr("composer.memory.region"),
                  title: tr("composer.memory.title"),
                  reviewedContext: tr("composer.memory.reviewed"),
                  notInstructions: tr("composer.memory.notInstructions"),
                  noFtsSessionEvidence: tr("composer.memory.noFts"),
                  itemCount: tr("composer.memory.count"),
                  clear: tr("composer.memory.clear"),
                  content: tr("composer.memory.content"),
                  expandItem: tr("composer.memory.expand"),
                  fullContent: tr("composer.memory.full"),
                  emptyContent: tr("composer.memory.empty"),
                  provenance: tr("composer.memory.provenance"),
                  typeLabels: {
                    user_preference: tr("settings.memory.type.preference"),
                    project_fact: tr("settings.memory.type.projectFact"),
                    workflow_hint: tr("settings.memory.type.workflowHint"),
                  },
                },
                onClear: () => {
                  memoryPackRestoreGenRef.current += 1;
                  setPendingMemoryContext(null);
                },
              }}
              menu={{
                open: composerMenuOpen,
                positioned: composerPlusPos != null,
                plusMode: plusMenuMode,
                showPlus: showComposerPlus,
                liveSlashPresent: liveSlash.present,
                liveAtPresent: liveAt.present,
                slashFilterQuery,
                atFilterQuery,
                skillsLoading,
                activeIndex: slashActiveIndex,
                entries: composerMenuEntries,
                style: composerPlusStyle,
                onActiveIndexChange: setSlashActiveIndex,
                onPickFiles: pickComposerFiles,
                onSelectAction: selectComposerPlusAction,
                onSelectSlash: applySlashItem,
                onSelectConnector: applyConnectorMention,
                resolveTitle: resolveSlashTitle,
                resolveDescription: resolveSlashDescription,
                onClose: closeComposerMenu,
                onTogglePlus: () => {
                  if (composerMenuOpen) {
                    closeComposerMenu();
                  } else {
                    setFinderSelectionFeedback(null);
                    setShowComposerPlus(true);
                  }
                },
              }}
              preferences={{
                mode,
                policy,
                modelId,
                effort,
                models: availableModels,
                onMode,
                onPolicy,
                onDisablePlan,
                onClearGoal: () => {
                  if (composerSettingsLocked) return;
                  setGoalMode(false);
                },
                onModel,
                onEffort,
                onReset,
              }}
              refs={{
                wrap: composerWrapRef,
                input: composerInputRef,
                shell: composerShellRef,
                plusTrigger: composerPlusTriggerRef,
                plusPanel: composerPlusPanelRef,
                menuEntries: composerMenuEntriesRef,
              }}
              onDraftChange={setDraft}
              onRemoveAttachment={(attachment) =>
                setAttachments((current) =>
                  current.filter((item) => item.path !== attachment.path),
                )
              }
              onAddAttachment={(attachment) =>
                setAttachments((current) =>
                  mergeAttachments(current, [attachment]),
                )
              }
              onPasteFiles={addAttachmentsFromFiles}
              onPasteMediaFallback={pasteMediaFromNativeClipboard}
              onSlashQueryChange={onSlashQueryChange}
              onAtQueryChange={onAtQueryChange}
              onCompact={() => {
                if (composerSettingsLocked) return;
                setCompactNote("");
                setShowCompactModal(true);
              }}
              onSend={send}
              onStop={stop}
            />
          </div>
          </>
          )}
        </main>

        {/* RIGHT — session-linked project resource viewer (fully hideable + resizable) */}
        <aside
          className={
            (layout.asideCollapsed ? "aside aside--hidden" : "aside") +
            (resizingAside ? " is-resizing" : "")
          }
          aria-hidden={layout.asideCollapsed}
          inert={layout.asideCollapsed ? true : undefined}
          style={
            !layout.asideCollapsed
              ? {
                  width: layout.asideWidth,
                  minWidth: layout.asideWidth,
                  maxWidth: layout.asideWidth,
                }
              : undefined
          }
        >
          {!layout.asideCollapsed && (
            <div
              className="aside-resizer"
              role="separator"
              aria-orientation="vertical"
              aria-label={tr("resources.resizeTree")}
              aria-valuemin={ASIDE_WIDTH_MIN}
              aria-valuemax={ASIDE_WIDTH_MAX}
              aria-valuenow={layout.asideWidth}
              tabIndex={0}
              onPointerDown={(e) => {
                e.preventDefault();
                setResizingAside(true);
              }}
              onKeyDown={(event) => {
                let next: number | null = null;
                if (event.key === "ArrowLeft") {
                  next = layout.asideWidth - 16;
                } else if (event.key === "ArrowRight") {
                  next = layout.asideWidth + 16;
                } else if (event.key === "Home") {
                  next = ASIDE_WIDTH_MIN;
                } else if (event.key === "End") {
                  next = ASIDE_WIDTH_MAX;
                }
                if (next == null) return;
                event.preventDefault();
                const width = clampAsideWidth(next);
                setLayout((current) => {
                  const updated = {
                    ...current,
                    asideWidth: width,
                    asideCollapsed: false,
                  };
                  saveLayout(localStorage, updated);
                  return updated;
                });
              }}
            />
          )}
          {!layout.asideCollapsed ? (
            <div className="aside__inner">
              <Suspense
                fallback={
                  <div className="rp__empty-state" role="status">
                    <div className="rp__empty-desc">
                      {tr("resources.loading")}
                    </div>
                  </div>
                }
              >
                <ResourceViewer
                  projectPath={activeProject?.path ?? null}
                  projectName={activeProject?.name ?? null}
                  locale={locale}
                  paneActive
                  openRequest={resourceOpenTarget}
                  onOpenRequestConsumed={() => setResourceOpenTarget(null)}
                  sessionChanges={
                    sessionChangesById[session.sessionId || ""] ?? []
                  }
                  plan={plan}
                  planFocusKey={planFocusKey}
                  onClose={() =>
                    setLayout((l) => {
                      const n = { ...l, asideCollapsed: true };
                      saveLayout(localStorage, n);
                      return n;
                    })
                  }
                />
              </Suspense>
            </div>
          ) : null}
        </aside>
      </WorkbenchShell>
      ))}

      <SkillRecorderSheet
        open={skillRecorderOpen}
        projectPath={activeProject?.path ?? null}
        draft={skillCandidate?.draft ?? null}
        labels={{
          title: tr("skillRecorder.title"),
          close: tr("common.close"),
          sourceRange: tr("skillRecorder.sourceRange"),
          start: tr("skillRecorder.start"),
          end: tr("skillRecorder.end"),
          userRole: tr("skillRecorder.userRole"),
          assistantRole: tr("skillRecorder.assistantRole"),
          activityRole: tr("skillRecorder.activityRole"),
          noMaterial: tr("skillRecorder.noMaterial"),
          materialTooLarge: tr("skillRecorder.materialTooLarge"),
          visibleRequest: tr("skillRecorder.visibleRequest"),
          generate: tr("skillRecorder.generate"),
          generating: tr("skillRecorder.generating"),
          regenerate: tr("skillRecorder.regenerate"),
          retry: tr("skillRecorder.retry"),
          draft: tr("skillRecorder.draft"),
          name: tr("skillRecorder.name"),
          nameHint: tr("skillRecorder.nameHint"),
          description: tr("skillRecorder.description"),
          descriptionHint: tr("skillRecorder.descriptionHint"),
          skillMd: tr("skillRecorder.skillMd"),
          skillMdHint: tr("skillRecorder.skillMdHint"),
          references: tr("skillRecorder.references"),
          addReference: tr("skillRecorder.addReference"),
          removeReference: tr("skillRecorder.removeReference"),
          referencePath: tr("skillRecorder.referencePath"),
          referenceContent: tr("skillRecorder.referenceContent"),
          noReferences: tr("skillRecorder.noReferences"),
          scope: tr("skillRecorder.scope"),
          projectScope: tr("skillRecorder.projectScope"),
          projectScopeHint: tr("skillRecorder.projectScopeHint"),
          userScope: tr("skillRecorder.userScope"),
          userScopeHint: tr("skillRecorder.userScopeHint"),
          save: tr("skillRecorder.save"),
          saving: tr("skillRecorder.saving"),
          saved: tr("skillRecorder.saved"),
          validationFailed: tr("skillRecorder.validationFailed"),
          overwriteTitle: tr("skillRecorder.overwriteTitle"),
          overwriteBody: tr("skillRecorder.overwriteBody"),
          overwriteConfirm: tr("skillRecorder.overwriteConfirm"),
          overwriteCancel: tr("skillRecorder.overwriteCancel"),
        }}
        onGenerate={generateSkillDraft}
        onSave={async (draft, scope, overwrite) => {
          try {
            const projectPath =
              scope === "project" ? activeProject?.path ?? null : null;
            if (skillCandidate) {
              const request = {
                id: skillCandidate.id,
                scope,
                projectPath,
                draft,
                overwrite,
                userConfirmedOverwrite: overwrite,
              };
              if (skillCandidate.reviewContentHash) {
                const finalContentHash = await skillDraftContentHash(draft);
                await api.skillCandidateApproveV2({
                  ...request,
                  expectedContentHash: skillCandidate.reviewContentHash,
                  finalContentHash,
                });
              } else {
                await api.skillCandidateApproveV1(request);
              }
            } else {
              await api.skillDraftSave({
                ...draft,
                scope,
                projectPath,
                overwrite,
              });
            }
            return { status: "saved" as const };
          } catch (error) {
            const detail = String(error);
            if (detail.includes("SKILL_EXISTS:")) {
              return {
                status: "conflict" as const,
                message: tr("skillRecorder.overwriteBody"),
              };
            }
            throw error instanceof Error ? error : new Error(detail);
          }
        }}
        onSaved={() => {
          setSkillCandidate(null);
          setSkillsReloadToken((token) => token + 1);
          showToast(tr("skillRecorder.savedToast"), 3200);
        }}
        onClose={() => setSkillRecorderOpen(false)}
      />

      <DoctorModal
        open={showDoctor}
        onClose={() => setShowDoctor(false)}
        locale={locale}
        onConfirm={({ title, message, confirmLabel, danger, onConfirm }) => {
          setAppDialog({
            kind: "confirm",
            title,
            message,
            confirmLabel,
            danger,
            onConfirm,
          });
        }}
        onResetDone={() => {
          void refreshLists();
        }}
      />
      <GlassModal
        open={showShortcuts}
        onClose={() => setShowShortcuts(false)}
        title={tr("shortcuts.title")}
        size="md"
        closeLabel={tr("shortcuts.close")}
        footer={
          <button
            type="button"
            className="btn btn--ghost"
            onClick={() => setShowShortcuts(false)}
          >
            {tr("shortcuts.close")}
          </button>
        }
      >
        <ul className="shortcuts-list">
          {shortcutsForPlatform(
            platform === "mac" ? "mac" : platform === "win" ? "win" : "other",
          ).map((row) => (
            <li key={row.id} className="shortcuts-list__row">
              <span className="shortcuts-list__label">
                {tr(row.labelKey as MessageKey)}
              </span>
              <kbd className="shortcuts-list__keys">{row.keys}</kbd>
            </li>
          ))}
        </ul>
      </GlassModal>
      <StatusModal
        open={showStatusModal}
        locale={locale}
        sessionId={session.sessionId}
        agentSessionId={session.agentSessionId}
        modelId={modelId}
        effort={effort}
        mode={mode}
        policy={policy}
        projectPath={activeProject?.path}
        messageCount={transcriptMeta.length}
        onClose={closeStatusModal}
      />
      <McpStatusModal
        open={showMcpModal}
        locale={locale}
        servers={mcpServers}
        error={mcpError}
        loading={mcpLoading}
        onClose={closeMcpModal}
        onManage={() => navigateSettings("extensions")}
      />
      {rewindTimeline && (
        <div
          className="overlay"
          role="presentation"
          onClick={() => {
            if (!rewindBusy) setRewindTimeline(null);
          }}
        >
          <div
            className="modal rewind-modal"
            role="dialog"
            aria-modal="true"
            aria-labelledby="rewind-modal-title"
            onClick={(e) => e.stopPropagation()}
          >
            <header className="modal-head">
              <h2 id="rewind-modal-title" className="modal-title">
                {tr("session.rewindTitle")}
              </h2>
              <button
                type="button"
                className="icon-btn modal-close"
                onClick={() => setRewindTimeline(null)}
                aria-label={tr("common.close")}
                disabled={rewindBusy}
              >
                <IconClose size={16} />
              </button>
            </header>
            <p className="rewind-modal__msg">{tr("session.rewindHint")}</p>
            <div className="rewind-modal__list" role="list">
              {rewindTimeline.points.map((p) => {
                const isLast =
                  p.promptIndex ===
                  rewindTimeline.points[rewindTimeline.points.length - 1]
                    ?.promptIndex;
                return (
                  <button
                    key={`${p.promptIndex}-${p.messageId ?? ""}`}
                    type="button"
                    role="listitem"
                    className="rewind-modal__item"
                    disabled={rewindBusy || isLast}
                    title={
                      isLast
                        ? tr("session.rewindNoop")
                        : tr("message.rewindHere")
                    }
                    onClick={() => {
                      if (isLast) {
                        showToast(tr("session.rewindNoop"));
                        return;
                      }
                      confirmRewindToPrompt(
                        rewindTimeline.sessionId,
                        p.promptIndex,
                        p.preview,
                      );
                    }}
                  >
                    <span className="rewind-modal__idx">
                      #{p.promptIndex + 1}
                    </span>
                    <span className="rewind-modal__preview">
                      {p.preview || "…"}
                    </span>
                  </button>
                );
              })}
            </div>
            <div className="modal-actions">
              <button
                type="button"
                className="btn btn--ghost"
                disabled={rewindBusy}
                onClick={() => setRewindTimeline(null)}
              >
                {tr("common.cancel")}
              </button>
            </div>
          </div>
        </div>
      )}

      {showCompactModal && (
        <div
          className="overlay"
          role="presentation"
          onClick={() => {
            setShowCompactModal(false);
            setCompactNote("");
          }}
        >
          <form
            className="modal compact-modal"
            onClick={(e) => e.stopPropagation()}
            role="dialog"
            aria-modal="true"
            aria-labelledby="compact-modal-title"
            onSubmit={(e) => {
              e.preventDefault();
              if (composerSettingsLocked) return;
              const note = compactNote;
              setShowCompactModal(false);
              setCompactNote("");
              void (async () => {
                const cmd = note.trim()
                  ? `/compact ${note.trim()}`
                  : "/compact";
                try {
                  const sid = await ensureConnected();
                  if (!sid) return;
                  await api.sessionSend(cmd);
                } catch (err) {
                  setLocalError(String(err));
                }
              })();
            }}
          >
            <header className="modal-head">
              <h2 id="compact-modal-title" className="modal-title">
                {tr("slash.compact")}
              </h2>
              <button
                type="button"
                className="icon-btn modal-close"
                onClick={() => {
                  setShowCompactModal(false);
                  setCompactNote("");
                }}
                aria-label={tr("common.close")}
              >
                <IconClose size={16} />
              </button>
            </header>
            <p className="compact-modal__msg">
              {tr("slash.compactConfirm")}
            </p>
            <input
              ref={compactNoteRef}
              className="compact-modal__field"
              value={compactNote}
              onChange={(e) => setCompactNote(e.target.value)}
              placeholder={tr("slash.compactNote")}
              autoFocus
              autoComplete="off"
            />
            <div className="modal-actions">
              <button
                type="button"
                className="btn btn--ghost"
                onClick={() => {
                  setShowCompactModal(false);
                  setCompactNote("");
                }}
              >
                {tr("slash.compactConfirmCancel")}
              </button>
              <button type="submit" className="btn btn--solid">
                {tr("slash.compactConfirmOk")}
              </button>
            </div>
          </form>
        </div>
      )}

      {/* Search / command palette (Codex-style) */}
      {showSearch && (
        <div
          className="overlay"
          onClick={() => setShowSearch(false)}
        >
          <div
            className="search-panel"
            onClick={(e) => e.stopPropagation()}
            role="dialog"
            aria-label={tr("sidebar.search")}
          >
            <div className="search-panel__head">
              <IconSearch size={16} />
              <input
                autoFocus
                className="search-panel__input"
                placeholder={
                  tr("search.placeholder")
                }
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
              />
              <button
                type="button"
                className="icon-btn modal-close"
                onClick={() => setShowSearch(false)}
                aria-label={tr("common.close")}
              >
                <IconClose size={16} />
              </button>
            </div>
            {searchHits.matchedProjects.length > 0 && (
              <>
                <div className="search-panel__section">
                  {tr("sidebar.projects")}
                </div>
                {searchHits.matchedProjects.map((p) => (
                  <button
                    key={p.id}
                    type="button"
                    className="search-panel__row"
                    onClick={() => {
                      setShowSearch(false);
                      // Project is a folder: expand only; selection is for sessions.
                      setProjectsOpen(true);
                      setExpandedProjects((e) => ({ ...e, [p.id]: true }));
                    }}
                  >
                    <IconFolder size={15} />
                    <span className="search-panel__title">{p.name}</span>
                    <span className="search-panel__meta">{p.path}</span>
                  </button>
                ))}
              </>
            )}
            <div className="search-panel__section">
              {tr("search.chats")}
            </div>
            {searchHits.matchedSessions.length === 0 && (
              <div className="sidebar-empty" style={{ padding: 12 }}>
                {tr("search.noMatches")}
              </div>
            )}
            {searchHits.matchedSessions.map((hit, i) => {
              const s = sessions.find((x) => x.id === hit.id);
              if (!s) return null;
              const proj = projects.find((p) => p.id === s.projectId);
              const contentHit = contentSearchHits.find(
                (candidate) => candidate.sessionId === s.id,
              );
              return (
                <button
                  key={s.id}
                  type="button"
                  className="search-panel__row"
                  onClick={() => {
                    setShowSearch(false);
                    void openSession(s, proj ?? null);
                  }}
                >
                  <IconSquarePen size={15} />
                  <span className="search-panel__title">
                    {s.title || tr("session.untitled")}
                  </span>
                  <span className="search-panel__meta">
                    {contentHit?.snippet || proj?.name || "—"}
                    {i < 9 ? `  ⌘${i + 1}` : ""}
                  </span>
                </button>
              );
            })}
            <div className="search-panel__foot">
              <button
                type="button"
                className="search-panel__row"
                onClick={() => {
                  setShowSearch(false);
                  void newChat(activeProject);
                }}
              >
                <IconSquarePen size={15} />
                <span className="search-panel__title">
                  {tr("search.newChat")}
                </span>
              </button>
              <button
                type="button"
                className="search-panel__row"
                onClick={() => {
                  setShowSearch(false);
                  void addProject(false);
                }}
              >
                <IconFolder size={15} />
                <span className="search-panel__title">
                  {tr("sidebar.addProject")}
                </span>
              </button>
            </div>
          </div>
        </div>
      )}

      <AppDialogHost
        appDialog={appDialog}
        setAppDialog={setAppDialog}
        dialogInput={dialogInput}
        setDialogInput={setDialogInput}
        dialogPath={dialogPath}
        setDialogPath={setDialogPath}
        dialogInputRef={dialogInputRef}
        confirmBtnRef={confirmBtnRef}
        appDialogRef={appDialogRef}
        tr={tr}
      />

      {/* Floating context menu (project / session) — unified ContextMenu */}
      {(() => {
        let items: ContextMenuItem[] = [];
        if (ctxMenu?.kind === "project") {
          const proj = projects.find((p) => p.id === ctxMenu.id);
          if (proj) {
            items = [
              {
                id: "pin",
                label: proj.pinned
                  ? tr("project.unpin")
                  : tr("project.pin"),
                icon: proj.pinned ? (
                  <IconPinOff size={16} />
                ) : (
                  <IconPin size={16} />
                ),
                onClick: () => {
                  void api
                    .projectSetPinned(proj.id, !proj.pinned)
                    .then(() => refreshProjects());
                },
              },
              {
                id: "rename",
                label: tr("project.edit"),
                icon: <IconRename size={16} />,
                onClick: () => editProject(proj),
              },
              {
                id: "reveal",
                label: tr("project.reveal"),
                icon: <IconExternalLink size={16} />,
                separatorBefore: true,
                onClick: () => {
                  void api
                    .projectReveal(proj.id)
                    .catch((e) => setLocalError(String(e)));
                },
              },
              ...(proj.trusted
                ? [
                    {
                      id: "permission",
                      label: tr("project.permission"),
                      icon: <IconShield size={16} />,
                      onClick: () => {
                        setCtxMenu({
                          kind: "project-policy",
                          id: proj.id,
                          x: ctxMenu.x,
                          y: ctxMenu.y,
                        });
                      },
                    } satisfies ContextMenuItem,
                  ]
                : []),
              {
                id: "archive-chats",
                label: tr("project.archiveChats"),
                icon: <IconArchive size={16} />,
                separatorBefore: true,
                onClick: () => {
                  void archiveProjectSessions(proj);
                },
              },
              {
                id: "remove",
                label: tr("project.remove"),
                icon: <IconTrash size={16} />,
                danger: true,
                onClick: () => removeProjectFromApp(proj),
              },
            ];
          }
        } else if (ctxMenu?.kind === "project-policy") {
          const proj = projects.find((p) => p.id === ctxMenu.id);
          if (proj && proj.trusted) {
            const current = proj.permissionPolicy?.trim() || null;
            const policyLabel = (id: PermissionPolicyId) =>
              tr(
                (
                  {
                    ask: "policy.ask",
                    accept_edits: "policy.accept_edits",
                    allow_for_session: "policy.allow_for_session",
                    dont_ask: "policy.dont_ask",
                    always_approve: "policy.always_approve",
                  } as const
                )[id],
              );
            items = [
              {
                id: "inherit",
                label: tr("project.permissionInherit"),
                icon: !current ? <IconCheck size={16} /> : undefined,
                onClick: () => applyProjectPermissionPolicy(proj, null),
              },
              ...PERMISSION_POLICIES.map(
                (p) =>
                  ({
                    id: `policy-${p.id}`,
                    label: policyLabel(p.id),
                    icon:
                      current === p.id ? <IconCheck size={16} /> : undefined,
                    danger: !!p.dangerous,
                    onClick: () => applyProjectPermissionPolicy(proj, p.id),
                  }) satisfies ContextMenuItem,
              ),
            ];
          }
        } else if (ctxMenu?.kind === "session") {
          const s = sessions.find((x) => x.id === ctxMenu.id);
          if (s) {
            const isOpen =
              session.sessionId === s.id ||
              viewingSessionIdRef.current === s.id;
            const moveDests = sessionMoveDestinations(
              projects.map((project) => project.id),
              s.projectId ?? null,
            );
            items = [
              {
                id: "rename",
                label: tr("session.rename"),
                icon: <IconRename size={16} />,
                onClick: () => renameSession(s),
              },
              {
                id: "move",
                label: tr("session.moveToProject"),
                icon: <IconFolder size={16} />,
                disabled: !sessionMoveAvailable(moveDests),
                submenu: moveDests.map((dest) => {
                  const project = dest.projectId
                    ? projects.find((item) => item.id === dest.projectId)
                    : null;
                  return {
                    id: dest.projectId ? `move-${dest.projectId}` : "move-none",
                    label: project
                      ? project.name
                      : tr("session.moveToNone"),
                    icon: dest.current ? <IconCheck size={16} /> : undefined,
                    disabled: dest.current,
                    onClick: () => {
                      void moveSessionToProject(s, project ?? null);
                    },
                  } satisfies ContextMenuItem;
                }),
              },
              {
                id: "export-md",
                label: tr("session.exportMd"),
                icon: <IconCopy size={16} />,
                separatorBefore: true,
                onClick: () => {
                  void exportActiveSessionMd({
                    id: s.id,
                    title: s.title,
                    projectId: s.projectId,
                  });
                },
              },
              {
                id: "export-bundle",
                label: tr("session.exportBundle"),
                icon: <IconCopy size={16} />,
                onClick: () => {
                  void exportSessionDiagnostic(s.id);
                },
              },
              {
                id: "fork",
                label: tr("session.fork"),
                icon: <IconFork size={16} />,
                separatorBefore: true,
                onClick: () => confirmForkSession(s),
              },
              {
                id: "rewind",
                label: tr("session.rewind"),
                icon: <IconRewind size={16} />,
                disabled: !isOpen || !canRewindSession,
                onClick: () => {
                  void openRewindTimeline(s.id);
                },
              },
              {
                id: "copy-id",
                label: tr("session.copyId"),
                icon: <IconCopy size={16} />,
                onClick: () => {
                  void copySessionId(s);
                },
              },
              {
                id: "archive",
                label: s.archived
                  ? tr("sidebar.unarchive")
                  : tr("sidebar.archive"),
                icon: <IconArchive size={16} />,
                separatorBefore: true,
                onClick: () => {
                  void archiveSession(s, !s.archived);
                },
              },
              {
                id: "delete",
                label: tr("session.delete"),
                icon: <IconTrash size={16} />,
                danger: true,
                onClick: () => deleteSessionConfirm(s),
              },
            ];
          }
        }
        return (
          <ContextMenu
            open={!!ctxMenu && items.length > 0}
            x={ctxMenu?.x ?? 0}
            y={ctxMenu?.y ?? 0}
            anchorRect={ctxMenu?.anchorRect}
            restoreFocusTo={ctxMenu?.restoreFocusTo}
            onClose={() => setCtxMenu(null)}
            items={items}
            estimatedHeight={ctxMenu?.kind === "project-policy" ? 280 : ctxMenu?.kind === "session" ? 360 : 240}
            estimatedWidth={ctxMenu?.kind === "session" ? 236 : 200}
            className={ctxMenu?.kind === "session" ? "context-menu--session" : undefined}
          />
        );
      })()}

      <span hidden data-layout-default={JSON.stringify(DEFAULT_LAYOUT)} />
    </div>
    </ImageViewerProvider>
  );
}
