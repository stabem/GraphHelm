import "./proof-table.css";
import type { Mission, StepStatus } from "../runtime/mission";
import { layoutMission } from "../runtime/mission-layout";
import { STATUS_LABEL } from "./mission-graph";

const HINT: Record<StepStatus, string> = {
  proven: "Proven by a machine. Seen by you closes the ladder.",
  needs_you: "Only you can close this. Watch it, then mark it safe or send it back.",
  failed: "The replay failed here. Open the test to see what it saw.",
  preview_only: "A draft preview is not proof. Approve the flow to replay it.",
  not_run: "Not replayed yet.",
};

interface Props {
  mission: Mission;
  onOpenTest(stepId: string): void;
  /** The recorded frame for a step, if any; absent or null draws the placeholder browser. */
  frameUrl?: (stepId: string) => string | null;
  /** Absent: the replay starts from the journey panel, so the button says so and is disabled. */
  onReplay?: () => void;
}

export function ProofTable({ mission, onOpenTest, frameUrl, onReplay }: Props) {
  const placed = layoutMission(mission).placed;
  return (
    <section className="proof" aria-label="Proof">
      <div className="proof-head">
        <div className="proof-title">
          <span className="proof-cap">{`Journey · proof · ${mission.summary.proven}/${mission.summary.total} proven`}</span>
          <h2>{`Can I trust “${mission.title}”?`}</h2>
          <span className="proof-sub">Every step is a promise to a person. Each row shows who built it, what checked it, and the frame the replay saw.</span>
        </div>
        {onReplay
          ? <button type="button" className="proof-primary" onClick={onReplay}>Replay whole journey</button>
          : <button type="button" className="proof-primary" disabled title="Start the replay from the journey panel">Replay whole journey — start it from the journey panel</button>}
      </div>
      <div className="proof-cols" aria-hidden="true"><span>Step</span><span>Promise</span><span>Replay saw</span><span>Chain of custody</span><span>Your call</span></div>
      <ol className="proof-rows" aria-label="Steps">
        {mission.steps.map((s) => {
          const src = frameUrl?.(s.stepId) ?? null;
          const tasks = placed.filter((p) => p.col === s.index).map((p) => p.task);
          return (
            <li key={s.stepId} className="proof-row" data-status={s.status}>
              <div className="proof-n"><span>{s.index + 1}</span><span className="proof-dot" data-status={s.status} /></div>
              <div className="proof-promise">
                <span className="proof-name">{s.title}</span>
                {s.promise && <span className="proof-sub">{s.promise}</span>}
                <span className="proof-badge" data-status={s.status}>{`${STATUS_LABEL[s.status]}${s.reason ? ` · ${s.reason}` : ""}`}</span>
              </div>
              <button type="button" className="proof-frame" data-status={s.status} aria-label={`Open the test canvas for step ${s.index + 1}`} onClick={() => onOpenTest(s.stepId)}>
                {src ? <img src={src} alt={`Replay frame of step ${s.index + 1}`} /> : (
                  <span className="proof-mini" aria-hidden="true">
                    <span className="proof-mini-bar"><i /><i /><i /></span>
                    <span className="proof-mini-body"><span className="proof-mini-hl" data-status={s.status} /><span className="proof-mini-line" /><span className="proof-mini-line" /></span>
                  </span>
                )}
                <span className="proof-frame-txt"><span>{src ? "frame recorded" : "no frame recorded"}</span><span>open test →</span></span>
              </button>
              <div className="proof-chain">
                {tasks.length === 0 ? <span className="proof-sub">No work linked to this step</span> : tasks.map((t) => (
                  <span key={t.key} className="proof-chips">
                    <span className="proof-chip" data-tone={t.step === "implement" ? "run" : "ok"}><span className="proof-stage">impl</span>{` ${t.lane ?? "—"}`}</span>
                    {t.reviewers.length > 0 && <span className="proof-chip" data-tone={t.blocked ? "no" : t.step === "review" ? "run" : "ok"}><span className="proof-stage">rev</span>{` ${t.reviewers.join(", ")}${t.blocked ? " BLOCK" : ""}`}</span>}
                    {t.step === "merged" && <span className="proof-chip" data-tone="ok"><span className="proof-stage">merge</span>{` ${t.mergeSha?.slice(0, 8) ?? (t.pr ? `#${t.pr}` : "—")}`}</span>}
                  </span>
                ))}
              </div>
              <div className="proof-call">
                <button type="button" className={s.status === "needs_you" ? "proof-primary" : "proof-secondary"} aria-label={`Open test for step ${s.index + 1}`} onClick={() => onOpenTest(s.stepId)}>
                  {s.status === "needs_you" ? "Watch it, mark safe" : "Open test"}
                </button>
                <span className="proof-hint">{HINT[s.status]}</span>
              </div>
            </li>
          );
        })}
      </ol>
    </section>
  );
}
