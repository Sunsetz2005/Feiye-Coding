# Runtime 成熟能力增量迁移 v1

本文是 2026-08 增量迁移的权威边界说明。产品内核归 Sunsetz 所有：

`Tauri 工作台 → Host 会话层 → Sunsetz agent loop（默认）`

默认不再 spawn `grok agent stdio`。Grok ACP 适配器保留在显式 legacy 开关后（`runtimeBackend=grok_acp` / `SUNSETZ_RUNTIME_BACKEND=grok_acp`），本切片不删除。Host 工具是可信项目根内的 `read_file`、`list_directory`、`grep`、`write_file`、`search_replace`、`run_command`，以及父会话的 `spawn_agent` / `agent_output` / `kill_agent`。`grep` 只读。子代理是同一进程内的 `run_turn`，深度上限 1，不 spawn grok、不创建 worktree。explore / plan 只有读工具；general 的写和命令走父会话同一条权限条。写入、替换和命令必须经过现有权限条（或明确自动放行策略）才执行；`AcceptEdits` 只自动放行根内 `write_file` / `search_replace`，不能自动放行 `run_command`。权限预览只有相对路径加字节数或替换次数，或精确命令加相对 cwd，不得带文件正文。相同工具加相同参数连续 3 次会被打断并把错误交回模型。逃出根目录、拒绝和 Stop 不得有副作用。`run_command` 本切片无沙箱，只靠信任根、权限闸、cwd 钉死和 60 秒超时。第三方项目只作为设计与契约研究材料；本轮没有复制 OpenWork `/ee` 或其他受限源码。

## 交互生命周期

`InteractionSnapshotV1` 统一承载 `permission | ask_user | plan`，状态为 `pending | resolving | resolved | failed | interrupted`。Plan 与 Permission 仍是不同语义，只共享生命周期、路由和审计外壳。`approved / executing / done` 属于独立的 `PlanArtifactV1` 产物 schema，不得写入 snapshot status 或 `SessionState`。

- 新事件：`session://interaction`、`session://plan_artifact`。
- 新命令：`session_interactions_list`、`session_resolve_interaction_v1`、只读 `session_plan_artifacts_list_v1`。
- 旧 permission、ask-user、plan 命令和事件保留一个兼容周期。没有 `session_resolve_plan_artifact`。
- 前台和后台会话进入同一 reducer；切换任务或 WebView 重载后，只要原 ACP 进程仍活着，pending 交互可继续处理。批准后的计划正文改从 `plan-artifacts.v1.json` 恢复。
- 回复先以 `interactionId + processId + rpcId` 比较并将状态置为 `resolving`，Runtime 写成功后才清除；写失败恢复 `pending`。重复、过期或跨进程回复被拒绝。
- 每个会话的 `interactions.v1.json` 只保存最多 256 条有界审计快照。完整权限 scope 仅存哈希，ask-user 已提交答案不落盘，敏感键和值做去敏。
- 审计边车不是 RPC 恢复队列。Runtime 进程退出后，未完成项改为 `interrupted`，不能继续回复；Plan sidecar 同样不能把死亡 JSON-RPC 重新显示为待审阅。

## 桌面信任边界

- HTML 预览使用无权限 `sandbox` iframe 和预览专用 CSP。脚本、同源、网络、表单、对象、子框架和顶层导航均被阻断；只允许内联 CSS 与 `data:` / `blob:` 图片、字体和媒体。
- 主 WebView 使用非空 CSP；Tauri 全局 asset protocol 已禁用。
- 本地预览优先使用 `ResourceHandleV1` 与 `resource://`。Host 只为可信项目、应用附件、Runtime 会话产物、已持久化消息附件或原生选择/拖入的文件签发 12 小时不透明句柄。
- 旧 `media://` 仍服务聊天中的图片与媒体，但每次读取都复用相同 provenance 校验；不能再借绝对路径读取任意用户文件。
- `fs_list_dir`、`fs_read_file`、`fs_write_file` 要求精确匹配可信项目根；绝对路径读写要求已授权资源。
- 远程 ACP 只接受字面 loopback 或 `localhost:port`，连接超时 5 秒。非本机历史配置会被拒绝；远端 Runtime 必须由用户建立本地 SSH 端口转发。

## 持久化

`update_json_locked` / `update_text_locked` 的锁覆盖读取、修改、临时文件写入和原子替换。锁侧车保持稳定 inode，防止等待者与新进程锁住不同文件。Unix 在替换前同步临时文件并同步父目录；Windows 使用 `MoveFileExW(REPLACE_EXISTING | WRITE_THROUGH)`。

已迁移的读改写包括 session index、消息 journal、automations、extensions JSON、Runtime `config.toml` 开关和 Settings 字段级更新。`settings_patch_v1` 只接受白名单字段，以同一把锁完成读取、合并和原子替换；前端失败时从 Host 权威值回滚当前字段，并用字段级 sequence 防止较早响应覆盖较新操作。旧 `settings_set` 仅作为 fail-safe 兼容适配，不再允许多字段全量替换。Keychain 偏好切换另由共同事务锁串行化，避免快速开关与回滚交错。JSON 形状不变，旧数据按需读取，无一次性迁移。

## Runtime 能力与事件

- `runtime_capabilities_v1` 返回 Runtime/client/protocol 版本，以及 sandbox、memory、plugin catalog、hooks inventory、MCP 的真实状态；不读取或返回 Secret。
- `session://runtime_event_v1` 与旧 `session://*` 双发。`RuntimeEventEnvelopeV1` 含事件 ID、单会话 sequence、session/agent/process/turn/tool 标识和有界去敏 payload。
- 未识别通知进入 `unknown` envelope；未识别的带 ID 请求仍返回 JSON-RPC method-not-found，不能静默吞掉。
- Sandbox profile 是进程复用键的一部分，修改会回收不匹配 Runtime。默认 `off`；Linux 通过 bubblewrap 实际执行 `workspace_write` / `read_only`。macOS 与 Windows 当前无受支持适配器，请求非 `off` 时 fail-closed，UI 分开显示 requested、applied、verified。
- `HostCapabilities v2` 已纠正已实现的 session/project preview、git summary、resource review 和 app-resident background scheduler 状态。

## 后续能力的当前实现

### 插件

`runtime_plugins_catalog_v1(query)` 提供 Runtime CLI 事实源的只读目录与搜索，`runtime_hooks_inventory_v1` 提供 hooks inventory。本机 Runtime `0.2.111` 只有 list/marketplace list 的稳定 JSON，安装和卸载仍由 Runtime CLI 完成；目录显式返回 `installActionAvailable=false`、`uninstallActionAvailable=false`，旧 Host 缺字段也 fail-closed。legacy `plugin_uninstall` 保留命令兼容但只返回不可用错误，不启动 CLI 或改状态。Runtime 提供唯一目标选择与机器可读 post-state 前，不开放应用内 mutation。

### 会话检索

JSON journal 仍是事实源。`session-search.v1.sqlite3` 是可删除、可重建的 FTS5 缓存，只索引 user/assistant/tool 的可见 `content`，不索引 thought、原始工具 payload 或附件内容。中文查询使用可见文本子串路径。命令：`session_search_v1`、`session_search_rebuild_v1`、`session_search_delete_index_v1`。这不是长期记忆。

### 自学习 Skill

只有完成且实际使用工具的任务才可能生成 Host-owned pending candidate。候选记录来源 session/message、来源内容哈希、审阅内容哈希、所有权和有界去敏审计；用户审阅后才复用 `skill_draft_save` 的原子保存。V2 决策用 expected hash 拒绝陈旧窗口，编辑后的最终草稿另以 final hash 绑定。目标 Skill 的所有权检查、树 hash 和替换位于目标级事务边界内；候选状态提交失败必须回滚或由恢复记录重放。自动流程不能覆盖用户、插件或外部 Skill；覆盖冲突必须再次明确确认。

Host 内核路径在用户显式选择 Skill 后，从库存已经信任的用户/项目/插件 Skill 目录读取有界 `SKILL.md`，按 tree hash 复核后写入该轮模型提示，不写入可见 journal。清单 DTO 仍不含正文或本地路径。默认内核不 spawn grok 二进制，也不把 GROK_HOME 当作产品内核；ACP legacy 路径仍可用 Runtime inspect。Ranking 只是 suggestion，必须用户接受才变成 `accepted_suggestion`。扩展页的 Skill learning 面板只展示本会话使用证据和改进建议，不能直接写 Skill。单轮最多 8 个 Skill、合计 16,000 字；哈希过期、符号链接、逃出可信目录或敏感材料 fail-closed。

### 项目说明文件

信任项目根内按顺序读取第一个普通文件（非符号链接）：`AGENTS.md`、`Sunsetz.md`、`.sunsetz/instructions.md`、`CLAUDE.md`。上限 16,000 字，写入该轮 system 提示，并标明不能覆盖权限或逃出根目录。输入器只显示相对路径和是否截断。读失败、空文件、非 UTF-8 或超限截断不得把会话打成 error。命令：`project_instruction_inspect_v1`。

默认内核单轮最多 16 次工具调用。

### 有界 Memory 候选

`memory-candidates.v1.json` 是独立的待审事实源，只接受 `user_preference | project_fact | workflow_hint`，状态为 `pending | approved | rejected | superseded`。创建必须引用 Host 已持久化的真实 user 消息；内容限制为 2,000 字符、总量限制为 256 条，并在写入前拒绝 API key、token、私钥、带密码数据库 URL 和 JWT 等敏感材料。批准、拒绝、替代和删除都使用内容 hash CAS。

批准只表示用户确认了候选；不会自动注入 Runtime prompt、工具上下文或 FTS，会话检索也不会被称作长期记忆。用户可显式选择已批准且 hash 未变化的候选，通过 `memory_context_pack_build_v1` 构建确定性只读 JSON：最多 8 条、单条 1,000 字、总计 4,000 字。Host 在锁定快照内重新校验 approved、CAS、来源、所有权和敏感材料。选用后输入器显示 `MemoryContextBadge`；`session_send_v2` 把有界 `prompt_fragment` 写入该轮 Sunsetz 内核提示，journal 只留 `memory_injection` marker。注入账本状态为 prepared / dispatching / applied / failed / removed，可反馈或删除。Composer recovery 只存候选 id 与 hash，恢复时重新构建 pack。

### 自动化

Rust Host 每 30 秒通过可独立调用的 `tick_once` 检查到期任务，并在 `automation-runs.v1.json` 中原子认领、设置 10 分钟 lease、记录 claimed/succeeded/failed/interrupted/skipped。每个任务显式保存 `run_once | skip` missed-run policy。WebView 只负责为认领项创建会话并绑定 `claimId → sessionId`；真实 ACP turn 完成后由 Host 结账。重载后的已绑定 claim 不会重复发送。

绑定会话产生真实 Runtime 进度事件时，Host 使用单会话严格递增 sequence 作为 heartbeat 证据，同一 session 最多每 30 秒原子记录一次，并把 lease 重设为 Host 当前时间后 10 分钟。只有 stream、tool call、plan、ask-user、permission、retry、compact 和 usage 算进度；错误、stderr、process exit 与 unknown event 不续租。普通会话没有绑定 claim 时不写账本，新字段均可选，旧 JSON 无需迁移。

Heartbeat 只能证明近期有 Runtime 进度，不能证明过期 Runtime 已终止。第一次进度 heartbeat 会把 claim 钉到 `session_id + process_id`；`process_exited` 才能写入终止证明。无证明的 lease 过期仍只标记 interrupted。有证明且 `missedRunPolicy=run_once` 时，Host 用 CAS 生成恰好一条 replacement claim；`skip` 仍不补跑。已被替换的原 claim 拒绝晚到 completion，避免双跑副作用。应用关闭后的系统服务仍是独立里程碑。

该调度器只在应用进程存活时运行。应用关闭后的系统服务、launchd、Task Scheduler 或 headless 常驻仍是独立里程碑。

### 跨 Agent 共享

`capability_manifest_export_v1` / `capability_manifest_validate_v1` 只交换 metadata contract。清单拒绝源码、prompt、可执行 payload、文件路径、未知字段和无效 hash；实现内容始终省略。

## 已知技术债与发布门禁

1. Windows 物理机仍需验证文件 replace、WebView CSP/resource protocol、loopback ACP、沙箱 fail-closed、200% 缩放与完整键盘路径；没有实机证据不得宣称本阶段发布完成。
2. macOS/Windows Runtime 子进程沙箱适配器尚未实现；默认 `off` 不等于已隔离。
3. 交互式 HTML 容器、应用退出后的系统级自动化、稳定机器可读插件安装/卸载仍未实现。
4. `media://` 是受 provenance 校验的兼容通道；全部调用方迁移到 ResourceHandle 后再删除。
5. 旧 `session://*` 事件至少保留一个完整版本周期；移除必须单独立项并更新契约 golden。
6. SQLite 索引可在崩溃后短暂落后，下一次搜索会按 journal 指纹重建并清理已删除会话；不得把索引当事实源或备份。
7. Memory 对 Sunsetz 内核是显式、可见、可审计注入，不是自动长时记忆；未审阅的 FTS 证据不得进入 pack。信任项目的 `AGENTS.md` / `Sunsetz.md` / `.sunsetz/instructions.md` / `CLAUDE.md` 作为有界项目说明进入 system 提示，符号链接和超限失败则跳过。
8. Automation 已有进程终止证明与 replacement CAS；无证明的过期 claim 仍不重试。系统级常驻调度仍需单独里程碑。

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

本轮自动验收快照：152 个 Tauri command、25 个 event、519 个前端测试、45 个视觉回归用例、319 个 Rust 测试通过（另 1 个夹具生成测试按设计忽略）；改动代码覆盖率为 90.41% 行 / 86.05% 分支，Rust 行覆盖率为 44.39%。
