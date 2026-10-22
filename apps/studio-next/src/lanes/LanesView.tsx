import { useCallback, useEffect, useRef, useState, type RefObject } from "react";

import { OWNER, type JourneyFlow, type LaneAction, type Runtime, type TasksBoard } from "../runtime/client";
import { controlBody, draftFor, messageBody, type Draft, type LaneControl } from "./drafts";
import { agents, blocks, COLUMNS, idleText, idleTone, OWNER_ADDRESS, type Agent, type Column } from "./board";

/** How often the board is read again: one tasks read, plus one activity read per lane whose last
 * record moved since the previous read. */
const POLL_MS = 5_000;
const LAST_ACTIONS = 10;

const COLUMN_NAME: Record<Column, string> = { backlog: "Backlog", implement: "Implement", review: "Review", blocked: "Blocked", silent: "Silent", merge: "Merge" };
type Filter = "all" | "reply" | "idle";

function clock(at: string | null | undefined): string {
  if (!at) return "--:--";
  const date = new Date(at);
  return Number.isNaN(date.getTime()) ? "--:--" : `${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`;
}

/** One action as the card and the panel print it. */
function actionLine(action: LaneAction): string {
  const pr = action.pr === undefined ? "" : ` PR #${action.pr}`;
  const issue = action.issue === undefined ? "" : ` #${action.issue}`;
  const what: Record<string, string> = {
    "task.claimed": `claimed${issue}`,
    "task.planned": `planned${issue}`,
    "task.released": `released${issue}`,
    "task.heartbeat": action.reason ?? "heartbeat",
    "task.pr_opened": `opened${pr}`,
    "task.review_assigned": `assigned${pr} to ${action.reviewer ?? "a reviewer"}`,
    "task.review_verdict": `${action.verdict ?? "verdict"} on${pr}`,
    "task.review_activity": action.activity ?? `reviewing${pr}`,
    "task.merged": `merged${pr}`,
    "task.closed": `closed${issue}`,
    "task.critic_verdict": `critic verdict${issue}`,
    "task.queued": "queued the wave",
    "task.lane_retired": "retired lanes",
  };
  return `${clock(action.occurredAt)} ${what[action.kind] ?? action.kind}`;
}

/** What a control did, as the status and the board print it. */
const DONE: Record<LaneControl, { status: string; word: string }> = {
  wake: { status: "Wake sent", word: "woken" },
  reassign: { status: "Work reassigned", word: "reassigned" },
  stop: { status: "Agent stopped", word: "stopped" },
};

/** A new idempotency key: `randomUUID` where the browser has it, else 128 random bits. */
function newId(prefix: string): string {
  const uuid = globalThis.crypto?.randomUUID?.();
  if (uuid) return `${prefix}-${uuid}`;
  const bytes = globalThis.crypto.getRandomValues(new Uint8Array(16));
  return `${prefix}-${[...bytes].map((byte) => byte.toString(16).padStart(2, "0")).join("")}`;
}

const askedTo = (waitsOn: string) => (waitsOn === OWNER_ADDRESS ? "you" : waitsOn);

interface Props {
  runtime: Runtime;
  executionId: string | null;
  journeys: JourneyFlow[] | null;
}

/** The Lanes region (docs/design/studio/Lanes.dc.html): the filters, one card per agent in its
 * column, and the selected agent's panel with its questions, the reply box and its last actions. */
export function LanesView({ runtime, executionId, journeys }: Props) {
  const [board, setBoard] = useState<TasksBoard | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [picked, setPicked] = useState<string | null>(null);
  const [filter, setFilter] = useState<Filter>("all");
  const [actions, setActions] = useState<Record<string, { at: string | null; rows: LaneAction[] }>>({});
  const seen = useRef<Record<string, string | null>>({});
  const replyBox = useRef<HTMLTextAreaElement>(null);
  // #1837: the request each lane has in flight or that last failed, kept across panel switches so
  // a resend reuses its id and bytes.
  const drafts = useRef(new Map<string, Draft>());
  // The controls this Studio sent, so the board shows what each one did.
  const [done, setDone] = useState<{ lane: string; word: string; at: string }[]>([]);

  const read = useCallback(async () => {
    if (executionId === null) return;
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

  if (executionId === null) return <section id="panel-lanes" aria-label="Lanes" className="view"><p>Open a team run to see its lanes.</p></section>;
  if (board === null) return <section id="panel-lanes" aria-label="Lanes" className="view"><p className={error ? "error" : undefined}>{error ?? "Reading the lanes…"}</p></section>;

  const all = agents(board);
  const asking = all.filter((agent) => agent.questions.length > 0);
  const idle = all.filter((agent) => agent.idleMinutes === null || agent.idleMinutes >= 30);
  const working = all.filter((agent) => agent.task !== null && agent.questions.length === 0 && agent.idleMinutes !== null && agent.idleMinutes < 30);
  const shown = filter === "reply" ? asking : filter === "idle" ? idle : all;
  const selected = all.find((agent) => agent.lane === picked) ?? asking[0] ?? all[0] ?? null;
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
        <ul className="board" aria-label="Agent board">
          {COLUMNS.map((name) => {
            const cards = shown.filter((agent) => agent.column === name);
            return (
              <li key={name} className={`board-column ${name}`}>
                <h3><span className={`col-dot ${name}`} />{COLUMN_NAME[name]}<span className="mono count">{cards.length}</span></h3>
                {cards.length === 0 ? <span className="none">none</span> : (
                  <ul aria-label={COLUMN_NAME[name]}>
                    {cards.map((agent) => <Card key={agent.lane} agent={agent} selected={agent.lane === selected?.lane} last={actions[agent.lane]?.rows[0]} onPick={pick} />)}
                  </ul>
                )}
              </li>
            );
          })}
          {done.length > 0 && (
            <li className="board-column controls" aria-label="Recent controls">
              <h3>Recent controls</h3>
              <ul>{done.slice(-5).reverse().map((row, index) => <li key={index} className="mono">{clock(row.at)} {row.lane} {row.word}</li>)}</ul>
            </li>
          )}
        </ul>
      </div>
      {selected !== null && (
        <AgentPanel
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

function Card({ agent, selected, last, onPick }: { agent: Agent; selected: boolean; last: LaneAction | undefined; onPick: (lane: string, focus?: boolean) => void }) {
  const task = agent.task;
  const tone = idleTone(agent.idleMinutes);
  const asking = agent.questions[0];
  return (
    <li className={`agent-card${selected ? " selected" : ""}${asking ? " asking" : ""}`}>
      <button type="button" className="b agent-name" aria-pressed={selected} onClick={() => onPick(agent.lane)}>
        <span className={`agent-dot ${asking ? "asks" : tone === "late" ? "quiet" : "on"}`} />
        <strong>{agent.lane}</strong>
      </button>
      {(agent.kind !== null || agent.model !== null) && (
        <span role="group" aria-label="Agent kind" className="mono agent-kind">{[agent.kind, agent.model].filter((part) => part !== null).join(" · ")}</span>
      )}
      <span role="group" aria-label="Idle time" className={`mono idle ${tone}`}>{idleText(agent.idleMinutes)}</span>
      <span role="group" aria-label="Request" className="request">
        {task === null ? "no task" : <><span className="mono">#{task.issue ?? "—"}</span> {task.prTitle ?? task.title ?? task.key}{task.pr !== null && <span className="mono"> · PR #{task.pr}</span>}</>}
      </span>
      <span role="group" aria-label="Progress" className="blocks">
        {blocks(task?.step ?? null, agent.column === "blocked").map((block, index) => <span key={index} className={`block ${block}`} />)}
      </span>
      <span role="group" aria-label="Last action" className="mono last-action">{last ? actionLine(last) : agent.lastRecordAt ? `${clock(agent.lastRecordAt)} last record` : "no record yet"}</span>
      {asking && (
        <section aria-label="Question" className="question">
          <span className="mono">ASKS {askedTo(asking.waitsOn)} · {clock(asking.since)}</span>
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
}

function AgentPanel({ runtime, executionId, agent, actions, journeys, replyBox, drafts, lanes, onSent, onControl }: PanelProps) {
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
      <div className="panel-head"><span className={`agent-dot ${answering ? "asks" : idleTone(agent.idleMinutes) === "late" ? "quiet" : "on"}`} /><strong>{agent.lane}</strong></div>
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
            <li key={row.signalId}><span className="mono">{agent.lane} → {askedTo(row.waitsOn)} · {clock(row.since)}</span><span>{row.question}</span></li>
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
        <ul className="last-actions">{actions.map((action) => <li key={action.sequence} className="mono">{actionLine(action)}</li>)}</ul>
      )}
    </aside>
  );
}
