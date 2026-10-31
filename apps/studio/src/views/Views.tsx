import type { ReactNode } from "react";

import type { RunSummary, Runtime } from "../runtime/client";
import { Empty } from "../states/ViewState";
import { When } from "../time/Stamp";
import { runState } from "./attention";
import { useRunWaits, WaitsOn } from "./RunAttention";

/** The Graph region: which run is open and how it stands (the run index), then `requests`, the
 * run's owner requests as a flowchart or Kanban (#1801). */
/** #2230: `allRuns` is how many executions are read as one project; then no single run is claimed. */
export function GraphView({ run, runId, children, requests, onStart = null, allRuns = null, runtime = null }: { run: RunSummary | null; runId: string | null; children?: ReactNode; requests?: ReactNode; onStart?: (() => void) | null; allRuns?: number | null; runtime?: Runtime | null }) {
  // #2324: what a run that waits for you waits on, read only while the Runtime says it needs you.
  const waits = useRunWaits(run?.attention === "needs_you" ? runtime : null, run?.executionId ?? null, run?.lastEventAt ?? null);
  return (
    <section id="panel-graph" aria-label="Graph" className="view">
      {children}
      {allRuns !== null ? (
        <>
          <h1>All {allRuns} runs of this project</h1>
          {requests}
        </>
      ) : runId === null ? (
        children ? null : <Empty title="No team run yet" hint="No team run is open in this project. Requests and their issues appear here once one starts." onStart={onStart} />
      ) : run === null ? (
        <p>Run {runId} is not in this project's run list yet.</p>
      ) : (
        <>
          <h1>{run.objective || run.executionId}</h1>
          <dl className="facts">
            <dt>Run</dt><dd className="mono">{run.executionId}</dd>
            <dt>Status</dt><dd className={run.attention === "needs_you" ? "run-waits" : undefined}>{runState(run.status, run.attention)}</dd>
            {Array.isArray(waits) && waits.length > 0 && (
              <><dt>Waits on</dt><dd>{waits.map((reason, index) => <div key={index}><WaitsOn reason={reason} executionId={run.executionId} /></div>)}</dd></>
            )}
            <dt>Started</dt><dd className="mono">{run.startedAt ? <When at={run.startedAt} /> : "not recorded"}</dd>
            <dt>Last event</dt><dd className="mono">{run.lastEventAt ? <When at={run.lastEventAt} /> : "none"}</dd>
          </dl>
          {requests}
        </>
      )}
    </section>
  );
}
