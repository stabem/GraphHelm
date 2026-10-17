import "./mission-graph.css";
import "./work-kanban.css";
import type { MissionTask } from "../runtime/mission";
import { stageDuration, type StageHealth } from "../runtime/stage-health";
import { slowestText, summaryCells } from "../runtime/step-durations";
import type { BacklogIssue, TaskDurations } from "../runtime/tasks-route";
import type { PrRow, WorkStage } from "../runtime/work-groups";
import { bucketKanban, kanbanColumn, KANBAN_COLUMNS, type KanbanColumn } from "../runtime/work-kanban";
import { CurrentCard, type Pace } from "./mission-graph";

/** #668: one open PR as the kanban reads it: the Graph's current-stage node and where it lives. */
export interface KanbanItem { groupKey: string; row: PrRow; stage: WorkStage; task: MissionTask; health: StageHealth | null; pace: Pace | null }

const DOT: Record<KanbanColumn, string> = {
  backlog: "#8D929C", implement: "#F5A524", review: "#8FB3D9", blocked: "#FF9F43", build: "#9AA7FF", silent: "#FF6B5E", merge: "#7ED4A6",
};

/** #668: all open work by stage, no clicks needed; a card opens its PR on the Graph. */
export function WorkKanban({ items, onOpen, historyLoading = false, backlog = [], durations, now }: { items: KanbanItem[]; onOpen(groupKey: string, taskKey: string): void; historyLoading?: boolean; backlog?: BacklogIssue[]; durations?: TaskDurations; now: number }) {
  if (historyLoading) return <section className="wk-loading" role="status">Loading the team's history…</section>;
  const cols = bucketKanban(items, (i) => kanbanColumn(i.stage, i.health));
  const cells = summaryCells(durations);
  return (
    <>
    {cells.length > 0 ? (
      <ul className="wk-durations" aria-label="Step durations, median and p90">
        {cells.map((c) => (
          <li key={c.id} className="wk-duration" data-step={c.id}>
            <span className="wk-duration-label">{c.label}</span>
            <span className="wk-duration-value">{`median ${stageDuration(c.stat.medianSeconds * 1000)} · p90 ${stageDuration(c.stat.p90Seconds * 1000)}`}</span>
          </li>
        ))}
      </ul>
    ) : null}
    <section className="wk" aria-label="Work by stage">
      {KANBAN_COLUMNS.map((c) => {
        const id = `wk-col-${c.id}`;
        if (c.id === "backlog") {
          return (
            <section key={c.id} className="wk-col" data-col={c.id} aria-labelledby={id}>
              <h3 className="wk-head" id={id}>
                <span className="wk-dot" style={{ background: DOT[c.id] }} aria-hidden="true" />
                <span className="wk-label">{c.label.toUpperCase()}</span>
                <span className="wk-count">{` · ${backlog.length}`}</span>
              </h3>
              <ul className="wk-cards wk-backlog">
                {backlog.length === 0 ? <li className="wk-none">none</li> : backlog.map((b) => {
                  const opened = Date.parse(b.openedAt);
                  return (
                    <li key={b.issue} className="wk-backlog-card">
                      <span className="wk-backlog-num">#{b.issue}</span>
                      <span className="wk-backlog-title">{b.title}</span>
                      <span className="wk-backlog-wait">{Number.isFinite(opened) ? `waiting ${stageDuration(Math.max(0, now - opened))}` : "waiting"}</span>
                    </li>
                  );
                })}
              </ul>
            </section>
          );
        }
        const list = cols[c.id];
        return (
          <section key={c.id} className="wk-col" data-col={c.id} aria-labelledby={id}>
            <h3 className="wk-head" id={id} aria-label={c.short ? `${c.label} · ${list.length}` : undefined}>
              <span className="wk-dot" style={{ background: DOT[c.id] }} aria-hidden="true" />
              <span className="wk-label" title={c.short ? c.label : undefined}>{(c.short ?? c.label).toUpperCase()}</span>
              <span className="wk-count">{` · ${list.length}`}</span>
            </h3>
            <div className="wk-cards">
              {list.length === 0 ? <p className="wk-none">none</p> : list.map((i) => {
                const cell = i.row.cells.find((x) => x.state === "current");
                const slowest = slowestText(durations, i.task.key);
                return cell ? (
                  <div key={i.task.key} className="wk-card">
                    <CurrentCard cell={cell} stage={i.stage} task={i.task} health={i.health} pace={i.pace} selected={false}
                      onSelect={() => onOpen(i.groupKey, i.task.key)} />
                    {slowest ? <p className="wk-slowest">{slowest}</p> : null}
                  </div>
                ) : null;
              })}
            </div>
          </section>
        );
      })}
    </section>
    </>
  );
}
