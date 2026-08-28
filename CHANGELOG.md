# Changelog

## [UNRELEASED] — 2026-08-28

- 已审阅记忆会显示在输入器上，并可随草稿恢复；发送时显式注入默认内核，设置页提供召回、稽核、导出和删除。
- 技能芯片绑定库存身份，扩展页可审阅本会话使用记录和学习建议；过期 tree hash 不会悄悄装载正文。
- 信任项目后，内建循环会有界读取 `AGENTS.md` / `Sunsetz.md` / `.sunsetz/instructions.md` / `CLAUDE.md`，并在输入器显示只读项目说明。
- 默认内核单轮工具上限提高到 16 次，并在不确定仓库布局时先读文件；离线模型目录不再写死过期具体 ID。
- 计划的 approved / executing / done 已作为可恢复的 `PlanArtifactV1` 产物，与 Permission 的 live RPC 生命周期分开，切会话后仍能回看正文且不会把死亡审阅重新打开。
- Host 内建循环对显式选择的 Skill 读取库存已信任目录中的有界 `SKILL.md` 并写入该轮模型提示，不依赖 grok 二进制，也不把 GROK_HOME 当作产品内核。
- Host 内建循环新增可信项目根内的 `write_file` 与 `run_command`，经现有权限条批准后才执行；`AcceptEdits` 不自动放行命令，权限预览不含文件正文。
- Sunsetz 默认使用内建 Agent 循环调用已配置的 OpenAI 兼容聊天接口，不再把 grok CLI 作为进入工作台或运行会话的硬门槛。
- 将底部三层输入器抽离为 `ComposerDock`，工作台只保留会话协调以及提问/权限接管。
- 统一 permission、ask-user、plan 的版本化交互生命周期，支持前后台会话与 WebView 重载恢复，并新增有界去敏审计边车。
- 权限改为 fail-closed：未知编辑路径、目录穿越和下载不再自动批准；会话授权按完整规范化命令或资源匹配。
- 隔离静态 HTML 预览，启用主 WebView CSP，禁用全局 asset protocol，并引入受 provenance 校验的 `ResourceHandleV1`。
- 远程 ACP 限制为 loopback；JSON/TOML 读改写迁移到覆盖完整事务的跨进程锁与原子替换。
- 新增 RuntimeCapabilitiesV1、RuntimeEventEnvelopeV1、Linux bubblewrap sandbox profile 与 clean-room capability manifest。
- 新增只读 Runtime 插件目录/搜索/hooks inventory、可重建 FTS5 会话检索、待审 Skill 候选和 Rust Host 自动化认领账本；无法机器校验目标的插件安装/卸载继续由 CLI 执行。
- 新增引用真实 user 消息、拒绝敏感材料且可删除的有限 Memory 候选事实源，以及只读、有界、显式选择的上下文包；两者都不会暗中注入 Runtime。
- Skill 候选治理升级为双哈希审阅、目标级所有权/CAS 与失败恢复；用户、插件和外部 Skill 继续禁止自动覆盖。
- 自动化新增 `run_once | skip` missed-run policy 和 Runtime 进度 heartbeat；过期 claim 仍不自动生成 replacement，避免长任务并发重复副作用。
- Settings 改为白名单字段级原子 patch、前端权威回滚和 Keychain 事务串行化；旧全量 setter 只保留 fail-safe 兼容语义。
- macOS/Windows 非 off Runtime 沙箱、应用关闭后的系统调度和 Windows 物理机发布验收仍未完成。

## [1.0.0] — 2026-07-27 11:55

- 建立 Sunsetz 私有远程仓库并重写中英文项目说明。
- 完成工作台侧栏、顶部栏、响应式资源面板和焦点恢复基础重构。
- 引入 HostCapabilities v2，隐藏未实现或不可用的功能入口。
- 建立前端覆盖率、契约检查、视觉回归和三平台 Rust 持续集成。
- 清理与 Sunsetz 品牌无关的截图、二维码、链接和过期文档。
- 修复 Windows 上技能目录原子保存失败的问题。
