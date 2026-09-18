# 应用内弹窗（禁止 window.confirm / prompt）

**强制**：Tauri WebView 下 **`window.confirm` / `window.prompt` / `window.alert` 不可靠**（常无对话框、恒为 false、或阻塞异常）。  
用户确认、输入、危险操作 **必须** 使用应用内弹窗，禁止再引入浏览器原生对话框。

## 视觉：复用现有面板样式

**不强制** 毛玻璃 / 半透明浮层。新浮层 **优先复用** 应用内已有面板样式，与邻近控件保持一致：

| 场景 | 优先样式 | 参考 |
|------|----------|------|
| Composer 芯片菜单（模型 / 权限 / 项目） | `.cmm__pop` + `.cmm__opt` / `.cmm__section` | `ComposerModelMenu`、`ComposerProjectMenu` |
| 右键 / 行操作 / 位置菜单 | 实心 `.menu-panel` + context tokens（`--menu-context-*`） | `ContextMenu`、`OpenLocationButton` |
| 确认 / 输入 / 业务对话框 | `.modal` · `GlassModal` · `setAppDialog` | `App.tsx`、`GlassModal` |
| 搜索 / 侧栏表单 / 斜杠 | 现有 `.search-panel` / `.auto-panel` / `.slash-palette` | 对应组件 |

布局 token（圆角、pad、item 间距）仍可用 `--menu-*` / `--modal-*`；材质以**该区域既有实现**为准，不要为「统一毛玻璃」另起一套。

**可选**：存量仍有 `.glass-surface` / `--glass-*`（部分 modal、历史浮层）。新代码不要求套用；也**不要**再写「浮层禁止不透明底」之类规则。

## 公共壳：`GlassModal`

业务对话框可用公共壳（名字历史遗留，不代表必须毛玻璃）：

```tsx
import { GlassModal } from "@/components/GlassModal";

<GlassModal
  open={open}
  onClose={onClose}
  title={tr("…")}
  size="sm" | "md" | "lg"   // 420 / 480 / 560
  closeLabel={tr("common.close")}
  footer={
    <>
      <button type="button" className="btn btn--ghost" onClick={onClose}>
        {tr("common.cancel")}
      </button>
      <button type="button" className="btn btn--solid" onClick={onSave}>
        {tr("common.save")}
      </button>
    </>
  }
>
  {/* 业务内容 */}
</GlassModal>
```

结构：`.overlay` → `.modal.glass-modal[--sm|--md|--lg]` → `header.modal-head` + body + `.modal-actions`。

`StatusModal` / `McpStatusModal` 的开关状态，以及 MCP inspect 的 `loading` / `error` / `servers`，由 `useStatusModals` 集中管理；`App.tsx` 只传当前项目路径并渲染组件。

Compact 弹窗的开关、备注、输入焦点和 Escape 关闭由 `useCompactModal` 管理，原有 DOM/CSS 由 `CompactModal` 渲染；`App.tsx` 仍负责连接会话并发送 `/compact`，避免把 Host 协调塞进展示组件。

存量也可用同一 DOM/CSS（不强制立刻迁组件）：

```html
<div class="overlay">
  <div class="modal app-dialog" role="dialog">…</div>
</div>
```

## 首选：App 级 `appDialog`（`src/hooks/useAppDialog.ts` + `src/components/AppDialogHost.tsx`）

状态、类型和键盘（Escape/Enter）逻辑在 `useAppDialog` hook；`createPortal` 渲染在 `AppDialogHost` 组件；`App.tsx` 只调一次 hook 并渲染一次 `<AppDialogHost />`（已从 `App.tsx` 抽出，行为不变）。

工作台内主流程（项目 / 会话重命名、YOLO 二次确认等）使用：

```ts
setAppDialog({
  kind: "confirm",
  title: tr("…"),
  message: tr("…", { name }),
  confirmLabel: tr("…"), // optional
  danger: true,          // optional → 危险按钮样式
  onConfirm: () => { void doSomething(); },
});

// 或输入
setAppDialog({
  kind: "prompt",
  title: tr("…"),
  initial: current,
  placeholder: tr("…"),
  onSubmit: (value) => { void rename(value); },
});
```

- 渲染：`createPortal` → `.app-dialog-overlay` + `.modal.app-dialog`。  
- 文案：全部走 `src/i18n/`（见 [i18n.md](./i18n.md)）。  
- **禁止** 在 `onConfirm` / `onSubmit` 里再套 `window.confirm`。

## 子页面 / 独立面板

若组件拿不到 `setAppDialog`（如 `AutomationsPage`）：

1. **优先**：通过 props 回调把确认上抛到 `App`（`onRequestConfirm`），由 `appDialog` 统一处理。  
2. **可接受**：组件内用同一套 DOM/CSS 自建确认（`createPortal` + `overlay` / `modal app-dialog`），或 `GlassModal`。  
3. 参考：`AutomationsPage` 删除确认（禁止 `window.confirm`）。

## 浮层清单（改样式时勿漏）

| 类型 | 选择器 / 组件 |
|------|----------------|
| App 确认/输入 | `.modal.app-dialog` · `setAppDialog` |
| Compact / Doctor / Status / MCP | `.modal` · `CompactModal` · `DoctorModal` · `GlassModal` · `useCompactModal` · `useStatusModals` |
| 文件详情 | `.modal.file-path-details` |
| 搜索面板 | `.search-panel` |
| 模型 / 权限 / 项目 / 用户 / 斜杠 / + | `.cmm__pop` · `.menu-panel` · `.slash-palette` · `.composer-plus` |
| 上下文 / 附件 / 打开位置 / Select | `.ctx-menu` · `.att-menu` · `.open-loc-menu` · `.c-select__menu` |
| 自动化表单侧栏 / 行菜单 | `.auto-panel` · `.auto-row__menu` |
| Toast / 权限条 / 拖放卡 | `.app-toast` · `.perm-bar` · `.drop-overlay__card` |
| 左栏 | `.sidebar` |

## 禁止清单

| API / 模式 | 状态 |
|------------|------|
| `window.confirm(...)` | **禁止** |
| `window.prompt(...)` | **禁止** |
| `window.alert(...)` | **禁止**（用户可见错误用 toast / error banner / 应用内 dialog） |
| `confirm` / `prompt` 全局别名 | **禁止** |

存量调用发现即改（搜索 `window.confirm`、`window.prompt`）。

## 验收

- [ ] 新增删除 / 信任 / 危险开关等路径均有应用内确认，无 `window.confirm`。  
- [ ] 确认框文案中英键齐全。  
- [ ] Tauri 真机：点确认执行、点取消/遮罩关闭、无「无反应」。  
- [ ] 危险操作（删除任务、YOLO、移除项目）使用 `danger` 样式并写清后果。  
- [ ] 新浮层与同区域既有面板（`.cmm__pop` / `.menu-panel` / `.modal`）观感一致，不另造透明材质规范。

## 相关源码

- `src/components/GlassModal.tsx` — 公共对话框壳  
- `src/hooks/useAppDialog.ts` — `AppDialog` 类型、状态、Escape/Enter 键盘处理  
- `src/components/AppDialogHost.tsx` — portal 渲染（confirm/prompt/edit-project 三种表单）  
- `src/hooks/useStatusModals.ts` — Status / MCP 开关及 MCP 探测状态；保持 Host 调用与组件接口不变
- `src/hooks/useCompactModal.ts` — Compact 开关、备注、焦点与 Escape 关闭状态
- `src/components/CompactModal.tsx` — Compact 原有 DOM/CSS 和提交表单；Host 发送仍上抛给 `App.tsx`
- `src/App.tsx` — 调用 `useAppDialog()` 并渲染 `<AppDialogHost />`；24 处 `setAppDialog(...)` 调用点仍在此文件  
- `src/styles/tokens.css` — `--menu-*` / `--modal-*` / 可选 `--glass-*`  
- `src/styles/app.css` — modal / menu / cmm 布局  
- `src/components/ComposerModelMenu.tsx` / `ComposerProjectMenu.tsx` — composer 芯片菜单范例  
- `src/components/StatusModal.tsx` / `McpStatusModal.tsx` — GlassModal 范例（MCP 弹窗可跳转 Settings → Extensions）
- `src/components/ExtensionsPanel.tsx` — Settings → Extensions 全页技能 / MCP 管理  
- `src/components/AutomationsPage.tsx` — 子页面自建删除确认范例  
- `src/i18n/messages.ts` — `common.cancel` / `common.confirm` / `common.close` 等  
