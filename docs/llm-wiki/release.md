# 发版与 Release 维护（AI 必读）

> 任何 Agent 接手发版时**只读本文件 + `docs/BUILD.md` + `CHANGELOG.md`**，不要靠会话记忆。

## 目标

- 每次正式版 = **一个 git tag `vX.Y.Z`** + **GitHub Release** + **多平台安装包**  
  - macOS Apple Silicon (`aarch64` `.dmg`)  
  - macOS Intel (`x64` `.dmg`)  
  - Windows x64 安装版 (`*-setup.exe`) + **绿色版** (`*-portable.zip`)  
  - Linux x64：**AppImage** + **.deb**（Debian/Ubuntu 系）+ **.rpm**（Fedora/RHEL 系）
- Release 正文**只保留本版变更**（`CHANGELOG.md` 对应 `## [X.Y.Z]` 章节）。  
  下载资产由 GitHub 自动挂在下方；安装 / Gatekeeper / SmartScreen / CLI 说明见 README，**不要**在每个 Release 重复长文。

正文由 CI 调用 `scripts/changelog-for-release.py` 自动生成，**禁止**在 workflow 里写死静态 Release body 覆盖 CHANGELOG。

## 版本号三处同步

| 文件 | 字段 |
|------|------|
| `package.json` | `version` |
| `src-tauri/tauri.conf.json` | `version` |
| `src-tauri/Cargo.toml` | `[package].version` |
| `src/i18n/messages.ts` / `src/i18n/zh-tw.ts` | `app.versionFooter` 内 `Sunsetz vX.Y.Z`（英 / 简中 / 繁中） |

`scripts/release-tag.sh` 会改以上文件。Tag 格式：`v0.1.0`（`v` + semver）。

## CHANGELOG 写法（强制）

文件：`CHANGELOG.md`（[Keep a Changelog](https://keepachangelog.com/) + SemVer）。

每个正式版在 tag **之前**必须有：

```markdown
## [X.Y.Z] - YYYY-MM-DD

> 中英文对照。English first，再写 **中文 · …** 摘要。
>
> **Highlight:** 一句话亮点。

### Added
- …

### Changed
- …

### Fixed
- …

### Notes
- 非官方、CLI 依赖等说明

**中文 · 新增**
- …

**中文 · 变更**
- …

**中文 · 修复**
- …
```

规则：

1. **没有对应 `## [X.Y.Z]` 章节 → 禁止 tag**（`release-tag.sh` 与 CI 都会 fail）。  
2. 列表要**可验收**：用户/AI 读完知道本版改了什么，不要空话。  
3. 发版当天把 `[Unreleased]` 里准备进本版的条目**挪进** `## [X.Y.Z]`。  
4. 后续每次功能合并，Agent 应在 PR/提交中**同步改 Unreleased 或即将发的版本节**。  

生成 Release 预览（本地）：

```bash
python3 scripts/changelog-for-release.py 0.1.0
```

## 标准发版步骤（复制即用）

```bash
# 0) 工作区干净、main 最新
git checkout main
git pull origin main   # 若已有远程历史
git status             # 必须 clean

# 1) 写好 CHANGELOG.md → ## [X.Y.Z] - 日期
# 2) 自测（至少）：
pnpm typecheck && pnpm test
# 可选本地装包：pnpm build:mac-arm / pnpm build:win

# 3) 打 tag（会 bump 版本并 annotated tag）
./scripts/release-tag.sh X.Y.Z
# 确认后推送：
./scripts/release-tag.sh X.Y.Z --push
# 或：git push origin HEAD && git push origin vX.Y.Z
```

若当前 **package 版本已经是 X.Y.Z** 且 CHANGELOG 已写好，只需：

```bash
git tag -a vX.Y.Z -m "Release vX.Y.Z"
git push origin main
git push origin vX.Y.Z
```

（或仍走 `release-tag.sh`，脚本会校验 CHANGELOG。）

## CI 行为

| 工作流 | 触发 | 作用 |
|--------|------|------|
| `.github/workflows/ci.yml` | push/PR → main | typecheck、test、`build:ui`、mac/win `cargo test` |
| `.github/workflows/release.yml` | tag `v*` 或手动 | 矩阵：mac×2 + win（setup+portable）+ linux（AppImage/deb/rpm）→ 同一 Release |

Release job 关键：

1. 从 tag 名或 `package.json` 解析版本  
2. `python3 scripts/changelog-for-release.py "$VER"` → `RELEASE_BODY`  
3. `tauri-apps/tauri-action` 构建并挂资产  

### 仓库权限（人类一次性配置）

GitHub → **Settings → Actions → General → Workflow permissions**  
→ **Read and write permissions**（否则无法创建/更新 Release）。

未配置 Apple 签名时 macOS 包**未公证**，属预期；Release 正文必须带 `xattr` 说明。

## macOS「已损坏 / 无法打开」

未签名下载后 Gatekeeper 可能拦截。**用户说明放在 README**（不要每个 Release 正文再贴一遍）：

```bash
xattr -cr /Applications/Sunsetz.app
open /Applications/Sunsetz.app
```

改安装说明时改 `README.md` / `README_EN.md`。

## Windows 说明

- **安装版** NSIS + **绿色版** zip（解压即用）均上传到同一 Release。  
- SmartScreen 可能提示未知发布者 →「更多信息」→「仍要运行」。  
- 需 **WebView2**（Win10/11 多已预装）。  
- 真 Agent 需本机 **Grok Build CLI**（`grok.exe`）。

## Linux 说明

- **AppImage**：通用桌面；`chmod +x` 后运行。  
- **.deb**：Ubuntu / Debian / Mint / Pop!_OS 等。  
- **.rpm**：Fedora / RHEL / openSUSE 等。  
- CI 使用 `ubuntu-22.04` + `rpm` 工具链打出三种格式。

## 本地交叉编译（可选，不替代 CI）

见 [docs/BUILD.md](../BUILD.md)。macOS 上 Windows：

```bash
export PATH="/opt/homebrew/opt/llvm/bin:$PATH"
pnpm setup:cross
pnpm build:win   # tauri + cargo-xwin + makensis
```

注意：`macos-private-api` feature 与 `tauri.conf.json` 的 `macOSPrivateApi: true` 必须一致，否则 Windows 交叉会在 build-script 校验失败。

## 发版后检查清单

- [ ] Actions `release` 四个 job 全绿（macOS-ARM64 / macOS-x64 / Windows-x64 / Linux-x64）  
- [ ] GitHub Release 页含：两 dmg、setup.exe、portable.zip、AppImage、deb、rpm  
- [ ] Release body 仅为该版本变更列表（无整页下载表/安装长文）  
- [ ] README 下载链接指向 Releases（相对路径已写）  
- [ ] 版本号与 tag 一致  

失败时：

| 现象 | 处理 |
|------|------|
| Resource not accessible | 打开 workflow 写权限 |
| no CHANGELOG section | 补章节后删 tag 重打 |
| macOS 证书 import 失败 | 勿传空 `APPLE_*` secrets |
| 前端 typecheck 挂 | 本地 `pnpm typecheck` 修后推修丁 tag 或新 patch 版 |

## 禁止事项

- 不在未写 CHANGELOG 时 tag  
- 不把 `secrets.json` / `auth.json` / 真实 API key 打进仓库  
- 不把 `dist-installers/`、`src-tauri/target/` 提交进 git  
- 不用 `window.confirm` 等（产品规则见 dialogs.md）  
- 不手写覆盖 CI 生成的 Release body（改脚本 + CHANGELOG）

## 相关路径速查

| 路径 | 用途 |
|------|------|
| `CHANGELOG.md` | 版本更新列表 SoT |
| `scripts/changelog-for-release.py` | Release body = 该版本 CHANGELOG 章节（精简） |
| `scripts/release-tag.sh` | bump + tag |
| `.github/workflows/release.yml` | 三端构建与上传 |
| `docs/BUILD.md` | 本地构建细节 |
| `README.md` / `README_EN.md` | 用户安装与 Gatekeeper |
