# 阶段一 Windows 实机验收

本验收必须在交互式物理 Windows 设备上执行。GitHub Actions 的 Windows runner、WebView2 CDP 和虚拟机截图只能作为自动诊断，不能替代本记录。

## 前置条件

- 使用与待验收提交一致、工作区干净的 Sunsetz 调试包或安装包。含未提交补丁的 exe 不能代表该提交。
- 该提交必须能在 Windows 上 `pnpm tauri build --debug` 成功。指定提交若编不过，先等包含 Windows 编译修复的新 SHA，不要用本地私改包签收。
- 在 Windows 设置中把 Sunsetz 所在显示器缩放设为 200%。
- 关闭无关窗口并打开空任务，避免截图包含私人项目、凭据、浏览器或文件选择器。
- `display` 项只截 Windows 显示缩放页，不要打开账户页。200% 后若设置页展开含姓名/邮箱的导航，把窗口缩小或裁到只露「缩放」下拉；含账户信息的截图视为无效。
- `GetDpiForSystem` 只能辅助核对。PowerShell 记录 96（100%）不能单独否决设置页已显示 200% 的合规截图。
- 保持 Sunsetz 为还原窗口，不使用最大化掩盖工作区和响应式问题。
- 七项都要给出 PASS 或 FAIL。未做完的检查记 FAIL，不要把「没测完」写成通过。
- 不要把本清单粘贴进 Sunsetz 对话里跑。Sunsetz 不能验收自己。

## 执行

在仓库根目录运行：

```powershell
pwsh -File .\scripts\windows-stage1-manual.ps1 `
  -Executable "C:\path\to\sunsetz.exe"
```

脚本逐项引导检查：

1. Windows 显示设置中的 200% 缩放证据。
2. 200% 缩放与系统工作区边界。
3. Tab、Shift+Tab、Enter、Space 和 Escape 键盘路径及可见焦点。
4. 资源面板关闭后的卸载与触发器焦点恢复。
5. 侧栏隐藏后的不可聚焦状态、焦点恢复和 Space 重开。
6. 窄窗口覆盖层、会话滚动保持和输入器可见性。
7. 深色、浅色、高对比度和中性项目选中态。

每项操作完成后脚本截取桌面并要求明确输入 `PASS` 或 `FAIL`。任何失败都会使脚本返回非零状态。

## 证据

默认输出到被 Git 忽略的 `test-results/windows-stage1-manual-<时间>/`：

- `manifest.json`：提交、可执行文件版本与 SHA-256、系统、DPI、显示器工作区和逐项结论。
- `report.md`：供审阅的结果表。
- `display.png`、`scale.png`、`keyboard.png`、`resources.png`、`sidebar.png`、`responsive.png`、`themes.png`：对应步骤截图。

验收者必须人工确认 Windows 显示设置确为 200%。`GetDpiForSystem` 的记录用于辅助核对，不单独证明每显示器缩放。

只有七项全部通过、截图无敏感信息且 `manifest.json` 对应当前提交时，才能把阶段一的 Windows 实机门禁标记为完成。

## 从 macOS 开发机怎么做

当前 Sunsetz 开发机是 macOS。下面这些**不能**把本门禁标为完成：UTM / Parallels / ARM 虚拟机、GitHub Actions `windows-native-smoke`、本机 Playwright。

可行路径只有一条：把与待验收提交一致的调试包拿到**物理 Windows PC**，显示器缩放到 200%，运行上面的 `windows-stage1-manual.ps1`。

建议步骤：

1. 在 macOS 记下提交：`git rev-parse HEAD`。
2. 在 Windows 上检出同一提交，或拷贝已构建的 `sunsetz.exe`。
3. Windows 上构建调试包（若没有现成 exe）：

```powershell
pnpm install
pnpm tauri build --debug
```

4. 关闭无关窗口，缩放 200%，还原窗口运行验收脚本。
5. 把 `test-results/windows-stage1-manual-*` 拷回（该目录已被 gitignore）。七项 PASS 后再改阶段状态。

CI 原生 smoke 仍应跑，作为额外诊断，不是本页的完成证明。
