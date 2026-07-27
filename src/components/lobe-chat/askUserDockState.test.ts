import { describe, expect, it } from "vitest";
import type {
  AskUserOption,
  AskUserPayload,
  AskUserQuestionItem,
} from "@/lib/session";
import {
  buildAskUserAnswers,
  createAskUserDockState,
  parseRecommendedOption,
  questionHasAnswer,
  questionStateKey,
  reduceAskUserDockState,
  skippedAskUserQuestionIds,
} from "./askUserDockState";

const single: AskUserQuestionItem = {
  id: "scope",
  question: "Which scope?",
  options: [
    { id: "composer", label: "Composer" },
    { id: "workbench", label: "Whole workbench (Recommended)" },
  ],
};

const multi: AskUserQuestionItem = {
  id: "areas",
  question: "Which areas?",
  multiSelect: true,
  options: [
    { id: "nav", label: "Navigation" },
    { id: "chat", label: "Chat" },
  ],
};

const payload: AskUserPayload = {
  rpcId: 7,
  sessionId: "session",
  questions: [single, multi],
};

describe("parseRecommendedOption", () => {
  it("understands localized suffixes while preserving the wire label", () => {
    expect(
      parseRecommendedOption({
        id: "a",
        label: "整個工作台（推薦）",
      }),
    ).toEqual({
      rawLabel: "整個工作台（推薦）",
      displayLabel: "整個工作台",
      recommended: true,
    });
  });

  it("supports an explicit recommended field from forward-compatible payloads", () => {
    const option = {
      id: "a",
      label: "Whole workbench",
      recommended: true,
    } as AskUserOption & { recommended: boolean };
    expect(parseRecommendedOption(option).recommended).toBe(true);
  });
});

describe("ask user draft state", () => {
  it("builds single and multiple option answers", () => {
    let state = createAskUserDockState();
    state = reduceAskUserDockState(state, {
      type: "select",
      question: single,
      questionIndex: 0,
      optionId: "composer",
    });
    state = reduceAskUserDockState(state, {
      type: "select",
      question: multi,
      questionIndex: 1,
      optionId: "nav",
    });
    state = reduceAskUserDockState(state, {
      type: "select",
      question: multi,
      questionIndex: 1,
      optionId: "chat",
    });

    expect(buildAskUserAnswers(payload, state)).toEqual({
      "Which scope?": "Composer",
      "Which areas?": "Navigation, Chat",
    });
  });

  it("makes free text replace option selection and trims the wire answer", () => {
    let state = reduceAskUserDockState(createAskUserDockState(), {
      type: "select",
      question: single,
      questionIndex: 0,
      optionId: "composer",
    });
    state = reduceAskUserDockState(state, {
      type: "write",
      question: single,
      questionIndex: 0,
      value: "  Only the input dock  ",
    });

    expect(questionHasAnswer(state, single, 0)).toBe(true);
    expect(buildAskUserAnswers(payload, state)).toEqual({
      "Which scope?": "Only the input dock",
    });
  });

  it("submits partial answers and reports skipped questions", () => {
    let state = reduceAskUserDockState(createAskUserDockState(), {
      type: "select",
      question: single,
      questionIndex: 0,
      optionId: "workbench",
    });
    state = reduceAskUserDockState(state, {
      type: "skip",
      question: multi,
      questionIndex: 1,
    });

    expect(buildAskUserAnswers(payload, state)).toEqual({
      "Which scope?": "Whole workbench (Recommended)",
    });
    expect(skippedAskUserQuestionIds(payload, state)).toEqual(["areas"]);
  });

  it("keeps duplicate prompt drafts isolated by index", () => {
    const duplicate = { ...single, id: "", options: single.options };
    expect(questionStateKey(duplicate, 0)).not.toBe(
      questionStateKey(duplicate, 1),
    );
  });

  it("restores submitted answers retained after a Runtime write failure", () => {
    const restored = createAskUserDockState({
      questions: [single, multi],
      partialAnswers: {
        "Which scope?": "Composer",
        "Which areas?": "Navigation, Chat",
      },
    });

    expect(restored.selected[questionStateKey(single, 0)]).toEqual([
      "composer",
    ]);
    expect(restored.selected[questionStateKey(multi, 1)]).toEqual([
      "nav",
      "chat",
    ]);
    expect(buildAskUserAnswers(payload, restored)).toEqual({
      "Which scope?": "Composer",
      "Which areas?": "Navigation, Chat",
    });
  });
});
