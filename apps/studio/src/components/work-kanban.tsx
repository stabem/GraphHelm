import "./mission-graph.css";
import "./work-kanban.css";
import type { MissionTask } from "../runtime/mission";
import type { StageHealth } from "../runtime/stage-health";
import type { PrRow, WorkStage } from "../runtime/work-groups";
import { bucketKanban, kanbanColumn, KANBAN_COLUMNS, type KanbanColumn } from "../runtime/work-kanban";
import { CurrentCard, type Pace } from "./mission-graph";

/** #668: one open PR as the kanban reads it: the Graph's current-stage node and where it lives. */
export interface KanbanItem { groupKey: string; row: PrRow; stage: WorkStage; task: MissionTask; health: StageHealth | null; pace: Pace | null }

const DOT: Record<KanbanColumn, string> = {
  implement: "#F5A524", review: "#8FB3D9", blocked: "#FF9F43", build: "#9AA7FF", silent: "#FF6B5E", merge: "#7ED4A6",
};

/** #668: all open work by stage, no clicks needed; a card opens its PR on the Graph. */
export function WorkKanban({ items, onOpen }: { items: KanbanItem[]; onOpen(groupKey: string, taskKey: string): void }) {
  const cols = bucketKanban(items, (i) => kanbanColumn(i.stage, i.health));
  return (
    <section className="wk" aria-label="Work by stage">
      {KANBAN_COLUMNS.map((c) => {
        const list = cols[c.id], id = `wk-col-${c.id}`;
        return (
          <section key={c.id} className="wk-col" data-col={c.id} aria-labelledby={id}>
            <h3 className="wk-head" id={id}>
              <span className="wk-dot" style={{ background: DOT[c.id] }} aria-hidden="true" />
              <span className="wk-label">{c.label.toUpperCase()}</span>
              <span className="wk-count">{` · ${list.length}`}</span>
            </h3>
            <div className="wk-cards">
              {list.length === 0 ? <p className="wk-none">none</p> : list.map((i) => {
                const cell = i.row.cells.find((x) => x.state === "current");
                return cell ? (
                  <CurrentCard key={i.task.key} cell={cell} stage={i.stage} task={i.task} health={i.health} pace={i.pace} selected={false}
                    onSelect={() => onOpen(i.groupKey, i.task.key)} />
                ) : null;
              })}
            </div>
          </section>
        );
      })}
    </section>
  );
}
