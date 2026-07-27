import {
  isToolStepMessage,
  parseToolStepContent,
  toolStepDisplayTitle,
  type ChatMessage,
} from "@/lib/session";

export type ActivityCategory =
  | "skill"
  | "compact"
  | "file"
  | "command"
  | "image"
  | "browser"
  | "subtask"
  | "ask"
  | "generic";

export type ActivityStatus =
  | "running"
  | "completed"
  | "failed"
  | "cancelled";

export interface ActivityItem {
  id: string;
  category: ActivityCategory;
  status: ActivityStatus;
  title: string;
  detail?: string;
  path?: string;
  toolCallId?: string;
  source: ChatMessage;
}

export interface ActivityGroup {
  id: string;
  category: ActivityCategory;
  status: ActivityStatus;
  items: ActivityItem[];
}

export interface ActivityRun {
  startIndex: number;
  endIndex: number;
  group: ActivityGroup;
}

const FILE_EXTENSION_RE =
  /\.(?:[cm]?[jt]sx?|json|ya?ml|toml|mdx?|txt|css|scss|html?|rs|py|go|java|kt|swift|c|h|cpp|hpp|pdf|docx?|xlsx?|pptx?)\b/i;
const IMAGE_EXTENSION_RE = /\.(?:png|jpe?g|gif|webp|svg|heic|avif)\b/i;
const MAX_ACTIVITY_TITLE_LENGTH = 120;

function isCompactActivity(message: ChatMessage): boolean {
  return (
    message.marker === "context_compact" ||
    message.content?.startsWith("context_compact") === true ||
    Boolean(message.compactMeta)
  );
}

function isTurnCancelledActivity(message: ChatMessage): boolean {
  return (
    message.marker === "turn_cancelled" ||
    (message.role === "tool" &&
      message.content?.startsWith("turn_cancelled") === true)
  );
}

/**
 * Activity text is deliberately single-line and bounded. Tool output remains
 * available on the stored source record, but must not become a transcript
 * dump (or accidentally expose a credential) just because an old journal row
 * lacks modern tool metadata.
 */
function safeActivityTitle(value: string): string {
  const normalized = value
    .replace(/[\u0000-\u001f\u007f]+/g, " ")
    .replace(/\s+/g, " ")
    .trim();
  const points = Array.from(normalized);
  if (points.length <= MAX_ACTIVITY_TITLE_LENGTH) return normalized;
  return `${points.slice(0, MAX_ACTIVITY_TITLE_LENGTH - 1).join("")}…`;
}

function legacyToolTitle(message: ChatMessage): string {
  const metadataTitle = message.toolKind?.trim() || message.marker?.trim() || "";
  if (!metadataTitle) return "";
  return safeActivityTitle(metadataTitle.replace(/[_./-]+/g, " "));
}

function searchableActivityText(message: ChatMessage): string {
  const parsed = parseToolStepContent(message.content || "");
  const raw = [
    message.toolKind,
    message.toolDetail,
    message.toolPath,
    parsed?.kind,
    parsed?.title,
    parsed?.detail,
    parsed?.path,
    message.content,
  ]
    .filter(Boolean)
    .join(" ")
    .toLowerCase();
  // Keep the raw form for extensions/paths and add a tokenized form so
  // snake_case tool names still satisfy ordinary word-boundary matching.
  return `${raw} ${raw.replace(/[_./-]+/g, " ")}`;
}

export function classifyActivity(message: ChatMessage): ActivityCategory {
  if (isCompactActivity(message)) {
    return "compact";
  }

  const haystack = searchableActivityText(message);

  if (
    /\b(?:ask[_ -]?user|request[_ -]?user[_ -]?input|questionnaire)\b/.test(
      haystack,
    ) ||
    /(?:询问|提问|問卷|問題|问题)/.test(haystack)
  ) {
    return "ask";
  }
  if (
    /\b(?:skill|skills|skill\.md)\b/.test(haystack) ||
    /(?:技能|技巧)/.test(haystack)
  ) {
    return "skill";
  }
  if (
    /\b(?:compact|compaction|context[_ -]?compact)\b/.test(haystack) ||
    /(?:压缩上下文|上下文压缩|壓縮上下文|上下文壓縮)/.test(haystack)
  ) {
    return "compact";
  }
  if (
    /\b(?:browser|playwright|search[_ -]?query|web[_ -]?search|navigate|open[_ -]?url)\b/.test(
      haystack,
    ) ||
    /(?:浏览器|瀏覽器|网页搜索|網頁搜尋)/.test(haystack)
  ) {
    return "browser";
  }
  if (
    /\b(?:subagent|sub-agent|spawn[_ -]?agent|delegate|collaboration|task[_ -]?agent)\b/.test(
      haystack,
    ) ||
    /(?:子任务|子任務|子代理|委派)/.test(haystack)
  ) {
    return "subtask";
  }
  if (
    /\b(?:image|screenshot|view[_ -]?image|imagegen|vision)\b/.test(
      haystack,
    ) ||
    IMAGE_EXTENSION_RE.test(haystack) ||
    /(?:图片|圖像|截图|截圖|查看图像|查看圖片)/.test(haystack)
  ) {
    return "image";
  }
  if (
    /\b(?:exec|shell|terminal|command|bash|zsh|powershell|cmd\.exe|run[_ -]?command)\b/.test(
      haystack,
    ) ||
    /(?:运行命令|執行命令|终端|終端)/.test(haystack)
  ) {
    return "command";
  }
  if (
    /\b(?:read|write|edit|patch|file|folder|directory|filesystem|fs[_ -])\b/.test(
      haystack,
    ) ||
    FILE_EXTENSION_RE.test(haystack) ||
    /(?:文件|资料夹|資料夾|目录|目錄)/.test(haystack)
  ) {
    return "file";
  }
  return "generic";
}

export function normalizeActivityStatus(message: ChatMessage): ActivityStatus {
  if (message.streaming) return "running";
  const parsed = parseToolStepContent(message.content || "");
  const raw = (message.toolStatus || parsed?.status || "completed").toLowerCase();
  if (
    raw === "in_progress" ||
    raw === "pending" ||
    raw === "running" ||
    raw === "started"
  ) {
    return "running";
  }
  if (raw === "failed" || raw === "error") return "failed";
  if (raw === "cancelled" || raw === "canceled" || raw === "aborted") {
    return "cancelled";
  }
  if (message.isError) return "failed";
  return "completed";
}

export function activityItemFromMessage(
  message: ChatMessage,
): ActivityItem | null {
  if (isTurnCancelledActivity(message)) return null;

  const compact = isCompactActivity(message);
  const modernToolStep = isToolStepMessage(message);
  const legacyTool = message.role === "tool" && !modernToolStep && !compact;
  if (
    !modernToolStep &&
    !legacyTool &&
    !compact
  ) {
    return null;
  }

  const parsed = parseToolStepContent(message.content || "");
  const title = legacyTool
    ? legacyToolTitle(message)
    : safeActivityTitle(
        toolStepDisplayTitle(message) ||
          parsed?.title?.trim() ||
          message.toolKind?.trim() ||
          "",
      );
  return {
    id: message.id,
    category: legacyTool ? "generic" : classifyActivity(message),
    status: normalizeActivityStatus(message),
    title,
    // Unknown legacy rows can contain arbitrary raw output. Keep the source
    // record intact, while showing only bounded metadata in the timeline.
    detail: legacyTool
      ? undefined
      : message.toolDetail?.trim() || parsed?.detail?.trim() || undefined,
    path: legacyTool
      ? undefined
      : message.toolPath?.trim() || parsed?.path?.trim() || undefined,
    toolCallId: message.toolCallId,
    source: message,
  };
}

function canMergeActivity(left: ActivityItem, right: ActivityItem): boolean {
  return (
    left.status === "completed" &&
    right.status === "completed" &&
    left.category === right.category
  );
}

/**
 * Build render runs without reordering the journal.
 *
 * Only adjacent, completed entries of the same semantic category collapse.
 * Running/failed/cancelled entries stay individually addressable.
 */
export function buildActivityRuns(
  messages: ChatMessage[],
  options: { includeRunning?: boolean; includeCompactMarkers?: boolean } = {},
): ActivityRun[] {
  const includeRunning = options.includeRunning ?? true;
  const includeCompactMarkers = options.includeCompactMarkers ?? false;
  const runs: ActivityRun[] = [];

  for (let index = 0; index < messages.length; index += 1) {
    const message = messages[index]!;
    const item = activityItemFromMessage(message);
    if (!item) continue;
    if (!includeCompactMarkers && isCompactActivity(message)) {
      continue;
    }
    if (!includeRunning && item.status === "running") continue;

    const items = [item];
    let endIndex = index;
    while (endIndex + 1 < messages.length) {
      const nextMessage = messages[endIndex + 1]!;
      const next = activityItemFromMessage(nextMessage);
      if (!next) break;
      if (!includeCompactMarkers && isCompactActivity(nextMessage)) {
        break;
      }
      if (!includeRunning && next.status === "running") break;
      if (!canMergeActivity(items[items.length - 1]!, next)) break;
      items.push(next);
      endIndex += 1;
    }

    runs.push({
      startIndex: index,
      endIndex,
      group: {
        id: `activity-${item.id}-${items.length}`,
        category: item.category,
        status: item.status,
        items,
      },
    });
    index = endIndex;
  }

  return runs;
}

export function activityGroupSummary(
  group: ActivityGroup,
  fallback: string,
  categoryTemplates?: Partial<Record<ActivityCategory, string>>,
): string {
  const first = group.items[0]?.title.trim() || fallback;
  if (group.items.length <= 1) return first;

  const template = categoryTemplates?.[group.category];
  return template
    ? template.replaceAll("{count}", String(group.items.length))
    : `${fallback} · ${group.items.length}`;
}
