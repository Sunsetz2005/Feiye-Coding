// @vitest-environment jsdom

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { ActivityRow } from "./ActivityTimeline";
import { activityItemFromMessage } from "./activityTimelineModel";
import type { ChatMessage } from "@/lib/session";

afterEach(() => cleanup());

const labels = {
  running: "Running",
  completed: "Done",
  failed: "Failed",
  cancelled: "Cancelled",
  fallback: "Tool",
  askCompleted: "Asked {count} · {answered} answered · {skipped} skipped",
};

function askTool(title: string, status = "completed"): ChatMessage {
  return {
    id: "ask-1",
    role: "tool",
    content: `tool_step|${status}|ask_user_question|${title}`,
    marker: "tool_step",
    toolKind: "ask_user_question",
    toolStatus: status,
  };
}

describe("ActivityRow ask titles", () => {
  it("does not invent skipped counts for a raw question header", () => {
    const item = activityItemFromMessage(
      askTool("question|quiz|poll|选择题|提问"),
    )!;
    render(<ActivityRow item={item} labels={labels} />);
    expect(screen.queryByText(/skipped/i)).toBeNull();
    expect(screen.getByText(/选择题|提问/)).toBeTruthy();
  });

  it("localizes Host ask summaries that already include counts", () => {
    const item = activityItemFromMessage(
      askTool("Asked 1 question · 0 answered · 1 skipped"),
    )!;
    render(<ActivityRow item={item} labels={labels} />);
    expect(screen.getByText("Asked 1 · 0 answered · 1 skipped")).toBeTruthy();
  });
});
