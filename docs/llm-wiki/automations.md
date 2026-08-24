# 自动化 / 已安排任务

**状态**：P1 UI + 本地存储 + 对话静默创建 + 应用进程存活时 Rust Host 调度。
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
3. WebView 空闲后执行：`session_create` → `automation_claim_bind_v1` → 写 session prefs → `session_connect` → `session_send`。
4. 绑定后由 Host 依据真实 ACP turn 结果调用账本完成逻辑；“prompt 已发送”不等于成功。
5. Host 从版本化 Runtime event envelope 的 `stream`、`tool_call`、`plan`、`ask_user`、`permission`、`retry_state`、`context_compact` 或 `usage` 识别进度；同一 session 最多每 30 秒将严格递增的 Runtime sequence 写入 `lastRuntimeSessionSequence` / `lastHeartbeatAt`，并把 lease 设为 Host 当前时间后 10 分钟。错误、stderr、进程退出和 unknown event 不续租。
6. WebView 重载时，已绑定 claim 会重新广播但不会再次发送。无进度且 lease 到期的记录为 `interrupted`；heartbeat 只证明近期 Runtime 进度，仍不会自动生成 replacement claim。晚到 completion 只能结算原 claim，且重复完成 fail-closed。
7. **connect 失败**：删除空壳 session并记 failed；**send/turn 失败**：保留会话错误记录并记 failed。
8. 完成后原子推进 `lastRunAt` / `nextRunAt`；`once` 任务禁用。

与 Build 的 `/loop`、`scheduler_*` 可并存：用户也可在会话里让 Agent 直接调度；壳层清单是独立 SoT。

## UI 约定

- **欢迎态**：仅无 `sessionId` 的草稿空会话。
- **已有 sessionId 但无消息**：提示「此会话暂无消息…」，不显示新建页大牌。
- **删除 / 危险操作**：禁止 `window.confirm`；用应用内弹窗（见 [dialogs.md](./dialogs.md)）。`AutomationsPage` 删除确认即范例。

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
- [ ] 后台无窗口常驻触发（可选 P2：系统服务 / headless CLI）
- [ ] 基于 heartbeat 的安全 replacement/retry；当前只续租，仍保持过期不重试
- [ ] 与 CLI scheduler 双向同步（可选 P2）
