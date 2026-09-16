// @vitest-environment jsdom

import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ComposerWorktreeMenu } from "./ComposerProjectMenu";
import type { GitWorktreeEntry } from "@/lib/api";

afterEach(cleanup);

const labels = {
  noProject: "No project",
  pickProject: "Pick project",
  addProject: "Add project",
  worktrees: "Worktrees",
  worktreesEmpty: "No worktrees",
  worktreesUnavailable: "Unavailable",
  worktreeCurrent: "current",
  worktreeSwitch: "Switch",
  worktreeMain: "main",
  worktreeDetached: "detached",
  worktreeCreatePlaceholder: "New branch name…",
  worktreeCreateButton: "New worktree",
  worktreeRemove: "Remove worktree",
};

const MAIN: GitWorktreeEntry = {
  path: "/repo",
  head: "abc",
  branch: "main",
  detached: false,
  isMain: true,
  locked: false,
  prunable: false,
};

const FEATURE: GitWorktreeEntry = {
  path: "/repo-feature",
  head: "def",
  branch: "feature",
  detached: false,
  isMain: false,
  locked: false,
  prunable: false,
};

const activeProject = { id: "p1", name: "repo", path: "/repo", trusted: true, pathOk: true };

async function openMenu() {
  const user = userEvent.setup();
  // Trigger's accessible name is the current worktree label ("main" here).
  await user.click(screen.getByRole("button", { name: "main" }));
  return user;
}

describe("ComposerWorktreeMenu", () => {
  it("submits a new branch name to onCreateWorktree and clears the input", async () => {
    const onCreateWorktree = vi.fn();
    render(
      <ComposerWorktreeMenu
        activeProject={activeProject}
        worktrees={[MAIN, FEATURE]}
        worktreesAvailable
        labels={labels}
        onCreateWorktree={onCreateWorktree}
      />,
    );
    const user = await openMenu();

    const input = screen.getByPlaceholderText("New branch name…");
    await user.type(input, "feature-x");
    await user.click(screen.getByRole("button", { name: /new worktree/i }));

    expect(onCreateWorktree).toHaveBeenCalledWith("feature-x", true);
  });

  it("does not submit an empty branch name", async () => {
    const onCreateWorktree = vi.fn();
    render(
      <ComposerWorktreeMenu
        activeProject={activeProject}
        worktrees={[MAIN]}
        worktreesAvailable
        labels={labels}
        onCreateWorktree={onCreateWorktree}
      />,
    );
    await openMenu();

    expect(
      (screen.getByRole("button", { name: /new worktree/i }) as HTMLButtonElement)
        .disabled,
    ).toBe(true);
    expect(onCreateWorktree).not.toHaveBeenCalled();
  });

  it("shows a remove button only for non-current, non-main worktrees", async () => {
    const onRemoveWorktree = vi.fn();
    render(
      <ComposerWorktreeMenu
        activeProject={activeProject}
        worktrees={[MAIN, FEATURE]}
        worktreesAvailable
        labels={labels}
        onRemoveWorktree={onRemoveWorktree}
      />,
    );
    const user = await openMenu();

    expect(
      screen.queryByRole("button", { name: /remove worktree/i }),
    ).toBeTruthy();
    await user.click(screen.getByRole("button", { name: /remove worktree/i }));
    expect(onRemoveWorktree).toHaveBeenCalledWith(FEATURE);
  });

  it("omits create/remove affordances when the callbacks are not provided", async () => {
    render(
      <ComposerWorktreeMenu
        activeProject={activeProject}
        worktrees={[MAIN, FEATURE]}
        worktreesAvailable
        labels={labels}
      />,
    );
    await openMenu();

    expect(screen.queryByPlaceholderText("New branch name…")).toBeNull();
    expect(
      screen.queryByRole("button", { name: /remove worktree/i }),
    ).toBeNull();
  });
});
