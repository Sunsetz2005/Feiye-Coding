import type { OrbState } from "thinking-orbs";
import type { SessionState } from "@/lib/session";
import type { ActivityItem } from "./activityTimelineModel";

export type LiveStatus = {
  state: OrbState;
  label: string;
};

const WRITE_KIND_RE = /\b(?:write_file|write|edit|patch|apply_patch)\b/;
const READ_KIND_RE =
  /\b(?:read_file|list_directory|read|list|search|glob|grep)\b/;

export function isWriteFileActivity(item: ActivityItem): boolean {
  const kind = `${item.source.toolKind ?? ""} ${item.source.marker ?? ""}`.toLowerCase();
  if (WRITE_KIND_RE.test(kind)) return true;
  if (READ_KIND_RE.test(kind)) return false;
  return WRITE_KIND_RE.test(item.title.toLowerCase());
}

export function orbStateForActivity(item: ActivityItem): OrbState {
  switch (item.category) {
    case "compact":
      return "weaving";
    case "skill":
      return "connecting";
    case "file":
      return isWriteFileActivity(item) ? "shaping" : "searching";
    case "command":
      return "working";
    case "ask":
      return "listening";
    case "browser":
      return "searching";
    case "image":
      return "composing";
    case "subtask":
      return "working";
    default:
      return "working";
  }
}

export function isLiveSessionState(state: SessionState): boolean {
  return (
    state === "connecting" ||
    state === "streaming" ||
    state === "awaiting_permission"
  );
}

export function resolveLiveStatus(input: {
  sessionState: SessionState;
  liveActivity: ActivityItem | null;
  streamingHasContent: boolean;
  thinkingLabel: string;
  replyingLabel: string;
}): LiveStatus | null {
  if (!isLiveSessionState(input.sessionState)) return null;

  if (input.liveActivity?.status === "running") {
    const title = input.liveActivity.title.trim();
    return {
      state: orbStateForActivity(input.liveActivity),
      label: title || input.thinkingLabel,
    };
  }

  if (input.sessionState === "awaiting_permission") {
    return { state: "listening", label: input.thinkingLabel };
  }

  if (input.streamingHasContent) {
    return { state: "composing", label: input.replyingLabel };
  }

  return { state: "breathing", label: input.thinkingLabel };
}
