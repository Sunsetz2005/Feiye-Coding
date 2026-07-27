import {
  useEffect,
  useMemo,
  useReducer,
  useRef,
  useState,
  type KeyboardEvent,
} from "react";
import {
  IconCheck,
  IconChevronLeft,
  IconChevronRight,
  IconClose,
  IconSend,
} from "@/components/icons";
import type { AskUserPayload } from "@/lib/session";
import {
  buildAskUserAnswers,
  createAskUserDockState,
  parseRecommendedOption,
  questionHasAnswer,
  questionStateKey,
  reduceAskUserDockState,
  skippedAskUserQuestionIds,
  type AskUserDockState,
} from "./askUserDockState";
import "./workbench-content.css";

export type AskUserDockLabels = {
  title: string;
  submit: string;
  cancel: string;
  otherPlaceholder: string;
  freeTextHint: string;
  multiHint: string;
  close: string;
  previous?: string;
  next?: string;
  skip?: string;
  progress?: string;
  recommended?: string;
  submitFailed?: string;
};

/** Compatibility name for callers migrating from the modal. */
export type AskUserLabels = AskUserDockLabels;

export interface AskUserDockSubmitMeta {
  skippedQuestionIds: string[];
}

export interface AskUserDockProps {
  payload: AskUserPayload | null;
  labels: AskUserDockLabels;
  onSubmit: (
    answers: Record<string, string>,
    meta?: AskUserDockSubmitMeta,
  ) => void | Promise<void>;
  onCancel: () => void | Promise<void>;
}

function progressLabel(
  template: string | undefined,
  current: number,
  total: number,
): string {
  return (template || "{current} / {total}")
    .replace("{current}", String(current))
    .replace("{total}", String(total));
}

function failureText(error: unknown, fallback: string | undefined): string {
  if (fallback) return fallback;
  if (error instanceof Error && error.message.trim()) return error.message;
  return String(error || "Unable to submit");
}

export function AskUserDock({
  payload,
  labels,
  onSubmit,
  onCancel,
}: AskUserDockProps) {
  const [state, dispatch] = useReducer(
    reduceAskUserDockState,
    payload,
    createAskUserDockState,
  );
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const busyRef = useRef(false);
  const questionBodyRef = useRef<HTMLDivElement>(null);

  const questions = payload?.questions ?? [];
  const questionCount = questions.length;
  const safeIndex = Math.min(state.index, Math.max(0, questionCount - 1));
  const question = questions[safeIndex] ?? null;
  const stateKey = question
    ? questionStateKey(question, safeIndex)
    : "question::0";
  const selected = state.selected[stateKey] ?? [];
  const freeText = state.freeText[stateKey] ?? "";

  useEffect(() => {
    dispatch({ type: "reset", payload });
    busyRef.current = false;
    setBusy(false);
    setError("");
  }, [payload?.rpcId, payload?.partialAnswers]);

  useEffect(() => {
    if (!question) return;
    const frame = window.requestAnimationFrame(() => {
      const target = questionBodyRef.current?.querySelector<HTMLElement>(
        '[data-ask-user-autofocus="true"]',
      );
      target?.focus({ preventScroll: true });
    });
    return () => window.cancelAnimationFrame(frame);
  }, [payload?.rpcId, safeIndex, question]);

  const canContinue = useMemo(
    () =>
      question
        ? questionHasAnswer(state, question, safeIndex)
        : false,
    [question, safeIndex, state],
  );

  if (!payload || !question) return null;

  const submit = async (nextState: AskUserDockState) => {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setError("");
    try {
      await onSubmit(buildAskUserAnswers(payload, nextState), {
        skippedQuestionIds: skippedAskUserQuestionIds(payload, nextState),
      });
    } catch (submitError) {
      setError(failureText(submitError, labels.submitFailed));
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  };

  const closeGroup = async () => {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setError("");
    try {
      await onCancel();
    } catch (cancelError) {
      setError(failureText(cancelError, labels.submitFailed));
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  };

  const goTo = (index: number) => {
    if (busy) return;
    setError("");
    dispatch({ type: "go", index, questionCount });
  };

  const advanceOrSubmit = (nextState: AskUserDockState) => {
    if (safeIndex < questionCount - 1) {
      dispatch({
        type: "go",
        index: safeIndex + 1,
        questionCount,
      });
      return;
    }
    const unresolvedIndex = questions.findIndex((candidate, index) => {
      const key = questionStateKey(candidate, index);
      return (
        !nextState.skipped[key] &&
        !questionHasAnswer(nextState, candidate, index)
      );
    });
    if (unresolvedIndex >= 0) {
      dispatch({
        type: "go",
        index: unresolvedIndex,
        questionCount,
      });
      return;
    }
    void submit(nextState);
  };

  const chooseOption = (optionId: string) => {
    const action = {
      type: "select" as const,
      question,
      questionIndex: safeIndex,
      optionId,
    };
    const nextState = reduceAskUserDockState(state, action);
    dispatch(action);
    setError("");
    if (!question.multiSelect) {
      advanceOrSubmit(nextState);
    }
  };

  const writeAnswer = (value: string) => {
    dispatch({
      type: "write",
      question,
      questionIndex: safeIndex,
      value,
    });
    setError("");
  };

  const commitText = (
    event?: KeyboardEvent<HTMLTextAreaElement>,
  ) => {
    if (event) {
      if (
        event.key !== "Enter" ||
        event.shiftKey ||
        event.nativeEvent.isComposing
      ) {
        return;
      }
      event.preventDefault();
    }
    const value = event?.currentTarget.value ?? freeText;
    const action = {
      type: "write" as const,
      question,
      questionIndex: safeIndex,
      value,
    };
    const nextState = reduceAskUserDockState(state, action);
    if (!questionHasAnswer(nextState, question, safeIndex)) return;
    dispatch(action);
    advanceOrSubmit(nextState);
  };

  const skipCurrent = () => {
    const action = {
      type: "skip" as const,
      question,
      questionIndex: safeIndex,
    };
    const nextState = reduceAskUserDockState(state, action);
    dispatch(action);
    setError("");
    advanceOrSubmit(nextState);
  };

  return (
    <section
      className="ask-user-dock"
      role="region"
      aria-labelledby="ask-user-dock-title"
      aria-busy={busy || undefined}
      data-testid="ask-user-dock"
    >
      <header className="ask-user-dock__header">
        <h2 id="ask-user-dock-title" className="ask-user-dock__title">
          {labels.title}
        </h2>
        <div className="ask-user-dock__paging">
          {questionCount > 1 ? (
            <>
              <button
                type="button"
                className="ask-user-dock__icon-btn"
                onClick={() => goTo(safeIndex - 1)}
                disabled={busy || safeIndex === 0}
                aria-label={labels.previous || "Previous"}
                title={labels.previous}
              >
                <IconChevronLeft size={16} />
              </button>
              <span className="ask-user-dock__progress">
                {progressLabel(labels.progress, safeIndex + 1, questionCount)}
              </span>
              <button
                type="button"
                className="ask-user-dock__icon-btn"
                onClick={() => goTo(safeIndex + 1)}
                disabled={busy || safeIndex === questionCount - 1}
                aria-label={labels.next || "Next"}
                title={labels.next}
              >
                <IconChevronRight size={16} />
              </button>
            </>
          ) : null}
          <button
            type="button"
            className="ask-user-dock__icon-btn"
            onClick={() => void closeGroup()}
            disabled={busy}
            aria-label={labels.close}
            title={labels.close}
          >
            <IconClose size={16} />
          </button>
        </div>
      </header>

      <div
        ref={questionBodyRef}
        className="ask-user-dock__body"
        role="group"
        aria-labelledby={`ask-user-dock-question-${safeIndex}`}
      >
        <div className="ask-user-dock__question-row">
          <span className="ask-user-dock__question-number" aria-hidden>
            {safeIndex + 1}
          </span>
          <div
            id={`ask-user-dock-question-${safeIndex}`}
            className="ask-user-dock__question"
          >
            {question.question}
          </div>
        </div>

        {question.multiSelect ? (
          <p className="ask-user-dock__hint">{labels.multiHint}</p>
        ) : null}

        {question.options.length ? (
          <div className="ask-user-dock__options">
            {question.options.map((option, optionIndex) => {
              const parsed = parseRecommendedOption(option);
              const active = selected.includes(option.id);
              return (
                <button
                  key={option.id}
                  type="button"
                  className={
                    "ask-user-dock__option" +
                    (active ? " is-selected" : "")
                  }
                  aria-pressed={active}
                  disabled={busy}
                  data-ask-user-autofocus={
                    optionIndex === 0 ? "true" : undefined
                  }
                  onClick={() => chooseOption(option.id)}
                >
                  <span className="ask-user-dock__option-index">
                    {active ? <IconCheck size={13} /> : optionIndex + 1}
                  </span>
                  <span className="ask-user-dock__option-copy">
                    <span className="ask-user-dock__option-title">
                      {parsed.displayLabel}
                      {parsed.recommended ? (
                        <span className="ask-user-dock__recommended">
                          {labels.recommended || "Recommended"}
                        </span>
                      ) : null}
                    </span>
                    {option.description ? (
                      <span className="ask-user-dock__option-description">
                        {option.description}
                      </span>
                    ) : null}
                  </span>
                  {!question.multiSelect ? (
                    <IconChevronRight
                      size={15}
                      className="ask-user-dock__option-arrow"
                    />
                  ) : null}
                </button>
              );
            })}
          </div>
        ) : null}

        <label className="ask-user-dock__free">
          <span className="ask-user-dock__free-label">
            {question.options.length
              ? labels.freeTextHint
              : labels.otherPlaceholder}
          </span>
          <span className="ask-user-dock__free-control">
            <textarea
              className="ask-user-dock__textarea"
              rows={1}
              value={freeText}
              disabled={busy}
              placeholder={labels.otherPlaceholder}
              data-ask-user-autofocus={
                question.options.length ? undefined : "true"
              }
              onChange={(event) => writeAnswer(event.currentTarget.value)}
              onKeyDown={commitText}
            />
            <button
              type="button"
              className="ask-user-dock__send"
              onClick={() => commitText()}
              disabled={busy || !freeText.trim()}
              aria-label={safeIndex === questionCount - 1 ? labels.submit : labels.next}
            >
              <IconSend size={15} />
            </button>
          </span>
        </label>
      </div>

      {error ? (
        <p className="ask-user-dock__error" role="alert">
          {error}
        </p>
      ) : null}

      <footer className="ask-user-dock__footer">
        <button
          type="button"
          className="ask-user-dock__skip"
          disabled={busy}
          onClick={skipCurrent}
        >
          {labels.skip || labels.cancel}
        </button>
        {question.multiSelect ? (
          <button
            type="button"
            className="ask-user-dock__continue"
            disabled={busy || !canContinue}
            onClick={() => advanceOrSubmit(state)}
          >
            {safeIndex === questionCount - 1
              ? labels.submit
              : labels.next || labels.submit}
            <IconChevronRight size={15} />
          </button>
        ) : null}
      </footer>
    </section>
  );
}

/** Named alias lets root callers swap the import without changing JSX. */
export const AskUserModal = AskUserDock;
