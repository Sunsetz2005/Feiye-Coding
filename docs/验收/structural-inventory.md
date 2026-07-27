# Sunsetz 工作台结构清单

本清单记录当前实现与空工作台 Playwright 基线。它不代替 macOS / Windows 原生窗口验收。

## Shell

- Frameless：`src-tauri/tauri.conf.json` → `decorations: false`
- Sunsetz 语义令牌：`src/styles/tokens.css` + `src/styles/codex-workbench.css`
- 三栏布局根：`WorkbenchShell`；状态和 Host 协调仍由 `App.tsx` 持有
- 顶栏：任务标题、更多菜单、左右面板开关；异常状态按需出现
- 主消息轨和 composer：`--wb-composer-width: 46rem`

## 左侧导航

| Item | Source |
|------|--------|
| Sunsetz identity | `.sidebar-brand-row` |
| 新建、搜索、已安排、插件 | `.sidebar-nav` 真实入口 |
| 项目与任务 | 项目树；项目/任务分别高亮 |
| 待回答状态 | `.tree-l3__status--question` |
| 账户入口 | `.sidebar__footer` |
| 窄屏覆盖层 | `codex-workbench.css` `@media (max-width: 900px)` |
| 完全隐藏 | `.sidebar--hidden` + `inert`，不占布局、不进入 Tab 顺序 |

## 中栏

| Item | Source |
|------|--------|
| 消息与推理 | `ConversationThread` / `Thinking` |
| 工具活动 | `ActivityTimeline`；历史 `role=tool` 不再静默隐藏 |
| 计划正文 | `PlanArtifactCard`，可打开右侧 Plan 审阅 |
| Agent 提问/计划确认 | `AskUserDock`，出现时替换普通 composer |
| 运行步骤 | `TaskProgressRail`，只显示 Runtime 可证明的数据 |
| 权限审批 | `.perm-bar` + Runtime optionId |
| Composer 上层 | 项目或目标/计划摘要 |
| Composer 内容层 | 80×80 图片预览、文件/目录、可增长编辑器 |
| Composer 底层 | 加号、权限、目标 / 上下文、模型、发送或停止 |
| 浮层 | `FloatingSurfaceProvider` 单一所有权 |

## 右侧资源

- `ResourceViewer` 提供 Files、Changes 和 Plan。
- 宽屏可调宽；窄屏为非模态覆盖层。
- `.aside--hidden` 时从可见性、指针和焦点路径移除。

## 已完成的自动截图

| 项目 | 实测 |
|------|------|
| viewport | 900×600、1200×800、1600×1000 |
| 主题 | 深色、浅色、高对比度 |
| 基线数量 | 9 |
| horizontal overflow | 0 |
| composer / main | 分别大于 300px / 360px |
| 命令 | `pnpm test:visual` |

尚未据此宣称 200% 缩放、减少动态/透明度、长会话和各浮层场景完成，也未宣称 Windows 或 macOS 原生平台手测完成。
