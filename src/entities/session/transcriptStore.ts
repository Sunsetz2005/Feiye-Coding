import { useSyncExternalStore } from "react";
import type { ChatMessage, SessionState } from "@/lib/session";
import { isSessionBusy, isSessionLiveStreaming } from "@/lib/session";
import {
  createMessageCache,
  MESSAGE_CACHE_IDLE_LIMIT,
} from "./messageCache";
import { DRAFT_SESSION_KEY } from "./streamCoalesce";

export type TranscriptPatch = (prev: ChatMessage[]) => ChatMessage[];

export type TranscriptMeta = {
  viewingId: string | null;
  length: number;
  lastUserId: string | null;
  hasError: boolean;
  streaming: boolean;
  hasUser: boolean;
  hasAssistant: boolean;
};

const EMPTY_MESSAGES: ChatMessage[] = [];

const EMPTY_META: TranscriptMeta = {
  viewingId: null,
  length: 0,
  lastUserId: null,
  hasError: false,
  streaming: false,
  hasUser: false,
  hasAssistant: false,
};

function computeMeta(
  viewingId: string | null,
  messages: readonly ChatMessage[],
): TranscriptMeta {
  let lastUserId: string | null = null;
  let hasError = false;
  let streaming = false;
  let hasUser = false;
  let hasAssistant = false;
  for (const message of messages) {
    if (message.role === "user") {
      lastUserId = message.id;
      if (message.content?.trim()) hasUser = true;
    } else if (message.role === "assistant") {
      if (message.streaming) streaming = true;
      if (message.content?.trim()) hasAssistant = true;
    }
    if (message.isError) hasError = true;
  }
  return {
    viewingId,
    length: messages.length,
    lastUserId,
    hasError,
    streaming,
    hasUser,
    hasAssistant,
  };
}

function metaEqual(a: TranscriptMeta, b: TranscriptMeta): boolean {
  return (
    a.viewingId === b.viewingId &&
    a.length === b.length &&
    a.lastUserId === b.lastUserId &&
    a.hasError === b.hasError &&
    a.streaming === b.streaming &&
    a.hasUser === b.hasUser &&
    a.hasAssistant === b.hasAssistant
  );
}

function cacheKey(sessionId: string | null | undefined): string {
  return sessionId || DRAFT_SESSION_KEY;
}

export function createTranscriptStore(idleLimit = MESSAGE_CACHE_IDLE_LIMIT) {
  const cache = createMessageCache(idleLimit);
  let viewingId: string | null = null;
  let viewed: ChatMessage[] = EMPTY_MESSAGES;
  let meta: TranscriptMeta = EMPTY_META;
  const viewedListeners = new Set<() => void>();
  const metaListeners = new Set<() => void>();

  const emitViewed = () => {
    for (const listener of viewedListeners) listener();
  };
  const emitMeta = () => {
    for (const listener of metaListeners) listener();
  };

  const refreshMeta = () => {
    const next = computeMeta(viewingId, viewed);
    if (metaEqual(meta, next)) return false;
    meta = next;
    return true;
  };

  const persistViewed = (messages: ChatMessage[]) => {
    viewed = messages;
    cache.set(cacheKey(viewingId), messages);
  };

  const store = {
    subscribeViewed(listener: () => void) {
      viewedListeners.add(listener);
      return () => {
        viewedListeners.delete(listener);
      };
    },
    subscribeMeta(listener: () => void) {
      metaListeners.add(listener);
      return () => {
        metaListeners.delete(listener);
      };
    },
    getViewed: () => viewed,
    getMeta: () => meta,
    getViewingId: () => viewingId,
    getCached(sessionId: string | null | undefined): ChatMessage[] | undefined {
      return cache.get(cacheKey(sessionId));
    },
    setBusy(sessionId: string | null | undefined, busy: boolean) {
      if (!sessionId) return;
      cache.markBusy(sessionId, busy);
    },
    markHostState(sessionId: string | null | undefined, state: SessionState) {
      if (!sessionId) return;
      cache.markBusy(
        sessionId,
        isSessionBusy(state) || isSessionLiveStreaming(state),
      );
    },
    setViewing(sessionId: string | null, messages?: ChatMessage[]) {
      viewingId = sessionId;
      cache.pin(sessionId ? cacheKey(sessionId) : DRAFT_SESSION_KEY);
      if (messages) {
        persistViewed(messages);
      } else if (sessionId) {
        viewed = cache.get(sessionId) ?? EMPTY_MESSAGES;
      } else {
        viewed = cache.get(DRAFT_SESSION_KEY) ?? EMPTY_MESSAGES;
      }
      const metaChanged = refreshMeta();
      emitViewed();
      if (metaChanged) emitMeta();
    },
    replaceViewed(messages: ChatMessage[]) {
      persistViewed(messages);
      const metaChanged = refreshMeta();
      emitViewed();
      if (metaChanged) emitMeta();
    },
    write(sessionId: string | null | undefined, messages: ChatMessage[]) {
      const key = cacheKey(sessionId);
      cache.set(key, messages);
      if (cacheKey(viewingId) === key) {
        viewed = messages;
        const metaChanged = refreshMeta();
        emitViewed();
        if (metaChanged) emitMeta();
      }
    },
    /**
     * Move the viewed transcript from a draft/old id onto a materialized
     * session without flashing an empty thread.
     */
    adoptViewing(
      fromSessionId: string | null | undefined,
      toSessionId: string | null,
    ) {
      const fromKey = cacheKey(fromSessionId);
      const toKey = cacheKey(toSessionId);
      const messages =
        cacheKey(viewingId) === fromKey
          ? viewed
          : (cache.get(fromKey) ?? EMPTY_MESSAGES);
      cache.set(toKey, messages);
      viewingId = toSessionId;
      cache.pin(toKey);
      viewed = messages;
      if (fromKey !== toKey) cache.delete(fromKey);
      const metaChanged = refreshMeta();
      emitViewed();
      if (metaChanged) emitMeta();
    },
    /** Point viewing at an already-cached session (e.g. after optimistic send). */
    rebindViewing(sessionId: string | null) {
      const key = cacheKey(sessionId);
      if (cacheKey(viewingId) === key) return;
      viewingId = sessionId;
      cache.pin(key);
      viewed = cache.get(key) ?? EMPTY_MESSAGES;
      const metaChanged = refreshMeta();
      emitViewed();
      if (metaChanged) emitMeta();
    },
    patch(sessionId: string | null | undefined, reduce: TranscriptPatch) {
      const key = cacheKey(sessionId);
      const isViewed = cacheKey(viewingId) === key;
      const prev = isViewed ? viewed : (cache.get(key) ?? EMPTY_MESSAGES);
      const next = reduce(prev);
      if (next === prev) return;
      cache.set(key, next);
      if (isViewed) {
        viewed = next;
        const metaChanged = refreshMeta();
        emitViewed();
        if (metaChanged) emitMeta();
      }
    },
    evict(sessionId: string) {
      cache.delete(sessionId);
      if (cacheKey(viewingId) === sessionId) {
        viewed = EMPTY_MESSAGES;
        const metaChanged = refreshMeta();
        emitViewed();
        if (metaChanged) emitMeta();
      }
    },
    clearViewed() {
      persistViewed(EMPTY_MESSAGES);
      const metaChanged = refreshMeta();
      emitViewed();
      if (metaChanged) emitMeta();
    },
    size: () => cache.size(),
    reset() {
      cache.clear();
      viewingId = null;
      viewed = EMPTY_MESSAGES;
      meta = EMPTY_META;
      emitViewed();
      emitMeta();
    },
  };

  return store;
}

export type TranscriptStore = ReturnType<typeof createTranscriptStore>;

export const transcriptStore = createTranscriptStore();

export function useViewedMessages(
  store: TranscriptStore = transcriptStore,
): ChatMessage[] {
  return useSyncExternalStore(
    store.subscribeViewed,
    store.getViewed,
    store.getViewed,
  );
}

export function useTranscriptMeta(
  store: TranscriptStore = transcriptStore,
): TranscriptMeta {
  return useSyncExternalStore(
    store.subscribeMeta,
    store.getMeta,
    store.getMeta,
  );
}
