import type { ChatMessage } from "@/lib/session";

/** Idle sessions kept warm after the user leaves them. Busy sessions never evict. */
export const MESSAGE_CACHE_IDLE_LIMIT = 8;

export type MessageCache = {
  get: (sessionId: string) => ChatMessage[] | undefined;
  set: (sessionId: string, messages: ChatMessage[]) => void;
  delete: (sessionId: string) => void;
  touch: (sessionId: string) => void;
  markBusy: (sessionId: string, busy: boolean) => void;
  isBusy: (sessionId: string) => boolean;
  pin: (sessionId: string | null) => void;
  evictIdle: () => string[];
  size: () => number;
  clear: () => void;
};

/**
 * LRU cache of per-session transcripts.
 * Never evicts the pinned (viewed) session or sessions marked busy
 * (streaming / permission / background turn).
 */
export function createMessageCache(
  idleLimit = MESSAGE_CACHE_IDLE_LIMIT,
): MessageCache {
  const map = new Map<string, ChatMessage[]>();
  const busy = new Set<string>();
  const order: string[] = [];
  let pinned: string | null = null;

  const touch = (sessionId: string) => {
    const at = order.indexOf(sessionId);
    if (at >= 0) order.splice(at, 1);
    order.push(sessionId);
  };

  const evictIdle = (): string[] => {
    const dropped: string[] = [];
    let idle = 0;
    for (let i = order.length - 1; i >= 0; i--) {
      const id = order[i]!;
      if (id === pinned || busy.has(id)) continue;
      idle += 1;
      if (idle > idleLimit) {
        map.delete(id);
        order.splice(i, 1);
        dropped.push(id);
      }
    }
    return dropped;
  };

  return {
    get: (sessionId) => map.get(sessionId),
    set(sessionId, messages) {
      map.set(sessionId, messages);
      touch(sessionId);
      evictIdle();
    },
    delete(sessionId) {
      map.delete(sessionId);
      busy.delete(sessionId);
      const at = order.indexOf(sessionId);
      if (at >= 0) order.splice(at, 1);
    },
    touch,
    markBusy(sessionId, nextBusy) {
      if (nextBusy) busy.add(sessionId);
      else busy.delete(sessionId);
    },
    isBusy: (sessionId) => busy.has(sessionId),
    pin(sessionId) {
      pinned = sessionId;
      if (sessionId) touch(sessionId);
    },
    evictIdle,
    size: () => map.size,
    clear() {
      map.clear();
      busy.clear();
      order.length = 0;
      pinned = null;
    },
  };
}
