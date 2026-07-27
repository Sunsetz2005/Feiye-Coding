import { createRoot } from "react-dom/client";
import { SidebarNavigator } from "@/components/SidebarNavigator";
import type { SessionPreviewV1 } from "@/lib/api";
import "@/styles/tokens.css";
import "@/styles/tailwind.css";
import "@/styles/app.css";
import "@/styles/apple.css";
import "@/styles/workbench.css";

const noop = () => {};

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
          busySessionId: null,
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
          onToggleProjects: noop,
          onAddProject: noop,
          onToggleProject: noop,
          onSelectProject: noop,
          onTrustProject: noop,
          onProjectMenu: noop,
          onToggleHistory: noop,
          onOpenSession: noop,
          onArchiveSession: noop,
          onSessionMenu: noop,
          loadSessionPreview: async () => sessionPreview,
        }}
        account={{
          open: false,
          theme: "dark",
          account: null,
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
      <main className="main" aria-label="预览验收画布" />
    </div>
  </div>,
);
