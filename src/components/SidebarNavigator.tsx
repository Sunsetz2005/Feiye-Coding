import {
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
  type MouseEvent as ReactMouseEvent,
} from "react";
import { createPortal } from "react-dom";
import {
  IconArchive,
  IconChevronDown,
  IconChevronRight,
  IconClock,
  IconFolder,
  IconFolderPlus,
  IconMore,
  IconNewChat,
  IconPanel,
  IconPin,
  IconPlus,
  IconPuzzle,
  IconScheduled,
  IconSearch,
} from "@/components/icons";
import { OverlayScroll } from "@/components/OverlayScroll";
import { SunsetzLogo } from "@/components/SunsetzLogo";
import { Tip } from "@/components/ui/tooltip";
import { Spinner } from "@/components/ui/spinner";
import { UserMenu, remainingPercent } from "@/components/UserMenu";
import { VirtualList } from "@/components/VirtualList";
import {
  projectGitSummaryV1,
  type AccountStatus,
  type CustomProvider,
  type ProjectGitSummaryV1,
  type SessionPreviewV1,
} from "@/lib/api";
import {
  accountDisplayName,
  accountInitials,
} from "@/lib/accountUi";
import type { Theme } from "@/lib/theme";
import {
  SIDEBAR_SESSION_ROW_GAP,
  SIDEBAR_SESSION_ROW_HEIGHT,
} from "@/lib/virtualList";

export interface SidebarSessionItem {
  id: string;
  title: string;
  updatedAt: string;
  archived: boolean;
  scheduled: boolean;
}

export interface SidebarProjectItem {
  id: string;
  name: string;
  path: string;
  trusted: boolean;
  pinned: boolean;
  open: boolean;
  sessions: SidebarSessionItem[];
}

interface SidebarLabels {
  chrome: {
    hide: string;
  };
  drag: {
    addProjectTitle: string;
    addProjectHint: string;
  };
  navigation: {
    label: string;
    newSession: string;
    search: string;
    scheduled: string;
    plugins: string;
  };
  tree: {
    projects: string;
    addProject: string;
    noProjects: string;
    collapseProject: string;
    expandProject: string;
    untrusted: string;
    menu: string;
    trustProject: string;
    noChats: string;
    otherSessions: string;
    untitled: string;
    scheduledTag: string;
    answerNeeded: string;
    sessionWorking: string;
    previewTasks: string;
    previewActive: string;
    previewUpdated: string;
    previewPinned: string;
    previewNoSummary: string;
    previewGitRef: string;
    previewGitAhead: string;
    previewGitBehind: string;
    previewGitDirty: string;
    previewGitConflicts: string;
    previewGitCountsCapped: string;
    unarchive: string;
    archive: string;
  };
  account: {
    trigger: string;
    settings: string;
    theme: string;
    themeLight: string;
    themeDark: string;
    local: string;
    signedIn: string;
    signedOut: string;
    login: string;
    logout: string;
    remaining: string;
    usage: string;
    customProvider: string;
    resetsAt: string;
  };
}

interface SidebarChromeModel {
  useCustomWindowChrome: boolean;
  onHide: () => void;
  onToggleMaximize: () => void;
}

interface SidebarNavigationModel {
  activePane: "chat" | "automations";
  onNewSession: () => void;
  onSearch: () => void;
  onOpenAutomations: () => void;
  onOpenExtensions: () => void;
}

interface SidebarTreeModel {
  projectsOpen: boolean;
  historyOpen: boolean;
  activeProjectId: string | null;
  activeSessionId: string | null;
  busySessionId: string | null;
  pendingAskSessionIds: ReadonlySet<string>;
  projects: readonly SidebarProjectItem[];
  orphanSessions: readonly SidebarSessionItem[];
  onToggleProjects: () => void;
  onAddProject: () => void;
  onToggleProject: (projectId: string, open: boolean) => void;
  onSelectProject: (projectId: string) => void;
  onTrustProject: (projectId: string) => void;
  onProjectMenu: (
    event: ReactMouseEvent<HTMLElement>,
    projectId: string,
  ) => void;
  onToggleHistory: () => void;
  onOpenSession: (sessionId: string, projectId: string | null) => void;
  onArchiveSession: (sessionId: string, archived: boolean) => void;
  onSessionMenu: (
    event: ReactMouseEvent<HTMLElement>,
    sessionId: string,
  ) => void;
  loadSessionPreview: (
    sessionId: string,
  ) => Promise<SessionPreviewV1 | null>;
  loadProjectGitSummary?: (
    projectId: string,
    projectPath: string,
  ) => Promise<ProjectGitSummaryV1 | null>;
}

interface SidebarAccountModel {
  open: boolean;
  theme: Theme;
  account: AccountStatus | null;
  activeProvider: CustomProvider | null;
  busy: boolean;
  customRouteActive: boolean;
  onClose: () => void;
  onToggle: (open: boolean) => void;
  onSettings: () => void;
  onAccountSettings: () => void;
  onToggleTheme: () => void;
  onLogin: () => void;
  onLogout: () => void;
}

export interface SidebarNavigatorProps {
  collapsed: boolean;
  dragZone: "sidebar" | "main" | null;
  labels: SidebarLabels;
  chrome: SidebarChromeModel;
  navigation: SidebarNavigationModel;
  tree: SidebarTreeModel;
  account: SidebarAccountModel;
}

function SessionItem({
  item,
  projectId,
  active,
  working,
  needsAnswer,
  labels,
  onOpen,
  onArchive,
  onMenu,
  onPreview,
  onPreviewEnd,
}: {
  item: SidebarSessionItem;
  projectId: string | null;
  active: boolean;
  working: boolean;
  needsAnswer: boolean;
  labels: SidebarLabels["tree"];
  onOpen: (sessionId: string, projectId: string | null) => void;
  onArchive: (sessionId: string, archived: boolean) => void;
  onMenu: (
    event: ReactMouseEvent<HTMLElement>,
    sessionId: string,
  ) => void;
  onPreview: (anchor: HTMLElement, immediate: boolean) => void;
  onPreviewEnd: () => void;
}) {
  return (
    <div
      className={
        "tree-l3" +
        (projectId == null ? " tree-l3--orphan" : "") +
        (active ? " tree-l3--active" : "") +
        (item.archived ? " tree-l3--archived" : "") +
        (working ? " tree-l3--working" : "")
      }
      onContextMenu={(event) => onMenu(event, item.id)}
      onMouseEnter={(event) => onPreview(event.currentTarget, false)}
      onMouseLeave={onPreviewEnd}
    >
      <button
        type="button"
        className="tree-l3__open"
        aria-current={active ? "page" : undefined}
        onClick={() => onOpen(item.id, projectId)}
        onFocus={(event) => onPreview(event.currentTarget, true)}
        onBlur={onPreviewEnd}
      >
        <span className="tree-l3__title">
          {item.scheduled ? (
            <span
              className="tree-l3__kind"
              title={labels.scheduledTag}
              aria-label={labels.scheduledTag}
            >
              <IconClock size={13} />
            </span>
          ) : null}
          <span className="tree-l3__name">
            {item.title || labels.untitled}
          </span>
        </span>
      </button>

      {needsAnswer ? (
        <Tip label={labels.answerNeeded}>
          <span
            className="tree-l3__status tree-l3__status--question"
            aria-label={labels.answerNeeded}
          >
            ?
          </span>
        </Tip>
      ) : working ? (
        <Tip label={labels.sessionWorking}>
          <span
            className="tree-l3__status"
            aria-label={labels.sessionWorking}
          >
            <Spinner size={14} className="tree-l3__spinner" />
          </span>
        </Tip>
      ) : (
        <span className="tree-l3__actions">
          <Tip label={item.archived ? labels.unarchive : labels.archive}>
            <button
              type="button"
              className="tree-icon-btn"
              aria-label={item.archived ? labels.unarchive : labels.archive}
              onClick={() => onArchive(item.id, !item.archived)}
            >
              <IconArchive size={13} />
            </button>
          </Tip>
          <Tip label={labels.menu}>
            <button
              type="button"
              className="tree-icon-btn"
              aria-label={labels.menu}
              onClick={(event) => {
                onPreviewEnd();
                onMenu(event, item.id);
              }}
            >
              <IconMore size={13} />
            </button>
          </Tip>
        </span>
      )}
    </div>
  );
}

type PreviewAnchor = {
  left: number;
  right: number;
  top: number;
};

type PreviewState =
  | {
      kind: "project";
      anchor: PreviewAnchor;
      project: SidebarProjectItem;
      activeCount: number;
      lastActivity: string | null;
      gitSummary: ProjectGitSummaryV1 | null;
      requestId: number;
    }
  | {
      kind: "session";
      anchor: PreviewAnchor;
      item: SidebarSessionItem;
      projectName: string | null;
      working: boolean;
      needsAnswer: boolean;
      data: SessionPreviewV1 | null;
    };

const PREVIEW_DELAY_MS = 450;
const PREVIEW_CACHE_MS = 30_000;
const PROJECT_GIT_PREVIEW_CACHE_MS = 5_000;
const PROJECT_GIT_PREVIEW_CACHE_LIMIT = 64;

function anchorFromElement(element: HTMLElement): PreviewAnchor {
  const rect = element.getBoundingClientRect();
  return { left: rect.left, right: rect.right, top: rect.top };
}

function formatPreviewTime(value: string): string {
  const date = new Date(value);
  if (!Number.isFinite(date.getTime())) return "—";
  return date.toLocaleString([], {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function previewPosition(anchor: PreviewAnchor): React.CSSProperties {
  const width = 310;
  const gap = 10;
  const viewportWidth =
    typeof window === "undefined" ? 1200 : window.innerWidth;
  const viewportHeight =
    typeof window === "undefined" ? 800 : window.innerHeight;
  const left =
    anchor.right + gap + width <= viewportWidth - 8
      ? anchor.right + gap
      : Math.max(8, anchor.left - width - gap);
  return {
    left,
    top: Math.max(8, Math.min(anchor.top - 8, viewportHeight - 230)),
  };
}

function SidebarPreviewCard({
  state,
  labels,
}: {
  state: PreviewState;
  labels: SidebarLabels["tree"];
}) {
  const status = (countLabel: string, count: number) =>
    countLabel.replace("{count}", String(count));
  const updated = (value: string) =>
    labels.previewUpdated.replace("{time}", formatPreviewTime(value));
  const gitSummary = state.kind === "project" ? state.gitSummary : null;

  return createPortal(
    <aside
      className="sidebar-preview"
      role="tooltip"
      style={previewPosition(state.anchor)}
      data-kind={state.kind}
    >
      {state.kind === "project" ? (
        <>
          <header className="sidebar-preview__header">
            <IconFolder size={16} aria-hidden />
            <strong>{state.project.name}</strong>
            {state.project.pinned ? (
              <span className="sidebar-preview__badge">
                {labels.previewPinned}
              </span>
            ) : null}
          </header>
          <div className="sidebar-preview__meta">
            <span>
              {status(labels.previewTasks, state.project.sessions.length)}
            </span>
            <span>·</span>
            <span>{status(labels.previewActive, state.activeCount)}</span>
          </div>
          <div className="sidebar-preview__path" title={state.project.path}>
            {state.project.path}
          </div>
          {gitSummary?.available && gitSummary.isRepo ? (
            <div className="sidebar-preview__git" aria-label="Git">
              <span
                className="sidebar-preview__git-ref"
                title={gitSummary.head ?? undefined}
              >
                {labels.previewGitRef.replace(
                  "{ref}",
                  gitSummary.branch ?? gitSummary.head ?? "—",
                )}
              </span>
              {gitSummary.ahead != null && gitSummary.ahead > 0 ? (
                <span>
                  {status(labels.previewGitAhead, gitSummary.ahead)}
                </span>
              ) : null}
              {gitSummary.behind != null && gitSummary.behind > 0 ? (
                <span>
                  {status(labels.previewGitBehind, gitSummary.behind)}
                </span>
              ) : null}
              {gitSummary.dirty > 0 ? (
                <span>
                  {status(labels.previewGitDirty, gitSummary.dirty)}
                </span>
              ) : null}
              {gitSummary.conflicts > 0 ? (
                <span className="sidebar-preview__git-conflicts">
                  {status(
                    labels.previewGitConflicts,
                    gitSummary.conflicts,
                  )}
                </span>
              ) : null}
              {gitSummary.countsCapped ? (
                <span
                  className="sidebar-preview__git-capped"
                  aria-label={labels.previewGitCountsCapped}
                  title={labels.previewGitCountsCapped}
                >
                  +
                </span>
              ) : null}
            </div>
          ) : null}
          {!state.project.trusted ? (
            <div className="sidebar-preview__warning">
              {labels.untrusted}
            </div>
          ) : null}
          {state.lastActivity ? (
            <footer>{updated(state.lastActivity)}</footer>
          ) : null}
        </>
      ) : (
        <>
          <header className="sidebar-preview__header">
            <strong>{state.data?.title || state.item.title}</strong>
          </header>
          <div className="sidebar-preview__meta">
            {state.projectName ? <span>{state.projectName}</span> : null}
            {state.working ? (
              <span>{labels.sessionWorking}</span>
            ) : state.needsAnswer ? (
              <span>{labels.answerNeeded}</span>
            ) : null}
          </div>
          {state.data?.recentUserSummary ? (
            <p>{state.data.recentUserSummary}</p>
          ) : null}
          {state.data?.recentAssistantSummary ? (
            <p className="sidebar-preview__assistant">
              {state.data.recentAssistantSummary}
            </p>
          ) : null}
          {state.data &&
          !state.data.recentUserSummary &&
          !state.data.recentAssistantSummary ? (
            <p className="sidebar-preview__empty">
              {labels.previewNoSummary}
            </p>
          ) : null}
          {state.data ? (
            <footer>
              <span>{updated(state.data.updatedAt)}</span>
              {state.data.modelId ? <span>{state.data.modelId}</span> : null}
            </footer>
          ) : (
            <div className="sidebar-preview__loading" aria-hidden />
          )}
        </>
      )}
    </aside>,
    document.body,
  );
}

export function SidebarNavigator({
  collapsed,
  dragZone,
  labels,
  chrome,
  navigation,
  tree,
  account,
}: SidebarNavigatorProps) {
  const [preview, setPreview] = useState<PreviewState | null>(null);
  const previewTimerRef = useRef<number | null>(null);
  const previewRequestRef = useRef(0);
  const previewCacheRef = useRef(
    new Map<string, { expiresAt: number; data: SessionPreviewV1 }>(),
  );
  const previewInflightRef = useRef(
    new Map<string, Promise<SessionPreviewV1 | null>>(),
  );
  const projectGitCacheRef = useRef(
    new Map<
      string,
      { expiresAt: number; data: ProjectGitSummaryV1 | null }
    >(),
  );
  const projectGitInflightRef = useRef(
    new Map<string, Promise<ProjectGitSummaryV1 | null>>(),
  );
  const closePreview = useCallback(() => {
    if (previewTimerRef.current != null) {
      window.clearTimeout(previewTimerRef.current);
      previewTimerRef.current = null;
    }
    previewRequestRef.current += 1;
    setPreview(null);
  }, []);
  useEffect(() => {
    if (collapsed || !tree.projectsOpen || account.open) closePreview();
  }, [account.open, closePreview, collapsed, tree.projectsOpen]);
  useEffect(
    () => () => {
      if (previewTimerRef.current != null) {
        window.clearTimeout(previewTimerRef.current);
      }
      previewRequestRef.current += 1;
    },
    [],
  );
  const previewProjectId =
    preview?.kind === "project" ? preview.project.id : null;
  const previewProjectPath =
    preview?.kind === "project" ? preview.project.path : null;
  const previewProjectRequestId =
    preview?.kind === "project" ? preview.requestId : null;
  const previewProjectExists =
    previewProjectId == null ||
    tree.projects.some(
      (project) =>
        project.id === previewProjectId &&
        project.path === previewProjectPath,
    );
  useEffect(() => {
    if (!previewProjectExists) closePreview();
  }, [closePreview, previewProjectExists]);
  useEffect(() => {
    if (
      previewProjectId == null ||
      previewProjectPath == null ||
      previewProjectRequestId == null ||
      previewRequestRef.current !== previewProjectRequestId ||
      collapsed ||
      !tree.projectsOpen ||
      !previewProjectExists ||
      account.open
    ) {
      return;
    }

    const projectId = previewProjectId;
    const projectPath = previewProjectPath;
    const key = `${projectId}\0${projectPath}`;
    const requestId = previewProjectRequestId;
    const applySummary = (data: ProjectGitSummaryV1 | null) => {
      if (previewRequestRef.current !== requestId) return;
      setPreview((current) => {
        if (
          current?.kind !== "project" ||
          current.project.id !== projectId ||
          current.project.path !== projectPath ||
          current.gitSummary === data
        ) {
          return current;
        }
        return { ...current, gitSummary: data };
      });
    };

    const cached = projectGitCacheRef.current.get(key);
    if (cached && cached.expiresAt > Date.now()) {
      applySummary(cached.data);
      return;
    }
    if (cached) projectGitCacheRef.current.delete(key);

    let pending = projectGitInflightRef.current.get(key);
    if (!pending) {
      const load = tree.loadProjectGitSummary ?? projectGitSummaryV1;
      pending = load(projectId, projectPath);
      projectGitInflightRef.current.set(key, pending);
      void pending.then(
        () => {
          if (projectGitInflightRef.current.get(key) === pending) {
            projectGitInflightRef.current.delete(key);
          }
        },
        () => {
          if (projectGitInflightRef.current.get(key) === pending) {
            projectGitInflightRef.current.delete(key);
          }
        },
      );
    }

    void pending.then(
      (data) => {
        if (previewRequestRef.current !== requestId) return;
        if (
          data != null &&
          (data.version !== 1 || data.projectId !== projectId)
        ) {
          return;
        }
        const now = Date.now();
        const cache = projectGitCacheRef.current;
        for (const [cachedKey, cached] of cache) {
          if (cached.expiresAt <= now) cache.delete(cachedKey);
        }
        if (!cache.has(key) && cache.size >= PROJECT_GIT_PREVIEW_CACHE_LIMIT) {
          const oldestKey = cache.keys().next().value;
          if (oldestKey != null) cache.delete(oldestKey);
        }
        cache.set(key, {
          expiresAt: now + PROJECT_GIT_PREVIEW_CACHE_MS,
          data,
        });
        applySummary(data);
      },
      () => undefined,
    );
  }, [
    account.open,
    collapsed,
    previewProjectExists,
    previewProjectId,
    previewProjectPath,
    previewProjectRequestId,
    tree.projectsOpen,
    tree.loadProjectGitSummary,
  ]);
  const scheduleProjectPreview = useCallback(
    (
      project: SidebarProjectItem,
      anchor: HTMLElement,
      immediate: boolean,
    ) => {
      closePreview();
      const show = () => {
        const activeCount = project.sessions.filter(
          (session) =>
            session.id === tree.busySessionId ||
            tree.pendingAskSessionIds.has(session.id),
        ).length;
        const lastActivity =
          project.sessions
            .map((session) => session.updatedAt)
            .sort()
            .at(-1) ?? null;
        setPreview({
          kind: "project",
          anchor: anchorFromElement(anchor),
          project,
          activeCount,
          lastActivity,
          gitSummary: null,
          requestId: previewRequestRef.current,
        });
      };
      if (immediate) show();
      else previewTimerRef.current = window.setTimeout(show, PREVIEW_DELAY_MS);
    },
    [closePreview, tree.busySessionId, tree.pendingAskSessionIds],
  );
  const scheduleSessionPreview = useCallback(
    (
      item: SidebarSessionItem,
      projectName: string | null,
      working: boolean,
      needsAnswer: boolean,
      anchor: HTMLElement,
      immediate: boolean,
    ) => {
      closePreview();
      const requestId = previewRequestRef.current;
      const show = async () => {
        const anchored = anchorFromElement(anchor);
        const cached = previewCacheRef.current.get(item.id);
        if (cached && cached.expiresAt > Date.now()) {
          setPreview({
            kind: "session",
            anchor: anchored,
            item,
            projectName,
            working,
            needsAnswer,
            data: cached.data,
          });
          return;
        }
        setPreview({
          kind: "session",
          anchor: anchored,
          item,
          projectName,
          working,
          needsAnswer,
          data: null,
        });
        let pending = previewInflightRef.current.get(item.id);
        if (!pending) {
          pending = tree
            .loadSessionPreview(item.id)
            .catch(() => null)
            .finally(() => previewInflightRef.current.delete(item.id));
          previewInflightRef.current.set(item.id, pending);
        }
        const data = await pending;
        if (previewRequestRef.current !== requestId) return;
        if (!data) {
          setPreview(null);
          return;
        }
        previewCacheRef.current.set(item.id, {
          expiresAt: Date.now() + PREVIEW_CACHE_MS,
          data,
        });
        setPreview({
          kind: "session",
          anchor: anchored,
          item,
          projectName,
          working,
          needsAnswer,
          data,
        });
      };
      if (immediate) void show();
      else {
        previewTimerRef.current = window.setTimeout(
          () => void show(),
          PREVIEW_DELAY_MS,
        );
      }
    },
    [closePreview, tree],
  );
  const idPrefix = useId().replaceAll(":", "");
  const projectListId = `${idPrefix}-projects`;
  const historyListId = `${idPrefix}-history`;
  const providerName =
    account.activeProvider?.name.trim() || account.activeProvider?.id.trim();
  const accountInitial = account.activeProvider
    ? Array.from(providerName || "P")[0]?.toUpperCase() || "P"
    : account.account?.profile
      ? accountInitials(account.account.profile)
      : "G";
  const accountName = account.activeProvider
    ? providerName || "P"
    : account.account?.profile
      ? accountDisplayName(account.account.profile, labels.account.local)
      : labels.account.local;
  const quota =
    !account.customRouteActive && account.account?.profile?.signedIn
      ? remainingPercent(account.account)
      : null;

  return (
    <aside
      className={
        "sidebar" +
        (collapsed ? " sidebar--hidden" : "") +
        (dragZone === "sidebar" ? " is-drop-target" : "") +
        (dragZone === "main" ? " is-drop-idle" : "")
      }
      aria-label={labels.navigation.label}
      aria-hidden={collapsed}
      inert={collapsed ? true : undefined}
      data-testid="sidebar-navigator"
      onScrollCapture={closePreview}
    >
      {dragZone === "sidebar" ? (
        <div className="drop-overlay drop-overlay--project" aria-hidden>
          <div className="drop-overlay__card">
            <span className="drop-overlay__icon">
              <IconFolderPlus size={22} />
            </span>
            <strong>{labels.drag.addProjectTitle}</strong>
            <span>{labels.drag.addProjectHint}</span>
          </div>
        </div>
      ) : null}

      <div
        className="sidebar-chrome"
        data-tauri-drag-region
        onDoubleClick={() => {
          if (chrome.useCustomWindowChrome) chrome.onToggleMaximize();
        }}
      >
        <Tip label={labels.chrome.hide}>
          <button
            type="button"
            className="chrome-btn chrome-btn--traffic main__pane-toggle is-on"
            aria-label={labels.chrome.hide}
            onClick={chrome.onHide}
          >
            <IconPanel size={16} />
          </button>
        </Tip>
        <div className="sidebar-chrome__drag" data-tauri-drag-region />
      </div>

      <div className="sidebar-brand-row">
        <div className="sidebar-brand-row__left">
          <SunsetzLogo size={20} />
          <span>Sunsetz</span>
        </div>
      </div>

      <nav className="sidebar-nav" aria-label={labels.navigation.label}>
        <button
          type="button"
          className="nav-new"
          onClick={navigation.onNewSession}
        >
          <span className="nav-item__icon">
            <IconNewChat size={16} />
          </span>
          {labels.navigation.newSession}
        </button>
        <button
          type="button"
          className="nav-item"
          onClick={navigation.onSearch}
        >
          <span className="nav-item__icon">
            <IconSearch size={16} />
          </span>
          {labels.navigation.search}
        </button>
        <button
          type="button"
          className={
            "nav-item" +
            (navigation.activePane === "automations"
              ? " nav-item--active"
              : "")
          }
          aria-current={
            navigation.activePane === "automations" ? "page" : undefined
          }
          onClick={navigation.onOpenAutomations}
        >
          <span className="nav-item__icon">
            <IconScheduled size={16} />
          </span>
          {labels.navigation.scheduled}
        </button>
        <button
          type="button"
          className="nav-item"
          onClick={navigation.onOpenExtensions}
        >
          <span className="nav-item__icon">
            <IconPuzzle size={16} />
          </span>
          {labels.navigation.plugins}
        </button>
      </nav>

      <OverlayScroll
        className="sidebar__scroll"
        viewportClassName="sidebar__scroll-inner"
      >
        <div className="tree-l1">
          <button
            type="button"
            className="tree-l1__head"
            aria-expanded={tree.projectsOpen}
            aria-controls={projectListId}
            onClick={() => {
              closePreview();
              tree.onToggleProjects();
            }}
          >
            {tree.projectsOpen ? (
              <IconChevronDown size={14} />
            ) : (
              <IconChevronRight size={14} />
            )}
            <span className="tree-l1__label">{labels.tree.projects}</span>
          </button>
          <Tip label={labels.tree.addProject}>
            <button
              type="button"
              className="tree-l1__action"
              aria-label={labels.tree.addProject}
              onClick={tree.onAddProject}
            >
              <IconPlus size={15} />
            </button>
          </Tip>
        </div>

        <div id={projectListId} hidden={!tree.projectsOpen}>
          {tree.projects.length === 0 ? (
            <div className="sidebar-empty">{labels.tree.noProjects}</div>
          ) : null}

          {tree.projects.map((project, index) => {
            const sessionsOpen = project.open;
            const sessionListId = `${idPrefix}-project-${index}`;

            return (
              <div key={project.id} className="tree-project">
                <div
                  className={
                    "tree-l2" +
                    (!tree.activeSessionId &&
                    tree.activeProjectId === project.id
                      ? " tree-l2--active"
                      : "")
                  }
                  onContextMenu={(event) => {
                    closePreview();
                    tree.onProjectMenu(event, project.id);
                  }}
                >
                  <button
                    type="button"
                    className="tree-l2__disclosure"
                    aria-label={
                      sessionsOpen
                        ? labels.tree.collapseProject
                        : labels.tree.expandProject
                    }
                    aria-expanded={sessionsOpen}
                    aria-controls={sessionListId}
                    onClick={() => {
                      closePreview();
                      tree.onToggleProject(project.id, !sessionsOpen);
                    }}
                  >
                    {sessionsOpen ? (
                      <IconChevronDown size={13} />
                    ) : (
                      <IconChevronRight size={13} />
                    )}
                  </button>
                  <Tip label={project.path}>
                    <button
                      type="button"
                      className="tree-l2__select"
                      aria-current={
                        !tree.activeSessionId &&
                        tree.activeProjectId === project.id
                          ? "page"
                          : undefined
                      }
                      disabled={!project.trusted}
                      onClick={() => tree.onSelectProject(project.id)}
                      onMouseEnter={(event) =>
                        scheduleProjectPreview(
                          project,
                          event.currentTarget,
                          false,
                        )
                      }
                      onMouseLeave={closePreview}
                      onFocus={(event) =>
                        scheduleProjectPreview(
                          project,
                          event.currentTarget,
                          true,
                        )
                      }
                      onBlur={closePreview}
                    >
                      <span className="tree-l2__icon" aria-hidden>
                        <IconFolder size={15} />
                      </span>
                      <span className="tree-l2__name">
                        {project.pinned ? (
                          <IconPin size={12} className="tree-l2__pin" />
                        ) : null}
                        {project.name}
                      </span>
                    </button>
                  </Tip>
                  {!project.trusted ? (
                    <span className="project-row__badge">
                      {labels.tree.untrusted}
                    </span>
                  ) : null}
                  <span className="tree-l2__actions">
                    <Tip label={labels.tree.menu}>
                      <button
                        type="button"
                        className="tree-icon-btn"
                        aria-label={labels.tree.menu}
                        onClick={(event) => {
                          closePreview();
                          tree.onProjectMenu(event, project.id)
                        }}
                      >
                        <IconMore size={14} />
                      </button>
                    </Tip>
                  </span>
                </div>

                <div id={sessionListId} hidden={!sessionsOpen}>
                  <div className="tree-l3-list-wrap">
                    {!project.trusted ? (
                      <button
                        type="button"
                        className="tree-l3 tree-l3--hint"
                        onClick={() => tree.onTrustProject(project.id)}
                      >
                        {labels.tree.trustProject}
                      </button>
                    ) : null}
                    {project.sessions.length > 0 ? (
                      <VirtualList
                        className="tree-l3-list"
                        items={project.sessions}
                        getKey={(item) => item.id}
                        rowHeight={SIDEBAR_SESSION_ROW_HEIGHT}
                        gap={SIDEBAR_SESSION_ROW_GAP}
                        scrollToKey={
                          tree.activeSessionId &&
                          project.sessions.some(
                            (item) => item.id === tree.activeSessionId,
                          )
                            ? tree.activeSessionId
                            : null
                        }
                        renderItem={(item) => (
                          <SessionItem
                            item={item}
                            projectId={project.id}
                            active={tree.activeSessionId === item.id}
                            working={tree.busySessionId === item.id}
                            needsAnswer={tree.pendingAskSessionIds.has(item.id)}
                            labels={labels.tree}
                            onOpen={tree.onOpenSession}
                            onArchive={tree.onArchiveSession}
                            onMenu={(event, sessionId) => {
                              closePreview();
                              tree.onSessionMenu(event, sessionId);
                            }}
                            onPreview={(anchor, immediate) =>
                              scheduleSessionPreview(
                                item,
                                project.name,
                                tree.busySessionId === item.id,
                                tree.pendingAskSessionIds.has(item.id),
                                anchor,
                                immediate,
                              )
                            }
                            onPreviewEnd={closePreview}
                          />
                        )}
                      />
                    ) : null}
                    {project.sessions.length === 0 && project.trusted ? (
                      <div className="sidebar-empty sidebar-empty--compact">
                        {labels.tree.noChats}
                      </div>
                    ) : null}
                  </div>
                </div>
              </div>
            );
          })}
        </div>

        <div className="tree-l1 tree-l1--history">
          <button
            type="button"
            className="tree-l1__head"
            aria-expanded={tree.historyOpen}
            aria-controls={historyListId}
            onClick={() => {
              closePreview();
              tree.onToggleHistory();
            }}
          >
            {tree.historyOpen ? (
              <IconChevronDown size={14} />
            ) : (
              <IconChevronRight size={14} />
            )}
            <span className="tree-l1__label">
              {labels.tree.otherSessions}
            </span>
          </button>
        </div>
        <div id={historyListId} hidden={!tree.historyOpen}>
          {tree.orphanSessions.length > 0 ? (
            <VirtualList
              className="tree-orphan-list"
              items={tree.orphanSessions}
              getKey={(item) => item.id}
              rowHeight={SIDEBAR_SESSION_ROW_HEIGHT}
              gap={SIDEBAR_SESSION_ROW_GAP}
              scrollToKey={
                tree.activeSessionId &&
                tree.orphanSessions.some(
                  (item) => item.id === tree.activeSessionId,
                )
                  ? tree.activeSessionId
                  : null
              }
              renderItem={(item) => (
                <SessionItem
                  item={item}
                  projectId={null}
                  active={tree.activeSessionId === item.id}
                  working={tree.busySessionId === item.id}
                  needsAnswer={tree.pendingAskSessionIds.has(item.id)}
                  labels={labels.tree}
                  onOpen={tree.onOpenSession}
                  onArchive={tree.onArchiveSession}
                  onMenu={(event, sessionId) => {
                    closePreview();
                    tree.onSessionMenu(event, sessionId);
                  }}
                  onPreview={(anchor, immediate) =>
                    scheduleSessionPreview(
                      item,
                      null,
                      tree.busySessionId === item.id,
                      tree.pendingAskSessionIds.has(item.id),
                      anchor,
                      immediate,
                    )
                  }
                  onPreviewEnd={closePreview}
                />
              )}
            />
          ) : null}
        </div>
      </OverlayScroll>
      {preview && typeof document !== "undefined" ? (
        <SidebarPreviewCard state={preview} labels={labels.tree} />
      ) : null}

      <UserMenu
        open={account.open}
        onClose={account.onClose}
        theme={account.theme}
        account={account.account}
        activeProvider={account.activeProvider}
        accountBusy={account.busy}
        labels={{
          settings: labels.account.settings,
          theme: labels.account.theme,
          themeLight: labels.account.themeLight,
          themeDark: labels.account.themeDark,
          local: labels.account.local,
          signedIn: labels.account.signedIn,
          signedOut: labels.account.signedOut,
          login: labels.account.login,
          logout: labels.account.logout,
          remaining: labels.account.remaining,
          usage: labels.account.usage,
          customProvider: labels.account.customProvider,
          resetsAt: labels.account.resetsAt,
        }}
        onSettings={account.onSettings}
        onAccountSettings={account.onAccountSettings}
        onToggleTheme={account.onToggleTheme}
        onLogin={account.onLogin}
        onLogout={account.onLogout}
      >
        <Tip label={labels.account.trigger}>
          <button
            type="button"
            className={"sidebar__footer" + (account.open ? " is-open" : "")}
            aria-haspopup="menu"
            aria-expanded={account.open}
            aria-label={labels.account.trigger}
            onClick={() => {
              closePreview();
              account.onToggle(!account.open);
            }}
          >
            <div className="user-avatar" aria-hidden>
              {accountInitial}
            </div>
            <div className="user-meta">
              <span className="user-meta__name">{accountName}</span>
              {quota != null ? (
                <span className="user-meta__quota">{quota.toFixed(0)}%</span>
              ) : null}
            </div>
          </button>
        </Tip>
      </UserMenu>
    </aside>
  );
}
