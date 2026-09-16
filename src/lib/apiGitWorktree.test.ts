// @vitest-environment jsdom

import { afterEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

import { gitWorktreeAdd, gitWorktreeRemove } from "./api";

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

describe("gitWorktreeAdd", () => {
  afterEach(() => {
    setDesktop(false);
    invokeMock.mockReset();
  });

  it("invokes git_worktree_add with the exact request shape", async () => {
    setDesktop(true);
    invokeMock.mockResolvedValueOnce({ available: true, worktrees: [] });

    await gitWorktreeAdd("/proj", "/proj-feature-x", "feature-x", true);

    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith("git_worktree_add", {
      projectPath: "/proj",
      newPath: "/proj-feature-x",
      branchName: "feature-x",
      createBranch: true,
    });
  });

  it("does not swallow a Host invoke rejection", async () => {
    setDesktop(true);
    const error = new Error("git worktree add failed");
    invokeMock.mockRejectedValueOnce(error);

    await expect(
      gitWorktreeAdd("/proj", "/proj-x", null, false),
    ).rejects.toBe(error);
  });
});

describe("gitWorktreeRemove", () => {
  afterEach(() => {
    setDesktop(false);
    invokeMock.mockReset();
  });

  it("invokes git_worktree_remove with the exact request shape", async () => {
    setDesktop(true);
    invokeMock.mockResolvedValueOnce({ available: true, worktrees: [] });

    await gitWorktreeRemove("/proj", "/proj-feature-x", true);

    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith("git_worktree_remove", {
      projectPath: "/proj",
      worktreePath: "/proj-feature-x",
      force: true,
    });
  });

  it("does not swallow a Host invoke rejection", async () => {
    setDesktop(true);
    const error = new Error("dirty worktree");
    invokeMock.mockRejectedValueOnce(error);

    await expect(
      gitWorktreeRemove("/proj", "/proj-x", false),
    ).rejects.toBe(error);
  });
});
