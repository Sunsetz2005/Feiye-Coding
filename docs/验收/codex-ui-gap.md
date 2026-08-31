# Codex ↔ Sunsetz UI 对照

对照 Codex 桌面工作台（本机 `/Applications/ChatGPT.app`，bundle id `com.openai.codex`）与 Sunsetz 当前工作台。只校准信息层级、侧栏职责和局内交互，不复制品牌、源码、图标或未实现入口。

本页不是阶段完成证明。没有新的 Codex 真机截图；第 2 节用官方文档 + 仓库已有参考图审计重建信息层级。

## 证据与限度

| 来源 | 用处 | 限度 |
|------|------|------|
| OpenAI Codex app 文档（Features / Review / Commands / Settings） | 官方信息架构、快捷键、右栏职责 | 官方站点本机解析被拦截，细节来自公开文档摘录，不是逐像素截图 |
| [`reference-screenshot-audit.md`](./reference-screenshot-audit.md) 图 1–32 | 2026-07-26 参考产品剪贴板截图的可见事实 | 图已不随仓库分发；只保留当时映射，不能当 2026-08 最新 Codex UI |
| Sunsetz 代码与 [`agent-gui-reference.md`](../llm-wiki/agent-gui-reference.md)、[`structural-inventory.md`](./structural-inventory.md) | 当前落地 | 以代码为准 |
| 本机 ChatGPT.app | 确认对标对象就是 Codex 桌面端 | 本回合未打开窗口、未截图、未拆包 |

状态口径：

- **已对齐**：Sunsetz 已有同等职责的界面，品牌和文案保持 Sunsetz。
- **可做**：Host 或 UI 已有数据/命令，差的是密度、入口位置或恢复。
- **缺后端**：没有版本化能力或真实命令前，不得做装饰入口。
- **明确不做**：产品边界禁止，或会破坏已落地的 Sunsetz 规则。

---

## 1. 对照表

### 1.1 整窗骨架

| Codex | Sunsetz | 状态 |
|-------|---------|------|
| 三栏：左去哪、中干活、右看结果 | `WorkbenchShell`：侧栏 / 会话中栏 / `ResourceViewer` | 已对齐 |
| 顶栏偏 IDE：项目、模式、模型、diff、终端 | 顶栏只有任务标题、更多菜单、左右面板开关 | 已对齐（有意更克制）；模型在 composer，不搬到顶栏常驻 |
| 底栏可出现内置终端（⌘J） | 无底部终端抽屉 | 缺后端 + 明确不做（本阶段）。命令走权限条和活动时间线，不内嵌 shell |
| 窄屏仍保持三栏职责，面板可关 | 900×600 侧栏留在布局内；资源面板窄屏覆盖；关闭后 `inert` 卸载 | 已对齐 |
| 品牌 ChatGPT / Codex | Sunsetz coral，不出现 Codex 名称 | 明确不做复制品牌 |

### 1.2 左栏

| Codex | Sunsetz | 状态 |
|-------|---------|------|
| 顶部：Threads / Skills / Automations 一类模块入口 | 新建、搜索、已安排、插件市场（精选连接器 + 详情三条提示词） | GitHub / Notion / Slack（粘贴令牌）与 Gmail / Drive / Calendar（Google 登录）在 App 内连接并注入默认内核；其余项显示即将在 App 内连接，不要求本机 Open Connector |
| 项目列表，悬停三点：Remove，⌘O 再添加 | 项目行无披露箭头；栏标题悬停折叠；悬停三点/新对话；编辑名与源文件夹；磁盘不删 | 已对齐密度；不创建 worktree、不做远程项目 |
| 项目下线程列表；过滤器（含 Chronological） | 项目下任务虚拟列表；后台待回答标记 | 可做：缺线程状态过滤（进行中 / 待审阅 / 时间序）。数据已有 session 状态，不必等新后端 |
| 线程搜索，文档中曾为 ⌘G；较新文档改为可自定义、默认未指定 | ⌘K 搜会话/项目；FTS5 可搜可见正文 | 已对齐能力；快捷键不要改成 Codex 的 ⌘G（Sunsetz ⌘K 已是命令搜索） |
| 归档线程在 Settings | 设置「已归档」栏目 + 侧栏可显示归档 | 已对齐 |
| 侧栏 Skills：浏览团队/项目 skill | 加号/`/` 可调用已信任 skill；设置扩展里有 skill 列表；无独立 Skills 浏览页 | 可做浏览页，但清单不得带正文或本地路径；安装/市场 **缺后端** |
| 侧栏 Automations：收件箱 + 创建流 | 「已安排」页 + 应用内调度账本 | 已对齐入口；Codex 式「唤醒同一线程」**缺后端**（现策略过期 claim 不自动 replacement） |
| 无品牌色左边框的中性选中 | 项目/任务中性整行背景，coral 不用于选中 | 已对齐 |
| 任务/线程悬停预览 | 450ms 停留预览；键盘立即等价；项目 Git 摘要惰性读取；会话菜单可移动到已有项目 | 已对齐能力；原生窗口验收仍缺 |

### 1.3 中栏与局内交互

| Codex | Sunsetz | 状态 |
|-------|---------|------|
| 低噪声消息轨 | `ConversationThread` + `ActivityTimeline` | 已对齐 |
| 底部 composer，运行中仍可输入 | 三层 `ComposerDock`；streaming 可排队；排队条有调整方向 / 删除 / 编辑 | 已对齐；侧边聊天明确不做；调整方向先停本轮再发队首 |
| 加号 / 附件 / 斜杠命令 | 加号与 `/` 语义分离；加号只出真实入口 | 已对齐 |
| `/plan`、`/review`、`/mcp`、`/status`、`$skill` | `/plan` `/goal` `/compact` `/status` `/mcp` `/doctor` `/new` `/automations` `/settings` `/yolo` + 已信任 skill | 可做：`/review` 没有。Changes 面板已在，不要先做空的 review 模式 |
| 权限提示 | `ComposerDock` 权限条；`AcceptEdits` 不自动放行命令 | 已对齐（Sunsetz 语义，不是 Codex 文案） |
| Agent 提问 | 底部 `AskUserDock`，与 composer 同宽 | 已对齐 |
| 计划确认 | 同一 `AskUserDock`；卡片和右栏只读 | **明确不做**右栏第二决策入口 |
| 运行中进度：plan / sources / artifacts / summary | `TaskProgressRail` 只用可证明步骤；无 sources 侧栏 | 可做步骤密度；sources **缺后端**（没有独立引用图） |
| 侧边线程 / 独立聊天（⌘⌥S / ⌘⌥O） | 无；多会话在左栏切换 | 明确不做侧边窗口，参考图审计已禁止未实现入口 |
| 语音输入 ⌃M | `speechRecognition: false` | 缺后端；未完成前不显示 |
| 电脑使用 / Appshots | 能力表 `unavailable` | 缺后端 |
| 线程内查找 ⌘F | 无当前会话内查找 | 可做：正文已在 journal，不必新协议 |

### 1.4 右栏 / 任务侧栏

Codex 右栏是「当前线程的结果舱」，不是通用 IDE。Sunsetz 右栏是 Files / Changes / Plan 三个 mode。无打开文件时，预览区是 Files / 审查 / 计划（有产物才出现）动作列表，不是空白「尚未打开文件」。不做终端或浏览器入口。

| Codex | Sunsetz | 状态 |
|-------|---------|------|
| Files / 文件树 | Files + 预览/编辑/Reveal；空态可从动作列表打开文件树 | 已对齐 |
| Review pane：Uncommitted / All branch / Last turn；Unstaged / Staged | Changes：Session 工具改动 + Workspace git；打开编辑器、Reveal、复制路径；回合结束后对话内摘要卡打开 Changes | 可做 last-turn / 分支范围。**不**做静默 discard / Undo |
| 行内评论 → 让 Agent 改 | 无 | 缺后端：没有「选中 diff 行发回同一轮」的版本化契约 |
| stage / commit / push / 开 PR | 无应用内 Git 写操作 | 缺后端；危险 Git 必须确认。阶段 11 才考虑 PR |
| 运行中 task sidebar：plan、sources、artifacts、summary | Plan 只读全文；无 sources；artifacts 仅计划卡 + 文件预览 | 可做：把 approved/executing/done 产物状态投影到 Plan tab（上一份 Plan schema 计划）。sources 缺后端 |
| 非代码产物预览：PDF / Office / 图片 | 已有代码、Markdown、图片、媒体、PDF、Office 预览 | 已对齐预览族；交互式 HTML、大文件虚拟化仍缺 |
| 大 diff 一次只展示部分文件 | 无 Codex 那种 Large-diff 限流文案 | 可做，属于 Changes 性能，不是复制文案 |

### 1.5 Composer 细节

| Codex | Sunsetz | 状态 |
|-------|---------|------|
| 项目/worktree 作为线程工作区 | 上层项目条；worktree 只在 `ComposerProjectMenu` 列出已有 worktree，不创建 | 已对齐 MVP；创建/删除 worktree **明确不做** |
| 模型入口在顶栏或线程头 | 底层紧凑模型级联；无独立 speed | 已对齐 |
| 上下文/用量 | 14px 实线圆环；悬停三行摘要；点击才打开 Compact 详情；无容量不估算 | 已对齐 |
| 计划模式开关 | 访问权限右侧的计划按钮，悬停可关 | 已对齐 |
| IDE context / Auto Context 同步 | 无 VS Code 活动文件注入 | 缺后端；不要做假的「当前文件」芯片 |

### 1.6 快捷键

Sunsetz 帮助面板只登记已工作的绑定。不要为了对齐 Codex 改掉已有 ⌘K。

| 动作 | Codex（macOS，官方） | Sunsetz | 状态 |
|------|---------------------|---------|------|
| 命令/搜索 | ⌘⇧P / ⌘K；线程搜索曾为 ⌘G | ⌘K 搜会话/项目 | 已对齐 ⌘K；不做第二套命令面板 |
| 设置 | ⌘, | ⌘, | 已对齐 |
| 快捷键说明 | ⌘/ | ⌘/ | 已对齐 |
| 新建 | ⌘N / ⌘⇧O | ⌘N | 已对齐 |
| 打开文件夹 | ⌘O | 加号/侧栏添加项目 | 可做绑定到现有 `project_add` |
| 切换侧栏 | ⌘B | 顶栏按钮，无 ⌘B | 可做 |
| 切换 diff | ⌘⌥B | 顶栏资源按钮 | 可做绑定到现有面板 |
| 终端 | ⌘J / 较新文档还有 ⌃` | 无 | 缺后端 / 明确不做本阶段 |
| 上/下一线程 | ⌘⇧[ / ⌘⇧] | 无 | 可做，数据已有 |
| 线程内查找 | ⌘F | 无 | 可做 |
| 停止 | Esc（生成） | Esc 停生成/关浮层 | 已对齐 |
| 发送 | 可要求 ⌘Enter | ⌘Enter | 已对齐 |
| 字号 | ⌘+ / ⌘- / ⌘0 | 无 | 可做外观设置，非阻塞 |
| 语音 | ⌃M | 无 | 缺后端 |
| 侧边聊天 | ⌘⌥S | 无 | 明确不做 |
| Doctor | — | ⌘⇧D | Sunsetz 自有，保留 |

### 1.7 设置

参考图 13–32 已规定：栏目必须有真实后端。Sunsetz 现注册表：常规、外观、账户、已归档、扩展、Runtime、关于。

| Codex 设置 | Sunsetz | 状态 |
|------------|---------|------|
| General / Appearance / Profile / Shortcuts | 常规、外观、账户；快捷键是只读帮助 | 可做：可配置快捷键 **缺后端** |
| Pets / overlay | 禁止占位 | 明确不做 |
| 智能快照、浏览器、电脑控制 | 能力 unavailable | 缺后端 |
| Hooks / 插件市场安装 | 只读目录；安装卸载 fail-closed | 缺后端 |
| Git / 环境 / Worktree 高级写配置 | Changes + composer worktree 只读 | 缺后端 |
| PR 工作流页 | 无 | 缺后端（阶段 11） |

---

## 2. 无新截图时的 Codex 信息层级（重建）

没有本回合真机截图。下面把「打开 Codex 后眼睛会扫到的顺序」写成层级，依据官方功能页 + 图 1–32 审计。有真机截图后再替换本节，不改第 1 节口径。

### 2.1 第一眼（空闲工作台）

1. **左**：模块入口（新建/线程、Skills、Automations）→ 项目树 → 当前项目下的线程。选中是整行中性底，不是强调色竖条。
2. **中**：垂直中轴是对话；空任务时引导在视觉中心，输入器贴底。
3. **右**：Git 项目才稳定出现文件树 / Review；非 Git 时右栏弱化或提示初始化仓库。
4. **顶**：项目与线程身份、模式、模型、打开 diff/终端。比 Sunsetz 顶栏信息多，也更吵。
5. **底**：无终端时只有窗口/状态；打开后终端是当前线程 cwd 的抽屉，不是全局 IDE 终端。

Sunsetz 已经学走了「中轴对话 + 底栏输入 + 左栏去哪」。没有学走顶栏工具堆和底栏终端。应保持。

### 2.2 运行中（局内）

Codex 把「Agent 正在干什么」拆到两处：

- **中栏时间线**：工具、命令、简短进度。
- **右栏任务侧栏**：plan、sources、生成物、摘要。用户用右边 *看结果、改方向*，用中间 *说话*。

Sunsetz 把进度收进 `TaskProgressRail`，计划正文进卡片和 Plan tab，提问/计划决定收进底部 dock。这是有意的：参考图 9 已经证明「底部确认 + 右侧再批一次」会造成重复。对齐 Codex 右栏时，只搬 **只读结果**，不搬决策。

### 2.3 Review pane（Git 项目）

官方行为：

- 显示整个仓库 Git 状态，不限于 Agent 刚改的文件。
- 默认 Uncommitted；可切 All branch changes、Last turn changes。
- 本地还可切 Unstaged / Staged。
- 点文件名：打开外部编辑器；点文件名背景：展开/折叠 diff。
- 可对 chunk/文件 stage 或 revert；可 commit / push / 开 PR。
- `/review` 的评论出现在 diff 行内。
- 超大 diff 会退化成「一次一个文件」。

Sunsetz Changes 已有 Session vs Workspace 两堆，但没有 Codex 那三个 scope，也没有写 Git。要对齐的是 **看清改了什么**，不是把 GitHub 搬进应用。

### 2.4 参考图 1–32 里已经校准、且现在仍有效的密度

这些来自当时截图，不依赖新图：

- 输入器右侧用量是紧凑实线环，悬停三行摘要，点击才打开详情。
- 任务预览短、不抢焦点。
- Changes / 文件 / 正文可并列。
- 计划标签在访问权限右侧，不是顶上第二条计划带。
- 计划待审阅全页只出现一次。
- 顶部三点菜单按按钮锚定、分组、无未实现项。
- 账户菜单以真实额度开头。
- 设置是窄导航 + 搜索到具体配置项。
- 插件、语音、电脑控制、Hooks、PR、市场安装：没有后端就不进导航。

### 2.5 和 Codex 当前文档相比、参考图没覆盖到的新层

这些是 2026 文档里有、仓库参考图没单独编号的：

- 左栏 Skills 选择器（官方有 skill-selector 图）。
- 中栏下方集成终端抽屉。
- 多项目并行线程（multitask 图）。
- 线程自动化 / 唤醒同一会话。
- 侧边线程、独立聊天、Quick Chat。
- 可自定义快捷键、宠物、桌面 overlay。

对 Sunsetz：Skills 选择器可以做成只读库存页；终端、侧边线程、宠物、overlay 不进入当前切片。

---

## 3. 建议落地顺序（仍不改代码）

只在 Host 已能证明的前提下搬界面。前三项不破坏 AskUserDock 唯一计划入口。

1. **右栏只读结果舱**：Plan tab 接 `PlanArtifactV1` 的 approved / executing / done；Changes 增加 last-turn / workspace scope 文案，仍禁止 discard 全部。
2. **左栏密度**：线程状态过滤（运行中、待回答、最近）。项目 Git 摘要与移动到项目级联已有代码，差原生验收。
3. **快捷键补齐已有动作**：⌘B 侧栏、⌘⌥B 资源面板、⌘O 添加项目、⌘⇧[ / ] 切任务。不改 ⌘K。
4. **会话内查找 ⌘F**：只搜当前 journal 可见文本。
5. **Skills 只读浏览页**：复用库存，不开放安装。
6. **明确不做**：内置终端、侧边线程、Review 行内评论闭环、应用内 commit/PR、语音、电脑控制、宠物。

有 Codex 真机截图后，用同一编号补「像素级密度」一节，替换第 2 节中「重建」字样，不重写产品边界。
