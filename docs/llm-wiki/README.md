# Sunsetz · llm-wiki

Agent 与贡献者的**可执行知识库**。改产品行为或 UI 文案前，先读本目录对应条目。

| 文档 | 用途 |
|------|------|
| [workbench-conversation.md](./workbench-conversation.md) | **当前工作台权威说明**：输入器、浮层、活动顺序、提问/计划、附件与能力边界 |
| [settings-center.md](./settings-center.md) | **设置中心权威说明**：真实栏目注册表、搜索、能力门控和视觉层级 |
| [i18n.md](./i18n.md) | **多语言强制规则**：所有 UI 文案、增改键、双语同步 |
| [dialogs.md](./dialogs.md) | **禁止 `window.confirm` / `prompt` / `alert`**；应用内弹窗；复用现有面板样式（不强制毛玻璃） |
| [catalog.md](./catalog.md) | 与 Grok Build CLI 对齐的模型 / 推理强度 / 权限（含 YOLO） |
| [automations.md](./automations.md) | 自动化任务设计（Build `/loop` / scheduler；不阻塞 P0） |
| [slash-composer.md](./slash-composer.md) | 斜杠面板、技能标签、Goal 模式、Doctor |
| [open-connector.md](./open-connector.md) | GitHub/Notion/Slack 令牌、Gmail/Drive/Calendar Google 登录、`@` 芯片；其余连接器即将在 App 内连接 |
| [local-data-isolation.md](./local-data-isolation.md) | MCP / 模型 / 连接器数据按操作系统用户隔离，禁止提交到 GitHub |
| [session-continuity.md](./session-continuity.md) | Agent 续会话（load/bootstrap）、自动压缩归属 |
| [runtime-migration-v1.md](./runtime-migration-v1.md) | **成熟能力迁移权威边界**：统一交互、桌面安全、原子 Settings、Runtime 事件、检索、Memory/Skill 候选、显式上下文包与 Host 调度 |
| [runtime-backend.md](./runtime-backend.md) | **内核选择**：内建 agent loop 是产品路径；Grok ACP 只在显式 legacy 开关后保留；mock/环境变量盖过设置时必须标明 |
| [account.md](./account.md) | 官方登录 / 会员额度 / 热力图 / 调用日志 |
| [providers.md](./providers.md) | 自定义中转、agent GROK_HOME、编辑器探测 |
| [setup.md](./setup.md) | 首次初始化门禁：登录 Sunsetz 或跳过进入工作台 |
| [release.md](./release.md) | **发版 / Release 强制流程**：CHANGELOG、tag、三端 CI、macOS 损坏处理 |
| [maintain.md](./maintain.md) | **开源维护**：Issue 分拣、PR 审核、社区反馈入库、修复闭环 |

## 原则

1. **可检索**：一条知识一个文件，标题即意图。  
2. **可执行**：写清路径、键名、禁止事项，而不是空泛建议。  
3. **变更同步**：改代码必改 wiki；改 wiki 后实现要跟上。  
4. **跨 Agent**：后续 Agent 接手时以本目录为准，不靠会话记忆。

## 相关源码

- i18n：`src/i18n/`
- Build 目录：`src/lib/grokCatalog.ts`
- 模型/effort/mode/policy 状态：`src/hooks/useComposerCatalog.ts`
- UI 入口：`src/App.tsx`
- 输入器：`src/components/ComposerDock.tsx`
- 会话渲染：`src/components/lobe-chat/ConversationThread.tsx`
- 活动时间线：`src/components/lobe-chat/activityTimelineModel.ts`
- Host 会话协调：`src-tauri/src/session_manager/`
- 内建 Agent 循环：`src-tauri/src/agent_loop.rs`
