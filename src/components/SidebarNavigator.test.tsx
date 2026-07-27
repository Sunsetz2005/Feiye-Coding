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
      busySessionId: null,
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
      onToggleProjects: vi.fn(),
      onAddProject: vi.fn(),
      onToggleProject: vi.fn(),
      onSelectProject: vi.fn(),
      onTrustProject: vi.fn(),
      onProjectMenu: vi.fn(),
      onToggleHistory: vi.fn(),
      onOpenSession: vi.fn(),
      onArchiveSession: vi.fn(),
      onSessionMenu: vi.fn(),
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

    const projectDisclosure = screen.getByRole("button", {
      name: "Collapse project",
    });
    expect(projectDisclosure.getAttribute("aria-expanded")).toBe("true");
    expect(projectDisclosure.getAttribute("aria-controls")).toBeTruthy();
    await user.click(projectDisclosure);
    expect(props.tree.onToggleProject).toHaveBeenCalledWith("project-1", false);
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
    expect(projectSelect.hasAttribute("disabled")).toBe(true);
    expect(screen.getByText("Untrusted")).toBeTruthy();
    expect(
      screen.getByRole("button", { name: "Expand project" }).getAttribute(
        "aria-expanded",
      ),
    ).toBe("false");
    await user.click(screen.getByRole("button", { name: "Expand project" }));
    expect(props.tree.onToggleProject).toHaveBeenCalledWith("project-1", true);

    const projectRow = container.querySelector<HTMLElement>(".tree-l2")!;
    fireEvent.contextMenu(projectRow);
    expect(props.tree.onProjectMenu).toHaveBeenCalledWith(
      expect.anything(),
      "project-1",
    );
    await user.click(within(projectRow).getByRole("button", { name: "More" }));
    expect(props.tree.onProjectMenu).toHaveBeenCalledTimes(2);

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
        busySessionId: "session-1",
      },
    });
    rerender(<SidebarNavigator {...workingProps} />);
    expect(screen.getByLabelText("Working")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Archive" })).toBeNull();

    const pendingProps = createProps({
      tree: {
        ...base.tree,
        activeSessionId: null,
        busySessionId: "session-1",
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

  it("shows project facts on keyboard focus without a Host request", () => {
    const props = createProps();
    render(<SidebarNavigator {...props} />);

    fireEvent.focus(screen.getByRole("button", { name: "Sunsetz" }));
    const preview = screen.getByRole("tooltip");
    expect(preview.textContent).toContain("1 tasks");
    expect(preview.textContent).toContain("/tmp/sunsetz");
    expect(props.tree.loadSessionPreview).not.toHaveBeenCalled();
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
