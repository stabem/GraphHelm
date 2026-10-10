import { useId, useLayoutEffect, useRef, type KeyboardEvent } from "react";
import { buildDelegationTree } from "../runtime/delegation-tree";
import type { TaskState } from "../runtime/team-tasks";
import "./delegation-tree.css";

/** Always-expanded tree: arrow keys walk the recorded hierarchy, with one tab stop. */
export function DelegationTree({ tasks }: { tasks: TaskState[] }) {
  const id = useId();
  const tree = useRef<HTMLUListElement>(null);
  const roots = buildDelegationTree(tasks);
  useLayoutEffect(() => {
    const items = Array.from(tree.current?.querySelectorAll<HTMLElement>('[role="treeitem"]') ?? []);
    const active = items.find((item) => item === document.activeElement) ?? items[0];
    for (const item of items) item.tabIndex = item === active ? 0 : -1;
  }, [tasks]);
  const navigate = (event: KeyboardEvent<HTMLUListElement>) => {
    const current = event.target as HTMLElement;
    if (current.getAttribute("role") !== "treeitem") return;
    const items = Array.from(event.currentTarget.querySelectorAll<HTMLElement>('[role="treeitem"]'));
    const index = items.indexOf(current);
    let next: HTMLElement | undefined | null;
    switch (event.key) {
      case "ArrowDown": next = items[index + 1]; break;
      case "ArrowUp": next = items[index - 1]; break;
      case "ArrowRight": next = current.querySelector<HTMLElement>(':scope > [role="group"] > [role="treeitem"]'); break;
      case "ArrowLeft": next = current.parentElement?.closest<HTMLElement>('[role="treeitem"]'); break;
      case "Home": next = items[0]; break;
      case "End": next = items.at(-1); break;
      default: return;
    }
    event.preventDefault();
    next?.focus();
  };
  return <section className="delegation-tree" aria-label="Delegation">
    <h2>Recorded handoffs</h2>
    <p className="delegation-note">Assignments are reported by lanes. Missing records stay unknown.</p>
    {roots.length === 0 ? <p>No task handoffs recorded.</p> : <ul ref={tree} className="delegation-roots" role="tree" aria-label="Recorded handoffs"
      onKeyDown={navigate} onFocus={(event) => {
        if (event.target.getAttribute("role") !== "treeitem") return;
        for (const item of event.currentTarget.querySelectorAll<HTMLElement>('[role="treeitem"]')) item.tabIndex = item === event.target ? 0 : -1;
      }}>
      {roots.map((root, r) => <li key={root.assigner ?? "\u0000"} role="treeitem" aria-expanded="true" tabIndex={r === 0 ? 0 : -1} aria-labelledby={`${id}-${r}`}>
        <div className="delegation-label" id={`${id}-${r}`}>{root.assigner ?? "assigner unrecorded"}</div>
        <ul role="group">
          {root.lanes.map((lane, l) => <li key={lane.lane ?? "\u0000"} role="treeitem" aria-expanded="true" tabIndex={-1} aria-labelledby={`${id}-${r}-${l}`}>
            <div className="delegation-label" id={`${id}-${r}-${l}`}>
              <strong>{lane.lane ?? "lane unrecorded"}</strong>
              <span className="delegation-source">{lane.source ? `claimed.assignedBy · reported by ${lane.lane}` : "assigner unrecorded"}</span>
            </div>
            <ul role="group">
              {lane.tasks.map(({ task, source, reviewers }, t) => <li key={`${task.taskId}:${task.branch ?? task.key}`} role="treeitem" aria-expanded={reviewers.length ? true : undefined} tabIndex={-1} aria-labelledby={`${id}-${r}-${l}-${t}`}>
                <div className="delegation-label" id={`${id}-${r}-${l}-${t}`}>
                  <strong>{task.issue === null ? task.taskId : `#${task.issue}`}{task.title ? ` · ${task.title}` : ""}</strong>
                  <span>{task.blockedBy ? `BLOCK · ${task.blockedBy.reviewer}` : task.step}{task.pr !== null ? ` · PR #${task.pr}` : ""}</span>
                  <span className="delegation-source">{source ? `claimed.assignedBy · reported by ${lane.lane}` : "assigner unrecorded"}</span>
                </div>
                {reviewers.length > 0 && <ul role="group">{reviewers.map(({ reviewer, source }, v) => <li key={reviewer} role="treeitem" tabIndex={-1} aria-labelledby={`${id}-${r}-${l}-${t}-${v}`}>
                  <div className="delegation-label" id={`${id}-${r}-${l}-${t}-${v}`}>
                    <strong>{reviewer}</strong><span className="delegation-source">{source}</span>
                  </div>
                </li>)}</ul>}
              </li>)}
            </ul>
          </li>)}
        </ul>
      </li>)}
    </ul>}
  </section>;
}
