// The selected run's team reads: who is on the team, the Claude task board, and the Runtime's task graphs.
import { useEffect, useState, type MutableRefObject } from "react";

import { RuntimeError, type RuntimeClient } from "./client";
import { readRunTeam, type RunTeamReadModel } from "./run-team";
import type { LaneFold } from "./tasks-route";
import { readClaudeTasks, type ClaudeTaskReadModel, type TaskEventRecord, type TaskState } from "./team-tasks";
import type { EventPage, EvidenceContent, RuntimeEvent } from "./types";

export interface TeamReadsInputs {
  clientRef: MutableRefObject<RuntimeClient | null>;
  selected: string;
  events: EventPage | null;
  eventList: RuntimeEvent[];
  openEvidence: (executionId: string, evidenceId: string) => Promise<EvidenceContent>;
}

export function useTeamReads({ clientRef, selected, events, eventList, openEvidence }: TeamReadsInputs) {
  const [runTeamRead, setRunTeamRead] = useState<RunTeamReadModel | null>(null);
  const [claudeTaskRead, setClaudeTaskRead] = useState<ClaudeTaskReadModel | null>(null);
  // #391: the per-task graphs, folded from the run's `task.*` records.
  const [taskGraphs, setTaskGraphs] = useState<({ executionId: string; tasks: TaskState[]; records: TaskEventRecord[] } & LaneFold) | null>(null);
  const [historyFailure, setHistoryFailure] = useState<{ executionId: string; count: number } | null>(null);
  const [historyRetry, setHistoryRetry] = useState(0);
  // #1192: the run whose Runtime answered the tasks route with "no such route" (an older binary).
  // The Team, Graph and Lanes views then say so in one sentence instead of drawing an empty board.
  const [tasksRouteMissing, setTasksRouteMissing] = useState<string | null>(null);
  // #1192: the run whose Runtime refused the tasks route (seen live: GHE013_READ_BUDGET_EXCEEDED
  // on a run with 4307 task envelopes). The Runtime's own code and sentence are shown; "N records
  // could not be read" is a different failure (envelopes that did not verify) and stays for that.
  const [tasksRouteFailure, setTasksRouteFailure] = useState<{ executionId: string; text: string } | null>(null);
  useEffect(() => {
    if (!selected || events === null) {
      setRunTeamRead(null);
      setClaudeTaskRead(null);
      return;
    }
    let cancelled = false;
    const run = selected;
    const client = clientRef.current;
    setHistoryFailure(null);
    setTasksRouteFailure((current) => (current?.executionId === run ? null : current));
    void readRunTeam(run, eventList, openEvidence)
      .then((result) => { if (!cancelled) setRunTeamRead(result); })
      .catch(() => { if (!cancelled) setRunTeamRead({ executionId: run, members: [], messages: [], rejected: 0, unavailable: true }); });
    void readClaudeTasks({ executionId: run, events: eventList, readEvidence: openEvidence })
      .then((result) => { if (!cancelled) setClaudeTaskRead(result); })
      .catch(() => { if (!cancelled) setClaudeTaskRead({ executionId: run, tasks: [], rejected: 1 }); });
    // #1192 (plan A1): the task projection is the Runtime's (`GET /v1/executions/{id}/tasks`,
    // #1149): one request returns the slices and the verified records; the browser opens no
    // envelope and folds nothing. `rejected` is the Runtime's count of envelopes that did not
    // verify, shown where the per-envelope read failures were. `null` is an older Runtime.
    if (client === null) return () => { cancelled = true; };
    void client.getTasks(run)
      .then((reply) => {
        if (cancelled || clientRef.current !== client) return;
        if (reply === null) {
          setTasksRouteMissing(run);
          setTaskGraphs({ executionId: run, tasks: [], records: [], slices: [], routes: [] });
          return;
        }
        setTasksRouteMissing((current) => (current === run ? null : current));
        setTaskGraphs({ executionId: run, tasks: reply.tasks, records: reply.records, slices: reply.slices, routes: reply.routes, liveness: reply.liveness, leases: reply.leases, backlog: reply.backlog, durations: reply.durations });
        setHistoryFailure(reply.rejected > 0 ? { executionId: run, count: reply.rejected } : null);
      })
      .catch((error: unknown) => { if (!cancelled && clientRef.current === client) {
        const text = error instanceof RuntimeError && error.code !== ""
          ? `${error.code} · ${error.diagnostics[0]?.message ?? error.message}`
          : error instanceof Error ? error.message : String(error);
        setTasksRouteFailure({ executionId: run, text });
        setTaskGraphs((current) => (current?.executionId === run ? current : { executionId: run, tasks: [], records: [], slices: [], routes: [] }));
      } });
    return () => { cancelled = true; };
  }, [selected, events, eventList, openEvidence, historyRetry]);
  return {
    runTeamRead, claudeTaskRead, taskGraphs, historyFailure, setHistoryRetry, tasksRouteMissing, tasksRouteFailure,
  };
}
