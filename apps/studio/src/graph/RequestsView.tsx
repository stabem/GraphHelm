import { useCallback, useEffect, useState, type CSSProperties } from "react";

import type { BoardTask, JourneyFlow, Runtime, TasksBoard } from "../runtime/client";
import { ReviewModal } from "./ReviewModal";
import { reviewable } from "./review";
import { idleText, stepAged } from "../lanes/board";
import { cards, KANBAN, KANBAN_NAME, type OwnerRequest } from "./requests";
import { COLUMN_COLOR, counts, detail, graphRequests, journeyCards, journeyMeta } from "./layout";

/** The board and the requests are read again this often. */
const POLL_MS = 5_000;

type Mode = "graph" | "kanban";

interface Props {
  runtime: Runtime;
  executionId: string;
  journeys: JourneyFlow[] | null;
  /** Opens Proof on a journey ("Open the journey in Proof"). */
  onOpenProof?: (journey: string) => void;
}

/**
 * #1801 (S3), redrawn to the approved design by #1903 (docs/design/studio/Graph.dc.html): the
 * JOURNEYS WITH WORK column, the header with its counts and the Kanban | Graph toggle, the graph
 * from what was asked to the issues, their pull requests and the result, and the panel of the
 * selected issue. "Merged" is not done: a card reads Proven only when its journeys passed.
 */
export function RequestsView({ runtime, executionId, journeys, onOpenProof }: Props) {
  const [mode, setMode] = useState<Mode>("graph");
  const [journey, setJourney] = useState<string | null>(null);
  const [board, setBoard] = useState<TasksBoard | null>(null);
  const [requests, setRequests] = useState<OwnerRequest[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [reviewing, setReviewing] = useState<BoardTask | null>(null);
  const [selected, setSelected] = useState<number | null>(null);

  const read = useCallback(async () => {
    try {
      const [nextBoard, nextRequests] = await Promise.all([runtime.board(executionId), runtime.requests(executionId)]);
      setBoard(nextBoard);
      setRequests(nextRequests);
      setError(null);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "The requests could not be read.");
    }
  }, [runtime, executionId]);

  useEffect(() => {
    void read();
    const timer = setInterval(() => void read(), POLL_MS);
    return () => clearInterval(timer);
  }, [read]);

  if (error !== null && board === null) return <p role="alert">{error}</p>;
  if (board === null || requests === null) return <p>Reading the run's requests.</p>;

  const flows = journeys ?? [];
  const withWork = journeyCards(board, flows);
  const picked = withWork.find((card) => card.id === journey) ?? null;
  const graph = graphRequests(requests, board, flows, journey);
  const tally = counts(board, flows, journey);
  const allIssues = new Set(board.tasks.filter((task) => task.releasedBy === undefined).map((task) => task.issue ?? task.key)).size;
  const flat = graph.flatMap((row) => row.items.map((item) => ({ item, row })));
  const chosen = flat.find((entry) => entry.item.issue === selected) ?? flat[0] ?? null;
  const panel = chosen === null ? null : detail(chosen.item, chosen.row, board);
  const idle = flows.length - withWork.filter((card) => flows.some((flow) => flow.id === card.id)).length;

  return (
    <div className="gv">
      <aside aria-label="Journeys with work" className="gv-journeys">
        <span className="mono gv-label">JOURNEYS WITH WORK · {withWork.length} of {Math.max(flows.length, withWork.length)}</span>
        <button type="button" className="b gv-journey gv-all" aria-label="All journeys" aria-pressed={journey === null} onClick={() => setJourney(null)}>
          <span className="gv-grow">All journeys</span><span className="mono gv-muted">{allIssues} issue{allIssues === 1 ? "" : "s"}</span>
        </button>
        {withWork.map((card) => (
          <button key={card.id} type="button" className="b gv-journey" aria-pressed={journey === card.id} onClick={() => setJourney(card.id)}>
            <span className="gv-row"><span className="gv-swatch" style={{ background: card.color }} /><span className="gv-grow gv-strong">{card.title}</span></span>
            <span className="mono gv-meta">{journeyMeta(card)}</span>
            <span className="gv-lanes">{card.lanes.join(", ") || "no lane"}</span>
          </button>
        ))}
        {idle > 0 && <span className="gv-note">{idle} journey{idle === 1 ? " has" : "s have"} no work in flight. <a href={`?view=proof`} onClick={(event) => { if (onOpenProof && flows[0]) { event.preventDefault(); onOpenProof(flows[0].id); } }}>See them in Proof</a></span>}
      </aside>

      <div className="gv-main">
        <div className="gv-head">
          <h2 className="gv-title">{picked === null ? "Work in flight, all journeys" : picked.title}</h2>
          <span className="mono gv-counts">
            <span style={{ color: COLUMN_COLOR.implementing }}>{tally.implementing} implementing</span>{"  "}
            <span style={{ color: COLUMN_COLOR.review }}>{tally.review} in review</span>{"  "}
            <span style={{ color: COLUMN_COLOR.merged }}>{tally.merged} merged, waits proof</span>{"  "}
            <span style={{ color: COLUMN_COLOR.ready }}>{tally.ready} stalled</span>
          </span>
          <span className="gv-grow" />
          <div role="tablist" aria-label="View" className="gv-tabs">
            <button type="button" role="tab" className="b gv-tab" aria-label="Kanban view" aria-selected={mode === "kanban"} onClick={() => setMode("kanban")}>Kanban</button>
            <button type="button" role="tab" className="b gv-tab" aria-label="Graph view" aria-selected={mode === "graph"} onClick={() => setMode("graph")}>Graph</button>
          </div>
          {error !== null && <span role="status" className="muted small">Last read failed: {error}</span>}
        </div>

        {mode === "graph" ? (
          <section aria-label="Request graph" className="gv-canvas">
            {graph.length === 0 ? (
              <p>{requests.length === 0 ? "No owner request is recorded on this run yet." : "No request has work in this journey."}</p>
            ) : (
              <>
                <div className="mono gv-cols" aria-hidden="true">
                  <span style={{ left: 0 }}>ASKED</span><span style={{ left: 300 }}>ISSUES THE ORCHESTRATOR OPENED</span><span style={{ left: 600 }}>PULL REQUESTS</span><span style={{ left: 840 }}>RESULT</span>
                </div>
                <ul aria-label="Owner requests" className="gv-requests">
                  {graph.map((row) => (
                    <li key={row.request.requestId} aria-label={row.request.title ?? row.request.ask} className="gv-request" style={{ "--h": `${row.height}px` } as CSSProperties}>
                      <svg aria-hidden="true" className="gv-links" width="1040" height={row.height}>
                        {row.links.map((link, index) => <path key={index} d={link.d} fill="none" stroke={link.color} strokeWidth="1.5" strokeDasharray={link.dashed ? "4 4" : undefined} />)}
                      </svg>
                      <div className="gv-ask gv-node" style={{ "--x": "0px", "--y": `${row.askY}px` } as CSSProperties}>
                        <span className="mono gv-ask-when">YOU ASKED · {row.when}</span>
                        <span className="gv-ask-text">{row.request.title ?? row.request.ask}</span>
                        <span className="gv-bar">{row.bar.map((color, index) => <span key={index} className="gv-sq" style={{ background: color }} />)}</span>
                        <span className="mono gv-meta">{row.items.length === 0 ? "no issue opened yet" : row.summary}</span>
                      </div>
                      {row.items.map((item) => (
                        <div key={item.issue} className="gv-lane-row">
                          <button
                            type="button"
                            className="b gv-issue gv-node"
                            aria-pressed={chosen?.item.issue === item.issue}
                            aria-label={`Issue #${item.issue}`}
                            style={{ "--x": "300px", "--y": `${item.y}px` } as CSSProperties}
                            onClick={() => {
                              setSelected(item.issue);
                              if (item.task !== null && item.column !== null && reviewable(item.column, item.pr)) setReviewing(item.task);
                            }}
                          >
                            <span className="gv-row"><span className="mono gv-muted">#{item.issue}</span><span className="gv-grow gv-ellipsis">{item.title}</span></span>
                            <span className="gv-row" aria-label={`Steps ${item.blocks.filter((b) => b.color === "#3ddc97").length} of ${item.blocks.length}`}>
                              {item.blocks.map((block) => <span key={block.step} title={block.step} className="gv-sq small" style={{ background: block.color }} />)}
                              <span className="mono gv-meta gv-agent">{item.lane ?? "no lane"}</span>
                            </span>
                            <span className="mono gv-meta gv-ellipsis">{item.last}</span>
                            <span className="gv-row gv-jname"><span className="gv-swatch small" style={{ background: item.journey?.color ?? "#45454e" }} />{item.journey?.title ?? "no journey"}</span>
                          </button>
                          <div role="group" aria-label={`Pull request of #${item.issue}`} className={`gv-pr gv-node${item.pr === null ? " none" : ""}`} style={{ "--x": "600px", "--y": `${item.y + 16}px` } as CSSProperties}>
                            <span className="mono" style={{ color: item.pr === null ? "#8a8a96" : "#8ab4ff" }}>{item.pr === null ? "no PR yet" : `PR #${item.pr}`}</span>
                            <span className="mono gv-meta gv-ellipsis">{item.prLine}</span>
                          </div>
                          <div role="group" aria-label={`Result of #${item.issue}`} className="gv-res gv-node" style={{ "--x": "840px", "--y": `${item.y + 16}px`, borderColor: item.result.color === "#8b7cff" ? "#3a3466" : undefined } as CSSProperties}>
                            <span style={{ color: item.result.color }}>{item.result.word}</span>
                            <span className="mono gv-meta">{item.result.detail}</span>
                          </div>
                        </div>
                      ))}
                    </li>
                  ))}
                </ul>
              </>
            )}
          </section>
        ) : (
          <section aria-label="Kanban" className="kanban">
            {KANBAN.map((column) => {
              const inColumn = cards(board, flows, journey).filter((card) => card.column === column);
              return (
                <div key={column} className="kanban-col">
                  <h2><span className="gv-dot" style={{ background: COLUMN_COLOR[column] }} />{KANBAN_NAME[column]} <span className="muted">{inColumn.length}</span></h2>
                  <ul aria-label={KANBAN_NAME[column]}>
                    {inColumn.map(({ task, column: at, stepMinutes }) => (
                      <li key={task.key} className={`kanban-card${stepAged(stepMinutes) ? " aged" : ""}`}>
                        <span className="mono small">{task.issue === null ? task.key : `#${task.issue}`}{task.pr === null ? "" : ` · PR #${task.pr}`}</span>
                        <span>{task.prTitle ?? task.title ?? ""}</span>
                        <span className="muted small">{task.lane ?? "no lane"} · {task.step}{stepMinutes === null ? "" : " "}{stepMinutes !== null && <span role="group" aria-label="Time in step" className={`mono step-age${stepAged(stepMinutes) ? " aged" : ""}`}>{idleText(stepMinutes)}</span>}</span>
                        {reviewable(at, task.pr) && (
                          <button type="button" className="b ghost small" onClick={() => setReviewing(task)}>Open review</button>
                        )}
                      </li>
                    ))}
                  </ul>
                </div>
              );
            })}
          </section>
        )}
        <div className="gv-foot"><span>Cards older than 1 h in a column turn red.</span><span className="gv-grow" /><span>Merged is not done. Done = the journey replay proves it.</span></div>
      </div>

      <aside aria-label="Selected work" className="gv-detail">
        {chosen === null || panel === null ? (
          <span className="gv-muted">Pick an issue in the graph.</span>
        ) : (
          <>
            <span className="mono gv-label">ISSUE #{chosen.item.issue}</span>
            <strong className="gv-detail-title">{chosen.item.title}</strong>
            <span className="gv-row gv-muted"><span className="gv-swatch" style={{ background: chosen.item.journey?.color ?? "#45454e" }} />{chosen.item.journey?.title ?? "no journey"}</span>
            <span className="mono gv-label gv-gap">NOW</span>
            {panel.now.map((line) => <span key={line}>{line}</span>)}
            <span className="mono gv-label gv-gap">HISTORY</span>
            <span className="mono gv-history">{panel.history.join("\n")}</span>
            {chosen.item.journey !== null && (
              <a
                className="gv-gap"
                href={`?view=proof&journey=${encodeURIComponent(chosen.item.journey.id)}`}
                onClick={(event) => { if (onOpenProof && chosen.item.journey) { event.preventDefault(); onOpenProof(chosen.item.journey.id); } }}
              >Open the journey in Proof</a>
            )}
          </>
        )}
      </aside>
      {reviewing !== null && <ReviewModal runtime={runtime} executionId={executionId} task={reviewing} onClose={() => setReviewing(null)} />}
    </div>
  );
}
