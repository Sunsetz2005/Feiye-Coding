// @vitest-environment jsdom

import {
  act,
  cleanup,
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

vi.mock("@/components/ResourceViewer", () => ({
  ResourceViewer: () => <aside data-testid="resource-viewer-mock" />,
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
});
