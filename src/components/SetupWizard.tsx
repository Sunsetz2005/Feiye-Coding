/**
 * Full-screen first-run gate: install Sunsetz Runtime (required) → account (skippable) → enter home.
 * No page scrollbars; content is centered and compact.
 */

import { useCallback, useEffect, useMemo, useState } from "react";
import { SunsetzLogo } from "@/components/SunsetzLogo";
import { Select } from "@/components/Select";
import { Spinner } from "@/components/ui/spinner";
import * as api from "@/lib/api";
import type { createT } from "@/i18n";

type Tr = ReturnType<typeof createT>;

export type SetupCliInfo = {
  found: boolean;
  path: string | null;
  version: string | null;
  source: string;
  cliAuthPresent: boolean;
};

type Step = "runtime" | "account" | "ready";
type AccountPanel = "menu" | "key" | "relay";

type Props = {
  tr: Tr;
  platform: "mac" | "win" | "other";
  useCustomWindowChrome: boolean;
  initialCli: SetupCliInfo;
  onComplete: (cli: SetupCliInfo) => void;
  onAccountLoginOauth: () => Promise<boolean>;
};

function mirrorHost(url: string | null | undefined): string {
  if (!url) return "";
  try {
    return new URL(url).host;
  } catch {
    return url.replace(/^https?:\/\//, "").split("/")[0] || url;
  }
}

export function SetupWizard({
  tr,
  platform,
  useCustomWindowChrome,
  initialCli,
  onComplete,
  onAccountLoginOauth,
}: Props) {
  const [step, setStep] = useState<Step>(initialCli.found ? "account" : "runtime");
  const [cli, setCli] = useState<SetupCliInfo>(initialCli);
  const [probing, setProbing] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [progress, setProgress] = useState<api.CliInstallProgress | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [statusMsg, setStatusMsg] = useState<string | null>(null);
  const [installCmds, setInstallCmds] = useState<api.CliInstallCommands | null>(
    null,
  );
  const [copied, setCopied] = useState(false);
  const [accountPanel, setAccountPanel] = useState<AccountPanel>("menu");
  const [accountBusy, setAccountBusy] = useState(false);
  const [authOk, setAuthOk] = useState(
    () => initialCli.cliAuthPresent,
  );
  const [authDeferred, setAuthDeferred] = useState(false);
  const [officialKey, setOfficialKey] = useState("");
  const [relayBase, setRelayBase] = useState("");
  const [relayKey, setRelayKey] = useState("");
  /** Default: OpenAI Responses — preferred for modern gateways. */
  const [relayBackend, setRelayBackend] = useState("responses");

  const protocolOptions = useMemo(
    () => [
      { value: "responses", label: tr("prov.protocol.responses") },
      {
        value: "chat_completions",
        label: tr("prov.protocol.chatCompletions"),
      },
      { value: "messages", label: tr("prov.protocol.messages") },
    ],
    [tr],
  );

  useEffect(() => {
    void api.cliInstallCommands().then(setInstallCmds).catch(() => null);
  }, []);

  // Live install progress from Host
  useEffect(() => {
    if (!api.isTauri()) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void (async () => {
      try {
        const { listen } = await import("@tauri-apps/api/event");
        unlisten = await listen<api.CliInstallProgress>(
          "setup://cli-install-progress",
          (ev) => {
            if (!cancelled) setProgress(ev.payload);
          },
        );
      } catch {
        /* ignore */
      }
    })();
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  const recheck = useCallback(async (manualPath?: string | null) => {
    setProbing(true);
    setError(null);
    try {
      const r = await api.probeCli(manualPath || undefined);
      const next: SetupCliInfo = {
        found: r.found,
        path: r.path,
        version: r.version,
        source: r.source || "",
        cliAuthPresent: !!r.cliAuthPresent,
      };
      setCli(next);
      if (next.cliAuthPresent) setAuthOk(true);
      if (next.found) {
        setStatusMsg(null);
      }
      return next;
    } catch (e) {
      setError(String(e));
      return null;
    } finally {
      setProbing(false);
    }
  }, []);

  // Soft auto-detect once when opening runtime step without CLI
  useEffect(() => {
    if (step !== "runtime" || cli.found) return;
    void recheck();
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const runInstall = useCallback(async () => {
    if (installing) return;
    setInstalling(true);
    setError(null);
    setProgress({
      phase: "resolving",
      message: tr("setup.detecting"),
      percent: 0,
    });
    try {
      const res = await api.cliInstallLatest();
      if (!res.ok) {
        setError(res.message || tr("setup.error"));
        return;
      }
      const next = await recheck(res.path);
      if (next?.found) {
        setStep("account");
      } else {
        setError(tr("setup.cli.missing"));
      }
    } catch (e) {
      const msg = String(e);
      setError(msg);
      setProgress((p) =>
        p
          ? { ...p, phase: "error", message: msg }
          : { phase: "error", message: msg },
      );
    } finally {
      setInstalling(false);
    }
  }, [installing, recheck, tr]);

  const pickBinary = useCallback(async () => {
    setError(null);
    try {
      const path = await api.pickCliBinary();
      if (!path) return;
      await api.settingsGet().then((s) =>
        api.settingsSet({ ...s, manualCliPath: path }),
      );
      const next = await recheck(path);
      if (next?.found) {
        setStep("account");
      } else {
        setError(tr("setup.cli.missing"));
      }
    } catch (e) {
      setError(String(e));
    }
  }, [recheck, tr]);

  const copyCmd = useCallback(async () => {
    const cmd = installCmds?.primary;
    if (!cmd) return;
    try {
      await navigator.clipboard.writeText(cmd);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1800);
    } catch {
      setError(tr("setup.error"));
    }
  }, [installCmds, tr]);

  const openDocs = useCallback(() => {
    const url = installCmds?.docsUrl || "https://docs.x.ai/build/overview";
    void api.openExternalUrl(url).catch((e) => setError(String(e)));
  }, [installCmds]);

  const finishWizard = useCallback(
    async (opts: { authDeferred: boolean; authOk: boolean }) => {
      try {
        const s = await api.settingsGet();
        await api.settingsSet({
          ...s,
          setupWizardCompleted: true,
          authSetupDeferred: opts.authDeferred && !opts.authOk,
          onboardingDone: true,
          setupSkipped: opts.authDeferred && !opts.authOk,
        });
      } catch {
        /* still enter if probe ok */
      }
      onComplete(cli);
    },
    [cli, onComplete],
  );

  const goAccountContinue = useCallback(() => {
    if (!cli.found) return;
    setStep("ready");
  }, [cli.found]);

  const skipAccount = useCallback(() => {
    setAuthDeferred(true);
    setStep("ready");
  }, []);

  const saveOfficialKey = useCallback(async () => {
    const key = officialKey.trim();
    if (!key) return;
    setAccountBusy(true);
    setError(null);
    try {
      await api.secretsSet({ officialApiKey: key });
      setAuthOk(true);
      setStatusMsg(tr("setup.account.ok"));
      setAccountPanel("menu");
      setStep("ready");
    } catch (e) {
      setError(String(e));
    } finally {
      setAccountBusy(false);
    }
  }, [officialKey, tr]);

  const saveRelay = useCallback(async () => {
    const base = relayBase.trim();
    const key = relayKey.trim();
    if (!base || !key) return;
    setAccountBusy(true);
    setError(null);
    try {
      const discovered = await api.providersListModels({
        baseUrl: base,
        apiKey: key,
      });
      const model = discovered.models[0]?.id?.trim();
      if (!model) {
        setError(tr("prov.err.noModels"));
        return;
      }
      const ping = await api.providersPing({ baseUrl: base, apiKey: key });
      if (!ping.ok) {
        setError(
          tr("prov.err.pingFailed", {
            status: ping.status ? String(ping.status) : "—",
          }),
        );
        return;
      }
      // Only persist and activate after the normalized endpoint is verified.
      const saved = await api.providersUpsert({
        id: "relay",
        model,
        baseUrl: base,
        name: "Custom relay",
        apiKey: key,
        apiBackend: relayBackend || "responses",
        setAsDefault: true,
      });
      const normalizedBase =
        saved.providers.find((provider) => provider.id === "relay")?.baseUrl ??
        base;
      await api.secretsSet({
        relayBaseUrl: normalizedBase,
        relayApiKey: key,
      });
      setStatusMsg(tr("setup.account.ok"));
      setAuthOk(true);
      setAccountPanel("menu");
      setStep("ready");
    } catch (e) {
      setError(tr("error.command.invalid"));
    } finally {
      setAccountBusy(false);
    }
  }, [relayBase, relayKey, relayBackend, tr]);

  const runOauth = useCallback(async () => {
    setAccountBusy(true);
    setError(null);
    try {
      const ok = await onAccountLoginOauth();
      if (ok) {
        setAuthOk(true);
        setStatusMsg(tr("setup.account.ok"));
        setStep("ready");
      }
      const next = await recheck(cli.path);
      if (next?.cliAuthPresent) {
        setAuthOk(true);
        setStep("ready");
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setAccountBusy(false);
    }
  }, [cli.path, onAccountLoginOauth, recheck, tr]);

  const importCli = useCallback(async () => {
    setAccountBusy(true);
    setError(null);
    try {
      const r = await api.importGrokCli();
      if ((r as { ok?: boolean }).ok) {
        setAuthOk(true);
        setStatusMsg(tr("setup.account.ok"));
        setStep("ready");
      } else {
        setStatusMsg(JSON.stringify((r as { messages?: string[] }).messages || r));
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setAccountBusy(false);
    }
  }, [tr]);

  const importGo = useCallback(async () => {
    setAccountBusy(true);
    setError(null);
    try {
      await api.importGrokGo();
      setAuthOk(true);
      setStatusMsg(tr("setup.account.ok"));
      setStep("ready");
    } catch (e) {
      setError(String(e));
    } finally {
      setAccountBusy(false);
    }
  }, [tr]);

  /** Abort the running login (OAuth/device) and unlock the UI immediately.
   *  The backend kills the `grok login` child; the pending handler's `finally`
   *  also clears accountBusy, but we reset here so the UI is instant. */
  const cancelAccountLogin = useCallback(async () => {
    try {
      await api.accountLoginCancel();
    } catch {
      /* host may be unavailable; still unlock UI below */
    }
    setAccountBusy(false);
  }, []);

  const percent = useMemo(() => {
    const p = progress?.percent;
    if (p == null || Number.isNaN(p)) return installing ? 8 : 0;
    return Math.max(0, Math.min(100, Math.round(p)));
  }, [progress, installing]);

  const stepIndex = step === "runtime" ? 0 : step === "account" ? 1 : 2;

  return (
    <div
      className={
        "setup-gate" +
        (useCustomWindowChrome ? " setup-gate--custom-chrome" : "")
      }
      data-platform={platform}
      data-testid="setup-wizard"
    >
      <div className="setup-gate__drag" data-tauri-drag-region />

      <div className="setup-gate__center">
        <div className="setup-hero">
          <div
            className={
              "setup-logo" +
              (installing || probing ? " setup-logo--spin" : " setup-logo--pulse")
            }
          >
            <SunsetzLogo size={44} />
          </div>
          <h1 className="setup-title">{tr("setup.title")}</h1>
          <p className="setup-subtitle">{tr("setup.subtitle")}</p>
        </div>

        <ol className="setup-steps" aria-label="Setup steps">
          {(
            [
              ["runtime", "setup.step.runtime"],
              ["account", "setup.step.account"],
              ["ready", "setup.step.ready"],
            ] as const
          ).map(([id, key], i) => (
            <li
              key={id}
              className={
                "setup-steps__item" +
                (i === stepIndex ? " is-active" : "") +
                (i < stepIndex ? " is-done" : "")
              }
            >
              <span className="setup-steps__dot" />
              <span className="setup-steps__label">{tr(key)}</span>
            </li>
          ))}
        </ol>

        <div className="setup-card">
          {step === "runtime" && (
            <>
              <div className="setup-card__head">
                <h2>
                  {cli.found
                    ? tr("setup.cli.found")
                    : tr("setup.cli.required")}
                </h2>
                <p>
                  {cli.found
                    ? tr("setup.cli.foundHint", {
                        version: cli.version || "—",
                      })
                    : tr("setup.cli.requiredHint")}
                </p>
                {cli.path && (
                  <p className="setup-mono">
                    {tr("setup.cli.path", { path: cli.path })}
                  </p>
                )}
              </div>

              {(installing || progress) && (
                <div className="setup-progress" aria-live="polite">
                  <div className="setup-progress__track">
                    <div
                      className="setup-progress__fill"
                      style={{ width: `${percent}%` }}
                    />
                  </div>
                  <div className="setup-progress__meta">
                    <span>
                      {progress?.message ||
                        (installing
                          ? tr("setup.installing")
                          : tr("setup.detecting"))}
                    </span>
                    <span>{tr("setup.progress", { percent })}</span>
                  </div>
                  {progress?.mirror && (
                    <div className="setup-progress__mirror">
                      {tr("setup.mirror", {
                        host: mirrorHost(progress.mirror),
                      })}
                    </div>
                  )}
                </div>
              )}

              {!cli.found && !installing && (
                <p className="setup-hint">{tr("setup.manualHint")}</p>
              )}

              <div className="setup-actions">
                {cli.found ? (
                  <button
                    type="button"
                    className="btn btn--primary setup-btn-primary"
                    onClick={() => setStep("account")}
                  >
                    {tr("setup.continue")}
                  </button>
                ) : (
                  <button
                    type="button"
                    className="btn btn--primary setup-btn-primary"
                    disabled={installing || probing}
                    onClick={() => void runInstall()}
                  >
                    {installing ? (
                      <>
                        <Spinner className="size-4" />
                        {tr("setup.installing")}
                      </>
                    ) : (
                      tr("setup.install")
                    )}
                  </button>
                )}
                <div className="setup-actions__row">
                  <button
                    type="button"
                    className="btn btn--ghost"
                    disabled={installing || probing}
                    onClick={() => void recheck()}
                  >
                    {probing ? <Spinner className="size-3.5" /> : null}
                    {tr("setup.recheck")}
                  </button>
                  <button
                    type="button"
                    className="btn btn--ghost"
                    disabled={installing}
                    onClick={() => void pickBinary()}
                  >
                    {tr("setup.pickBinary")}
                  </button>
                </div>
                <div className="setup-actions__row">
                  <button
                    type="button"
                    className="btn btn--ghost"
                    disabled={!installCmds?.primary}
                    onClick={() => void copyCmd()}
                  >
                    {copied ? tr("setup.copied") : tr("setup.copyCmd")}
                  </button>
                  <button
                    type="button"
                    className="btn btn--ghost"
                    onClick={openDocs}
                  >
                    {tr("setup.openDocs")}
                  </button>
                </div>
                {installCmds?.primary && (
                  <code className="setup-cmd">{installCmds.primary}</code>
                )}
              </div>
            </>
          )}

          {step === "account" && (
            <>
              <div className="setup-card__head">
                <h2>{tr("setup.account.title")}</h2>
                <p>{tr("setup.account.hint")}</p>
              </div>

              {accountPanel === "menu" && (
                <div className="setup-entry-grid">
                  <button
                    type="button"
                    className="setup-entry"
                    disabled={accountBusy}
                    onClick={() => void runOauth()}
                  >
                    <strong>{tr("setup.account.oauth")}</strong>
                    <span>{tr("setup.account.oauthHint")}</span>
                  </button>
                  <button
                    type="button"
                    className="setup-entry"
                    disabled={accountBusy}
                    onClick={() => setAccountPanel("key")}
                  >
                    <strong>{tr("setup.account.key")}</strong>
                    <span>{tr("setup.account.keyHint")}</span>
                  </button>
                  <button
                    type="button"
                    className="setup-entry"
                    disabled={accountBusy}
                    onClick={() => setAccountPanel("relay")}
                  >
                    <strong>{tr("setup.account.relay")}</strong>
                    <span>{tr("setup.account.relayHint")}</span>
                  </button>
                  {cli.cliAuthPresent && (
                    <button
                      type="button"
                      className="setup-entry"
                      disabled={accountBusy}
                      onClick={() => void importCli()}
                    >
                      <strong>{tr("setup.account.importCli")}</strong>
                      <span>{tr("setup.account.importCliHint")}</span>
                    </button>
                  )}
                  <button
                    type="button"
                    className="setup-entry"
                    disabled={accountBusy}
                    onClick={() => void importGo()}
                  >
                    <strong>{tr("setup.account.importGo")}</strong>
                    <span>{tr("onboarding.importGoHint")}</span>
                  </button>
                </div>
              )}

              {accountPanel === "key" && (
                <div className="setup-form">
                  <input
                    className="setup-input"
                    type="password"
                    autoComplete="off"
                    placeholder={tr("setup.account.keyPh")}
                    value={officialKey}
                    onChange={(e) => setOfficialKey(e.target.value)}
                  />
                  <div className="setup-actions__row">
                    <button
                      type="button"
                      className="btn btn--ghost"
                      onClick={() => setAccountPanel("menu")}
                    >
                      {tr("common.cancel")}
                    </button>
                    <button
                      type="button"
                      className="btn btn--primary"
                      disabled={accountBusy || !officialKey.trim()}
                      onClick={() => void saveOfficialKey()}
                    >
                      {tr("setup.account.saveKey")}
                    </button>
                  </div>
                </div>
              )}

              {accountPanel === "relay" && (
                <div className="setup-form">
                  <input
                    className="setup-input"
                    type="url"
                    autoComplete="off"
                    placeholder={tr("setup.account.basePh")}
                    value={relayBase}
                    onChange={(e) => setRelayBase(e.target.value)}
                  />
                  <input
                    className="setup-input"
                    type="password"
                    autoComplete="off"
                    placeholder={tr("setup.account.relayKeyPh")}
                    value={relayKey}
                    onChange={(e) => setRelayKey(e.target.value)}
                  />
                  <label className="setup-field">
                    <span className="setup-field__label">
                      {tr("setup.account.protocol")}
                    </span>
                    <Select
                      value={relayBackend}
                      onChange={setRelayBackend}
                      options={protocolOptions}
                      aria-label={tr("setup.account.protocol")}
                    />
                  </label>
                  <div className="setup-actions__row">
                    <button
                      type="button"
                      className="btn btn--ghost"
                      onClick={() => setAccountPanel("menu")}
                    >
                      {tr("common.cancel")}
                    </button>
                    <button
                      type="button"
                      className="btn btn--primary"
                      disabled={
                        accountBusy || !relayBase.trim() || !relayKey.trim()
                      }
                      onClick={() => void saveRelay()}
                    >
                      {tr("setup.account.saveRelay")}
                    </button>
                  </div>
                </div>
              )}

              {accountBusy && (
                <div className="setup-busy">
                  <Spinner className="size-4" />
                  {tr("setup.account.busy")}
                  <button
                    type="button"
                    className="btn btn--ghost btn--sm"
                    onClick={() => void cancelAccountLogin()}
                  >
                    {tr("setup.account.cancelBusy")}
                  </button>
                </div>
              )}

              <div className="setup-actions setup-actions--footer">
                <button
                  type="button"
                  className="btn btn--ghost"
                  disabled={accountBusy}
                  onClick={skipAccount}
                >
                  {tr("setup.account.skip")}
                </button>
                {(authOk || accountPanel === "menu") && (
                  <button
                    type="button"
                    className="btn btn--primary"
                    disabled={accountBusy}
                    onClick={goAccountContinue}
                  >
                    {tr("setup.continue")}
                  </button>
                )}
              </div>
            </>
          )}

          {step === "ready" && (
            <>
              <div className="setup-card__head">
                <h2>{tr("setup.ready.title")}</h2>
              </div>
              <ul className="setup-checklist">
                <li className="is-ok">
                  <span className="setup-check" />
                  {tr("setup.ready.cliOk")}
                  {cli.version ? (
                    <span className="setup-check-meta">{cli.version}</span>
                  ) : null}
                </li>
                <li className={authOk ? "is-ok" : "is-soft"}>
                  <span className="setup-check" />
                  {authOk
                    ? tr("setup.ready.authOk")
                    : tr("setup.ready.authSkip")}
                </li>
              </ul>
              <div className="setup-actions">
                <button
                  type="button"
                  className="btn btn--primary setup-btn-primary"
                  disabled={!cli.found}
                  onClick={() =>
                    void finishWizard({
                      authDeferred: authDeferred || !authOk,
                      authOk,
                    })
                  }
                >
                  {tr("setup.ready.enter")}
                </button>
              </div>
            </>
          )}

          {error && (
            <div className="setup-error" role="alert">
              <strong>{tr("setup.error")}</strong>
              <span>{error}</span>
              {/network|timeout|mirror|download|HTTP|failed/i.test(error) && (
                <span className="setup-error__hint">
                  {tr("setup.networkHint")}
                </span>
              )}
            </div>
          )}
          {statusMsg && !error && (
            <div className="setup-status" role="status">
              {statusMsg}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
