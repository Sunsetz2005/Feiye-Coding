# Runtime 成熟能力增量迁移 v1

本文是 2026-08 增量迁移的权威边界说明。架构保持单内核：

`Tauri Host → ACP → Sunsetz Runtime`

不得在应用内引入第二套 Agent Loop、工具执行器或 Provider 调用栈。第三方项目只作为设计与契约研究材料；本轮没有复制 OpenWork `/ee` 或其他受限源码。

## 交互生命周期

`InteractionSnapshotV1` 统一承载 `permission | ask_user | plan`，状态为 `pending | resolving | resolved | failed | interrupted`。Plan 与 Permission 仍是不同语义，只共享生命周期、路由和审计外壳。

- 新事件：`session://interaction`。
- 新命令：`session_interactions_list`、`session_resolve_interaction_v1`。
- 旧 permission、ask-user、plan 命令和事件保留一个兼容周期。
- 前台和后台会话进入同一 reducer；切换任务或 WebView 重载后，只要原 ACP 进程仍活着，pending 交互可继续处理。
- 回复先以 `interactionId + processId + rpcId` 比较并将状态置为 `resolving`，Runtime 写成功后才清除；写失败恢复 `pending`。重复、过期或跨进程回复被拒绝。
- 每个会话的 `interactions.v1.json` 只保存最多 256 条有界审计快照。完整权限 scope 仅存哈希，ask-user 已提交答案不落盘，敏感键和值做去敏。
- 审计边车不是 RPC 恢复队列。Runtime 进程退出后，未完成项改为 `interrupted`，不能继续回复。

## 桌面信任边界

- HTML 预览使用无权限 `sandbox` iframe 和预览专用 CSP。脚本、同源、网络、表单、对象、子框架和顶层导航均被阻断；只允许内联 CSS 与 `data:` / `blob:` 图片、字体和媒体。
- 主 WebView 使用非空 CSP；Tauri 全局 asset protocol 已禁用。
- 本地预览优先使用 `ResourceHandleV1` 与 `resource://`。Host 只为可信项目、应用附件、Runtime 会话产物、已持久化消息附件或原生选择/拖入的文件签发 12 小时不透明句柄。
- 旧 `media://` 仍服务聊天中的图片与媒体，但每次读取都复用相同 provenance 校验；不能再借绝对路径读取任意用户文件。
- `fs_list_dir`、`fs_read_file`、`fs_write_file` 要求精确匹配可信项目根；绝对路径读写要求已授权资源。
- 远程 ACP 只接受字面 loopback 或 `localhost:port`，连接超时 5 秒。非本机历史配置会被拒绝；远端 Runtime 必须由用户建立本地 SSH 端口转发。

## 持久化

`update_json_locked` / `update_text_locked` 的锁覆盖读取、修改、临时文件写入和原子替换。锁侧车保持稳定 inode，防止等待者与新进程锁住不同文件。Unix 在替换前同步临时文件并同步父目录；Windows 使用 `MoveFileExW(REPLACE_EXISTING | WRITE_THROUGH)`。

已迁移的读改写包括 session index、消息 journal、automations、extensions JSON 和 Runtime `config.toml` 开关。JSON 形状不变，旧数据按需读取，无一次性迁移。

## Runtime 能力与事件

- `runtime_capabilities_v1` 返回 Runtime/client/protocol 版本，以及 sandbox、memory、plugin catalog、hooks inventory、MCP 的真实状态；不读取或返回 Secret。
- `session://runtime_event_v1` 与旧 `session://*` 双发。`RuntimeEventEnvelopeV1` 含事件 ID、单会话 sequence、session/agent/process/turn/tool 标识和有界去敏 payload。
- 未识别通知进入 `unknown` envelope；未识别的带 ID 请求仍返回 JSON-RPC method-not-found，不能静默吞掉。
- Sandbox profile 是进程复用键的一部分，修改会回收不匹配 Runtime。默认 `off`；Linux 通过 bubblewrap 实际执行 `workspace_write` / `read_only`。macOS 与 Windows 当前无受支持适配器，请求非 `off` 时 fail-closed，UI 分开显示 requested、applied、verified。
- `HostCapabilities v2` 已纠正已实现的 session/project preview、git summary、resource review 和 app-resident background scheduler 状态。

## 后续能力的当前实现

### 插件

`runtime_plugins_catalog_v1(query)` 提供 Runtime CLI 事实源的只读目录与搜索，`runtime_hooks_inventory_v1` 提供 hooks inventory。安装仍由 Runtime CLI 完成，`installActionAvailable=false`；在 CLI 提供稳定机器可读安装结果前，不增加一键安装。

### 会话检索

JSON journal 仍是事实源。`session-search.v1.sqlite3` 是可删除、可重建的 FTS5 缓存，只索引 user/assistant/tool 的可见 `content`，不索引 thought、原始工具 payload 或附件内容。中文查询使用可见文本子串路径。命令：`session_search_v1`、`session_search_rebuild_v1`、`session_search_delete_index_v1`。这不是长期记忆。

### 自学习 Skill

只有完成且实际使用工具的任务才可能生成 Host-owned pending candidate。候选记录来源 session/message、内容哈希和所有权；用户审阅后才复用 `skill_draft_save` 的原子保存。自动流程不能覆盖用户、插件或外部 Skill；覆盖冲突必须再次明确确认。

### 自动化

Rust Host 每 30 秒检查到期任务，并在 `automation-runs.v1.json` 中原子认领、设置 10 分钟 lease、记录 scheduled/claimed/succeeded/failed/interrupted 和一次 catch-up。WebView 只负责为认领项创建会话并绑定 `claimId → sessionId`；真实 ACP turn 完成后由 Host 结账。重载后的已绑定 claim 不会重复发送。

该调度器只在应用进程存活时运行。应用关闭后的系统服务、launchd、Task Scheduler 或 headless 常驻仍是独立里程碑。

### 跨 Agent 共享

`capability_manifest_export_v1` / `capability_manifest_validate_v1` 只交换 metadata contract。清单拒绝源码、prompt、可执行 payload、文件路径、未知字段和无效 hash；实现内容始终省略。

## 已知技术债与发布门禁

1. Windows 物理机仍需验证文件 replace、WebView CSP/resource protocol、loopback ACP、沙箱 fail-closed、200% 缩放与完整键盘路径；没有实机证据不得宣称本阶段发布完成。
2. macOS/Windows Runtime 子进程沙箱适配器尚未实现；默认 `off` 不等于已隔离。
3. 交互式 HTML 容器、应用退出后的系统级自动化、稳定机器可读插件安装仍未实现。
4. `media://` 是受 provenance 校验的兼容通道；全部调用方迁移到 ResourceHandle 后再删除。
5. 旧 `session://*` 事件至少保留一个完整版本周期；移除必须单独立项并更新契约 golden。
6. SQLite 索引可在崩溃后短暂落后，下一次搜索会按 journal 指纹重建并清理已删除会话；不得把索引当事实源或备份。

## 验证入口

每次修改本契约至少运行：

```bash
pnpm verify:contracts
pnpm typecheck
pnpm test
pnpm build:ui
pnpm test:visual
cd src-tauri && cargo test
```

覆盖率门禁另运行 `pnpm test:coverage`、`pnpm coverage:changed` 和 Rust coverage audit。Windows 发布还必须执行仓库中的物理机验收脚本。

本轮自动验收快照：141 个 Tauri command、25 个 event、501 个前端测试、45 个视觉回归用例、272 个 Rust 测试通过（另 1 个夹具生成测试按设计忽略）；改动代码覆盖率为 80.42% 行 / 73.93% 分支，Rust 行覆盖率为 38.73%。
