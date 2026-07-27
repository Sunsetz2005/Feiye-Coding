import type {
  AskUserOption,
  AskUserPayload,
  AskUserQuestionItem,
} from "@/lib/session";

export interface AskUserDockState {
  index: number;
  selected: Record<string, string[]>;
  freeText: Record<string, string>;
  skipped: Record<string, boolean>;
}

export type AskUserDockAction =
  | { type: "go"; index: number; questionCount: number }
  | {
      type: "select";
      question: AskUserQuestionItem;
      questionIndex: number;
      optionId: string;
    }
  | {
      type: "write";
      question: AskUserQuestionItem;
      questionIndex: number;
      value: string;
    }
  | {
      type: "skip";
      question: AskUserQuestionItem;
      questionIndex: number;
    }
  | {
      type: "reset";
      payload?: Pick<AskUserPayload, "questions" | "partialAnswers"> | null;
    };

export interface RecommendedOption {
  rawLabel: string;
  displayLabel: string;
  recommended: boolean;
}

const RECOMMENDED_SUFFIX_RE =
  /\s*(?:\((?:recommended|recommend)\)|\[(?:recommended|recommend)\]|（(?:推荐|推薦)）|\[(?:推荐|推薦)\])\s*$/i;

export function questionStateKey(
  question: AskUserQuestionItem,
  index: number,
): string {
  const identity =
    question.id?.trim() || question.question?.trim() || "question";
  return `${identity}::${index}`;
}

/**
 * Keep the ACP wire key compatible with the former modal.
 * The index suffix above is intentionally local-only so duplicate prompts do not
 * overwrite each other's draft state.
 */
export function answerWireKey(
  question: AskUserQuestionItem,
  index: number,
): string {
  return question.question?.trim() || question.id?.trim() || String(index);
}

export function parseRecommendedOption(
  option: AskUserOption,
): RecommendedOption {
  const rawLabel = option.label;
  const explicit =
    (option as AskUserOption & { recommended?: boolean }).recommended === true;
  const hasSuffix = RECOMMENDED_SUFFIX_RE.test(rawLabel);
  const stripped = hasSuffix
    ? rawLabel.replace(RECOMMENDED_SUFFIX_RE, "").trim()
    : rawLabel.trim();
  return {
    rawLabel,
    displayLabel: stripped || rawLabel,
    recommended: explicit || hasSuffix,
  };
}

export function createAskUserDockState(
  payload?: Pick<AskUserPayload, "questions" | "partialAnswers"> | null,
): AskUserDockState {
  const state: AskUserDockState = {
    index: 0,
    selected: {},
    freeText: {},
    skipped: {},
  };
  const partialAnswers = payload?.partialAnswers;
  if (!partialAnswers) return state;

  payload.questions.forEach((question, index) => {
    const answer = partialAnswers[answerWireKey(question, index)];
    if (typeof answer !== "string" || !answer.trim()) return;
    const value = answer.trim();
    const stateKey = questionStateKey(question, index);

    if (!question.options.length) {
      state.freeText[stateKey] = value;
      return;
    }

    if (!question.multiSelect) {
      const option = question.options.find(
        (candidate) => candidate.label === value,
      );
      if (option) {
        state.selected[stateKey] = [option.id];
      } else {
        state.freeText[stateKey] = value;
      }
      return;
    }

    const values = value.split(",").map((part) => part.trim());
    const selected = question.options.filter((candidate) =>
      values.includes(candidate.label),
    );
    if (selected.length > 0 && selected.length === values.length) {
      state.selected[stateKey] = selected.map((option) => option.id);
    } else {
      state.freeText[stateKey] = value;
    }
  });
  return state;
}

function withoutKey<T>(record: Record<string, T>, key: string): Record<string, T> {
  if (!(key in record)) return record;
  const next = { ...record };
  delete next[key];
  return next;
}

export function reduceAskUserDockState(
  state: AskUserDockState,
  action: AskUserDockAction,
): AskUserDockState {
  if (action.type === "reset") return createAskUserDockState(action.payload);

  if (action.type === "go") {
    const last = Math.max(0, action.questionCount - 1);
    return {
      ...state,
      index: Math.max(0, Math.min(action.index, last)),
    };
  }

  const key = questionStateKey(action.question, action.questionIndex);
  if (action.type === "select") {
    const current = state.selected[key] ?? [];
    const selected = action.question.multiSelect
      ? current.includes(action.optionId)
        ? current.filter((id) => id !== action.optionId)
        : [...current, action.optionId]
      : [action.optionId];
    return {
      ...state,
      selected: { ...state.selected, [key]: selected },
      freeText: withoutKey(state.freeText, key),
      skipped: withoutKey(state.skipped, key),
    };
  }

  if (action.type === "write") {
    return {
      ...state,
      selected: withoutKey(state.selected, key),
      freeText: { ...state.freeText, [key]: action.value },
      skipped: withoutKey(state.skipped, key),
    };
  }

  return {
    ...state,
    selected: withoutKey(state.selected, key),
    freeText: withoutKey(state.freeText, key),
    skipped: { ...state.skipped, [key]: true },
  };
}

export function questionHasAnswer(
  state: AskUserDockState,
  question: AskUserQuestionItem,
  index: number,
): boolean {
  const key = questionStateKey(question, index);
  return (
    (state.freeText[key] ?? "").trim().length > 0 ||
    (state.selected[key]?.length ?? 0) > 0
  );
}

export function buildAskUserAnswers(
  payload: Pick<AskUserPayload, "questions">,
  state: AskUserDockState,
): Record<string, string> {
  const answers: Record<string, string> = {};
  payload.questions.forEach((question, index) => {
    const stateKey = questionStateKey(question, index);
    if (state.skipped[stateKey]) return;

    const text = (state.freeText[stateKey] ?? "").trim();
    if (text) {
      answers[answerWireKey(question, index)] = text;
      return;
    }

    const optionIds = state.selected[stateKey] ?? [];
    if (!optionIds.length) return;
    const labels = optionIds.map((id) => {
      const option = question.options.find((candidate) => candidate.id === id);
      return option?.label ?? id;
    });
    answers[answerWireKey(question, index)] = labels.join(", ");
  });
  return answers;
}

export function skippedAskUserQuestionIds(
  payload: Pick<AskUserPayload, "questions">,
  state: AskUserDockState,
): string[] {
  const skipped: string[] = [];
  payload.questions.forEach((question, index) => {
    if (!state.skipped[questionStateKey(question, index)]) return;
    skipped.push(question.id?.trim() || answerWireKey(question, index));
  });
  return skipped;
}
