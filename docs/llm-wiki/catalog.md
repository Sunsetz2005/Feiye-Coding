# Sunsetz Runtime 对齐：模型 / 推理 / 权限 / 模式

源码：`src/lib/grokCatalog.ts`（静态兜底）、`src-tauri/src/models_catalog.rs`、`src-tauri/src/session_manager.rs`、`src-tauri/src/agent_prefs.rs`。

## 模型与 Runtime capability

UI 只展示 `models_list_available` 返回的可用模型。服务商是后端渠道，只在设置 → 账户 → 自定义提供商切换，不进入 composer 模型菜单。

| 来源 | 说明 |
|------|------|
| Runtime `models_cache.json` | 当前官方目录与默认模型 |
| `AvailableModel.capabilities` | 当前模型真实支持的推理档位 |
| 静态兜底 | 缓存不可用时使用已知可工作的 `grok-4.5` |

Host：`models_list_available`。连接参数为：

```text
grok agent --model <id> --reasoning-effort <e> [--always-approve] stdio
```

Flags 必须在 `stdio` 之前。连接后 Host 用 `session/set_model` 对齐实际模型。

### Composer 模型入口

- 一级菜单：模型、Runtime 声明时的推理强度、重置模型/推理默认值。
- 二级菜单从当前行侧边展开，空间不足时翻转。
- 当前没有独立 speed capability，因此不显示速度行。
- 运行中、等待权限、等待 Agent 问题或计划确认时可以查看，不能修改。

## 推理强度

UI 不再假定所有模型都有 effort。只有当前 `AvailableModel.capabilities.reasoningEfforts` 返回档位时才显示；当前已知能力为 `low` / `medium` / `high`，默认值为 `medium`。

Runtime 没有 mid-session `set_effort` RPC。Host 更新目标 effort 后软断开当前 Agent，下一次连接以新的 `--reasoning-effort` 重建。

## H01 / H02 live apply 与回滚

- **H01 模型**：UI 先乐观更新；`composer_prefs_set` 调用 `SessionManager::set_model`，当前 ACP `session/set_model` 成功后才保存 scoped preference。失败时 UI 用 `rollbackOptimisticSetting` 恢复上一个值并显示错误。
- **H02 effort**：UI 只接受 Runtime capability 中的合法档位；Host 用 `set_effort_and_respawn_needed` 把变更应用到真实重连路径，再保存 preference。Host 返回错误时 UI 对称回滚。
- 重置默认值也走相同 live apply / rollback，只重置模型和推理，不修改权限或项目。

## 会话模式

| App | 作用 |
|-----|------|
| `agent` | 默认编码 Agent |
| `plan` | 计划模式（ACP `session/set_mode`） |
| `ask` | 询问 / 偏只读协作 |

连接成功后尝试 `session/set_mode`。中途切换优先 live apply，Runtime 拒绝时走 soft-respawn。模式按 `composerPrefsScope` 记忆。

## 权限

| App ID | Agent 配置 | Spawn |
|--------|------------|-------|
| `ask` | `default` | — |
| `accept_edits` | `acceptEdits` | — |
| `allow_for_session` | `default` + Host 会话缓存 | — |
| `dont_ask` | `dontAsk` | — |
| `always_approve` | `always-approve` / `bypassPermissions` | `--always-approve` |

默认权限是 Ask。Host 继续使用 Runtime optionId 和 scopeKey；完全允许保留二次确认。权限变化会同步 Agent profile，并在进程参数可能变化时 soft-respawn。

## 偏好记忆范围

`composerPrefsScope` = `global` | `project` | `session`，覆盖 model / effort / mode / permission。当前 composer 使用独立的模型级联菜单与访问菜单，不再使用旧“闪电合并 chip”。

## 上下文

上下文环只展示 Runtime 精确上报的数据；没有容量时显示未知，不按消息长度估算。弹层提供真实 `/compact` 入口，并受同一 busy-state 锁控制。
