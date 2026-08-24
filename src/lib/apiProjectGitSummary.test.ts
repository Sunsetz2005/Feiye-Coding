// @vitest-environment jsdom

import { afterEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

import {
  projectGitSummaryV1,
  type ProjectGitSummaryV1,
} from "./api";

function setDesktop(enabled: boolean): void {
  if (enabled) {
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      value: {},
      configurable: true,
    });
  } else {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
    delete (window as unknown as Record<string, unknown>).__TAURI__;
  }
}

describe("projectGitSummaryV1", () => {
  afterEach(() => {
    setDesktop(false);
    invokeMock.mockReset();
  });

  it("returns null in the browser without invoking Host", async () => {
    setDesktop(false);

    await expect(
      projectGitSummaryV1("project-1", "/Users/me/project"),
    ).resolves.toBeNull();

    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("invokes the exact Host command and request payload", async () => {
    setDesktop(true);
    const response = {
      version: 1,
      projectId: "project-1",
      available: true,
      isRepo: true,
      branch: "main",
      ahead: 1,
      behind: 0,
      dirty: 2,
      conflicts: 0,
      countsCapped: false,
      head: "abcdef012345",
      observedAt: "2026-08-24T00:00:00Z",
      source: "git_status_porcelain_v2",
      unavailableReason: null,
    } satisfies ProjectGitSummaryV1;
    invokeMock.mockResolvedValueOnce(response);

    await expect(
      projectGitSummaryV1("project-1", "/Users/me/project"),
    ).resolves.toBe(response);

    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith("project_git_summary_v1", {
      request: {
        version: 1,
        projectId: "project-1",
        projectPath: "/Users/me/project",
      },
    });
  });

  it("does not swallow a Host invoke rejection", async () => {
    setDesktop(true);
    const error = new Error("boom");
    invokeMock.mockRejectedValueOnce(error);

    await expect(
      projectGitSummaryV1("project-1", "/Users/me/project"),
    ).rejects.toBe(error);
  });
});
