# Workbench 与会话交互

这是 Sunsetz 当前工作台与会话交互的权威说明。设计稿、截图和实施计划只表达目标；行为冲突时，以本文和对应代码为准。

## 1. 界面结构

工作台由侧栏、顶部栏、会话中栏、可选资源面板和底部交互区组成：

- 侧栏只展示已有数据和真实入口，并分别表达项目选择、任务选择和后台待回答状态。
- 顶部栏展示当前项目或任务，以及实际可用的菜单和面板开关；普通在线状态不占用常驻位置。
- `ConversationThread` 按 journal 顺序渲染用户消息、assistant phase、活动和压缩记录。
- 宽屏资源面板与中栏并列；隐藏后不应保留可聚焦控件。窄屏使用覆盖式面板。
- 新任务的引导内容可位于视觉中心，输入器仍保持在底部工作区。

当前会话协调仍位于 `src/App.tsx`；已独立的关键界面包括：

- `src/components/WorkbenchShell.tsx`
- `src/components/WorkbenchTopbar.tsx`
- `src/components/SidebarNavigator.tsx`
- `src/components/ComposerDock.tsx`
- `src/components/lobe-chat/ConversationThread.tsx`
- `src/components/lobe-chat/ActivityTimeline.tsx`
- `src/components/lobe-chat/AskUserDock.tsx`
- `src/components/lobe-chat/TaskProgressRail.tsx`
- `src/components/ComposerPlusPanel.tsx`
- `src/components/ComposerModelMenu.tsx`
- `src/components/ComposerPlanModeButton.tsx`
- `src/components/ContextUsageChip.tsx`
- `src/components/FloatingSurfaceProvider.tsx`

`SidebarNavigator` 拥有侧栏渲染、项目/任务披露语义、当前项语义、虚拟任务行、只读悬停预览和账户入口；数据加载、菜单动作与 Host 协调仍由 `App.tsx` 提供。任务预览停留 450ms 后调用 `session_preview`，以 30 秒短缓存合并同一 session 的并发请求，并丢弃移出、滚动、折叠、菜单打开或虚拟行卸载后的过期响应。键盘聚焦跳过停留延迟，但预览自身不可交互、不夺取焦点。Host 只返回最近 `user` 与 `assistant` 的可见正文摘要；思考、附件、工具输出和完整 journal 不进入 DTO。项目预览使用已加载的项目与 session 元数据，Git 摘要完成惰性能力前不显示。

`WorkbenchShell` 拥有三栏布局根节点，并在侧栏或资源面板关闭后把焦点恢复到对应顶部栏按钮。会话中栏在面板切换时保持挂载，因此原生滚动位置不被重建。

`ComposerDock` 拥有底部浮层、三层输入器和运行中的 `TaskProgressRail`；草稿、附件、队列、模型/权限偏好、发送停止和 Host 调用仍由 `App.tsx` 提供。权限条与 `AskUserDock` 作为并列或接管槽位传入，提问和权限决策不搬进输入器。不要把尚不存在的 `ConversationSurface` 当成当前模块边界。

顶部任务菜单与侧栏任务菜单共用 `ContextMenu` 和同一组真实 session 动作。按钮打开时必须暴露 `aria-haspopup="menu"` 与展开状态；菜单按重命名、导出、分叉/回退/复制、归档/删除分组，危险删除保持末项。左下账户菜单只从 `AccountStatus`、当前 Provider 和 billing 快照派生账户、额度、重置时间、主题、设置与登录动作；未实现的宠物、支持、更新检查或云入口不得作为占位项出现。

## 2. 三层输入器

输入器按以下三层组织：

1. **状态层**：运行计划时显示 `TaskProgressRail`；否则显示当前目标或 `ComposerProjectMenu`。
2. **内容层**：发送队列、附件预览和可增长的 `ComposerEditor`。
3. **操作层**：左侧为加号、权限、启用中的计划模式和目标；右侧为上下文、模型及发送/停止。

行为约束：

- 空闲时提交按钮发送；运行时同一位置显示停止。
- 运行中仍可编辑和排队；等待权限、Agent 提问或计划确认时按对应锁定规则处理。
- 附件元数据随 `session_send` 写入已有 `ChatMessageStored.attachments`，重载后恢复名称、目录类型和可用缩略图。
- 旧消息若没有附件元数据，只保留可证明的信息，不猜测回填。
- 当前 Host 没有语音识别适配器；`HostCapabilities v2` 将 `speechRecognition` / `nativeSpeech` 声明为 `unavailable`，兼容布尔值仍为 `false`，输入器不渲染麦克风。
- 项目条外层是静态容器；只有内部文件夹、项目名和箭头按钮响应悬停与点击。运行或 pending interaction 锁定设置时不可切换项目。
- 计划模式启用后只在访问权限右侧出现一次；悬停或键盘聚焦时灯泡变为圆圈叉，点击切回 Agent。Host 保存失败时恢复原模式。
- 900×600 及更宽的桌面布局保持侧栏在正常文档流中，输入器不得落到侧栏之下；仅 720px 及以下使用侧栏覆盖层。

## 3. 加号菜单与斜杠面板

`ComposerPlusPanel` 保持两种不同语义：

- 加号入口是 ARIA menu，提供文件、文件夹、macOS Finder 所选项、项目、目标、计划模式、Record a skill，以及已安装且可调用的技能。
- `/` 是编辑器内命令 listbox，不与加号菜单合并成同一个命令体系。

所有入口必须由能力或真实命令支撑：

- 文件和文件夹：`pick_attach_files`、`pick_attach_folder`。
- Finder 所选项：仅 `HostCapabilities v2.capabilities.finderSelection.state` 为 `available` 时展示；`finder_selected_paths` 会规范化、去重并验证路径。
- Record a skill：仅存在可用会话材料且 `skillDraftSave` 能力为 `available` 时启用。
- 不支持的平台或能力直接隐藏，不显示装饰性禁用按钮。

## 4. 单一浮层

应用根节点由 `FloatingSurfaceProvider` 包裹。任一新浮层取得所有权时，先前浮层退出，因此菜单、上下文菜单和级联面板不能同时悬挂。

`FloatingSurfaceProvider` 只负责单一所有权和存在期；定位、碰撞处理、外部点击、Escape 和焦点恢复由 `useFloatingMenu` 或具体组件完成。新增浮层必须接入这套所有权，不得另建互不知情的 popover 状态。

## 5. 模型、推理与上下文

模型控件只展示 Runtime/Host 声明的真实能力：

- `AvailableModel.capabilities.reasoningEfforts` 是可选字段。
- 当前 Host 对 `grok-4.5` 声明 `low`、`medium`、`high`；未知模型不显示推理强度。
- 当前没有独立 speed 映射，因此不显示速度。
- 模型或推理变更失败时，UI 回滚到服务端已确认值。

上下文环只使用 Runtime 遥测和已知模型容量：

- 有精确使用量和窗口容量时显示比例。
- 只有使用量时显示 token 数，不推算百分比。
- 没有可靠数据时显示未知状态 `—`，不按字符数估算。
- 悬停或键盘聚焦显示紧凑摘要；点击同一圆环才打开详细 Runtime 遥测与 `/compact`。摘要和详情使用同一精确数据源。
- `/compact` 是真实 Agent 操作；Host 只发送命令、记录事件和展示结果，不改写可见历史。

## 6. 活动记录与 assistant phase

产品语义使用 **ActivityRecord** 和 **AssistantPhase**；当前落地结构分别是：

- 前端 `ActivityItem` / `ActivityRun`，由 `activityTimelineModel.ts` 从 `ChatMessage` 归一化。
- Host 通过稳定 phase id、journal marker 和活动边界表达 assistant phase，不新增持久化 schema。

活动分类覆盖技能、上下文压缩、文件、命令、图像、浏览器、子任务、Agent 提问和通用工具；状态覆盖运行中、完成、失败和取消。

新记录的顺序保证：

1. 第一次收到活动时，Host 提交此前尚未落盘的 assistant 内容。
2. 在原位置插入或更新活动消息。
3. 活动后的正文使用新的稳定 phase id。
4. `ConversationThread` 视觉上保持同一轮回复，但不会把活动挪到轮次末尾。

`context_compact` 在前台和后台任务中同样建立 phase 边界。连续、相邻、已完成且同类的活动可以汇总；运行中、失败或取消的活动保持独立。未知旧工具行只显示受限元数据，原始输出不会被当成时间线标题倾倒。

旧 journal 没有 phase 或插入证据时，按已有存储顺序降级展示，不伪造历史穿插位置。

## 7. Agent 提问与计划确认

`AskUserDock` 与输入器同宽并在底部接管普通输入区：

- 每次展示一题，并提供进度、上一题、下一题、跳过和关闭。
- 单选可自动前进；多选和自由文本由下一题或提交确认。
- 只有 Runtime 的 `recommended: true` 或精确推荐后缀才显示推荐标记。
- 跳过只跳过当前题；全部跳过仍提交 accepted、空 `answers` 和空 `partial_answers`。
- 关闭取消整组询问，与全部跳过不同。

Host 按会话保存所有 pending interaction：

- 内建 Sunsetz kernel 的 `write_file` / `run_command` 复用同一条 `ComposerDock` 权限条和 `InteractionSnapshotV1` kind `permission`；Host 会话没有 ACP 客户端，回复走 oneshot，不发 JSON-RPC。`AcceptEdits` 只自动放行根内写入，命令仍要问。
- `InteractionSnapshotV1` 以判别 payload 表示 permission、ask_user 和 plan；三者共享 pending/resolving/resolved/failed/interrupted 生命周期但不混淆业务语义。
- `session_interactions_list` 返回前台和后台任务的 live interaction；旧 `session_pending_interactions` 继续作为 ask-user 兼容接口。
- 切换任务或 WebView 重载时可在 Agent 进程仍存活的前提下恢复。
- 解析前先以 interactionId/processId/rpcId compare-and-claim 为 `resolving`；Runtime 写入失败时恢复 pending 并保留内存中的 `partialAnswers`。
- `sessionId` 与 `processId` 用于消除不同 Agent 进程间 `rpcId` 冲突，重复或陈旧决策 fail-closed。
- `interactions.v1.json` 只记录有界、去敏审计；权限 scope 只留 hash，ask-user 答案不落盘。

进程退出后的 pending RPC 会标记 `interrupted`。磁盘边车不能恢复已经死亡的 Runtime RPC，也不得把它显示为仍可回答。

计划模式复用同一底部交互语言：

- 底部 `AskUserDock` 是批准、要求修改或放弃的唯一决策入口，也是全页面唯一显示“计划待审阅”的位置。
- 计划正文可在会话或资源面板查看；消息卡片和资源面板只显示中性“计划”状态，不提供第二组决策按钮。
- `session_resolve_plan` 只从底部决策入口调用；Host 成功前不提前清除 pending 状态，失败时保留当前计划与用户输入。
- 批准后保留计划正文供后续执行和回看，不自动打开资源面板；用户主动点击计划卡片时才打开全文。
- `TaskProgressRail` 只显示可从真实计划和变更数据得出的步骤、文件数、行数和耗时；数据不完整时不伪造。

## 8. 从会话生成技能

Record a skill 只处理当前会话的可见材料：

- 默认使用完整会话，也可选择连续消息范围。
- 材料包含用户可见文本、assistant 正文和活动摘要。
- 思考、原始命令输出、凭据和无关内部字段不进入草稿。
- 超过输入保护上限时要求缩小范围，不静默截断。

草稿经审阅后由 `skill_draft_save` 保存：

- 项目级：`.grok/skills/<slug>-skill/`
- 用户级：`~/.grok/skills/<slug>-skill/`

Host 校验名称、frontmatter、相对路径、体积、路径穿越、符号链接、密钥特征和覆盖冲突，并通过暂存目录与 rename 完成原子保存。目标级跨进程锁覆盖二次所有权/tree-hash 检查、替换和候选状态提交；提交失败会恢复原目标，发现非协作外部改写时先隔离为隐藏 conflict 目录而不删除。取消审阅或校验失败不会留下已启用的半提交技能。

工具型任务完成后，Host 可另外生成 `SkillCandidateV1` pending 草稿。候选包含来源消息、来源 hash、审阅 hash、Host ownership 和有界审计；它不会自动保存，也不能自动覆盖用户、插件或外部 Skill。V2 用 expected hash 防陈旧窗口，编辑后的最终草稿另以 final hash 绑定。用户确认后才复用同一原子写入路径。

有限 Memory 候选是另一套独立事实源：只接受引用真实持久化 user 消息的偏好、项目事实或工作流提示，内容/总量有界并拒绝 Secret。批准只表示用户审阅通过。用户可显式选择已批准候选，构建最多 8 条、单条 1,000 字、总计 4,000 字的只读 JSON 上下文包并复制；该结果没有 `session_send`、ACP 或 Runtime 自动注入路径。

## 9. 能力与命令边界

`host_capabilities` 当前返回版本 2 的能力表。`available`、`unavailable`、`needs_permission`、`needs_install` 和 `unsupported_platform` 是入口门控的唯一状态；版本 2 中未声明的能力按未知处理，不得通过旧布尔值或调用方默认值放行，也不得进入 DOM、Tab 顺序或无障碍树。旧布尔字段只用于读取版本 1 或无版本响应时的兼容迁移。

| 能力 | 当前状态 | 对应命令/数据 |
|------|----------|---------------|
| 附件持久化 | 已实现 | `session_send.attachments` |
| Finder 所选项 | macOS 为 `available`；其他平台为 `unsupported_platform` | `host_capabilities`、`finder_selected_paths` |
| 会话技能保存 | `available` | `host_capabilities`、`skill_draft_save` |
| 全会话待回答查询 | Agent 进程存活期间可恢复 | `session_pending_interactions` |
| 统一交互查询/决策 | live Runtime 期间 `available`；死亡 RPC 仅审计为 interrupted | `session_interactions_list`、`session_resolve_interaction_v1` |
| Runtime 能力/事件 | `available` | `runtime_capabilities_v1`、`session://runtime_event_v1` |
| 会话可见消息检索 | `available`，SQLite 可删可重建 | `session_search_v1` |
| 有限 Memory 候选 | 可审阅并显式导出有界上下文包，但不自动注入 Runtime | `memory_candidates_list_v1`、`memory_candidate_*_v1`、`memory_context_pack_build_v1` |
| 模型/推理切换 | 按声明能力 | `models_list_available`、`session_set_model` |
| 精确上下文 | 仅有可靠遥测时展示 | Runtime usage + 已知 capacity |
| 原生语音 | `unavailable` | v2 能力表 + `speechRecognition: false`；无 speech 命令 |
| 会话/项目预览、Git 摘要、资源审阅 | `available` | `HostCapabilities v2` 对应实现 |
| 应用存活期间后台调度 | `available` | Rust claim ledger + 现有 ACP 会话路径 |
| 智能快照、电脑控制、系统级常驻调度 | `unavailable` | v2 能力表或专项里程碑 |

Runtime sandbox 默认 `off`。Linux 在 bubblewrap 可用时可验证应用 `workspace_write` / `read_only`；macOS、Windows 请求非 off 配置会拒绝启动，不会静默降级。完整边界见 [runtime-migration-v1.md](./runtime-migration-v1.md)。

不得据此声称以下项目已经完成：

- macOS/Windows 原生语音识别。
- 独立速度参数。
- 未知模型的上下文百分比。
- 完整的缩放、辅助偏好、内容状态与原生窗口 Playwright / 实机矩阵；当前只具备空工作台、资源面板和 200% 基本几何证据。

## 10. 维护检查

修改工作台行为时至少核对：

1. `SidebarNavigator` 的披露、当前项、原生按钮键盘语义和 `inert` 测试。
2. `WorkbenchShell` 的面板焦点恢复与会话滚动保持测试。
3. `HostCapabilities v2` 的显式状态、旧版兼容和未知能力隐藏测试。
4. `FloatingSurfaceProvider` 的单一浮层测试。
5. `ComposerDock` 的浮层壳、进度轨、权限槽、AskUser 接管，以及 `ComposerModelMenu`、上下文用量和加号菜单测试。
6. `ActivityTimeline` 的顺序、归并及旧历史降级测试。
7. `AskUserDock` 的逐题、跳过、取消和失败恢复测试。
8. Rust 的 ask_user、附件、Finder、技能保存、能力表和 compact phase golden 测试。
9. `pnpm test:visual` 的空工作台、上下文摘要、计划模式、资源面板、焦点恢复和 200% 基本几何矩阵；900×600 还必须断言 composer 完全位于 main 边界内。
10. Windows 窗口改动需运行 `scripts/windows-native-smoke.ps1` 与 `scripts/windows-native-webview-smoke.mjs` 的 CI 原生诊断，并检查上传的窗口截图；该诊断不替代实机缩放和辅助功能验收。

若接口或行为变化，同步更新本文、`session-continuity.md` 和 `docs/SPIKE-ACP.md`。
