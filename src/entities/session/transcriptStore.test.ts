import { describe, expect, it } from "vitest";
import type { ChatMessage } from "@/lib/session";
import { createTranscriptStore } from "./transcriptStore";

const user = (id: string): ChatMessage => ({
  id,
  role: "user",
  content: id,
});
const assistant = (id: string, extra?: Partial<ChatMessage>): ChatMessage => ({
  id,
  role: "assistant",
  content: extra?.content ?? id,
  streaming: extra?.streaming,
});

describe("transcriptStore", () => {
  it("notifies viewed listeners on token patches without changing meta", () => {
    const store = createTranscriptStore();
    store.setViewing("s1", [
      user("u1"),
      assistant("a1", { streaming: true, content: "H" }),
    ]);
    const startMeta = store.getMeta();
    let viewed = 0;
    let meta = 0;
    store.subscribeViewed(() => {
      viewed += 1;
    });
    store.subscribeMeta(() => {
      meta += 1;
    });
    store.patch("s1", (prev) => {
      const next = prev.slice();
      next[1] = { ...next[1]!, content: "Hel" };
      return next;
    });
    store.patch("s1", (prev) => {
      const next = prev.slice();
      next[1] = { ...next[1]!, content: "Hello" };
      return next;
    });
    expect(viewed).toBe(2);
    expect(meta).toBe(0);
    expect(store.getMeta()).toBe(startMeta);
    expect(store.getViewed()[1]?.content).toBe("Hello");
  });

  it("does not emit viewed listeners for a background session patch", () => {
    const store = createTranscriptStore();
    store.setViewing("focus", [user("u-focus")]);
    store.patch("bg", () => [user("u-bg"), assistant("a-bg", { streaming: true })]);
    let viewed = 0;
    store.subscribeViewed(() => {
      viewed += 1;
    });
    store.patch("bg", (prev) => {
      const next = prev.slice();
      next[1] = { ...next[1]!, content: "secret" };
      return next;
    });
    expect(viewed).toBe(0);
    expect(store.getCached("bg")?.[1]?.content).toBe("secret");
    expect(store.getViewed()[0]?.id).toBe("u-focus");
  });

  it("writes, marks busy, evicts, and resets", () => {
    const store = createTranscriptStore();
    store.setViewing("s1", [user("u1")]);
    store.write("bg", [user("u-bg")]);
    store.markHostState("bg", "streaming");
    store.setBusy("bg", true);
    store.setBusy("bg", false);
    store.markHostState("bg", "ready");
    store.setBusy(null, true);
    store.markHostState(null, "streaming");
    store.patch("s1", (prev) => prev);
    expect(store.getCached("bg")?.[0]?.id).toBe("u-bg");
    store.clearViewed();
    expect(store.getViewed()).toEqual([]);
    store.evict("s1");
    store.setViewing(null);
    store.patch(null, (prev) => [...prev, user("draft")]);
    expect(store.getViewed()[0]?.id).toBe("draft");
    store.reset();
    expect(store.size()).toBe(0);
    expect(store.getViewingId()).toBeNull();
  });

  it("keeps the optimistic user bubble when a draft is adopted onto a real id", () => {
    const store = createTranscriptStore();
    store.setViewing(null, [user("u-draft"), assistant("a-pending", { streaming: true })]);
    store.adoptViewing(null, "sess-1");
    expect(store.getViewingId()).toBe("sess-1");
    expect(store.getViewed().map((m) => m.id)).toEqual(["u-draft", "a-pending"]);
    expect(store.getCached("__draft__")).toBeUndefined();
  });

  it("rebinds viewed to an already-patched session cache", () => {
    const store = createTranscriptStore();
    store.setViewing(null, []);
    store.patch("sess-2", () => [user("u2")]);
    store.rebindViewing("sess-2");
    expect(store.getViewed()[0]?.id).toBe("u2");
  });

  it("emits meta when a new user message lands", () => {
    const store = createTranscriptStore();
    store.setViewing("s1", []);
    let meta = 0;
    store.subscribeMeta(() => {
      meta += 1;
    });
    store.replaceViewed([user("u1")]);
    expect(meta).toBe(1);
    expect(store.getMeta().lastUserId).toBe("u1");
    expect(store.getMeta().length).toBe(1);
  });
});
