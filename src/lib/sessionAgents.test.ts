import { describe, expect, it } from "vitest";
import type { ChatMessage } from "@/lib/session";
import { lastTurnSubagents } from "./sessionAgents";

function tool(
  id: string,
  content: string,
  extra?: Partial<ChatMessage>,
): ChatMessage {
  return {
    id,
    role: "tool",
    content,
    marker: "tool_step",
    ...extra,
  };
}

describe("lastTurnSubagents", () => {
  it("collects agent tool steps after the last user message", () => {
    const agents = lastTurnSubagents([
      { id: "u0", role: "user", content: "old" },
      tool("tool-old", "tool_step|completed|agent|old\nexplore\nold-id"),
      { id: "u1", role: "user", content: "search now" },
      tool(
        "tool-a",
        "tool_step|in_progress|agent|Subagent search repo\nexplore\nagent-1",
        { toolKind: "agent", toolStatus: "in_progress", toolPath: "agent-1" },
      ),
      { id: "a1", role: "assistant", content: "working" },
    ]);
    expect(agents).toHaveLength(1);
    expect(agents[0]?.id).toBe("agent-1");
    expect(agents[0]?.description).toBe("search repo");
    expect(agents[0]?.status).toBe("in_progress");
  });

  it("ignores non-agent tools", () => {
    const agents = lastTurnSubagents([
      { id: "u1", role: "user", content: "edit" },
      tool("tool-w", "tool_step|completed|write|Write a.ts"),
    ]);
    expect(agents).toEqual([]);
  });
});
