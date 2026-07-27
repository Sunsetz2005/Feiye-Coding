# Plugin marketplace install (design note)

Community ask: install Grok Build plugins from the App without dropping to CLI.

## Current behavior (0.1.3+)

| Action | Where | Effect |
|--------|--------|--------|
| List installed | Settings → Extensions → Plugins | `grok plugin list --json` + inspect enrich |
| Enable / disable | Same UI | CLI + `~/.grok/config.toml` `[plugins].disabled` |
| Details | Modal | `grok plugin details` |
| Uninstall | In-app confirm (GlassModal) | `grok plugin uninstall` |
| **Install from marketplace** | **CLI only** | e.g. `grok plugin install …` |

Skills / MCP enable toggles are App-side (`extensions.json` + ACP inject); plugins follow **CLI/config as source of truth**.

## Why not ship install UI yet

1. **Trust & supply chain** — marketplace install runs third-party code; needs clear origin, version pin, and user confirmation copy (not a silent one-click).
2. **CLI contract** — install flags, auth to marketplaces, and offline failure modes belong to Grok Build; App should wrap, not reimplement.
3. **Scope** — enable/disable/uninstall already covers day-2 management; install is day-1 discovery and needs catalog UX (search, filters, risk badges).

## Proposed product requirements (if we build it)

1. **Catalog**  
   - Source: only what `grok plugin` / inspect can list or a documented marketplace API.  
   - Show name, version, publisher, provides (skills / agents / hooks / MCP counts).

2. **Install flow**  
   - Explicit confirm modal (GlassModal; **no** `window.confirm`).  
   - Progress / error from CLI stderr (redacted).  
   - On success: refresh list + soft-respawn agent (same as enable).

3. **Safety**  
   - Never auto-install.  
   - Prefer pin to version when CLI supports it.  
   - Support zip / Doctor never include marketplace tokens.

4. **i18n**  
   - en + zh + zh-TW for all new strings.

5. **Non-goals (v1)**  
   - Publishing plugins from the App.  
   - Parallel package manager (npm/pip) installs outside `grok plugin`.

## Decision

- **Short term:** keep install CLI-only; document in Extensions footnote (already).  
- **Next:** open Issue when CLI has stable `plugin search` / `plugin install --json` for a clean Host wrapper.  
- **Do not** invent a second plugin store under `~/.grok-app`.
