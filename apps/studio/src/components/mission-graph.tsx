import "./mission-graph.css";
import { TRUST_LABELS, type Mission, type MissionTask, type StepStatus } from "../runtime/mission";

export const STATUS_LABEL: Record<StepStatus, string> = {
  proven: "Proven", failed: "Failed", needs_you: "Needs you", preview_only: "Preview only", not_run: "Not run",
};
const TASK_LABEL: Record<MissionTask["step"], string> = { implement: "Writing", review: "In review", merge: "Merging", merged: "Merged · not proven" };

interface Props {
  mission: Mission;
  selectedStepId: string | null;
  selectedTaskKey: string | null;
  onSelectStep(stepId: string): void;
  onSelectTask(key: string): void;
  onOpenTest(stepId: string): void;
}

export function MissionGraph({ mission, selectedStepId, selectedTaskKey, onSelectStep, onSelectTask, onOpenTest }: Props) {
  const { summary } = mission;
  const task = mission.tasks.find((t) => t.key === selectedTaskKey) ?? null;
  return (
    <div className="mg">
      <p className="mg-summary">{`${summary.proven}/${summary.total} proven · ${summary.inFlight} in flight · ${summary.needYou} need you`}</p>
      <div className="mg-body">
        <nav className="mg-rail" aria-label="Journey steps">
          <h2>{mission.title}</h2>
          {mission.steps.map((s) => (
            <button key={s.stepId} type="button" className="mg-step" data-status={s.status} aria-pressed={s.stepId === selectedStepId}
              aria-label={`Step ${s.index + 1}: ${s.title}, ${STATUS_LABEL[s.status]}`} onClick={() => onSelectStep(s.stepId)}>
              <span className="mg-step-n">{s.index + 1}</span>
              <span className="mg-step-title">{s.title}</span>
              <span className="mg-step-status">{STATUS_LABEL[s.status]}</span>
            </button>
          ))}
        </nav>
        <section className="mg-graph" aria-label="Work graph">
          <div className="mg-cols" style={{ gridTemplateColumns: `repeat(${mission.steps.length}, minmax(140px, 1fr))` }}>
            {mission.steps.map((s) => (
              <div key={s.stepId} className="mg-col" data-status={s.status} data-selected={s.stepId === selectedStepId}>
                <span className="mg-col-head">{`STEP ${s.index + 1} · ${STATUS_LABEL[s.status]}`}</span>
                <span>{s.title}</span>
              </div>
            ))}
          </div>
          {mission.tasks.length === 0 ? (
            <p className="mg-empty">No work linked to this journey yet</p>
          ) : (
            <div className="mg-tasks">
              {mission.tasks.map((t) => (
                <button key={t.key} type="button" className="mg-node" data-step={t.step} data-blocked={t.blocked}
                  aria-pressed={t.key === selectedTaskKey} onClick={() => onSelectTask(t.key)}>
                  <span className="mg-node-head">{`${t.blocked ? "Blocked" : TASK_LABEL[t.step]} · ${t.pr ? `#${t.pr}` : "no PR"}`}</span>
                  <span className="mg-node-title">{t.title}</span>
                  <span className="mg-node-who">{[t.lane, ...t.reviewers].filter(Boolean).join(" → ")}</span>
                </button>
              ))}
            </div>
          )}
        </section>
        {task && (
          <aside className="mg-inspector" aria-label="Selected work">
            <h3>{task.title}</h3>
            <ul className="mg-ladder" aria-label="How far it got">
              {TRUST_LABELS.map((label, i) => (
                <li key={label} data-lit={i < task.trust}>{label}</li>
              ))}
            </ul>
            <button type="button" onClick={() => onOpenTest(selectedStepId ?? mission.steps[0]?.stepId ?? "")}>Open its test</button>
          </aside>
        )}
      </div>
    </div>
  );
}
