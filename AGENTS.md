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

Coverage policy is versioned in `coverage-policy.json`. Run `pnpm test:coverage`, `pnpm coverage:audit`, and `pnpm coverage:changed` for the current frontend baseline and enforced changed-code gate. Rust coverage remains audit-only; do not describe the final 80% line / 70% branch targets as achieved until the corresponding final audits pass.

## Changelog

- Keep `## [UNRELEASED] — YYYY-MM-DD HH:mm` as the first and empty version section in `CHANGELOG.md`.
- Add each release immediately below it as `## [X.Y.Z] — YYYY-MM-DD HH:mm`, newest first.
- Write every change as one short sentence describing the delivered result; omit implementation detail and promotional copy.
- Keep the SemVer value synchronized across the frontend manifest, Tauri manifest, Cargo manifest, lockfile, and localized version footer.
