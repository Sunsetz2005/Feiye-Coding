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
    sessionsList: vi.fn(async () => []),
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
});

afterEach(() => {
  cleanup();
  localStorage.clear();
  sidebarCapture.current = null;
  apiListenerCapture.handlers.clear();
  apiListenerCapture.tauri = false;
  apiListenerCapture.resolvePlan.mockClear();
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
});
