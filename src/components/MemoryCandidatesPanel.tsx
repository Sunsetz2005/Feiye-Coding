import { useCallback, useEffect, useMemo, useState } from "react";
import * as api from "@/lib/api";
import { createT, type Locale } from "@/i18n";

export interface MemoryCandidatesPanelProps {
  locale: Locale;
  source: { sessionId: string; messageId: string } | null;
  onUseContextPack?: (pack: api.MemoryContextPackV1) => void;
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

const MEMORY_INJECTION_STATUS_LABELS = {
  prepared: "settings.memory.injections.status.prepared",
  dispatching: "settings.memory.injections.status.dispatching",
  applied: "settings.memory.injections.status.applied",
  failed: "settings.memory.injections.status.failed",
  removed: "settings.memory.injections.status.removed",
} as const;

const MEMORY_INJECTION_FAILURE_LABELS = {
  runtime_write_failed: "settings.memory.injections.failure.runtimeWrite",
  interrupted: "settings.memory.injections.failure.interrupted",
  context_unavailable: "settings.memory.injections.failure.contextUnavailable",
} as const;

const MAX_MEMORY_INJECTION_AUDIT_ROWS = 128;

function formatInjectionTime(value: string, locale: Locale, fallback: string) {
  const date = new Date(value);
  if (!Number.isFinite(date.getTime())) return fallback;
  return new Intl.DateTimeFormat(locale, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(date);
}

function boundedAttempt(value: number) {
  if (!Number.isFinite(value)) return 1;
  return Math.min(Math.max(Math.trunc(value), 1), 4_294_967_295);
}

function canRemoveInjection(status: api.MemoryInjectionStatusV1) {
  return status === "prepared" || status === "applied" || status === "failed";
}

export function MemoryCandidatesPanel({
  locale,
  source,
  onUseContextPack,
}: MemoryCandidatesPanelProps) {
  const t = useMemo(() => createT(locale), [locale]);
  const [candidates, setCandidates] = useState<api.MemoryCandidateV1[]>([]);
  const [candidateType, setCandidateType] =
    useState<api.MemoryCandidateTypeV1>("user_preference");
  const [content, setContent] = useState("");
  const [selectedHashes, setSelectedHashes] = useState<Record<string, string>>(
    {},
  );
  const [contextPack, setContextPack] =
    useState<api.MemoryContextPackV1 | null>(null);
  const [recallQuery, setRecallQuery] = useState("");
  const [recallPreview, setRecallPreview] =
    useState<api.MemoryRecallPreviewV1 | null>(null);
  const [clearPlan, setClearPlan] =
    useState<api.MemoryClearPlanV1 | null>(null);
  const [injections, setInjections] = useState<api.MemoryInjectionRecordV1[]>(
    [],
  );
  const [injectionSessionId, setInjectionSessionId] = useState<string | null>(
    null,
  );
  const [injectionLoadingSessionId, setInjectionLoadingSessionId] = useState<
    string | null
  >(null);
  const [injectionError, setInjectionError] = useState<{
    sessionId: string;
    message: string;
  } | null>(null);
  const [injectionBusyKey, setInjectionBusyKey] = useState<string | null>(null);
  const [injectionRefreshKey, setInjectionRefreshKey] = useState(0);
  const [copied, setCopied] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (!api.isTauri()) {
      setCandidates([]);
      return;
    }
    try {
      const rows = await api.memoryCandidatesListV1();
      setCandidates(rows);
      const approvedHashes = new Map(
        rows
          .filter((candidate) => candidate.status === "approved")
          .map((candidate) => [candidate.id, candidate.contentHash]),
      );
      setSelectedHashes((current) =>
        Object.fromEntries(
          Object.entries(current).filter(
            ([id, hash]) => approvedHashes.get(id) === hash,
          ),
        ),
      );
      setContextPack(null);
      setClearPlan(null);
      setCopied(false);
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    const sessionId = source?.sessionId.trim() || null;
    let active = true;
    setInjections([]);
    setInjectionSessionId(null);
    setInjectionError(null);
    if (!sessionId || !api.isTauri()) {
      setInjectionLoadingSessionId(null);
      return () => {
        active = false;
      };
    }

    setInjectionLoadingSessionId(sessionId);
    void api
      .memoryInjectionsListV1(sessionId)
      .then((rows) => {
        if (!active) return;
        setInjections(
          rows
            .filter((record) => record.sessionId === sessionId)
            .slice(0, MAX_MEMORY_INJECTION_AUDIT_ROWS),
        );
        setInjectionSessionId(sessionId);
      })
      .catch((reason) => {
        if (!active) return;
        setInjectionError({ sessionId, message: String(reason) });
        setInjectionSessionId(sessionId);
      })
      .finally(() => {
        if (active) setInjectionLoadingSessionId(null);
      });

    return () => {
      active = false;
    };
  }, [source?.sessionId, injectionRefreshKey]);

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
      if (action === "delete") {
        setClearPlan(
          await api.memoryClearPreviewV1({
            candidateSelections: [request],
            sourceSessionIds: [],
          }),
        );
        return;
      }
      if (action === "approve") await api.memoryCandidateApproveV1(request);
      if (action === "reject") await api.memoryCandidateRejectV1(request);
      if (action === "supersede")
        await api.memoryCandidateSupersedeV1(request);
      await refresh();
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(null);
    }
  };

  const selectedCandidates = useMemo(
    () =>
      candidates.filter(
        (candidate) =>
          candidate.status === "approved" &&
          selectedHashes[candidate.id] === candidate.contentHash,
      ),
    [candidates, selectedHashes],
  );

  const toggleContextCandidate = (
    candidate: api.MemoryCandidateV1,
    checked: boolean,
  ) => {
    setContextPack(null);
    setCopied(false);
    setSelectedHashes((current) => {
      const next = { ...current };
      if (checked) next[candidate.id] = candidate.contentHash;
      else delete next[candidate.id];
      return next;
    });
  };

  const buildContextPack = async () => {
    if (busy || selectedCandidates.length === 0) return;
    setBusy("context-pack");
    setError(null);
    setCopied(false);
    try {
      setContextPack(
        await api.memoryContextPackBuildV1(
          selectedCandidates.map((candidate) => ({
            id: candidate.id,
            expectedContentHash: candidate.contentHash,
          })),
        ),
      );
    } catch (reason) {
      setContextPack(null);
      setError(String(reason));
    } finally {
      setBusy(null);
    }
  };

  const contextPackJson = contextPack
    ? JSON.stringify(contextPack, null, 2)
    : "";

  const copyContextPack = async () => {
    if (!contextPackJson) return;
    try {
      await navigator.clipboard.writeText(contextPackJson);
      setCopied(true);
      setError(null);
    } catch (reason) {
      setCopied(false);
      setError(String(reason));
    }
  };

  const runRecallPreview = async () => {
    const query = recallQuery.trim();
    if (!query || busy) return;
    setBusy("recall");
    setError(null);
    try {
      setRecallPreview(
        await api.memoryRecallPreviewV1(query, source?.sessionId ?? null),
      );
    } catch (reason) {
      setRecallPreview(null);
      setError(String(reason));
    } finally {
      setBusy(null);
    }
  };

  const useReviewedRecallMatches = async () => {
    const selections = recallPreview?.contextPackSelections ?? [];
    if (!selections.length || busy) return;
    setBusy("recall-pack");
    setError(null);
    try {
      const pack = await api.memoryContextPackBuildV1(selections);
      setSelectedHashes(
        Object.fromEntries(
          selections.map((selection) => [
            selection.id,
            selection.expectedContentHash,
          ]),
        ),
      );
      setContextPack(pack);
      setCopied(false);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(null);
    }
  };

  const exportReviewedMemory = async () => {
    if (busy) return;
    setBusy("export");
    setError(null);
    try {
      const exported = await api.memoryExportV1(true);
      const blob = new Blob([JSON.stringify(exported, null, 2)], {
        type: "application/json;charset=utf-8",
      });
      const url = URL.createObjectURL(blob);
      const anchor = document.createElement("a");
      anchor.href = url;
      anchor.download = "sunsetz-memory-v1.json";
      anchor.click();
      URL.revokeObjectURL(url);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(null);
    }
  };

  const confirmClear = async () => {
    if (!clearPlan || busy) return;
    setBusy("clear-confirm");
    setError(null);
    try {
      await api.memoryClearConfirmV1(clearPlan);
      setClearPlan(null);
      setInjectionRefreshKey((value) => value + 1);
      await refresh();
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(null);
    }
  };

  const recordInjectionFeedback = async (
    record: api.MemoryInjectionRecordV1,
    feedback: api.MemoryInjectionFeedbackV1,
  ) => {
    if (injectionBusyKey || record.status !== "applied" || record.feedback) {
      return;
    }
    setInjectionBusyKey(`feedback:${feedback}:${record.injectionId}`);
    setInjectionError(null);
    try {
      await api.memoryInjectionFeedbackV1(record, feedback);
      setInjectionRefreshKey((value) => value + 1);
    } catch (reason) {
      setInjectionError({
        sessionId: record.sessionId,
        message: String(reason),
      });
    } finally {
      setInjectionBusyKey(null);
    }
  };

  const removeInjection = async (record: api.MemoryInjectionRecordV1) => {
    if (injectionBusyKey || !canRemoveInjection(record.status)) return;
    setInjectionBusyKey(`remove:${record.injectionId}`);
    setInjectionError(null);
    try {
      await api.memoryInjectionRemoveV1(record);
      setInjectionRefreshKey((value) => value + 1);
    } catch (reason) {
      setInjectionError({
        sessionId: record.sessionId,
        message: String(reason),
      });
    } finally {
      setInjectionBusyKey(null);
    }
  };

  const activeInjectionSessionId = source?.sessionId.trim() || null;
  const visibleInjections =
    injectionSessionId === activeInjectionSessionId ? injections : [];
  const visibleInjectionError =
    injectionError?.sessionId === activeInjectionSessionId
      ? injectionError.message
      : null;
  const injectionsLoading =
    injectionLoadingSessionId === activeInjectionSessionId;

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

        <article className="settings-row settings-row--stack">
          <div className="settings-row__text">
            <div className="settings-row__label">
              {t("settings.memory.recallTitle")}
            </div>
            <div className="settings-row__desc">
              {t("settings.memory.recallDesc")}
            </div>
          </div>
          <input
            className="settings-input"
            value={recallQuery}
            maxLength={1_000}
            onChange={(event) => setRecallQuery(event.target.value)}
            placeholder={t("settings.memory.recallPlaceholder")}
            aria-label={t("settings.memory.recallQuery")}
          />
          <div className="settings-row__actions">
            <button
              type="button"
              className="btn btn--primary btn--sm"
              disabled={busy !== null || !recallQuery.trim()}
              onClick={() => void runRecallPreview()}
            >
              {busy === "recall"
                ? t("settings.memory.recalling")
                : t("settings.memory.recall")}
            </button>
            <button
              type="button"
              className="btn btn--ghost btn--sm"
              disabled={busy !== null}
              onClick={() => void exportReviewedMemory()}
            >
              {busy === "export"
                ? t("settings.memory.exporting")
                : t("settings.memory.export")}
            </button>
          </div>
          {recallPreview ? (
            <div aria-label={t("settings.memory.recallResults")}>
              <div className="settings-row__label">
                {t("settings.memory.reviewedMatches", {
                  n: recallPreview.memoryCandidates.length,
                })}
              </div>
              {recallPreview.memoryCandidates.map((candidate) => (
                <div
                  className="settings-row__desc"
                  key={`${candidate.candidateId}:${candidate.contentHash}`}
                >
                  {candidate.content}
                </div>
              ))}
              <button
                type="button"
                className="btn btn--ghost btn--sm"
                disabled={
                  busy !== null ||
                  recallPreview.contextPackSelections.length === 0
                }
                onClick={() => void useReviewedRecallMatches()}
              >
                {t("settings.memory.useReviewedMatches")}
              </button>
              <div className="settings-row__label">
                {t("settings.memory.evidenceMatches", {
                  n: recallPreview.sessionEvidence.length,
                })}
              </div>
              <div className="settings-row__hint">
                {t("settings.memory.evidenceWarning")}
              </div>
              {recallPreview.sessionEvidence.map((evidence) => (
                <div
                  className="settings-row__desc"
                  key={`${evidence.sessionId}:${evidence.messageId}`}
                >
                  {evidence.sessionTitle}: {evidence.snippet}
                </div>
              ))}
            </div>
          ) : null}
        </article>

        <article
          className="settings-row settings-row--stack"
          aria-label={t("settings.memory.injections.title")}
        >
          <div className="settings-row__text">
            <div className="settings-row__label">
              {t("settings.memory.injections.title")}
            </div>
            <div className="settings-row__desc">
              {t("settings.memory.injections.desc")}
            </div>
          </div>
          {!activeInjectionSessionId ? (
            <div className="settings-row__hint">
              {t("settings.memory.injections.noSession")}
            </div>
          ) : injectionsLoading ? (
            <div className="settings-row__hint">
              {t("settings.memory.injections.loading")}
            </div>
          ) : visibleInjections.length === 0 ? (
            <div className="settings-row__hint">
              {t("settings.memory.injections.empty")}
            </div>
          ) : (
            <ul className="ext-list">
              {visibleInjections.map((record) => {
                const selectionCount = Math.min(
                  Math.max(record.selections.length, 0),
                  8,
                );
                const feedbackBusy = injectionBusyKey?.startsWith(
                  "feedback:",
                );
                const removeBusy =
                  injectionBusyKey === `remove:${record.injectionId}`;
                return (
                  <li className="ext-item" key={record.injectionId}>
                    <div className="ext-item__head">
                      <strong className="ext-item__name">
                        {t(MEMORY_INJECTION_STATUS_LABELS[record.status])}
                      </strong>
                    </div>
                    <div className="ext-item__meta">
                      <span>
                        {t("settings.memory.injections.selectionCount", {
                          n: selectionCount,
                        })}
                      </span>
                      <span>
                        {t("settings.memory.injections.attempt", {
                          n: boundedAttempt(record.attempt),
                        })}
                      </span>
                      <span>
                        {t("settings.memory.injections.updatedAt")}:{" "}
                        <time dateTime={record.updatedAt}>
                          {formatInjectionTime(
                            record.updatedAt,
                            locale,
                            t("settings.memory.injections.timeUnavailable"),
                          )}
                        </time>
                      </span>
                    </div>
                    {record.failureCode ? (
                      <div className="settings-row__hint">
                        {t("settings.memory.injections.failure")}:{" "}
                        {t(MEMORY_INJECTION_FAILURE_LABELS[record.failureCode])}
                      </div>
                    ) : null}
                    {record.feedback ? (
                      <div className="settings-row__hint">
                        {t("settings.memory.injections.feedbackRecorded")}:{" "}
                        {record.feedback === "helpful"
                          ? t("settings.memory.injections.helpful")
                          : t("settings.memory.injections.unhelpful")}
                      </div>
                    ) : null}
                    <div className="ext-item__actions">
                      {record.status === "applied" && !record.feedback ? (
                        <>
                          <button
                            type="button"
                            className="btn btn--ghost btn--sm"
                            disabled={injectionBusyKey !== null}
                            onClick={() =>
                              void recordInjectionFeedback(record, "helpful")
                            }
                          >
                            {injectionBusyKey ===
                            `feedback:helpful:${record.injectionId}`
                              ? t("settings.memory.injections.savingFeedback")
                              : t("settings.memory.injections.helpful")}
                          </button>
                          <button
                            type="button"
                            className="btn btn--ghost btn--sm"
                            disabled={injectionBusyKey !== null}
                            onClick={() =>
                              void recordInjectionFeedback(record, "unhelpful")
                            }
                          >
                            {injectionBusyKey ===
                            `feedback:unhelpful:${record.injectionId}`
                              ? t("settings.memory.injections.savingFeedback")
                              : t("settings.memory.injections.unhelpful")}
                          </button>
                        </>
                      ) : null}
                      {canRemoveInjection(record.status) ? (
                        <button
                          type="button"
                          className="btn btn--ghost btn--sm btn--danger"
                          disabled={injectionBusyKey !== null || feedbackBusy}
                          onClick={() => void removeInjection(record)}
                        >
                          {removeBusy
                            ? t("settings.memory.injections.removing")
                            : t("settings.memory.injections.remove")}
                        </button>
                      ) : null}
                    </div>
                  </li>
                );
              })}
            </ul>
          )}
          {visibleInjectionError ? (
            <div className="settings-row__hint" role="alert">
              {visibleInjectionError}
            </div>
          ) : null}
        </article>

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
                <div className="settings-row__hint">
                  {t("settings.memory.provenance", {
                    sessionId: candidate.source.sessionId,
                    messageId: candidate.source.messageId,
                  })}
                </div>
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
                  <>
                    <label className="settings-row__hint">
                      <input
                        type="checkbox"
                        checked={selectedHashes[candidate.id] === candidate.contentHash}
                        disabled={
                          busy !== null ||
                          (selectedHashes[candidate.id] !== candidate.contentHash &&
                            selectedCandidates.length >= 8)
                        }
                        onChange={(event) =>
                          toggleContextCandidate(candidate, event.target.checked)
                        }
                      />{" "}
                      {t("settings.memory.selectForPack")}
                    </label>
                    <button
                      type="button"
                      className="btn btn--ghost btn--sm"
                      disabled={busy !== null}
                      onClick={() => void mutate(candidate, "supersede")}
                    >
                      {t("settings.memory.supersede")}
                    </button>
                  </>
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

        {clearPlan ? (
          <article
            className="settings-row settings-row--stack"
            aria-label={t("settings.memory.clearPreview")}
          >
            <div className="settings-row__text">
              <div className="settings-row__label">
                {t("settings.memory.clearTitle")}
              </div>
              <div className="settings-row__desc">
                {t("settings.memory.clearDesc", {
                  candidates: clearPlan.candidates.length,
                  injections: clearPlan.injections.length,
                })}
              </div>
              <div className="settings-row__hint">
                {t("settings.memory.clearIndexNote")}
              </div>
            </div>
            <div className="settings-row__actions">
              <button
                type="button"
                className="btn btn--danger btn--sm"
                disabled={busy !== null}
                onClick={() => void confirmClear()}
              >
                {busy === "clear-confirm"
                  ? t("settings.memory.clearing")
                  : t("settings.memory.confirmClear")}
              </button>
              <button
                type="button"
                className="btn btn--ghost btn--sm"
                disabled={busy !== null}
                onClick={() => setClearPlan(null)}
              >
                {t("common.cancel")}
              </button>
            </div>
          </article>
        ) : null}

        <article className="settings-row settings-row--stack">
          <div className="settings-row__text">
            <div className="settings-row__label">
              {t("settings.memory.packTitle")}
            </div>
            <div className="settings-row__desc">
              {t("settings.memory.packDesc")}
            </div>
            <div className="settings-row__hint">
              {t("settings.memory.selectedCount", {
                n: selectedCandidates.length,
              })}
            </div>
          </div>
          <div className="settings-row__actions">
            <button
              type="button"
              className="btn btn--primary btn--sm"
              disabled={busy !== null || selectedCandidates.length === 0}
              onClick={() => void buildContextPack()}
            >
              {busy === "context-pack"
                ? t("settings.memory.buildingPack")
                : t("settings.memory.buildPack")}
            </button>
          </div>
          {selectedCandidates.length === 0 ? (
            <div className="settings-row__hint">
              {t("settings.memory.selectApproved")}
            </div>
          ) : null}
          {contextPack ? (
            <>
              <textarea
                className="settings-input"
                value={contextPackJson}
                readOnly
                rows={8}
                aria-label={t("settings.memory.packPreview")}
              />
              <div className="settings-row__actions">
                {onUseContextPack ? (
                  <button
                    type="button"
                    className="btn btn--primary btn--sm"
                    onClick={() => onUseContextPack(contextPack)}
                  >
                    {t("settings.memory.useNextTurn")}
                  </button>
                ) : null}
                <button
                  type="button"
                  className="btn btn--ghost btn--sm"
                  onClick={() => void copyContextPack()}
                >
                  {t("settings.memory.copyPack")}
                </button>
                {copied ? (
                  <span className="settings-row__hint" aria-live="polite">
                    {t("settings.memory.copiedPack")}
                  </span>
                ) : null}
              </div>
            </>
          ) : null}
        </article>
      </div>
    </section>
  );
}
