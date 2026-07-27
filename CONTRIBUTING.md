# Sunsetz 开发守则

本文面向参与 Sunsetz 开发、审阅和发布的人类开发者与自动化 Agent。

## 开发环境

- Node.js 22+
- pnpm 9
- Rust stable
- macOS：Xcode Command Line Tools
- Windows：Visual Studio Build Tools 与 WebView2

```bash
pnpm install
pnpm dev
```

仅调试界面时可运行：

```bash
SUNSETZ_ACP=mock pnpm dev
```

## 开发边界

- 保持 Tauri command/event、会话状态机、权限语义和已有持久化数据兼容。
- 新增持久化字段必须可选、版本化，并提供旧数据降级路径。
- 未被 Host 或 Runtime 明确声明为 `available` 的功能不得进入界面、Tab 顺序或无障碍树。
- 用户可见文案必须同步维护英文、简体中文和繁体中文。
- 新界面必须使用 Sunsetz 设计令牌，并支持浅色、深色和高对比度主题。
- 禁止使用 `window.alert`、`window.confirm` 或 `window.prompt`，统一使用应用内弹窗。
- 不得提交凭据、认证文件、本地 Runtime 目录、支持包、私人项目内容或构建产物。
- 不复制第三方产品的品牌、图标、文案或未经后端支撑的功能入口。

## 代码与文档

- 一次提交只解决一个清晰问题，避免顺带进行无关重构。
- 行为、协议或界面规则变化时，同步修改 `docs/llm-wiki/` 下对应的权威文档。
- 构建与打包规则见 `docs/BUILD.md`，Runtime 兼容边界见 `docs/runtime-compatibility.md`。
- 提交前检查 `git diff`，不得覆盖或删除不属于当前任务的改动。

## CHANGELOG 规则

- `CHANGELOG.md` 第一行固定为 `# Changelog`。
- `## [UNRELEASED] — YYYY-MM-DD HH:mm` 必须始终是第一条版本标题，并保持内容为空。
- 每次发布都在 `UNRELEASED` 下方空一行后新增 `## [X.Y.Z] — YYYY-MM-DD HH:mm`。
- 最新正式版本放在最上方，旧版本依次向下排列。
- 每个条目只用一句简短的话说明实际完成了什么，不写实现过程、宣传语或无关技术细节。
- 版本号遵循 SemVer，并与 `package.json`、Tauri、Cargo 和界面版本文案保持一致。
- 没有对应版本章节时禁止创建 tag 或发布 Release。

示例：

```markdown
# Changelog

## [UNRELEASED] — 2026-01-01 09:00

## [1.1.0] — 2026-01-01 09:00

- 增加项目级会话预览。
```

## 提交前验证

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

文档专用改动至少检查 Markdown 链接和 `git diff --check`。只有人工确认视觉变化正确后才能更新 Playwright 截图基线。

涉及 Windows 工作台窗口、键盘或焦点时，除 CI 外还要按 `docs/验收/windows-stage1-manual.md` 在物理 Windows 设备留存实机证据。

## Pull Request

- 说明改动目的、实际结果、验证方式和仍未覆盖的风险。
- UI 变化附上对应主题和尺寸的截图。
- 危险操作、权限、秘密处理和持久化变化必须说明失败与恢复路径。
- CI 未通过、文档未同步或 CHANGELOG 格式不符合规则时不得合并。
