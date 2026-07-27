import { describe, expect, it } from "vitest";
import type { ChatMessage } from "@/lib/session";
import {
  activityGroupSummary,
  activityItemFromMessage,
  buildActivityRuns,
  classifyActivity,
  normalizeActivityStatus,
  type ActivityCategory,
} from "./activityTimelineModel";

function tool(
  id: string,
  kind: string,
  content = kind,
  status = "completed",
): ChatMessage {
  return {
    id,
    role: "tool",
    content,
    marker: "tool_step",
    toolKind: kind,
    toolStatus: status,
    streaming: status === "in_progress",
  };
}

describe("classifyActivity", () => {
  const cases: Array<[ActivityCategory, ChatMessage]> = [
    ["skill", tool("skill", "read_skill", "Read SKILL.md")],
    [
      "compact",
      { id: "compact", role: "tool", content: "context_compact", marker: "context_compact" },
    ],
    ["file", tool("file", "read_file", "Read src/App.tsx")],
    ["command", tool("command", "exec_command", "Run pnpm test")],
    ["image", tool("image", "view_image", "View result.png")],
    ["browser", tool("browser", "browser_search", "Search the web")],
    ["subtask", tool("subtask", "spawn_agent", "Delegate a subtask")],
    ["ask", tool("ask", "ask_user_question", "Ask user")],
    ["generic", tool("generic", "custom_tool", "Process data")],
  ];

  it.each(cases)("maps %s activity", (expected, message) => {
    expect(classifyActivity(message)).toBe(expected);
  });
});

describe("activity status and runs", () => {
  it("normalizes running, failed and cancelled status", () => {
    expect(normalizeActivityStatus(tool("a", "exec", "A", "in_progress"))).toBe(
      "running",
    );
    expect(normalizeActivityStatus(tool("b", "exec", "B", "error"))).toBe(
      "failed",
    );
    expect(normalizeActivityStatus(tool("c", "exec", "C", "aborted"))).toBe(
      "cancelled",
    );
  });

  it("groups only adjacent completed activity of the same category", () => {
    const messages: ChatMessage[] = [
      tool("a", "read_file", "Read a.ts"),
      tool("b", "edit_file", "Edit b.ts"),
      { id: "u", role: "user", content: "continue" },
      tool("c", "read_file", "Read c.ts"),
      tool("d", "read_file", "Read d.ts", "in_progress"),
      tool("e", "exec_command", "Run tests", "failed"),
    ];
    const runs = buildActivityRuns(messages, { includeRunning: false });

    expect(runs).toHaveLength(3);
    expect(runs[0]).toMatchObject({ startIndex: 0, endIndex: 1 });
    expect(runs[0]!.group.items.map((item) => item.id)).toEqual(["a", "b"]);
    expect(runs[1]).toMatchObject({ startIndex: 3, endIndex: 3 });
    expect(runs[2]!.group.status).toBe("failed");
  });

  it("keeps running activity individually visible when requested", () => {
    const run = buildActivityRuns(
      [
        tool("a", "exec_command", "Build", "in_progress"),
        tool("b", "exec_command", "Test"),
      ],
      { includeRunning: true },
    );
    expect(run).toHaveLength(2);
    expect(run[0]!.group.items).toHaveLength(1);
    expect(activityItemFromMessage(run[0]!.group.items[0]!.source)?.status).toBe(
      "running",
    );
  });

  it("summarizes a collapsed group without losing the first title", () => {
    const run = buildActivityRuns([
      tool("a", "read_file", "Read a.ts"),
      tool("b", "edit_file", "Edit b.ts"),
    ])[0]!;
    expect(
      activityGroupSummary(run.group, "Tool", {
        file: "Handled {count} files",
      }),
    ).toBe("Handled 2 files");
    expect(activityGroupSummary(run.group, "Tool")).toBe("Tool · 2");
  });

  it("normalizes unknown legacy tool rows without exposing raw output", () => {
    const legacy: ChatMessage = {
      id: "legacy",
      role: "tool",
      content: "API_TOKEN=should-not-render\nraw command output",
      toolKind: "custom_private_tool",
      toolDetail: "also-sensitive",
      toolPath: "/private/result.txt",
      isError: true,
    };

    const item = activityItemFromMessage(legacy);
    expect(item).toMatchObject({
      category: "generic",
      status: "failed",
      title: "custom private tool",
      detail: undefined,
      path: undefined,
    });
    expect(item?.title).not.toContain("should-not-render");
  });

  it("keeps every legacy tool row in activity runs with a safe fallback", () => {
    const messages: ChatMessage[] = [
      { id: "old-a", role: "tool", content: "unstructured output" },
      { id: "old-b", role: "tool", content: "more unstructured output" },
    ];
    const runs = buildActivityRuns(messages);

    expect(runs).toHaveLength(1);
    expect(runs[0]).toMatchObject({ startIndex: 0, endIndex: 1 });
    expect(runs[0]!.group.category).toBe("generic");
    expect(runs[0]!.group.items.map((item) => item.title)).toEqual(["", ""]);
  });

  it("leaves cancellation and compaction to their specialized renderers", () => {
    const cancelled: ChatMessage = {
      id: "cancelled",
      role: "tool",
      marker: "turn_cancelled",
      content: "turn_cancelled|user",
      toolStatus: "cancelled",
    };
    const compact: ChatMessage = {
      id: "compact",
      role: "tool",
      content: "legacy compact record",
      compactMeta: { trigger: "manual" },
    };

    expect(activityItemFromMessage(cancelled)).toBeNull();
    expect(activityItemFromMessage(compact)?.category).toBe("compact");
    expect(buildActivityRuns([cancelled, compact])).toEqual([]);
  });
});
