# Runtime compatibility boundary

Sunsetz presents a product-owned `Sunsetz Runtime` interface. The product kernel is the in-process agent loop in `src-tauri/src/agent_loop.rs`. The private adapter in `src-tauri/src/runtime_compat.rs` preserves compatibility with the upstream `grok` executable, `GROK_HOME`, ACP messages, model IDs, authentication, and quota responses when the explicit legacy flag `runtimeBackend=grok_acp` (or `SUNSETZ_RUNTIME_BACKEND=grok_acp`) is set.

Product-owned settings use `SUNSETZ_HOME` and `SUNSETZ_ACP`. The adapter accepts `GROK_APP_HOME` and `GROK_APP_ACP` as legacy development aliases. Do not expose those upstream identifiers through ordinary UI copy.

Raw upstream terms may appear in opt-in diagnostic bundles or externally controlled authentication pages. They must not be rewritten on the wire.

## Versioned Runtime surface

- `runtime_capabilities_v1` reports Runtime/client/protocol versions and the actual state of sandbox, memory, plugin catalog, hooks inventory, and MCP. It never reads or serializes API keys or authentication material.
- Raw ACP events are dual-emitted as the bounded `session://runtime_event_v1` envelope for one compatibility cycle. Unknown notifications remain observable; unknown requests are rejected on the wire.
- Sandbox profile is spawn-critical. `off` is the default. The built-in kernel uses Linux bubblewrap, macOS sandbox-exec, or Windows AppContainer for each `run_command`. The legacy ACP adapter still fail-closes non-off profiles except on Linux.
- TCP ACP accepts only loopback endpoints. Use a user-managed local SSH tunnel for a Runtime on another machine.

The detailed migration and remaining platform gates are documented in [`llm-wiki/runtime-migration-v1.md`](llm-wiki/runtime-migration-v1.md).

## Legacy identifier allowlist

The following identifiers remain only to preserve existing user data or import compatibility:

- `GROK_APP_HOME`, `GROK_APP_ACP`, and `grok-app.theme`;
- the historical keyring service `com.grokapp.grok-app`;
- the internal `import_grok_go_config` command and its known read-only source paths.

These identifiers must not appear as Sunsetz product branding, repository metadata, ordinary UI labels, screenshots, release names, or support links. New persisted data uses the Sunsetz application identifier and `SUNSETZ_HOME`.
