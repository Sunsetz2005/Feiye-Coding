import { describe, expect, it } from "vitest";
import {
  planActionsEnabled,
  planDisplayMarkdown,
  planEntriesToMarkdown,
  planIsAwaitingReview,
} from "./planBody";
import type { PlanEntry } from "./planStatus";

describe("planEntriesToMarkdown", () => {
  it("formats statuses as task markers", () => {
    const entries: PlanEntry[] = [
      { content: "A", status: "completed" },
      { content: "B", status: "in_progress", priority: "high" },
      { content: "C", status: "pending" },
    ];
    const md = planEntriesToMarkdown(entries);
    expect(md).toContain("1. [x] A");
    expect(md).toContain("2. [~] B *(high)*");
    expect(md).toContain("3. [ ] C");
  });
});

describe("planDisplayMarkdown", () => {
  it("prefers body over entries", () => {
    expect(
      planDisplayMarkdown("# Hello", [{ content: "step", status: "pending" }]),
    ).toBe("# Hello");
  });

  it("falls back to entries markdown", () => {
    const md = planDisplayMarkdown("", [
      { content: "Do thing", status: "pending" },
    ]);
    expect(md).toContain("Do thing");
    expect(md).toMatch(/\[ \]/);
  });

  it("returns empty when both missing", () => {
    expect(planDisplayMarkdown("", [])).toBe("");
    expect(planDisplayMarkdown(null, null)).toBe("");
  });
});

describe("plan gate helpers", () => {
  it("never enables resource-surface plan actions", () => {
    expect(planActionsEnabled({ rpcId: 3, liveReview: true })).toBe(false);
    expect(planActionsEnabled({ rpcId: null })).toBe(false);
  });

  it("awaiting review only with a live pending/resolving plan interaction", () => {
    expect(planIsAwaitingReview({ visible: true, liveReview: true })).toBe(true);
    expect(planIsAwaitingReview({ visible: true, liveReview: false })).toBe(false);
    expect(planIsAwaitingReview({ visible: true })).toBe(false);
    expect(planIsAwaitingReview({ visible: false, liveReview: true })).toBe(false);
  });
});
