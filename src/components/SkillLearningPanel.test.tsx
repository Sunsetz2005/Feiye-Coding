// @vitest-environment jsdom

import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  SkillImprovementProposalV1,
  SkillMetadataRankingResultV1,
  SkillUseRecordV1,
} from "@/lib/api";

const mocks = vi.hoisted(() => ({
  isTauri: vi.fn(() => true),
  list: vi.fn(),
  feedback: vi.fn(),
  proposal: vi.fn(),
  rank: vi.fn(),
}));

vi.mock("@/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api")>();
  return {
    ...actual,
    isTauri: mocks.isTauri,
    skillUsesListV1: mocks.list,
    skillUseFeedbackV1: mocks.feedback,
    skillImprovementProposalV1: mocks.proposal,
    skillMetadataRankV1: mocks.rank,
  };
});

function skillUse(
  index: number,
  overrides: Partial<SkillUseRecordV1> = {},
): SkillUseRecordV1 {
  const base: SkillUseRecordV1 = {
    version: 1,
    id: `use-${index}`,
    revision: 3,
    selection: "explicit",
    sessionId: "session-a",
    turnId: `turn-${index}`,
    skill: {
      id: `skill-${index}`,
      name: `Skill ${index}`,
      treeHash: `${index}`.padStart(64, "0"),
      sourceCandidateId: null,
    },
    status: "succeeded",
    statusEvents: [
      { status: "succeeded", occurredAt: "2026-08-24T08:00:00.000Z" },
    ],
    feedback: null,
    createdAt: "2026-08-24T07:00:00.000Z",
    updatedAt: "2026-08-24T08:00:00.000Z",
  };
  return {
    ...base,
    ...overrides,
    skill: overrides.skill ?? base.skill,
  };
}

function safeProposal(skillName = "Host Skill"): SkillImprovementProposalV1 {
  return {
    version: 1,
    id: "proposal-1",
    status: "pending_review",
    owner: {
      kind: "host_candidate",
      namespace: "sunsetz",
      mayOverwriteExternal: false,
    },
    skillId: "host-skill",
    skillName,
    lineage: {
      priorSkillTreeHash: "a".repeat(64),
      sourceCandidateId: "candidate-1",
      evidenceUseIds: ["use-1", "use-2"],
    },
    successfulUseCount: 2,
    helpfulUseCount: 1,
    repeatedEvidence: true,
    requiresUserReview: true,
    mayWriteSkill: false,
  };
}

function suggestionResult(name = "Suggested Skill"): SkillMetadataRankingResultV1 {
  return {
    version: 1,
    disposition: "suggestion_only",
    requiresExplicitAcceptance: true,
    items: [
      {
        skill: {
          id: "suggested-skill",
          name,
          treeHash: "b".repeat(64),
          sourceCandidateId: null,
        },
        score: 42,
        matchedTerms: ["release", "notes"],
      },
    ],
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

beforeEach(() => {
  mocks.isTauri.mockReturnValue(true);
  mocks.list.mockResolvedValue([]);
  mocks.feedback.mockImplementation(
    async (record: SkillUseRecordV1, rating: "helpful" | "unhelpful") => ({
      ...record,
      revision: record.revision + 1,
      feedback: { rating, occurredAt: "2026-08-24T09:00:00.000Z" },
      updatedAt: "2026-08-24T09:00:00.000Z",
    }),
  );
  mocks.proposal.mockResolvedValue(safeProposal());
  mocks.rank.mockResolvedValue(suggestionResult());
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("SkillLearningPanel", () => {
  it("does not query the use ledger without an active session", async () => {
    const { SkillLearningPanel } = await import("./SkillLearningPanel");
    render(<SkillLearningPanel locale="en" activeSessionId={null} />);

    expect(screen.getByText("No active session is available for review.")).toBeTruthy();
    expect(mocks.list).not.toHaveBeenCalled();
  });

  it("shows at most 64 metadata-only records and treats hostile names as text", async () => {
    const hostileName = '<img src=x onerror="prompt-payload-secret">';
    const records = Array.from({ length: 65 }, (_, index) =>
      skillUse(index, {
        skill: {
          id: `skill-${index}`,
          name: index === 0 ? hostileName : `Skill ${index}`,
          treeHash: `${index}`.padStart(64, "0"),
          sourceCandidateId: null,
        },
        ...(index === 0
          ? ({
              prompt: "prompt-payload-secret",
              toolPayload: "tool-payload-secret",
            } as unknown as Partial<SkillUseRecordV1>)
          : {}),
      }),
    );
    mocks.list.mockResolvedValue(records);
    const { SkillLearningPanel } = await import("./SkillLearningPanel");
    render(<SkillLearningPanel locale="en" activeSessionId="session-a" />);

    const uses = await screen.findByRole("article", {
      name: "Current session Skill uses",
    });
    await waitFor(() => expect(within(uses).getAllByRole("listitem")).toHaveLength(64));
    expect(within(uses).getByText(hostileName)).toBeTruthy();
    expect(uses.querySelector("img")).toBeNull();
    expect(within(uses).queryByText("Skill 64")).toBeNull();
    expect(screen.queryByText("prompt-payload-secret")).toBeNull();
    expect(screen.queryByText("tool-payload-secret")).toBeNull();
  });

  it("records terminal feedback once with the full CAS record", async () => {
    const succeeded = skillUse(1, {
      skill: {
        id: "host-skill",
        name: "Successful Skill",
        treeHash: "a".repeat(64),
        sourceCandidateId: "candidate-1",
      },
    });
    const failed = skillUse(2, {
      skill: {
        id: "failed-skill",
        name: "Failed Skill",
        treeHash: "c".repeat(64),
        sourceCandidateId: null,
      },
      status: "failed",
      statusEvents: [
        { status: "failed", occurredAt: "2026-08-24T08:00:00.000Z" },
      ],
    });
    let records = [succeeded, failed];
    mocks.list.mockImplementation(async () => records);
    mocks.feedback.mockImplementation(
      async (record: SkillUseRecordV1, rating: "helpful" | "unhelpful") => {
        const updated = {
          ...record,
          revision: record.revision + 1,
          feedback: {
            rating,
            occurredAt: "2026-08-24T09:00:00.000Z",
          },
        } satisfies SkillUseRecordV1;
        records = records.map((item) => (item.id === record.id ? updated : item));
        return updated;
      },
    );

    const { SkillLearningPanel } = await import("./SkillLearningPanel");
    render(<SkillLearningPanel locale="en" activeSessionId="session-a" />);

    const successfulItem = (await screen.findByText("Successful Skill")).closest("li");
    if (!successfulItem) throw new Error("missing successful use item");
    fireEvent.click(
      within(successfulItem).getByRole("button", { name: "Helpful" }),
    );

    await waitFor(() =>
      expect(mocks.feedback).toHaveBeenCalledWith(succeeded, "helpful"),
    );
    await waitFor(() =>
      expect(
        within(successfulItem).queryByRole("button", { name: "Helpful" }),
      ).toBeNull(),
    );
    expect(within(successfulItem).getByText(/Feedback:/).textContent).toContain(
      "Helpful",
    );

    const failedItem = screen.getByText("Failed Skill").closest("li");
    if (!failedItem) throw new Error("missing failed use item");
    expect(
      within(failedItem).queryByRole("button", { name: "Helpful" }),
    ).toBeNull();
    expect(
      within(failedItem).getByRole("button", { name: "Unhelpful" }),
    ).toBeTruthy();
  });

  it("surfaces stale CAS feedback and reloads the authoritative use", async () => {
    const record = skillUse(1);
    mocks.list.mockResolvedValue([record]);
    mocks.feedback.mockRejectedValue(new Error("STALE_SKILL_USE_REVISION"));
    const { SkillLearningPanel } = await import("./SkillLearningPanel");
    render(<SkillLearningPanel locale="en" activeSessionId="session-a" />);

    fireEvent.click(
      await screen.findByRole("button", { name: "Helpful" }),
    );

    expect(
      await screen.findByText(
        "Host Skill review error: STALE_SKILL_USE_REVISION",
      ),
    ).toBeTruthy();
    expect(mocks.list).toHaveBeenCalledTimes(2);
    expect(screen.getByRole("button", { name: "Helpful" })).toBeTruthy();
  });

  it("builds only a review-only proposal for succeeded Host-owned evidence", async () => {
    const hostUse = skillUse(1, {
      skill: {
        id: "host-skill",
        name: "Host Skill",
        treeHash: "a".repeat(64),
        sourceCandidateId: "candidate-1",
      },
    });
    const externalUse = skillUse(2, {
      skill: {
        id: "external-skill",
        name: "External Skill",
        treeHash: "d".repeat(64),
        sourceCandidateId: null,
      },
    });
    mocks.list.mockResolvedValue([hostUse, externalUse]);
    const proposal = safeProposal();
    mocks.proposal.mockResolvedValue(proposal);

    const { SkillLearningPanel } = await import("./SkillLearningPanel");
    render(
      <SkillLearningPanel
        locale="en"
        activeSessionId="session-a"
        activeProjectPath="/trusted/project"
      />,
    );

    await screen.findByText("Host Skill");
    expect(
      screen.getAllByRole("button", {
        name: "Generate pending-review proposal",
      }),
    ).toHaveLength(1);
    fireEvent.click(
      screen.getByRole("button", {
        name: "Generate pending-review proposal",
      }),
    );

    await waitFor(() =>
      expect(mocks.proposal).toHaveBeenCalledWith(
        hostUse.skill,
        "/trusted/project",
      ),
    );
    const proposalPanel = await screen.findByRole("article", {
      name: "Pending Skill improvement proposal",
    });
    const valueFor = (label: string) =>
      within(proposalPanel).getByText(label).parentElement?.querySelector("dd")
        ?.textContent;
    expect(valueFor("requiresUserReview")).toBe("true");
    expect(valueFor("mayWriteSkill")).toBe("false");
    expect(valueFor("mayOverwriteExternal")).toBe("false");
    expect(
      within(proposalPanel).getByText(
        "Review DTO only. This action does not create, edit, or overwrite any Skill.",
      ),
    ).toBeTruthy();
    expect(
      within(proposalPanel).queryByRole("button", {
        name: /save|write|overwrite|install/i,
      }),
    ).toBeNull();
  });

  it("shows an explicit no-proposal result when the evidence threshold is not met", async () => {
    const hostUse = skillUse(1, {
      skill: {
        id: "host-skill",
        name: "Host Skill",
        treeHash: "a".repeat(64),
        sourceCandidateId: "candidate-1",
      },
    });
    mocks.list.mockResolvedValue([hostUse]);
    mocks.proposal.mockResolvedValue(null);
    const { SkillLearningPanel } = await import("./SkillLearningPanel");
    render(<SkillLearningPanel locale="en" activeSessionId="session-a" />);

    fireEvent.click(
      await screen.findByRole("button", {
        name: "Generate pending-review proposal",
      }),
    );

    expect(
      await screen.findByText(
        "The evidence threshold is not met, or unhelpful evidence prevents a proposal.",
      ),
    ).toBeTruthy();
  });

  it("runs ranking only after an explicit query and never offers invocation", async () => {
    const hostileSuggestion = "<script>invokeSkill()</script>";
    mocks.rank.mockResolvedValue(suggestionResult(hostileSuggestion));
    const { SkillLearningPanel } = await import("./SkillLearningPanel");
    render(
      <SkillLearningPanel
        locale="en"
        activeSessionId="session-a"
        activeProjectPath="/trusted/project"
      />,
    );

    expect(mocks.rank).not.toHaveBeenCalled();
    fireEvent.change(screen.getByLabelText("Skill metadata query"), {
      target: { value: "release notes" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Find suggestions" }));

    await waitFor(() =>
      expect(mocks.rank).toHaveBeenCalledWith(
        "release notes",
        "/trusted/project",
        8,
      ),
    );
    const result = await screen.findByRole("region", {
      name: "Skill metadata suggestions",
    });
    expect(within(result).getByText("Suggestion only")).toBeTruthy();
    expect(within(result).getByText(hostileSuggestion)).toBeTruthy();
    expect(result.querySelector("script")).toBeNull();
    expect(
      within(result).queryByRole("button", { name: /use|insert|invoke/i }),
    ).toBeNull();
  });

  it("rejects Host results that break review-only or suggestion-only boundaries", async () => {
    const hostUse = skillUse(1, {
      skill: {
        id: "host-skill",
        name: "Host Skill",
        treeHash: "a".repeat(64),
        sourceCandidateId: "candidate-1",
      },
    });
    mocks.list.mockResolvedValue([hostUse]);
    mocks.rank.mockResolvedValue({
      ...suggestionResult(),
      requiresExplicitAcceptance: false,
    } as unknown as SkillMetadataRankingResultV1);
    mocks.proposal.mockResolvedValue({
      ...safeProposal(),
      mayWriteSkill: true,
    } as unknown as SkillImprovementProposalV1);
    const { SkillLearningPanel } = await import("./SkillLearningPanel");
    render(<SkillLearningPanel locale="en" activeSessionId="session-a" />);

    fireEvent.change(screen.getByLabelText("Skill metadata query"), {
      target: { value: "unsafe suggestion" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Find suggestions" }));
    expect(
      await screen.findByText(
        "The Host result did not preserve the suggestion-only boundary and was rejected.",
      ),
    ).toBeTruthy();

    fireEvent.click(
      screen.getByRole("button", {
        name: "Generate pending-review proposal",
      }),
    );
    expect(
      await screen.findByText(
        "The Host proposal could write or overwrite a Skill and was rejected.",
      ),
    ).toBeTruthy();
    expect(
      screen.queryByRole("article", {
        name: "Pending Skill improvement proposal",
      }),
    ).toBeNull();
  });

  it("bounds explicit ranking queries to the Host UTF-8 limit", async () => {
    const { SkillLearningPanel } = await import("./SkillLearningPanel");
    render(<SkillLearningPanel locale="en" />);
    const input = screen.getByLabelText("Skill metadata query") as HTMLInputElement;

    fireEvent.change(input, { target: { value: "界".repeat(1_000) } });

    expect(new TextEncoder().encode(input.value).length).toBeLessThanOrEqual(2_048);
    expect(
      screen.getByText("Query was limited to 2048 UTF-8 bytes."),
    ).toBeTruthy();
  });

  it("discards an old use-list response after the session and project change", async () => {
    const first = deferred<SkillUseRecordV1[]>();
    const second = deferred<SkillUseRecordV1[]>();
    mocks.list.mockImplementation((sessionId: string) =>
      sessionId === "session-a" ? first.promise : second.promise,
    );
    const { SkillLearningPanel } = await import("./SkillLearningPanel");
    const { rerender } = render(
      <SkillLearningPanel
        locale="en"
        activeSessionId="session-a"
        activeProjectPath="/project-a"
      />,
    );
    await waitFor(() => expect(mocks.list).toHaveBeenCalledWith("session-a"));

    rerender(
      <SkillLearningPanel
        locale="en"
        activeSessionId="session-b"
        activeProjectPath="/project-b"
      />,
    );
    await waitFor(() => expect(mocks.list).toHaveBeenCalledWith("session-b"));
    await act(async () => {
      second.resolve([
        skillUse(2, {
          sessionId: "session-b",
          skill: {
            id: "current-skill",
            name: "Current Skill",
            treeHash: "e".repeat(64),
            sourceCandidateId: null,
          },
        }),
      ]);
    });
    expect(await screen.findByText("Current Skill")).toBeTruthy();

    await act(async () => {
      first.resolve([
        skillUse(1, {
          skill: {
            id: "stale-skill",
            name: "Stale Skill",
            treeHash: "f".repeat(64),
            sourceCandidateId: null,
          },
        }),
      ]);
    });
    expect(screen.queryByText("Stale Skill")).toBeNull();
    expect(screen.getByText("Current Skill")).toBeTruthy();
  });

  it("discards a pending metadata ranking when its project scope changes", async () => {
    const pending = deferred<SkillMetadataRankingResultV1>();
    mocks.rank.mockReturnValue(pending.promise);
    const { SkillLearningPanel } = await import("./SkillLearningPanel");
    const { rerender } = render(
      <SkillLearningPanel
        locale="en"
        activeSessionId="session-a"
        activeProjectPath="/project-a"
      />,
    );
    fireEvent.change(screen.getByLabelText("Skill metadata query"), {
      target: { value: "old project" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Find suggestions" }));
    await waitFor(() => expect(mocks.rank).toHaveBeenCalledTimes(1));

    rerender(
      <SkillLearningPanel
        locale="en"
        activeSessionId="session-a"
        activeProjectPath="/project-b"
      />,
    );
    await act(async () => pending.resolve(suggestionResult("Stale Suggestion")));

    expect(screen.queryByText("Stale Suggestion")).toBeNull();
    expect(
      screen.queryByRole("region", { name: "Skill metadata suggestions" }),
    ).toBeNull();
  });

  it("discards a pending proposal when the project scope changes", async () => {
    const hostUse = skillUse(1, {
      skill: {
        id: "host-skill",
        name: "Host Skill",
        treeHash: "a".repeat(64),
        sourceCandidateId: "candidate-1",
      },
    });
    mocks.list.mockResolvedValue([hostUse]);
    const pending = deferred<SkillImprovementProposalV1 | null>();
    mocks.proposal.mockReturnValue(pending.promise);
    const { SkillLearningPanel } = await import("./SkillLearningPanel");
    const { rerender } = render(
      <SkillLearningPanel
        locale="en"
        activeSessionId="session-a"
        activeProjectPath="/project-a"
      />,
    );
    fireEvent.click(
      await screen.findByRole("button", {
        name: "Generate pending-review proposal",
      }),
    );
    await waitFor(() => expect(mocks.proposal).toHaveBeenCalledTimes(1));

    rerender(
      <SkillLearningPanel
        locale="en"
        activeSessionId="session-a"
        activeProjectPath="/project-b"
      />,
    );
    await act(async () => pending.resolve(safeProposal("Stale Proposal Skill")));

    expect(screen.queryByText("Stale Proposal Skill")).toBeNull();
    expect(
      screen.queryByRole("article", {
        name: "Pending Skill improvement proposal",
      }),
    ).toBeNull();
  });

  it("shows Host list failures and makes the browser fallback inert", async () => {
    mocks.list.mockRejectedValue(new Error("ledger unavailable"));
    const { SkillLearningPanel } = await import("./SkillLearningPanel");
    render(<SkillLearningPanel locale="en" activeSessionId="session-a" />);

    expect(
      await screen.findByText("Host Skill review error: ledger unavailable"),
    ).toBeTruthy();

    cleanup();
    mocks.list.mockClear();
    mocks.isTauri.mockReturnValue(false);
    render(<SkillLearningPanel locale="en" activeSessionId="session-a" />);
    expect(
      screen.getByText(
        "Skill learning review is available in the Sunsetz desktop window.",
      ),
    ).toBeTruthy();
    expect(
      screen.getByLabelText("Skill metadata query").hasAttribute("disabled"),
    ).toBe(true);
    expect(mocks.list).not.toHaveBeenCalled();
  });
});
