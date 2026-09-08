// @vitest-environment jsdom

import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ComposerPrefs } from "@/lib/api";

const isTauriMock = vi.fn(() => true);
const composerPrefsResolveMock = vi.fn<
  (opts?: { projectId?: string | null; sessionId?: string | null }) => Promise<ComposerPrefs>
>();
const composerPrefsSetMock = vi.fn(async (body: Record<string, unknown>) => body);
const sessionSetPolicyMock = vi.fn(async (_policy?: unknown, _opts?: unknown) => null);
const providersActivateMock = vi.fn(async (..._args: unknown[]) => {});

vi.mock("@/lib/api", () => ({
  isTauri: () => isTauriMock(),
  composerPrefsResolve: (opts?: unknown) => composerPrefsResolveMock(opts as never),
  composerPrefsSet: (body: unknown) => composerPrefsSetMock(body as never),
  sessionSetPolicy: (policy: unknown, opts: unknown) =>
    sessionSetPolicyMock(policy as never, opts as never),
  providersActivate: (...args: unknown[]) => providersActivateMock(...args),
}));

import { useComposerCatalog } from "./useComposerCatalog";

function baseParams(overrides: Partial<Parameters<typeof useComposerCatalog>[0]> = {}) {
  return {
    activeProjectId: null,
    sessionId: null,
    locked: false,
    showToast: vi.fn(),
    tr: ((key: string) => key) as never,
    confirmAlwaysApprove: (onConfirmed: () => void) => onConfirmed(),
    refreshProviderRoute: vi.fn(async () => {}),
    patchSettingsSafely: vi.fn(async () => undefined),
    ...overrides,
  };
}

describe("useComposerCatalog", () => {
  beforeEach(() => {
    isTauriMock.mockReturnValue(true);
    composerPrefsResolveMock.mockReset();
    composerPrefsResolveMock.mockResolvedValue({
      modelId: "grok",
      effort: "medium",
      mode: "agent",
      permissionPolicy: "ask",
      scope: "global",
      source: "global",
    });
    composerPrefsSetMock.mockClear();
    sessionSetPolicyMock.mockClear();
    providersActivateMock.mockClear();
  });
  afterEach(() => {
    vi.clearAllMocks();
  });

  // The re-resolve effect also fires on mount (same as the real App, where
  // refreshLists' bootstrap and the project/session re-resolve effect both
  // run on first render) — settle it before exercising applyBootstrap
  // directly, so its resolution doesn't race the one under test.
  async function mountAndSettleInitialResolve(
    params = baseParams(),
  ) {
    const rendered = renderHook(() => useComposerCatalog(params));
    await waitFor(() => expect(composerPrefsResolveMock).toHaveBeenCalledTimes(1));
    composerPrefsResolveMock.mockClear();
    return rendered;
  }

  it("applyBootstrap falls back to raw settings when composerPrefsResolve fails", async () => {
    const { result } = await mountAndSettleInitialResolve();
    composerPrefsResolveMock.mockRejectedValueOnce(new Error("no prefs yet"));
    await act(async () => {
      await result.current.applyBootstrap(null, {
        permissionPolicy: "accept_edits",
        effort: "high",
        mode: "ask",
        modelId: "grok",
      } as never);
    });
    expect(result.current.policy).toBe("accept_edits");
    expect(result.current.effort).toBe("high");
    expect(result.current.mode).toBe("ask");
    expect(result.current.modelId).toBe("grok");
  });

  it("applyBootstrap applies resolved prefs when composerPrefsResolve succeeds", async () => {
    const { result } = await mountAndSettleInitialResolve();
    // Not `...Once`: applyBootstrap's own resolve call can race the
    // project/session re-resolve effect re-firing off the `availableModels`
    // update inside applyBootstrap — both should land on the same prefs.
    composerPrefsResolveMock.mockResolvedValue({
      modelId: "grok",
      effort: "low",
      mode: "plan",
      permissionPolicy: "dont_ask",
      scope: "project",
      source: "project",
    });
    await act(async () => {
      await result.current.applyBootstrap(null, {} as never);
    });
    expect(result.current.effort).toBe("low");
    expect(result.current.mode).toBe("plan");
    expect(result.current.policy).toBe("dont_ask");
    expect(result.current.prefsScope).toBe("project");
  });

  it("re-resolves prefs when the active project or session changes", async () => {
    const { rerender } = renderHook(
      (props: { activeProjectId: string | null }) =>
        useComposerCatalog(baseParams({ activeProjectId: props.activeProjectId })),
      { initialProps: { activeProjectId: null as string | null } },
    );
    await waitFor(() => expect(composerPrefsResolveMock).toHaveBeenCalledTimes(1));
    rerender({ activeProjectId: "proj-1" });
    await waitFor(() => expect(composerPrefsResolveMock).toHaveBeenCalledTimes(2));
    expect(composerPrefsResolveMock).toHaveBeenLastCalledWith({
      projectId: "proj-1",
      sessionId: null,
    });
  });

  it("onEffort rolls back on composer_prefs_set failure", async () => {
    const { result } = renderHook(() => useComposerCatalog(baseParams()));
    await waitFor(() => expect(composerPrefsResolveMock).toHaveBeenCalled());
    const previousEffort = result.current.effort;
    composerPrefsSetMock.mockRejectedValueOnce(new Error("network down"));
    await act(async () => {
      result.current.onEffort("high");
      await Promise.resolve();
      await Promise.resolve();
    });
    await waitFor(() => expect(result.current.effort).toBe(previousEffort));
  });

  it("onModel rolls back when providersActivate fails, without persisting", async () => {
    const { result } = renderHook(() => useComposerCatalog(baseParams()));
    await waitFor(() => expect(composerPrefsResolveMock).toHaveBeenCalled());
    const previousModelId = result.current.modelId;
    providersActivateMock.mockRejectedValueOnce(new Error("no route"));
    await act(async () => {
      result.current.onModel("grok");
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();
    });
    await waitFor(() => expect(result.current.modelId).toBe(previousModelId));
    expect(composerPrefsSetMock).not.toHaveBeenCalledWith(
      expect.objectContaining({ modelId: "grok" }),
    );
  });

  it("locked suppresses onMode/onEffort/onReset/onDisablePlan", async () => {
    const { result } = renderHook(() => useComposerCatalog(baseParams({ locked: true })));
    await waitFor(() => expect(composerPrefsResolveMock).toHaveBeenCalled());
    act(() => {
      result.current.onMode("plan");
      result.current.onEffort("high");
      result.current.onReset();
      result.current.onDisablePlan();
    });
    expect(composerPrefsSetMock).not.toHaveBeenCalled();
  });

  it("onPolicy commits immediately for non-YOLO policies", async () => {
    const { result } = renderHook(() => useComposerCatalog(baseParams()));
    await waitFor(() => expect(composerPrefsResolveMock).toHaveBeenCalled());
    act(() => {
      result.current.onPolicy("accept_edits");
    });
    expect(result.current.policy).toBe("accept_edits");
    expect(sessionSetPolicyMock).toHaveBeenCalledWith(
      "accept_edits",
      expect.objectContaining({ projectId: null, sessionId: null }),
    );
  });

  it("onPolicy routes always_approve through confirmAlwaysApprove before committing", async () => {
    const confirmAlwaysApprove = vi.fn((onConfirmed: () => void) => onConfirmed());
    const { result } = renderHook(() =>
      useComposerCatalog(baseParams({ confirmAlwaysApprove })),
    );
    await waitFor(() => expect(composerPrefsResolveMock).toHaveBeenCalled());
    act(() => {
      result.current.onPolicy("always_approve");
    });
    expect(confirmAlwaysApprove).toHaveBeenCalledTimes(1);
    expect(result.current.policy).toBe("always_approve");
    expect(sessionSetPolicyMock).toHaveBeenCalledWith(
      "always_approve",
      expect.anything(),
    );
  });

  it("onPolicy does not commit always_approve when confirmAlwaysApprove withholds confirmation", async () => {
    const confirmAlwaysApprove = vi.fn(() => {
      // User dismissed the two-step dialog — never calls onConfirmed.
    });
    const { result } = renderHook(() =>
      useComposerCatalog(baseParams({ confirmAlwaysApprove })),
    );
    await waitFor(() => expect(composerPrefsResolveMock).toHaveBeenCalled());
    act(() => {
      result.current.onPolicy("always_approve");
    });
    expect(sessionSetPolicyMock).not.toHaveBeenCalled();
  });

  it("onPrefsScope persists via patchSettingsSafely and re-resolves", async () => {
    const patchSettingsSafely = vi.fn(async () => undefined);
    const { result } = await mountAndSettleInitialResolve(
      baseParams({ patchSettingsSafely }),
    );
    act(() => {
      result.current.onPrefsScope("session");
    });
    expect(result.current.prefsScope).toBe("session");
    expect(patchSettingsSafely).toHaveBeenCalledWith({ composerPrefsScope: "session" });
    // Changing prefsScope also re-triggers the project/session re-resolve
    // effect (same as the pre-extraction inline handler did) in addition to
    // onPrefsScope's own explicit resolve — assert it fired, not an exact count.
    await waitFor(() => expect(composerPrefsResolveMock).toHaveBeenCalled());
  });
});
