// @vitest-environment jsdom

import { cleanup, render, screen } from "@testing-library/react";
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import type { ChatMessage } from "@/lib/session";
import { CONVERSATION_WINDOW_THRESHOLD } from "@/entities/session";
import { ConversationThread } from "./ConversationThread";

const attachLabels = {
  open: "Open",
  reveal: "Reveal",
  copyPath: "Copy path",
  copyImage: "Copy image",
  addToComposer: "Add",
  remove: "Remove",
};

beforeAll(() => {
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
  vi.stubGlobal(
    "IntersectionObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
  HTMLCanvasElement.prototype.getContext = (() =>
    ({
      setTransform() {},
      clearRect() {},
      beginPath() {},
      arc() {},
      fill() {},
      stroke() {},
      moveTo() {},
      lineTo() {},
    })) as unknown as typeof HTMLCanvasElement.prototype.getContext;
});
afterAll(() => vi.unstubAllGlobals());
afterEach(() => cleanup());

function fill(n: number): ChatMessage[] {
  return Array.from({ length: n }, (_, i) => ({
    id: `m${i}`,
    role: i % 2 === 0 ? "user" : "assistant",
    content: `row ${i}`,
  }));
}

describe("ConversationThread", () => {
  it("shows the empty start copy", () => {
    render(
      <ConversationThread
        locale="en"
        messages={[]}
        sessionState="idle"
        attachLabels={attachLabels}
      />,
    );
    expect(screen.getByText(/start/i)).toBeTruthy();
  });

  it("renders a user and assistant turn", () => {
    render(
      <ConversationThread
        locale="en"
        messages={[
          { id: "u1", role: "user", content: "ask this" },
          { id: "a1", role: "assistant", content: "here is the answer" },
        ]}
        sessionState="ready"
        attachLabels={attachLabels}
      />,
    );
    expect(screen.getByText("ask this")).toBeTruthy();
    expect(screen.getByText("here is the answer")).toBeTruthy();
    expect(
      document.querySelector(".lobe-chat-item--frozen"),
    ).toBeTruthy();
  });

  it("shows a last-turn file change card after the agent finishes", async () => {
    const { default: userEvent } = await import("@testing-library/user-event");
    const user = userEvent.setup();
    const onOpenTurnChanges = vi.fn();
    const onOpenResource = vi.fn();
    render(
      <ConversationThread
        locale="en"
        sessionState="ready"
        projectPath="/tmp/proj"
        attachLabels={attachLabels}
        onOpenTurnChanges={onOpenTurnChanges}
        onOpenResource={onOpenResource}
        messages={[
          { id: "u1", role: "user", content: "edit files" },
          {
            id: "t1",
            role: "tool",
            content: "tool_step|completed|write|Write a.ts",
            marker: "tool_step",
            toolKind: "write",
            toolPath: "/tmp/proj/a.ts",
            toolStatus: "completed",
          },
          { id: "a1", role: "assistant", content: "done" },
        ]}
      />,
    );
    expect(screen.getByTestId("turn-changes")).toBeTruthy();
    expect(screen.getByText("Edited 1 files")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Review" }));
    expect(onOpenTurnChanges).toHaveBeenCalledOnce();
    await user.click(screen.getByRole("button", { name: "a.ts" }));
    expect(onOpenResource).toHaveBeenCalledWith({
      type: "changes",
      path: "/tmp/proj/a.ts",
      title: "a.ts",
    });
  });

  it("renders a last-turn subagent card and opens the agent pane", async () => {
    const { default: userEvent } = await import("@testing-library/user-event");
    const user = userEvent.setup();
    const onOpenResource = vi.fn();
    render(
      <ConversationThread
        locale="en"
        sessionState="streaming"
        attachLabels={attachLabels}
        onOpenResource={onOpenResource}
        messages={[
          { id: "u1", role: "user", content: "search the repo" },
          {
            id: "tool-a",
            role: "tool",
            content:
              "tool_step|in_progress|agent|Subagent search repo\nexplore\nagent-9",
            marker: "tool_step",
            toolKind: "agent",
            toolStatus: "in_progress",
            toolPath: "agent-9",
          },
        ]}
      />,
    );
    expect(screen.getByTestId("turn-subagents")).toBeTruthy();
    await user.click(screen.getByTestId("subagent-card-agent-9"));
    expect(onOpenResource).toHaveBeenCalledWith({
      type: "agent",
      id: "agent-9",
      title: "search repo",
    });
  });

  it("windows long history behind a spacer", () => {
    const messages = fill(CONVERSATION_WINDOW_THRESHOLD + 4);
    messages.push({ id: "u-live", role: "user", content: "latest question" });
    messages.push({
      id: "a-live",
      role: "assistant",
      content: "latest answer",
      streaming: true,
    });
    render(
      <ConversationThread
        locale="en"
        messages={messages}
        sessionState="streaming"
        attachLabels={attachLabels}
      />,
    );
    expect(document.querySelector(".lobe-chat-history-spacer")).toBeTruthy();
    expect(screen.getByText("latest question")).toBeTruthy();
    expect(screen.queryByText("row 0")).toBeNull();
    expect(screen.getByTestId("agent-status-pill").textContent).toMatch(
      /Replying/i,
    );
    expect(
      document.querySelector(".lobe-chat__inner")?.lastElementChild,
    ).toBe(screen.getByTestId("agent-status-pill"));
  });

  it("shows a thinking pill before assistant tokens arrive", () => {
    render(
      <ConversationThread
        locale="en"
        sessionState="streaming"
        attachLabels={attachLabels}
        messages={[{ id: "u1", role: "user", content: "hello" }]}
      />,
    );
    const pill = screen.getByTestId("agent-status-pill");
    expect(pill.getAttribute("data-orb-state")).toBe("breathing");
    expect(pill.textContent).toMatch(/Thinking/i);
    expect(
      document.querySelector(".lobe-chat__inner")?.lastElementChild,
    ).toBe(pill);
  });

  it("maps a running read onto the searching pill", () => {
    render(
      <ConversationThread
        locale="en"
        sessionState="streaming"
        attachLabels={attachLabels}
        messages={[
          { id: "u1", role: "user", content: "read it" },
          {
            id: "a1",
            role: "assistant",
            content: "looking",
            streaming: true,
          },
          {
            id: "t1",
            role: "tool",
            content: "tool_step|in_progress|read_file|Read src/App.tsx",
            marker: "tool_step",
            toolKind: "read_file",
            toolStatus: "in_progress",
            streaming: true,
          },
        ]}
      />,
    );
    const pill = screen.getByTestId("agent-status-pill");
    expect(pill.getAttribute("data-orb-state")).toBe("searching");
    expect(pill.textContent).toMatch(/Read src\/App\.tsx/);
    expect(screen.queryAllByTestId("agent-status-pill")).toHaveLength(1);
    expect(
      document.querySelector(".lobe-chat__inner")?.lastElementChild,
    ).toBe(pill);
  });

  it("keeps the status pill at the thread tail while assistant text streams", () => {
    render(
      <ConversationThread
        locale="en"
        sessionState="streaming"
        attachLabels={attachLabels}
        messages={[
          { id: "u1", role: "user", content: "hello" },
          {
            id: "a1",
            role: "assistant",
            content: "here is a long reply that keeps going",
            streaming: true,
          },
        ]}
      />,
    );
    const pill = screen.getByTestId("agent-status-pill");
    expect(pill.getAttribute("data-orb-state")).toBe("composing");
    expect(pill.textContent).toMatch(/Replying/i);
    expect(
      document.querySelector(".lobe-chat__inner")?.lastElementChild,
    ).toBe(pill);
  });

  it("removes the pill when the session is idle", () => {
    render(
      <ConversationThread
        locale="en"
        sessionState="ready"
        attachLabels={attachLabels}
        messages={[
          { id: "u1", role: "user", content: "ask this" },
          { id: "a1", role: "assistant", content: "here is the answer" },
        ]}
      />,
    );
    expect(screen.queryByTestId("agent-status-pill")).toBeNull();
  });
});
