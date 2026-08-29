import { describe, expect, it } from "vitest";
import type { ChatMessage } from "@/lib/session";
import { activityItemFromMessage } from "./activityTimelineModel";
import {
  isWriteFileActivity,
  orbStateForActivity,
  resolveLiveStatus,
} from "./liveStatus";

function tool(
  kind: string,
  title: string,
  status = "in_progress",
): ChatMessage {
  return {
    id: kind,
    role: "tool",
    content: `tool_step|${status}|${kind}|${title}`,
    marker: "tool_step",
    toolKind: kind,
    toolStatus: status,
    streaming: status === "in_progress",
  };
}

describe("orbStateForActivity", () => {
  it("maps host tools onto thinking-orbs states", () => {
    expect(
      orbStateForActivity(activityItemFromMessage(tool("read_file", "Read a.ts"))!),
    ).toBe("searching");
    expect(
      orbStateForActivity(
        activityItemFromMessage(tool("list_directory", "List src"))!,
      ),
    ).toBe("searching");
    expect(
      orbStateForActivity(
        activityItemFromMessage(tool("write_file", "Write a.ts"))!,
      ),
    ).toBe("shaping");
    expect(
      orbStateForActivity(
        activityItemFromMessage(tool("run_command", "Run pnpm test"))!,
      ),
    ).toBe("working");
    expect(
      orbStateForActivity(
        activityItemFromMessage({
          id: "c",
          role: "tool",
          content: "context_compact",
          marker: "context_compact",
          streaming: true,
        })!,
      ),
    ).toBe("weaving");
    expect(
      orbStateForActivity(
        activityItemFromMessage(tool("read_skill", "Read SKILL.md"))!,
      ),
    ).toBe("connecting");
    expect(
      orbStateForActivity(
        activityItemFromMessage(tool("ask_user_question", "Ask user"))!,
      ),
    ).toBe("listening");
  });

  it("treats write_file as write-like", () => {
    expect(
      isWriteFileActivity(
        activityItemFromMessage(tool("write_file", "Write a.ts"))!,
      ),
    ).toBe(true);
    expect(
      isWriteFileActivity(
        activityItemFromMessage(tool("read_file", "Read a.ts"))!,
      ),
    ).toBe(false);
  });
});

describe("resolveLiveStatus", () => {
  const labels = {
    thinkingLabel: "Thinking…",
    replyingLabel: "Replying…",
  };

  it("is empty when the session is idle", () => {
    expect(
      resolveLiveStatus({
        sessionState: "idle",
        liveActivity: null,
        streamingHasContent: false,
        ...labels,
      }),
    ).toBeNull();
  });

  it("uses breathing while waiting for the first tokens", () => {
    expect(
      resolveLiveStatus({
        sessionState: "streaming",
        liveActivity: null,
        streamingHasContent: false,
        ...labels,
      }),
    ).toEqual({ state: "breathing", label: "Thinking…" });
  });

  it("uses composing once assistant text is streaming", () => {
    expect(
      resolveLiveStatus({
        sessionState: "streaming",
        liveActivity: null,
        streamingHasContent: true,
        ...labels,
      }),
    ).toEqual({ state: "composing", label: "Replying…" });
  });

  it("uses the running tool title and mapped orb", () => {
    const liveActivity = activityItemFromMessage(
      tool("read_file", "Read src/App.tsx"),
    );
    expect(
      resolveLiveStatus({
        sessionState: "streaming",
        liveActivity,
        streamingHasContent: true,
        ...labels,
      }),
    ).toEqual({ state: "searching", label: "Read src/App.tsx" });
  });

  it("listens while waiting for permission without a tool row", () => {
    expect(
      resolveLiveStatus({
        sessionState: "awaiting_permission",
        liveActivity: null,
        streamingHasContent: false,
        ...labels,
      }),
    ).toEqual({ state: "listening", label: "Thinking…" });
  });

  it("unloads after the turn ends", () => {
    expect(
      resolveLiveStatus({
        sessionState: "ready",
        liveActivity: null,
        streamingHasContent: false,
        ...labels,
      }),
    ).toBeNull();
  });
});
