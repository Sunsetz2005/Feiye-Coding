// @vitest-environment jsdom

import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import {
  afterAll,
  afterEach,
  beforeAll,
  describe,
  expect,
  it,
  vi,
} from "vitest";
import { SidebarNavigator } from "./SidebarNavigator";
import type {
  AccountStatus,
  CustomProvider,
  ProjectGitSummaryV1,
  SessionPreviewV1,
} from "@/lib/api";

beforeAll(() => {
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
});

afterAll(() => {
  vi.unstubAllGlobals();
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

function createProps(
  overrides: Partial<React.ComponentProps<typeof SidebarNavigator>> = {},
): React.ComponentProps<typeof SidebarNavigator> {
  return {
    collapsed: false,
    dragZone: null,
    labels: {
      chrome: { hide: "Hide sidebar" },
      drag: {
        addProjectTitle: "Add project",
        addProjectHint: "Drop a folder",
      },
      navigation: {
        label: "Workspace",
        newSession: "New task",
        search: "Search",
        scheduled: "Scheduled",
        plugins: "Extensions",
      },
      tree: {
        projects: "Projects",
        addProject: "Add project",
        noProjects: "No projects",
        collapseProject: "Collapse project",
        expandProject: "Expand project",
        untrusted: "Untrusted",
        menu: "More",
        organize: "Organize sidebar",
        groupByProject: "By project",
        groupByList: "In a list",
        chatSort: "Chat sort",
        sortPriority: "Priority",
        sortRecent: "Recently updated",
        newConversation: "New conversation",
        editProject: "Edit project",
        collapseProjects: "Collapse projects",
        expandProjects: "Expand projects",
        trustProject: "Trust project",
        noChats: "No tasks",
        otherSessions: "Other tasks",
        untitled: "Untitled",
        scheduledTag: "Scheduled task",
        answerNeeded: "Answer needed",
        sessionWorking: "Working",
        previewTasks: "{count} tasks",
        previewActive: "{count} active",
        previewUpdated: "Updated {time}",
        previewPinned: "Pinned",
        previewNoSummary: "No visible messages",
        previewGitRef: "Git · {ref}",
        previewGitAhead: "Ahead {count}",
        previewGitBehind: "Behind {count}",
        previewGitDirty: "{count} changed",
        previewGitConflicts: "{count} conflicts",
        previewGitCountsCapped: "Counts capped",
        unarchive: "Unarchive",
        archive: "Archive",
      },
      account: {
        trigger: "Account",
        settings: "Settings",
        theme: "Theme",
        themeLight: "Light",
        themeDark: "Dark",
        local: "Local",
        signedIn: "Signed in",
        signedOut: "Signed out",
        login: "Log in",
        logout: "Log out",
        remaining: "Remaining",
        usage: "Usage",
        customProvider: "Provider",
        resetsAt: "Resets",
      },
    },
    chrome: {
      useCustomWindowChrome: false,
      onHide: vi.fn(),
      onToggleMaximize: vi.fn(),
    },
    navigation: {
      activePane: "chat",
      onNewSession: vi.fn(),
      onSearch: vi.fn(),
      onOpenAutomations: vi.fn(),
      onOpenExtensions: vi.fn(),
    },
    tree: {
      projectsOpen: true,
      historyOpen: true,
      activeProjectId: "project-1",
      activeSessionId: "session-1",
      busySessionIds: new Set(),
      pendingAskSessionIds: new Set(),
      projects: [
        {
          id: "project-1",
          name: "Sunsetz",
          path: "/tmp/sunsetz",
          trusted: true,
          pinned: false,
          open: true,
          sessions: [
            {
              id: "session-1",
              title: "Refactor sidebar",
              updatedAt: "2026-07-27T08:30:00Z",
              archived: false,
              scheduled: false,
            },
          ],
        },
      ],
      orphanSessions: [],
      groupBy: "project",
      sessionSort: "recent",
      onToggleProjects: vi.fn(),
      onAddProject: vi.fn(),
      onToggleProject: vi.fn(),
      onSelectProject: vi.fn(),
      onNewSessionInProject: vi.fn(),
      onEditProject: vi.fn(),
      onOrganize: vi.fn(),
      onTrustProject: vi.fn(),
      onProjectMenu: vi.fn(),
      onToggleHistory: vi.fn(),
      onOpenSession: vi.fn(),
      onArchiveSession: vi.fn(),
      onSessionMenu: vi.fn(),
      loadProjectGitSummary: vi.fn(
        async (
          projectId: string,
          _projectPath: string,
        ): Promise<ProjectGitSummaryV1> =>
          createProjectGitSummary({ projectId }),
      ),
      loadSessionPreview: vi.fn(
        async (sessionId: string): Promise<SessionPreviewV1> => ({
          version: 1,
          sessionId,
          projectId: "project-1",
          title: "Refactor sidebar",
          updatedAt: "2026-07-27T08:30:00Z",
          modelId: "sunsetz-4.5",
          contextUsage: null,
          archived: false,
          scheduled: false,
          recentUserSummary: "Please refine the sidebar preview.",
          recentAssistantSummary: "Added a bounded, read-only preview.",
        }),
      ),
    },
    account: {
      open: false,
      theme: "dark",
      account: null,
      activeProvider: null,
      busy: false,
      customRouteActive: false,
      onClose: vi.fn(),
      onToggle: vi.fn(),
      onSettings: vi.fn(),
      onAccountSettings: vi.fn(),
      onToggleTheme: vi.fn(),
      onLogin: vi.fn(),
      onLogout: vi.fn(),
    },
    ...overrides,
  };
}

function createProjectGitSummary(
  overrides: Partial<ProjectGitSummaryV1> = {},
): ProjectGitSummaryV1 {
  return {
    version: 1,
    projectId: "project-1",
    available: true,
    isRepo: true,
    branch: "main",
    ahead: 2,
    behind: 1,
    dirty: 4,
    conflicts: 1,
    countsCapped: false,
    head: "0123456789ab",
    observedAt: "2026-08-24T00:00:00Z",
    source: "git_status_porcelain_v2",
    unavailableReason: null,
    ...overrides,
  };
}

function deferred<T>(): {
  promise: Promise<T>;
  resolve: (value: T) => void;
} {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise;
  });
  return { promise, resolve };
}

function projectPreview(): HTMLElement | null {
  return document.querySelector<HTMLElement>(
    ".sidebar-preview[data-kind='project']",
  );
}

function projectRowFrom(name = "Sunsetz"): HTMLElement {
  return screen.getByRole("button", { name }).closest(".tree-l2")!;
}

async function advanceTimers(ms: number): Promise<void> {
  await act(async () => {
    vi.advanceTimersByTime(ms);
    await Promise.resolve();
  });
}

function createAccountStatus(
  overrides: Partial<AccountStatus> = {},
): AccountStatus {
  return {
    profile: {
      signedIn: true,
      authMode: "oauth",
      email: "ada@example.com",
      displayName: "Ada Lovelace",
      userId: "user-1",
      teamId: null,
      principalType: null,
      expiresAt: null,
      expired: false,
      hasRefresh: true,
      oidcIssuer: null,
    },
    hasOfficialKey: false,
    hasRelayKey: false,
    relayBaseUrl: null,
    cliAuthPresent: true,
    cliFound: true,
    cliPath: "/usr/local/bin/sunsetz",
    channel: "official_oauth",
    billing: {
      available: true,
      source: "test",
      message: null,
      subscriptionTier: "Sunsetz Pro",
      creditUsagePercent: 42,
      remainingPercent: 58,
      monthlyLimit: null,
      includedUsed: null,
      totalUsed: null,
      prepaidBalance: null,
      onDemandEnabled: null,
      onDemandCap: null,
      onDemandUsed: null,
      billingPeriodStart: null,
      billingPeriodEnd: null,
      resetsAt: null,
      isUnifiedBillingUser: true,
      products: [],
      manageUrl: "",
      subscribeUrl: "",
      fetchedAt: null,
    },
    heatmap: [],
    callLogs: [],
    usageManageUrl: "",
    subscribeUrl: "",
    ...overrides,
  };
}

describe("SidebarNavigator", () => {
  it("connects project disclosures to their controlled regions", async () => {
    const user = userEvent.setup();
    const props = createProps();
    render(<SidebarNavigator {...props} />);

    const projects = screen.getByRole("button", { name: "Projects" });
    expect(projects.getAttribute("aria-expanded")).toBe("true");
    expect(projects.getAttribute("aria-controls")).toBeTruthy();

    const projectRow = screen.getByRole("button", { name: "Sunsetz" });
    expect(projectRow.getAttribute("aria-expanded")).toBe("true");
    expect(projectRow.getAttribute("aria-controls")).toBeTruthy();
    await user.click(projectRow);
    expect(props.tree.onSelectProject).toHaveBeenCalledWith("project-1");
    expect(props.tree.onToggleProject).toHaveBeenCalledWith("project-1", false);
  });

  it("shows a working spinner on a collapsed project that has a running chat", () => {
    const base = createProps();
    render(
      <SidebarNavigator
        {...createProps({
          tree: {
            ...base.tree,
            busySessionIds: new Set(["session-1"]),
            projects: [
              {
                ...base.tree.projects[0]!,
                open: false,
              },
            ],
          },
        })}
      />,
    );
    const row = projectRowFrom();
    expect(row.className).toContain("tree-l2--working");
    expect(
      within(row).getByLabelText("Working"),
    ).toBeTruthy();
  });

  it("shows working spinners on two background sessions at once", () => {
    const base = createProps();
    const extra = {
      id: "session-2",
      title: "Nightly review",
      updatedAt: "2026-07-27T08:31:00Z",
      archived: false,
      scheduled: true,
    };
    render(
      <SidebarNavigator
        {...createProps({
          tree: {
            ...base.tree,
            busySessionIds: new Set(["session-1", "session-2"]),
            projects: [
              {
                ...base.tree.projects[0]!,
                sessions: [...base.tree.projects[0]!.sessions, extra],
              },
            ],
          },
        })}
      />,
    );
    expect(screen.getAllByLabelText("Working")).toHaveLength(2);
  });

  it("uses a native current-page button for tasks", async () => {
    const user = userEvent.setup();
    const props = createProps();
    render(<SidebarNavigator {...props} />);

    const task = screen.getByRole("button", { name: "Refactor sidebar" });
    expect(task.getAttribute("aria-current")).toBe("page");
    task.focus();
    await user.keyboard(" ");
    expect(props.tree.onOpenSession).toHaveBeenCalledWith(
      "session-1",
      "project-1",
    );
  });

  it("removes the collapsed sidebar from the interaction tree", () => {
    const props = createProps({ collapsed: true });
    render(<SidebarNavigator {...props} />);

    const sidebar = screen.getByTestId("sidebar-navigator");
    expect(sidebar.getAttribute("aria-hidden")).toBe("true");
    expect(sidebar.hasAttribute("inert")).toBe(true);
  });

  it("routes chrome and primary navigation actions", async () => {
    const user = userEvent.setup();
    const base = createProps();
    const props = createProps({
      dragZone: "sidebar",
      chrome: {
        ...base.chrome,
        useCustomWindowChrome: true,
      },
      navigation: {
        ...base.navigation,
        activePane: "automations",
      },
    });
    const { container, rerender } = render(<SidebarNavigator {...props} />);

    const sidebar = screen.getByTestId("sidebar-navigator");
    expect(sidebar.className).toContain("is-drop-target");
    expect(screen.getByText("Drop a folder")).toBeTruthy();
    fireEvent.doubleClick(container.querySelector(".sidebar-chrome")!);
    expect(props.chrome.onToggleMaximize).toHaveBeenCalledTimes(1);
    expect(
      container
        .querySelector(".sidebar-chrome")
        ?.hasAttribute("data-tauri-drag-region"),
    ).toBe(false);
    expect(
      container
        .querySelector(".sidebar-chrome__drag")
        ?.hasAttribute("data-tauri-drag-region"),
    ).toBe(true);

    await user.click(screen.getByRole("button", { name: "Hide sidebar" }));
    await user.click(screen.getByRole("button", { name: "New task" }));
    await user.click(screen.getByRole("button", { name: "Search" }));
    const scheduled = screen.getByRole("button", { name: "Scheduled" });
    expect(scheduled.getAttribute("aria-current")).toBe("page");
    await user.click(scheduled);
    await user.click(screen.getByRole("button", { name: "Extensions" }));

    expect(props.chrome.onHide).toHaveBeenCalledTimes(1);
    expect(props.navigation.onNewSession).toHaveBeenCalledTimes(1);
    expect(props.navigation.onSearch).toHaveBeenCalledTimes(1);
    expect(props.navigation.onOpenAutomations).toHaveBeenCalledTimes(1);
    expect(props.navigation.onOpenExtensions).toHaveBeenCalledTimes(1);

    const mainDragProps = createProps({
      dragZone: "main",
      chrome: {
        ...base.chrome,
        useCustomWindowChrome: false,
        onToggleMaximize: vi.fn(),
      },
    });
    rerender(<SidebarNavigator {...mainDragProps} />);
    expect(sidebar.className).toContain("is-drop-idle");
    fireEvent.doubleClick(container.querySelector(".sidebar-chrome")!);
    expect(mainDragProps.chrome.onToggleMaximize).not.toHaveBeenCalled();
  });

  it("renders empty and collapsed tree states without fake rows", async () => {
    const user = userEvent.setup();
    const base = createProps();
    const emptyProps = createProps({
      tree: {
        ...base.tree,
        projects: [],
        orphanSessions: [],
      },
    });
    const { rerender } = render(<SidebarNavigator {...emptyProps} />);

    expect(screen.getByText("No projects")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Projects" }));
    await user.click(screen.getByRole("button", { name: "Add project" }));
    await user.click(screen.getByRole("button", { name: "Other tasks" }));
    expect(emptyProps.tree.onToggleProjects).toHaveBeenCalledTimes(1);
    expect(emptyProps.tree.onAddProject).toHaveBeenCalledTimes(1);
    expect(emptyProps.tree.onToggleHistory).toHaveBeenCalledTimes(1);

    const closedProps = createProps({
      tree: {
        ...base.tree,
        projectsOpen: false,
        historyOpen: false,
      },
    });
    rerender(<SidebarNavigator {...closedProps} />);
    expect(
      screen.getByRole("button", { name: "Projects" }).getAttribute(
        "aria-expanded",
      ),
    ).toBe("false");
    expect(
      screen.getByRole("button", { name: "Other tasks" }).getAttribute(
        "aria-expanded",
      ),
    ).toBe("false");
  });

  it("handles untrusted, pinned, closed, and taskless projects", async () => {
    const user = userEvent.setup();
    const base = createProps();
    const project = {
      ...base.tree.projects[0]!,
      trusted: false,
      pinned: true,
      open: false,
      sessions: [],
    };
    const props = createProps({
      tree: {
        ...base.tree,
        activeSessionId: null,
        projects: [project],
      },
    });
    const { container, rerender } = render(<SidebarNavigator {...props} />);

    const projectSelect = screen.getByRole("button", { name: "Sunsetz" });
    expect(projectSelect.getAttribute("aria-current")).toBe("page");
    expect(projectSelect.hasAttribute("disabled")).toBe(false);
    expect(projectSelect.getAttribute("aria-expanded")).toBe("false");
    expect(screen.getByText("Untrusted")).toBeTruthy();
    await user.click(projectSelect);
    expect(props.tree.onSelectProject).toHaveBeenCalledWith("project-1");
    expect(props.tree.onToggleProject).toHaveBeenCalledWith("project-1", true);

    const projectRow = container.querySelector<HTMLElement>(".tree-l2")!;
    fireEvent.contextMenu(projectRow);
    expect(props.tree.onProjectMenu).toHaveBeenCalledWith(
      expect.anything(),
      "project-1",
    );
    await user.click(within(projectRow).getByRole("button", { name: "More" }));
    expect(props.tree.onProjectMenu).toHaveBeenCalledTimes(2);
    await user.click(
      within(projectRow).getByRole("button", { name: "New conversation" }),
    );
    expect(props.tree.onNewSessionInProject).toHaveBeenCalledWith("project-1");

    const openUntrusted = createProps({
      tree: {
        ...base.tree,
        activeSessionId: null,
        projects: [{ ...project, open: true }],
      },
    });
    rerender(<SidebarNavigator {...openUntrusted} />);
    await user.click(screen.getByRole("button", { name: "Trust project" }));
    expect(openUntrusted.tree.onTrustProject).toHaveBeenCalledWith("project-1");

    const taskless = createProps({
      tree: {
        ...base.tree,
        activeSessionId: null,
        projects: [
          {
            ...project,
            trusted: true,
            pinned: false,
            open: true,
          },
        ],
      },
    });
    rerender(<SidebarNavigator {...taskless} />);
    expect(screen.getByText("No tasks")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Sunsetz" }));
    expect(taskless.tree.onSelectProject).toHaveBeenCalledWith("project-1");
  });

  it("renders scheduled, archived, working, pending, and orphan tasks", async () => {
    const user = userEvent.setup();
    const base = createProps();
    const scheduledArchived = {
      id: "session-special",
      title: "",
      updatedAt: "2026-07-27T08:30:00Z",
      archived: true,
      scheduled: true,
    };
    const props = createProps({
      tree: {
        ...base.tree,
        activeSessionId: "session-special",
        projects: [
          {
            ...base.tree.projects[0]!,
            sessions: [scheduledArchived],
          },
        ],
      },
    });
    const { container, rerender } = render(<SidebarNavigator {...props} />);

    const task = screen.getByRole("button", { name: /Untitled/ });
    expect(task.getAttribute("aria-current")).toBe("page");
    expect(screen.getByLabelText("Scheduled task")).toBeTruthy();
    await user.click(task);
    await user.click(screen.getByRole("button", { name: "Unarchive" }));
    const taskRow = container.querySelector<HTMLElement>(".tree-l3")!;
    fireEvent.contextMenu(taskRow);
    await user.click(within(taskRow).getByRole("button", { name: "More" }));
    expect(props.tree.onOpenSession).toHaveBeenCalledWith(
      "session-special",
      "project-1",
    );
    expect(props.tree.onArchiveSession).toHaveBeenCalledWith(
      "session-special",
      false,
    );
    expect(props.tree.onSessionMenu).toHaveBeenCalledTimes(2);

    const workingProps = createProps({
      tree: {
        ...base.tree,
        activeSessionId: null,
        busySessionIds: new Set(["session-1"]),
      },
    });
    rerender(<SidebarNavigator {...workingProps} />);
    expect(screen.getByLabelText("Working")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Archive" })).toBeNull();

    const pendingProps = createProps({
      tree: {
        ...base.tree,
        activeSessionId: null,
        busySessionIds: new Set(["session-1"]),
        pendingAskSessionIds: new Set(["session-1"]),
      },
    });
    rerender(<SidebarNavigator {...pendingProps} />);
    expect(screen.getByLabelText("Answer needed")).toBeTruthy();
    expect(screen.queryByLabelText("Working")).toBeNull();

    const orphanProps = createProps({
      tree: {
        ...base.tree,
        activeProjectId: null,
        activeSessionId: "orphan-1",
        projects: [],
        orphanSessions: [
          {
            id: "orphan-1",
            title: "Loose task",
            updatedAt: "2026-07-27T08:30:00Z",
            archived: false,
            scheduled: false,
          },
        ],
      },
    });
    rerender(<SidebarNavigator {...orphanProps} />);
    const orphan = screen.getByRole("button", { name: "Loose task" });
    await user.click(orphan);
    expect(orphanProps.tree.onOpenSession).toHaveBeenCalledWith(
      "orphan-1",
      null,
    );
    await user.click(screen.getByRole("button", { name: "Archive" }));
    expect(orphanProps.tree.onArchiveSession).toHaveBeenCalledWith(
      "orphan-1",
      true,
    );
  });

  it("derives provider and official account footer states from real data", async () => {
    const user = userEvent.setup();
    const base = createProps();
    const provider: CustomProvider = {
      id: "relay-id",
      name: "  Coral Relay  ",
      model: "coral-1",
      baseUrl: "https://example.invalid/v1",
      hasApiKey: true,
      apiBackend: "responses",
      isDefault: true,
    };
    const providerProps = createProps({
      account: {
        ...base.account,
        activeProvider: provider,
      },
    });
    const { rerender } = render(<SidebarNavigator {...providerProps} />);

    expect(screen.getByText("Coral Relay")).toBeTruthy();
    expect(screen.getByText("C")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Account" }));
    expect(providerProps.account.onToggle).toHaveBeenCalledWith(true);

    const fallbackProviderProps = createProps({
      account: {
        ...base.account,
        activeProvider: { ...provider, name: " ", id: " provider-id " },
      },
    });
    rerender(<SidebarNavigator {...fallbackProviderProps} />);
    expect(screen.getByText("provider-id")).toBeTruthy();
    expect(screen.getByText("P")).toBeTruthy();

    const officialProps = createProps({
      account: {
        ...base.account,
        account: createAccountStatus(),
      },
    });
    rerender(<SidebarNavigator {...officialProps} />);
    expect(screen.getByText("Ada Lovelace")).toBeTruthy();
    expect(screen.getByText("AL")).toBeTruthy();
    expect(screen.getByText("58%")).toBeTruthy();

    const customRouteProps = createProps({
      account: {
        ...base.account,
        account: createAccountStatus(),
        customRouteActive: true,
      },
    });
    rerender(<SidebarNavigator {...customRouteProps} />);
    expect(screen.queryByText("58%")).toBeNull();
  });

  it("shows only real account, usage, appearance, and auth actions", async () => {
    const user = userEvent.setup();
    const base = createProps();
    const props = createProps({
      account: {
        ...base.account,
        open: true,
        account: createAccountStatus(),
      },
    });
    render(<SidebarNavigator {...props} />);

    expect(await screen.findByRole("menu")).toBeTruthy();
    expect(screen.getByText("Usage")).toBeTruthy();
    expect(screen.getByText(/58% Remaining/)).toBeTruthy();
    expect(screen.queryByText(/pet/i)).toBeNull();
    await user.click(screen.getByRole("menuitem", { name: /Usage/ }));
    expect(props.account.onClose).toHaveBeenCalledTimes(1);
    expect(props.account.onAccountSettings).toHaveBeenCalledTimes(1);
  });

  it("keeps unavailable usage and provider account states truthful", async () => {
    const base = createProps();
    const noUsage = createAccountStatus({
      billing: {
        ...createAccountStatus().billing,
        remainingPercent: null,
        creditUsagePercent: null,
        resetsAt: null,
      },
    });
    const noUsageProps = createProps({
      account: {
        ...base.account,
        open: true,
        account: noUsage,
      },
    });
    const { rerender } = render(<SidebarNavigator {...noUsageProps} />);

    expect(await screen.findByRole("menuitem", { name: /Usage/ })).toBeTruthy();
    expect(screen.getByText("—")).toBeTruthy();

    const providerProps = createProps({
      account: {
        ...base.account,
        open: true,
        activeProvider: {
          id: "relay",
          name: "Coral Relay",
          model: "coral-1",
          baseUrl: "https://example.invalid/v1",
          hasApiKey: true,
          apiBackend: "responses",
          isDefault: true,
        },
      },
    });
    rerender(<SidebarNavigator {...providerProps} />);
    expect(screen.getAllByText("Coral Relay")).toHaveLength(2);
    expect(screen.getByText("Provider / coral-1")).toBeTruthy();
    expect(screen.queryByText("Usage")).toBeNull();

    const signedOutProps = createProps({
      account: {
        ...base.account,
        open: true,
        theme: "light",
      },
    });
    rerender(<SidebarNavigator {...signedOutProps} />);
    expect(screen.getByText("Signed out")).toBeTruthy();
    expect(screen.getByRole("menuitem", { name: "Log in" })).toBeTruthy();
    expect(screen.getByText("Dark")).toBeTruthy();
  });

  it("delays task previews, caches results, and closes on leave", async () => {
    vi.useFakeTimers();
    const props = createProps();
    render(<SidebarNavigator {...props} />);
    const row = screen
      .getByRole("button", { name: "Refactor sidebar" })
      .closest<HTMLElement>(".tree-l3")!;

    fireEvent.mouseEnter(row);
    await act(async () => {
      vi.advanceTimersByTime(449);
    });
    expect(props.tree.loadSessionPreview).not.toHaveBeenCalled();

    await act(async () => {
      vi.advanceTimersByTime(1);
      await Promise.resolve();
    });
    expect(props.tree.loadSessionPreview).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("tooltip").textContent).toContain(
      "bounded, read-only preview",
    );

    fireEvent.mouseLeave(row);
    expect(screen.queryByRole("tooltip")).toBeNull();
    fireEvent.mouseEnter(row);
    await act(async () => {
      vi.advanceTimersByTime(450);
    });
    expect(props.tree.loadSessionPreview).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("tooltip")).toBeTruthy();

    fireEvent.click(within(row).getByRole("button", { name: "More" }));
    expect(screen.queryByRole("tooltip")).toBeNull();
    expect(props.tree.onSessionMenu).toHaveBeenCalledTimes(1);
  });

  it("places the project preview outside the project row action slot", async () => {
    vi.useFakeTimers();
    const props = createProps();
    render(<SidebarNavigator {...props} />);
    const row = projectRowFrom();
    row.getBoundingClientRect = () =>
      ({
        x: 0,
        y: 80,
        left: 0,
        right: 248,
        top: 80,
        bottom: 108,
        width: 248,
        height: 28,
        toJSON() {
          return {};
        },
      }) as DOMRect;
    fireEvent.mouseEnter(row);
    await advanceTimers(450);
    const preview = projectPreview();
    expect(preview).not.toBeNull();
    expect(Number.parseFloat(preview!.style.left)).toBeGreaterThanOrEqual(248);
    const actions = row.querySelector(".tree-l2__actions");
    expect(actions).toBeTruthy();
    fireEvent.click(within(row).getByRole("button", { name: "More" }));
    expect(props.tree.onProjectMenu).toHaveBeenCalled();
  });

  it("waits 450ms, renders project facts first, then loads Git summary", async () => {
    vi.useFakeTimers();
    const pending = deferred<ProjectGitSummaryV1 | null>();
    const previewAtLoad: Array<string | null> = [];
    const base = createProps();
    const loadProjectGitSummary = vi.fn(
      (_projectId: string, _projectPath: string) => {
        previewAtLoad.push(projectPreview()?.textContent ?? null);
        return pending.promise;
      },
    );
    const props = createProps({
      tree: { ...base.tree, loadProjectGitSummary },
    });
    render(<SidebarNavigator {...props} />);
    const project = screen.getByRole("button", { name: "Sunsetz" });

    fireEvent.mouseEnter(project);
    await advanceTimers(449);
    expect(projectPreview()).toBeNull();
    expect(loadProjectGitSummary).not.toHaveBeenCalled();

    fireEvent.mouseLeave(projectRowFrom());
    await advanceTimers(1_000);
    expect(projectPreview()).toBeNull();
    expect(loadProjectGitSummary).not.toHaveBeenCalled();

    fireEvent.mouseEnter(project);
    await advanceTimers(450);
    expect(projectPreview()?.textContent).toContain("1 tasks");
    expect(projectPreview()?.textContent).toContain("/tmp/sunsetz");
    expect(loadProjectGitSummary).toHaveBeenCalledTimes(1);
    expect(loadProjectGitSummary).toHaveBeenCalledWith(
      "project-1",
      "/tmp/sunsetz",
    );
    expect(previewAtLoad[0]).toContain("/tmp/sunsetz");
    expect(projectPreview()?.querySelector(".sidebar-preview__git")).toBeNull();

    await act(async () => {
      pending.resolve(createProjectGitSummary({ countsCapped: true }));
      await pending.promise;
    });
    const git = projectPreview()?.querySelector(".sidebar-preview__git");
    expect(git?.textContent).toContain("Git · main");
    expect(git?.textContent).toContain("Ahead 2");
    expect(git?.textContent).toContain("Behind 1");
    expect(git?.textContent).toContain("4 changed");
    expect(git?.textContent).toContain("1 conflicts");
    expect(
      projectPreview()?.querySelector("[aria-label='Counts capped']")
        ?.textContent,
    ).toBe("+");
  });

  it(
    "coalesces project requests and caches their result for five seconds",
    async () => {
      vi.useFakeTimers();
      const pending = deferred<ProjectGitSummaryV1 | null>();
      const base = createProps();
      const loadProjectGitSummary = vi.fn(() => pending.promise);
      const props = createProps({
        tree: { ...base.tree, loadProjectGitSummary },
      });
      render(<SidebarNavigator {...props} />);
      const project = screen.getByRole("button", { name: "Sunsetz" });

      fireEvent.mouseEnter(project);
      await advanceTimers(450);
      fireEvent.mouseLeave(projectRowFrom());
      await advanceTimers(160);
      fireEvent.mouseEnter(project);
      await advanceTimers(450);
      expect(loadProjectGitSummary).toHaveBeenCalledTimes(1);

      await act(async () => {
        pending.resolve(createProjectGitSummary());
        await pending.promise;
      });
      expect(projectPreview()?.textContent).toContain("Git · main");

      fireEvent.mouseLeave(projectRowFrom());
      await advanceTimers(160);
      fireEvent.mouseEnter(project);
      await advanceTimers(450);
      expect(loadProjectGitSummary).toHaveBeenCalledTimes(1);
      expect(projectPreview()?.textContent).toContain("Git · main");

      fireEvent.mouseLeave(projectRowFrom());
      await advanceTimers(5_001);
      fireEvent.mouseEnter(project);
      await advanceTimers(450);
      expect(loadProjectGitSummary).toHaveBeenCalledTimes(2);
    },
  );

  it.each([
    [
      "an unavailable summary",
      createProjectGitSummary({
        available: false,
        source: "unavailable",
        unavailableReason: "git_failed",
      }),
    ],
    [
      "a non-repository summary",
      createProjectGitSummary({
        isRepo: false,
        branch: null,
        ahead: null,
        behind: null,
        dirty: 0,
        conflicts: 0,
        head: null,
        source: "filesystem_marker",
      }),
    ],
  ])("keeps %s silent and cached", async (_label, summary) => {
    const base = createProps();
    const loadProjectGitSummary = vi.fn(async () => summary);
    const props = createProps({
      tree: { ...base.tree, loadProjectGitSummary },
    });
    render(<SidebarNavigator {...props} />);
    const project = screen.getByRole("button", { name: "Sunsetz" });

    await act(async () => {
      fireEvent.focus(project);
      await Promise.resolve();
    });
    expect(projectPreview()?.textContent).toContain("/tmp/sunsetz");
    expect(projectPreview()?.querySelector(".sidebar-preview__git")).toBeNull();
    expect(projectPreview()?.textContent).not.toContain("git_failed");

    await act(async () => {
      fireEvent.blur(project);
      fireEvent.focus(project);
      await Promise.resolve();
    });
    expect(loadProjectGitSummary).toHaveBeenCalledTimes(1);
    expect(projectPreview()?.querySelector(".sidebar-preview__git")).toBeNull();
  });

  it("does not cache a rejected project summary request", async () => {
    const base = createProps();
    const loadProjectGitSummary = vi
      .fn<() => Promise<ProjectGitSummaryV1 | null>>()
      .mockRejectedValueOnce(new Error("Host unavailable"))
      .mockResolvedValueOnce(createProjectGitSummary());
    const props = createProps({
      tree: { ...base.tree, loadProjectGitSummary },
    });
    render(<SidebarNavigator {...props} />);
    const project = screen.getByRole("button", { name: "Sunsetz" });

    await act(async () => {
      fireEvent.focus(project);
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(loadProjectGitSummary).toHaveBeenCalledTimes(1);
    expect(projectPreview()?.textContent).toContain("/tmp/sunsetz");
    expect(projectPreview()?.querySelector(".sidebar-preview__git")).toBeNull();

    await act(async () => {
      fireEvent.blur(project);
      await new Promise((resolve) => window.setTimeout(resolve, 180));
      fireEvent.focus(project);
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(loadProjectGitSummary).toHaveBeenCalledTimes(2);
    expect(projectPreview()?.textContent).toContain("Git · main");
  });

  it.each([
    "mouse leave",
    "scroll",
    "sidebar collapse",
    "project collapse",
    "project menu",
  ])("discards a late project response after %s", async (cancellation) => {
    const pending = deferred<ProjectGitSummaryV1 | null>();
    const base = createProps();
    const loadProjectGitSummary = vi.fn(() => pending.promise);
    const props = createProps({
      tree: { ...base.tree, loadProjectGitSummary },
    });
    const view = render(<SidebarNavigator {...props} />);
    const project = screen.getByRole("button", { name: "Sunsetz" });

    await act(async () => {
      fireEvent.focus(project);
      await Promise.resolve();
    });
    expect(loadProjectGitSummary).toHaveBeenCalledTimes(1);
    expect(projectPreview()).not.toBeNull();

    if (cancellation === "mouse leave") {
      fireEvent.mouseLeave(projectRowFrom());
      await act(async () => {
        await new Promise((resolve) => window.setTimeout(resolve, 180));
      });
    } else if (cancellation === "scroll") {
      fireEvent.scroll(
        view.container.querySelector(".overlay-scroll__viewport")!,
      );
    } else if (cancellation === "sidebar collapse") {
      view.rerender(<SidebarNavigator {...props} collapsed />);
    } else if (cancellation === "project collapse") {
      fireEvent.click(project);
    } else {
      fireEvent.click(
        within(project.closest<HTMLElement>(".tree-l2")!).getByRole("button", {
          name: "More",
        }),
      );
    }
    expect(projectPreview()).toBeNull();

    await act(async () => {
      pending.resolve(createProjectGitSummary());
      await pending.promise;
    });
    expect(projectPreview()).toBeNull();
  });

  it("does not revive a project preview after unmount", async () => {
    const pending = deferred<ProjectGitSummaryV1 | null>();
    const base = createProps();
    const loadProjectGitSummary = vi.fn(() => pending.promise);
    const props = createProps({
      tree: { ...base.tree, loadProjectGitSummary },
    });
    const view = render(<SidebarNavigator {...props} />);

    await act(async () => {
      fireEvent.focus(screen.getByRole("button", { name: "Sunsetz" }));
      await Promise.resolve();
    });
    expect(loadProjectGitSummary).toHaveBeenCalledTimes(1);
    view.unmount();

    await act(async () => {
      pending.resolve(createProjectGitSummary());
      await pending.promise;
    });
    expect(projectPreview()).toBeNull();
  });

  it("keeps a newer project preview when an older response arrives late", async () => {
    const first = deferred<ProjectGitSummaryV1 | null>();
    const second = deferred<ProjectGitSummaryV1 | null>();
    const base = createProps();
    const projects = [
      base.tree.projects[0]!,
      {
        ...base.tree.projects[0]!,
        id: "project-2",
        name: "Second",
        path: "/tmp/second",
        sessions: [],
      },
    ];
    const loadProjectGitSummary = vi.fn((projectId: string) =>
      projectId === "project-1" ? first.promise : second.promise,
    );
    const props = createProps({
      tree: { ...base.tree, projects, loadProjectGitSummary },
    });
    render(<SidebarNavigator {...props} />);
    const firstProject = screen.getByRole("button", { name: "Sunsetz" });
    const secondProject = screen.getByRole("button", { name: "Second" });

    await act(async () => {
      fireEvent.focus(firstProject);
      await Promise.resolve();
      fireEvent.blur(firstProject);
      fireEvent.focus(secondProject);
      await Promise.resolve();
    });
    expect(loadProjectGitSummary).toHaveBeenCalledTimes(2);

    await act(async () => {
      second.resolve(
        createProjectGitSummary({
          projectId: "project-2",
          branch: "feature/current",
        }),
      );
      await second.promise;
    });
    expect(projectPreview()?.textContent).toContain("Second");
    expect(projectPreview()?.textContent).toContain("Git · feature/current");

    await act(async () => {
      first.resolve(createProjectGitSummary({ branch: "stale/main" }));
      await first.promise;
    });
    expect(projectPreview()?.textContent).toContain("Second");
    expect(projectPreview()?.textContent).toContain("Git · feature/current");
    expect(projectPreview()?.textContent).not.toContain("stale/main");
  });

  it("does not leave a loading card when the Host has no preview", async () => {
    const base = createProps();
    const props = createProps({
      tree: {
        ...base.tree,
        loadSessionPreview: vi.fn(async () => null),
      },
    });
    render(<SidebarNavigator {...props} />);

    await act(async () => {
      fireEvent.focus(
        screen.getByRole("button", { name: "Refactor sidebar" }),
      );
      await Promise.resolve();
    });
    expect(screen.queryByRole("tooltip")).toBeNull();
  });
});
