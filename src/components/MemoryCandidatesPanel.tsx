import { useCallback, useEffect, useMemo, useState } from "react";
import * as api from "@/lib/api";
import { createT, type Locale } from "@/i18n";

export interface MemoryCandidatesPanelProps {
  locale: Locale;
  source: { sessionId: string; messageId: string } | null;
}

const MEMORY_TYPE_LABELS = {
  user_preference: "settings.memory.type.preference",
  project_fact: "settings.memory.type.projectFact",
  workflow_hint: "settings.memory.type.workflowHint",
} as const;

const MEMORY_STATUS_LABELS = {
  pending: "settings.memory.status.pending",
  approved: "settings.memory.status.approved",
  rejected: "settings.memory.status.rejected",
  superseded: "settings.memory.status.superseded",
} as const;

export function MemoryCandidatesPanel({
  locale,
  source,
}: MemoryCandidatesPanelProps) {
  const t = useMemo(() => createT(locale), [locale]);
  const [candidates, setCandidates] = useState<api.MemoryCandidateV1[]>([]);
  const [candidateType, setCandidateType] =
    useState<api.MemoryCandidateTypeV1>("user_preference");
  const [content, setContent] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (!api.isTauri()) {
      setCandidates([]);
      return;
    }
    try {
      setCandidates(await api.memoryCandidatesListV1());
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const createPending = async () => {
    const next = content.trim();
    if (!source || !next || busy) return;
    setBusy("create");
    setError(null);
    try {
      await api.memoryCandidateCreateV1({
        type: candidateType,
        content: next,
        source,
      });
      setContent("");
      await refresh();
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(null);
    }
  };

  const mutate = async (
    candidate: api.MemoryCandidateV1,
    action: "approve" | "reject" | "supersede" | "delete",
  ) => {
    if (busy) return;
    setBusy(`${action}:${candidate.id}`);
    setError(null);
    const request = {
      id: candidate.id,
      expectedContentHash: candidate.contentHash,
    };
    try {
      if (action === "approve") await api.memoryCandidateApproveV1(request);
      if (action === "reject") await api.memoryCandidateRejectV1(request);
      if (action === "supersede")
        await api.memoryCandidateSupersedeV1(request);
      if (action === "delete") await api.memoryCandidateDeleteV1(request);
      await refresh();
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(null);
    }
  };

  return (
    <section aria-labelledby="memory-candidates-title">
      <h2 className="settings-page__h2" id="memory-candidates-title">
        {t("settings.memory.title")}
      </h2>
      <div className="settings-card">
        <div className="settings-row settings-row--stack">
          <div className="settings-row__text">
            <div className="settings-row__label">
              {t("settings.memory.reviewOnly")}
            </div>
            <div className="settings-row__desc">
              {t("settings.memory.desc")}
            </div>
          </div>
          <select
            className="settings-input"
            value={candidateType}
            onChange={(event) =>
              setCandidateType(event.target.value as api.MemoryCandidateTypeV1)
            }
            aria-label={t("settings.memory.type")}
          >
            <option value="user_preference">
              {t("settings.memory.type.preference")}
            </option>
            <option value="project_fact">
              {t("settings.memory.type.projectFact")}
            </option>
            <option value="workflow_hint">
              {t("settings.memory.type.workflowHint")}
            </option>
          </select>
          <textarea
            className="settings-input"
            value={content}
            maxLength={2_000}
            rows={3}
            onChange={(event) => setContent(event.target.value)}
            placeholder={t("settings.memory.placeholder")}
            aria-label={t("settings.memory.content")}
          />
          <div className="settings-row__hint">
            {source
              ? t("settings.memory.sourceReady")
              : t("settings.memory.sourceRequired")}
          </div>
          <button
            type="button"
            className="btn btn--primary btn--sm"
            disabled={!source || !content.trim() || busy !== null}
            onClick={() => void createPending()}
          >
            {busy === "create"
              ? t("settings.memory.saving")
              : t("settings.memory.createCandidate")}
          </button>
          {error ? <div className="settings-row__hint">{error}</div> : null}
        </div>

        {candidates.length === 0 ? (
          <div className="settings-row__desc">
            {t("settings.memory.empty")}
          </div>
        ) : (
          candidates.map((candidate) => (
            <article
              className="settings-row settings-row--stack"
              key={candidate.id}
            >
              <div className="settings-row__text">
                <div className="settings-row__label">
                  {t(MEMORY_TYPE_LABELS[candidate.type])} ·{" "}
                  {t(MEMORY_STATUS_LABELS[candidate.status])}
                </div>
                <div className="settings-row__desc">{candidate.content}</div>
              </div>
              <div className="settings-row__actions">
                {candidate.status === "pending" ? (
                  <>
                    <button
                      type="button"
                      className="btn btn--primary btn--sm"
                      disabled={busy !== null}
                      onClick={() => void mutate(candidate, "approve")}
                    >
                      {t("settings.memory.approve")}
                    </button>
                    <button
                      type="button"
                      className="btn btn--ghost btn--sm"
                      disabled={busy !== null}
                      onClick={() => void mutate(candidate, "reject")}
                    >
                      {t("settings.memory.reject")}
                    </button>
                  </>
                ) : null}
                {candidate.status === "approved" ? (
                  <button
                    type="button"
                    className="btn btn--ghost btn--sm"
                    disabled={busy !== null}
                    onClick={() => void mutate(candidate, "supersede")}
                  >
                    {t("settings.memory.supersede")}
                  </button>
                ) : null}
                <button
                  type="button"
                  className="btn btn--ghost btn--sm btn--danger"
                  disabled={busy !== null}
                  onClick={() => void mutate(candidate, "delete")}
                >
                  {t("settings.memory.delete")}
                </button>
              </div>
            </article>
          ))
        )}
      </div>
    </section>
  );
}
