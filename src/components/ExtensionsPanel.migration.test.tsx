// @vitest-environment jsdom

import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  catalog: vi.fn(),
  hooks: vi.fn(),
  uninstall: vi.fn(),
  skillUsesList: vi.fn(),
  skillFeedback: vi.fn(),
  skillProposal: vi.fn(),
  skillRank: vi.fn(),
}));

vi.mock("@/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api")>();
  return {
    ...actual,
    isTauri: () => true,
    skillsList: vi.fn(async () => ({ skills: [] })),
    inspectMcp: vi.fn(async () => ({ servers: [] })),
    runtimePluginsCatalogV1: mocks.catalog,
    runtimeHooksInventoryV1: mocks.hooks,
    pluginUninstall: mocks.uninstall,
    skillUsesListV1: mocks.skillUsesList,
    skillUseFeedbackV1: mocks.skillFeedback,
    skillImprovementProposalV1: mocks.skillProposal,
    skillMetadataRankV1: mocks.skillRank,
    providersList: vi.fn(async () => null),
  };
});

const plugin = (name: string) => ({
  name,
  status: "installed",
  enabled: true,
  version: "1.0.0",
  provides: { skills: 0, agents: 0, hooks: true, mcpServers: 0 },
});

beforeEach(() => {
  mocks.catalog.mockImplementation(async (query?: string | null) => ({
    version: 1,
    source: "runtime_cli",
    installActionAvailable: false,
    uninstallActionAvailable: false,
    actionUnavailableReason:
      "runtime_plugin_mutations_are_not_machine_verifiable",
    plugins: [plugin(query?.trim() ? "needle-plugin" : "base-plugin")],
  }));
  mocks.hooks.mockResolvedValue([
    { version: 1, pluginName: "one", source: "runtime_inspect" },
    { version: 1, pluginName: "two", source: "runtime_inspect" },
  ]);
  mocks.skillUsesList.mockResolvedValue([]);
  mocks.skillRank.mockResolvedValue({
    version: 1,
    disposition: "suggestion_only",
    requiresExplicitAcceptance: true,
    items: [],
  });
  mocks.skillProposal.mockResolvedValue(null);
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("ExtensionsPanel Runtime catalog bridge", () => {
  it("shows the read-only catalog, hooks inventory, and debounced Runtime search", async () => {
    const { ExtensionsPanel } = await import("./ExtensionsPanel");
    render(<ExtensionsPanel locale="en" />);

    expect(await screen.findByText("base-plugin")).toBeTruthy();
    expect(screen.getByText(/Hooks providers: 2/)).toBeTruthy();
    expect(
      screen.getByRole("button", { name: "Uninstall" }).hasAttribute("disabled"),
    ).toBe(true);
    expect(
      screen.getByText(/Install and uninstall stay in the Runtime CLI/),
    ).toBeTruthy();

    fireEvent.change(screen.getByLabelText("Search Runtime plugins…"), {
      target: { value: "needle" },
    });

    await waitFor(
      () => expect(mocks.catalog).toHaveBeenCalledWith("needle"),
      { timeout: 1_500 },
    );
    expect(await screen.findByText("needle-plugin")).toBeTruthy();
    expect(screen.queryByText("base-plugin")).toBeNull();
  });

  it("keeps the panel usable when catalog and hooks inspection fail", async () => {
    mocks.catalog.mockRejectedValue(new Error("catalog unavailable"));
    mocks.hooks.mockRejectedValue(new Error("hooks unavailable"));
    const { ExtensionsPanel } = await import("./ExtensionsPanel");
    render(<ExtensionsPanel locale="en" />);

    expect((await screen.findByRole("alert")).textContent).toContain(
      "catalog unavailable",
    );
    expect(screen.getByText(/No plugins installed/)).toBeTruthy();
  });

  it("fails closed when an older Host omits uninstall availability", async () => {
    mocks.catalog.mockResolvedValue({
      version: 1,
      source: "runtime_cli",
      installActionAvailable: false,
      plugins: [plugin("legacy-host-plugin")],
    });
    const { ExtensionsPanel } = await import("./ExtensionsPanel");
    render(<ExtensionsPanel locale="en" />);

    expect(await screen.findByText("legacy-host-plugin")).toBeTruthy();
    const uninstall = screen.getByRole("button", { name: "Uninstall" });
    expect(uninstall.hasAttribute("disabled")).toBe(true);
    fireEvent.click(uninstall);
    expect(mocks.uninstall).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("passes the active project and session into the Skill learning review", async () => {
    const { ExtensionsPanel } = await import("./ExtensionsPanel");
    render(
      <ExtensionsPanel
        locale="en"
        projectPath="/trusted/project"
        activeSessionId="session-review"
      />,
    );

    const panel = screen.getByTestId("skill-learning-panel");
    expect(within(panel).getByText("Metadata only")).toBeTruthy();
    expect(
      within(panel).getByText(/cannot write a Skill/),
    ).toBeTruthy();
    await waitFor(() =>
      expect(mocks.skillUsesList).toHaveBeenCalledWith("session-review"),
    );

    fireEvent.change(within(panel).getByLabelText("Skill metadata query"), {
      target: { value: "release notes" },
    });
    fireEvent.click(
      within(panel).getByRole("button", { name: "Find suggestions" }),
    );
    await waitFor(() =>
      expect(mocks.skillRank).toHaveBeenCalledWith(
        "release notes",
        "/trusted/project",
        8,
      ),
    );
  });
});
