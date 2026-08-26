# Agent notes — Sunsetz

## Product boundary

- Product name, window chrome, icons, ordinary logs, documentation, and UI copy use **Sunsetz**.
- User-facing runtime copy uses **Sunsetz Runtime** and paid membership copy uses **Sunsetz Pro**.
- Sunsetz owns the agent kernel. The product runtime is the in-process loop in `src-tauri/src/agent_loop.rs`, not a spawned `grok agent stdio` process. Keep the Tauri workbench.
- Default `runtimeBackend` is `sunsetz`. The Grok ACP adapter in `acp_client.rs` stays behind an explicit legacy flag (`runtimeBackend=grok_acp` or `SUNSETZ_RUNTIME_BACKEND=grok_acp`) and must not be deleted without a dedicated slice.
- The private compatibility boundary for the legacy adapter is `src-tauri/src/runtime_compat.rs` and `docs/runtime-compatibility.md`. Protocol identifiers, model IDs, executable names, upstream environment variables, and wire values must not be renamed outside a tested adapter.
- All user-visible strings go through `src/i18n/`; keep English, Simplified Chinese, and Traditional Chinese catalogs in sync.
- Do not use browser `alert`, `confirm`, or `prompt`; use the existing in-app dialogs.

## Product rules

- Preserve the Tauri command/event interfaces, session state machine, permission semantics, and persisted data shapes.
- The setup wizard and home route must not require a Grok CLI binary. Host tools are `read_file`, `list_directory`, `write_file`, and `run_command` inside a trusted project root. Writes and commands go through the existing permission dock; `AcceptEdits` auto-allows in-root writes only and never commands. Explicit Skills attach bounded `SKILL.md` text from Host-trusted skill directories; the product kernel remains `agent_loop`, not GROK_HOME or a grok binary. This slice does not add sandbox.
- The application has light, dark, and high-contrast themes. New surfaces must use tokens from `src/styles/tokens.css`.
- Keep Sunsetz coral as the single product accent. Editor, media, and code surfaces stay opaque; glass is limited to navigation, composer, menus, and dialogs.
- Workbench and conversation changes must preserve the invariants in [`docs/llm-wiki/workbench-conversation.md`](docs/llm-wiki/workbench-conversation.md).
- App icons and tray icons are separate assets. Regenerate both when the source mark changes.
- Never commit credentials, auth files, local runtime homes, or support bundles.

## Verification

Run `pnpm typecheck`, `pnpm test`, `pnpm build:ui`, `pnpm verify:contracts`, `pnpm test:visual`, and `cargo test` before release work. The app and Rust tests must not require a `grok` binary. Use `SUNSETZ_ACP=mock pnpm dev` for UI-only smoke tests and `SUNSETZ_RUNTIME_BACKEND=grok_acp` only when exercising the legacy ACP adapter. Only update Playwright snapshots after manually confirming the visual change.

Coverage policy is versioned in `coverage-policy.json`. Run `pnpm test:coverage`, `pnpm coverage:audit`, and `pnpm coverage:changed` for the current frontend baseline and enforced changed-code gate. Rust coverage remains audit-only; do not describe the final 80% line / 70% branch targets as achieved until the corresponding final audits pass.

Physical Windows stage-one evidence uses `scripts/windows-stage1-manual.ps1`; CI native smoke is additional evidence, not a substitute.

## Changelog

- Keep `## [UNRELEASED] — YYYY-MM-DD HH:mm` as the first and empty version section in `CHANGELOG.md`.
- Add each release immediately below it as `## [X.Y.Z] — YYYY-MM-DD HH:mm`, newest first.
- Write every change as one short sentence describing the delivered result; omit implementation detail and promotional copy.
- Keep the SemVer value synchronized across the frontend manifest, Tauri manifest, Cargo manifest, lockfile, and localized version footer.

## Obsidian knowledge base maintenance

- The project handoff knowledge base is `/Users/a10954/Documents/Documents - Sunsetz的MacBook Pro/Obsidian Vault/Sunsetz/`; its entry point is `Sunsetz 项目知识库 MOC.md`.
- At the start of every AI development task, read `00-AI接手总览.md`, `01-当前状态.md`, `03-未来开发规划.md`, and any topic note relevant to the change, then run `scripts/sync-obsidian-knowledge.sh`.
- Before finishing any task that changes code, configuration, tests, release state, deployment assumptions, or product decisions, update the affected knowledge-base notes. Always update `01-当前状态.md`; update `02-已完成开发进度.md`, `03-未来开发规划.md`, `08-已知问题与技术债.md`, and `09-变更时间线.md` when their facts change. Run the sync script again after the edits.
- Git hooks in `.githooks/` refresh the objective mirrors after commit, checkout, and merge. Keep local `core.hooksPath` set to `.githooks`; reinstall with `scripts/install-obsidian-hooks.sh` if needed.
- Treat repository code, tests, Git history, `CHANGELOG.md`, and `docs/长期重构-执行状态.md` as evidence. Never turn an intention into a completed claim, and record the commit or verification source for completion claims.
- The Vault may record server IP addresses, login user names, and private-key file locations, but never private-key contents, passwords, tokens, cookies, `.env` values, or database connection secrets.
