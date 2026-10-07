import type { TaskState, TaskStep } from "../runtime/team-tasks";

/* #391 (journey-first spec §7, Rule 5): one small graph per task, folded from the `task.*` records
 * alone. GitHub is linked, never polled: the view works offline and the records are the audit
 * trail. */

const STEPS: { step: Exclude<TaskStep, "merged">; label: string }[] = [
  { step: "implement", label: "Implement" },
  { step: "review", label: "Review" },
  { step: "merge", label: "Merge" },
];

export interface TaskGraphsProps {
  tasks: TaskState[];
  onOpenJourney: (contractId: string) => void;
}

function title(task: TaskState): string {
  return task.issue !== null ? `Issue #${task.issue}` : task.pr !== null ? `PR #${task.pr}` : task.taskId;
}

function Node({ task, step, label }: { task: TaskState; step: Exclude<TaskStep, "merged">; label: string }) {
  const order = STEPS.findIndex((entry) => entry.step === step);
  const current = STEPS.findIndex((entry) => entry.step === task.step);
  const state = task.step === "merged" || order < current ? "done" : order === current ? "current" : "next";
  const who = step === "implement" ? task.lane
    : step === "review" ? (task.reviewers.length > 0 ? task.reviewers.join(", ") : null)
    : task.mergeSha;
  return (
    <li className={`task-node task-node-${state}`} aria-current={state === "current" ? "step" : undefined}>
      <span className="task-node-label">{label}</span>
      {who !== null && <span className="task-node-agent">{who}</span>}
    </li>
  );
}

export function TaskGraphs({ tasks, onOpenJourney }: TaskGraphsProps) {
  if (tasks.length === 0) return null;
  return (
    <section className="task-graphs" aria-label="Tasks">
      {tasks.map((task) => (
        <div key={task.taskId} className="task-graph" role="group" aria-label={title(task)}>
          <div className="task-graph-head">
            {task.repoUrl !== null && task.issue !== null
              ? <a href={`${task.repoUrl}/issues/${task.issue}`} target="_blank" rel="noreferrer">{title(task)}</a>
              : <strong>{title(task)}</strong>}
            {task.pr !== null && (task.repoUrl !== null
              ? <a href={`${task.repoUrl}/pull/${task.pr}`} target="_blank" rel="noreferrer">PR #{task.pr}</a>
              : <span>PR #{task.pr}</span>)}
            {task.step === "merged" && <span className="task-graph-merged">merged</span>}
            {task.journeys.map((journey) => (
              <button key={journey} type="button" className="task-graph-journey" onClick={() => onOpenJourney(journey)}>{journey}</button>
            ))}
          </div>
          <ol className="task-graph-steps">
            {STEPS.map(({ step, label }) => <Node key={step} task={task} step={step} label={label} />)}
          </ol>
          {task.blockedBy !== null && (
            <a className="task-graph-blocked" href={task.blockedBy.commentUrl} target="_blank" rel="noreferrer">
              blocked by {task.blockedBy.reviewer} at {task.blockedBy.headSha.slice(0, 8)}
            </a>
          )}
        </div>
      ))}
    </section>
  );
}
