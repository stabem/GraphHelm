import { useCallback, useEffect, useRef, useState, type CSSProperties, type RefObject } from "react";

import { OWNER, type JourneyFlow, type LaneAction, type Runtime, type TasksBoard } from "../runtime/client";
import { controlBody, draftFor, messageBody, newId, type Draft, type LaneControl } from "./drafts";
import { actionText } from "./format";
import { When } from "../time/Stamp";
import { Empty, Loading, ReadFailure } from "../states/ViewState";
import { readCollapsed, writeCollapsed } from "./panel-rail";
import { LanesPanels } from "./LanesPanels";
import { RequestProgress } from "../graph/RequestProgress";
import { agentsFor, arrangeAsked, askedCards, requestColors, requestTitles, unlinkedIssue } from "./asked";
import type { OwnerRequest } from "../graph/requests";
import { BoardColumn } from "./BoardColumn";
import { agents, blocks, COLUMNS, idleText, idleTone, stepAged, OWNER_ADDRESS, type Agent, type Column } from "./board";

/** How often the board is read again: one tasks read, plus one activity read per lane whose last
 * record moved since the previous read. */
const POLL_MS = 5_000;
const LAST_ACTIONS = 10;

const COLUMN_NAME: Record<Column, string> = { backlog: "Backlog", implement: "Implement", review: "Review", blocked: "Blocked", silent: "Silent", merge: "Merge" };
type Filter = "all" | "reply" | "idle";

/** What a control did, as the status and the board print it. */
const DONE: Record<LaneControl, { status: string; word: string }> = {
  wake: { status: "Wake sent", word: "woken" },
  reassign: { status: "Work reassigned", word: "reassigned" },
  stop: { status: "Agent stopped", word: "stopped" },
};

const askedTo = (waitsOn: string) => (waitsOn === OWNER_ADDRESS ? "you" : waitsOn);

interface Props {
  runtime: Runtime;
  executionId: string | null;
  journeys: JourneyFlow[] | null;
  /** Opens the team-run composer; null when a run is already open or the Studio cannot start one. */
  onStart?: (() => void) | null;
}

/** The Lanes region (docs/design/studio/Lanes.dc.html): the filters, one card per agent in its
 * column, and the selected agent's panel with its questions, the reply box and its last actions. */
export function LanesView({ runtime, executionId, journeys, onStart = null }: Props) {
  // #1934: the agent panel folds to a 40px rail; the choice is kept per viewer.
  const [collapsed, setCollapsed] = useState(readCollapsed);
  const toggle = () => setCollapsed((prior) => { writeCollapsed(!prior); return !prior; });
  const [board, setBoard] = useState<TasksBoard | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [picked, setPicked] = useState<string | null>(null);
  const [filter, setFilter] = useState<Filter>("all");
  const [deliveredOpen, setDeliveredOpen] = useState<boolean | null>(null);
  const [olderShown, setOlderShown] = useState(false);
  // #1901: the owner's requests (WHAT I ASKED) and the one picked to narrow the board, if any.
  const [requests, setRequests] = useState<OwnerRequest[] | null>(null);
  const [request, setRequest] = useState<string | null>(null);
  const [actions, setActions] = useState<Record<string, { at: string | null; rows: LaneAction[] }>>({});
  const seen = useRef<Record<string, string | null>>({});
  const replyBox = useRef<HTMLTextAreaElement>(null);
  // #1837: the request each lane has in flight or that last failed, kept across panel switches so
  // a resend reuses its id and bytes.
  const drafts = useRef(new Map<string, Draft>());
  // The agent the panel last showed: a lane that only asked leaves the board once answered, and
  // the panel stays on it so the owner sees the reply land (#1808).
  const shownAgent = useRef<Agent | null>(null);
  // The controls this Studio sent, so the board shows what each one did.
  const [done, setDone] = useState<{ lane: string; word: string; at: string }[]>([]);

  const read = useCallback(async () => {
    if (executionId === null) return;
    // An older Runtime without the requests route leaves the column empty, never the board.
    runtime.requests(executionId).then(setRequests, () => setRequests((prior) => prior ?? []));
    try {
      const next = await runtime.board(executionId);
      setBoard(next);
      setError(null);
      // Each lane's log is read again only when its last record moved.
      for (const agent of agents(next)) {
        if (agent.lane in seen.current && seen.current[agent.lane] === agent.lastRecordAt) continue;
        seen.current[agent.lane] = agent.lastRecordAt;
        runtime.laneActivity(executionId, agent.lane, LAST_ACTIONS).then(
          (rows) => setActions((prior) => ({ ...prior, [agent.lane]: { at: agent.lastRecordAt, rows } })),
          () => { delete seen.current[agent.lane]; },
        );
      }
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : "The board could not be read.");
    }
  }, [runtime, executionId]);

  // The first card shown stays selected while the board moves under it (a reply takes it out of
  // "wait for a reply" but the panel stays on it).
  const firstShown = board === null ? null : (agents(board).find((agent) => agent.questions.length > 0) ?? agents(board)[0] ?? null)?.lane ?? null;
  useEffect(() => {
    if (picked === null && firstShown !== null) setPicked(firstShown);
  }, [picked, firstShown]);

  useEffect(() => {
    void read();
    const timer = setInterval(() => void read(), POLL_MS);
    return () => clearInterval(timer);
  }, [read]);

  if (executionId === null) return <section id="panel-lanes" aria-label="Lanes" className="view"><Empty title="No work yet" hint="Lanes appear here once a team picks up a task." onStart={onStart} /></section>;
  if (board === null) return <section id="panel-lanes" aria-label="Lanes" className="view">{error === null ? <Loading text="Reading the lanes…" /> : <ReadFailure what="lanes list" route={`/v1/executions/${executionId}/tasks`} message={error} onRetry={() => void read()} />}</section>;

  const all = agents(board);
  const asking = all.filter((agent) => agent.questions.length > 0);
  const idle = all.filter((agent) => agent.idleMinutes === null || agent.idleMinutes >= 30);
  const working = all.filter((agent) => agent.task !== null && agent.questions.length === 0 && agent.idleMinutes !== null && agent.idleMinutes < 30);
  const pickedRequest = requests?.find((row) => row.requestId === request) ?? null;
  const forRequest = pickedRequest === null ? null : new Set(agentsFor(pickedRequest, all, board.tasks).map((agent) => agent.lane));
  const shown = (filter === "reply" ? asking : filter === "idle" ? idle : all).filter((agent) => forRequest === null || forRequest.has(agent.lane));
  const asked = askedCards(requests ?? [], all, board.tasks, journeys ?? [], new Set(board.stalls.map((stall) => stall.key)), new Date(Date.parse(board.now) || Date.now()));
  const arrangedAsked = arrangeAsked(asked, Date.parse(board.now) || Date.now());
  const deliveredShown = deliveredOpen ?? arrangedAsked.open.length === 0;
  const shownAsked = [...arrangedAsked.open, ...(deliveredShown ? (olderShown ? arrangedAsked.delivered : arrangedAsked.recent) : [])];
  const olderAsked = arrangedAsked.delivered.length - arrangedAsked.recent.length;
  const serves = requestTitles(requests ?? [], all, board.tasks);
  const stripes = requestColors(requests ?? [], all, board.tasks);
  const gone = picked !== null && shownAgent.current?.lane === picked ? { ...shownAgent.current, questions: [], moreQuestions: 0 } : null;
  const selected = all.find((agent) => agent.lane === picked) ?? gone ?? asking[0] ?? all[0] ?? null;
  shownAgent.current = selected;
  const pick = (lane: string, focus = false) => {
    setPicked(lane);
    if (focus) requestAnimationFrame(() => replyBox.current?.focus());
  };

  return (
    <section id="panel-lanes" aria-label="Lanes" className="lanes">
      <div className="lanes-main">
        <div className="lane-filters" role="group" aria-label="Filter agents">
          <button type="button" className="b filter" aria-pressed={filter === "all"} onClick={() => setFilter("all")}>{all.length} {all.length === 1 ? "agent" : "agents"}</button>
          <button type="button" className="b filter reply" aria-pressed={filter === "reply"} onClick={() => setFilter("reply")}>{asking.length} wait for a reply</button>
          <button type="button" className="b filter idle" aria-pressed={filter === "idle"} onClick={() => setFilter("idle")}>{idle.length} idle over 30 min</button>
          <span className="filter working">{working.length} working</span>
          {error !== null && <span className="error">{error}</span>}
        </div>
        <ul className="board with-asked" aria-label="Agent board">
          <li className="board-column asked">
            <section aria-label="What I asked">
              <h3><span className="col-dot asked" />What I asked<span className="mono count">{requests === null ? "…" : asked.length}</span></h3>
              {requests !== null && asked.length === 0 && <span className="none">No request recorded yet.</span>}
              <ul aria-label="Owner requests">
                {shownAsked.map((card) => (
                  <li key={card.requestId}>
                    <button type="button" style={{ "--rq": card.color } as CSSProperties} className="b asked-card rq" aria-pressed={card.requestId === request} onClick={() => setRequest((prior) => (prior === card.requestId ? null : card.requestId))}>
                      {card.when !== null && <span className="mono asked-when">{card.when}</span>}
                      <span className="asked-title">{card.title}</span>
                      <RequestProgress progress={card.progress} />
                      {card.agents.length > 0 && <span className="blocks" aria-hidden="true">{card.agents.map((agent) => <span key={agent.lane} className={`block asked-${agent.tone}${agent.tone === "on" ? " curstep" : ""}`} />)}</span>}
                      <span className="mono asked-summary">{card.summary}</span>
                      {card.agents.length > 0 && (
                        <span className="asked-agents">{card.agents.map((agent) => <span key={agent.lane} className="mono asked-chip"><span className={`asked-dot asked-${agent.tone}`} />{agent.lane}</span>)}</span>
                      )}
                    </button>
                  </li>
                ))}
              </ul>
              {arrangedAsked.delivered.length > 0 && (
                <div className="asked-delivered">
                  <button type="button" className="b asked-delivered-row" aria-expanded={deliveredShown} onClick={() => setDeliveredOpen(!deliveredShown)}>Delivered ({arrangedAsked.delivered.length})</button>
                  {deliveredShown && olderAsked > 0 && !olderShown && <button type="button" className="b asked-delivered-row" onClick={() => setOlderShown(true)}>show older ({olderAsked})</button>}
                </div>
              )}
            </section>
          </li>
          {COLUMNS.map((name) => {
            const cards = shown.filter((agent) => agent.column === name);
            return (
              <BoardColumn key={name} name={name} label={COLUMN_NAME[name]} count={cards.length}>
                <ul aria-label={COLUMN_NAME[name]}>
                  {cards.map((agent) => <Card key={agent.lane} agent={agent} selected={agent.lane === selected?.lane} serves={serves.get(agent.lane) ?? null} stripe={stripes.get(agent.lane) ?? null} unlinked={unlinkedIssue(requests, agent.task?.issue)} last={actions[agent.lane]?.rows[0]} onPick={pick} />)}
                </ul>
              </BoardColumn>
            );
          })}
          {done.length > 0 && (
            <li className="board-column controls" aria-label="Recent controls">
              <h3>Recent controls</h3>
              <ul>{done.slice(-5).reverse().map((row, index) => <li key={index} className="mono"><When at={row.at} /> {row.lane} {row.word}</li>)}</ul>
            </li>
          )}
        </ul>
        <LanesPanels runtime={runtime} executionId={executionId} board={board} requests={requests ?? []} actions={actions} onAnswer={(lane) => pick(lane, true)} onSent={read} />
      </div>
      {selected !== null && collapsed && (
        <aside aria-label="Agent" className="agent-rail">
          <button type="button" className="b rail-toggle" aria-label="Open agent panel" aria-expanded="false" onClick={toggle}><svg aria-hidden="true" width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"><path d="M10 4l-4 4 4 4" /></svg></button>
          <span className={`agent-dot ${selected.questions.length > 0 ? "asks" : idleTone(selected.idleMinutes) === "late" ? "quiet" : "on"}`} title={selected.lane} />
        </aside>
      )}
      {selected !== null && !collapsed && (
        <AgentPanel
          onCollapse={toggle}
          key={selected.lane}
          runtime={runtime}
          executionId={executionId}
          agent={selected}
          actions={actions[selected.lane]?.rows ?? null}
          journeys={journeys}
          replyBox={replyBox}
          drafts={drafts.current}
          lanes={all.map((agent) => agent.lane)}
          onSent={read}
          onControl={(lane, control) => setDone((prior) => [...prior.slice(-19), { lane, word: DONE[control].word, at: new Date().toISOString() }])}
        />
      )}
    </section>
  );
}

function Card({ agent, selected, serves, stripe, unlinked, last, onPick }: { agent: Agent; selected: boolean; serves: string | null; stripe: string | null; unlinked: boolean; last: LaneAction | undefined; onPick: (lane: string, focus?: boolean) => void }) {
  const task = agent.task;
  const tone = idleTone(agent.idleMinutes);
  const asking = agent.questions[0];
  const aged = stepAged(agent.stepMinutes);
  return (
    <li style={stripe === null ? undefined : ({ "--rq": stripe } as CSSProperties)} className={`agent-card${stripe === null ? "" : " rq"}${selected ? " selected" : ""}${asking ? " asking" : ""}${aged ? " aged" : ""}`}>
      <button type="button" className="b agent-name" aria-pressed={selected} onClick={() => onPick(agent.lane)}>
        <span className={`agent-dot ${asking ? "asks" : tone === "late" ? "quiet" : "on"}`} />
        <strong>{agent.lane}</strong>
      </button>
      {(agent.kind !== null || agent.model !== null) && (
        <span role="group" aria-label="Agent kind" className="mono agent-kind">{[agent.kind, agent.model].filter((part) => part !== null).join(" · ")}</span>
      )}
      <span role="group" aria-label="Idle time" className={`mono idle ${tone}`}>{idleText(agent.idleMinutes)}</span>
      {task !== null && agent.stepMinutes !== null && (
        <span role="group" aria-label="Time in step" className={`mono step-age${aged ? " aged" : ""}`}>{task.step} {idleText(agent.stepMinutes)}</span>
      )}
      <span role="group" aria-label="Request" className="request">
        {task === null ? "no task" : <><span className="mono">#{task.issue ?? "—"}</span> {task.prTitle ?? task.title ?? task.key}{task.pr !== null && <span className="mono"> · PR #{task.pr}</span>}</>}
      </span>
      {serves !== null && <span role="group" aria-label="For request" className="serves">for: {serves}</span>}
      {unlinked && <span role="group" aria-label="Not linked" className="serves unlinked">not linked to an ask</span>}
      <span role="group" aria-label="Progress" className="blocks">
        {blocks(task?.step ?? null, agent.column === "blocked").map((block, index) => <span key={index} className={`block ${block}`} />)}
      </span>
      <span role="group" aria-label="Last action" className="mono last-action">{last ? <><When at={last.occurredAt} /> {actionText(last)}</> : agent.lastRecordAt ? <><When at={agent.lastRecordAt} /> last record</> : "no record yet"}</span>
      {asking && (
        <section aria-label="Question" className="question">
          <span className="mono">ASKS {askedTo(asking.waitsOn)} · <When at={asking.since} /></span>
          <span>{asking.question}</span>
          {agent.questions.length + agent.moreQuestions > 1 && <span className="mono more">+{agent.questions.length - 1 + agent.moreQuestions} more</span>}
          <button type="button" className="b ghost small" onClick={() => onPick(agent.lane, true)}>Reply</button>
        </section>
      )}
    </li>
  );
}

interface PanelProps {
  runtime: Runtime;
  executionId: string;
  agent: Agent;
  actions: LaneAction[] | null;
  journeys: JourneyFlow[] | null;
  replyBox: RefObject<HTMLTextAreaElement | null>;
  drafts: Map<string, Draft>;
  lanes: string[];
  onSent: () => Promise<void>;
  onControl: (lane: string, control: LaneControl) => void;
  onCollapse: () => void;
}

function AgentPanel({ runtime, executionId, agent, actions, journeys, replyBox, drafts, lanes, onSent, onControl, onCollapse }: PanelProps) {
  const [text, setText] = useState("");
  const [sending, setSending] = useState<string | null>(null);
  const others = lanes.filter((lane) => lane !== agent.lane);
  const [target, setTarget] = useState<string>(others[0] ?? "");
  const [confirmStop, setConfirmStop] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  // The question a reply answers: the lane's oldest open one shown.
  const answering = agent.questions[0] ?? null;
  const task = agent.task;

  // The slice Reassign hands over: the lane's own newest open slice, not one it only reviews.
  const slice = task !== null && task.lane === agent.lane ? task : null;
  const to = others.includes(target) ? target : (others[0] ?? "");

  /** Sends the pending draft for `slot`, or a new one when the intent changed. The draft is
   * dropped only once the Runtime accepted it, so a failed request is resent unchanged (#1837). */
  const submit = async (slot: string, intent: string, make: () => { id: string; body: Record<string, unknown> }, post: (draft: Draft) => Promise<void>, done: string) => {
    if (sending !== null) return false;
    const key = `${slot}:${agent.lane}`;
    const draft = draftFor(drafts.get(key), intent, make);
    drafts.set(key, draft);
    setSending(slot);
    setFailure(null);
    setStatus(null);
    try {
      await post(draft);
      drafts.delete(key);
      setStatus(done);
      return true;
    } catch (error) {
      setFailure(error instanceof Error ? error.message : "The Runtime did not record it.");
      return false;
    } finally {
      setSending(null);
    }
  };

  const send = async () => {
    const body = text.trim();
    if (body === "") return;
    const replyTo = answering?.signalId ?? null;
    const sent = await submit("message", JSON.stringify([body, replyTo]), () => {
      const id = newId("studio-reply");
      return { id, body: messageBody(executionId, id, agent.lane, body, replyTo, new Date(), OWNER.id) };
    }, (draft) => runtime.messageLane(executionId, draft), answering ? "Reply sent" : "Message sent");
    if (sent) {
      setText("");
      await onSent();
    }
  };

  const control = async (which: LaneControl) => {
    setConfirmStop(false);
    const reassign = which === "reassign" && slice !== null ? { sliceKey: slice.key, to } : undefined;
    const reason = which === "wake" ? "The owner woke this lane from the Studio."
      : which === "stop" ? "The owner stopped this lane from the Studio."
      : `The owner reassigned ${reassign?.sliceKey ?? "its slice"} to ${to} from the Studio.`;
    const sent = await submit(which, JSON.stringify([which, reassign ?? null]), () => ({
      id: newId(`studio-${which}`), body: controlBody(which, reason, new Date(), reassign),
    }), (draft) => runtime.controlLane(executionId, agent.lane, which, draft), DONE[which].status);
    if (sent) {
      onControl(agent.lane, which);
      await onSent();
    }
  };

  const flows = new Map((journeys ?? []).map((flow) => [flow.id, flow]));
  return (
    <aside aria-label="Agent" className="agent-panel">
      <div className="panel-head"><span className={`agent-dot ${answering ? "asks" : idleTone(agent.idleMinutes) === "late" ? "quiet" : "on"}`} /><strong>{agent.lane}</strong><button type="button" className="b rail-toggle" aria-label="Collapse agent panel" aria-expanded="true" onClick={onCollapse}><svg aria-hidden="true" width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"><path d="M6 4l4 4-4 4" /></svg></button></div>
      <span className="muted">{task === null ? "no task" : <><span className="mono">#{task.issue ?? "—"}</span> {task.prTitle ?? task.title ?? task.key}</>} · {COLUMN_NAME[agent.column].toLowerCase()}</span>
      <span className={`mono idle ${idleTone(agent.idleMinutes)}`}>{agent.idleMinutes === null ? "no record yet" : `last record ${idleText(agent.idleMinutes)} ago`}</span>
      {task !== null && task.journeys.length > 0 && (
        <div className="panel-journeys">
          <span className="section-label">JOURNEYS</span>
          {task.journeys.map((id) => {
            const last = flows.get(id)?.lastRun;
            return <span key={id} className="mono">{id} · {last === undefined ? "unknown" : last.status.replace("_", " ")}</span>;
          })}
        </div>
      )}
      <span className="section-label">MESSAGES</span>
      {agent.questions.length === 0 ? <span className="muted">No open question.</span> : (
        <ul className="thread">
          {agent.questions.map((row) => (
            <li key={row.signalId}><span className="mono">{agent.lane} → {askedTo(row.waitsOn)} · <When at={row.since} /></span><span>{row.question}</span></li>
          ))}
          {agent.moreQuestions > 0 && <li className="mono more">+{agent.moreQuestions} more open questions</li>}
        </ul>
      )}
      <label className="reply">
        <span className="section-label">{answering ? `REPLY TO ${agent.lane}` : `MESSAGE ${agent.lane}`}</span>
        <textarea ref={replyBox} aria-label="Reply" rows={3} value={text} placeholder="Answer, or give the next task" onChange={(event) => setText(event.target.value)} />
      </label>
      <div className="panel-actions">
        <button type="button" className="b act" disabled={sending !== null || text.trim() === ""} onClick={() => void send()}>{answering ? "Send reply" : "Send"}</button>
        <button type="button" className="b ghost" disabled={sending !== null} onClick={() => void control("wake")}>Wake</button>
        <button type="button" className="b ghost" disabled={sending !== null || slice === null || to === ""} onClick={() => void control("reassign")}>Reassign</button>
        {confirmStop
          ? <>
              <button type="button" className="b ghost stop" disabled={sending !== null} onClick={() => void control("stop")}>Confirm stop</button>
              <button type="button" className="b ghost" onClick={() => setConfirmStop(false)}>Keep running</button>
            </>
          : <button type="button" className="b ghost stop" disabled={sending !== null} onClick={() => setConfirmStop(true)}>Stop</button>}
      </div>
      {slice !== null && others.length > 0 && (
        <label className="reassign-to">
          <span className="section-label">REASSIGN #{slice.issue ?? slice.key} TO</span>
          <select aria-label="Reassign to" value={to} onChange={(event) => setTarget(event.target.value)}>
            {others.map((lane) => <option key={lane} value={lane}>{lane}</option>)}
          </select>
        </label>
      )}
      {slice === null && <span className="muted small">Reassign needs a slice this agent holds.</span>}
      {confirmStop && <span className="muted small">Stop retires {agent.lane}: it stops counting as live until it is restored.</span>}
      {status !== null && <span role="status" aria-label={status} className="sent">{status}</span>}
      {failure !== null && <span role="alert" className="error">{failure}</span>}
      <span className="section-label">LAST ACTIONS</span>
      {actions === null ? <span className="muted">Reading…</span> : actions.length === 0 ? <span className="muted">No action recorded.</span> : (
        <ul className="last-actions">{actions.map((action) => <li key={action.sequence} className="mono"><When at={action.occurredAt} /> {actionText(action)}</li>)}</ul>
      )}
    </aside>
  );
}
