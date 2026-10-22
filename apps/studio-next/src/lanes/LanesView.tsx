import { useCallback, useEffect, useRef, useState, type RefObject } from "react";

import type { JourneyFlow, LaneAction, Runtime, TasksBoard } from "../runtime/client";
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
          onSent={read}
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
  onSent: () => Promise<void>;
}

function AgentPanel({ runtime, executionId, agent, actions, journeys, replyBox, onSent }: PanelProps) {
  const [text, setText] = useState("");
  const [sending, setSending] = useState<{ id: string } | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  // The question a reply answers: the lane's oldest open one shown.
  const answering = agent.questions[0] ?? null;
  const task = agent.task;

  const send = async () => {
    const body = text.trim();
    if (body === "" || sending !== null) return;
    // One id per message: a retry of the same message reuses it and the Runtime reconciles it.
    const id = `studio-reply-${crypto.randomUUID()}`;
    setSending({ id });
    setFailure(null);
    setStatus(null);
    try {
      await runtime.messageLane(executionId, id, agent.lane, body, answering?.signalId ?? null);
      setText("");
      setStatus(answering ? "Reply sent" : "Message sent");
      await onSent();
    } catch (error) {
      setFailure(error instanceof Error ? error.message : "The reply was not recorded.");
    } finally {
      setSending(null);
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
        <button type="button" className="b ghost" disabled>Wake</button>
        <button type="button" className="b ghost" disabled>Reassign</button>
        <button type="button" className="b ghost stop" disabled>Stop</button>
      </div>
      <span className="muted small">Wake, Reassign and Stop are available once lane controls land (#1847).</span>
      {status !== null && <span role="status" aria-label={status} className="sent">{status}</span>}
      {failure !== null && <span role="alert" className="error">{failure}</span>}
      <span className="section-label">LAST ACTIONS</span>
      {actions === null ? <span className="muted">Reading…</span> : actions.length === 0 ? <span className="muted">No action recorded.</span> : (
        <ul className="last-actions">{actions.map((action) => <li key={action.sequence} className="mono">{actionLine(action)}</li>)}</ul>
      )}
    </aside>
  );
}
