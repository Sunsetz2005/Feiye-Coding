// @vitest-environment jsdom

import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
  waitFor,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import {
  afterAll,
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from "vitest";
import type { SidebarNavigatorProps } from "@/components/SidebarNavigator";

type EventHandler = (payload: unknown) => void;

const apiListenerCapture = vi.hoisted(() => ({
  handlers: new Map<string, EventHandler>(),
  tauri: false,
  resolvePlan: vi.fn(async () => undefined),
  sessionState: {
    sessionId: null as string | null,
    agentSessionId: null,
    state: "idle",
    lastError: null,
    streamingMessageId: null,
    backend: "grok_agent_stdio",
    title: "",
  },
  sessions: [] as Array<Record<string, unknown>>,
  interactionRows: [] as Array<Record<string, unknown>>,
  candidateResponses: [] as Array<Array<Record<string, unknown>>>,
  searchHits: [] as Array<Record<string, unknown>>,
  sessionCreate: vi.fn(async () => ({ id: "scheduled-session", title: "Scheduled" })),
  sessionDisconnect: vi.fn(async () => undefined),
  sessionConnect: vi.fn(async (args: { sessionId?: string }) => ({
    sessionId: args.sessionId ?? "scheduled-session",
    agentSessionId: "agent",
    state: "ready",
    lastError: null,
    streamingMessageId: null,
    backend: "grok_agent_stdio",
    title: "Scheduled",
  })),
  sessionSend: vi.fn(async () => undefined),
  automationBind: vi.fn(async () => undefined),
  automationComplete: vi.fn(async () => undefined),
  sessionSearch: vi.fn(async () => [] as Array<Record<string, unknown>>),
  settingsSet: vi.fn(async () => undefined),
}));

vi.mock("@/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api")>();
  return {
    ...actual,
    isTauri: () => apiListenerCapture.tauri,
    listen: vi.fn(async (event: string, handler: EventHandler) => {
      apiListenerCapture.handlers.set(event, handler);
      return () => {
        if (apiListenerCapture.handlers.get(event) === handler) {
          apiListenerCapture.handlers.delete(event);
        }
      };
    }),
    sessionResolvePlan: apiListenerCapture.resolvePlan,
    projectsList: vi.fn(async () => []),
    sessionsList: vi.fn(async () => apiListenerCapture.sessions),
    settingsGet: vi.fn(async () => ({
      locale: "zh",
      setupWizardCompleted: true,
    })),
    probeCli: vi.fn(async () => ({
      found: true,
      path: "/test/sunsetz",
      version: "test",
      source: "test",
      cliAuthPresent: false,
    })),
    modelsListAvailable: vi.fn(async () => ({
      models: [],
      defaultModelId: null,
    })),
    composerPrefsResolve: vi.fn(async () => null),
    secretsGetMasked: vi.fn(async () => ({
      hasOfficialKey: false,
      hasRelayKey: false,
    })),
    sessionPendingInteractions: vi.fn(async () => []),
    sessionInteractionsList: vi.fn(async () => apiListenerCapture.interactionRows),
    sessionGetState: vi.fn(async () => apiListenerCapture.sessionState),
    skillCandidatesListV1: vi.fn(async () =>
      apiListenerCapture.candidateResponses.shift() ?? [],
    ),
    sessionSearchV1: apiListenerCapture.sessionSearch,
    sessionCreate: apiListenerCapture.sessionCreate,
    sessionDisconnect: apiListenerCapture.sessionDisconnect,
    sessionConnect: apiListenerCapture.sessionConnect,
    sessionSend: apiListenerCapture.sessionSend,
    automationClaimBindV1: apiListenerCapture.automationBind,
    automationClaimCompleteV1: apiListenerCapture.automationComplete,
    settingsSet: apiListenerCapture.settingsSet,
    trayRefresh: vi.fn(async () => undefined),
  };
});

vi.mock("@/components/ResourceViewer", () => ({
  ResourceViewer: () => <aside data-testid="resource-viewer-mock" />,
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => undefined),
}));

const sidebarCapture = vi.hoisted(() => ({
  current: null as SidebarNavigatorProps | null,
}));

vi.mock("@/components/SidebarNavigator", () => ({
  SidebarNavigator: (props: SidebarNavigatorProps) => {
    sidebarCapture.current = props;
    return <aside data-testid="sidebar-navigator" />;
  },
}));

class ObserverStub {
  observe() {}
  unobserve() {}
  disconnect() {}
}

function createMemoryStorage(): Storage {
  const values = new Map<string, string>();
  return {
    get length() {
      return values.size;
    },
    clear: () => values.clear(),
    getItem: (key) => values.get(key) ?? null,
    key: (index) => Array.from(values.keys())[index] ?? null,
    removeItem: (key) => {
      values.delete(key);
    },
    setItem: (key, value) => {
      values.set(key, String(value));
    },
  };
}

beforeAll(() => {
  vi.stubGlobal("localStorage", createMemoryStorage());
  vi.stubGlobal("ResizeObserver", ObserverStub);
  vi.stubGlobal("IntersectionObserver", ObserverStub);
  vi.stubGlobal(
    "matchMedia",
    vi.fn().mockImplementation((query: string) => ({
      matches: false,
      media: query,
      onchange: null,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      addListener: vi.fn(),
      removeListener: vi.fn(),
      dispatchEvent: vi.fn(),
    })),
  );
  Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
    configurable: true,
    value: vi.fn(),
  });
});

beforeEach(() => {
  window.location.hash = "";
  apiListenerCapture.sessionState = {
    sessionId: null,
    agentSessionId: null,
    state: "idle",
    lastError: null,
    streamingMessageId: null,
    backend: "grok_agent_stdio",
    title: "",
  };
  apiListenerCapture.sessions = [];
  apiListenerCapture.interactionRows = [];
  apiListenerCapture.candidateResponses = [];
  apiListenerCapture.searchHits = [];
  apiListenerCapture.sessionSearch.mockImplementation(async () =>
    apiListenerCapture.searchHits,
  );
});

afterEach(() => {
  cleanup();
  localStorage.clear();
  sidebarCapture.current = null;
  apiListenerCapture.handlers.clear();
  apiListenerCapture.tauri = false;
  apiListenerCapture.resolvePlan.mockClear();
  apiListenerCapture.sessionCreate.mockClear();
  apiListenerCapture.sessionDisconnect.mockClear();
  apiListenerCapture.sessionConnect.mockClear();
  apiListenerCapture.sessionSend.mockClear();
  apiListenerCapture.automationBind.mockClear();
  apiListenerCapture.automationComplete.mockClear();
  apiListenerCapture.sessionSearch.mockClear();
  apiListenerCapture.settingsSet.mockClear();
  delete (window as Window & { __TAURI_INTERNALS__?: unknown })
    .__TAURI_INTERNALS__;
});

afterAll(() => {
  vi.unstubAllGlobals();
});

describe("App workbench integration", () => {
  it(
    "renders the browser fallback through the real workbench boundaries",
    async () => {
      localStorage.setItem("sunsetz.theme", "dark");
      const { default: App } = await import("./App");
      render(<App />);

      await waitFor(() => {
        expect(screen.getByTestId("workbench-shell")).toBeTruthy();
      });
      expect(screen.getByTestId("sidebar-navigator")).toBeTruthy();
      expect(screen.getByRole("heading", { name: /new|新会话/i })).toBeTruthy();

      const sidebar = sidebarCapture.current;
      expect(sidebar).not.toBeNull();
      if (!sidebar) return;

      await act(async () => {
        sidebar.chrome.onHide();
        sidebar.chrome.onToggleMaximize();
        sidebar.navigation.onNewSession();
        sidebar.navigation.onSearch();
        sidebar.navigation.onOpenAutomations();
        sidebar.navigation.onOpenExtensions();
        sidebar.tree.onToggleProjects();
        sidebar.tree.onAddProject();
        sidebar.tree.onToggleProject("missing-project", true);
        sidebar.tree.onSelectProject("missing-project");
        sidebar.tree.onTrustProject("missing-project");
        sidebar.tree.onProjectMenu(
          { currentTarget: document.body } as never,
          "missing-project",
        );
        sidebar.tree.onToggleHistory();
        sidebar.tree.onOpenSession("missing-session", null);
        sidebar.tree.onArchiveSession("missing-session", true);
        sidebar.tree.onSessionMenu(
          { currentTarget: document.body } as never,
          "missing-session",
        );
        sidebar.account.onClose();
        sidebar.account.onToggle(false);
        sidebar.account.onToggle(true);
        sidebar.account.onSettings();
        sidebar.account.onAccountSettings();
        sidebar.account.onToggleTheme();
        sidebar.account.onLogin();
        sidebar.account.onLogout();
        await Promise.resolve();
      });

      expect(window.location.hash).toMatch(/^#\/settings\/(account|general)$/);
      const settingsSearch = await screen.findByRole("textbox", {
        name: /Search settings|搜索设置/,
      });
      fireEvent.change(settingsSearch, {
        target: { value: "setting-that-does-not-exist" },
      });
      expect(screen.getByRole("status").textContent).toMatch(
        /No matching settings|没有匹配的设置/,
      );
    },
    20_000,
  );

  it(
    "persists plan mode beside access and can disable it in place",
    async () => {
      const user = userEvent.setup();
      const { default: App } = await import("./App");
      render(<App />);
      await screen.findByTestId("workbench-shell");

      const invoke = vi.fn(
        async (command: string, args: Record<string, unknown>) => {
          if (command !== "composer_prefs_set") return null;
          return {
            modelId: "sunsetz-4.5",
            effort: "medium",
            mode: args.mode ?? "agent",
            permissionPolicy: "ask",
            scope: "global",
            source: "test",
          };
        },
      );
      Object.assign(window, { __TAURI_INTERNALS__: { invoke } });

      await user.click(
        screen.getByRole("button", { name: /Access|访问/ }),
      );
      const access = await screen.findByRole("dialog", {
        name: /Access|访问/,
      });
      await user.click(
        within(access).getByText("Plan").closest("button")!,
      );

      const planMode = await screen.findByRole("button", {
        name: /Plan mode|计划模式/,
      });
      expect(
        document.querySelector(".composer-context-rail__activity"),
      ).toBeNull();
      await user.click(planMode);
      await waitFor(() => {
        expect(
          screen.queryByRole("button", {
            name: /Plan mode|计划模式/,
          }),
        ).toBeNull();
      });
      expect(invoke).toHaveBeenCalledTimes(2);
    },
    20_000,
  );

  it(
    "rolls plan mode back when Host persistence fails",
    async () => {
      const user = userEvent.setup();
      const { default: App } = await import("./App");
      render(<App />);
      await screen.findByTestId("workbench-shell");

      Object.assign(window, {
        __TAURI_INTERNALS__: {
          invoke: vi.fn(async (command: string) => {
            if (command === "composer_prefs_set") {
              throw new Error("save failed");
            }
            return null;
          }),
        },
      });

      await user.click(
        screen.getByRole("button", { name: /Access|访问/ }),
      );
      const access = await screen.findByRole("dialog", {
        name: /Access|访问/,
      });
      await user.click(
        within(access).getByText("Plan").closest("button")!,
      );

      await waitFor(() => {
        expect(
          screen.queryByRole("button", {
            name: /Plan mode|计划模式/,
          }),
        ).toBeNull();
      });
      expect(screen.getByText(/save failed/)).toBeTruthy();
    },
    20_000,
  );

  it(
    "keeps plan review in the dock after approval without opening resources",
    async () => {
      const user = userEvent.setup();
      apiListenerCapture.tauri = true;

      const { default: App } = await import("./App");
      render(<App />);
      await screen.findByTestId("workbench-shell");
      await waitFor(() => {
        expect(apiListenerCapture.handlers.has("session://plan")).toBe(true);
      });

      act(() => {
        apiListenerCapture.handlers.get("session://plan")?.({
          rpcId: 41,
          body: "# Delivery plan\n\n1. Update the workbench",
          entries: [{ content: "Update the workbench", status: "pending" }],
        });
      });

      await waitFor(() => {
        expect(screen.getAllByText(/Plan ready|计划待审阅/)).toHaveLength(1);
      });
      expect(screen.queryByTestId("resource-viewer-mock")).toBeNull();

      await user.click(
        screen.getByRole("button", { name: /Approve plan|实施此计划/ }),
      );
      await waitFor(() => {
        expect(apiListenerCapture.resolvePlan).toHaveBeenCalledWith({
          decision: "approved",
          rpcId: 41,
        });
      });

      expect(screen.queryByText(/Plan ready|计划待审阅/)).toBeNull();
      expect(screen.getByTestId("plan-artifact-card")).toBeTruthy();
      expect(screen.queryByTestId("resource-viewer-mock")).toBeNull();
    },
    20_000,
  );

  it(
    "reduces foreground and background interaction snapshots across all kinds",
    async () => {
      apiListenerCapture.tauri = true;
      apiListenerCapture.sessionState = {
        ...apiListenerCapture.sessionState,
        sessionId: "s1",
        state: "ready",
        title: "Task one",
      };
      const { default: App } = await import("./App");
      render(<App />);
      await waitFor(() => {
        expect(apiListenerCapture.handlers.has("session://interaction")).toBe(true);
      });

      const emit = (payload: Record<string, unknown>) =>
        act(() => apiListenerCapture.handlers.get("session://interaction")?.(payload));
      const common = {
        version: 1,
        processId: "p1",
        rpcId: 1,
        createdAt: "2026-01-01T00:00:00Z",
        updatedAt: "2026-01-01T00:00:00Z",
      };

      emit({
        ...common,
        interactionId: "perm",
        sessionId: "s1",
        status: "pending",
        payload: {
          kind: "permission",
          toolName: "write",
          title: "Write migration file",
          preview: "preview",
          scopeKey: "write:/project/a",
          options: [],
        },
      });
      expect(await screen.findByText("Write migration file")).toBeTruthy();
      emit({
        ...common,
        interactionId: "perm",
        sessionId: "s1",
        status: "resolved",
        payload: {
          kind: "permission",
          toolName: "write",
          title: "Write migration file",
          preview: "preview",
          scopeKey: "write:/project/a",
          options: [],
        },
      });
      await waitFor(() => expect(screen.queryByText("Write migration file")).toBeNull());

      emit({
        ...common,
        interactionId: "ask",
        sessionId: "s1",
        status: "resolving",
        payload: {
          kind: "ask_user",
          questions: [{ id: "q", question: "Choose migration?", options: [], multiSelect: false }],
          partialAnswers: { q: "yes" },
        },
      });
      expect(await screen.findByText("Choose migration?")).toBeTruthy();
      emit({
        ...common,
        interactionId: "ask",
        sessionId: "s1",
        status: "interrupted",
        payload: { kind: "ask_user", questions: [] },
      });

      emit({
        ...common,
        interactionId: "plan",
        sessionId: "s1",
        status: "pending",
        payload: { kind: "plan", entries: [], body: "Migration plan body" },
      });
      expect(await screen.findByText("Migration plan body")).toBeTruthy();
      emit({
        ...common,
        interactionId: "background",
        sessionId: "s2",
        status: "pending",
        payload: { kind: "ask_user", questions: [] },
      });
      await waitFor(() => {
        expect(sidebarCapture.current?.tree.pendingAskSessionIds.has("s2")).toBe(true);
      });
      emit({
        ...common,
        interactionId: "background",
        sessionId: "s2",
        status: "failed",
        payload: { kind: "ask_user", questions: [] },
      });
      await waitFor(() => {
        expect(sidebarCapture.current?.tree.pendingAskSessionIds.has("s2")).toBe(false);
      });
    },
    20_000,
  );

  it(
    "runs only unbound Host automation claims and binds before sending",
    async () => {
      apiListenerCapture.tauri = true;
      apiListenerCapture.sessionState = {
        ...apiListenerCapture.sessionState,
        sessionId: "s1",
        state: "ready",
      };
      const { default: App } = await import("./App");
      render(<App />);
      await waitFor(() => {
        expect(apiListenerCapture.handlers.has("automation://claim_v1")).toBe(true);
      });
      const automation = {
        id: "auto",
        title: "Daily audit",
        prompt: "Run audit",
        enabled: true,
        projectId: null,
        modelId: null,
        effort: null,
        frequency: "daily",
        time: "09:00",
        weekdays: [],
        notify: "none",
        createdAt: "2026-01-01T00:00:00Z",
        updatedAt: "2026-01-01T00:00:00Z",
      };
      act(() => {
        apiListenerCapture.handlers.get("automation://claim_v1")?.({
          version: 1,
          claimId: "bound",
          sessionId: "existing",
          scheduledFor: "2026-01-01T00:00:00Z",
          catchUp: false,
          automation,
        });
      });
      expect(apiListenerCapture.sessionCreate).not.toHaveBeenCalled();

      act(() => {
        apiListenerCapture.handlers.get("automation://claim_v1")?.({
          version: 1,
          claimId: "fresh",
          sessionId: null,
          scheduledFor: "2026-01-01T00:00:00Z",
          catchUp: true,
          automation,
        });
      });
      await waitFor(() => {
        expect(apiListenerCapture.automationBind).toHaveBeenCalledWith(
          "fresh",
          "scheduled-session",
        );
        expect(apiListenerCapture.sessionSend).toHaveBeenCalledWith(
          "[Scheduled: Daily audit]\n\nRun audit",
        );
      });
      expect(apiListenerCapture.automationComplete).not.toHaveBeenCalled();
    },
    20_000,
  );

  it(
    "merges FTS content hits into session search results",
    async () => {
      apiListenerCapture.tauri = true;
      apiListenerCapture.sessions = [
        {
          id: "s-content",
          title: "Unrelated title",
          projectId: null,
          updatedAt: "2026-01-01T00:00:00Z",
          archived: false,
        },
      ];
      apiListenerCapture.searchHits = [
        {
          version: 1,
          sessionId: "s-content",
          sessionTitle: "Unrelated title",
          messageId: "m1",
          role: "assistant",
          snippet: "migration needle",
          rank: 0,
        },
      ];
      const { default: App } = await import("./App");
      render(<App />);
      await screen.findByTestId("workbench-shell");
      await waitFor(() => {
        expect(
          sidebarCapture.current?.tree.orphanSessions.some(
            (session) => session.id === "s-content",
          ),
        ).toBe(true);
      });
      await act(async () => sidebarCapture.current?.navigation.onSearch());
      const input = await screen.findByPlaceholderText(
        /Search chats \/ projects|搜索会话 \/ 项目/,
      );
      fireEvent.change(input, { target: { value: "needle" } });
      await waitFor(() => expect(apiListenerCapture.sessionSearch).toHaveBeenCalled());
      await waitFor(() => {
        expect(
          screen.getByRole("dialog", { name: /Search|搜索/ }).textContent,
        ).toContain("migration needle");
      });
    },
    20_000,
  );

  it(
    "opens a newly generated pending Skill candidate for review",
    async () => {
      apiListenerCapture.tauri = true;
      apiListenerCapture.sessionState = {
        ...apiListenerCapture.sessionState,
        sessionId: "s1",
        state: "ready",
      };
      const candidate = {
        version: 1,
        id: "candidate",
        status: "pending",
        createdAt: "2026-01-01T00:00:00Z",
        updatedAt: "2026-01-01T00:00:00Z",
        contentHash: "hash",
        source: { sessionId: "s1", sessionTitle: "Task", messageIds: ["u", "a"] },
        owner: { kind: "host_generated", namespace: "sunsetz", mayOverwriteExternal: false },
        draft: {
          name: "migration-helper",
          description: "Review migration workflow",
          skillMd: "---\nname: migration-helper\ndescription: Review migration workflow\n---\n",
          references: [],
        },
        approvedPath: null,
      };
      apiListenerCapture.candidateResponses = [[], [candidate]];
      const { default: App } = await import("./App");
      render(<App />);
      expect(await screen.findByDisplayValue("migration-helper")).toBeTruthy();
    },
    20_000,
  );
});
