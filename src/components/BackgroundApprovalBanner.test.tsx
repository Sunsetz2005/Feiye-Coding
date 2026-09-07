// @vitest-environment jsdom

import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup } from "@testing-library/react";
import { BackgroundApprovalBanner } from "./BackgroundApprovalBanner";

const labels = {
  one: "Another chat is waiting on your approval.",
  many: (n: number) => `${n} other chats are waiting on your approval.`,
  go: "Go",
};

afterEach(() => cleanup());

describe("BackgroundApprovalBanner", () => {
  it("renders nothing when no session is pending", () => {
    const { container } = render(
      <BackgroundApprovalBanner
        pendingSessionIds={new Set()}
        labels={labels}
        onGo={vi.fn()}
      />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("shows the singular copy for one pending session and jumps to it on click", async () => {
    const user = userEvent.setup();
    const onGo = vi.fn();
    render(
      <BackgroundApprovalBanner
        pendingSessionIds={new Set(["sess-a"])}
        labels={labels}
        onGo={onGo}
      />,
    );
    expect(
      screen.getByText("Another chat is waiting on your approval."),
    ).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Go" }));
    expect(onGo).toHaveBeenCalledWith("sess-a");
  });

  it("shows the plural copy and count for multiple pending sessions", () => {
    render(
      <BackgroundApprovalBanner
        pendingSessionIds={new Set(["sess-a", "sess-b"])}
        labels={labels}
        onGo={vi.fn()}
      />,
    );
    expect(
      screen.getByText("2 other chats are waiting on your approval."),
    ).toBeTruthy();
  });

  it("clears (unmounts) once the pending set empties after navigating away", () => {
    const { rerender, container } = render(
      <BackgroundApprovalBanner
        pendingSessionIds={new Set(["sess-a"])}
        labels={labels}
        onGo={vi.fn()}
      />,
    );
    expect(screen.getByRole("button", { name: "Go" })).toBeTruthy();
    // Simulate the app switching to sess-a: its interaction resolves and the
    // pending set (driven by session://interaction) drops the entry.
    rerender(
      <BackgroundApprovalBanner
        pendingSessionIds={new Set()}
        labels={labels}
        onGo={vi.fn()}
      />,
    );
    expect(container.firstChild).toBeNull();
  });
});
