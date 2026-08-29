export const SESSION_ENTITY_MODULE = "session";

export {
  applyStreamChunks,
  createStreamCoalescer,
  DRAFT_SESSION_KEY,
  streamSessionKey,
  type StreamApply,
  type StreamCoalescer,
} from "./streamCoalesce";
export {
  conversationWindow,
  CONVERSATION_WINDOW_TAIL,
  CONVERSATION_WINDOW_THRESHOLD,
  hiddenHistorySpacerPx,
  type ConversationWindow,
} from "./conversationWindow";
export {
  createMessageCache,
  MESSAGE_CACHE_IDLE_LIMIT,
  type MessageCache,
} from "./messageCache";
export { splitStreamingMarkdown } from "./streamingMarkdown";
export {
  createTranscriptStore,
  transcriptStore,
  useTranscriptMeta,
  useViewedMessages,
  type TranscriptMeta,
  type TranscriptPatch,
  type TranscriptStore,
} from "./transcriptStore";
