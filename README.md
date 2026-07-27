# Sunsetz

[English](README_EN.md)

![CI](https://github.com/Sunsetz2005/Sunsetz/actions/workflows/ci.yml/badge.svg)

Sunsetz 是一款本地优先的桌面 Agent 工作台。它把项目、会话、权限、计划、文件审阅、扩展和自动化集中在一个由 Tauri 驱动的原生应用中。

![Sunsetz 图标](assets/logo.png)

> Sunsetz 正在持续重构中。界面只展示 Host 和 Runtime 已明确声明可用的能力；未完成的路线图功能不会作为已交付能力宣传。

## 当前能力

- 多项目与多会话管理，支持搜索、归档、分叉和时间线回退。
- Runtime 流式会话、工具活动时间线、停止与发送队列。
- Ask、单次允许、会话允许、拒绝和受控无人值守权限策略。
- 三层输入器、附件持久化、精确上下文用量和 Runtime 模型选择。
- 可恢复的 Agent 提问与底部计划确认流程。
- Files、Changes 和 Plan 资源面板，支持代码、Markdown、图片、媒体、PDF 和 Office 预览。
- 自定义 Provider、MCP、技能、插件、账号与计划任务的现有管理入口。
- 简体中文、繁体中文、英文，以及浅色、深色和高对比度主题。
- `HostCapabilities v2` 能力门控：未知、不支持或未安装的入口不会进入界面和键盘路径。

阶段状态、已验证证据和仍缺门禁见[长期重构执行状态](docs/长期重构-执行状态.md)。

## 技术结构

| 层 | 技术与职责 |
|----|------------|
| 桌面 Host | Rust、Tauri 2；窗口、文件、权限、持久化和 Runtime 生命周期 |
| 工作台 UI | React 19、TypeScript、Vite；会话、输入器、资源和设置界面 |
| Runtime 边界 | 版本化能力与 DTO；兼容细节隔离在私有适配层 |
| 验证 | Vitest、Playwright、Rust 测试、Tauri 命令与事件契约扫描 |

## 本地开发

需要 Node.js 22+、pnpm 9 和 Rust stable。macOS 构建还需要 Xcode Command Line Tools；Windows 构建需要 Visual Studio Build Tools 和 WebView2。

```bash
pnpm install
pnpm dev
```

仅运行 Web UI：

```bash
pnpm dev:ui
```

使用本地模拟 Runtime：

```bash
SUNSETZ_ACP=mock pnpm dev
```

应用数据目录可通过 `SUNSETZ_HOME` 覆盖。

## 验证

```bash
pnpm verify:contracts
pnpm typecheck
pnpm test
pnpm test:coverage
pnpm coverage:audit
pnpm coverage:changed
pnpm build:ui
pnpm test:visual
cd src-tauri && cargo test
```

覆盖率策略见 [`coverage-policy.json`](coverage-policy.json)，构建与打包说明见 [`docs/BUILD.md`](docs/BUILD.md)。

## 安全与数据

- 不要提交 Token、API Key、认证文件、支持包或私人项目内容。
- 项目信任和 Ask 权限默认开启；无人值守能力需要明确配置。
- 秘密值应进入系统安全存储，不得写入 journal、预览、技能、日志或诊断包。
- 安全问题请按 [`SECURITY.md`](SECURITY.md) 私下报告。

## 文档

- [工作台与会话行为](docs/llm-wiki/workbench-conversation.md)
- [Runtime 兼容边界](docs/runtime-compatibility.md)
- [设计令牌](docs/design-tokens.md)
- [长期重构执行状态](docs/长期重构-执行状态.md)
- [贡献指南](CONTRIBUTING.md)

## 许可证与品牌

源码按 [`LICENSE`](LICENSE) 中的 MIT 条款发布，并保留其中要求的第三方版权声明。Sunsetz 名称、图标和品牌资产不包含在源码许可授予的商标权中，详见 [`TRADEMARKS.md`](TRADEMARKS.md)。
