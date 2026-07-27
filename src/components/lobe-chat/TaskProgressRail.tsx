import { useEffect, useMemo, useState } from "react";
import {
  IconChevronRight,
  IconHandStop,
  IconRename,
  IconTarget,
  IconTrash,
} from "@/components/icons";
import {
  buildTaskProgressMetrics,
  formatTaskElapsed,
  type TaskProgressChange,
  type TaskProgressChangeTotals,
} from "./taskProgressRailModel";
import "./workbench-content.css";

export interface TaskProgressRailLabels {
  /** Supports {current} and {total}. */
  step: string;
  /** Supports {count}. */
  filesChanged: string;
  activeGoal: string;
  details: string;
  edit: string;
  pause: string;
  delete: string;
}

export interface TaskProgressRailProps {
  entries: unknown[];
  changes: readonly TaskProgressChange[];
  /** Optional authoritative aggregate when per-file line stats are unavailable. */
  changeTotals?: TaskProgressChangeTotals | null;
  goalSummary?: string | null;
  /** Real elapsed duration at render time; ticks forward only while streaming. */
  elapsedMs?: number | null;
  streaming: boolean;
  labels: TaskProgressRailLabels;
  onOpenDetails?: () => void;
  onEdit?: () => void;
  onPause?: () => void;
  onDelete?: () => void;
}

function fillTemplate(
  template: string,
  values: Record<string, string | number>,
): string {
  let output = template;
  Object.entries(values).forEach(([key, value]) => {
    output = output.replace(`{${key}}`, String(value));
  });
  return output;
}

export function TaskProgressRail({
  entries,
  changes,
  changeTotals,
  goalSummary,
  elapsedMs,
  streaming,
  labels,
  onOpenDetails,
  onEdit,
  onPause,
  onDelete,
}: TaskProgressRailProps) {
  const metrics = useMemo(
    () => buildTaskProgressMetrics(entries, changes, changeTotals),
    [entries, changes, changeTotals],
  );
  const [elapsedOffset, setElapsedOffset] = useState(0);

  useEffect(() => {
    setElapsedOffset(0);
    if (!streaming || elapsedMs == null) return;
    const anchor = Date.now();
    const timer = window.setInterval(() => {
      setElapsedOffset(Date.now() - anchor);
    }, 1000);
    return () => window.clearInterval(timer);
  }, [elapsedMs, streaming]);

  const summary = (goalSummary || "").trim();
  const elapsed = formatTaskElapsed(
    elapsedMs == null ? null : elapsedMs + elapsedOffset,
  );
  const hasActions = Boolean(
    onOpenDetails || onEdit || onPause || onDelete,
  );

  if (!metrics.hasSteps && !summary && !elapsed && !hasActions) return null;

  const stepText =
    metrics.hasSteps && metrics.currentStep != null
      ? fillTemplate(labels.step, {
          current: metrics.currentStep,
          total: metrics.totalSteps,
        })
      : "";
  const hasChangeFacts =
    metrics.changedFiles != null ||
    metrics.additions != null ||
    metrics.deletions != null;

  return (
    <section
      className={
        "task-progress-rail" + (streaming ? " is-streaming" : "")
      }
      aria-label={labels.activeGoal}
      data-testid="task-progress-rail"
    >
      {stepText ? (
        <div className="task-progress-rail__facts" role="status">
          <span className="task-progress-rail__fact">{stepText}</span>
          {hasChangeFacts ? (
            <span className="task-progress-rail__change-facts">
              <span className="task-progress-rail__separator" aria-hidden>
                ·
              </span>
              {metrics.changedFiles != null ? (
                <span className="task-progress-rail__fact">
                  {fillTemplate(labels.filesChanged, {
                    count: metrics.changedFiles,
                  })}
                </span>
              ) : null}
              {metrics.additions != null ? (
                <span className="task-progress-rail__fact is-add">
                  +{metrics.additions}
                </span>
              ) : null}
              {metrics.deletions != null ? (
                <span className="task-progress-rail__fact is-delete">
                  -{metrics.deletions}
                </span>
              ) : null}
            </span>
          ) : null}
        </div>
      ) : null}

      {(summary || elapsed || hasActions) ? (
        <div className="task-progress-rail__main">
          <span className="task-progress-rail__goal-icon" aria-hidden>
            <IconTarget size={15} />
          </span>
          <div className="task-progress-rail__copy">
            <span className="task-progress-rail__active-label">
              {labels.activeGoal}
            </span>
            {summary ? (
              <span
                className="task-progress-rail__summary"
                title={summary}
              >
                {summary}
              </span>
            ) : null}
          </div>
          {elapsed ? (
            <time className="task-progress-rail__elapsed">{elapsed}</time>
          ) : null}
          {hasActions ? (
            <div className="task-progress-rail__actions">
              {onEdit ? (
                <button
                  type="button"
                  className="task-progress-rail__action"
                  onClick={onEdit}
                  aria-label={labels.edit}
                  title={labels.edit}
                >
                  <IconRename size={15} />
                </button>
              ) : null}
              {onPause ? (
                <button
                  type="button"
                  className="task-progress-rail__action"
                  onClick={onPause}
                  aria-label={labels.pause}
                  title={labels.pause}
                >
                  <IconHandStop size={15} />
                </button>
              ) : null}
              {onDelete ? (
                <button
                  type="button"
                  className="task-progress-rail__action is-danger"
                  onClick={onDelete}
                  aria-label={labels.delete}
                  title={labels.delete}
                >
                  <IconTrash size={15} />
                </button>
              ) : null}
              {onOpenDetails ? (
                <button
                  type="button"
                  className="task-progress-rail__action"
                  onClick={onOpenDetails}
                  aria-label={labels.details}
                  title={labels.details}
                >
                  <IconChevronRight size={15} />
                </button>
              ) : null}
            </div>
          ) : null}
        </div>
      ) : null}
    </section>
  );
}
