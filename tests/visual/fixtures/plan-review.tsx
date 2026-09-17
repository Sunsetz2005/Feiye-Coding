import { createRoot } from "react-dom/client";
import { AskUserDock } from "@/components/lobe-chat/AskUserDock";
import { PlanArtifactCard } from "@/components/lobe-chat/PlanArtifactCard";
import { PlanReviewPanel } from "@/components/PlanReviewPanel";
import "@/styles/tokens.css";
import "@/styles/tailwind.css";
import "@/styles/app.css";
import "@/styles/apple.css";
import "@/styles/workbench.css";
import "./plan-review.css";

const noop = () => {};
const plan = {
  visible: true,
  waiting: false,
  title: "工作台交互优化计划",
  body:
    "## 实施范围\n\n统一计划确认入口，并让资源面板只负责阅读计划正文和步骤。",
  entries: [
    { content: "核对现有计划状态", status: "completed" },
    { content: "移除右栏决策按钮", status: "in_progress" },
    { content: "验证唯一确认入口", status: "pending" },
  ],
  rpcId: 42,
  artifactStatus: "proposed" as const,
  liveReview: true,
};

createRoot(document.getElementById("root")!).render(
  <div className="app-shell">
    <div className="plan-fixture">
      <main className="plan-fixture__main">
        <header className="plan-fixture__topbar">
          <span>工作台交互优化</span>
        </header>
        <div className="plan-fixture__conversation">
          <PlanArtifactCard
            artifact={plan}
            labels={{
              plan: "计划",
              empty: "暂无计划内容",
              open: "查看详情",
              approved: "已批准",
              executing: "执行中",
              done: "已完成",
            }}
            onOpen={noop}
          />
        </div>
        <div className="plan-fixture__dock">
          <AskUserDock
            payload={{
              rpcId: 42,
              sessionId: "session-plan",
              toolCallId: "tool-plan",
              questions: [
                {
                  id: "plan-approval",
                  question: "确认后开始实施，或填写需要修改的内容。",
                  multiSelect: false,
                  options: [
                    {
                      id: "approve",
                      label: "批准并构建",
                    },
                  ],
                },
              ],
            }}
            labels={{
              title: "计划待审阅",
              submit: "提交",
              cancel: "放弃计划",
              otherPlaceholder: "说明需要修改的内容",
              freeTextHint: "或请求修改",
              multiHint: "",
              close: "放弃计划",
              previous: "上一题",
              next: "下一题",
              skip: "放弃计划",
              progress: "{current} / {total}",
              recommended: "推荐",
              submitFailed: "无法提交，请重试",
            }}
            onSubmit={noop}
            onCancel={noop}
          />
        </div>
      </main>
      <aside className="plan-fixture__aside" aria-label="资源面板">
        <PlanReviewPanel
          plan={plan}
          labels={{
            plan: "计划",
            waiting: "等待计划",
            progress: "执行中",
            done: "已完成",
            empty: "暂无计划内容",
            steps: "任务步骤",
            fraction: "{n}",
            expandDetails: "展开详情",
            collapseDetails: "收起详情",
            current: "当前",
          }}
        />
      </aside>
    </div>
  </div>,
);
