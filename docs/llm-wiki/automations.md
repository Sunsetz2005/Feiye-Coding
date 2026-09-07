# 自动化 / 已安排任务

**状态**：P1 UI + 本地存储 + 对话静默创建 + Host 点火；可选 OS 登录/间隔唤醒。
**原则**：能接 Build 就接 Build；壳层做清单、表单与编排。用户对话不暴露 JSON schema。

## 产品入口（Codex 对标）

| 入口 | 行为 |
|------|------|
| 侧栏 logo 下 **新建会话** | 无项目归属的草稿会话 → 首次发送落入「其他会话」 |
| 侧栏 **已安排** | 主栏打开任务列表（`#/automations`） |
| 列表 **创建 → 用 AI 创建** | 切到无项目会话；composer 预填**自然语言**引导（不展示字段 schema） |
| 列表 **创建 → 手动创建** | 右侧表单：标题 / 指令 / 项目 / 模型 / 推理 / 频率 / 时间 / 通知 |
| Composer「+」→ 创建自动化 | 跳转已安排页 |

## 对话创建协议（静默）

1. 用户用自然语言描述「做什么 + 何时跑」。
2. 发送时 Host 给 Agent 追加**不进 journal 展示**的 setup 前缀（`wrapAutomationSetupAgentText`）。
3. Agent 用自然语言确认；收齐信息后在回复**末尾**附加唯一 fence：

````text
```grok-automation
{"title":"...","prompt":"...","frequency":"daily|weekly|weekdays|once","time":"HH:MM","weekdays":[],"enabled":true}
```
````

4. 壳层在 stream `done` 时 `extractAutomationPayload`：从气泡**剥掉 fence**，调用 `automation_create`，toast「已安排：{title}」。
5. 同一会话只 apply 一次；reload 时也会剥 fence，避免用户看到 JSON。

实现：`src/lib/automationSetup.ts` · 拦截在 `App.tsx` `tryApplyAutomationFromSession`。

## 数据

- 文件：`paths::automations_file()`（macOS 常见：`~/Library/Application Support/dev.sunsetz.desktop/automations.json`）
- 运行账本：`automation-runs.v1.json`（最多 512 条；10 分钟 claim lease；旧行缺少 heartbeat 字段时兼容读取）
- 浏览器兜底：`localStorage["sunsetz.automations"]`
- 字段：`title` `prompt` `enabled` `projectId` `modelId` `effort` `frequency` `time` `weekdays` `missedRunPolicy` `notify` `lastRunAt` `nextRunAt`
- `missedRunPolicy`：`run_once`（默认，只对尚未认领的错过 occurrence 补跑一次）或 `skip`（记录 skipped 并推进计划）。旧数据缺字段时按 `run_once` 读取。

## 执行

1. Rust Host 每 30s 检查 `enabled` 且 `nextRunAt` 到期的任务，并在账本中原子认领；同一 occurrence 只能认领一次。
2. Host 只保留一个 active claim；尚未认领的错过周期按任务的 `run_once | skip` 策略处理，不回放一串历史周期。
3. 默认内核：Host 创建 scheduled 会话、绑定 claim、在后台接通并发送，不抢 live 会话。仍 emit `automation://claim_v1`（带 `sessionId`）让 WebView 刷新列表。legacy ACP 仍由 WebView 空闲后 `session_create` → `automation_claim_bind_v1` → `session_connect` → `session_send`。
4. 绑定后由 Host 依据真实 ACP turn 结果调用账本完成逻辑；“prompt 已发送”不等于成功。
5. Host 从版本化 Runtime event envelope 的 `stream`、`tool_call`、`plan`、`ask_user`、`permission`、`retry_state`、`context_compact` 或 `usage` 识别进度；同一 session 最多每 30 秒将严格递增的 Runtime sequence 写入 `lastRuntimeSessionSequence` / `lastHeartbeatAt`，并把 lease 设为 Host 当前时间后 10 分钟。错误、stderr、进程退出和 unknown event 不续租。
6. WebView 重载时，已绑定 claim 会重新广播但不会再次发送。无进度且 lease 到期的记录为 `interrupted`。heartbeat 钉死 `process_id` 后，`process_exited` 才构成终止证明；有证明且 `run_once` 时 Host CAS 生成一条 replacement。无证明不补跑。已被替换的原 claim 拒绝晚到 completion。
7. **connect 失败**：删除空壳 session并记 failed；**send/turn 失败**：保留会话错误记录并记 failed。
8. 完成后原子推进 `lastRunAt` / `nextRunAt`；`once` 任务禁用。

父会话 `schedule_task` 写入同一份清单（create / list / update / set_enabled / delete）。频率除 daily/weekly/weekdays/once 外还可 `hourly` 与 `interval`（`intervalMinutes` ≥ 15）。可选 `skillIds: [{ id, treeHash }]`：到期时重新校验库存，失败则 `blocked_config`、不调用模型。定时回合禁用该工具。默认内核触发会话由 Host 点火；legacy ACP 仍走 WebView。

与 Build 的 `/loop`、`scheduler_*` 可并存：用户也可在会话里让 Agent 直接调度；壳层清单是独立 SoT。

## UI 约定

- **欢迎态**：仅无 `sessionId` 的草稿空会话。
- **已有 sessionId 但无消息**：提示「此会话暂无消息…」，不显示新建页大牌。
- **删除 / 危险操作**：禁止 `window.confirm`；用应用内弹窗（见 [dialogs.md](./dialogs.md)）。`AutomationsPage` 删除确认即范例。

## 同会话循环（`loop_start` / `loop_cancel`）与本账本的边界

本节机制与上面整节描述的 automation 账本是**两回事**，不要混用：

| | 已安排任务（本节以上） | 同会话循环 |
|---|---|---|
| 存储 | `automations.json` + `automation-runs.v1.json` 磁盘账本 | 纯内存，进程内 `SessionManager::session_loops`（`session_manager/session_loop.rs`） |
| 生命周期 | 跨应用重启存活；由 Host 每 30s 轮询到期认领 | 应用重启或进程退出即消失；不落盘、无 claim/heartbeat/replacement CAS |
| 触发的会话 | 每次到期新建/复用一个独立的 scheduled 会话 | 只绑定当前 `sessionId`，tick 直接在同一会话发起无用户气泡的 wake 回合 |
| 最小间隔 | `intervalMinutes ≥ 15`（`interval` 频率）或 daily/weekly 等日历频率 | `interval_secs`，Host 侧强制 `.max(60)` 地板，不信任模型传入的原始值 |
| 过期 | 不自动过期（`enabled` 手动关闭或 `once` 用完） | 7 天 TTL（`MAX_LOOP_TTL`），到期自动停止；也可 `loop_cancel` 提前停止 |
| 上限 | 无单会话上限（清单式管理） | 每会话最多 `MAX_LOOPS_PER_SESSION`（4）个并发循环 |
| 适用场景 | "每天早上帮我总结邮件" 这类需要独立会话、跨重启持续存在的日程 | "每隔 2 分钟看一眼这个部署有没有完成" 这类绑定当前对话、活到会话结束就够的短期轮询 |

**实现要点**（`session_manager/session_loop.rs`）：

- `loop_start(interval_secs, prompt)`：仅父会话可调用（`spawn_depth == 0`），与 `command_output`/`monitor` 同一收紧规则。返回 `id`，`intervalSecs` 为地板夹紧后的实际值。
- 每个循环有独立的 tokio 计时任务：`sleep(min(interval, 距 7 天 TTL 剩余时间))` 后置位一次 `tick_pending`，再调用 `on_tick` 触发 `SessionManager::maybe_start_wake_turn`——与 `monitor` 的行唤醒复用同一条"跳过 `prepare_user_send`、不写用户气泡"wake-turn 路径。
- `tick_pending` 走的是与 subagent/command-job/monitor 完全相同的"排队 pending → 下次真正唤醒时统一取走并拼进 `wake_context`"模型（`pending_loop_tick_count` / `take_pending_loop_ticks` / `wake_prompt`），因此 `decide_auto_wake` 的"父回合空闲才唤醒"闸门天然覆盖循环，不需要新的决策逻辑。
- 定时器**不因未被消费而停摆**：即使上一次 tick 还没被 drain（父回合仍在流式输出），下一次 tick 到时依然会重新置位 `tick_pending` 并再次尝试唤醒；真正是否打断由共享的 `decide_auto_wake` 决定。
- `loop_cancel(id)` 按 `sessionId` 校验归属——不能取消别的会话的循环；命中后 `Notify::notify_waiters()` 立即唤醒计时任务退出，不必等到下一次 `sleep` 到期。
- Stop/Steer 不取消循环本身（与 `command_jobs.rs` 里 hosted job 的既有约定一致）；应用重启或会话真正消失后，循环因为整个进程内存态注册表被丢弃而自然失效,不需要显式清理钩子。

## Tauri 命令

- `automations_list`
- `automation_create` / `automation_update`
- `automation_set_enabled`
- `automation_mark_run`
- `automation_delete`
- `automation_claim_bind_v1`
- `automation_claim_complete_v1`（兼容 WebView 启动前失败；正常 turn 由 Host 完成）

## 验收

- [x] 侧栏新建会话不依赖当前项目，会话出现在「其他会话」
- [x] 已安排列表 / 筛选 / 搜索 / 启停 / 删除
- [x] 手动表单创建与编辑
- [x] AI 创建入口：自然语言 seed，不暴露 JSON schema
- [x] 助手 fence 自动 `automation_create`，气泡不展示配置块
- [x] 应用打开时到期可触发（不阻塞主对话架构）
- [x] 原子认领、运行账本、单次补跑与 WebView 重载去重
- [x] 每任务 `run_once | skip` missed-run policy；过期 claim 不自动重试
- [x] Runtime 进度 heartbeat 严格按 session/sequence 续租，普通会话与错误事件不写 heartbeat
- [x] connect 失败不留空壳会话；已有空会话不伪装成新建页
- [x] 设置可选登录/间隔 `--background` 唤醒（无 KeepAlive；默认关闭）
- [x] 基于进程终止证明的 replacement CAS；无证明的过期 claim 仍不重试
- [ ] 与 CLI scheduler 双向同步（可选 P2）
