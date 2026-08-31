// @vitest-environment jsdom

import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SettingsPage, type SettingsPageProps } from "./SettingsPage";

vi.mock("@/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api")>();
  return {
    ...actual,
    isTauri: () => false,
    runtimeCapabilitiesV1: vi.fn(async () => {
      throw new Error("not in tauri");
    }),
  };
});

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
