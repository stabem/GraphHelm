import { useEffect, useRef, useState } from "react";

import type { TaskState, TaskStep } from "../runtime/team-tasks";

/* #391 (journey-first spec §7, Rule 5): one small graph per task, folded from the `task.*` records
 * alone. GitHub is linked, never polled: the view works offline and the records are the audit
 * trail. */

const STEPS: { step: Exclude<TaskStep, "merged">; label: string }[] = [
  { step: "implement", label: "Implement" },
  { step: "review", label: "Review" },
  { step: "merge", label: "Merge" },
];

/** A recorded URL becomes a link only when it points at github.com: the records are written by
 * lanes, and a `javascript:` or look-alike URL must not become clickable in the owner's view. */
const GITHUB_URL = /^https:\/\/github\.com\/[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+\/(pull|issues)\/\d+(#[A-Za-z0-9_-]*)?$/;

export interface TaskGraphsProps {
  tasks: TaskState[];
  onOpenJourney: (contractId: string) => void;
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

function Node({ task, step, label }: { task: TaskState; step: Exclude<TaskStep, "merged">; label: string }) {
  const order = STEPS.findIndex((entry) => entry.step === step);
  const current = STEPS.findIndex((entry) => entry.step === task.step);
  const state = task.step === "merged" || order < current ? "done" : order === current ? "current" : "next";
  // #458: the merge sha short, as everywhere else in the Studio, linked when the repository is known.
  const merge = task.mergeSha === null ? null : task.repoUrl !== null && /^[0-9a-f]{7,64}$/.test(task.mergeSha)
    ? <a href={`${task.repoUrl}/commit/${task.mergeSha}`} target="_blank" rel="noreferrer">{task.mergeSha.slice(0, 8)}</a>
    : task.mergeSha.slice(0, 8);
  const who = step === "implement" ? task.lane
    : step === "review" ? (task.reviewers.length > 0 ? task.reviewers.join(", ") : null)
    : merge;
  return (
    <li className={`task-node task-node-${state}`} aria-current={state === "current" ? "step" : undefined}>
      <span className="task-node-label">{label}</span>
      {who !== null && <span className="task-node-agent">{who}</span>}
    </li>
  );
}

function Graph({ task, changed, onOpenJourney }: { task: TaskState; changed: boolean; onOpenJourney: (contractId: string) => void }) {
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
        {STEPS.map(({ step, label }) => <Node key={step} task={task} step={step} label={label} />)}
      </ol>
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

export function TaskGraphs({ tasks, onOpenJourney }: TaskGraphsProps) {
  const changed = useChanged(tasks);
  if (tasks.length === 0) return null;
  const { open, delivered } = ordered(tasks);
  return (
    <section className="task-graphs" aria-label="Tasks">
      {open.map((task) => <Graph key={task.key} task={task} changed={changed.has(task.key)} onOpenJourney={onOpenJourney} />)}
      {delivered.length > 0 && (
        <details className="task-graphs-delivered">
          <summary>Delivered ({delivered.length})</summary>
          {delivered.map((task) => <Graph key={task.key} task={task} changed={changed.has(task.key)} onOpenJourney={onOpenJourney} />)}
        </details>
      )}
    </section>
  );
}
