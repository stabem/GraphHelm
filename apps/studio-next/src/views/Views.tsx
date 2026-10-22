import type { ReactNode } from "react";

import type { JourneyFlow, RunSummary } from "../runtime/client";

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

/** The Proof region: the project's journeys from `GET /v1/journey-flows`. Run, compare and
 * approve are #1805-#1807 (S7-S9). */
export function ProofView({ journeys, error }: { journeys: JourneyFlow[] | null; error: string | null }) {
  return (
    <section id="panel-proof" aria-label="Proof" className="view">
      {error !== null ? <p className="error">{error}</p> : journeys === null ? <p>Reading the journeys…</p> : (
        <ul className="journeys" aria-label="Journeys">
          {journeys.length === 0 ? <li><span>This project has no journeys yet.</span></li> : journeys.map((flow) => (
            <li key={flow.id}>
              <span>{flow.name || flow.title || flow.id}</span>
              <span className="mono">{flow.status}</span>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
