import { describe, expect, it } from "vitest";
import {
  currentPlanRevision,
  planArtifactProductStatus,
  selectDisplayPlanArtifact,
  type PlanArtifactV1,
} from "./planArtifacts";

function artifact(
  partial: Partial<PlanArtifactV1> & Pick<PlanArtifactV1, "id" | "status">,
): PlanArtifactV1 {
  return {
    version: 1,
    sessionId: "s1",
    processId: "p1",
    currentRevision: 1,
    revisions: [
      {
        revision: 1,
        contentHash: "a".repeat(64),
        body: `${partial.id} body`,
        entries: [{ content: partial.id, status: "pending" }],
        createdAt: "2026-01-01T00:00:00Z",
      },
    ],
    transitions: [
      {
        status: partial.status,
        occurredAt: "2026-01-01T00:00:00Z",
      },
    ],
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
    ...partial,
  };
}

describe("selectDisplayPlanArtifact", () => {
  it("prefers live proposed over older completed", () => {
    const completed = artifact({
      id: "old",
      status: "completed",
      updatedAt: "2026-01-01T00:00:00Z",
    });
    const proposed = artifact({
      id: "live",
      status: "proposed",
      updatedAt: "2026-01-01T00:00:01Z",
    });
    expect(selectDisplayPlanArtifact([completed, proposed])?.id).toBe("live");
  });

  it("skips abandoned and falls back to the latest remaining artifact", () => {
    const completed = artifact({
      id: "done",
      status: "completed",
      updatedAt: "2026-01-01T00:00:00Z",
    });
    const abandoned = artifact({
      id: "drop",
      status: "abandoned",
      updatedAt: "2026-01-01T00:00:02Z",
    });
    expect(selectDisplayPlanArtifact([abandoned, completed])?.id).toBe("done");
    expect(selectDisplayPlanArtifact([abandoned])).toBeNull();
  });
});

describe("plan artifact helpers", () => {
  it("reads the current revision and maps completed to product done", () => {
    const row = artifact({
      id: "plan_1",
      status: "completed",
      currentRevision: 2,
      revisions: [
        {
          revision: 1,
          contentHash: "b".repeat(64),
          body: "old",
          entries: [],
          createdAt: "2026-01-01T00:00:00Z",
        },
        {
          revision: 2,
          contentHash: "c".repeat(64),
          body: "new",
          entries: [{ content: "Ship", status: "completed" }],
          createdAt: "2026-01-01T00:00:01Z",
        },
      ],
    });
    expect(currentPlanRevision(row)?.body).toBe("new");
    expect(planArtifactProductStatus("completed")).toBe("done");
    expect(planArtifactProductStatus("approved")).toBe("approved");
    expect(planArtifactProductStatus("executing")).toBe("executing");
    expect(planArtifactProductStatus("proposed")).toBeNull();
  });
});
