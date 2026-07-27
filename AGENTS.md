# Agent notes — Sunsetz

## Product boundary

- Product name, window chrome, icons, ordinary logs, documentation, and UI copy use **Sunsetz**.
- User-facing runtime copy uses **Sunsetz Runtime** and paid membership copy uses **Sunsetz Pro**.
- The private compatibility boundary is `src-tauri/src/runtime_compat.rs` and `docs/runtime-compatibility.md`. Protocol identifiers, model IDs, executable names, upstream environment variables, and wire values must not be renamed outside a tested adapter.
- All user-visible strings go through `src/i18n/`; keep English, Simplified Chinese, and Traditional Chinese catalogs in sync.
- Do not use browser `alert`, `confirm`, or `prompt`; use the existing in-app dialogs.

## Product rules

- Preserve the Tauri command/event interfaces, session state machine, permission semantics, and persisted data shapes.
- The application has light, dark, and high-contrast themes. New surfaces must use tokens from `src/styles/tokens.css`.
- Keep Sunsetz coral as the single product accent. Editor, media, and code surfaces stay opaque; glass is limited to navigation, composer, menus, and dialogs.
- Workbench and conversation changes must preserve the invariants in [`docs/llm-wiki/workbench-conversation.md`](docs/llm-wiki/workbench-conversation.md).
- App icons and tray icons are separate assets. Regenerate both when the source mark changes.
- Never commit credentials, auth files, local runtime homes, or support bundles.

## Verification

Run `pnpm typecheck`, `pnpm test`, `pnpm build:ui`, `pnpm verify:contracts`, `pnpm test:visual`, and `cargo test` before release work. Use `SUNSETZ_ACP=mock pnpm dev` for UI-only smoke tests. Only update Playwright snapshots after manually confirming the visual change.
