# Sunsetz Agent 工作台对标纪要

Codex 截图用于校准信息层级、空间来源与交互密度，不复制品牌或后端尚不支持的控件。`src/components/lobe-chat/` 是历史目录名，不代表当前界面仍采用旧 Lobe composer。

## 借鉴来源与当前落地

| 来源 | 借鉴点 | Sunsetz 落地 |
|------|--------|--------------|
| Codex 工作台参考图 | 低噪声消息轨、底部停靠输入器、加号菜单、模型级联菜单、Agent 提问卡 | `ConversationThread`、`AskUserDock`、`ComposerModelMenu`、`ComposerPlusPanel` |
| Apple 交互原则 | 控件从触发点展开、按下即时反馈、减少动态/透明度 | `workbench.css`、`composer-surfaces.css`、`FloatingSurfaceProvider` |
| AiderDesk 文件浏览 | 项目文件树、文件预览、搜索与刷新 | `ResourceViewer` + `fs_list_dir` / `fs_read_file` |
| Runtime 权限协议 | optionId、scopeKey、会话放行和高风险二次确认 | `PERMISSION_POLICIES` + Host `PermissionPolicy` |
| 三栏工作台 | 左项目/任务、中会话、右资源；面板可真正移出交互树 | `sidebar--hidden`、`aside--hidden`、`inert` |

## 当前工作台事实

1. 顶栏只保留任务标题、更多菜单和左右面板开关；普通空闲/在线状态不常驻。
2. 左栏展示真实的新建、搜索、已安排、插件、项目、任务和账户入口；项目与任务分别高亮，后台待回答任务有标记。
3. 右侧资源面板在宽屏可调宽，窄屏覆盖主栏；隐藏后不保留可聚焦后代。
4. 输入器是三层结构：上层为项目或目标/计划摘要；中层为附件与可增长编辑器；底层左侧为加号、权限、目标，右侧为精确上下文、模型和发送/停止。
5. 加号菜单与 `/` 命令面板保持不同语义；加号只展示有真实入口或受 Host capability 控制的动作。
6. 模型入口为紧凑文本控件；模型、Runtime 声明的推理档位和重置默认值使用级联菜单。没有独立 speed capability 时不显示速度。
7. `FloatingSurfaceProvider` 保证同一时间只有一个浮层拥有交互；加号、模型、上下文等支持键盘、Escape 关闭和焦点恢复。
8. 历史工具消息不再隐藏。`ActivityTimeline` 展示读取技能、压缩、文件、命令、图像、浏览器、子任务、提问和未知工具；旧未知 `role=tool` 使用安全 generic 摘要，失败状态保留。
9. Agent 问题使用与 composer 同宽的 `AskUserDock`；计划确认复用同一底部交互形态。计划正文同时保留线程内 `PlanArtifactCard` 和右侧资源审阅。
10. `TaskProgressRail` 只显示 Runtime 可证明的步骤、文件数和计划进度；无数据时不伪造增删行数。

## Changes 与 Plan

- **Changes**：右栏提供 Session / Workspace 两类真实变更；优先工具 payload，再回退到 git diff 或当前文件。保留打开编辑器、Reveal 和复制路径，不提供危险的静默 discard。
- **Plan**：线程内显示中性紧凑计划卡，右栏只读显示 Markdown 全文；批准、请求修改和放弃只在底部 `AskUserDock` 调用现有 `sessionResolvePlan`，不在资源栏重复决策。

## 当前视觉与原生证据

Playwright 覆盖空工作台和资源面板的 900×600、1200×800、1600×1000 × 深色、浅色、高对比度矩阵，并在每个 viewport 执行 200% 基本几何检查。Linux CI 会断言主题属性和布局行为；Darwin 本地像素基线仍是视觉差异的权威。

当前提交重新构建的 macOS 调试 `.app` 已在 `backingScaleFactor=2.0` 的 Retina 屏幕人工确认侧栏与资源面板关闭后的焦点恢复、资源内容卸载、侧栏 Space 重开和中性项目选中态。Windows CI 原生窗口诊断已通过。2026-08-31 物理 Windows 11 / 200% / `3f96d3a` 实机为 1 PASS / 6 FAIL，阶段一不得标记完成；见[阶段一 Windows 实机验收](../验收/windows-stage1-manual.md)。
