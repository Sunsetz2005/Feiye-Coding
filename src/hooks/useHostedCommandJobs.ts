import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import * as api from "@/lib/api";
import type { CommandJobEventV1, CommandJobSummaryV1 } from "@/lib/session";

/**
 * Per-session hosted `run_command background` job list — drives the
 * composer's hosted-count pill. Hydrates from `session_command_jobs_list_v1`
 * on session switch (so a reconnect doesn't wait for the next event) and
 * stays live via `session://command_job_v1` (registered, then one terminal
 * transition per job; no stdout body in either).
 */
export function useHostedCommandJobs(sessionId: string | null) {
  const [jobsBySession, setJobsBySession] = useState<
    Record<string, CommandJobSummaryV1[]>
  >({});
  const jobsBySessionRef = useRef(jobsBySession);
  jobsBySessionRef.current = jobsBySession;

  const writeSession = useCallback(
    (sid: string, jobs: CommandJobSummaryV1[]) => {
      const next = { ...jobsBySessionRef.current, [sid]: jobs };
      jobsBySessionRef.current = next;
      setJobsBySession(next);
    },
    [],
  );

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    void api
      .listen<CommandJobEventV1>("session://command_job_v1", (payload) => {
        if (cancelled || !payload?.sessionId || !payload.jobId) return;
        const current = jobsBySessionRef.current[payload.sessionId] ?? [];
        const next: CommandJobSummaryV1 = {
          id: payload.jobId,
          status: payload.status,
          command: payload.command,
          summary: payload.summary,
        };
        const existingIndex = current.findIndex(
          (job) => job.id === payload.jobId,
        );
        const updated =
          existingIndex >= 0
            ? current.map((job, index) =>
                index === existingIndex ? next : job,
              )
            : [...current, next];
        writeSession(payload.sessionId, updated);
      })
      .then((off) => {
        if (cancelled) off();
        else unlisten = off;
      });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [writeSession]);

  useEffect(() => {
    if (!sessionId) return;
    let cancelled = false;
    void api.sessionCommandJobsListV1(sessionId).then((jobs) => {
      if (cancelled) return;
      writeSession(sessionId, jobs);
    });
    return () => {
      cancelled = true;
    };
  }, [sessionId, writeSession]);

  const jobsForSession = useMemo(
    () => (sessionId ? (jobsBySession[sessionId] ?? []) : []),
    [jobsBySession, sessionId],
  );
  const runningJobs = useMemo(
    () => jobsForSession.filter((job) => job.status === "running"),
    [jobsForSession],
  );

  return { jobsForSession, runningJobs, runningCount: runningJobs.length };
}
