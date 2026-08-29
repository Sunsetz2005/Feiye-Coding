/**
 * First-run gate: sign in with a Sunsetz account, or skip into the workbench.
 * Custom APIs live in Settings → My models. Grok CLI is not part of setup.
 */

import { useCallback, useState } from "react";
import { SunsetzLogo } from "@/components/SunsetzLogo";
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

type Props = {
  tr: Tr;
  platform: "mac" | "win" | "other";
  useCustomWindowChrome: boolean;
  initialCli: SetupCliInfo;
  onComplete: (cli: SetupCliInfo) => void;
  onAccountLoginOauth: () => Promise<boolean>;
};

export function SetupWizard({
  tr,
  platform,
  useCustomWindowChrome,
  initialCli,
  onComplete,
  onAccountLoginOauth,
}: Props) {
  const [cli] = useState<SetupCliInfo>(initialCli);
  const [error, setError] = useState<string | null>(null);
  const [accountBusy, setAccountBusy] = useState(false);

  const finishWizard = useCallback(
    async (opts: { authDeferred: boolean; authOk: boolean }) => {
      try {
        await api.settingsPatchV1({
          setupWizardCompleted: true,
          authSetupDeferred: opts.authDeferred && !opts.authOk,
          onboardingDone: true,
          setupSkipped: opts.authDeferred && !opts.authOk,
          runtimeBackend: "sunsetz",
        });
      } catch {
        /* still enter */
      }
      onComplete(cli);
    },
    [cli, onComplete],
  );

  const runOauth = useCallback(async () => {
    setAccountBusy(true);
    setError(null);
    try {
      const ok = await onAccountLoginOauth();
      if (ok) {
        await finishWizard({ authDeferred: false, authOk: true });
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setAccountBusy(false);
    }
  }, [finishWizard, onAccountLoginOauth]);

  const skipAccount = useCallback(() => {
    void finishWizard({ authDeferred: true, authOk: false });
  }, [finishWizard]);

  const cancelAccountLogin = useCallback(async () => {
    try {
      await api.accountLoginCancel();
    } catch {
      /* still unlock */
    }
    setAccountBusy(false);
  }, []);

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
          <div className="setup-logo setup-logo--pulse">
            <SunsetzLogo size={44} />
          </div>
          <h1 className="setup-title">{tr("setup.title")}</h1>
          <p className="setup-subtitle">{tr("setup.subtitle")}</p>
        </div>

        <div className="setup-card">
          <div className="setup-card__head">
            <h2>{tr("setup.account.title")}</h2>
            <p>{tr("setup.account.hint")}</p>
          </div>

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
          </div>

          {accountBusy ? (
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
          ) : null}

          <div className="setup-actions setup-actions--footer">
            <button
              type="button"
              className="btn btn--ghost"
              disabled={accountBusy}
              onClick={skipAccount}
            >
              {tr("setup.account.skip")}
            </button>
          </div>

          {error ? (
            <div className="setup-error" role="alert">
              <strong>{tr("setup.error")}</strong>
              <span>{error}</span>
            </div>
          ) : null}
        </div>
      </div>
    </div>
  );
}
