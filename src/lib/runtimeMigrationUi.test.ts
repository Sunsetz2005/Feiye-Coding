import { describe, expect, it } from "vitest";
import type {
  AutomationClaimV1,
  SessionSearchResultV1,
  SkillCandidateV1,
} from "./api";
import type { InteractionSnapshotV1 } from "./session";
import {
  askUserFromInteraction,
  acpLoopbackSetupCommand,
  automationClaimIntake,
  canStartAutomationClaim,
  interactionMatches,
  isActiveInteraction,
  mergeContentSearchSessions,
  normalizeSandboxProfile,
  pendingInteractionSessionIds,
  permissionFromInteraction,
  planResolutionContext,
  rememberSkillCandidateIds,
  sandboxStateMessageKey,
  skillCandidateAtBootstrap,
  unseenSkillCandidateForSession,
  updateActiveInteractions,
} from "./runtimeMigrationUi";

const base = {
  version: 1 as const,
  interactionId: "i1",
  sessionId: "s1",
  processId: "p1",
  rpcId: 7,
  toolCallId: "t1",
  status: "pending" as const,
  createdAt: "2026-01-01T00:00:00Z",
  updatedAt: "2026-01-01T00:00:00Z",
};

const permission: InteractionSnapshotV1 = {
  ...base,
  payload: {
    kind: "permission",
    toolName: "write",
    title: "Write",
    preview: "preview",
    scopeKey: "write:/project/a",
    options: [],
  },
};

const ask: InteractionSnapshotV1 = {
  ...base,
  interactionId: "i2",
  payload: {
    kind: "ask_user",
    questions: [{ id: "q", question: "Continue?", options: [], multiSelect: false }],
    partialAnswers: { q: "yes" },
  },
};

describe("runtime migration UI reducers", () => {
  it("tracks active interactions and derives background sessions", () => {
    expect(isActiveInteraction(permission)).toBe(true);
    let map = updateActiveInteractions(new Map(), permission);
    map = updateActiveInteractions(map, ask);
    expect([...pendingInteractionSessionIds(map, "s1")]).toEqual([]);
    expect([...pendingInteractionSessionIds(map, "other")]).toEqual(["s1"]);
    map = updateActiveInteractions(map, { ...permission, status: "resolved" });
    expect(map.has("i1")).toBe(false);
    expect(isActiveInteraction({ ...ask, status: "interrupted" })).toBe(false);
  });

  it("projects permission and ask-user payloads without mixing kinds", () => {
    expect(permissionFromInteraction(permission)).toMatchObject({
      interactionId: "i1",
      toolCallId: "t1",
      toolName: "write",
    });
    expect(permissionFromInteraction(ask)).toBe(null);
    expect(askUserFromInteraction(ask)).toMatchObject({
      interactionId: "i2",
      partialAnswers: { q: "yes" },
    });
    expect(askUserFromInteraction(permission)).toBe(null);
    expect(interactionMatches("i1", permission)).toBe(true);
    expect(interactionMatches(null, permission)).toBe(false);
  });

  it("normalizes sandbox values and optional plan routing", () => {
    expect(normalizeSandboxProfile("workspace_write")).toBe("workspace_write");
    expect(normalizeSandboxProfile("read_only")).toBe("read_only");
    expect(normalizeSandboxProfile("future")).toBe("off");
    expect(sandboxStateMessageKey("applied")).toBe("settings.sandboxState.applied");
    expect(sandboxStateMessageKey("future")).toBe("settings.sandboxState.unknown");
    expect(acpLoopbackSetupCommand("[::1]:9000")).toContain("TCP-LISTEN:9000");
    expect(acpLoopbackSetupCommand("bad")).toContain("TCP-LISTEN:8799");
    expect(acpLoopbackSetupCommand("localhost:9000")).toContain("bind=127.0.0.1");
    expect(planResolutionContext("i1", "s1")).toEqual({
      interactionId: "i1",
      sessionId: "s1",
    });
    expect(planResolutionContext(null, undefined)).toEqual({});
  });

  it("selects only unseen pending skill candidates", () => {
    const candidate = {
      id: "c1",
      status: "pending",
      source: { sessionId: "s1" },
    } as SkillCandidateV1;
    const approved = {
      id: "c2",
      status: "approved",
      source: { sessionId: "s1" },
    } as SkillCandidateV1;
    expect(skillCandidateAtBootstrap([approved, candidate])).toBe(candidate);
    expect(unseenSkillCandidateForSession([candidate], "s1", new Set())).toBe(candidate);
    expect(unseenSkillCandidateForSession([candidate], "s2", new Set())).toBe(null);
    expect(unseenSkillCandidateForSession([candidate], "s1", new Set(["c1"]))).toBe(null);
    const seen = new Set<string>();
    rememberSkillCandidateIds(seen, [candidate, approved]);
    expect([...seen]).toEqual(["c1", "c2"]);
  });

  it("merges content hits without archived rows or duplicates and respects limit", () => {
    const sessions = [
      { id: "s1", title: "one", projectId: null, archived: false },
      { id: "s2", title: "two", projectId: null, archived: false },
      { id: "s3", title: "three", projectId: null, archived: true },
    ];
    const hit = (sessionId: string) => ({ sessionId }) as SessionSearchResultV1;
    expect(
      mergeContentSearchSessions(
        [sessions[0]!],
        sessions,
        [hit("s1"), hit("s3"), hit("x"), hit("s2")],
      ).map((session) => session.id),
    ).toEqual(["s1", "s2"]);
    expect(mergeContentSearchSessions([], sessions, [hit("s1"), hit("s2")], 1)).toEqual([
      sessions[0],
    ]);
  });

  it("classifies and gates Host automation claims", () => {
    const claim = {
      claimId: "claim",
      sessionId: null,
    } as AutomationClaimV1;
    expect(automationClaimIntake(claim, new Set(), false)).toBe("queue");
    expect(automationClaimIntake({ ...claim, sessionId: "s1" }, new Set(), false)).toBe("bound");
    expect(automationClaimIntake(claim, new Set(["claim"]), false)).toBe("ignore");
    expect(automationClaimIntake(claim, new Set(), true)).toBe("ignore");
    expect(
      canStartAutomationClaim({
        claim,
        connecting: false,
        sessionState: "ready",
        runLocked: false,
        handledIds: new Set(),
      }),
    ).toBe(true);
    for (const blocked of [
      { connecting: true, sessionState: "ready", runLocked: false, handledIds: new Set<string>() },
      { connecting: false, sessionState: "streaming", runLocked: false, handledIds: new Set<string>() },
      { connecting: false, sessionState: "awaiting_permission", runLocked: false, handledIds: new Set<string>() },
      { connecting: false, sessionState: "ready", runLocked: true, handledIds: new Set<string>() },
      { connecting: false, sessionState: "ready", runLocked: false, handledIds: new Set(["claim"]) },
    ]) {
      expect(canStartAutomationClaim({ claim, ...blocked })).toBe(false);
    }
    expect(
      canStartAutomationClaim({
        claim: null,
        connecting: false,
        sessionState: "ready",
        runLocked: false,
        handledIds: new Set(),
      }),
    ).toBe(false);
  });
});
