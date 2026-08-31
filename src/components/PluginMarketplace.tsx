import { useEffect, useMemo, useState } from "react";
import * as api from "@/lib/api";
import { createT, type Locale, type MessageKey } from "@/i18n";
import {
  CONNECTOR_CATALOG,
  connectorById,
  connectorConnectsInApp,
  connectorUsesGoogleSignIn,
  connectorUsesTokenPaste,
  featuredConnectors,
  productivityConnectors,
  type ConnectorAudience,
  type ConnectorCatalogEntry,
} from "@/lib/connectorCatalog";
import { ConnectorLogo } from "@/components/ConnectorLogo";
import { GlassModal } from "@/components/GlassModal";
import { IconArrowLeft, IconCheck } from "@/components/icons";
import {
  Button,
  EmptyState,
  SearchField,
  SegmentedControl,
  Skeleton,
} from "@/shared/ui";
import "@/styles/plugins.css";

export type PluginSkillRow = {
  id: string;
  name: string;
  description: string;
};

const GITHUB_TOKEN_URL = "https://github.com/settings/tokens";
const NOTION_TOKEN_URL = "https://www.notion.so/my-integrations";
const SLACK_TOKEN_URL = "https://api.slack.com/apps";

function credentialDialogCopy(id: string): {
  titleKey: MessageKey;
  bodyKey: MessageKey;
  createKey: MessageKey;
  placeholderKey: MessageKey;
  createUrl: string;
} {
  if (id === "notion") {
    return {
      titleKey: "plugin.credential.notion.title",
      bodyKey: "plugin.credential.notion.body",
      createKey: "plugin.credential.notion.create",
      placeholderKey: "plugin.credential.notion.placeholder",
      createUrl: NOTION_TOKEN_URL,
    };
  }
  if (id === "slack") {
    return {
      titleKey: "plugin.credential.slack.title",
      bodyKey: "plugin.credential.slack.body",
      createKey: "plugin.credential.slack.create",
      placeholderKey: "plugin.credential.slack.placeholder",
      createUrl: SLACK_TOKEN_URL,
    };
  }
  return {
    titleKey: "plugin.credential.title",
    bodyKey: "plugin.credential.body",
    createKey: "plugin.credential.create",
    placeholderKey: "plugin.credential.placeholder",
    createUrl: GITHUB_TOKEN_URL,
  };
}

function detailConnectHint(entry: ConnectorCatalogEntry): MessageKey | null {
  if (connectorUsesGoogleSignIn(entry)) return "plugin.google.connectHint";
  if (entry.id === "notion") return "plugin.notion.connectHint";
  if (entry.id === "slack") return "plugin.slack.connectHint";
  return null;
}

function mapConnectorError(raw: string, tr: (key: string) => string): string {
  if (raw.includes("CONNECTOR_CREDENTIAL_MISSING")) return tr("plugin.credentialMissing");
  if (raw.includes("CONNECTOR_OAUTH_CLIENT_MISSING")) return tr("plugin.oauthClientMissing");
  if (raw.includes("CONNECTOR_UNREACHABLE")) return tr("plugin.unreachable");
  if (raw.includes("CONNECTOR_AUTH_FAILED")) return tr("plugin.authFailed");
  if (raw.includes("CONNECTOR_PROBE_FAILED")) return tr("plugin.probeFailed");
  if (raw.includes("CONNECTOR_RUNTIME_REJECTED")) return tr("plugin.runtimeRejected");
  if (raw.includes("CONNECTOR_RUNTIME_MISSING")) return tr("plugin.runtimeMissing");
  return raw;
}

export function PluginMarketplace({
  locale,
  skills = [],
  onUsePrompt,
  onConnectorsChange,
}: {
  locale: Locale;
  skills?: PluginSkillRow[];
  onUsePrompt: (text: string) => void;
  onConnectorsChange?: (states: api.ConnectorStateV1[]) => void;
}) {
  const catalogT = useMemo(() => createT(locale), [locale]);
  const tr = (key: string) => catalogT(key as MessageKey);
  const [tab, setTab] = useState<"plugins" | "skills">("plugins");
  const [audience, setAudience] = useState<ConnectorAudience>("public");
  const [query, setQuery] = useState("");
  const [detailId, setDetailId] = useState<string | null>(null);
  const [states, setStates] = useState<api.ConnectorStateV1[]>([]);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(() => api.isTauri());
  const [credentialId, setCredentialId] = useState<string | null>(null);
  const [credentialValue, setCredentialValue] = useState("");

  useEffect(() => {
    if (!api.isTauri()) return;
    void api
      .connectorsList()
      .then((rows) => {
        setStates(rows);
        onConnectorsChange?.(rows);
      })
      .catch((err) => setError(mapConnectorError(String(err), tr)))
      .finally(() => setLoading(false));
  }, []);

  const stateById = useMemo(
    () => new Map(states.map((row) => [row.id, row])),
    [states],
  );
  const connected = CONNECTOR_CATALOG.filter(
    (entry) => stateById.get(entry.id)?.connected,
  );
  const hay = query.trim().toLowerCase();
  const matches = (entry: ConnectorCatalogEntry) => {
    if (entry.audience !== audience) return false;
    if (!hay) return true;
    return [
      tr(entry.nameKey),
      tr(entry.descriptionKey),
      entry.developer,
    ]
      .join(" ")
      .toLowerCase()
      .includes(hay);
  };
  const available = CONNECTOR_CATALOG.filter(
    (entry) => connectorConnectsInApp(entry) && matches(entry),
  );
  const featured = featuredConnectors().filter(
    (entry) => !connectorConnectsInApp(entry) && matches(entry),
  );
  const productivity = productivityConnectors().filter(
    (entry) => !connectorConnectsInApp(entry) && matches(entry),
  );
  const detail = detailId ? connectorById(detailId) : null;

  const applyState = (next: api.ConnectorStateV1) => {
    setStates((prev) => {
      const rows = [...prev.filter((row) => row.id !== next.id), next];
      onConnectorsChange?.(rows);
      return rows;
    });
  };

  const runConnect = async (
    id: string,
    connected: boolean,
    credential?: string,
  ) => {
    const entry = connectorById(id);
    if (!connected && (!entry || !connectorConnectsInApp(entry))) {
      return;
    }
    if (!api.isTauri()) {
      setError(tr("plugin.runtimeMissing"));
      return;
    }
    setBusyId(id);
    setError(null);
    try {
      const next = connected
        ? await api.connectorsDisconnect(id)
        : await api.connectorsConnect(id, credential);
      applyState(next);
      setCredentialId(null);
      setCredentialValue("");
    } catch (err) {
      const message = String(err);
      if (!connected && message.includes("CONNECTOR_CREDENTIAL_MISSING")) {
        setCredentialId(id);
        setError(null);
      } else {
        setError(mapConnectorError(message, tr));
      }
    } finally {
      setBusyId(null);
    }
  };

  const startConnect = (entry: ConnectorCatalogEntry, connectedNow: boolean) => {
    if (connectedNow) {
      void runConnect(entry.id, true);
      return;
    }
    if (!connectorConnectsInApp(entry)) return;
    if (connectorUsesTokenPaste(entry)) {
      setCredentialId(entry.id);
      setCredentialValue("");
      setError(null);
      return;
    }
    void runConnect(entry.id, false);
  };

  const credentialCopy = credentialDialogCopy(credentialId ?? "github");
  const credentialDialog = (
    <GlassModal
      open={credentialId != null}
      onClose={() => {
        setCredentialId(null);
        setCredentialValue("");
      }}
      title={tr(credentialCopy.titleKey)}
      closeLabel={tr("plugin.credential.cancel")}
      footer={
        <>
          <Button
            variant="secondary"
            onClick={() => {
              setCredentialId(null);
              setCredentialValue("");
            }}
          >
            {tr("plugin.credential.cancel")}
          </Button>
          <Button
            loading={busyId === credentialId}
            disabled={!credentialValue.trim()}
            onClick={() => {
              if (!credentialId) return;
              void runConnect(credentialId, false, credentialValue.trim());
            }}
          >
            {tr("plugin.credential.submit")}
          </Button>
        </>
      }
    >
      <p>{tr(credentialCopy.bodyKey)}</p>
      <p>
        <a href={credentialCopy.createUrl} target="_blank" rel="noreferrer">
          {tr(credentialCopy.createKey)}
        </a>
      </p>
      {error ? (
        <div className="plugin-market__error" role="status">
          {error}
        </div>
      ) : null}
      <input
        className="plugin-credential-input"
        type="password"
        autoComplete="off"
        placeholder={tr(credentialCopy.placeholderKey)}
        value={credentialValue}
        onChange={(event) => setCredentialValue(event.target.value)}
      />
    </GlassModal>
  );

  if (detail) {
    const connectedNow = !!stateById.get(detail.id)?.connected;
    const hintKey = detailConnectHint(detail);
    return (
      <div className="plugin-market" data-testid="plugin-marketplace">
        <button
          type="button"
          className="plugin-market__crumb"
          onClick={() => setDetailId(null)}
        >
          <IconArrowLeft size={16} />
          {tr("plugin.market.title")}
          <span aria-hidden>/</span>
          <strong>{tr(detail.nameKey)}</strong>
        </button>
        <header className="plugin-detail__head">
          <ConnectorLogo connector={detail} size={56} />
          <div className="plugin-detail__identity">
            <h1>{tr(detail.nameKey)}</h1>
            <p>{tr(detail.descriptionKey)}</p>
          </div>
          <div className="plugin-detail__actions">
            <button
              type="button"
              className="plugin-chip-btn"
              onClick={() => {
                void navigator.clipboard.writeText(detail.website);
              }}
            >
              {tr("plugin.market.copyLink")}
            </button>
            {connectedNow || connectorConnectsInApp(detail) ? (
              <Button
                variant={connectedNow ? "secondary" : "default"}
                loading={busyId === detail.id}
                onClick={() => startConnect(detail, connectedNow)}
              >
                {connectedNow
                  ? tr("plugin.market.disconnect")
                  : tr("plugin.market.connect")}
              </Button>
            ) : (
              <span className="plugin-card__coming">
                {tr("plugin.comingSoon")}
              </span>
            )}
          </div>
        </header>
        {error ? (
          <div className="plugin-market__error" role="status">
            {error}
          </div>
        ) : null}
        {!connectedNow && hintKey ? (
          <p className="plugin-detail__hint">{tr(hintKey)}</p>
        ) : null}
        <div className="plugin-detail__prompts">
          {detail.prompts.map((prompt) => (
            <button
              key={prompt.id}
              type="button"
              className="plugin-prompt"
              onClick={() => onUsePrompt(prompt.text)}
            >
              <ConnectorLogo connector={detail} size={18} />
              <span>
                <strong>{tr(detail.nameKey)}</strong> {prompt.text}
              </span>
            </button>
          ))}
        </div>
        <p className="plugin-detail__about">{tr(detail.aboutKey)}</p>
        <h2>{tr("plugin.market.info")}</h2>
        <dl className="plugin-detail__meta">
          <div>
            <dt>{tr("plugin.market.functions")}</dt>
            <dd>{detail.capabilities}</dd>
          </div>
          <div>
            <dt>{tr("plugin.market.developer")}</dt>
            <dd>{detail.developer}</dd>
          </div>
          <div>
            <dt>{tr("plugin.market.category")}</dt>
            <dd>
              {detail.category === "featured"
                ? tr("plugin.market.featured")
                : tr("plugin.market.productivity")}
            </dd>
          </div>
          <div>
            <dt>{tr("plugin.market.version")}</dt>
            <dd>{detail.version}</dd>
          </div>
          <div>
            <dt>{tr("plugin.market.website")}</dt>
            <dd>
              <a href={detail.website} target="_blank" rel="noreferrer">
                {detail.website}
              </a>
            </dd>
          </div>
          <div>
            <dt>{tr("plugin.market.privacy")}</dt>
            <dd>
              <a href={detail.privacy} target="_blank" rel="noreferrer">
                {detail.privacy}
              </a>
            </dd>
          </div>
          <div>
            <dt>{tr("plugin.market.terms")}</dt>
            <dd>
              <a href={detail.terms} target="_blank" rel="noreferrer">
                {detail.terms}
              </a>
            </dd>
          </div>
        </dl>
        {credentialDialog}
      </div>
    );
  }

  return (
    <div className="plugin-market" data-testid="plugin-marketplace">
      <SegmentedControl
        ariaLabel={tr("plugin.market.title")}
        value={tab}
        onChange={setTab}
        options={[
          { value: "plugins", label: tr("plugin.tab.plugins") },
          { value: "skills", label: tr("plugin.tab.skills") },
        ]}
      />
      {tab === "skills" ? (
        <div className="plugin-skills">
          <h1>{tr("plugin.tab.skills")}</h1>
          {skills.length === 0 ? (
            <EmptyState title={tr("plugin.skills.empty")} />
          ) : (
            <ul className="plugin-skills__list">
              {skills.map((skill) => (
                <li key={skill.id}>
                  <strong>{skill.name}</strong>
                  <span>{skill.description}</span>
                </li>
              ))}
            </ul>
          )}
        </div>
      ) : (
        <>
          <header className="plugin-market__hero">
            <h1>{tr("plugin.market.title")}</h1>
            <p>{tr("plugin.market.subtitle")}</p>
            <SearchField
              label={tr("plugin.market.search")}
              visuallyHideLabel
              value={query}
              onChange={setQuery}
            />
          </header>
          {connected.length > 0 ? (
            <section className="plugin-market__installed">
              <h2>{tr("plugin.market.installed")}</h2>
              <div className="plugin-market__installed-row">
                {connected.map((entry) => (
                  <button
                    key={entry.id}
                    type="button"
                    className="plugin-market__installed-logo"
                    onClick={() => setDetailId(entry.id)}
                    aria-label={tr(entry.nameKey)}
                  >
                    <ConnectorLogo connector={entry} size={44} />
                  </button>
                ))}
              </div>
            </section>
          ) : null}
          <SegmentedControl
            ariaLabel={tr("plugin.market.public")}
            value={audience}
            onChange={setAudience}
            options={[
              { value: "public", label: tr("plugin.market.public") },
              { value: "personal", label: tr("plugin.market.personal") },
            ]}
          />
          {error ? (
            <div className="plugin-market__error" role="status">
              {error}
            </div>
          ) : null}
          {loading ? (
            <div aria-busy="true" aria-label={tr("plugin.market.title")}>
              <Skeleton lines={4} media />
            </div>
          ) : null}
          {!loading &&
          audience === "personal" &&
          available.length === 0 &&
          featured.length === 0 &&
          productivity.length === 0 ? (
            <EmptyState title={tr("plugin.skills.empty")} />
          ) : null}
          {available.length > 0 ? (
            <section>
              <h2>{tr("plugin.market.available")}</h2>
              <div className="plugin-grid">
                {available.map((entry) => (
                  <ConnectorCard
                    key={entry.id}
                    entry={entry}
                    connected={!!stateById.get(entry.id)?.connected}
                    busy={busyId === entry.id}
                    tr={tr}
                    onOpen={() => setDetailId(entry.id)}
                    onConnect={() =>
                      startConnect(
                        entry,
                        !!stateById.get(entry.id)?.connected,
                      )
                    }
                  />
                ))}
              </div>
            </section>
          ) : null}
          <section>
            <h2>{tr("plugin.market.featured")}</h2>
            <div className="plugin-grid">
              {featured.map((entry) => (
                <ConnectorCard
                  key={entry.id}
                  entry={entry}
                  connected={!!stateById.get(entry.id)?.connected}
                  busy={busyId === entry.id}
                  tr={tr}
                  onOpen={() => setDetailId(entry.id)}
                  onConnect={() =>
                    startConnect(
                      entry,
                      !!stateById.get(entry.id)?.connected,
                    )
                  }
                />
              ))}
            </div>
          </section>
          <section>
            <h2>{tr("plugin.market.productivity")}</h2>
            <div className="plugin-grid">
              {productivity.map((entry) => (
                <ConnectorCard
                  key={entry.id}
                  entry={entry}
                  connected={!!stateById.get(entry.id)?.connected}
                  busy={busyId === entry.id}
                  tr={tr}
                  onOpen={() => setDetailId(entry.id)}
                  onConnect={() =>
                    startConnect(
                      entry,
                      !!stateById.get(entry.id)?.connected,
                    )
                  }
                />
              ))}
            </div>
          </section>
          <p className="plugin-market__hint">{tr("plugin.githubPatHint")}</p>
        </>
      )}
      {credentialDialog}
    </div>
  );
}

function ConnectorCard({
  entry,
  connected,
  busy,
  tr,
  onOpen,
  onConnect,
}: {
  entry: ConnectorCatalogEntry;
  connected: boolean;
  busy: boolean;
  tr: (key: string) => string;
  onOpen: () => void;
  onConnect: () => void;
}) {
  return (
    <article className="plugin-card">
      <button type="button" className="plugin-card__main" onClick={onOpen}>
        <ConnectorLogo connector={entry} size={36} />
        <span>
          <strong>{tr(entry.nameKey)}</strong>
          <small>{tr(entry.descriptionKey)}</small>
        </span>
      </button>
      {connected ? (
        <span className="plugin-card__status">
          <IconCheck size={14} />
        </span>
      ) : connectorConnectsInApp(entry) ? (
        <Button
          variant="secondary"
          size="sm"
          loading={busy}
          onClick={onConnect}
        >
          {tr("plugin.market.connect")}
        </Button>
      ) : (
        <span className="plugin-card__coming">{tr("plugin.comingSoon")}</span>
      )}
    </article>
  );
}
