# Runtime compatibility boundary

Sunsetz presents a product-owned `Sunsetz Runtime` interface. The private adapter in `src-tauri/src/runtime_compat.rs` preserves compatibility with the upstream `grok` executable, `GROK_HOME`, ACP messages, model IDs, authentication, and quota responses.

Product-owned settings use `SUNSETZ_HOME` and `SUNSETZ_ACP`. The adapter accepts `GROK_APP_HOME` and `GROK_APP_ACP` as legacy development aliases. Do not expose those upstream identifiers through ordinary UI copy.

Raw upstream terms may appear in opt-in diagnostic bundles or externally controlled authentication pages. They must not be rewritten on the wire.
