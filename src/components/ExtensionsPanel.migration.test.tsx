// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  catalog: vi.fn(),
  hooks: vi.fn(),
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
    plugins: [plugin(query?.trim() ? "needle-plugin" : "base-plugin")],
  }));
  mocks.hooks.mockResolvedValue([
    { version: 1, pluginName: "one", source: "runtime_inspect" },
    { version: 1, pluginName: "two", source: "runtime_inspect" },
  ]);
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
});
