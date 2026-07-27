import { useState } from "react";
import { createRoot } from "react-dom/client";
import {
  SettingsPage,
  type SettingsSectionId,
} from "@/components/SettingsPage";
import "@/styles/tokens.css";
import "@/styles/tailwind.css";
import "@/styles/app.css";
import "@/styles/apple.css";

const noop = () => {};

function SettingsFixture() {
  const [section, setSection] = useState<SettingsSectionId>("general");
  const [locale, setLocale] = useState("zh");
  const [theme, setTheme] = useState<"light" | "dark" | "high-contrast">(
    "dark",
  );
  const [sessionDataMode, setSessionDataMode] = useState("independent");
  const [policy, setPolicy] = useState("ask");
  const [prefsScope, setPrefsScope] = useState("global");

  return (
    <div className="app-shell platform-mac">
      <SettingsPage
        section={section}
        onSection={setSection}
        onBack={noop}
        labels={{}}
        locale={locale}
        onLocale={setLocale}
        theme={theme}
        onTheme={setTheme}
        sessionDataMode={sessionDataMode}
        onSessionDataMode={setSessionDataMode}
        policy={policy}
        onPolicy={setPolicy}
        prefsScope={prefsScope}
        onPrefsScope={setPrefsScope}
        availableModels={[
          {
            id: "sunsetz-4.5",
            label: "Sunsetz 4.5",
            source: "runtime",
          },
        ]}
        manualCliPath="/usr/local/bin/sunsetz"
        onManualCliPath={noop}
        onCliBlur={noop}
        acpServerAddr=""
        onAcpServerAddr={noop}
        maxConcurrentAgents={3}
        onMaxConcurrentAgents={noop}
        agentIdleMinutes={30}
        onAgentIdleMinutes={noop}
        streamStallSeconds={120}
        onStreamStallSeconds={noop}
        storeApiKeysInKeychain
        onStoreApiKeysInKeychain={noop}
        cliInfo={{
          found: true,
          path: "/usr/local/bin/sunsetz",
          version: "1.0.0",
          source: "visual",
          cliAuthPresent: true,
        }}
        onDoctor={noop}
        versionFooter="Sunsetz v1.0.0 · MIT"
        account={null}
        accountLoading={false}
        accountBusy={false}
        onAccountLoginOauth={noop}
        onAccountLoginDevice={noop}
        onCancelLogin={noop}
        onAccountLogout={noop}
        onAccountRefresh={noop}
        onAccountManageUsage={noop}
        onAccountSubscribe={noop}
        defaultOpenTarget="finder"
        onDefaultOpenTarget={noop}
        archivedGroups={[]}
      />
    </div>
  );
}

createRoot(document.getElementById("root")!).render(<SettingsFixture />);
