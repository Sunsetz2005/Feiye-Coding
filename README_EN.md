# Sunsetz

Sunsetz is a local-first desktop agent workbench for projects, sessions, permissions, file and media previews, extensions, accounts, and automations.

![Sunsetz icon](assets/logo.png)

## Highlights

- Multi-project session management, search, pinning, archiving, forks, and rewind
- A Sunsetz three-layer composer for project/goal status, attachments and growing editor content, then access/context/model/send controls
- An in-conversation activity timeline that preserves and groups skill, compaction, file, command, image, browser, and subtask activity in its real order
- Recoverable Agent questions with one-at-a-time navigation, distinct skip/cancel semantics, reload and task-switch recovery, and pending badges for background tasks
- Message-persisted attachment metadata with real native pickers for files, folders, and the current macOS Finder selection
- Ask, one-time, session, deny, and unattended permission policies
- Markdown, code, image, video, PDF, Office, and embedded web previews
- Bottom-docked plan confirmation, execution-step progress, resource-pane plan documents, conversation-to-skill drafts, MCP, plugins, custom providers, and scheduled tasks
- Accounts, quota, activity heatmaps, and profile switching
- English, Simplified Chinese, and Traditional Chinese
- Light, dark, and high-contrast themes

## Development

Requires Node.js 22+, pnpm 9, Rust stable, and Xcode Command Line Tools on macOS.

```bash
pnpm install
pnpm dev
```

Use `SUNSETZ_ACP=mock pnpm dev` for UI development without a live runtime. Set `SUNSETZ_HOME` to override the application data directory.

```bash
pnpm typecheck
pnpm test
pnpm build:ui
pnpm verify:contracts
pnpm test:visual
cd src-tauri && cargo test
```

The private runtime compatibility boundary is documented in [docs/runtime-compatibility.md](docs/runtime-compatibility.md). Workbench and conversation invariants are documented in [docs/llm-wiki/workbench-conversation.md](docs/llm-wiki/workbench-conversation.md).

## License and brand

This codebase is adapted from RongleCat's MIT-licensed desktop workbench snapshot. The original copyright and license remain in [LICENSE](LICENSE). The Sunsetz name and brand assets are not granted under the MIT trademark rights; see [TRADEMARKS.md](TRADEMARKS.md).
