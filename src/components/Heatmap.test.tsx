// @vitest-environment jsdom

import { cleanup, render, screen } from "@testing-library/react";
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { Heatmap } from "./Heatmap";

beforeAll(() => {
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
});
afterAll(() => vi.unstubAllGlobals());
afterEach(() => cleanup());

describe("Heatmap", () => {
  it("renders coral level cells for activity days", () => {
    render(
      <Heatmap
        locale="en"
        metric="requests"
        days={[
          { date: "2026-01-01", requests: 0, tokens: 0, costUsd: 0 },
          { date: "2026-01-02", requests: 4, tokens: 40, costUsd: 0.1 },
          { date: "2026-01-03", requests: 12, tokens: 400, costUsd: 1.2 },
        ]}
        labels={{
          less: "Less",
          more: "More",
          noData: "No data",
          aria: "Activity",
          requests: "Requests",
          tokens: "Tokens",
        }}
      />,
    );
    expect(screen.getByLabelText("Activity")).toBeTruthy();
    expect(screen.getByText("Less")).toBeTruthy();
    expect(screen.getByText("More")).toBeTruthy();
  });
});
