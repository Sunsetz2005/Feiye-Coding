// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  rows: [] as Array<Record<string, unknown>>,
  list: vi.fn(),
  create: vi.fn(),
  update: vi.fn(),
  setEnabled: vi.fn(),
  remove: vi.fn(),
}));

vi.mock("@/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api")>();
  return {
    ...actual,
    automationsList: mocks.list,
    automationCreate: mocks.create,
    automationUpdate: mocks.update,
    automationSetEnabled: mocks.setEnabled,
    automationDelete: mocks.remove,
  };
});

vi.mock("@/components/Select", () => ({
  Select: ({
    value,
    options,
    onChange,
    "aria-label": ariaLabel,
  }: {
    value: string;
    options: Array<{ value: string; label: string; disabled?: boolean }>;
    onChange: (value: string) => void;
    "aria-label"?: string;
  }) => (
    <select
      aria-label={ariaLabel}
      value={value}
      onChange={(event) => onChange(event.target.value)}
    >
      {options.map((option) => (
        <option
          key={option.value}
          value={option.value}
          disabled={option.disabled}
        >
          {option.label}
        </option>
      ))}
    </select>
  ),
}));

import { AutomationsPage } from "./AutomationsPage";

const t = (key: string) => key;

function automation(overrides: Record<string, unknown> = {}) {
  return {
    id: "automation-1",
    title: "Existing automation",
    prompt: "Run the existing workflow",
    enabled: true,
    projectId: null,
    modelId: null,
    effort: null,
    frequency: "daily",
    time: "09:00",
    weekdays: [],
    notify: "all",
    missedRunPolicy: "skip",
    createdAt: "2026-08-24T00:00:00.000Z",
    updatedAt: "2026-08-24T00:00:00.000Z",
    lastRunAt: null,
    nextRunAt: "2026-08-25T01:00:00.000Z",
    ...overrides,
  };
}

beforeEach(() => {
  mocks.rows = [];
  mocks.list.mockImplementation(async () => mocks.rows);
  mocks.create.mockResolvedValue(automation({ id: "created" }));
  mocks.update.mockImplementation(async (_id, input) =>
    automation(input as Record<string, unknown>),
  );
  mocks.setEnabled.mockResolvedValue(automation());
  mocks.remove.mockResolvedValue(undefined);
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

function renderPage() {
  return render(
    <AutomationsPage t={t} projects={[]} onAiCreate={vi.fn()} />,
  );
}

describe("AutomationsPage missed-run policy", () => {
  it("defaults a new automation to run_once and sends the selected skip policy", async () => {
    renderPage();
    await screen.findByText("automations.emptyTitle");

    fireEvent.click(
      screen.getByRole("button", { name: "automations.createManual" }),
    );
    const policy = screen.getByLabelText(
      "automations.field.missedRun",
    ) as HTMLSelectElement;
    expect(policy.value).toBe("run_once");

    fireEvent.change(
      screen.getByPlaceholderText("automations.field.titlePh"),
      { target: { value: "New automation" } },
    );
    fireEvent.change(
      screen.getByPlaceholderText("automations.field.promptPh"),
      { target: { value: "Run the new workflow" } },
    );
    fireEvent.change(policy, { target: { value: "skip" } });

    const panel = screen.getByLabelText("automations.formTitle");
    fireEvent.click(
      within(panel).getByRole("button", { name: "automations.create" }),
    );

    await waitFor(() => expect(mocks.create).toHaveBeenCalledTimes(1));
    expect(mocks.create.mock.calls[0]?.[0]).toMatchObject({
      title: "New automation",
      prompt: "Run the new workflow",
      missedRunPolicy: "skip",
    });
  });

  it("loads and preserves skip while editing an existing automation", async () => {
    mocks.rows = [automation()];
    renderPage();

    fireEvent.click(
      await screen.findByRole("button", { name: /Existing automation/ }),
    );
    const policy = screen.getByLabelText(
      "automations.field.missedRun",
    ) as HTMLSelectElement;
    expect(policy.value).toBe("skip");

    const panel = screen.getByLabelText("automations.formTitle");
    fireEvent.click(
      within(panel).getByRole("button", { name: "automations.save" }),
    );

    await waitFor(() => expect(mocks.update).toHaveBeenCalledTimes(1));
    expect(mocks.update).toHaveBeenCalledWith(
      "automation-1",
      expect.objectContaining({ missedRunPolicy: "skip" }),
    );
  });

  it("preserves skip when resuming requires a next-run update", async () => {
    const paused = automation({ enabled: false, nextRunAt: null });
    mocks.rows = [paused];
    mocks.setEnabled.mockResolvedValue(
      automation({ enabled: true, nextRunAt: null }),
    );
    renderPage();

    await screen.findByText("Existing automation");
    const menuTrigger = document.querySelector<HTMLButtonElement>(
      '[data-auto-row-trigger="automation-1"]',
    );
    expect(menuTrigger).not.toBeNull();
    fireEvent.click(menuTrigger!);
    fireEvent.click(
      screen.getByRole("menuitem", { name: "automations.resume" }),
    );

    await waitFor(() =>
      expect(mocks.setEnabled).toHaveBeenCalledWith("automation-1", true),
    );
    await waitFor(() => expect(mocks.update).toHaveBeenCalledTimes(1));
    expect(mocks.update).toHaveBeenCalledWith(
      "automation-1",
      expect.objectContaining({
        enabled: true,
        missedRunPolicy: "skip",
      }),
    );
  });
});
