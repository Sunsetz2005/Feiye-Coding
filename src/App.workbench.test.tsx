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
import type { SettingsPageProps } from "@/components/SettingsPage";
import type { ComposerDockProps } from "@/components/ComposerDock";
import type { ResourceViewerProps } from "@/components/ResourceViewer";
import type { SkillMetadataRankingResultV1 } from "@/lib/api";

type EventHandler = (payload: unknown) => void;

const apiListenerCapture = vi.hoisted(() => ({
  handlers: new Map<string, EventHandler>(),
  tauri: false,
  resolvePlan: vi.fn(async () => undefined),
  sessionState: {
    sessionId: null as string | null,
    agentSessionId: null as string | null,
    state: "idle",
    lastError: null,
    streamingMessageId: null,
    backend: "sunsetz",
    title: "",
  },
  sessions: [] as Array<Record<string, unknown>>,
  projects: [] as Array<Record<string, unknown>>,
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
    backend: "sunsetz",
    title: "Scheduled",
  })),
  sessionSend: vi.fn(async () => undefined),
  sessionSendV2: vi.fn(async () => ({
    version: 2,
    snapshot: {
      sessionId: "memory-session",
      agentSessionId: "agent",
      state: "streaming",
      lastError: null,
      streamingMessageId: "memory-stream",
      backend: "sunsetz",
      title: "Memory",
    },
  })),
  skillInventory: vi.fn(async () => ({
    version: 1,
    items: [] as Array<Record<string, unknown>>,
  })),
  skillRank: vi.fn(async () => ({
    version: 1,
    disposition: "suggestion_only" as const,
    requiresExplicitAcceptance: true as const,
    items: [] as SkillMetadataRankingResultV1["items"],
  })),
  sessionAutoTitle: vi.fn(async () => null),
  pathsClassify: vi.fn(async (paths: string[]) =>
    paths.map((path) => ({
      path,
      name: path.split(/[/\\]/).pop() || path,
      isDir: false,
      exists: true,
    })),
  ),
  automationBind: vi.fn(async () => undefined),
  automationComplete: vi.fn(async () => undefined),
  sessionSearch: vi.fn(async () => [] as Array<Record<string, unknown>>),
  sessionMessages: vi.fn(async () => [] as Array<Record<string, unknown>>),
  skillApprove: vi.fn(async () => ({
    path: "/tmp/migration-helper",
    slug: "migration-helper",
    scope: "user",
    overwritten: false,
  })),
  settingsGet: vi.fn(async () => ({
    theme: "dark",
    locale: "zh",
    sessionDataMode: "independent",
    manualCliPath: null,
    permissionPolicy: "ask",
    modelId: null,
    effort: "medium",
    mode: "agent",
    onboardingDone: true,
    setupSkipped: false,
    setupWizardCompleted: true,
    authSetupDeferred: false,
    defaultOpenTarget: "finder",
    composerPrefsScope: "global",
    acpServerAddr: null,
    maxConcurrentAgents: 3,
    agentIdleMinutes: 30,
    streamStallSeconds: 120,
    sandboxProfile: "off",
    storeApiKeysInKeychain: false,
  })),
  settingsSet: vi.fn(async () => undefined),
  settingsPatch: vi.fn(async (patch: Record<string, unknown>) => ({
    theme: "dark",
    locale: "zh",
    sessionDataMode: "independent",
    manualCliPath: null,
    permissionPolicy: "ask",
    modelId: null,
    effort: "medium",
    mode: "agent",
    onboardingDone: true,
    setupSkipped: false,
    setupWizardCompleted: true,
    authSetupDeferred: false,
    defaultOpenTarget: "finder",
    composerPrefsScope: "global",
    acpServerAddr: null,
    maxConcurrentAgents: 3,
    agentIdleMinutes: 30,
    streamStallSeconds: 120,
    sandboxProfile: "off",
    storeApiKeysInKeychain: false,
    ...patch,
  })),
}));

const recoveryCapture = vi.hoisted(() => ({
  get: vi.fn(),
  put: vi.fn(),
  migrate: vi.fn(),
  delete: vi.fn(),
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
    projectsList: vi.fn(async () => apiListenerCapture.projects),
    sessionsList: vi.fn(async () => apiListenerCapture.sessions),
    settingsGet: apiListenerCapture.settingsGet,
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
    skillCandidateApproveV2: apiListenerCapture.skillApprove,
    sessionSearchV1: apiListenerCapture.sessionSearch,
    sessionMessages: apiListenerCapture.sessionMessages,
    sessionCreate: apiListenerCapture.sessionCreate,
    sessionDisconnect: apiListenerCapture.sessionDisconnect,
    sessionConnect: apiListenerCapture.sessionConnect,
    sessionSend: apiListenerCapture.sessionSend,
    sessionSendV2: apiListenerCapture.sessionSendV2,
    skillInventoryV1: apiListenerCapture.skillInventory,
    skillMetadataRankV1: apiListenerCapture.skillRank,
    sessionAutoTitle: apiListenerCapture.sessionAutoTitle,
    pathsClassify: apiListenerCapture.pathsClassify,
    automationClaimBindV1: apiListenerCapture.automationBind,
    automationClaimCompleteV1: apiListenerCapture.automationComplete,
    settingsSet: apiListenerCapture.settingsSet,
    settingsPatchV1: apiListenerCapture.settingsPatch,
    trayRefresh: vi.fn(async () => undefined),
  };
});

vi.mock("@/lib/composerRecovery", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("@/lib/composerRecovery")>();
  return {
    ...actual,
    composerRecoveryGetV1: recoveryCapture.get,
    composerRecoveryPutV1: recoveryCapture.put,
    composerRecoveryMigrateV1: recoveryCapture.migrate,
    composerRecoveryDeleteV1: recoveryCapture.delete,
  };
});

const resourceCapture = vi.hoisted(() => ({
  current: null as ResourceViewerProps | null,
}));

vi.mock("@/components/ResourceViewer", () => ({
  ResourceViewer: (props: ResourceViewerProps) => {
    resourceCapture.current = props;
    return <aside data-testid="resource-viewer-mock" />;
  },
}));

const composerCapture = vi.hoisted(() => ({
  current: null as ComposerDockProps | null,
}));

vi.mock("@/components/ComposerDock", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("@/components/ComposerDock")>();
  return {
    ...actual,
    ComposerDock: (props: ComposerDockProps) => {
      composerCapture.current = props;
      return <actual.ComposerDock {...props} />;
    },
  };
});

const settingsCapture = vi.hoisted(() => ({
  current: null as SettingsPageProps | null,
}));

vi.mock("@/components/SettingsPage", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("@/components/SettingsPage")>();
  return {
    ...actual,
    SettingsPage: (props: SettingsPageProps) => {
      settingsCapture.current = props;
      return <actual.SettingsPage {...props} />;
    },
  };
});

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
    backend: "sunsetz",
    title: "",
  };
  apiListenerCapture.sessions = [];
  apiListenerCapture.projects = [];
  apiListenerCapture.interactionRows = [];
  apiListenerCapture.candidateResponses = [];
  apiListenerCapture.searchHits = [];
  apiListenerCapture.sessionSearch.mockImplementation(async () =>
    apiListenerCapture.searchHits,
  );
  apiListenerCapture.skillInventory
    .mockReset()
    .mockResolvedValue({ version: 1, items: [] });
  apiListenerCapture.skillRank.mockReset().mockResolvedValue({
    version: 1,
    disposition: "suggestion_only",
    requiresExplicitAcceptance: true,
    items: [] as SkillMetadataRankingResultV1["items"],
  });
  recoveryCapture.get.mockReset().mockResolvedValue(null);
  recoveryCapture.put.mockReset().mockImplementation(
    async (key: string, _state: unknown, revision: number) => ({
      version: 1,
      key,
      revision: revision + 1,
    }),
  );
  recoveryCapture.migrate.mockReset().mockImplementation(
    async (toKey: string, fromRevision: number, toRevision: number) => ({
      version: 1,
      fromKey: "__draft__",
      fromRevision: fromRevision + 1,
      toKey,
      toRevision: toRevision + 1,
    }),
  );
  recoveryCapture.delete.mockReset().mockImplementation(
    async (key: string, revision: number) => ({
      version: 1,
      key,
      revision: revision + 1,
    }),
  );
});

afterEach(() => {
  cleanup();
  localStorage.clear();
  sidebarCapture.current = null;
  settingsCapture.current = null;
  composerCapture.current = null;
  resourceCapture.current = null;
  apiListenerCapture.handlers.clear();
  apiListenerCapture.tauri = false;
  apiListenerCapture.resolvePlan.mockClear();
  apiListenerCapture.sessionCreate.mockClear();
  apiListenerCapture.sessionDisconnect.mockClear();
  apiListenerCapture.sessionConnect.mockClear();
  apiListenerCapture.sessionSend.mockClear();
  apiListenerCapture.sessionSendV2.mockClear();
  apiListenerCapture.sessionAutoTitle.mockClear();
  apiListenerCapture.pathsClassify.mockClear();
  apiListenerCapture.automationBind.mockClear();
  apiListenerCapture.automationComplete.mockClear();
  apiListenerCapture.sessionSearch.mockClear();
  apiListenerCapture.sessionMessages.mockClear();
  apiListenerCapture.skillInventory.mockClear();
  apiListenerCapture.skillRank.mockClear();
  apiListenerCapture.skillApprove.mockClear();
  apiListenerCapture.settingsSet.mockClear();
  apiListenerCapture.settingsGet.mockClear();
  apiListenerCapture.settingsPatch.mockClear();
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
      expect(recoveryCapture.get).not.toHaveBeenCalled();

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
      expect(
        screen.getByText(/No matching settings|没有匹配的设置/),
      ).toBeTruthy();
    },
    20_000,
  );

  it(
    "restores composer state on reload and keeps drafts isolated while switching sessions",
    async () => {
      apiListenerCapture.tauri = true;
      apiListenerCapture.sessionState = {
        ...apiListenerCapture.sessionState,
        sessionId: "session-1",
        agentSessionId: "agent-1",
        state: "ready",
      };
      apiListenerCapture.sessions = [
        {
          id: "session-1",
          title: "One",
          projectId: null,
          updatedAt: "2026-08-24T00:00:00Z",
        },
        {
          id: "session-2",
          title: "Two",
          projectId: null,
          updatedAt: "2026-08-24T00:00:01Z",
        },
      ];
      recoveryCapture.get.mockImplementation(async (key: string) => ({
        version: 1,
        key,
        revision: key === "session-1" ? 3 : 7,
        state: {
          draft: key === "session-1" ? "draft one" : "draft two",
          attachments:
            key === "session-1"
              ? [{ path: "/tmp/one.txt", name: "one.txt", isDir: false }]
              : [],
          queue:
            key === "session-1"
              ? [
                  {
                    id: "queued-one",
                    storedDisplay: "follow up",
                    attachments: [],
                    goalMode: false,
                    createdAt: 1,
                  },
                ]
              : [],
        },
        filteredAttachmentCount: 0,
        filteredQueueItemCount: 0,
      }));

      const { default: App } = await import("./App");
      render(<App />);
      await screen.findByTestId("workbench-shell");
      await waitFor(() => {
        expect(composerCapture.current?.draft).toBe("draft one");
        expect(composerCapture.current?.attachments[0]?.path).toBe(
          "/tmp/one.txt",
        );
        expect(composerCapture.current?.queue.items[0]?.id).toBe("queued-one");
        expect(composerCapture.current?.queue.flushHold).toBe(true);
      });

      act(() => sidebarCapture.current?.tree.onOpenSession("session-2", null));
      await waitFor(() => {
        expect(composerCapture.current?.draft).toBe("draft two");
      });
      act(() => composerCapture.current?.onDraftChange("edited draft two"));
      await waitFor(() => {
        expect(composerCapture.current?.draft).toBe("edited draft two");
      });

      act(() => sidebarCapture.current?.tree.onOpenSession("session-1", null));
      await waitFor(() => {
        expect(composerCapture.current?.draft).toBe("draft one");
      });
      act(() => sidebarCapture.current?.tree.onOpenSession("session-2", null));
      await waitFor(() => {
        expect(composerCapture.current?.draft).toBe("edited draft two");
      });
      expect(
        recoveryCapture.get.mock.calls.filter(([key]) => key === "session-2"),
      ).toHaveLength(1);
    },
    20_000,
  );

  it(
    "reclassifies restored attachments and migrates draft recovery before first send",
    async () => {
      apiListenerCapture.tauri = true;
      apiListenerCapture.sessionState = {
        ...apiListenerCapture.sessionState,
        sessionId: null,
        state: "idle",
      };
      recoveryCapture.get.mockImplementation(async (key: string) => {
        if (key !== "__draft__") return null;
        return {
          version: 1,
          key,
          revision: 4,
          state: {
            draft: "restored prompt",
            attachments: [
              { path: "/tmp/stored.txt", name: "stored.txt", isDir: false },
            ],
            queue: [],
          },
          filteredAttachmentCount: 0,
          filteredQueueItemCount: 0,
        };
      });
      apiListenerCapture.sessionCreate.mockResolvedValueOnce({
        id: "materialized-session",
        title: "Materialized",
      });
      apiListenerCapture.pathsClassify.mockResolvedValueOnce([
        {
          path: "/tmp/canonical.txt",
          name: "canonical.txt",
          isDir: false,
          exists: true,
        },
      ]);

      const { default: App } = await import("./App");
      render(<App />);
      await screen.findByTestId("workbench-shell");
      await waitFor(() => {
        expect(composerCapture.current?.draft).toBe("restored prompt");
      });

      await act(async () => {
        await composerCapture.current?.onSend();
      });
      await waitFor(() => {
        expect(recoveryCapture.migrate).toHaveBeenCalledWith(
          "materialized-session",
          5,
          0,
        );
        expect(apiListenerCapture.sessionSend).toHaveBeenCalledTimes(1);
      });

      expect(apiListenerCapture.pathsClassify).toHaveBeenCalledWith([
        "/tmp/stored.txt",
      ]);
      expect(apiListenerCapture.sessionSend).toHaveBeenCalledWith(
        expect.any(String),
        "restored prompt",
        [
          {
            path: "/tmp/canonical.txt",
            name: "canonical.txt",
            isDir: false,
          },
        ],
      );
      expect(recoveryCapture.migrate.mock.invocationCallOrder[0]).toBeLessThan(
        apiListenerCapture.sessionSend.mock.invocationCallOrder[0]!,
      );
    },
    20_000,
  );

  it(
    "keeps the restored composer intact when attachment reclassification fails",
    async () => {
      apiListenerCapture.tauri = true;
      recoveryCapture.get.mockImplementation(async (key: string) => ({
        version: 1,
        key,
        revision: 2,
        state: {
          draft: "do not lose this",
          attachments: [
            { path: "/tmp/missing.txt", name: "missing.txt", isDir: false },
          ],
          queue: [],
        },
        filteredAttachmentCount: 0,
        filteredQueueItemCount: 0,
      }));
      apiListenerCapture.pathsClassify.mockResolvedValueOnce([
        {
          path: "/tmp/missing.txt",
          name: "missing.txt",
          isDir: false,
          exists: false,
        },
      ]);

      const { default: App } = await import("./App");
      render(<App />);
      await screen.findByTestId("workbench-shell");
      await waitFor(() => {
        expect(composerCapture.current?.draft).toBe("do not lose this");
      });

      await act(async () => {
        await composerCapture.current?.onSend();
      });

      expect(composerCapture.current?.draft).toBe("do not lose this");
      expect(composerCapture.current?.attachments[0]?.path).toBe(
        "/tmp/missing.txt",
      );
      expect(apiListenerCapture.sessionCreate).not.toHaveBeenCalled();
      expect(recoveryCapture.migrate).not.toHaveBeenCalled();
      expect(apiListenerCapture.sessionSend).not.toHaveBeenCalled();
    },
    20_000,
  );

  it(
    "sends an explicitly reviewed Memory pack through the Host-owned v2 path once",
    async () => {
      apiListenerCapture.tauri = true;
      apiListenerCapture.sessionState = {
        ...apiListenerCapture.sessionState,
        sessionId: "memory-session",
        agentSessionId: "agent-memory",
        state: "ready",
        title: "Memory",
      };
      apiListenerCapture.sessions = [
        {
          id: "memory-session",
          title: "Memory",
          projectId: null,
          updatedAt: "2026-08-24T00:00:00Z",
        },
      ];
      const pack = {
        version: 1 as const,
        items: [
          {
            candidateId: "memory-candidate",
            contentHash: "a".repeat(64),
            type: "user_preference" as const,
            content: "Prefer concise, verified answers.",
            source: {
              sessionId: "source-session",
              messageId: "source-message",
            },
          },
        ],
      };

      const { default: App } = await import("./App");
      render(<App />);
      await screen.findByTestId("workbench-shell");

      act(() => sidebarCapture.current?.account.onSettings());
      await waitFor(() => expect(settingsCapture.current).not.toBeNull());
      act(() => settingsCapture.current?.onUseMemoryContext?.(pack));
      await waitFor(() => {
        expect(composerCapture.current?.memory?.pack).toEqual(pack);
      });

      act(() => composerCapture.current?.onDraftChange("Use my reviewed preference"));
      await waitFor(() => {
        expect(composerCapture.current?.draft).toBe(
          "Use my reviewed preference",
        );
      });
      await act(async () => {
        await composerCapture.current?.onSend();
      });

      await waitFor(() => {
        expect(apiListenerCapture.sessionSendV2).toHaveBeenCalledTimes(1);
      });
      expect(apiListenerCapture.sessionSendV2).toHaveBeenCalledWith({
        sessionId: "memory-session",
        text: expect.any(String),
        displayText: "Use my reviewed preference",
        attachments: [],
        memoryContextPack: {
          version: 1,
          selections: [
            {
              id: "memory-candidate",
              expectedContentHash: "a".repeat(64),
            },
          ],
        },
        skillSelections: [],
      });
      expect(apiListenerCapture.sessionSend).not.toHaveBeenCalled();
      await waitFor(() => {
        expect(composerCapture.current?.memory?.pack).toBeNull();
      });
    },
    20_000,
  );

  it(
    "sends a selected Skill only through the fresh identity and tree-hash v2 path",
    async () => {
      apiListenerCapture.tauri = true;
      apiListenerCapture.sessionState = {
        ...apiListenerCapture.sessionState,
        sessionId: "skill-session",
        agentSessionId: "agent-skill",
        state: "ready",
        title: "Skill",
      };
      apiListenerCapture.sessions = [
        {
          id: "skill-session",
          title: "Skill",
          projectId: null,
          updatedAt: "2026-08-24T00:00:00Z",
        },
      ];
      apiListenerCapture.skillInventory.mockResolvedValue({
        version: 1,
        items: [
          {
            version: 1,
            id: "a".repeat(64),
            name: "review",
            description: "Review the current change",
            whenToUse: "Before merging a change",
            source: "user",
            path: "/runtime/skills/review",
            enabled: true,
            userInvocable: true,
            treeHash: "b".repeat(64),
            sourceCandidateId: null,
          },
        ],
      });

      const { default: App } = await import("./App");
      render(<App />);
      await screen.findByTestId("workbench-shell");
      await waitFor(() => {
        expect(
          composerCapture.current?.menu.entries.some(
            (entry) =>
              entry.kind === "slash" &&
              entry.item.kind === "skill" &&
              entry.item.name === "review",
          ),
        ).toBe(true);
      });
      const skillEntry = composerCapture.current?.menu.entries.find(
        (entry) =>
          entry.kind === "slash" &&
          entry.item.kind === "skill" &&
          entry.item.name === "review",
      );
      if (!skillEntry || skillEntry.kind !== "slash") {
        throw new Error("review Skill missing from composer");
      }

      act(() => composerCapture.current?.menu.onSelectSlash(skillEntry.item));
      await waitFor(() => {
        expect(composerCapture.current?.draft).toContain("[[skill-v1:review|");
      });
      await act(async () => {
        await composerCapture.current?.onSend();
      });

      await waitFor(() => {
        expect(apiListenerCapture.sessionSendV2).toHaveBeenCalledTimes(1);
      });
      expect(apiListenerCapture.sessionSendV2).toHaveBeenCalledWith({
        sessionId: "skill-session",
        text: expect.stringMatching(/^\/review(?:\n|$)/),
        displayText: expect.stringContaining("[[skill:review]]"),
        attachments: [],
        memoryContextPack: null,
        skillSelections: [
          {
            id: "a".repeat(64),
            expectedTreeHash: "b".repeat(64),
            selection: "explicit",
          },
        ],
      });
      expect(apiListenerCapture.sessionSend).not.toHaveBeenCalled();
    },
    20_000,
  );

  it(
    "pins an explicitly accepted ranked suggestion into the v1 Skill chip",
    async () => {
      apiListenerCapture.tauri = true;
      apiListenerCapture.sessionState = {
        ...apiListenerCapture.sessionState,
        sessionId: "ranked-skill-session",
        agentSessionId: "agent-ranked-skill",
        state: "ready",
        title: "Ranked Skill",
      };
      apiListenerCapture.sessions = [
        {
          id: "ranked-skill-session",
          title: "Ranked Skill",
          projectId: null,
          updatedAt: "2026-08-24T00:00:00Z",
        },
      ];
      const skillId = "d".repeat(64);
      const treeHash = "e".repeat(64);
      const skill = {
        id: skillId,
        name: "review",
        description: "Review the current change",
        whenToUse: "Before merging a change",
        source: "user" as const,
        enabled: true,
        userInvocable: true,
        treeHash,
        sourceCandidateId: null,
      };
      apiListenerCapture.skillInventory.mockResolvedValue({
        version: 1,
        items: [skill],
      });
      apiListenerCapture.skillRank.mockResolvedValue({
        version: 1,
        disposition: "suggestion_only",
        requiresExplicitAcceptance: true,
        items: [
          {
            skill: {
              id: skillId,
              name: "review",
              treeHash,
              sourceCandidateId: null,
            },
            score: 10,
            matchedTerms: ["review"],
          },
        ],
      });

      const { default: App } = await import("./App");
      render(<App />);
      await screen.findByTestId("workbench-shell");
      await waitFor(() => {
        expect(
          composerCapture.current?.menu.entries.some(
            (entry) =>
              entry.kind === "slash" &&
              entry.item.kind === "skill" &&
              entry.item.name === "review",
          ),
        ).toBe(true);
      });

      act(() =>
        composerCapture.current?.onDraftChange("Please review this change"),
      );
      await waitFor(() => {
        expect(composerCapture.current?.draft).toBe("Please review this change");
      });
      act(() => composerCapture.current?.menu.onTogglePlus());
      await waitFor(() => {
        expect(apiListenerCapture.skillRank).toHaveBeenCalledWith(
          "Please review this change",
          null,
          4,
        );
      });
      await waitFor(() => {
        expect(
          composerCapture.current?.menu.entries.some(
            (entry) =>
              entry.kind === "slash" &&
              entry.item.kind === "skill" &&
              entry.item.name === "review" &&
              entry.item.suggested === true,
          ),
        ).toBe(true);
      });
      const suggestedEntry = composerCapture.current?.menu.entries.find(
        (entry) =>
          entry.kind === "slash" &&
          entry.item.kind === "skill" &&
          entry.item.name === "review" &&
          entry.item.suggested === true,
      );
      if (!suggestedEntry || suggestedEntry.kind !== "slash") {
        throw new Error("ranked review Skill missing from composer");
      }

      act(() =>
        composerCapture.current?.menu.onSelectSlash(suggestedEntry.item),
      );
      const expectedToken = `[[skill-v1:review|${skillId}|${treeHash}|accepted_suggestion]]`;
      await waitFor(() => {
        expect(composerCapture.current?.draft).toBe(
          `Please review this change ${expectedToken} `,
        );
      });
    },
    20_000,
  );

  it(
    "refuses to rebind a recovered Skill chip when the same name has a new tree",
    async () => {
      apiListenerCapture.tauri = true;
      apiListenerCapture.sessionState = {
        ...apiListenerCapture.sessionState,
        sessionId: "skill-session",
        agentSessionId: "agent-skill",
        state: "ready",
        title: "Skill",
      };
      apiListenerCapture.sessions = [
        {
          id: "skill-session",
          title: "Skill",
          projectId: null,
          updatedAt: "2026-08-24T00:00:00Z",
        },
      ];
      const oldToken = `[[skill-v1:review|${"a".repeat(64)}|${"b".repeat(64)}|accepted_suggestion]]`;
      recoveryCapture.get.mockImplementation(async (key: string) => ({
        version: 1,
        key,
        revision: 4,
        state: { draft: oldToken, attachments: [], queue: [] },
        filteredAttachmentCount: 0,
        filteredQueueItemCount: 0,
      }));
      apiListenerCapture.skillInventory.mockResolvedValue({
        version: 1,
        items: [
          {
            id: "a".repeat(64),
            name: "review",
            description: "Review the current change",
            whenToUse: "Before merging a change",
            source: "user",
            enabled: true,
            userInvocable: true,
            treeHash: "c".repeat(64),
            sourceCandidateId: null,
          },
        ],
      });

      const { default: App } = await import("./App");
      render(<App />);
      await screen.findByTestId("workbench-shell");
      await waitFor(() => expect(composerCapture.current?.draft).toBe(oldToken));
      await act(async () => {
        await composerCapture.current?.onSend();
      });

      expect(apiListenerCapture.sessionSendV2).not.toHaveBeenCalled();
      expect(apiListenerCapture.sessionSend).not.toHaveBeenCalled();
      expect(composerCapture.current?.draft).toBe(oldToken);
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
    "drives the slash palette from editor events and keeps Escape dismissal sticky",
    async () => {
      const { default: App } = await import("./App");
      render(<App />);
      await screen.findByTestId("workbench-shell");

      const editor = screen.getByRole("textbox");
      editor.textContent = "/go";
      fireEvent.input(editor);
      expect(await screen.findByRole("listbox")).toBeTruthy();

      fireEvent.keyDown(editor, { key: "Escape" });
      await waitFor(() => {
        expect(screen.queryByRole("listbox")).toBeNull();
      });

      // ComposerEditor reports again on keyup. The unchanged token stays
      // dismissed instead of immediately reopening the palette.
      fireEvent.keyUp(editor, { key: "Escape" });
      await act(async () => Promise.resolve());
      expect(screen.queryByRole("listbox")).toBeNull();

      editor.textContent = "/goal";
      fireEvent.input(editor);
      expect(await screen.findByRole("listbox")).toBeTruthy();
    },
    20_000,
  );

  it(
    "keeps ComposerDock commands behind App-owned orchestration",
    async () => {
      const invoke = vi.fn(
        async (command: string, args: Record<string, unknown> = {}) => {
          if (command === "composer_prefs_set") {
            return {
              modelId: args.modelId ?? "sunsetz-4.5",
              effort: args.effort ?? "medium",
              mode: args.mode ?? "agent",
              permissionPolicy: "ask",
              scope: "global",
              source: "test",
            };
          }
          return null;
        },
      );
      Object.assign(window, { __TAURI_INTERNALS__: { invoke } });

      const { default: App } = await import("./App");
      render(<App />);
      await waitFor(() => expect(composerCapture.current).not.toBeNull());

      const dock = composerCapture.current;
      if (!dock) throw new Error("composer missing");
      expect(document.querySelector(".composer-wrap")).toBe(dock.refs.wrap.current);
      expect(document.querySelector(".composer-dock")).toBeTruthy();
      const modelId = dock.preferences.models[0]?.id ?? "sunsetz-4.5";
      const attachment = {
        path: "/tmp/coverage.png",
        name: "coverage.png",
        isDir: false,
      };

      await act(async () => {
        dock.project.onSelect(null);
        dock.project.onAdd();
        dock.project.onSwitchWorktree({
          path: "/tmp/worktree",
          branch: "coverage",
          detached: false,
          isMain: false,
          locked: false,
          prunable: false,
        });
        dock.menu.onTogglePlus();
        dock.preferences.onMode("ask");
        dock.preferences.onPolicy("ask");
        dock.preferences.onModel("invalid-model");
        dock.preferences.onModel(modelId);
        dock.preferences.onEffort("high");
        dock.preferences.onReset();
        dock.preferences.onClearGoal();
        dock.onRemoveAttachment(attachment);
        dock.onAddAttachment(attachment);
        dock.onCompact();
        await Promise.resolve();
        await Promise.resolve();
      });

      await waitFor(() => {
        expect(composerCapture.current?.menu.showPlus).toBe(true);
      });
      act(() => composerCapture.current?.menu.onTogglePlus());
      await waitFor(() => {
        expect(composerCapture.current?.menu.showPlus).toBe(false);
      });

      expect(
        invoke.mock.calls.filter(([command]) => command === "composer_prefs_set")
          .length,
      ).toBeGreaterThanOrEqual(3);
    },
    20_000,
  );

  it(
    "loads the resource viewer only after opening the right pane",
    async () => {
      const user = userEvent.setup();
      const { default: App } = await import("./App");
      render(<App />);
      await screen.findByTestId("workbench-shell");

      expect(resourceCapture.current).toBeNull();
      await user.click(
        screen.getByRole("button", { name: /Show files|显示文件/ }),
      );
      await waitFor(() => expect(resourceCapture.current).not.toBeNull());
      expect(await screen.findByTestId("resource-viewer-mock")).toBeTruthy();

      act(() => {
        resourceCapture.current?.onOpenRequestConsumed?.();
        resourceCapture.current?.onClose?.();
      });
      await waitFor(() => {
        expect(screen.queryByTestId("resource-viewer-mock")).toBeNull();
      });
    },
    20_000,
  );

  it(
    "patches only the settings field changed by each settings control",
    async () => {
      const { default: App } = await import("./App");
      render(<App />);
      await screen.findByTestId("workbench-shell");
      const sidebar = sidebarCapture.current;
      expect(sidebar).not.toBeNull();
      if (!sidebar) return;

      act(() => sidebar.account.onSettings());
      await waitFor(() => expect(settingsCapture.current).not.toBeNull());
      const settings = settingsCapture.current;
      if (!settings) return;

      act(() => {
        settings.onLocale("en");
        settings.onSessionDataMode("independent");
        settings.onPrefsScope?.("project");
        settings.onCliBlur("/opt/sunsetz/runtime");
        settings.onAcpServerAddr("localhost:9339");
        settings.onMaxConcurrentAgents?.(4);
        settings.onAgentIdleMinutes?.(15);
        settings.onStreamStallSeconds?.(90);
        settings.onStoreApiKeysInKeychain?.(true);
        settings.onDefaultOpenTarget?.("finder");
        settings.onSandboxProfile?.("read_only");
      });

      await waitFor(() => {
        expect(apiListenerCapture.settingsPatch.mock.calls).toEqual(
          expect.arrayContaining([
            [{ locale: "en" }],
            [{ sessionDataMode: "independent" }],
            [{ composerPrefsScope: "project" }],
            [{ manualCliPath: "/opt/sunsetz/runtime" }],
            [{ acpServerAddr: "localhost:9339" }],
            [{ maxConcurrentAgents: 4 }],
            [{ agentIdleMinutes: 15 }],
            [{ streamStallSeconds: 90 }],
            [{ storeApiKeysInKeychain: true }],
            [{ defaultOpenTarget: "finder" }],
            [{ sandboxProfile: "read_only" }],
          ]),
        );
      });
    },
    20_000,
  );

  it(
    "rolls sandbox UI back to authoritative Host settings when patching fails",
    async () => {
      const { default: App } = await import("./App");
      render(<App />);
      await screen.findByTestId("workbench-shell");
      const sidebar = sidebarCapture.current;
      if (!sidebar) throw new Error("sidebar missing");
      act(() => sidebar.account.onSettings());
      await waitFor(() => expect(settingsCapture.current).not.toBeNull());

      apiListenerCapture.settingsPatch.mockRejectedValueOnce(
        new Error("LOCK_BUSY: settings"),
      );
      act(() => settingsCapture.current?.onSandboxProfile?.("read_only"));

      await waitFor(() => {
        expect(settingsCapture.current?.sandboxProfile).toBe("off");
        expect(screen.getByText(/LOCK_BUSY: settings/)).toBeTruthy();
      });
    },
    20_000,
  );

  it(
    "uses the persisted Host user message id as memory provenance",
    async () => {
      apiListenerCapture.tauri = true;
      apiListenerCapture.sessionState = {
        ...apiListenerCapture.sessionState,
        sessionId: "session-memory",
        agentSessionId: "agent-memory",
        state: "ready",
      };
      apiListenerCapture.sessionMessages.mockResolvedValueOnce([
        {
          id: "host-user-uuid",
          role: "user",
          content: "remember this",
          createdAt: "2026-08-24T00:00:00Z",
        },
        {
          id: "host-assistant-uuid",
          role: "assistant",
          content: "done",
          createdAt: "2026-08-24T00:00:01Z",
        },
      ]);

      const { default: App } = await import("./App");
      render(<App />);
      await screen.findByTestId("workbench-shell");
      const sidebar = sidebarCapture.current;
      if (!sidebar) throw new Error("sidebar missing");
      act(() => sidebar.account.onSettings());

      await waitFor(() => {
        expect(apiListenerCapture.sessionMessages).toHaveBeenCalledWith(
          "session-memory",
        );
        expect(settingsCapture.current?.memorySource).toEqual({
          sessionId: "session-memory",
          messageId: "host-user-uuid",
        });
      });
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
    "reviews an edited Skill candidate with separate stale and final hashes",
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
        reviewContentHash: "a".repeat(64),
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
      fireEvent.change(screen.getByDisplayValue("Review migration workflow"), {
        target: { value: "Review edited migration workflow" },
      });
      fireEvent.click(
        screen.getByRole("button", { name: /Save skill|保存技能/ }),
      );
      await waitFor(() => {
        expect(apiListenerCapture.skillApprove).toHaveBeenCalledWith(
          expect.objectContaining({
            id: "candidate",
            expectedContentHash: "a".repeat(64),
            finalContentHash:
              "63e654cb61e4cb6c5131d91af44cac1b5d77281b789428f090b21c73aa0d26e5",
          }),
        );
      });
    },
    20_000,
  );
});
