// @vitest-environment jsdom

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ResourceViewer } from "@/components/ResourceViewer";

vi.mock("@/components/FileMediaPlayer", () => ({
  FileMediaPlayer: () => null,
}));
vi.mock("@/components/OfficeDocumentPreview", () => ({
  OfficeDocumentPreview: () => null,
}));

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
});
