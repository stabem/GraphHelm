import { useId, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import { handoffSummary } from "../runtime/handoff-summary";
import { buildDelegationTree } from "../runtime/delegation-tree";
import type { TaskState } from "../runtime/team-tasks";
import "./delegation-tree.css";

/** Arrow keys walk the recorded hierarchy; task disclosures expose only recorded history. */
export function DelegationTree({ tasks }: { tasks: TaskState[] }) {
  const id = useId();
  const tree = useRef<HTMLUListElement>(null);
  const roots = buildDelegationTree(tasks);
  const summary = handoffSummary(tasks);
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set());
  const toggle = (key: string) => setExpanded((previous) => {
    const next = new Set(previous);
    if (next.has(key)) next.delete(key); else next.add(key);
    return next;
  });
  const evidence = (url: string) => {
    let safe = false;
    try {
      const parsed = new URL(url);
      safe = parsed.protocol === "https:" && parsed.hostname === "github.com" && !parsed.username && !parsed.password && !parsed.port;
    } catch { /* Missing or malformed evidence stays text. */ }
    return safe ? <a href={url} rel="noreferrer">{url}</a> : <span>{url || "not recorded"}</span>;
  };
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
    <section className="handoff-summary" aria-label="Now, last, next">
      <div><h3>Now</h3><ul aria-label="Now">{summary.now.length ? summary.now.map((item) => <li key={item.key} aria-label={`#${item.issue ?? "not recorded"} - ${item.lane ?? "not recorded"} - ${item.step}`}>
        #{item.issue ?? "not recorded"} - {item.lane ?? "not recorded"} - {item.step}; entered {item.since ?? "not recorded"}
      </li>) : <li>none recorded</li>}</ul></div>
      <div><h3>Last</h3><ul aria-label="Last">{summary.last ? <li>
        #{summary.last.issue ?? "not recorded"} - {summary.last.lane ?? "not recorded"} - {summary.last.step}; record {summary.last.sequence}; time {summary.last.at ?? "not recorded"}
      </li> : <li>none recorded</li>}</ul></div>
      <div><h3>Next (owed)</h3><ul aria-label="Next">{summary.next.length ? summary.next.map((item) => <li key={item.key} aria-label={item.text}>{item.text}</li>) : <li>none recorded</li>}</ul></div>
    </section>
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
              {lane.tasks.map(({ task, source, reviewers }, t) => <li key={`${task.taskId}:${task.branch ?? task.key}`} role="treeitem" aria-expanded={expanded.has(task.key)} tabIndex={-1} aria-labelledby={`${id}-${r}-${l}-${t}`}
                onKeyDown={(event) => {
                  if (event.target === event.currentTarget && (event.key === "Enter" || event.key === " ")) {
                    event.preventDefault(); event.stopPropagation(); toggle(task.key);
                  }
                }} onClick={(event) => {
                  const target = event.target as HTMLElement;
                  if (target.closest('[role="treeitem"]') === event.currentTarget && !target.closest('[role="region"]')) toggle(task.key);
                }}>
                <div className="delegation-label" id={`${id}-${r}-${l}-${t}`}>
                  <strong>{task.issue === null ? task.taskId : `#${task.issue}`}{task.title ? ` · ${task.title}` : ""}</strong>
                  <span>{task.blockedBy ? `BLOCK · ${task.blockedBy.reviewer}` : task.step === "merged" && !task.mergeSha ? "not recorded" : task.step}{task.pr !== null ? ` · PR #${task.pr}` : ""}</span>
                  <span className="delegation-source">{source ? `claimed.assignedBy · reported by ${lane.lane}` : "assigner unrecorded"}</span>
                </div>
                {expanded.has(task.key) && <section className="delegation-details" role="region" aria-label={`${task.issue === null ? task.taskId : `#${task.issue}`} details`}>
                  <p>Title: {task.title ?? "not recorded"}</p>
                  <p>Summary: {task.summary ?? "not recorded"}</p>
                  <p>PR title: {task.prTitle ?? "not recorded"}</p>
                  <p>PR summary: {task.prSummary ?? "not recorded"}</p>
                  <p>Step: {task.step === "merged" && !task.mergeSha ? "not recorded" : task.step}</p>
                  <p>Recorded heads: {task.recordedHeads.join(", ") || "not recorded"}</p>
                  <h3>Review rounds</h3>
                  <ul aria-label="Review rounds">{task.rounds.length ? task.rounds.map((round, index) => <li key={index} aria-label={`BLOCK ${round.reviewer} on ${round.headSha}`}>
                    BLOCK {round.reviewer} on {round.headSha}; fix head: {round.fixHead ?? "not recorded"}; blocked at: {round.blockedAt ?? "not recorded"}; fixed at: {round.fixedAt ?? "not recorded"}. {evidence(round.commentUrl)}
                  </li>) : <li>not recorded</li>}</ul>
                  <h3>Stray verdicts</h3>
                  <ul aria-label="Stray verdicts">{task.strayVerdicts.length ? task.strayVerdicts.map((verdict, index) => <li key={index}>
                    {verdict.reviewer}: {verdict.verdict} on {verdict.headSha}; {verdict.reason}{verdict.reason === "superseded" ? ` by ${verdict.supersededBy}` : ""}
                    {verdict.reason === "unrecorded" && evidence(verdict.record.commentUrl ?? "")}
                  </li>) : <li>not recorded</li>}</ul>
                  <p>Merge SHA: {task.mergeSha ?? "not recorded"}</p>
                  <p>Critic: {task.critic ? `round ${task.critic.round}, score ${task.critic.score}, pass score ${task.critic.passScore}, maximum rounds ${task.critic.maxRounds}, ${task.critic.verdict}` : "not recorded"}</p>
                  <h3>Clock</h3><p>Current step entered: {task.clock.since ?? "not recorded"}</p>
                  <ul aria-label="Step clock">{(["plan", "critic", "implement", "review", "merge"] as const).map((step) => <li key={step}>{step}: {task.clock.spent[step] === undefined ? "not recorded" : `${task.clock.spent[step]} ms spent`}</li>)}</ul>
                  <p>Transcripts, live host delegation and advisor nodes: not recorded.</p>
                </section>}
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
