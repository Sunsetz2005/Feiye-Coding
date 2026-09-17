import { createRoot } from "react-dom/client";
import { ContextMenu } from "@/components/ContextMenu";
import {
  IconArchive,
  IconCopy,
  IconFork,
  IconRename,
  IconRewind,
  IconTrash,
} from "@/components/icons";
import { SidebarNavigator } from "@/components/SidebarNavigator";
import { WorkbenchTopbar } from "@/components/WorkbenchTopbar";
import type { AccountStatus, SessionPreviewV1 } from "@/lib/api";
import "@/styles/tokens.css";
import "@/styles/tailwind.css";
import "@/styles/app.css";
import "@/styles/apple.css";
import "@/styles/workbench.css";

const noop = () => {};
const fixtureQuery = new URLSearchParams(window.location.search);
const showAccount = fixtureQuery.has("account");
const showSessionMenu = fixtureQuery.has("session-menu");

const account: AccountStatus = {
  profile: {
    signedIn: true,
    authMode: "oauth",
    email: "leaf@example.com",
    displayName: "飞叶",
    userId: "visual-user",
    teamId: null,
    principalType: null,
    expiresAt: null,
    expired: false,
    hasRefresh: true,
    oidcIssuer: null,
  },
  hasOfficialKey: false,
  hasRelayKey: false,
  relayBaseUrl: null,
  cliAuthPresent: true,
  cliFound: true,
  cliPath: "/usr/local/bin/sunsetz",
  channel: "official_oauth",
  billing: {
    available: true,
    source: "visual",
    message: null,
    subscriptionTier: "Sunsetz Pro",
    creditUsagePercent: 30,
    remainingPercent: 70,
    monthlyLimit: null,
    includedUsed: null,
    totalUsed: null,
    prepaidBalance: null,
    onDemandEnabled: null,
    onDemandCap: null,
    onDemandUsed: null,
    billingPeriodStart: null,
    billingPeriodEnd: null,
    resetsAt: "2026-08-02T00:00:00Z",
    isUnifiedBillingUser: true,
    products: [],
    manageUrl: "",
    subscribeUrl: "",
    fetchedAt: null,
  },
  heatmap: [],
  callLogs: [],
  usageManageUrl: "",
  subscribeUrl: "",
};

const sessionPreview: SessionPreviewV1 = {
  version: 1,
  sessionId: "session-1",
  projectId: "project-1",
  title: "优化工作台侧栏预览",
  updatedAt: "2026-07-27T08:30:00Z",
  modelId: "sunsetz-4.5",
  contextUsage: null,
  archived: false,
  scheduled: false,
  recentUserSummary:
    "把任务摘要放在侧栏右侧，悬停后再加载，不要遮住当前任务。",
  recentAssistantSummary:
    "预览只展示经过脱敏和长度限制的最近可见消息。",
};

createRoot(document.getElementById("root")!).render(
  <div className="app-shell">
    <div className="workbench">
      <SidebarNavigator
        collapsed={false}
        dragZone={null}
        labels={{
          chrome: { hide: "隐藏侧栏" },
          drag: {
            addProjectTitle: "添加项目",
            addProjectHint: "拖放文件夹",
          },
          navigation: {
            label: "工作台",
            newSession: "新任务",
            search: "搜索",
            scheduled: "已安排",
            plugins: "扩展",
          },
          tree: {
            projects: "项目",
            addProject: "添加项目",
            noProjects: "暂无项目",
            collapseProject: "折叠项目",
            expandProject: "展开项目",
            untrusted: "未信任",
            menu: "更多",
            organize: "整理侧边栏",
            groupByProject: "按项目",
            groupByList: "在一个列表中",
            chatSort: "聊天排序方式",
            sortPriority: "优先级",
            sortRecent: "最近更新",
            newConversation: "新建会话",
            editProject: "编辑项目",
            collapseProjects: "折叠项目栏",
            expandProjects: "展开项目栏",
            trustProject: "信任项目",
            noChats: "暂无任务",
            otherSessions: "其他任务",
            untitled: "未命名",
            scheduledTag: "已安排任务",
            answerNeeded: "需要回答",
            sessionWorking: "运行中",
            previewTasks: "{count} 个任务",
            previewActive: "{count} 个活动任务",
            previewUpdated: "更新于 {time}",
            previewPinned: "已固定",
            previewNoSummary: "暂无可见消息",
            previewGitRef: "Git · {ref}",
            previewGitAhead: "领先 {count}",
            previewGitBehind: "落后 {count}",
            previewGitDirty: "{count} 项变更",
            previewGitConflicts: "{count} 项冲突",
            previewGitCountsCapped: "计数已封顶",
            unarchive: "取消归档",
            archive: "归档",
          },
          account: {
            trigger: "账户",
            settings: "设置",
            theme: "主题",
            themeLight: "浅色",
            themeDark: "深色",
            local: "本地",
            signedIn: "已登录",
            signedOut: "未登录",
            login: "登录",
            logout: "退出",
            remaining: "剩余",
            usage: "剩余用量",
            customProvider: "提供方",
            resetsAt: "重置时间",
          },
        }}
        chrome={{
          useCustomWindowChrome: false,
          onHide: noop,
          onToggleMaximize: noop,
        }}
        navigation={{
          activePane: "chat",
          onNewSession: noop,
          onSearch: noop,
          onOpenAutomations: noop,
          onOpenExtensions: noop,
        }}
        tree={{
          projectsOpen: true,
          historyOpen: true,
          activeProjectId: "project-1",
          activeSessionId: "session-1",
          busySessionIds: new Set(),
          pendingAskSessionIds: new Set(),
          projects: [
            {
              id: "project-1",
              name: "Sunsetz",
              path: "/Users/Shared/Coding/Sunsetz",
              trusted: true,
              pinned: true,
              open: true,
              sessions: [
                {
                  id: "session-1",
                  title: "优化工作台侧栏预览",
                  updatedAt: "2026-07-27T08:30:00Z",
                  archived: false,
                  scheduled: false,
                },
                {
                  id: "session-2",
                  title: "输入器上下文控件",
                  updatedAt: "2026-07-26T12:00:00Z",
                  archived: false,
                  scheduled: false,
                },
              ],
            },
          ],
          orphanSessions: [],
          groupBy: "project",
          sessionSort: "recent",
          onToggleProjects: noop,
          onAddProject: noop,
          onToggleProject: noop,
          onSelectProject: noop,
          onNewSessionInProject: noop,
          onEditProject: noop,
          onOrganize: noop,
          onTrustProject: noop,
          onProjectMenu: noop,
          onToggleHistory: noop,
          onOpenSession: noop,
          onArchiveSession: noop,
          onSessionMenu: noop,
          loadSessionPreview: async () => sessionPreview,
        }}
        account={{
          open: showAccount,
          theme: "dark",
          account: showAccount ? account : null,
          activeProvider: null,
          busy: false,
          customRouteActive: false,
          onClose: noop,
          onToggle: noop,
          onSettings: noop,
          onAccountSettings: noop,
          onToggleTheme: noop,
          onLogin: noop,
          onLogout: noop,
        }}
      />
      <main className="main" aria-label="预览验收画布">
        {showSessionMenu ? (
          <>
            <WorkbenchTopbar
              title="优化工作台菜单"
              projectContext
              scheduledLabel="已安排"
              sessionMenuLabel="任务菜单"
              sessionMenuOpen
              onOpenSessionMenu={noop}
              sidebarCollapsed={false}
              showSidebarLabel="显示侧栏"
              onShowSidebar={noop}
              asideCollapsed
              showAsideLabel="显示资源"
              hideAsideLabel="隐藏资源"
              onToggleAside={noop}
            />
            <ContextMenu
              open
              x={0}
              y={0}
              anchorRect={{
                left: 389,
                right: 421,
                top: 8,
                bottom: 42,
                width: 32,
                height: 34,
              }}
              estimatedWidth={236}
              estimatedHeight={330}
              className="context-menu--session"
              onClose={noop}
              items={[
                {
                  id: "rename",
                  label: "重命名任务",
                  icon: <IconRename size={16} />,
                  onClick: noop,
                },
                {
                  id: "export",
                  label: "导出 Markdown",
                  icon: <IconCopy size={16} />,
                  separatorBefore: true,
                  onClick: noop,
                },
                {
                  id: "diagnostic",
                  label: "导出诊断包…",
                  icon: <IconCopy size={16} />,
                  onClick: noop,
                },
                {
                  id: "fork",
                  label: "分叉任务",
                  icon: <IconFork size={16} />,
                  separatorBefore: true,
                  onClick: noop,
                },
                {
                  id: "rewind",
                  label: "回退时间线",
                  icon: <IconRewind size={16} />,
                  onClick: noop,
                },
                {
                  id: "copy",
                  label: "复制任务 ID",
                  icon: <IconCopy size={16} />,
                  onClick: noop,
                },
                {
                  id: "archive",
                  label: "归档任务",
                  icon: <IconArchive size={16} />,
                  separatorBefore: true,
                  onClick: noop,
                },
                {
                  id: "delete",
                  label: "删除任务",
                  icon: <IconTrash size={16} />,
                  danger: true,
                  onClick: noop,
                },
              ]}
            />
          </>
        ) : null}
      </main>
    </div>
  </div>,
);
