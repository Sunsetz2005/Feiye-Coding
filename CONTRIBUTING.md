# Contributing to Sunsetz

Use Node.js 22+, pnpm 9 or newer, and Rust stable.

```bash
pnpm install
pnpm typecheck
pnpm test
pnpm build:ui
cd src-tauri && cargo test
```

Keep all user-facing strings in the three locale catalogs. Preserve the Tauri command/event contracts and isolate upstream protocol details behind the Runtime compatibility boundary. New UI must use the Sunsetz design tokens and support light, dark, and high-contrast themes.

Never commit credentials, local Runtime homes, support bundles, build output, or private project data.
