import type { ChatMessage } from "@/lib/session";

/** Below this count the transcript renders every row. */
export const CONVERSATION_WINDOW_THRESHOLD = 48;

/** Always keep this many trailing messages mounted, plus the live turn. */
export const CONVERSATION_WINDOW_TAIL = 8;

export type ConversationWindow = {
  windowed: boolean;
  /** Inclusive start of mounted history (0 when not windowed). */
  start: number;
  /** Exclusive end of mounted history (messages.length). */
  end: number;
  /** Rows [0, start) are replaced by a spacer. */
  hidden: number;
};

function lastUserIndex(messages: readonly ChatMessage[]): number {
  for (let i = messages.length - 1; i >= 0; i--) {
    if (messages[i]?.role === "user") return i;
  }
  return -1;
}

/**
 * Keep the live turn (from the latest user message) plus a short tail of
 * earlier rows. History above `start` is not mounted.
 */
export function conversationWindow(
  messages: readonly ChatMessage[],
  threshold = CONVERSATION_WINDOW_THRESHOLD,
  tail = CONVERSATION_WINDOW_TAIL,
): ConversationWindow {
  const count = messages.length;
  if (count < threshold) {
    return { windowed: false, start: 0, end: count, hidden: 0 };
  }
  const lastUser = lastUserIndex(messages);
  const liveStart = lastUser >= 0 ? lastUser : count;
  const start = Math.max(0, Math.min(liveStart, count - Math.max(1, tail)));
  return {
    windowed: start > 0,
    start,
    end: count,
    hidden: start,
  };
}

/** Estimated spacer height so stick-to-bottom still has a stable scrollHeight. */
export function hiddenHistorySpacerPx(hidden: number, rowEstimatePx = 88): number {
  if (hidden <= 0) return 0;
  return hidden * rowEstimatePx;
}
