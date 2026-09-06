// @vitest-environment jsdom

import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as api from "@/lib/api";
import { SettingsPage, type SettingsPageProps } from "./SettingsPage";

vi.mock("@/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api")>();
  return {
    ...actual,
    isTauri: vi.fn(() => false),
    runtimeCapabilitiesV1: vi.fn(async () => {
      throw new Error("not in tauri");
    }),
  };
});

function hostCaps(kernel: api.RuntimeKernelV1): api.RuntimeCapabilitiesV1 {
  return {
    version: 1,
    runtimeVersion: null,
    protocolVersion: 1,
    clientVersion: "1.0.1",
    platform: "macos",
    sandbox: {
      requested: "off",
      applied: "off",
      verified: true,
      state: "off",
      platform: "macos",
    },
    memory: { state: "unavailable", source: "host" },
    pluginCatalog: { state: "unavailable", source: "runtime_cli" },
    hooksInventory: { state: "unavailable", source: "runtime_cli" },
    mcp: { state: "unavailable", source: "runtime_acp" },
    kernel,
  };
}

function renderRuntime(patch: Partial<SettingsPageProps> = {}) {
  const props: SettingsPageProps = {
    section: "runtime",
    onSection: vi.fn(),
    onBack: vi.fn(),
    labels: {},
    locale: "en",
    onLocale: vi.fn(),
    theme: "light",
    onTheme: vi.fn(),
    sessionDataMode: "independent",
    onSessionDataMode: vi.fn(),
    policy: "ask",
    onPolicy: vi.fn(),
    manualCliPath: "",
    onManualCliPath: vi.fn(),
    onCliBlur: vi.fn(),
    acpServerAddr: "",
    onAcpServerAddr: vi.fn(),
    cliInfo: {
      found: false,
      path: null,
      version: null,
      source: "none",
      cliAuthPresent: false,
    },
    onDoctor: vi.fn(),
    versionFooter: "1.0.0",
    account: null,
    accountLoading: false,
    accountBusy: false,
    onAccountLoginOauth: vi.fn(),
    onAccountLoginDevice: vi.fn(),
    onCancelLogin: vi.fn(),
    onAccountLogout: vi.fn(),
    onAccountRefresh: vi.fn(),
    onAccountManageUsage: vi.fn(),
    onAccountSubscribe: vi.fn(),
    kernelBackend: "sunsetz",
    ...patch,
  };
  return render(<SettingsPage {...props} />);
}

afterEach(() => {
  cleanup();
  vi.mocked(api.isTauri).mockReturnValue(false);
  vi.mocked(api.runtimeCapabilitiesV1).mockRejectedValue(
    new Error("not in tauri"),
  );
});

describe("SettingsPage runtime kernel", () => {
  it("presents the built-in kernel as the product runtime, not Grok CLI", () => {
    renderRuntime();
    expect(
      screen.getByText(/Sunsetz Runtime is built into this app/i),
    ).toBeTruthy();
    expect(screen.getByText("Built-in Sunsetz Runtime")).toBeTruthy();
    expect(screen.queryByText("Developer mock")).toBeNull();
    expect(screen.queryByText("Legacy Grok ACP")).toBeNull();
  });

  it("warns when the developer mock is on", () => {
    renderRuntime({ kernelBackend: "mock_acp" });
    expect(screen.getByText("Developer mock")).toBeTruthy();
    expect(
      screen.getByText(/does not read your project or run tools/i),
    ).toBeTruthy();
  });

  it("lets a legacy ACP session switch back to the built-in kernel", async () => {
    const onKernelBackend = vi.fn();
    const user = userEvent.setup();
    renderRuntime({
      kernelBackend: "grok_agent_stdio",
      onKernelBackend,
    });
    expect(screen.getByText("Legacy Grok ACP")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Use built-in kernel" }));
    expect(onKernelBackend).toHaveBeenCalledWith("sunsetz");
  });

  it("does not pretend Settings can switch the kernel while mock env is covering it", () => {
    renderRuntime({
      kernelBackend: "mock_acp",
      storedKernelBackend: "sunsetz",
      kernelOverrideSource: "sunsetz_acp",
    });
    expect(screen.getByText("Developer mock")).toBeTruthy();
    expect(
      screen.getByText(/SUNSETZ_ACP=mock is covering Settings/i),
    ).toBeTruthy();
    expect(
      screen.getByText(/Saved preference: Built-in Sunsetz Runtime/i),
    ).toBeTruthy();
    expect(
      screen.queryByRole("button", { name: "Use built-in kernel" }),
    ).toBeNull();
    expect(
      screen.queryByRole("button", { name: "Use Grok ACP adapter" }),
    ).toBeNull();
  });

  it("does not pretend Settings can switch the kernel while SUNSETZ_RUNTIME_BACKEND is covering it", () => {
    renderRuntime({
      kernelBackend: "grok_agent_stdio",
      storedKernelBackend: "sunsetz",
      kernelOverrideSource: "sunsetz_runtime_backend",
    });
    expect(screen.getByText("Legacy Grok ACP")).toBeTruthy();
    expect(
      screen.getByText(/SUNSETZ_RUNTIME_BACKEND is covering Settings/i),
    ).toBeTruthy();
    expect(
      screen.getByText(/Saved preference: Built-in Sunsetz Runtime/i),
    ).toBeTruthy();
    expect(
      screen.queryByRole("button", { name: "Use built-in kernel" }),
    ).toBeNull();
  });

  it("shows the saved legacy preference when mock env is covering it", () => {
    renderRuntime({
      kernelBackend: "mock_acp",
      storedKernelBackend: "grok_acp",
      kernelOverrideSource: "sunsetz_acp",
    });
    expect(
      screen.getByText(/Saved preference: Legacy Grok ACP/i),
    ).toBeTruthy();
  });

  it("shows the saved mock preference when a backend env is covering Settings", () => {
    renderRuntime({
      kernelBackend: "sunsetz",
      storedKernelBackend: "mock_acp",
      kernelOverrideSource: "sunsetz_runtime_backend",
    });
    expect(screen.getByText(/Saved preference: Developer mock/i)).toBeTruthy();
    expect(
      screen.queryByRole("button", { name: "Use Grok ACP adapter" }),
    ).toBeNull();
  });

  it("lets Host kernel capabilities cover the Settings props", async () => {
    vi.mocked(api.isTauri).mockReturnValue(true);
    vi.mocked(api.runtimeCapabilitiesV1).mockResolvedValue(
      hostCaps({
        stored: "sunsetz",
        effective: "grok_agent_stdio",
        overrideSource: "sunsetz_runtime_backend",
      }),
    );
    renderRuntime({
      kernelBackend: "sunsetz",
      storedKernelBackend: "sunsetz",
    });
    expect(await screen.findByText("Legacy Grok ACP")).toBeTruthy();
    expect(
      await screen.findByText(/SUNSETZ_RUNTIME_BACKEND is covering Settings/i),
    ).toBeTruthy();
    expect(
      screen.queryByRole("button", { name: "Use built-in kernel" }),
    ).toBeNull();
  });

  it("toggles running scheduled tasks after quit", async () => {
    const onRunScheduledTasksInBackground = vi.fn();
    const user = userEvent.setup();
    renderRuntime({
      runScheduledTasksInBackground: false,
      onRunScheduledTasksInBackground,
    });
    await user.click(
      screen.getByRole("checkbox", { name: "Run scheduled tasks after quit" }),
    );
    expect(onRunScheduledTasksInBackground).toHaveBeenCalledWith(true);
  });
});
