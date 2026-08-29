import { describe, expect, it } from "vitest";
import type { ChatMessage } from "@/lib/session";
import { createMessageCache } from "./messageCache";

const msg = (id: string): ChatMessage[] => [{ id, role: "assistant", content: id }];

describe("createMessageCache", () => {
  it("evicts idle sessions past the limit, keeping pin and busy", () => {
    const cache = createMessageCache(2);
    cache.pin("view");
    cache.set("view", msg("view"));
    cache.set("busy", msg("busy"));
    cache.markBusy("busy", true);
    cache.set("a", msg("a"));
    cache.set("b", msg("b"));
    cache.set("c", msg("c"));
    expect(cache.get("view")).toBeTruthy();
    expect(cache.get("busy")).toBeTruthy();
    expect(cache.size()).toBeLessThanOrEqual(4);
    expect(cache.get("a")).toBeUndefined();
  });

  it("does not evict a busy session even if it is oldest", () => {
    const cache = createMessageCache(1);
    cache.set("old", msg("old"));
    cache.markBusy("old", true);
    cache.set("new", msg("new"));
    expect(cache.get("old")).toBeTruthy();
    expect(cache.get("new")).toBeTruthy();
  });
});
