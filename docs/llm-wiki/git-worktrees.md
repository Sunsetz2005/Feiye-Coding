# Git worktrees

Worktree behavior is tracked as part of the Sunsetz project lifecycle.

## Behavior

When the active project path is a git work tree, the composer **project chip** menu lists linked worktrees from:

```bash
git worktree list --porcelain
```

- Selecting a worktree binds the open session (or draft context) to that path as agent **cwd**.
- If the path is already a project, switch only; otherwise `project_add` (trust inherited from the current project when possible).
- Entering a branch name in the project menu creates a new branch and a sibling worktree directory named `<project>-<branch-slug>`, then switches to it.
- A linked worktree can be removed from the project menu after an in-app confirmation. The main worktree and the current worktree are not removable there. Dirty or locked worktrees require a second explicit force-removal confirmation because Git will discard their local changes.
- Soft-fail when `git` is missing or the folder is not a repo (same spirit as Workspace Changes git status).
- **UI:** section is hidden until host confirms `available: true` (non-git / loading → no “GIT WORKTREES” block). Rows match project list height; branch + badges on one line.

Create/remove is a human-triggered UI flow. It is not exposed as an Agent tool, so in-process subagents still never create worktrees.

## Non-goals

- Full branch browser
- Remote worktree provisioning

## Implementation

- Host: `git_worktrees_list`, `git_worktree_add`, `git_worktree_remove` (`src-tauri/src/commands.rs`)
- Pure parse/path helpers: `src/lib/gitWorktree.ts` (+ unit tests)
- UI: `ComposerProjectMenu` worktrees section; `App.tsx` owns confirmation, project registration, and switching
