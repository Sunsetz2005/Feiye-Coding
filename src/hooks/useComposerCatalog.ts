import { useCallback, useEffect, useState } from "react";
import * as api from "@/lib/api";
import { createT } from "@/i18n";
import {
  DEFAULT_EFFORT,
  DEFAULT_MODEL_ID,
  GROK_BUILD_MODELS,
  isValidEffort,
  isValidModelId,
  isValidPolicy,
  isValidPrefsScope,
  pickDefaultModelId,
  type ComposerPrefsScope,
  type EffortOption,
  type ModelOption,
  type PermissionPolicyId,
} from "@/lib/grokCatalog";
import { rollbackOptimisticSetting } from "@/lib/composerSettings";

/**
 * Model / reasoning-effort / mode / permission-policy cluster — the
 * "catalog" aligned with Grok Build CLI (docs/llm-wiki/catalog.md). Pull +
 * optimistic-write only: no `listen()` event feeds this, so unlike
 * transcript/job state there is nothing to subscribe to — just a bootstrap
 * call, a re-resolve effect keyed on project/session, and a handful of
 * mutators that each optimistically set then persist via
 * `composer_prefs_set` / `session_set_policy`, rolling back on failure.
 */

export interface UseComposerCatalogParams {
  activeProjectId: string | null;
  sessionId: string | null;
  /** `shouldLockComposerSettings(...)` — Runtime-busy/prompt states where prefs are inspectable only. */
  locked: boolean;
  showToast: (message: string, durationMs?: number) => void;
  tr: ReturnType<typeof createT>;
  /** App-owned two-step AppDialog confirm chain for `always_approve` (YOLO). Never window.confirm. */
  confirmAlwaysApprove: (onConfirmed: () => void) => void;
  /** Activates the provider route after a model switch; App owns `activeCustomProvider`. */
  refreshProviderRoute: () => void | Promise<void>;
  /** Clears goal-mode when the user explicitly switches to a non-goal mode via the composer chip. */
  onModeApplied?: (value: string) => void;
  /**
   * App's sequenced settings-patch helper (guards against a slower stale
   * patch clobbering a newer one across all settings fields, not just
   * catalog's). Used only by `onPrefsScope`.
   */
  patchSettingsSafely: (patch: Partial<api.AppSettings>) => Promise<unknown>;
}

export interface ComposerCatalogApi {
  modelId: string;
  effort: EffortOption["id"];
  mode: string;
  policy: string;
  availableModels: ModelOption[];
  prefsScope: ComposerPrefsScope;

  onMode: (value: string) => void;
  onPolicy: (
    value: PermissionPolicyId,
    opts?: { toastYoloToggle?: boolean },
  ) => void;
  onDisablePlan: () => void;
  onModel: (value: string) => void;
  onEffort: (value: EffortOption["id"]) => void;
  onReset: () => void;
  onPrefsScope: (value: ComposerPrefsScope) => void;

  /**
   * Escape hatches for call sites whose rollback/no-persist semantics
   * differ from `onMode`/`onPrefsScope` (goal-mode short-circuits, the
   * no-rollback `/plan` slash action, and authoritative-settings sync).
   * Do not route generic UI through these.
   */
  setModeRaw: React.Dispatch<React.SetStateAction<string>>;
  setPrefsScopeRaw: React.Dispatch<React.SetStateAction<ComposerPrefsScope>>;

  applyBootstrap: (
    modelsRes: api.AvailableModelsResult | null,
    settings: api.AppSettings,
  ) => Promise<void>;
  reresolve: () => Promise<void>;
}

export function useComposerCatalog(
  params: UseComposerCatalogParams,
): ComposerCatalogApi {
  const {
    activeProjectId,
    sessionId,
    locked,
    showToast,
    tr,
    confirmAlwaysApprove,
    refreshProviderRoute,
    onModeApplied,
    patchSettingsSafely,
  } = params;

  const [modelId, setModelId] = useState(DEFAULT_MODEL_ID);
  const [effort, setEffort] = useState(DEFAULT_EFFORT);
  const [mode, setMode] = useState("agent");
  const [policy, setPolicy] = useState("ask");
  /** Live selectable models from Host (official CLI catalog only; not providers). */
  const [availableModels, setAvailableModels] =
    useState<ModelOption[]>(GROK_BUILD_MODELS);
  /** Where model/permission chips are remembered. */
  const [prefsScope, setPrefsScope] = useState<ComposerPrefsScope>("global");

  const applyComposerPrefs = useCallback(
    (prefs: api.ComposerPrefs, catalog: ModelOption[]) => {
      const models = catalog.length > 0 ? catalog : GROK_BUILD_MODELS;
      if (prefs.modelId && isValidModelId(prefs.modelId, models)) {
        setModelId(prefs.modelId);
      } else {
        setModelId(pickDefaultModelId(models));
      }
      setEffort(isValidEffort(prefs.effort) ? prefs.effort : DEFAULT_EFFORT);
      setMode(prefs.mode || "agent");
      setPolicy(
        isValidPolicy(prefs.permissionPolicy) ? prefs.permissionPolicy : "ask",
      );
      if (isValidPrefsScope(prefs.scope)) {
        setPrefsScope(prefs.scope);
      }
    },
    [],
  );

  const applyBootstrap = useCallback(
    async (modelsRes: api.AvailableModelsResult | null, settings: api.AppSettings) => {
      const catalog: ModelOption[] =
        modelsRes?.models?.length
          ? modelsRes.models.map((m) => ({
              id: m.id,
              label: m.label || m.id,
              source: m.source,
              isDefault: m.isDefault,
              capabilities: m.capabilities,
            }))
          : GROK_BUILD_MODELS;
      setAvailableModels(catalog);
      if (
        settings.composerPrefsScope &&
        isValidPrefsScope(settings.composerPrefsScope)
      ) {
        setPrefsScope(settings.composerPrefsScope);
      }
      // Bootstrap: global-effective prefs (context re-resolved when project/session changes).
      const prefs = await api
        .composerPrefsResolve({ projectId: null, sessionId: null })
        .catch(() => null);
      if (prefs) {
        applyComposerPrefs(prefs, catalog);
      } else {
        setPolicy(
          isValidPolicy(settings.permissionPolicy || "")
            ? settings.permissionPolicy
            : "ask",
        );
        setEffort(
          isValidEffort(settings.effort || "")
            ? (settings.effort as typeof effort)
            : DEFAULT_EFFORT,
        );
        setMode(settings.mode || "agent");
        if (settings.modelId && isValidModelId(settings.modelId, catalog)) {
          setModelId(settings.modelId);
        } else {
          setModelId(
            modelsRes?.defaultModelId &&
              isValidModelId(modelsRes.defaultModelId, catalog)
              ? modelsRes.defaultModelId
              : pickDefaultModelId(catalog),
          );
        }
      }
    },
    [applyComposerPrefs],
  );

  const reresolve = useCallback(async () => {
    const prefs = await api.composerPrefsResolve({
      projectId: activeProjectId,
      sessionId: sessionId ?? null,
    });
    applyComposerPrefs(prefs, availableModels);
  }, [activeProjectId, sessionId, applyComposerPrefs, availableModels]);

  // Re-resolve model/permission when project or chat changes.
  // Permission always cascades project/session tiers (L10), even when model
  // memory scope is global — so project-level tiers apply after a switch.
  useEffect(() => {
    if (!api.isTauri()) return;
    let cancelled = false;
    void api
      .composerPrefsResolve({
        projectId: activeProjectId,
        sessionId: sessionId ?? null,
      })
      .then((prefs) => {
        if (!cancelled) applyComposerPrefs(prefs, availableModels);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [activeProjectId, sessionId, prefsScope, applyComposerPrefs, availableModels]);

  const onMode = useCallback(
    (value: string) => {
      if (locked) return;
      const previousMode = mode;
      setMode(value);
      onModeApplied?.(value);
      void api
        .composerPrefsSet({
          projectId: activeProjectId,
          sessionId: sessionId ?? null,
          mode: value,
        })
        .catch((error) => {
          setMode((current) =>
            rollbackOptimisticSetting(current, value, previousMode),
          );
          showToast(String(error), 4000);
        });
    },
    [locked, mode, onModeApplied, activeProjectId, sessionId, showToast],
  );

  const onDisablePlan = useCallback(() => {
    if (locked) return;
    const previousMode = mode;
    setMode("agent");
    void api
      .composerPrefsSet({
        projectId: activeProjectId,
        sessionId: sessionId ?? null,
        mode: "agent",
      })
      .catch((error) => {
        setMode((current) =>
          rollbackOptimisticSetting(current, "agent", previousMode),
        );
        showToast(String(error), 4000);
      });
  }, [locked, mode, activeProjectId, sessionId, showToast]);

  const onModel = useCallback(
    (value: string) => {
      if (locked) return;
      if (!isValidModelId(value, availableModels)) return;
      const previousModelId = modelId;
      setModelId(value);
      const selected = availableModels.find((item) => item.id === value);
      void (async () => {
        try {
          if (api.isTauri()) {
            if (selected?.source === "custom") {
              await api.providersActivate("custom", value);
            } else {
              await api.providersActivate("official");
            }
            await refreshProviderRoute();
          }
          await api.composerPrefsSet({
            projectId: activeProjectId,
            sessionId: sessionId ?? null,
            modelId: value,
          });
        } catch (error) {
          setModelId((current) =>
            rollbackOptimisticSetting(current, value, previousModelId),
          );
          showToast(String(error), 4000);
        }
      })();
    },
    [
      locked,
      availableModels,
      modelId,
      refreshProviderRoute,
      activeProjectId,
      sessionId,
      showToast,
    ],
  );

  const onEffort = useCallback(
    (value: EffortOption["id"]) => {
      if (locked) return;
      if (!isValidEffort(value)) return;
      const previousEffort = effort;
      setEffort(value);
      void api
        .composerPrefsSet({
          projectId: activeProjectId,
          sessionId: sessionId ?? null,
          effort: value,
        })
        .catch((error) => {
          setEffort((current) =>
            rollbackOptimisticSetting(current, value, previousEffort),
          );
          showToast(String(error), 4000);
        });
    },
    [locked, effort, activeProjectId, sessionId, showToast],
  );

  const onReset = useCallback(() => {
    if (locked) return;
    const nextModelId = pickDefaultModelId(availableModels);
    const previousModelId = modelId;
    const previousEffort = effort;
    setModelId(nextModelId);
    setEffort(DEFAULT_EFFORT);
    void api
      .composerPrefsSet({
        projectId: activeProjectId,
        sessionId: sessionId ?? null,
        modelId: nextModelId,
        effort: DEFAULT_EFFORT,
      })
      .catch((error) => {
        setModelId((current) =>
          rollbackOptimisticSetting(current, nextModelId, previousModelId),
        );
        setEffort((current) =>
          rollbackOptimisticSetting(current, DEFAULT_EFFORT, previousEffort),
        );
        showToast(String(error), 4000);
      });
  }, [locked, availableModels, modelId, effort, activeProjectId, sessionId, showToast]);

  /**
   * Apply permission policy (incl. YOLO). Never use window.confirm in Tauri —
   * the WebView's implementation is unreliable (often always false).
   */
  const onPolicy = useCallback(
    (next: PermissionPolicyId, opts?: { toastYoloToggle?: boolean }) => {
      if (!isValidPolicy(next)) return;

      const commit = () => {
        setPolicy(next);
        void api
          .sessionSetPolicy(next, {
            projectId: activeProjectId,
            sessionId: sessionId ?? null,
          })
          .catch((e) => showToast(String(e), 4000));
        if (opts?.toastYoloToggle) {
          showToast(
            next === "always_approve" ? tr("slash.yoloOn") : tr("slash.yoloOff"),
            2500,
          );
        }
      };

      if (next !== "always_approve") {
        commit();
        return;
      }

      confirmAlwaysApprove(commit);
    },
    [activeProjectId, sessionId, showToast, tr, confirmAlwaysApprove],
  );

  const onPrefsScope = useCallback(
    (value: ComposerPrefsScope) => {
      if (!isValidPrefsScope(value)) return;
      setPrefsScope(value);
      void patchSettingsSafely({ composerPrefsScope: value });
      void api
        .composerPrefsResolve({
          projectId: activeProjectId,
          sessionId: sessionId ?? null,
        })
        .then((prefs) => applyComposerPrefs(prefs, availableModels))
        .catch(() => {});
    },
    [
      activeProjectId,
      sessionId,
      applyComposerPrefs,
      availableModels,
      patchSettingsSafely,
    ],
  );

  return {
    modelId,
    effort,
    mode,
    policy,
    availableModels,
    prefsScope,
    onMode,
    onPolicy,
    onDisablePlan,
    onModel,
    onEffort,
    onReset,
    onPrefsScope,
    setModeRaw: setMode,
    setPrefsScopeRaw: setPrefsScope,
    applyBootstrap,
    reresolve,
  };
}
