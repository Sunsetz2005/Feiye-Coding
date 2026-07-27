import { describe, expect, it } from "vitest";
import {
  buildTaskProgressMetrics,
  formatTaskElapsed,
} from "./taskProgressRailModel";

describe("buildTaskProgressMetrics", () => {
  it("shows no progress line without real plan entries", () => {
    expect(
      buildTaskProgressMetrics([], [
        { path: "src/App.tsx", additions: 3, deletions: 1 },
      ]),
    ).toEqual({
      hasSteps: false,
      currentStep: null,
      totalSteps: 0,
      changedFiles: 1,
      additions: 3,
      deletions: 1,
    });
  });

  it("uses the real active step index", () => {
    const metrics = buildTaskProgressMetrics(
      [
        { content: "Inspect", status: "completed" },
        { content: "Implement", status: "in_progress" },
        { content: "Verify", status: "pending" },
      ],
      [],
    );
    expect(metrics.currentStep).toBe(2);
    expect(metrics.totalSteps).toBe(3);
  });

  it("uses the next pending step and the final step when complete", () => {
    expect(
      buildTaskProgressMetrics(
        [
          { content: "A", status: "completed" },
          { content: "B", status: "pending" },
        ],
        [],
      ).currentStep,
    ).toBe(2);
    expect(
      buildTaskProgressMetrics(
        [
          { content: "A", status: "completed" },
          { content: "B", status: "completed" },
        ],
        [],
      ).currentStep,
    ).toBe(2);
  });

  it("deduplicates changed paths and sums only complete real line stats", () => {
    const metrics = buildTaskProgressMetrics(
      [{ content: "Implement", status: "in_progress" }],
      [
        { path: "src/App.tsx", additions: 4, deletions: 1 },
        { path: "src/App.tsx", additions: 99, deletions: 99 },
        { path: "src/ui.tsx", additions: 2, deletions: 3 },
      ],
    );
    expect(metrics.changedFiles).toBe(2);
    expect(metrics.additions).toBe(6);
    expect(metrics.deletions).toBe(4);
  });

  it("omits partial totals instead of extrapolating", () => {
    const metrics = buildTaskProgressMetrics(
      [{ content: "Implement", status: "in_progress" }],
      [
        { path: "src/App.tsx", additions: 4, deletions: 1 },
        { path: "src/ui.tsx", deletions: 2 },
      ],
    );
    expect(metrics.additions).toBeNull();
    expect(metrics.deletions).toBe(3);
  });

  it("accepts authoritative aggregate stats without inventing missing fields", () => {
    const metrics = buildTaskProgressMetrics(
      [{ content: "Implement", status: "in_progress" }],
      [{ path: "src/App.tsx" }],
      { additions: 28 },
    );
    expect(metrics.additions).toBe(28);
    expect(metrics.deletions).toBeNull();
  });
});

describe("formatTaskElapsed", () => {
  it("formats a complete live duration", () => {
    expect(formatTaskElapsed((1 * 3600 + 12 * 60 + 51) * 1000)).toBe(
      "1h 12m 51s",
    );
  });

  it("keeps zero real and omits missing or invalid durations", () => {
    expect(formatTaskElapsed(0)).toBe("0s");
    expect(formatTaskElapsed(null)).toBe("");
    expect(formatTaskElapsed(-1)).toBe("");
  });
});
