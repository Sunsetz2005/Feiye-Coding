// @vitest-environment jsdom

import { cleanup, render, screen } from "@testing-library/react";
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { transcriptStore } from "@/entities/session";
import { ConversationSurface } from "./ConversationSurface";

const attachLabels = {
  open: "Open",
  reveal: "Reveal",
  copyPath: "Copy path",
  copyImage: "Copy image",
  addToComposer: "Add",
  remove: "Remove",
};

beforeAll(() => {
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
});
afterAll(() => vi.unstubAllGlobals());
afterEach(() => {
  transcriptStore.reset();
  cleanup();
});

describe("ConversationSurface", () => {
  it("renders viewed transcript messages from the store", () => {
    transcriptStore.setViewing("s1", [
      { id: "u1", role: "user", content: "hello from store" },
      { id: "a1", role: "assistant", content: "reply from store" },
    ]);
    render(
      <ConversationSurface
        locale="en"
        sessionState="ready"
        attachLabels={attachLabels}
      />,
    );
    expect(screen.getByText("hello from store")).toBeTruthy();
    expect(screen.getByText("reply from store")).toBeTruthy();
  });
});
