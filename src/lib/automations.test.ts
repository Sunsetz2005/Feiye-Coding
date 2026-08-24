// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  computeNextRunAt,
  isDue,
  formatScheduleSummary,
  loadAutomationsLocal,
  parseScheduledUserContent,
} from "./automations";

const localValues = new Map<string, string>();
const localStorageMock: Storage = {
  get length() {
    return localValues.size;
  },
  clear: () => localValues.clear(),
  getItem: (key) => localValues.get(key) ?? null,
  key: (index) => [...localValues.keys()][index] ?? null,
  removeItem: (key) => localValues.delete(key),
  setItem: (key, value) => localValues.set(key, String(value)),
};

beforeEach(() => {
  localValues.clear();
  vi.stubGlobal("localStorage", localStorageMock);
});

afterEach(() => vi.unstubAllGlobals());

describe("automations schedule helpers", () => {
  it("computes next daily run after now", () => {
    const from = new Date("2026-07-22T08:00:00");
    const next = computeNextRunAt(
      { frequency: "daily", time: "09:00", weekdays: [], enabled: true },
      from,
    );
    expect(next).toBeTruthy();
    const d = new Date(next!);
    expect(d.getHours()).toBe(9);
    expect(d.getMinutes()).toBe(0);
    expect(d.getTime()).toBeGreaterThan(from.getTime());
  });

  it("returns null when disabled", () => {
    expect(
      computeNextRunAt(
        { frequency: "daily", time: "09:00", weekdays: [], enabled: false },
        new Date(),
      ),
    ).toBeNull();
  });

  it("detects due when nextRunAt is past", () => {
    const past = new Date(Date.now() - 5_000).toISOString();
    expect(
      isDue({
        id: "1",
        title: "t",
        prompt: "p",
        enabled: true,
        projectId: null,
        modelId: null,
        effort: null,
        frequency: "daily",
        time: "09:00",
        weekdays: [],
        notify: "all",
        missedRunPolicy: "run_once",
        createdAt: past,
        updatedAt: past,
        nextRunAt: past,
      }),
    ).toBe(true);
  });

  it("formats schedule summary", () => {
    const s = formatScheduleSummary(
      { frequency: "daily", time: "07:00", weekdays: [] },
      {
        daily: "Daily",
        weekly: "Weekly",
        weekdays: "Weekdays",
        once: "Once",
        at: "at",
      },
    );
    expect(s).toContain("Daily");
    expect(s).toContain("07:00");
  });

  it("parses scheduled user header into title + body", () => {
    const raw =
      "[Scheduled: 项目动态快检（对话 JSON 承接）]\n\n作为已安排任务：打开项目面板";
    const p = parseScheduledUserContent(raw);
    expect(p?.title).toBe("项目动态快检（对话 JSON 承接）");
    expect(p?.body).toContain("打开项目面板");
    expect(parseScheduledUserContent("普通消息")).toBeNull();
  });

  it("defaults legacy local records to run_once and preserves skip", () => {
    const base = {
      title: "Legacy",
      prompt: "Run",
      enabled: true,
      projectId: null,
      modelId: null,
      effort: null,
      frequency: "daily",
      time: "09:00",
      weekdays: [],
      notify: "all",
      createdAt: "2026-08-24T00:00:00.000Z",
      updatedAt: "2026-08-24T00:00:00.000Z",
    };
    localStorage.setItem(
      "sunsetz.automations",
      JSON.stringify([
        { ...base, id: "legacy" },
        { ...base, id: "skip", missedRunPolicy: "skip" },
      ]),
    );

    const rows = loadAutomationsLocal();
    expect(rows.find((row) => row.id === "legacy")?.missedRunPolicy).toBe(
      "run_once",
    );
    expect(rows.find((row) => row.id === "skip")?.missedRunPolicy).toBe(
      "skip",
    );
  });
});
