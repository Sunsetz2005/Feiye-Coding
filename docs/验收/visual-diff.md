# Sunsetz 工作台视觉验收记录

## 自动截图基线

Playwright 当前覆盖空工作台的 9 个组合：

- viewport：900×600、1200×800、1600×1000；
- 主题：深色、浅色、高对比度；
- document 横向 overflow 为 0；
- composer 宽度大于 300px，main 宽度大于 360px；
- 基线位于 `tests/visual/__screenshots__/workbench-baseline.spec.ts-snapshots/`；
- `pnpm test:visual` 用于回归，`pnpm test:visual:update` 只用于人工确认后的基线更新。

深色主题在不同运行间仅出现窗口底部 12px 原生圆角的 Chromium 抗锯齿波动。测试允许最多 100 个差异像素；当前波动为 34–68 个阈值像素，内容区仍按像素比较。

## 相对参考图的当前结果

- 品牌保持 Sunsetz；不复制 Codex 名称或未支持入口。
- composer 已从旧 Grok/Lobe 多 chip 布局改为三层结构，底栏不再挤成窄列。
- 加号菜单与 `/` 命令面板语义分离。
- 模型菜单只显示 Runtime 模型与 capability 声明的 effort；无真实 speed 时隐藏。
- 上下文无精确容量时显示未知，不估算。
- 历史工具行通过 `ActivityTimeline` 保留；旧未知工具使用 generic 安全摘要。
- Agent 提问与计划确认使用底部 `AskUserDock`，不再使用居中模态框。
- 计划在消息流中显示 `PlanArtifactCard`，可进入右侧完整审阅。

## 尚未完成的视觉证据

- 中栏 520px / 380px 与 200% 缩放；
- 减少动态、减少透明度组合；
- 长会话、附件、菜单、上下文详情、运行、权限、提问、计划、步骤、侧栏预览、账户菜单、资源面板和设置页面；
- macOS Finder / 原生窗口和 Windows 原生窗口手测。

参考图仍是设计输入，不作为伪造的“已通过截图”。
