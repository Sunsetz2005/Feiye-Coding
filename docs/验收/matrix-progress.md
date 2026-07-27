# P0 矩阵进度表（工作台重构同步 · 2026-07-26）

本页只记录当前仍有自动化或原生验收证据的工作台能力，不继承已删除的旧仓库矩阵编号。

## H 权限与控制条

| ID | 能力（矩阵原文） | 状态 | 证据 |
|----|------------------|------|------|
| **H01** | 模型选择 | **PASS** | `models_list_available` 返回 Runtime 模型；`ComposerModelMenu` 过滤非法值；`composer_prefs_set` 先执行 live ACP `session/set_model`、成功后持久化；UI 失败时 `rollbackOptimisticSetting` 恢复 |
| **H02** | Reasoning effort | **PASS** | 仅显示 `AvailableModel.capabilities.reasoningEfforts`；未知 capability 隐藏；Host `set_effort_and_respawn_needed` 进入真实 soft-respawn 路径；失败时 UI 对称回滚 |
| **H03** | 权限审批默认 | PASS | 默认 Ask；`PermissionPolicy::default()` + composer access menu |
| **H04** | Allow once | PASS | 权限条 + `pick_option_id(allow_once)` / UI map |
| **H05** | Allow for session | PASS | Host session scope cache + `resolve_permission` |
| **H06** | Deny | PASS | 权限条 Deny + Runtime reject optionId |
| **H07** | 不默认 Always 全局 | PASS | 默认 Ask；Always 仅在深层设置中出现并要求二次确认 |

## 当前自动视觉矩阵

| 项目 | 结果 |
|------|------|
| viewport | 900×600、1200×800、1600×1000 |
| 主题 | 深色、浅色、高对比度 |
| 空工作台 | 9 张像素基线 |
| 资源面板 | 9 张打开状态像素基线；关闭后卸载并恢复顶部栏触发按钮焦点 |
| 200% | 三个 viewport 的基本可操作性与几何断言，不生成像素基线 |
| 总计 | 18 张像素基线、21 个 Playwright 用例实例 |
| horizontal overflow | 0 |
| 稳定性 | 底部原生圆角允许最多 100 个抗锯齿差异像素 |
| 自动化入口 | `pnpm test:visual` |

## 证据边界

以上自动矩阵使用 Playwright 浏览器降级路径。当前提交重新构建的 macOS 调试 `.app` 已在 3024×1964 Retina 屏幕（`backingScaleFactor=2.0`）人工确认侧栏和资源面板关闭后的触发器焦点恢复、资源内容卸载、侧栏 Space 重开和无面板重叠。Windows CI 另以真实 Tauri 窗口和 WebView2 CDP 自动确认窗口不越过系统工作区、资源面板卸载、触发器焦点恢复及侧栏 Space 重开，并保留原生窗口和资源面板截图。既有原生证据还覆盖项目选中态无 coral/orange 边框、从输入器到主导航的反向 Tab 路径，以及项目披露按钮的 Space / Enter。资源面板自动证据仍只覆盖打开、关闭、卸载与焦点恢复；不代表有内容会话、菜单或全部面板内容已经完成，也不替代 Windows 实机手测。

## 尚待单独验收

- 200% 下的完整内容状态矩阵与中栏 520px / 380px；
- 减少动态、减少透明度组合；
- 长会话、附件、菜单、计划、提问和设置页面；
- macOS Finder，以及 Windows 原生窗口的 200% 缩放、键盘、焦点与面板生命周期实机手测；
- P0 矩阵其余条目仍以各自专项记录为准，本文件不因一次 browser smoke 批量改写状态。
