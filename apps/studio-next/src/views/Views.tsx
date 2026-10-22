import type { ReactNode } from "react";

import type { RunSummary } from "../runtime/client";

/** The Graph region. Here it says which run is open and how it stands, from the run index; the
 * requests flowchart and Kanban arrive with #1801 (S3). */
export function GraphView({ run, runId, children }: { run: RunSummary | null; runId: string | null; children?: ReactNode }) {
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
        </>
      )}
    </section>
  );
}

/** The Lanes region; the board and agent panel are #1803 (S5). */
export function LanesView() {
  return (
    <section id="panel-lanes" aria-label="Lanes" className="view">
      <p>The lanes board is not in this Studio yet (#1803).</p>
    </section>
  );
}
