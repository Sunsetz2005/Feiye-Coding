# Sunsetz 工作台结构清单

本清单记录当前实现与空工作台 Playwright 基线。它不代替 macOS / Windows 原生窗口验收。

## Shell

- Frameless：`src-tauri/tauri.conf.json` → `decorations: false`
- Sunsetz 语义令牌：`src/styles/tokens.css` + `src/styles/workbench.css`
- 三栏布局根：`WorkbenchShell`；状态和 Host 协调仍由 `App.tsx` 持有
- 面板关闭：`WorkbenchShell` 将焦点恢复到对应顶部栏按钮；会话中栏保持挂载，滚动位置不因面板切换重建
- 顶栏：任务标题、更多菜单、左右面板开关；异常状态按需出现
- 主消息轨和 composer：`--wb-composer-width: 46rem`

## 左侧导航

| Item | Source |
|------|--------|
| 模块边界 | `SidebarNavigator`；`App.tsx` 提供数据、Host 协调与动作回调 |
| Sunsetz identity | `.sidebar-brand-row` |
| 新建、搜索、已安排、插件 | `.sidebar-nav` 真实入口 |
| 项目与任务 | 披露按钮使用 `aria-expanded` / `aria-controls`；项目与任务选择分离，当前项使用 `aria-current` |
| 任务键盘行为 | 原生 `button` 响应 Enter / Space；可见 `:focus-visible` 焦点环 |
| 虚拟任务行 | CSS 与 `VirtualList` 共用 34px 行高、2px 间距 token |
| 待回答状态 | `.tree-l3__status--question` |
| 账户入口 | `.sidebar__footer` |
| 窄屏覆盖层 | `workbench.css` `@media (max-width: 720px)`；900×600 保持侧栏在布局内 |
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
| Composer 上层 | 静态项目条中的局部项目按钮，或目标摘要；计划模式不在此重复 |
| Composer 内容层 | 80×80 图片预览、文件/目录、可增长编辑器 |
| Composer 底层 | 加号、权限、启用中的计划模式、目标 / 上下文、模型、发送或停止 |
| 浮层 | `FloatingSurfaceProvider` 单一所有权 |

## 右侧资源

- `ResourceViewer` 提供 Files、Changes 和 Plan。
- 宽屏可调宽；窄屏为非模态覆盖层。
- `.aside--hidden` 时从可见性、指针和焦点路径移除。
- 关闭后 `ResourceViewer` 卸载，焦点返回顶部栏“显示文件”按钮。

## 当前自动视觉证据

| 项目 | 实测 |
|------|------|
| viewport | 900×600、1200×800、1600×1000 |
| 主题 | 深色、浅色、高对比度 |
| 空工作台像素基线 | 9 |
| 资源面板打开像素基线 | 9 |
| 上下文摘要像素基线 | 3 |
| 计划模式像素基线 | 3 |
| 计划确认像素基线 | 3 |
| 侧栏预览与菜单像素基线 | 12 |
| 设置中心像素基线 | 3 |
| 像素基线文件总数 | 42 |
| Playwright 用例实例 | 45：42 个截图实例 + 3 个 200% 几何实例 |
| horizontal overflow | 0 |
| composer 与 main 边界 | 三主题三 viewport 均断言完整包含；900×600 不被侧栏覆盖 |
| 资源面板生命周期 | 打开可见；关闭后卸载；触发按钮恢复焦点 |
| 200% 基本几何 | 三个 viewport 均检查无横向溢出、标题与侧栏可见、composer 位于 viewport 内 |
| 命令 | `pnpm test:visual` |

当前提交重新构建的 macOS 调试 `.app` 已在 Retina 2× 下人工确认侧栏和资源面板关闭后的触发器焦点恢复、资源内容卸载、侧栏 Space 重开，以及项目选中态无 coral/orange 边框。Windows CI 原生窗口自动诊断也已覆盖工作区边界和面板生命周期。2026-08-31 物理 Windows 11 / 200% / `3f96d3a` 实机为 1 PASS / 6 FAIL，须按 [`windows-stage1-manual.md`](./windows-stage1-manual.md)用含可见焦点修复的构建重跑。减少动态/透明度、中栏 520px / 380px 和完整内容状态矩阵仍未完成。
