# Sunsetz

Sunsetz 是本地优先的桌面 Agent 工作台，用于管理项目、会话、权限、文件与媒体预览、扩展、账号和自动化任务。

![Sunsetz icon](assets/logo.png)

## 功能

- 多项目、多会话、搜索、置顶、归档、分叉与时间线回退
- Sunsetz 三层输入器：项目/目标状态、附件与可增长编辑区、权限/上下文/模型/发送控制分层呈现
- 对话内活动时间线：按真实顺序展示并汇总技能、上下文压缩、文件、命令、图像、浏览器与子任务活动
- 可恢复的 Agent 提问：逐题作答、跳过与取消语义分离，切换会话或重载后仍可继续，后台待回答会话有标记
- 附件元数据随消息持久化；文件、文件夹与 macOS Finder 所选项均使用真实原生入口
- Ask、单次允许、会话允许、拒绝及无人值守权限模式
- Markdown、代码、图片、视频、PDF、Office 文件和内嵌网页预览
- 底部计划确认、执行步骤进度、资源面板计划文档、会话生成技能、MCP、插件、自定义提供商和计划任务
- 账号、额度、活动热力图及多账号切换
- 简体中文、繁体中文、英文
- 浅色、深色和高对比度主题

## 开发

要求 Node.js 22+、pnpm 9、Rust stable。macOS 构建还需要 Xcode Command Line Tools。

```bash
pnpm install
pnpm dev
```

仅运行前端：

```bash
pnpm dev:ui
```

使用模拟 Runtime：

```bash
SUNSETZ_ACP=mock pnpm dev
```

验证：

```bash
pnpm typecheck
pnpm test
pnpm build:ui
pnpm verify:contracts
pnpm test:visual
cd src-tauri && cargo test
```

应用数据可通过 `SUNSETZ_HOME` 覆盖。底层 Runtime 兼容边界见 [运行时兼容说明](docs/runtime-compatibility.md)，工作台与会话交互约束见 [工作台会话说明](docs/llm-wiki/workbench-conversation.md)。

## 许可证与品牌

代码基于 RongleCat 的 MIT 许可桌面工作台快照改造，原始版权和许可声明保留在 [LICENSE](LICENSE)。Sunsetz 名称与品牌资产不包含在 MIT 商标授权中，详见 [TRADEMARKS.md](TRADEMARKS.md)。
