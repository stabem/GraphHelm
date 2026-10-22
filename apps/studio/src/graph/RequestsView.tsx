import { useCallback, useEffect, useState } from "react";

import type { BoardTask, JourneyFlow, Runtime, TasksBoard } from "../runtime/client";
import { ReviewModal } from "./ReviewModal";
import { reviewable } from "./review";
import { idleText, stepAged } from "../lanes/board";
import { cards, journeysWithWork, KANBAN, KANBAN_NAME, requestsFor, result, type OwnerRequest } from "./requests";

/** The board and the requests are read again this often. */
const POLL_MS = 5_000;

type Mode = "graph" | "kanban";

interface Props {
  runtime: Runtime;
  executionId: string;
  journeys: JourneyFlow[] | null;
}

/**
 * #1801 (S3): the Graph region of a run. Each owner request is drawn as a row from what was asked to
 * the issues opened for it, their pull requests and the result (`/requests`, #1792), or the same
 * slices as Kanban columns (`/tasks`). A journey filter narrows both. "Merged" is not done: a card
 * and a request read Proven only when their journeys passed their last run (`/journey-flows`).
 */
export function RequestsView({ runtime, executionId, journeys }: Props) {
  const [mode, setMode] = useState<Mode>("graph");
  const [journey, setJourney] = useState<string | null>(null);
  const [board, setBoard] = useState<TasksBoard | null>(null);
  const [requests, setRequests] = useState<OwnerRequest[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [reviewing, setReviewing] = useState<BoardTask | null>(null);

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
  const withWork = journeysWithWork(board, flows);
  const shown = requestsFor(requests, board, journey);
  const titleOf = (issue: number) => board.tasks.find((task) => task.issue === issue)?.title ?? null;
  const laneOf = (issue: number) => board.tasks.find((task) => task.issue === issue)?.lane ?? null;

  return (
    <div className="requests">
      <div className="requests-bar">
        <label>
          <span className="muted small">Work by journey</span>{" "}
          <select aria-label="Journey filter" value={journey ?? ""} onChange={(event) => setJourney(event.target.value === "" ? null : event.target.value)}>
            <option value="">All journeys</option>
            {withWork.map((row) => <option key={row.id} value={row.id}>{row.title} · {row.slices}</option>)}
          </select>
        </label>
        <div role="group" aria-label="Graph mode" className="seg">
          <button type="button" className={mode === "kanban" ? "b on" : "b"} aria-pressed={mode === "kanban"} onClick={() => setMode("kanban")}>Kanban</button>
          <button type="button" className={mode === "graph" ? "b on" : "b"} aria-pressed={mode === "graph"} onClick={() => setMode("graph")}>Graph</button>
        </div>
        {error !== null && <span role="status" className="muted small">Last read failed: {error}</span>}
      </div>

      {mode === "graph" ? (
        shown.length === 0 ? (
          <p>{requests.length === 0 ? "No owner request is recorded on this run yet." : "No request has work in this journey."}</p>
        ) : (
          <ul aria-label="Owner requests" className="request-rows">
            {shown.map((request) => {
              const outcome = result(request, board, flows);
              return (
                <li key={request.requestId} className="request-row">
                  <div className="request-ask">
                    <span className="muted small">You asked · {request.recordedBy}</span>
                    <strong>{request.title ?? request.ask}</strong>
                    {request.title !== null && <span className="small">{request.ask}</span>}
                  </div>
                  <div role="group" aria-label="Request issues" className="request-col">
                    {request.issues.length === 0 ? <span className="muted small">no issue yet</span> : request.issues.map((row) => (
                      <span key={row.issue} className="request-item">
                        <span className="mono">#{row.issue}</span> {titleOf(row.issue) ?? ""} <span className="muted small">{laneOf(row.issue) ?? ""}</span>
                      </span>
                    ))}
                  </div>
                  <div role="group" aria-label="Request pull requests" className="request-col">
                    {request.issues.flatMap((row) => row.slices).filter((slice) => slice.pr !== null).length === 0
                      ? <span className="muted small">no PR yet</span>
                      : request.issues.flatMap((row) => row.slices.filter((slice) => slice.pr !== null).map((slice) => (
                        <span key={`${row.issue}-${slice.pr}`} className="request-item"><span className="mono">PR #{slice.pr}</span> <span className="muted small">{slice.step}</span></span>
                      )))}
                  </div>
                  <div role="group" aria-label="Request result" className={`request-col result-${outcome.word.replace(" ", "-")}`}>
                    <strong>{outcome.word}</strong>
                    <span className="muted small">{outcome.detail}</span>
                  </div>
                </li>
              );
            })}
          </ul>
        )
      ) : (
        <section aria-label="Kanban" className="kanban">
          {KANBAN.map((column) => {
            const inColumn = cards(board, flows, journey).filter((card) => card.column === column);
            return (
              <div key={column} className="kanban-col">
                <h2>{KANBAN_NAME[column]} <span className="muted">{inColumn.length}</span></h2>
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
      <p className="muted small">Merged is not done. Done is the journey replay proving it.</p>
      {reviewing !== null && <ReviewModal runtime={runtime} executionId={executionId} task={reviewing} onClose={() => setReviewing(null)} />}
    </div>
  );
}
