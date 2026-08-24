# Sunsetz

[简体中文](README.md)

![CI](https://github.com/Sunsetz2005/Feiye-Coding/actions/workflows/ci.yml/badge.svg)

Sunsetz is a local-first desktop agent workbench. It brings projects, sessions, permissions, plans, file review, extensions, and automations into one native Tauri application.

![Sunsetz icon](assets/logo.png)

> Sunsetz is under active reconstruction. The interface exposes only capabilities explicitly declared available by the Host and Runtime; roadmap items are not presented as shipped features.

## Current capabilities

- Multi-project and multi-session management with search, archive, fork, and rewind.
- Runtime-backed streaming sessions, ordered tool activity, stop, and send queues.
- Ask, allow-once, allow-for-session, deny, and controlled unattended permission policies.
- A three-layer composer with persistent attachments, exact context telemetry, and Runtime-provided models.
- One foreground/background interaction lifecycle for permissions, Agent questions, and plans, recoverable across WebView reloads while the Runtime remains alive.
- Files, Changes, and Plan resources with code, Markdown, image, media, PDF, and Office previews.
- Custom providers, MCP, skills, a read-only Runtime plugin/hooks catalog, accounts, and Rust Host scheduled-task claims backed by Runtime progress heartbeats.
- Rebuildable local session search, bounded Memory candidates sourced from real messages, explicit bounded context-pack export, and review-only Skill candidates; neither candidate system writes to the Runtime automatically.
- English, Simplified Chinese, Traditional Chinese, light, dark, and high-contrast themes.
- `HostCapabilities v2` gating: unknown, unsupported, or uninstalled features stay out of the DOM and keyboard path.

See the [long-running reconstruction status](docs/长期重构-执行状态.md) for verified evidence and remaining gates.

## Architecture

| Layer | Technology and responsibility |
|-------|-------------------------------|
| Desktop Host | Rust and Tauri 2; windows, files, permissions, persistence, and Runtime lifecycle |
| Workbench UI | React 19, TypeScript, and Vite; conversations, composer, resources, and settings |
| Runtime boundary | Versioned capabilities and DTOs with compatibility isolated behind a private adapter |
| Verification | Vitest, Playwright, Rust tests, and Tauri command/event contract scans |

## Local development

Requires Node.js 22+, pnpm 9, and Rust stable. macOS builds also need Xcode Command Line Tools; Windows builds need Visual Studio Build Tools and WebView2.

```bash
pnpm install
pnpm dev
```

Run only the Web UI:

```bash
pnpm dev:ui
```

Use the local mock Runtime:

```bash
SUNSETZ_ACP=mock pnpm dev
```

Set `SUNSETZ_HOME` to override the application data directory.

## Verification

```bash
pnpm verify:contracts
pnpm typecheck
pnpm test
pnpm test:coverage
pnpm coverage:audit
pnpm coverage:changed
pnpm build:ui
pnpm test:visual
cd src-tauri && cargo test
```

See [`coverage-policy.json`](coverage-policy.json) for coverage gates and [`docs/BUILD.md`](docs/BUILD.md) for packaging instructions.

## Security and data

- Never commit tokens, API keys, authentication files, support bundles, or private project contents.
- Project trust and Ask permissions remain enabled by default; unattended operation requires explicit configuration.
- Secrets belong in secure system storage and must not enter journals, previews, skills, logs, or diagnostics.
- Report vulnerabilities privately according to [`SECURITY.md`](SECURITY.md).

## Documentation

- [Workbench and conversation behavior](docs/llm-wiki/workbench-conversation.md)
- [Runtime compatibility boundary](docs/runtime-compatibility.md)
- [Runtime capability migration and technical debt](docs/llm-wiki/runtime-migration-v1.md)
- [Design tokens](docs/design-tokens.md)
- [Long-running reconstruction status](docs/长期重构-执行状态.md)
- [Contributing](CONTRIBUTING.md)

## License and brand

Source code is released under the MIT terms in [`LICENSE`](LICENSE), including the required third-party copyright notice. The Sunsetz name, icon, and brand assets are not granted as trademark rights by the source license; see [`TRADEMARKS.md`](TRADEMARKS.md).
