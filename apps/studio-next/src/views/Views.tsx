import type { ReactNode } from "react";

import type { RunSummary } from "../runtime/client";

/** The Graph region: which run is open and how it stands (the run index), then `requests`, the
 * run's owner requests as a flowchart or Kanban (#1801). */
export function GraphView({ run, runId, children, requests }: { run: RunSummary | null; runId: string | null; children?: ReactNode; requests?: ReactNode }) {
  return (
    <section id="panel-graph" aria-label="Graph" className="view">
      {children}
      {runId === null ? (
        <p>No team run is open in this project.</p>
      ) : run === null ? (
        <p>Run {runId} is not in this project's run list yet.</p>
      ) : (
        <>
          <h1>{run.objective || run.executionId}</h1>
          <dl className="facts">
            <dt>Run</dt><dd className="mono">{run.executionId}</dd>
            <dt>Status</dt><dd>{run.status}</dd>
            <dt>Started</dt><dd className="mono">{run.startedAt ?? "not recorded"}</dd>
            <dt>Last event</dt><dd className="mono">{run.lastEventAt ?? "none"}</dd>
          </dl>
          {requests}
        </>
      )}
    </section>
  );
}
