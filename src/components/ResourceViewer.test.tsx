// @vitest-environment jsdom

import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ResourceViewer } from "@/components/ResourceViewer";

vi.mock("@/components/FileMediaPlayer", () => ({
  FileMediaPlayer: () => null,
}));
vi.mock("@/components/OfficeDocumentPreview", () => ({
  OfficeDocumentPreview: () => null,
}));
vi.mock("@/lib/api", async () => {
  const actual = await vi.importActual<typeof import("@/lib/api")>("@/lib/api");
  return {
    ...actual,
    sessionSubagentGet: vi.fn(async () => ({
      id: "agent-1",
      parentSessionId: "s1",
      description: "search repo",
      agentType: "explore",
      status: "completed",
      background: false,
      summary: "found the gate",
      transcript: [{ kind: "assistant", text: "found the gate" }],
    })),
  };
});

afterEach(cleanup);

describe("ResourceViewer chrome", () => {
  it("labels the close control in the no-project state", () => {
    const onClose = vi.fn();
    render(
      <ResourceViewer
        projectPath={null}
        projectName={null}
        locale="en"
        onClose={onClose}
      />,
    );

    screen.getByRole("button", { name: "Close" }).click();
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("labels the close control in the project state", () => {
    render(
      <ResourceViewer
        projectPath="/missing"
        projectName="Missing"
        locale="en"
        onClose={() => {}}
        paneActive={false}
      />,
    );

    expect(screen.getByRole("button", { name: "Close" })).toBeTruthy();
  });

  it("shows Files and Review actions instead of a blank preview", () => {
    render(
      <ResourceViewer
        projectPath="/tmp/demo"
        projectName="Demo"
        locale="en"
        onClose={() => {}}
        paneActive
      />,
    );

    expect(screen.getByTestId("resource-home")).toBeTruthy();
    expect(screen.getByRole("button", { name: /Files/ })).toBeTruthy();
    expect(screen.getByRole("button", { name: /Review/ })).toBeTruthy();
    expect(screen.getByText("Browse the project tree")).toBeTruthy();
  });

  it("opens a read-only subagent transcript pane", async () => {
    render(
      <ResourceViewer
        projectPath="/tmp/demo"
        projectName="Demo"
        locale="en"
        paneActive
        openRequest={{ type: "agent", id: "agent-1", title: "search repo" }}
      />,
    );
    await waitFor(() => {
      expect(screen.getByTestId("resource-agent")).toBeTruthy();
      expect(screen.getByText("found the gate")).toBeTruthy();
    });
  });
});
