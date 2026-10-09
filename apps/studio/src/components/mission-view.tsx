import "./mission-view.css";
import { useMemo, useState } from "react";
import type { JourneyRunView, JourneyView } from "../runtime/types";
import type { TaskState } from "../runtime/team-tasks";
import type { Lane } from "../runtime/lane-bars";
import { buildMission, toMissionTask, unlinkedTasks, type Mission } from "../runtime/mission";
import { layoutMission } from "../runtime/mission-layout";
import { testFrames } from "../runtime/test-frames";
import { MissionGraph, STATUS_LABEL } from "./mission-graph";
import { ProofTable } from "./proof-table";
import { TestCanvas } from "./test-canvas";
import { LanesTimeline } from "./lanes-timeline";

type Sub = "graph" | "proof" | "test" | "lanes";
const WINDOW_MS = 14 * 3_600_000;

interface Props {
  journeys: JourneyView[];
  tasks: TaskState[];
  runFor(contractId: string): JourneyRunView | null;
  lanes: Lane[];
  now: number;
  frameUrl(stepId: string, contractId: string): string | null;
  /** The step and the journey it belongs to: the mark is recorded against that journey's flow. */
  onMarkSafe(stepId: string, contractId: string): void | Promise<void>;
  /** Absent: the Test canvas shows Send back as not available yet. */
  onSendBack?: (stepId: string, contractId: string) => void;
  /** Absent: the Proof view's "Replay whole journey" is disabled and says where the replay starts. */
  onReplay?: (contractId: string) => void;
}

/** The PR ids of the tasks the graph places in each step's column. */
function stepIds(m: Mission): string[] {
  const placed = layoutMission(m).placed;
  return m.steps.map((s) => {
    const prs = placed.filter((p) => p.col === s.index).map((p) => (p.task.pr ? `#${p.task.pr}` : "no PR"));
    return prs.length ? prs.join(" ") : "";
  });
}

export function MissionView({ journeys, tasks, runFor, lanes, now, frameUrl, onMarkSafe, onSendBack, onReplay }: Props) {
  const [journeyId, setJourneyId] = useState<string | null>(journeys[0]?.contractId ?? null);
  const [stepId, setStepId] = useState<string | null>(null);
  const [taskKey, setTaskKey] = useState<string | null>(null);
  const [sub, setSub] = useState<Sub>("graph");
  const [frame, setFrame] = useState(0);
  const [unlinkedOpen, setUnlinkedOpen] = useState(false);
  const journey = journeys.find((j) => j.contractId === journeyId) ?? journeys[0];
  const missions = useMemo(() => journeys.map((j) => buildMission(j, runFor(j.contractId), tasks)), [journeys, tasks, runFor]);
  const orphans = useMemo(() => unlinkedTasks(journeys, tasks), [journeys, tasks]);
  const allTasks = useMemo(() => tasks.map(toMissionTask), [tasks]);
  if (!journey) return <p>No journeys in this project yet</p>;
  const contractId = journey.contractId;
  const mission = missions.find((m) => m.contractId === contractId)!;
  const openTest = (id: string) => {
    setStepId(id);
    setFrame(Math.max(0, journey.steps.findIndex((s) => s.stepId === id)));
    setSub("test");
  };
  const pickJourney = (id: string) => { setJourneyId(id); setStepId(null); setTaskKey(null); if (sub === "test") setSub("graph"); };
  const pickStep = (jid: string, sid: string) => {
    if (jid !== contractId) pickJourney(jid);
    setStepId(sid); setTaskKey(null);
    if (sub !== "graph" && sub !== "proof") setSub("graph");
  };
  const pickTask = (key: string) => {
    setTaskKey(key);
    const placed = layoutMission(mission).placed.find((p) => p.task.key === key);
    if (placed) setStepId(mission.steps[placed.col]?.stepId ?? null);
  };
  const ids = stepIds(mission);
  return (
    // data-wide: the Studio layout gives the mission view the whole main area (mission-view.css).
    <div className="mv" data-wide="true">
      <header className="mv-head">
        <div role="tablist" aria-label="Mission views" className="mv-tabs">
          {(["graph", "lanes", "proof"] as const).map((s) => (
            <button key={s} role="tab" type="button" className="mv-tab" aria-selected={sub === s} onClick={() => setSub(s)}>
              {s === "graph" ? "Graph" : s === "proof" ? "Proof" : "Lanes"}
            </button>
          ))}
        </div>
        {sub === "test" && <button type="button" className="mv-back" onClick={() => setSub("proof")}>‹ Back to proof</button>}
      </header>
      <div className="mv-body">
        {sub !== "lanes" && (
          <nav aria-label="Journeys" className="mv-journeys">
            <span className="mv-cap">Journeys</span>
            {missions.map((m) => {
              const on = m.contractId === contractId;
              const issue = m.tasks.find((t) => t.issue !== null)?.issue ?? null;
              return (
                <div key={m.contractId} className="mv-jcard" data-selected={on}>
                  <button type="button" className="mv-journey" aria-pressed={on} title={m.title} onClick={() => pickJourney(m.contractId)}>
                    <span className="mv-journey-title">{m.title}</span>
                    <span className="mv-journey-count">{`${m.summary.proven}/${m.summary.total}`}</span>
                  </button>
                  {issue !== null && <span className="mv-journey-meta">{`issue #${issue} · ${m.tasks.length} ${m.tasks.length === 1 ? "task" : "tasks"}`}</span>}
                  <div className="mv-chips">
                    {m.steps.map((s) => (
                      <button key={s.stepId} type="button" className="mv-chip" data-status={s.status}
                        aria-pressed={on && s.stepId === stepId} aria-label={`Step ${s.index + 1}: ${s.title}, ${STATUS_LABEL[s.status]}`}
                        onClick={() => pickStep(m.contractId, s.stepId)}>{s.index + 1}</button>
                    ))}
                  </div>
                  {on && (
                    <div className="mv-steps">
                      {m.steps.map((s) => (
                        <button key={s.stepId} type="button" className="mv-step" data-status={s.status} aria-pressed={s.stepId === stepId}
                          onClick={() => pickStep(m.contractId, s.stepId)}>
                          <span className="mv-tick" data-status={s.status} />
                          <span className="mv-step-n">{s.index + 1}</span>
                          <span className="mv-step-title">{s.title}</span>
                          <span className="mv-step-ids">{ids[s.index] || "—"}</span>
                        </button>
                      ))}
                    </div>
                  )}
                </div>
              );
            })}
            {orphans.length > 0 && (
              <section aria-label="Unlinked work" className="mv-unlinked">
                <button type="button" className="mv-fold" aria-expanded={unlinkedOpen} onClick={() => setUnlinkedOpen((o) => !o)}>
                  {`Unlinked work · ${orphans.length}`}
                </button>
                {unlinkedOpen && <ul>{orphans.map((t) => <li key={t.key}>{`${t.pr ? `#${t.pr}` : "no PR"} ${t.title}`}</li>)}</ul>}
              </section>
            )}
          </nav>
        )}
        <div className="mv-content">
          {sub === "graph" && <MissionGraph mission={mission} selectedStepId={stepId} selectedTaskKey={taskKey}
            onSelectStep={(id) => { setStepId(id); setTaskKey(null); }} onSelectTask={pickTask} onOpenTest={openTest} />}
          {sub === "proof" && <ProofTable mission={mission} onOpenTest={openTest} frameUrl={(id) => frameUrl(id, contractId)}
            {...(onReplay ? { onReplay: () => onReplay(contractId) } : {})} />}
          {sub === "test" && <TestCanvas frames={testFrames(journey, runFor(contractId))} selected={frame} onSelect={setFrame}
            frameUrl={(id) => frameUrl(id, contractId)} onMarkSafe={(id) => onMarkSafe(id, contractId)} {...(onSendBack ? { onSendBack: (id: string) => onSendBack(id, contractId) } : {})} />}
          {sub === "lanes" && <LanesTimeline lanes={lanes} now={now} windowMs={WINDOW_MS} tasks={allTasks} />}
        </div>
      </div>
    </div>
  );
}
