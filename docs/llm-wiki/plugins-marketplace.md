# Plugin marketplace install (design note)

Community ask: install Grok Build plugins from the App without dropping to CLI.

## Current behavior (0.1.3+)

| Action | Where | Effect |
|--------|--------|--------|
| Read-only catalog + search | Settings → Extensions → Plugins | `runtime_plugins_catalog_v1(query)`；Runtime CLI inventory + inspect enrich |
| Hooks inventory | Same UI | `runtime_hooks_inventory_v1` |
| Enable / disable | Same UI | CLI + `~/.grok/config.toml` `[plugins].disabled` |
| Details | Modal | `grok plugin details` |
| Uninstall | **CLI only** | App 按钮禁用；legacy `plugin_uninstall` fail-closed |
| **Install from marketplace** | **CLI only** | e.g. `grok plugin install …` |

Catalog 明确返回 `installActionAvailable=false`、`uninstallActionAvailable=false`。字段缺失也按 false 处理。Skills / MCP enable toggles are App-side (`extensions.json` + ACP inject); plugins follow **CLI/config as source of truth**.

## Why not ship install UI yet

1. **Trust & supply chain** — marketplace install runs third-party code; needs clear origin, version pin, and user confirmation copy (not a silent one-click).
2. **CLI contract** — 本机验证的 Runtime `0.2.111` 只有 `plugin list --json` 与 `plugin marketplace list --json` 提供稳定 JSON；install、uninstall、update、enable、disable 和 details 都是面向人的输出。
3. **Uninstall target** — 当前 Runtime 只按 plugin name 选择目标，可能删除包含该插件的整个 repository；同名插件、连带删除集合和超时后的子进程状态均无法由 App 可靠证明。

## Proposed product requirements (if we build it)

1. **Catalog**  
   - Source: only what `grok plugin` / inspect can list or a documented marketplace API.  
   - Show name, version, publisher, provides (skills / agents / hooks / MCP counts).

2. **Mutation flow**
   - Explicit confirm modal (GlassModal; **no** `window.confirm`).  
   - 先返回 preview：唯一 repo key、inventory hash、连带插件集合和预计动作。
   - 执行时重新比较 preview，并在超时后终止子进程；成功后比较机器可读 post-state。
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

- **Short term:** read-only catalog/search/hooks inventory 已接入；install 和 uninstall 继续 CLI-only。legacy `plugin_uninstall` 保留契约但恒定返回 `PLUGIN_UNINSTALL_UNAVAILABLE`，不会启动 CLI、写状态或 soft-respawn。
- **Next:** Runtime 提供唯一目标选择和稳定机器可读 mutation/post-state 后，再设计 `preview_v1 + mutation_v2` Host 契约。
- **Do not** invent a second plugin store under the Sunsetz application data directory.
