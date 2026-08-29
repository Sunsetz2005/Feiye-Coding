// @vitest-environment jsdom

import { afterEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.hoisted(() => vi.fn(async (command: string, args?: unknown) => ({ command, args })));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

import * as api from "./api";

function setDesktop(enabled: boolean): void {
  if (enabled) {
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      value: {},
      configurable: true,
    });
  } else {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
    delete (window as unknown as Record<string, unknown>).__TAURI__;
  }
}

describe("migration API contracts", () => {
  afterEach(() => {
    setDesktop(false);
    invokeMock.mockClear();
  });

  it("returns explicit browser capability fallbacks", async () => {
    setDesktop(false);
    await expect(api.runtimeCapabilitiesV1()).resolves.toMatchObject({
      version: 1,
      clientVersion: "browser",
      sandbox: { requested: "off", verified: true },
    });
    await expect(api.capabilityManifestExportV1()).rejects.toThrow("desktop Host");
    await expect(
      api.capabilityManifestValidateV1({} as api.CapabilityManifestV1),
    ).resolves.toEqual({
      version: 1,
      valid: false,
      errors: ["Desktop Host required"],
    });
    await expect(api.sessionInteractionsList("s1")).resolves.toEqual([]);
    await expect(api.sessionPlanArtifactsListV1("s1")).resolves.toEqual([]);
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("routes capabilities and interaction commands with versioned payloads", async () => {
    setDesktop(true);
    const manifest = {
      schema: "sunsetz.capabilities.v1",
      version: 1,
      producer: "test",
      generatedAt: "2026-01-01T00:00:00Z",
      capabilities: [],
    } as api.CapabilityManifestV1;
    await api.runtimeCapabilitiesV1();
    await api.capabilityManifestExportV1();
    await api.capabilityManifestValidateV1(manifest);
    await api.sessionInteractionsList();
    await api.sessionInteractionsList("s1");
    await api.sessionPlanArtifactsListV1("s1");
    await api.connectorsList();
    await api.connectorsConnect("gmail");
    await api.connectorsDisconnect("gmail");
    await api.sessionResolveInteractionV1({
      interactionId: "i1",
      sessionId: "s1",
      decision: "deny",
    });
    expect(invokeMock.mock.calls).toEqual(
      expect.arrayContaining([
        ["runtime_capabilities_v1", undefined],
        ["capability_manifest_export_v1", undefined],
        ["capability_manifest_validate_v1", { manifest }],
        ["session_interactions_list", { sessionId: null }],
        ["session_interactions_list", { sessionId: "s1" }],
        ["session_plan_artifacts_list_v1", { sessionId: "s1" }],
        ["connectors_list", undefined],
        ["connectors_connect", { id: "gmail", credential: null }],
        ["connectors_disconnect", { id: "gmail" }],
      ]),
    );
  });

  it("keeps legacy interaction adapters compatible while forwarding stable ids", async () => {
    setDesktop(true);
    await api.sessionResolvePermission({
      interactionId: "i1",
      sessionId: "s1",
      rpcId: 1,
      decision: "allow",
      optionId: "once",
      scopeKey: "shell:npm test",
    });
    await api.sessionResolvePlan({ decision: "approved" });
    await api.sessionResolveAskUser({
      interactionId: "i2",
      decision: "accepted",
      answers: { q: "yes" },
    });
    expect(invokeMock).toHaveBeenCalledWith("session_resolve_permission", {
      interactionId: "i1",
      sessionId: "s1",
      rpcId: 1,
      decision: "allow",
      optionId: "once",
      scopeKey: "shell:npm test",
    });
    expect(invokeMock).toHaveBeenCalledWith(
      "session_resolve_plan",
      expect.objectContaining({ interactionId: null, sessionId: null, rpcId: null }),
    );
    expect(invokeMock).toHaveBeenCalledWith(
      "session_resolve_ask_user",
      expect.objectContaining({ interactionId: "i2", sessionId: null, rpcId: null }),
    );
  });

  it("routes candidate, resource, search, plugin, and automation bridges", async () => {
    setDesktop(true);
    await api.skillCandidatesListV1();
    await api.skillCandidateApproveV1({
      id: "c1",
      scope: "user",
      overwrite: false,
    });
    await api.skillCandidateRejectV1("c1");
    const skillDecision = { id: "c1", expectedContentHash: "b".repeat(64) };
    await api.skillCandidateApproveV2({
      ...skillDecision,
      scope: "user",
      overwrite: false,
    });
    await api.skillCandidateRejectV2(skillDecision);
    await api.skillCandidateCancelV2(skillDecision);
    const memoryMutation = { id: "m1", expectedContentHash: "a".repeat(64) };
    await api.memoryCandidatesListV1();
    await api.memoryCandidateCreateV1({
      type: "user_preference",
      content: "Prefer concise answers.",
      source: { sessionId: "s1", messageId: "msg1" },
    });
    await api.memoryCandidateApproveV1(memoryMutation);
    await api.memoryCandidateRejectV1(memoryMutation);
    await api.memoryCandidateSupersedeV1(memoryMutation);
    await api.memoryCandidateDeleteV1(memoryMutation);
    await api.memoryContextPackBuildV1([memoryMutation]);
    await api.resourceOpenV1("/project/a.png");
    await api.resourceReadV1("handle");
    await api.sessionSearchV1("migration");
    await api.sessionSearchV1("migration", 5);
    await api.sessionSearchRebuildV1();
    await api.sessionSearchDeleteIndexV1();
    await api.runtimePluginsCatalogV1("  hooks  ");
    await api.runtimePluginsCatalogV1("   ");
    await api.runtimeHooksInventoryV1();
    await api.automationClaimBindV1("claim", "session");
    await api.automationClaimCompleteV1("claim", false);
    await api.automationClaimCompleteV1("claim", true, "ignored");
    await api.settingsPatchV1({ theme: "light", manualCliPath: null });
    expect(invokeMock).toHaveBeenCalledWith("session_search_v1", {
      query: "migration",
      limit: 40,
    });
    expect(invokeMock).toHaveBeenCalledWith("memory_candidate_approve_v1", {
      request: memoryMutation,
    });
    expect(invokeMock).toHaveBeenCalledWith("memory_context_pack_build_v1", {
      request: { version: 1, selections: [memoryMutation] },
    });
    expect(invokeMock).toHaveBeenCalledWith("skill_candidate_cancel_v2", {
      request: skillDecision,
    });
    expect(invokeMock).toHaveBeenCalledWith("runtime_plugins_catalog_v1", {
      query: "hooks",
    });
    expect(invokeMock).toHaveBeenCalledWith("runtime_plugins_catalog_v1", {
      query: null,
    });
    expect(invokeMock).toHaveBeenCalledWith("automation_claim_complete_v1", {
      claimId: "claim",
      success: false,
      error: null,
    });
    expect(invokeMock).toHaveBeenCalledWith("settings_patch_v1", {
      patch: { theme: "light", manualCliPath: null },
    });
  });
});
