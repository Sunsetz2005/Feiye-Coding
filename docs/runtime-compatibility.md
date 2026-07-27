# Runtime compatibility boundary

Sunsetz presents a product-owned `Sunsetz Runtime` interface. The private adapter in `src-tauri/src/runtime_compat.rs` preserves compatibility with the upstream `grok` executable, `GROK_HOME`, ACP messages, model IDs, authentication, and quota responses.

Product-owned settings use `SUNSETZ_HOME` and `SUNSETZ_ACP`. The adapter accepts `GROK_APP_HOME` and `GROK_APP_ACP` as legacy development aliases. Do not expose those upstream identifiers through ordinary UI copy.

Raw upstream terms may appear in opt-in diagnostic bundles or externally controlled authentication pages. They must not be rewritten on the wire.

## Legacy identifier allowlist

The following identifiers remain only to preserve existing user data or import compatibility:

- `GROK_APP_HOME`, `GROK_APP_ACP`, and `grok-app.theme`;
- the historical keyring service `com.grokapp.grok-app`;
- the internal `import_grok_go_config` command and its known read-only source paths.

These identifiers must not appear as Sunsetz product branding, repository metadata, ordinary UI labels, screenshots, release names, or support links. New persisted data uses the Sunsetz application identifier and `SUNSETZ_HOME`.
