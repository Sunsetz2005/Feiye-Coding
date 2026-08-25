// @vitest-environment jsdom

import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createT } from "@/i18n";

const apiMock = vi.hoisted(() => ({
  pickCliBinary: vi.fn(async () => "/opt/sunsetz/bin/runtime"),
  probeCli: vi.fn(async (path?: string) => ({
    found: Boolean(path),
    path: path ?? null,
    version: path ? "2.0.0" : null,
    source: path ? "manual" : "none",
    cliAuthPresent: false,
  })),
  settingsPatchV1: vi.fn(async () => undefined),
}));

vi.mock("@/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api")>();
  return {
    ...actual,
    isTauri: () => false,
    cliInstallCommands: vi.fn(async () => null),
    pickCliBinary: apiMock.pickCliBinary,
    probeCli: apiMock.probeCli,
    settingsPatchV1: apiMock.settingsPatchV1,
  };
});

import { SetupWizard, type SetupCliInfo } from "./SetupWizard";

const missingCli: SetupCliInfo = {
  found: false,
  path: null,
  version: null,
  source: "none",
  cliAuthPresent: false,
};

const installedCli: SetupCliInfo = {
  found: true,
  path: "/opt/sunsetz/bin/runtime",
  version: "2.0.0",
  source: "manual",
  cliAuthPresent: false,
};

afterEach(() => {
  cleanup();
  apiMock.pickCliBinary.mockClear();
  apiMock.probeCli.mockClear();
  apiMock.settingsPatchV1.mockClear();
});

describe("SetupWizard settings patch migration", () => {
  it("persists a manually selected Runtime path as a field-level patch", async () => {
    const user = userEvent.setup();
    render(
      <SetupWizard
        tr={createT("en")}
        platform="mac"
        useCustomWindowChrome={false}
        initialCli={missingCli}
        onComplete={vi.fn()}
        onAccountLoginOauth={vi.fn(async () => false)}
      />,
    );

    await user.click(
      await screen.findByRole("button", { name: /Legacy Grok CLI/ }),
    );
    await user.click(
      await screen.findByRole("button", { name: "Choose local binary…" }),
    );

    await waitFor(() => {
      expect(apiMock.settingsPatchV1).toHaveBeenCalledWith({
        manualCliPath: "/opt/sunsetz/bin/runtime",
      });
    });
    expect(apiMock.probeCli).toHaveBeenCalledWith(
      "/opt/sunsetz/bin/runtime",
    );
  });

  it("commits completion flags together after account setup is skipped", async () => {
    const user = userEvent.setup();
    const onComplete = vi.fn();
    render(
      <SetupWizard
        tr={createT("en")}
        platform="mac"
        useCustomWindowChrome={false}
        initialCli={installedCli}
        onComplete={onComplete}
        onAccountLoginOauth={vi.fn(async () => false)}
      />,
    );

    await user.click(screen.getByRole("button", { name: "Skip for now" }));
    await user.click(
      await screen.findByRole("button", { name: "Enter Sunsetz" }),
    );

    await waitFor(() => {
      expect(apiMock.settingsPatchV1).toHaveBeenCalledWith({
        setupWizardCompleted: true,
        authSetupDeferred: true,
        onboardingDone: true,
        setupSkipped: true,
        runtimeBackend: "sunsetz",
      });
      expect(onComplete).toHaveBeenCalledWith(installedCli);
    });
  });

  it("reaches home without Grok CLI installed", async () => {
    const user = userEvent.setup();
    const onComplete = vi.fn();
    render(
      <SetupWizard
        tr={createT("en")}
        platform="mac"
        useCustomWindowChrome={false}
        initialCli={missingCli}
        onComplete={onComplete}
        onAccountLoginOauth={vi.fn(async () => false)}
      />,
    );

    await user.click(screen.getByRole("button", { name: "Skip for now" }));
    await user.click(
      await screen.findByRole("button", { name: "Enter Sunsetz" }),
    );

    await waitFor(() => {
      expect(apiMock.settingsPatchV1).toHaveBeenCalledWith({
        setupWizardCompleted: true,
        authSetupDeferred: true,
        onboardingDone: true,
        setupSkipped: true,
        runtimeBackend: "sunsetz",
      });
      expect(onComplete).toHaveBeenCalledWith(missingCli);
    });
  });
});
