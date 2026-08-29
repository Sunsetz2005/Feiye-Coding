import { IconActivity } from "@/components/icons";
import type { SessionSubagent } from "@/lib/sessionAgents";

export function SubagentCard({
  agents,
  labels,
  onOpen,
}: {
  agents: SessionSubagent[];
  labels: {
    running: string;
    completed: string;
    failed: string;
    open: string;
  };
  onOpen: (agent: SessionSubagent) => void;
}) {
  if (agents.length === 0) return null;

  return (
    <div className="turn-changes" data-testid="turn-subagents">
      <ul className="turn-changes__list" style={{ marginTop: 0 }}>
        {agents.map((agent) => {
          const running =
            agent.status === "in_progress" ||
            agent.status === "running" ||
            agent.status === "queued";
          const failed =
            agent.status === "failed" || agent.status === "cancelled";
          const title = running
            ? labels.running
                .replace("{desc}", agent.description)
                .replace("{type}", agent.agentType)
            : failed
              ? labels.failed
                  .replace("{desc}", agent.description)
                  .replace("{type}", agent.agentType)
              : labels.completed
                  .replace("{desc}", agent.description)
                  .replace("{type}", agent.agentType);
          return (
            <li key={agent.id}>
              <button
                type="button"
                className="turn-changes__file"
                data-testid={`subagent-card-${agent.id}`}
                onClick={() => onOpen(agent)}
              >
                <span className="turn-changes__icon" aria-hidden>
                  <IconActivity size={16} />
                </span>
                <span className="turn-changes__path" title={title}>
                  {title}
                </span>
                <span className="turn-changes__review">{labels.open}</span>
              </button>
            </li>
          );
        })}
      </ul>
    </div>
  );
}
