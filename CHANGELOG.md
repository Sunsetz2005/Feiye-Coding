# Changelog

## [UNRELEASED] — 2026-08-24

- 统一 permission、ask-user、plan 的版本化交互生命周期，支持前后台会话与 WebView 重载恢复，并新增有界去敏审计边车。
- 权限改为 fail-closed：未知编辑路径、目录穿越和下载不再自动批准；会话授权按完整规范化命令或资源匹配。
- 隔离静态 HTML 预览，启用主 WebView CSP，禁用全局 asset protocol，并引入受 provenance 校验的 `ResourceHandleV1`。
- 远程 ACP 限制为 loopback；JSON/TOML 读改写迁移到覆盖完整事务的跨进程锁与原子替换。
- 新增 RuntimeCapabilitiesV1、RuntimeEventEnvelopeV1、Linux bubblewrap sandbox profile 与 clean-room capability manifest。
- 新增只读 Runtime 插件目录/搜索/hooks inventory、可重建 FTS5 会话检索、待审 Skill 候选和 Rust Host 自动化认领账本。
- macOS/Windows 非 off Runtime 沙箱、应用关闭后的系统调度和 Windows 物理机发布验收仍未完成。

## [1.0.0] — 2026-07-27 11:55

- 建立 Sunsetz 私有远程仓库并重写中英文项目说明。
- 完成工作台侧栏、顶部栏、响应式资源面板和焦点恢复基础重构。
- 引入 HostCapabilities v2，隐藏未实现或不可用的功能入口。
- 建立前端覆盖率、契约检查、视觉回归和三平台 Rust 持续集成。
- 清理与 Sunsetz 品牌无关的截图、二维码、链接和过期文档。
- 修复 Windows 上技能目录原子保存失败的问题。
