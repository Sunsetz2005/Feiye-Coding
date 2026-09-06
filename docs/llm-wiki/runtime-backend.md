# Runtime 内核与 Grok ACP 适配器

产品内核是 Host 内建 `agent_loop`。Grok ACP 适配器（`src-tauri/src/acp_client.rs`）不是默认 Runtime，也不能在本兼容周期删除。

## 当前决策（2026-09-04）

**保留显式 legacy 开关，不删除适配器。** 完成一个完整版本周期、补齐仍只存在于 ACP 路径的能力、并单独立项迁移之后，才评估删除。

必须保留的开关：

- 设置 `runtimeBackend=grok_acp`
- 进程环境 `SUNSETZ_RUNTIME_BACKEND=grok_acp`（或 `grok_agent_stdio`）

开发桩是另一条路径，不是适配器本身：`SUNSETZ_ACP=mock`（及遗留别名 `GROK_APP_ACP=mock`）。它会盖过设置，必须在 Runtime 页标明，不能伪装成产品内核。

## 解析顺序

`resolve_backend` / `current_backend_report` 的优先级：

1. `SUNSETZ_ACP=mock`（或遗留 `GROK_APP_ACP=mock`）→ 有效内核 `mock_acp`，覆盖来源 `sunsetz_acp`
2. 非空 `SUNSETZ_RUNTIME_BACKEND` → 有效内核按该变量解析，覆盖来源 `sunsetz_runtime_backend`
3. 已保存 `runtimeBackend` → `sunsetz`（默认）或 `grok_acp`

Host 通过 `runtime_capabilities_v1.kernel` 同时报告 `stored`、`effective` 和 `overrideSource`。设置 → Runtime 显示**有效**内核；当环境变量盖过设置时，禁止把切换按钮画成立即生效，并写出已保存偏好。

`SUNSETZ_ACP=mock pnpm dev` 仍是 UI 烟测入口。真实本机工作必须去掉 mock，并在「我的模型」配置接口。

## 本周期为什么不能删除 ACP

这些路径仍接在适配器上，删除会破坏显式 legacy 用户和契约：

- `session_manager` 在非 mock、非 sunsetz 时仍 `connect` 到 `grok agent stdio`，含 TCP loopback `acpServerAddr`
- 计划产物仍来自 `AcpEvent::Plan`；内建循环还不发 Plan
- 自动化在 ACP 路径仍由 WebView 点火；默认内核已改 Host 点火
- 插件目录 / hooks / MCP 能力探测仍可读 Runtime CLI；应用内安装/卸载保持 fail-closed
- 握手、SPIKE 与 golden 仍覆盖 ACP 兼容边界
- 旧 `session://*` 双发与 `settings_set` 另有自己的兼容周期，不能绑在一次 ACP 删除里

私有兼容层仍是 `src-tauri/src/runtime_compat.rs` 与 [`runtime-compatibility.md`](../runtime-compatibility.md)。协议标识、模型 ID、可执行文件名、上游环境变量和 wire value 不得在适配器外重命名。

## 以后删除必须满足的条件

单独立项，且同时满足：

1. 至少一个已发布版本默认 `sunsetz`，legacy 仅显式开关（`1.0.1` 已开始这一周期，但周期尚未结束）。
2. 产品内核覆盖仍只存在于 ACP 的支持路径，或产品明确放弃：远程 loopback ACP、grok `session/load` 热复用、CLI 压缩、ACP 计划事件。
3. 设置页、Doctor、契约测试不再把 grok 二进制当作默认依赖。
4. 仍开着 `grok_acp` 的用户有版本内提示和一键改回内建内核。
5. 删除 `acp_client.rs` 的 spawn 路径后，`AcpEvent` 作为 UI 事件面可以保留；不得把 mock 开发桩一起删掉。
6. 更新 `AGENTS.md`、本页、`runtime-compatibility.md` 与契约 golden。

未满足前，Runtime 页可以继续把内建内核放在前面，并把 ACP/CLI 折进旧版区。

## 不要做的事

- 不要在本切片删除 `acp_client.rs`、握手或 `runtime_compat` 标识。
- 不要把 `SUNSETZ_ACP=mock` 当成产品 Agent，也不要让设置切换在该变量存在时假装生效。
- 不要引入 GROK_HOME、grok 二进制或 Rhai workflow 作为产品内核。
- 不要把旧 `session://*` 事件或 `settings_set` 的移除绑进 ACP 删除。
