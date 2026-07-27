# Git worktrees

Community request: [issue #42](https://github.com/RongleCat/grok-app/issues/42).

## Behavior

When the active project path is a git work tree, the composer **project chip** menu lists linked worktrees from:

```bash
git worktree list --porcelain
```

- Selecting a worktree binds the open session (or draft context) to that path as agent **cwd**.
- If the path is already a project, switch only; otherwise `project_add` (trust inherited from the current project when possible).
- Soft-fail when `git` is missing or the folder is not a repo (same spirit as Workspace Changes git status).
- **UI:** section is hidden until host confirms `available: true` (non-git / loading → no “GIT WORKTREES” block). Rows match project list height; branch + badges on one line.

## Non-goals (MVP)

- Creating or removing worktrees from the App
- Full branch browser

## Implementation

- Host: `git_worktrees_list` (`src-tauri/src/commands.rs`)
- Pure parse helpers: `src/lib/gitWorktree.ts` (+ unit tests)
- UI: `ComposerProjectMenu` worktrees section
