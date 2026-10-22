import { useEffect, useMemo, useRef, useState } from "react";

import { OWNER, type Capacity, type FileCollisions, type LaneAction, type Runtime, type TasksBoard } from "../runtime/client";
import { controlBody, draftFor, messageBody, newId, type Draft } from "./drafts";
import { actionText, clock } from "./format";
import { capacityRows, feed, needsAction, type Need } from "./panels";
import { idleText } from "./board";

/** Collisions are re-read with the board's pace slowed down; capacity is a machine sample. */
const COLLISIONS_MS = 15_000;
const CAPACITY_MS = 30_000;
const FEED_ROWS = 12;

interface Props {
  runtime: Runtime;
  executionId: string;
  board: TasksBoard;
  actions: Record<string, { rows: LaneAction[] }>;
  /** Answer: select the lane and focus its reply box. */
  onAnswer: (lane: string) => void;
  onSent: () => Promise<void>;
}

const DONE: Record<Need["verb"], string> = {
  Answer: "Answering", Assign: "Assigned", Merge: "Merge requested", Archive: "Archived", Reassign: "Reassigned", Wake: "Woken",
};

/** The design's bottom row (docs/design/studio/Lanes.dc.html): Needs action, file collisions with
 * capacity, and the live feed. Every read is a public Runtime route. */
export function LanesPanels({ runtime, executionId, board, actions, onAnswer, onSent }: Props) {
  const [collisions, setCollisions] = useState<FileCollisions | null>(null);
  const [collisionsError, setCollisionsError] = useState<string | null>(null);
  const [capacity, setCapacity] = useState<Capacity | null>(null);
  const [capacityError, setCapacityError] = useState<string | null>(null);
  const [sending, setSending] = useState<string | null>(null);
  const [last, setLast] = useState<string | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  // #1837: a failed request is resent with the same id and bytes.
  const drafts = useRef(new Map<string, Draft>());

  useEffect(() => {
    let live = true;
    const read = () => runtime.collisions(executionId).then(
      (view) => { if (live) { setCollisions(view); setCollisionsError(null); } },
      (error: unknown) => { if (live) setCollisionsError(error instanceof Error ? error.message : "The collisions could not be read."); },
    );
    void read();
    const timer = setInterval(() => void read(), COLLISIONS_MS);
    return () => { live = false; clearInterval(timer); };
  }, [runtime, executionId]);

  useEffect(() => {
    let live = true;
    const read = () => runtime.capacity().then(
      (view) => { if (live) { setCapacity(view); setCapacityError(null); } },
      (error: unknown) => { if (live) setCapacityError(error instanceof Error ? error.message : "The capacity could not be read."); },
    );
    void read();
    const timer = setInterval(() => void read(), CAPACITY_MS);
    return () => { live = false; clearInterval(timer); };
  }, [runtime]);

  const needs = useMemo(() => needsAction(board), [board]);
  const rows = useMemo(() => feed(actions, FEED_ROWS), [actions]);
  const now = Date.parse(board.now);

  const act = async (need: Need) => {
    if (sending !== null) return;
    if (need.verb === "Answer") {
      onAnswer(need.lane);
      setFailure(null);
      setLast(`${DONE.Answer} ${need.lane}`);
      return;
    }
    const pr = need.slice?.pr ?? null;
    const at = new Date();
    let intent: string;
    let make: () => { id: string; body: Record<string, unknown> };
    let post: (draft: Draft) => Promise<void>;
    if (need.verb === "Assign" || need.verb === "Merge") {
      const to = need.to;
      if (to === null) return;
      const text = need.verb === "Assign"
        ? `The owner assigns you the review of PR #${pr} (${need.lane}).`
        : `The owner asks you to merge PR #${pr}, approved and ready.`;
      intent = JSON.stringify([need.id, to]);
      make = () => { const id = newId("studio-need"); return { id, body: messageBody(executionId, id, to, text, null, at, OWNER.id) }; };
      post = (draft) => runtime.messageLane(executionId, draft);
    } else {
      const control = need.verb === "Archive" ? "stop" : need.verb === "Reassign" ? "reassign" : "wake";
      const reassign = control === "reassign" && need.slice !== null && need.to !== null ? { sliceKey: need.slice.key, to: need.to } : undefined;
      if (control === "reassign" && reassign === undefined) return;
      const reason = control === "stop" ? `The owner archived ${need.lane} from Needs action.`
        : control === "wake" ? `The owner woke ${need.lane} from Needs action.`
        : `The owner reassigned ${reassign?.sliceKey} to ${reassign?.to} from Needs action.`;
      intent = JSON.stringify([need.id, control, reassign ?? null]);
      make = () => ({ id: newId(`studio-${control}`), body: controlBody(control, reason, at, reassign) });
      post = (draft) => runtime.controlLane(executionId, need.lane, control, draft);
    }
    const draft = draftFor(drafts.current.get(need.id), intent, make);
    drafts.current.set(need.id, draft);
    setSending(need.id);
    setFailure(null);
    try {
      await post(draft);
      drafts.current.delete(need.id);
      setLast(`${DONE[need.verb]} ${need.to !== null ? `${need.slice?.pr ? `PR #${need.slice.pr} ` : need.slice?.issue ? `#${need.slice.issue} ` : ""}to ${need.to}` : need.lane}`);
      await onSent();
    } catch (error) {
      setFailure(error instanceof Error ? error.message : "The Runtime did not record it.");
    } finally {
      setSending(null);
    }
  };

  const waited = (since: string | null) => {
    const at = since === null ? NaN : Date.parse(since);
    return Number.isFinite(at) && Number.isFinite(now) ? `waiting ${idleText(Math.max(0, Math.floor((now - at) / 60_000)))}` : "no time recorded";
  };
  const capacityShown = capacity === null ? null : capacityRows(capacity);

  return (
    <div className="lane-panels">
      <section aria-label="Needs action" className="lane-box">
        <div className="box-head"><strong className="mono">NEEDS ACTION</strong><span className="mono faint">· oldest first</span><span className="mono count late">{needs.length}</span></div>
        {last !== null && <span role="status" aria-label="Last action" className="sent small">{last}</span>}
        {failure !== null && <span role="alert" className="error small">{failure}</span>}
        {needs.length === 0 ? <span className="muted small">Nothing waits on you.</span> : (
          <ul aria-label="Needs action items" className="needs">
            {needs.map((need, index) => (
              <li key={need.id} aria-label={`Needs action item ${index + 1}`} className="need">
                <span className={`need-dot ${need.tone}`} />
                <span className="need-text"><span className="need-what">{need.what}</span><span className="mono faint">{waited(need.since)}{need.to !== null && need.verb !== "Merge" ? ` · to ${need.to}` : ""}</span></span>
                <button type="button" className="b ghost small" disabled={sending !== null || ((need.verb === "Assign" || need.verb === "Reassign") && need.to === null)}
                  title={(need.verb === "Assign" || need.verb === "Reassign") && need.to === null ? "No free live lane can take it." : undefined}
                  onClick={() => void act(need)}>{need.verb}</button>
              </li>
            ))}
          </ul>
        )}
      </section>
      <section aria-label="Risks and capacity" className="lane-box">
        <strong className="mono">FILE COLLISIONS</strong>
        {collisionsError !== null ? <span className="error small">{collisionsError}</span>
          : collisions === null ? <span className="muted small">Reading…</span>
          : collisions.collisions.length === 0 ? <span className="muted small">No two open slices name the same file.</span> : (
            <ul aria-label="File collisions" className="collisions">
              {collisions.collisions.map((row) => (
                <li key={row.path} className="collision">
                  <span className="mono path">{row.path}</span>
                  <span className="muted">{row.holders.map((holder) => `${holder.pr !== undefined ? `PR #${holder.pr}` : holder.issue !== undefined ? `#${holder.issue}` : holder.key}${holder.lane ? ` (${holder.lane})` : ""}`).join(" and ")} {row.holders.length === 2 ? "both" : "all"} change it</span>
                </li>
              ))}
              {collisions.truncated && <li className="mono more">more paths collide than listed</li>}
            </ul>
          )}
        <strong className="mono">CAPACITY</strong>
        {capacityError !== null ? <span className="error small">{capacityError}</span>
          : capacityShown === null ? <span className="muted small">Reading…</span> : (
            <ul aria-label="Capacity" className="capacity">
              {capacityShown.map((row) => (
                <li key={row.name} title={row.reason ?? undefined}>
                  <span className="cap-line"><span>{row.name}</span><span className={`mono cap-value ${row.tone}`}>{row.value}</span></span>
                  {row.fill !== null && <span className="cap-bar"><span className={`cap-fill ${row.tone}`} style={{ width: `${Math.round(row.fill * 100)}%` }} /></span>}
                  {row.reason !== null && <span className="mono faint small">{row.reason}</span>}
                </li>
              ))}
            </ul>
          )}
      </section>
      <section aria-label="Live feed" className="lane-box">
        <div className="box-head"><strong className="mono">LIVE FEED</strong><span className="mono live-mark">● live</span></div>
        {rows.length === 0 ? <span className="muted small">No action recorded yet.</span> : (
          <ul aria-label="Live feed rows" className="feed">
            {rows.map((row) => (
              <li key={`${row.lane}:${row.action.sequence}`}>
                <span className="mono faint">{clock(row.action.occurredAt)}</span>
                <span className="mono feed-who">{row.lane}</span>
                <span className="muted">{actionText(row.action)}</span>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}
