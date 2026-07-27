# Sunsetz 工作台视觉验收记录

## 自动截图基线

Playwright 当前覆盖三组证据：

- 空工作台：900×600、1200×800、1600×1000 × 深色、浅色、高对比度，共 9 张像素基线；
- 资源面板打开：同一三尺寸三主题矩阵，共 9 张像素基线，并检查面板关闭后内容卸载和顶部栏触发按钮恢复焦点；
- 200%：三个 viewport 各执行一次空工作台基本几何检查，验证无横向溢出、标题与侧栏可见、composer 仍位于 viewport 内；该组不生成像素基线；
- 当前像素基线文件共 18 张，Playwright 用例实例共 21 个；
- 基线分别位于 `tests/visual/__screenshots__/workbench-baseline.spec.ts-snapshots/` 和 `tests/visual/__screenshots__/workbench-pane.spec.ts-snapshots/`；
- `pnpm test:visual` 用于回归，`pnpm test:visual:update` 只用于人工确认后的基线更新。

深色主题在不同运行间仅出现窗口底部 12px 原生圆角的 Chromium 抗锯齿波动。测试允许最多 100 个差异像素；当前波动为 34–68 个阈值像素，内容区仍按像素比较。

## 相对参考图的当前结果

- 品牌保持 Sunsetz；不复制 Codex 名称或未支持入口。
- 侧栏已抽离为 `SidebarNavigator`；项目披露、项目选择和任务选择使用分离的原生按钮语义，任务支持 Enter / Space，当前项和焦点状态可被识别。
- 侧栏或资源面板关闭后，焦点回到对应顶部栏按钮；资源面板内容随关闭卸载，会话中栏滚动节点保持挂载。
- `HostCapabilities v2` 只让显式 `available` 的入口进入界面；未知或不可用能力保持隐藏。
- 当前提交重新构建的 macOS 调试 `.app` 已在 3024×1964 Retina 屏幕（`backingScaleFactor=2.0`）确认侧栏/资源面板关闭后焦点返回对应顶部栏按钮、资源内容从辅助功能树卸载、侧栏可用 Space 重开且面板无重叠；既有原生证据还确认项目选中态无 coral/orange 边框。
- Windows CI 使用真实 Tauri 窗口和 WebView2 CDP 检查初始窗口位于系统工作区内、资源面板打开/卸载、触发器焦点恢复及侧栏 Space 重开，并上传 `resources-open.png` 与 `native-window.png`；该证据不替代 Windows 实机缩放和辅助功能手测。
- composer 已从旧 Grok/Lobe 多 chip 布局改为三层结构，底栏不再挤成窄列。
- 加号菜单与 `/` 命令面板语义分离。
- 模型菜单只显示 Runtime 模型与 capability 声明的 effort；无真实 speed 时隐藏。
- 上下文无精确容量时显示未知，不估算。
- 历史工具行通过 `ActivityTimeline` 保留；旧未知工具使用 generic 安全摘要。
- Agent 提问与计划确认使用底部 `AskUserDock`，不再使用居中模态框。
- 计划在消息流中显示 `PlanArtifactCard`，可进入右侧完整审阅。

## 尚未完成的视觉证据

- 中栏 520px / 380px；
- 200% 下的长会话、浮层、计划、提问和资源面板等完整状态矩阵；
- 减少动态、减少透明度组合；
- 长会话、附件、菜单、上下文详情、运行、权限、提问、计划、步骤、侧栏预览、账户菜单和设置页面；
- macOS Finder；Windows 原生窗口的 200% 缩放、完整键盘路径、焦点和面板生命周期实机手测。

外部设计参考只作为输入，不作为伪造的“已通过截图”，也不随仓库分发。
