import type { ReactNode } from "react";
import {
  IconActivity,
  IconAlertTriangle,
  IconArrowsMinimize,
  IconBolt,
  IconChat,
  IconFileText,
  IconImagine,
  IconRobot,
  IconSearch,
  IconSkills,
} from "@/components/icons";
import { Spinner } from "@/components/ui/spinner";
import {
  activityGroupSummary,
  type ActivityCategory,
  type ActivityGroup,
  type ActivityItem,
  type ActivityStatus,
} from "./activityTimelineModel";
import "./workbench-content.css";

export interface ActivityTimelineLabels {
  running: string;
  completed: string;
  failed: string;
  cancelled: string;
  fallback: string;
  askRunning?: string;
  askCompleted?: string;
  askCancelled?: string;
  askFailed?: string;
  groupSummary?: Partial<Record<ActivityCategory, string>>;
}

function CategoryIcon({ category }: { category: ActivityCategory }) {
  const iconByCategory: Record<ActivityCategory, ReactNode> = {
    skill: <IconSkills size={14} />,
    compact: <IconArrowsMinimize size={14} />,
    file: <IconFileText size={14} />,
    command: <IconBolt size={14} />,
    image: <IconImagine size={14} />,
    browser: <IconSearch size={14} />,
    subtask: <IconRobot size={14} />,
    ask: <IconChat size={14} />,
    generic: <IconActivity size={14} />,
  };
  return <>{iconByCategory[category]}</>;
}

function statusLabel(
  status: ActivityStatus,
  labels: ActivityTimelineLabels,
): string {
  if (status === "running") return labels.running;
  if (status === "failed") return labels.failed;
  if (status === "cancelled") return labels.cancelled;
  return labels.completed;
}

function fillActivityTemplate(
  template: string,
  values: Record<string, number>,
): string {
  return Object.entries(values).reduce(
    (text, [key, value]) =>
      text.replaceAll(`{${key}}`, String(value)),
    template,
  );
}

function visibleActivityTitle(
  item: ActivityItem,
  labels: ActivityTimelineLabels,
): string {
  const fallback = item.title.trim() || labels.fallback;
  if (item.category !== "ask") return fallback;
  const numbers = fallback.match(/\d+/g)?.map(Number) ?? [];
  const count = numbers[0] ?? 1;
  if (item.status === "running" && labels.askRunning) {
    return fillActivityTemplate(labels.askRunning, { count });
  }
  if (item.status === "completed" && labels.askCompleted) {
    const answered = numbers[1] ?? 0;
    const skipped = numbers[2] ?? Math.max(0, count - answered);
    return fillActivityTemplate(labels.askCompleted, {
      count,
      answered,
      skipped,
    });
  }
  if (item.status === "cancelled" && labels.askCancelled) {
    return fillActivityTemplate(labels.askCancelled, { count });
  }
  if (item.status === "failed" && labels.askFailed) {
    return fillActivityTemplate(labels.askFailed, { count });
  }
  return fallback;
}

export function ActivityRow({
  item,
  labels,
  nested = false,
}: {
  item: ActivityItem;
  labels: ActivityTimelineLabels;
  nested?: boolean;
}) {
  const label = visibleActivityTitle(item, labels);
  const detail = item.detail || item.path || "";
  return (
    <div
      className={
        "sunsetz-activity-row" +
        ` is-${item.status}` +
        (nested ? " is-nested" : "")
      }
      role={item.status === "running" ? "status" : undefined}
      aria-label={`${label} · ${statusLabel(item.status, labels)}`}
      title={detail || label}
      data-activity-category={item.category}
      data-tool-id={item.toolCallId}
    >
      <span className="sunsetz-activity-row__icon" aria-hidden>
        {item.status === "running" ? (
          <Spinner size={14} />
        ) : item.status === "failed" ? (
          <IconAlertTriangle size={14} />
        ) : (
          <CategoryIcon category={item.category} />
        )}
      </span>
      <span className="sunsetz-activity-row__title">{label}</span>
      {detail ? (
        <span className="sunsetz-activity-row__detail">{detail}</span>
      ) : null}
      {item.status !== "completed" ? (
        <span className="sunsetz-activity-row__status">
          {statusLabel(item.status, labels)}
        </span>
      ) : null}
    </div>
  );
}

export function ActivityTimeline({
  group,
  labels,
}: {
  group: ActivityGroup;
  labels: ActivityTimelineLabels;
}) {
  if (group.items.length === 1) {
    return <ActivityRow item={group.items[0]!} labels={labels} />;
  }

  const summary = activityGroupSummary(
    group,
    labels.fallback,
    labels.groupSummary,
  );
  return (
    <details
      className="sunsetz-activity-group"
      data-activity-category={group.category}
    >
      <summary className="sunsetz-activity-group__summary">
        <span className="sunsetz-activity-row__icon" aria-hidden>
          <CategoryIcon category={group.category} />
        </span>
        <span className="sunsetz-activity-row__title">{summary}</span>
      </summary>
      <div className="sunsetz-activity-group__items">
        {group.items.map((item) => (
          <ActivityRow
            key={item.id}
            item={item}
            labels={labels}
            nested
          />
        ))}
      </div>
    </details>
  );
}
