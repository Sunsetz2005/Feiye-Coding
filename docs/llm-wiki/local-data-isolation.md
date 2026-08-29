# 本机用户数据隔离

Sunsetz 的 MCP 配置、自定义模型密钥、连接器 token 和会话 journal 属于**当前操作系统用户**，不得进入 GitHub。

## 落点

| 数据 | 位置 |
|---|---|
| 应用数据（会话、项目、设置、MCP prefs、agent-home） | macOS `~/Library/Application Support/dev.sunsetz.desktop/`；Windows `%APPDATA%\Sunsetz`；否则 `~/.sunsetz` |
| GitHub PAT / Google OAuth 令牌 / 连接器凭据 | OS Keychain + `connector-credentials.json`（0600）；Gmail / Drive / Calendar 共用 `google` 键 |
| 模型 API key | OS Keychain，磁盘只留 has-key 标记 |
| 独立模式模型/MCP 配置 | `{app_data}/agent-home/config.toml` |
| 共享模式 | 仅当前用户的 `~/.grok`，不跨账号 |

`SUNSETZ_HOME` 只能指到**当前用户主目录**或进程临时目录。落在 git 仓库、`/Users/Shared`、其他用户主目录时会被忽略，并退回上述默认根。应用数据目录权限为 `0700`。

## 禁止上传

`.gitignore` 与 `.githooks/pre-commit` 拦截：`.env`、`secrets.json`、`auth.json`、`connector-credentials.json`、`connectors.json`、`extensions.json`、MCP 配置、`config.toml`、支持包 zip、密钥文件。

仓库根在 `/Users/Shared/...` 时，那只是源码检出，不是运行时数据根。不要把 `SUNSETZ_HOME` 指到检出目录。

## 验证

```bash
scripts/check-local-data-isolation.sh
```
