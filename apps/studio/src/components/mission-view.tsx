import "./mission-view.css";
import { useMemo, useState } from "react";
import type { JourneyRunView, JourneyView } from "../runtime/types";
import type { TaskState } from "../runtime/team-tasks";
import type { Lane } from "../runtime/lane-bars";
import { buildMission, unlinkedTasks } from "../runtime/mission";
import { testFrames } from "../runtime/test-frames";
import { MissionGraph } from "./mission-graph";
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
}

export function MissionView({ journeys, tasks, runFor, lanes, now, frameUrl, onMarkSafe, onSendBack }: Props) {
  const [journeyId, setJourneyId] = useState<string | null>(journeys[0]?.contractId ?? null);
  const [stepId, setStepId] = useState<string | null>(null);
  const [taskKey, setTaskKey] = useState<string | null>(null);
  const [sub, setSub] = useState<Sub>("graph");
  const [frame, setFrame] = useState(0);
  const [unlinkedOpen, setUnlinkedOpen] = useState(false);
  const journey = journeys.find((j) => j.contractId === journeyId) ?? journeys[0];
  const missions = useMemo(() => journeys.map((j) => buildMission(j, runFor(j.contractId), tasks)), [journeys, tasks, runFor]);
  const orphans = useMemo(() => unlinkedTasks(journeys, tasks), [journeys, tasks]);
  if (!journey) return <p>No journeys in this project yet</p>;
  const contractId = journey.contractId;
  const mission = missions.find((m) => m.contractId === contractId)!;
  const openTest = (id: string) => {
    setStepId(id);
    setFrame(Math.max(0, journey.steps.findIndex((s) => s.stepId === id)));
    setSub("test");
  };
  return (
    // data-wide: the Studio layout gives the mission view the whole main area (mission-view.css).
    <div className="mv" data-wide="true">
      <nav aria-label="Journeys" className="mv-journeys">
        {missions.map((m) => (
          <button key={m.contractId} type="button" className="mv-journey" aria-pressed={m.contractId === contractId} title={m.title}
            onClick={() => { setJourneyId(m.contractId); setStepId(null); setTaskKey(null); if (sub === "test") setSub("graph"); }}>
            <span className="mv-journey-title">{m.title}</span>
            <span className="mv-journey-count">{`${m.summary.proven}/${m.summary.total}`}</span>
          </button>
        ))}
      </nav>
      <div className="mv-content">
      {orphans.length > 0 && (
        <section aria-label="Unlinked work" className="mv-unlinked">
          <button type="button" aria-expanded={unlinkedOpen} onClick={() => setUnlinkedOpen((o) => !o)}>
            {`Unlinked work · ${orphans.length}`}
          </button>
          {unlinkedOpen && <ul>{orphans.map((t) => <li key={t.key}>{`${t.pr ? `#${t.pr}` : "no PR"} ${t.title}`}</li>)}</ul>}
        </section>
      )}
      <div role="tablist" aria-label="Mission views">
        {(["graph", "proof", "lanes"] as const).map((s) => (
          <button key={s} role="tab" type="button" aria-selected={sub === s} onClick={() => setSub(s)}>
            {s === "graph" ? "Graph" : s === "proof" ? "Proof" : "Lanes"}
          </button>
        ))}
      </div>
      {sub === "graph" && <MissionGraph mission={mission} selectedStepId={stepId} selectedTaskKey={taskKey} onSelectStep={setStepId} onSelectTask={setTaskKey} onOpenTest={openTest} />}
      {sub === "proof" && <ProofTable mission={mission} onOpenTest={openTest} />}
      {sub === "test" && <TestCanvas frames={testFrames(journey, runFor(contractId))} selected={frame} onSelect={setFrame}
        frameUrl={(id) => frameUrl(id, contractId)} onMarkSafe={(id) => onMarkSafe(id, contractId)} {...(onSendBack ? { onSendBack: (id: string) => onSendBack(id, contractId) } : {})} />}
      {sub === "lanes" && <LanesTimeline lanes={lanes} now={now} windowMs={WINDOW_MS} />}
      </div>
    </div>
  );
}
