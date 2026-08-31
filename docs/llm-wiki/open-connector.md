# Open Connector 与插件调用

侧栏市场连接器由 Host 拥有。默认内核 `agent_loop` 在已连接时注入工具；`runtimeBackend=grok_acp` 不走这条路径。

## 产品口径

- **GitHub** 是 Host 适配器：在 App 内粘贴 PAT（经典 `ghp_` 或细粒度 `github_pat_`），存在 Keychain 或 `connector-credentials.json`（0600）。校验先打 `/user`，细粒度无 profile 权限时改打 `/rate_limit`。走系统代理（`HTTPS_PROXY`），不用 `.no_proxy()`。网络失败是 `CONNECTOR_UNREACHABLE`，401/403 才是 `CONNECTOR_AUTH_FAILED`。不写入 journal 或 UI 日志。
- **Notion** 是 Host 适配器：粘贴 Internal Integration Token（`secret_` / `ntn_`），校验 `GET /v1/users/me`（`Notion-Version: 2022-06-28`）。工具：`notion_search`、`notion_get_page`、`notion_create_page`。写操作走权限条，`AcceptEdits` 不自动放行。不是浏览器 OAuth。
- **Slack** 是 Host 适配器：粘贴 Bot Token（`xoxb-`），校验 `POST /api/auth.test`。Slack 的 `{ok:false,error:invalid_auth}` 记为 `CONNECTOR_AUTH_FAILED`，不能当成功。工具：`slack_list_conversations`、`slack_get_conversation`、`slack_post_message`。写操作走权限条，`AcceptEdits` 不自动放行。不是浏览器 OAuth。
- **Gmail、Google Drive、Google Calendar** 共用一次 Google 登录（loopback PKCE，带 refresh）。点击连接后打开系统浏览器；已连接其中一个再连另一个时，只补该应用缺少的 scope，不重复整段登录。产品内置桌面 OAuth 客户端 ID（`src-tauri/oauth/google-desktop-client-id.txt`，公开 PKCE 值，不是密钥）。真连须在同一 Google Cloud 项目启用 Gmail、Drive、Calendar API 并把对应 scope 加到同意屏幕。`SUNSETZ_GOOGLE_OAUTH_CLIENT_ID` 只给测试覆盖。令牌写在共享键 `google`（Keychain / 0600 文件）；旧的 `gmail` 键仍可读。断开某一个只关掉该应用的工具；最后一个 Google 应用断开时才删除共享令牌。写操作走权限条，`AcceptEdits` 不自动放行。市场文案不得出现环境变量名。
  - Gmail：`gmail_list_messages`、`gmail_get_message`、`gmail_create_draft`
  - Drive：`google-drive_list_files`、`google-drive_get_file`、`google-drive_create_file`
  - Calendar：`google-calendar_list_events`、`google-calendar_get_event`、`google-calendar_create_event`
- **其他目录项**（Outlook、Granola、Fireflies、Plaud）在市场里显示「即将在 App 内连接」，不得要求用户安装 Open Connector。
- 目录字段 `connectKind`: GitHub、Gmail、Drive、Calendar、Notion、Slack 为 `in_app`，其余为 `coming`。探活失败或工具表为空时，不得标成已连接。
- 市场 logo 来自各产品官网并打包在 `src/assets/connectors/`（按 catalog id 命名），`ConnectorLogo` 渲染图片；没有资源时才回退字母。这些标记仅用于识别对应服务。
- grok CLI `plugin install` / uninstall 仍 fail-closed。
- MCP 仍只注入 ACP 会话，不进入默认内核。

## 两条调用路径

1. **自动**：已连接工具进入该轮 `tools`。模型可自行调用，不要求 `@`。
2. **明确 `@`**：输入器 `@github` 打开面板，已连接项插入 `[[connector-v1:github|explicit]]` 芯片。`session_send_v2.connectorSelections` 必须与 journal 的 `[[connector:github]]` 一致。未连接 id 返回 `CONNECTOR_NOT_CONNECTED`，整轮不发送。

`@` 只在行首或空白后触发。`user@gmail.com` 不打开面板。未连接项可点进市场，不能插芯片。

## Open Connector 协议（内部 leftover，非产品路径）

产品连接走 Host 适配器（GitHub / Notion / Slack 粘贴令牌，以及 Gmail / Drive / Calendar 的 Google 登录）。loopback sidecar **不是** Cursor/Codex 式连接，市场文案不得引导用户下载或启动它。若仍设置环境变量，`SUNSETZ_OPEN_CONNECTOR_URL` 必须是 `http(s)://127.0.0.1`、`::1` 或 `localhost`。

- `GET /health` → `200 { "ok": true }`
- `GET /v1/connectors/{slug}/tools` → `{ "tools": [ OpenAI function tools ] }`
- `POST /v1/connectors/{slug}/invoke` `{ "name", "arguments" }` → `{ "ok": true, "output" }` 或 `{ "ok": false, "error" }`

工具名必须 `{slug}_` 前缀，且不得覆盖 `read_file` / `list_directory` / `grep` / `write_file` / `search_replace` / `run_command`。连接超时 8s，调用 30s，输出截到 32_768 字符。

## 权限

连接器工具走现有权限条。`AcceptEdits` 不自动放行连接器写操作（含 `github_create_issue`、`gmail_create_draft`、`google-drive_create_file`、`google-calendar_create_event`、`notion_create_page`、`slack_post_message`）。`AlwaysApprove` 仍放行。只读 GitHub / Google / Notion / Slack 工具可走会话缓存。

## 测试入口

GitHub API 可用 `SUNSETZ_GITHUB_API_URL` 指向 mock。Gmail / Drive / Calendar 分别用 `SUNSETZ_GMAIL_API_URL`、`SUNSETZ_DRIVE_API_URL`、`SUNSETZ_CALENDAR_API_URL`；OAuth 用 `SUNSETZ_GOOGLE_OAUTH_*`。Notion / Slack 分别用 `SUNSETZ_NOTION_API_URL`、`SUNSETZ_SLACK_API_URL`。Open Connector 用 loopback HTTP mock。应用测试不得要求 grok 二进制。
