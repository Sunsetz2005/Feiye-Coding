import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ChangeEvent,
} from "react";
import * as api from "@/lib/api";
import { createT, type Locale } from "@/i18n";

export interface SkillLearningPanelProps {
  locale: Locale;
  activeProjectPath?: string | null;
  activeSessionId?: string | null;
  disabled?: boolean;
}

const MAX_VISIBLE_SKILL_USES = 64;
const MAX_SKILL_QUERY_BYTES = 2_048;
const MAX_SKILL_RANK_RESULTS = 8;

const SKILL_USE_STATUS_LABELS = {
  prepared: "ext.skillLearning.status.prepared",
  dispatching: "ext.skillLearning.status.dispatching",
  applied: "ext.skillLearning.status.applied",
  succeeded: "ext.skillLearning.status.succeeded",
  failed: "ext.skillLearning.status.failed",
  interrupted: "ext.skillLearning.status.interrupted",
} as const;

const SKILL_SELECTION_LABELS = {
  explicit: "ext.skillLearning.selection.explicit",
  accepted_suggestion: "ext.skillLearning.selection.acceptedSuggestion",
} as const;

function isTerminal(status: api.SkillUseStatusV1) {
  return (
    status === "succeeded" ||
    status === "failed" ||
    status === "interrupted"
  );
}

function normalizeScopePart(value: string | null | undefined) {
  return value?.trim() || null;
}

function formatTime(value: string, locale: Locale, fallback: string) {
  const date = new Date(value);
  if (!Number.isFinite(date.getTime())) return fallback;
  return new Intl.DateTimeFormat(locale, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(date);
}

function utf8Length(value: string) {
  return new TextEncoder().encode(value).length;
}

function truncateUtf8(value: string, maxBytes: number) {
  const encoder = new TextEncoder();
  let bounded = "";
  let used = 0;
  for (const character of value) {
    const bytes = encoder.encode(character).length;
    if (used + bytes > maxBytes) break;
    bounded += character;
    used += bytes;
  }
  return bounded;
}

type ScopedError = { scopeKey: string; message: string };
type ScopedProposal = {
  scopeKey: string;
  proposal: api.SkillImprovementProposalV1 | null;
};
type ScopedRanking = {
  scopeKey: string;
  result: api.SkillMetadataRankingResultV1;
};

export function SkillLearningPanel({
  locale,
  activeProjectPath = null,
  activeSessionId = null,
  disabled = false,
}: SkillLearningPanelProps) {
  const t = useMemo(() => createT(locale), [locale]);
  const projectPath = normalizeScopePart(activeProjectPath);
  const sessionId = normalizeScopePart(activeSessionId);
  const scopeKey = `${sessionId ?? ""}\u0000${projectPath ?? ""}`;
  const scopeKeyRef = useRef(scopeKey);
  scopeKeyRef.current = scopeKey;

  const [uses, setUses] = useState<api.SkillUseRecordV1[]>([]);
  const [usesScopeKey, setUsesScopeKey] = useState<string | null>(null);
  const [usesLoadingScopeKey, setUsesLoadingScopeKey] = useState<string | null>(
    null,
  );
  const [operationError, setOperationError] = useState<ScopedError | null>(null);
  const [actionBusyKey, setActionBusyKey] = useState<string | null>(null);
  const [proposalResult, setProposalResult] = useState<ScopedProposal | null>(
    null,
  );
  const [query, setQuery] = useState("");
  const [queryWasTruncated, setQueryWasTruncated] = useState(false);
  const [ranking, setRanking] = useState<ScopedRanking | null>(null);
  const listRequestRef = useRef(0);

  const desktopAvailable = api.isTauri();
  const visibleUses = usesScopeKey === scopeKey ? uses : [];
  const visibleError =
    operationError?.scopeKey === scopeKey ? operationError.message : null;
  const visibleProposal =
    proposalResult?.scopeKey === scopeKey ? proposalResult : null;
  const visibleRanking = ranking?.scopeKey === scopeKey ? ranking.result : null;
  const usesLoading = usesLoadingScopeKey === scopeKey;

  const loadUses = useCallback(async () => {
    const requestId = ++listRequestRef.current;
    setUses([]);
    setUsesScopeKey(null);
    setUsesLoadingScopeKey(null);

    if (!sessionId || disabled || !desktopAvailable) {
      if (scopeKeyRef.current === scopeKey) {
        setUsesScopeKey(scopeKey);
      }
      return;
    }

    setUsesLoadingScopeKey(scopeKey);
    try {
      const records = await api.skillUsesListV1(sessionId);
      if (
        requestId !== listRequestRef.current ||
        scopeKeyRef.current !== scopeKey
      ) {
        return;
      }
      setUses(records.slice(0, MAX_VISIBLE_SKILL_USES));
      setUsesScopeKey(scopeKey);
    } catch (reason) {
      if (
        requestId !== listRequestRef.current ||
        scopeKeyRef.current !== scopeKey
      ) {
        return;
      }
      setUsesScopeKey(scopeKey);
      setOperationError({
        scopeKey,
        message: t("ext.skillLearning.hostError", {
          error: reason instanceof Error ? reason.message : String(reason),
        }),
      });
    } finally {
      if (
        requestId === listRequestRef.current &&
        scopeKeyRef.current === scopeKey
      ) {
        setUsesLoadingScopeKey(null);
      }
    }
  }, [desktopAvailable, disabled, scopeKey, sessionId, t]);

  useEffect(() => {
    setOperationError(null);
    setActionBusyKey(null);
    setProposalResult(null);
    setRanking(null);
    void loadUses();
    return () => {
      listRequestRef.current += 1;
    };
  }, [loadUses]);

  const recordFeedback = async (
    record: api.SkillUseRecordV1,
    rating: api.SkillUseFeedbackRatingV1,
  ) => {
    if (
      disabled ||
      !desktopAvailable ||
      actionBusyKey ||
      record.feedback ||
      !isTerminal(record.status) ||
      (rating === "helpful" && record.status !== "succeeded")
    ) {
      return;
    }

    const capturedScopeKey = scopeKey;
    const busyKey = `feedback:${rating}:${record.id}`;
    setActionBusyKey(busyKey);
    setOperationError(null);
    try {
      const updated = await api.skillUseFeedbackV1(record, rating);
      if (scopeKeyRef.current !== capturedScopeKey) return;
      setUses((current) =>
        current.map((item) => (item.id === updated.id ? updated : item)),
      );
      await loadUses();
    } catch (reason) {
      if (scopeKeyRef.current !== capturedScopeKey) return;
      setOperationError({
        scopeKey: capturedScopeKey,
        message: t("ext.skillLearning.hostError", {
          error: reason instanceof Error ? reason.message : String(reason),
        }),
      });
      await loadUses();
    } finally {
      if (scopeKeyRef.current === capturedScopeKey) {
        setActionBusyKey(null);
      }
    }
  };

  const buildProposal = async (record: api.SkillUseRecordV1) => {
    if (
      disabled ||
      !desktopAvailable ||
      actionBusyKey ||
      record.status !== "succeeded" ||
      !record.skill.sourceCandidateId?.trim()
    ) {
      return;
    }

    const capturedScopeKey = scopeKey;
    setActionBusyKey(`proposal:${record.id}`);
    setOperationError(null);
    setProposalResult(null);
    try {
      const proposal = await api.skillImprovementProposalV1(
        record.skill,
        projectPath,
      );
      if (scopeKeyRef.current !== capturedScopeKey) return;
      if (
        proposal &&
        (proposal.status !== "pending_review" ||
          proposal.owner.kind !== "host_candidate" ||
          proposal.skillId !== record.skill.id ||
          proposal.lineage.priorSkillTreeHash !== record.skill.treeHash ||
          proposal.lineage.sourceCandidateId !==
            record.skill.sourceCandidateId ||
          proposal.requiresUserReview !== true ||
          proposal.mayWriteSkill !== false ||
          proposal.owner.mayOverwriteExternal !== false)
      ) {
        setOperationError({
          scopeKey: capturedScopeKey,
          message: t("ext.skillLearning.proposalBoundaryRejected"),
        });
        return;
      }
      setProposalResult({
        scopeKey: capturedScopeKey,
        proposal,
      });
    } catch (reason) {
      if (scopeKeyRef.current !== capturedScopeKey) return;
      setOperationError({
        scopeKey: capturedScopeKey,
        message: t("ext.skillLearning.hostError", {
          error: reason instanceof Error ? reason.message : String(reason),
        }),
      });
    } finally {
      if (scopeKeyRef.current === capturedScopeKey) {
        setActionBusyKey(null);
      }
    }
  };

  const changeQuery = (event: ChangeEvent<HTMLInputElement>) => {
    const next = event.target.value;
    const bounded = truncateUtf8(next, MAX_SKILL_QUERY_BYTES);
    setQuery(bounded);
    setQueryWasTruncated(bounded !== next);
    setRanking(null);
  };

  const rankMetadata = async () => {
    const normalizedQuery = query.trim();
    if (
      disabled ||
      !desktopAvailable ||
      actionBusyKey ||
      normalizedQuery.length === 0
    ) {
      return;
    }

    const capturedScopeKey = scopeKey;
    setActionBusyKey("rank");
    setOperationError(null);
    setRanking(null);
    try {
      const result = await api.skillMetadataRankV1(
        normalizedQuery,
        projectPath,
        MAX_SKILL_RANK_RESULTS,
      );
      if (scopeKeyRef.current !== capturedScopeKey) return;
      if (
        result.disposition !== "suggestion_only" ||
        result.requiresExplicitAcceptance !== true
      ) {
        setOperationError({
          scopeKey: capturedScopeKey,
          message: t("ext.skillLearning.rankingBoundaryRejected"),
        });
        return;
      }
      setRanking({
        scopeKey: capturedScopeKey,
        result: {
          ...result,
          items: result.items.slice(0, MAX_SKILL_RANK_RESULTS),
        },
      });
    } catch (reason) {
      if (scopeKeyRef.current !== capturedScopeKey) return;
      setOperationError({
        scopeKey: capturedScopeKey,
        message: t("ext.skillLearning.hostError", {
          error: reason instanceof Error ? reason.message : String(reason),
        }),
      });
    } finally {
      if (scopeKeyRef.current === capturedScopeKey) {
        setActionBusyKey(null);
      }
    }
  };

  return (
    <section
      className="settings-card ext-card skill-learning-panel"
      aria-label={t("ext.skillLearning.region")}
      data-testid="skill-learning-panel"
    >
      <header>
        <h2>{t("ext.skillLearning.title")}</h2>
        <p>
          <strong>{t("ext.skillLearning.metadataOnly")}</strong>
        </p>
        <p>{t("ext.skillLearning.boundary")}</p>
      </header>

      <article
        className="settings-row settings-row--stack"
        aria-label={t("ext.skillLearning.suggestionsTitle")}
      >
        <div className="settings-row__text">
          <div className="settings-row__label">
            {t("ext.skillLearning.suggestionsTitle")}
          </div>
          <div className="settings-row__desc">
            {t("ext.skillLearning.suggestionsDesc")}
          </div>
        </div>
        <input
          className="settings-input"
          value={query}
          maxLength={MAX_SKILL_QUERY_BYTES}
          disabled={disabled || !desktopAvailable || actionBusyKey !== null}
          onChange={changeQuery}
          placeholder={t("ext.skillLearning.queryPlaceholder")}
          aria-label={t("ext.skillLearning.queryLabel")}
        />
        <div className="settings-row__hint">
          {t("ext.skillLearning.queryBytes", {
            bytes: utf8Length(query),
            maxBytes: MAX_SKILL_QUERY_BYTES,
          })}
        </div>
        {queryWasTruncated ? (
          <div className="settings-row__hint" role="status">
            {t("ext.skillLearning.queryLimited", {
              maxBytes: MAX_SKILL_QUERY_BYTES,
            })}
          </div>
        ) : null}
        <div className="settings-row__actions">
          <button
            type="button"
            className="btn btn--primary btn--sm"
            disabled={
              disabled ||
              !desktopAvailable ||
              actionBusyKey !== null ||
              query.trim().length === 0
            }
            onClick={() => void rankMetadata()}
          >
            {actionBusyKey === "rank"
              ? t("ext.skillLearning.ranking")
              : t("ext.skillLearning.rank")}
          </button>
        </div>
        {visibleRanking ? (
          <section aria-label={t("ext.skillLearning.suggestionsResult")}>
            <div className="settings-row__label">
              {t("ext.skillLearning.suggestionOnly")}
            </div>
            <div className="settings-row__hint">
              {t("ext.skillLearning.explicitAcceptance")}
            </div>
            {visibleRanking.items.length === 0 ? (
              <div className="settings-row__desc">
                {t("ext.skillLearning.noSuggestions")}
              </div>
            ) : (
              <ul className="ext-list">
                {visibleRanking.items.map((item) => (
                  <li
                    className="ext-item"
                    key={`${item.skill.id}:${item.skill.treeHash}`}
                  >
                    <div className="ext-item__head">
                      <strong className="ext-item__name">
                        {item.skill.name}
                      </strong>
                      <span className="ext-badge ext-badge--muted">
                        {t("ext.skillLearning.score", { score: item.score })}
                      </span>
                    </div>
                    {item.matchedTerms.length > 0 ? (
                      <div className="ext-item__meta">
                        <span>
                          {t("ext.skillLearning.matchedTerms", {
                            terms: item.matchedTerms.join(", "),
                          })}
                        </span>
                      </div>
                    ) : null}
                  </li>
                ))}
              </ul>
            )}
          </section>
        ) : null}
      </article>

      <article
        className="settings-row settings-row--stack"
        aria-label={t("ext.skillLearning.usesTitle")}
      >
        <div className="settings-row__text">
          <div className="settings-row__label">
            {t("ext.skillLearning.usesTitle")}
          </div>
          <div className="settings-row__desc">
            {t("ext.skillLearning.usesDesc")}
          </div>
        </div>

        {!desktopAvailable ? (
          <div className="settings-row__hint">
            {t("ext.skillLearning.desktopRequired")}
          </div>
        ) : !sessionId ? (
          <div className="settings-row__hint">
            {t("ext.skillLearning.noSession")}
          </div>
        ) : usesLoading ? (
          <div className="settings-row__hint">
            {t("ext.skillLearning.loadingUses")}
          </div>
        ) : visibleUses.length === 0 ? (
          <div className="settings-row__hint">
            {t("ext.skillLearning.noUses")}
          </div>
        ) : (
          <ul className="ext-list">
            {visibleUses.map((record) => {
              const proposalBusy = actionBusyKey === `proposal:${record.id}`;
              return (
                <li className="ext-item" key={record.id}>
                  <div className="ext-item__head">
                    <strong className="ext-item__name">
                      {record.skill.name}
                    </strong>
                    <span className="ext-badge ext-badge--muted">
                      {t(SKILL_USE_STATUS_LABELS[record.status])}
                    </span>
                    {record.skill.sourceCandidateId?.trim() ? (
                      <span className="ext-badge ext-badge--scope">
                        {t("ext.skillLearning.hostOwned")}
                      </span>
                    ) : null}
                  </div>
                  <div className="ext-item__meta">
                    <span>{t(SKILL_SELECTION_LABELS[record.selection])}</span>
                    <span>
                      {t("ext.skillLearning.updatedAt")}: {" "}
                      <time dateTime={record.updatedAt}>
                        {formatTime(
                          record.updatedAt,
                          locale,
                          t("ext.skillLearning.timeUnavailable"),
                        )}
                      </time>
                    </span>
                  </div>
                  {record.feedback ? (
                    <div className="settings-row__hint">
                      {t("ext.skillLearning.feedbackRecorded")}: {" "}
                      {record.feedback.rating === "helpful"
                        ? t("ext.skillLearning.helpful")
                        : t("ext.skillLearning.unhelpful")}
                    </div>
                  ) : null}
                  <div className="ext-item__actions">
                    {isTerminal(record.status) && !record.feedback ? (
                      <>
                        {record.status === "succeeded" ? (
                          <button
                            type="button"
                            className="btn btn--ghost btn--sm"
                            disabled={actionBusyKey !== null}
                            onClick={() =>
                              void recordFeedback(record, "helpful")
                            }
                          >
                            {actionBusyKey === `feedback:helpful:${record.id}`
                              ? t("ext.skillLearning.savingFeedback")
                              : t("ext.skillLearning.helpful")}
                          </button>
                        ) : null}
                        <button
                          type="button"
                          className="btn btn--ghost btn--sm"
                          disabled={actionBusyKey !== null}
                          onClick={() =>
                            void recordFeedback(record, "unhelpful")
                          }
                        >
                          {actionBusyKey === `feedback:unhelpful:${record.id}`
                            ? t("ext.skillLearning.savingFeedback")
                            : t("ext.skillLearning.unhelpful")}
                        </button>
                      </>
                    ) : null}
                    {record.status === "succeeded" &&
                    record.skill.sourceCandidateId?.trim() ? (
                      <button
                        type="button"
                        className="btn btn--secondary btn--sm"
                        disabled={actionBusyKey !== null}
                        onClick={() => void buildProposal(record)}
                      >
                        {proposalBusy
                          ? t("ext.skillLearning.generatingProposal")
                          : t("ext.skillLearning.generateProposal")}
                      </button>
                    ) : null}
                  </div>
                </li>
              );
            })}
          </ul>
        )}
      </article>

      {visibleProposal ? (
        <article
          className="settings-row settings-row--stack"
          aria-label={t("ext.skillLearning.proposalTitle")}
        >
          <div className="settings-row__label">
            {t("ext.skillLearning.proposalTitle")}
          </div>
          {visibleProposal.proposal ? (
            <>
              <div className="settings-row__desc">
                {visibleProposal.proposal.skillName}
              </div>
              <dl>
                <div>
                  <dt>{t("ext.skillLearning.proposalStatus")}</dt>
                  <dd>{t("ext.skillLearning.pendingReview")}</dd>
                </div>
                <div>
                  <dt>{t("ext.skillLearning.requiresUserReview")}</dt>
                  <dd>true</dd>
                </div>
                <div>
                  <dt>{t("ext.skillLearning.mayWriteSkill")}</dt>
                  <dd>false</dd>
                </div>
                <div>
                  <dt>{t("ext.skillLearning.mayOverwriteExternal")}</dt>
                  <dd>false</dd>
                </div>
                <div>
                  <dt>{t("ext.skillLearning.successfulEvidence")}</dt>
                  <dd>{visibleProposal.proposal.successfulUseCount}</dd>
                </div>
                <div>
                  <dt>{t("ext.skillLearning.helpfulEvidence")}</dt>
                  <dd>{visibleProposal.proposal.helpfulUseCount}</dd>
                </div>
              </dl>
              <div className="settings-row__hint">
                {t("ext.skillLearning.proposalReadOnly")}
              </div>
            </>
          ) : (
            <div className="settings-row__hint">
              {t("ext.skillLearning.proposalUnavailable")}
            </div>
          )}
        </article>
      ) : null}

      {visibleError ? (
        <div className="settings-row__hint" role="alert">
          {visibleError}
        </div>
      ) : null}
    </section>
  );
}
