import { useEffect, useRef, useState, type ReactNode } from "react";

import { duration, pace, typicalStep, type TimedStep } from "../runtime/step-timing";
import type { TaskState } from "../runtime/team-tasks";

/* #391 (journey-first spec §7, Rule 5): one small graph per task, folded from the `task.*` records
 * alone. GitHub is linked, never polled: the view works offline and the records are the audit
 * trail. */


/** A recorded URL becomes a link only when it points at github.com: the records are written by
 * lanes, and a `javascript:` or look-alike URL must not become clickable in the owner's view. */
const GITHUB_URL = /^https:\/\/github\.com\/[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+\/(pull|issues)\/\d+(#[A-Za-z0-9_-]*)?$/;

export interface TaskGraphsProps {
  tasks: TaskState[];
  onOpenJourney: (contractId: string) => void;
  /** The clock the timers read (#502); tests pass a fixed one. Default: now, ticking every 30 s. */
  now?: number;
}

/** #502: what a node needs to show its time: the current clock, and the run's typical time per step
 * (median over the delivered slices; `null` below three samples) with how many samples it has. */
interface Timing {
  now: number;
  typical: Record<TimedStep, { ms: number; samples: number } | null>;
  samples: Record<TimedStep, number>;
}

function useNow(fixed: number | undefined): number {
  const [now, setNow] = useState(() => fixed ?? Date.now());
  useEffect(() => {
    if (fixed !== undefined) return;
    const timer = window.setInterval(() => setNow(Date.now()), 30_000);
    return () => window.clearInterval(timer);
  }, [fixed]);
  return fixed ?? now;
}

function title(task: TaskState): string {
  const name = task.issue !== null ? `Issue #${task.issue}` : task.pr !== null ? `PR #${task.pr}` : task.taskId;
  return task.title !== null ? `${name} · ${task.title}` : name;
}

/** #477: a PR title reads as what changes for the user; its conventional `type(area):` prefix is
 * for the squash commit, not for the owner. */
function plainTitle(text: string): string {
  return text.replace(/^[a-z]+(\([^)]*\))?!?:\s*/, "");
}

/** #477 (owner): what needs a person first. Blocked, then in review (review or merge), then
 * implementing; inside each, the newest activity first. Delivered tasks go last, collapsed. */
function rank(task: TaskState): number {
  return task.blockedBy !== null ? 0 : task.step === "review" || task.step === "merge" ? 1 : 2;
}
function ordered(tasks: TaskState[]): { open: TaskState[]; delivered: TaskState[] } {
  const newest = (a: TaskState, b: TaskState) => b.lastSequence - a.lastSequence;
  return {
    open: tasks.filter((task) => task.step !== "merged").sort((a, b) => rank(a) - rank(b) || newest(a, b)),
    delivered: tasks.filter((task) => task.step === "merged").sort(newest),
  };
}

/** #514 (owner): one card per issue. Each PR of the issue is a row; a task claimed with a `parent`
 * is a row in the parent issue's card when that card exists, else its own card. */
interface Card { issue: number | null; key: string; rows: TaskState[] }
function cardsOf(tasks: TaskState[]): Card[] {
  // A card's home follows the parent chain while the parent has rows here (a grandchild sits with
  // its root, never in a card its parent's row left), stopping at a cycle (#524 review).
  const parentOf = new Map<number, number>();
  for (const task of tasks) if (task.issue !== null && task.parent !== null) parentOf.set(task.issue, task.parent);
  const issues = new Set(tasks.map((task) => task.issue).filter((issue): issue is number => issue !== null));
  const root = (issue: number | null): number | null => {
    const seen = new Set<number>();
    let at = issue;
    while (at !== null && !seen.has(at)) {
      seen.add(at);
      const up = parentOf.get(at);
      if (up === undefined || !issues.has(up) || seen.has(up)) return at;
      at = up;
    }
    return at;
  };
  const cards = new Map<string, Card>();
  for (const task of tasks) {
    const home = task.parent !== null && issues.has(task.parent) ? root(task.parent) : task.issue;
    const key = home !== null ? `issue-${home}` : task.key;
    const card = cards.get(key) ?? { issue: home, key, rows: [] };
    card.rows.push(task);
    cards.set(key, card);
  }
  return [...cards.values()];
}
/** A card sits where its worst open row would; it is delivered only when every row is merged. */
function orderedCards(tasks: TaskState[]): { open: Card[]; delivered: Card[] } {
  const all = cardsOf(tasks).map((card) => ({ card, ...ordered(card.rows) }));
  const latest = (rows: TaskState[]) => Math.max(...rows.map((row) => row.lastSequence));
  const open = all.filter((entry) => entry.open.length > 0)
    .sort((a, b) => rank(a.open[0]) - rank(b.open[0]) || latest(b.card.rows) - latest(a.card.rows));
  const delivered = all.filter((entry) => entry.open.length === 0).sort((a, b) => latest(b.card.rows) - latest(a.card.rows));
  const rowsInOrder = (entry: (typeof all)[number]): Card => ({ ...entry.card, rows: [...entry.open, ...entry.delivered] });
  return { open: open.map(rowsInOrder), delivered: delivered.map(rowsInOrder) };
}

/** #477: the rows whose last record moved since the previous render, marked for a few seconds so
 * the owner sees what just changed. The first render marks nothing. */
function useChanged(tasks: TaskState[]): Set<string> {
  const seen = useRef<Map<string, number> | null>(null);
  const [changed, setChanged] = useState<Set<string>>(() => new Set());
  const now = new Map(tasks.map((task) => [task.key, task.lastSequence]));
  const fresh = seen.current === null ? [] : tasks.filter((task) => seen.current!.get(task.key) !== task.lastSequence).map((task) => task.key);
  const signature = fresh.join("\u0000");
  useEffect(() => {
    seen.current = now;
    if (fresh.length === 0) return;
    setChanged(new Set(fresh));
    const timer = setTimeout(() => setChanged(new Set()), 4000);
    return () => clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [signature]);
  if (seen.current === null) seen.current = now;
  return fresh.length > 0 ? new Set(fresh) : changed;
}

type NodeState = "done" | "current" | "next" | "blocked";
interface StepNode {
  key: string; label: string; who: ReactNode; state: NodeState; reason: string | null; review: boolean;
  /** #502: the step whose typical time this node is measured against (Fix and Re-review: none), and
   * when the node was entered (the Runtime's append time of the record that lit it). */
  timed: TimedStep | null;
  since: string | null;
}

/** #514 (owner: "a node for it: re-review and the agent working"): the row grows one Fix and one
 * Re-review per BLOCK round, so the story of the loop shows. A Review blocked is marked ✗ with its
 * reason linked; Fix is the author's, lit until a newer head is recorded; Re-review is lit on that
 * head. All from the records. */
function nodesOf(task: TaskState): StepNode[] {
  const merged = task.step === "merged";
  // #458: the merge sha short, as everywhere else in the Studio, linked when the repository is known.
  const sha = task.mergeSha === null ? null : task.repoUrl !== null && /^[0-9a-f]{7,64}$/.test(task.mergeSha)
    ? <a href={`${task.repoUrl}/commit/${task.mergeSha}`} target="_blank" rel="noreferrer">{task.mergeSha.slice(0, 8)}</a>
    : task.mergeSha.slice(0, 8);
  const reviewers = task.reviewers.length > 0 ? task.reviewers.join(", ") : null;
  const rounds = task.rounds;
  const nodes: StepNode[] = [
    { key: "implement", label: "Implement", who: task.lane, state: task.step === "implement" ? "current" : "done", reason: null, review: false,
      timed: "implement", since: task.clock.since },
    {
      key: "review", label: "Review", who: rounds.length > 0 ? rounds[0].reviewer : reviewers, review: true,
      state: rounds.length > 0 ? "blocked" : task.step === "review" ? "current" : task.step === "implement" ? "next" : "done",
      reason: rounds[0]?.commentUrl ?? null, timed: "review", since: task.clock.since,
    },
  ];
  rounds.forEach((round, index) => {
    const next = rounds[index + 1];
    const last = next === undefined;
    const tag = ` · round ${index + 1}`;
    nodes.push({
      key: `fix-${index}`, label: `Fix${tag}`, who: task.lane, review: false, reason: null, timed: null, since: round.blockedAt,
      state: round.fixHead !== null ? "done" : last && !merged ? "current" : "done",
    });
    nodes.push({
      key: `rereview-${index}`, label: `Re-review${tag}`, review: true,
      who: next !== undefined ? next.reviewer : reviewers ?? round.reviewer,
      state: next !== undefined ? "blocked" : round.fixHead === null ? "next" : task.step === "review" ? "current" : "done",
      reason: next?.commentUrl ?? null, timed: null, since: round.fixedAt,
    });
  });
  nodes.push({ key: "merge", label: "Merge", who: sha, state: merged ? "done" : task.step === "merge" ? "current" : "next", reason: null, review: false,
    timed: "merge", since: task.clock.since });
  return nodes;
}

/** #502: the lit node's time and, for a step with a typical time, a bar against it; a step the
 * slice left shows what it spent there. */
function StepTime({ task, node, timing }: { task: TaskState; node: StepNode; timing: Timing }) {
  if (node.state === "done") {
    const spent = node.timed === null ? undefined : task.clock.spent[node.timed];
    return spent === undefined ? null : <span className="task-node-time">{duration(spent)}</span>;
  }
  if (node.state !== "current" || node.since === null) return null;
  const elapsed = Math.max(0, timing.now - Date.parse(node.since));
  const typical = node.timed === null ? null : timing.typical[node.timed];
  return (
    <>
      <span className="task-node-time">in this step: {duration(elapsed)}</span>
      {node.timed !== null && (typical === null
        ? <span className="task-node-pace-none">no typical time yet ({timing.samples[node.timed]} of 3 past tasks)</span>
        : (
          <span className="task-node-bar" role="meter" aria-label={`time in ${node.timed}`} aria-valuemin={0}
            aria-valuemax={Math.round(2 * typical.ms)} aria-valuenow={Math.round(elapsed)}
            data-pace={pace(elapsed, typical.ms)} title={`typical ${duration(typical.ms)} (median of ${typical.samples})`}>
            <span className="task-node-bar-fill" style={{ width: `${Math.min(100, (100 * elapsed) / (2 * typical.ms))}%` }} />
          </span>
        ))}
    </>
  );
}

function Node({ task, node, timing }: { task: TaskState; node: StepNode; timing: Timing }) {
  return (
    <li className={`task-node task-node-${node.state}`} aria-current={node.state === "current" ? "step" : undefined}>
      <span className="task-node-label">{node.label}{node.state === "blocked" && <span className="task-node-cross" aria-label="blocked"> ✗</span>}</span>
      {node.who !== null && <span className="task-node-agent">{node.who}</span>}
      {node.state === "blocked" && node.reason !== null && GITHUB_URL.test(node.reason) && (
        <a className="task-node-reason" href={node.reason} target="_blank" rel="noreferrer">reason</a>
      )}
      {/* #508 (owner): a review in progress with nobody named means a record is missing; say so. */}
      {node.who === null && node.review && node.state === "current" && <span className="task-node-missing">no reviewer recorded</span>}
      <StepTime task={task} node={node} timing={timing} />
    </li>
  );
}

function Graph({ task, changed, onOpenJourney, timing }: { task: TaskState; changed: boolean; onOpenJourney: (contractId: string) => void; timing: Timing }) {
  return (
    <div className={changed ? "task-graph task-graph-changed" : "task-graph"} role="group" aria-label={title(task)}>
      <div className="task-graph-head">
        {task.issue !== null && (task.repoUrl !== null
          ? <a href={`${task.repoUrl}/issues/${task.issue}`} target="_blank" rel="noreferrer">#{task.issue}</a>
          : <strong>#{task.issue}</strong>)}
        {task.issue === null && <strong>{title(task)}</strong>}
        {task.title !== null && <span className="task-graph-title" title={task.title}>{task.title}</span>}
        {task.step === "merged" && <span className="task-graph-merged">merged</span>}
      </div>
      {task.summary !== null && <p className="task-graph-summary" title={task.summary}>{task.summary}</p>}
      {task.parent !== null && task.parent !== task.issue && <p className="task-graph-parent">found while working on #{task.parent}</p>}
      {task.pr !== null && (
        <p className="task-graph-pr">
          {task.repoUrl !== null
            ? <a href={`${task.repoUrl}/pull/${task.pr}`} target="_blank" rel="noreferrer">PR #{task.pr}</a>
            : <span>PR #{task.pr}</span>}
          {task.prTitle !== null && <span className="task-graph-title" title={task.prTitle}>{plainTitle(task.prTitle)}</span>}
          {task.journeys.map((journey) => (
            <a key={journey} href={`#journey/${encodeURIComponent(journey)}`} className="task-graph-journey"
              onClick={(event) => { event.preventDefault(); onOpenJourney(journey); }}>{journey}</a>
          ))}
        </p>
      )}
      <ol className="task-graph-steps">
        {nodesOf(task).map((node) => <Node key={node.key} task={task} node={node} timing={timing} />)}
      </ol>
      {task.critic !== null && (
        <p className="task-graph-summary">
          {task.critic.verdict === "pass" ? `design approved in round ${task.critic.round}, ${task.critic.score}/10`
            : task.critic.verdict === "revise" ? `design in revision: round ${task.critic.round} of ${task.critic.maxRounds}, ${task.critic.score}/10`
            : `design not approved after ${task.critic.maxRounds} rounds, ${task.critic.score}/10: needs a person`}
        </p>
      )}
      {task.blockedBy !== null && (GITHUB_URL.test(task.blockedBy.commentUrl)
        ? <a className="task-graph-blocked" href={task.blockedBy.commentUrl} target="_blank" rel="noreferrer">
            blocked by {task.blockedBy.reviewer} at {task.blockedBy.headSha.slice(0, 8)}
          </a>
        : <p className="task-graph-blocked">blocked by {task.blockedBy.reviewer} at {task.blockedBy.headSha.slice(0, 8)}</p>)}
      {task.strayVerdicts.length > 0 && (
        <details className="task-graph-details">
          <summary>Details</summary>
          {task.strayVerdicts.map((stray, index) => (
            <p key={index} className="task-graph-stray">
              {stray.verdict} by {stray.reviewer} on {stray.headSha.slice(0, 8)}, {stray.reason === "superseded"
                ? `superseded by ${stray.supersededBy.slice(0, 8)}` : "a head with no pr_opened record"}
            </p>
          ))}
        </details>
      )}
    </div>
  );
}

function CardView({ card, changed, onOpenJourney, timing }: { card: Card; changed: Set<string>; onOpenJourney: (contractId: string) => void; timing: Timing }) {
  const lead = card.rows.find((row) => row.issue === card.issue && row.title !== null) ?? card.rows.find((row) => row.issue === card.issue) ?? card.rows[0];
  const label = card.issue !== null ? `Issue #${card.issue}${lead.issue === card.issue && lead.title !== null ? ` · ${lead.title}` : ""}` : title(lead);
  return (
    <article className="task-card" aria-label={label}>
      {card.rows.length > 1 && (
        <header className="task-card-head">
          {card.issue !== null && (lead.repoUrl !== null
            ? <a href={`${lead.repoUrl}/issues/${card.issue}`} target="_blank" rel="noreferrer">#{card.issue}</a>
            : <strong>#{card.issue}</strong>)}
          {lead.issue === card.issue && lead.title !== null && <span className="task-graph-title" title={lead.title}>{lead.title}</span>}
          <span className="task-card-count">{card.rows.length} rows</span>
        </header>
      )}
      {card.rows.map((row) => <Graph key={row.key} task={row} changed={changed.has(row.key)} onOpenJourney={onOpenJourney} timing={timing} />)}
    </article>
  );
}

export function TaskGraphs({ tasks, onOpenJourney, now: fixedNow }: TaskGraphsProps) {
  const changed = useChanged(tasks);
  const now = useNow(fixedNow);
  if (tasks.length === 0) return null;
  const { open, delivered } = orderedCards(tasks);
  const clocks = tasks.filter((task) => task.step === "merged").map((task) => task.clock);
  const steps: TimedStep[] = ["implement", "review", "merge"];
  const timing: Timing = {
    now,
    typical: Object.fromEntries(steps.map((step) => [step, typicalStep(clocks, step)])) as Timing["typical"],
    samples: Object.fromEntries(steps.map((step) => [step, clocks.filter((clock) => clock.spent[step] !== undefined).length])) as Timing["samples"],
  };
  return (
    <section className="task-graphs" aria-label="Tasks">
      {open.map((card) => <CardView key={card.key} card={card} changed={changed} onOpenJourney={onOpenJourney} timing={timing} />)}
      {delivered.length > 0 && (
        <details className="task-graphs-delivered">
          <summary>Delivered ({delivered.length})</summary>
          {delivered.map((card) => <CardView key={card.key} card={card} changed={changed} onOpenJourney={onOpenJourney} timing={timing} />)}
        </details>
      )}
    </section>
  );
}
