// @vitest-environment jsdom

import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { CommandJobEventV1, CommandJobSummaryV1 } from "@/lib/session";

const listeners = new Map<string, (payload: unknown) => void>();
const listenMock = vi.fn(
  async (event: string, handler: (payload: unknown) => void) => {
    listeners.set(event, handler);
    return () => {
      listeners.delete(event);
    };
  },
);
const listMock = vi.fn(async (_sessionId: string): Promise<CommandJobSummaryV1[]> => []);

vi.mock("@/lib/api", () => ({
  listen: (event: string, handler: (payload: unknown) => void) =>
    listenMock(event, handler),
  sessionCommandJobsListV1: (sessionId: string) => listMock(sessionId),
}));

function emit(event: CommandJobEventV1) {
  listeners.get("session://command_job_v1")?.(event);
}

import { useHostedCommandJobs } from "./useHostedCommandJobs";

describe("useHostedCommandJobs", () => {
  beforeEach(() => {
    listeners.clear();
    listenMock.mockClear();
    listMock.mockClear();
  });
  afterEach(() => {
    vi.clearAllMocks();
  });

  it("hydrates from session_command_jobs_list_v1 on session switch", async () => {
    listMock.mockResolvedValueOnce([
      { id: "job-1", status: "running", command: "sleep 30", summary: "" },
    ]);
    const { result } = renderHook(
      ({ sessionId }: { sessionId: string | null }) =>
        useHostedCommandJobs(sessionId),
      { initialProps: { sessionId: "sess-a" } },
    );
    await waitFor(() => expect(result.current.jobsForSession).toHaveLength(1));
    expect(result.current.runningCount).toBe(1);
    expect(listMock).toHaveBeenCalledWith("sess-a");
  });

  it("applies session://command_job_v1 events: registers then transitions to terminal", async () => {
    const { result } = renderHook(() => useHostedCommandJobs("sess-a"));
    await waitFor(() => expect(listenMock).toHaveBeenCalled());
    act(() => {
      emit({
        sessionId: "sess-a",
        jobId: "job-1",
        status: "running",
        command: "sleep 30",
        summary: "",
      });
    });
    expect(result.current.jobsForSession).toHaveLength(1);
    expect(result.current.runningCount).toBe(1);
    act(() => {
      emit({
        sessionId: "sess-a",
        jobId: "job-1",
        status: "completed",
        command: "sleep 30",
        summary: "done",
      });
    });
    expect(result.current.jobsForSession).toHaveLength(1);
    expect(result.current.jobsForSession[0]?.status).toBe("completed");
    expect(result.current.runningCount).toBe(0);
  });

  it("scopes jobs by session — an event for another session does not leak in", async () => {
    const { result } = renderHook(() => useHostedCommandJobs("sess-a"));
    await waitFor(() => expect(listenMock).toHaveBeenCalled());
    act(() => {
      emit({
        sessionId: "sess-b",
        jobId: "job-1",
        status: "running",
        command: "sleep 30",
        summary: "",
      });
    });
    expect(result.current.jobsForSession).toHaveLength(0);
  });

  it("returns no jobs when sessionId is null", () => {
    const { result } = renderHook(() => useHostedCommandJobs(null));
    expect(result.current.jobsForSession).toHaveLength(0);
    expect(result.current.runningCount).toBe(0);
  });
});
