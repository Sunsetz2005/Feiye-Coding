# P0 矩阵进度表（工作台重构同步 · 2026-07-26）

ID 定义以 `docs/P0-能力矩阵.md` 为准。本页只更新本轮有直接实现或验收证据的项目。

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

## 本轮自动截图基线

| 项目 | 结果 |
|------|------|
| viewport | 900×600、1200×800、1600×1000 |
| 主题 | 深色、浅色、高对比度 |
| 用例 | 空工作台 9 张基线截图 |
| horizontal overflow | 0 |
| composer / main | 分别大于 300px / 360px |
| 稳定性 | 底部原生圆角允许最多 100 个抗锯齿差异像素 |
| 自动化入口 | `pnpm test:visual` |

## 证据边界

以上是 Playwright 浏览器降级路径和当前代码/测试证据，不是 macOS / Windows 原生窗口验收，也不覆盖有内容会话、菜单和资源面板等后续场景。

## 尚待单独验收

- 200% 缩放与中栏 520px / 380px；
- 减少动态、减少透明度组合；
- 长会话、附件、菜单、计划、提问、资源面板和设置页面；
- macOS Finder / 原生窗口与 Windows 原生窗口手测；
- P0 矩阵其余条目仍以各自专项记录为准，本文件不因一次 browser smoke 批量改写状态。
