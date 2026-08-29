import type { ChatMessage } from "@/lib/session";
import { isToolStepMessage, parseToolStepContent } from "@/lib/session";

export interface SessionSubagent {
  id: string;
  description: string;
  agentType: string;
  status: string;
  toolCallId?: string;
}

export function lastTurnSubagents(messages: ChatMessage[]): SessionSubagent[] {
  let lastUser = -1;
  for (let i = messages.length - 1; i >= 0; i--) {
    if (messages[i]?.role === "user") {
      lastUser = i;
      break;
    }
  }
  const found = new Map<string, SessionSubagent>();
  for (let i = lastUser + 1; i < messages.length; i++) {
    const m = messages[i]!;
    if (!isToolStepMessage(m)) continue;
    const parsed = parseToolStepContent(m.content || "");
    const kind = (m.toolKind || parsed?.kind || "").toLowerCase();
    if (kind !== "agent") continue;
    const title = parsed?.title || m.content || "subagent";
    const description = title.replace(/^Subagent\s+/i, "").trim() || "subagent";
    const id = parsed?.path || m.toolPath || m.id.replace(/^tool-/, "");
    if (!id) continue;
    found.set(id, {
      id,
      description,
      agentType: parsed?.detail || "general",
      status: (m.toolStatus || parsed?.status || "in_progress").toLowerCase(),
      toolCallId: m.id,
    });
  }
  return [...found.values()];
}
