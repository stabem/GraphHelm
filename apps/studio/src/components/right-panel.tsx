import type { BeforeAfterPair } from "../runtime/journeys";
import { journeySummary } from "../runtime/journeys";
import type { ActivityLine } from "../runtime/threads";
import type { JourneyView } from "../runtime/types";
import { ago } from "./format";

export interface RightPanelProps {
  activity: ActivityLine[];
  onOpenActivity: (sequence: number) => void;
  journeys?: JourneyView[];
  beforeAfter?: BeforeAfterPair[];
  onOpenJourney?: (contractId: string) => void;
  onOpenPair?: (pair: BeforeAfterPair) => void;
  botName?: (actorOrObserver: string) => string;
}

function stepTitle(journeys: JourneyView[], pair: BeforeAfterPair): string {
  const step = journeys.find((j) => j.contractId === pair.contractId)?.steps.find((s) => s.stepId === pair.stepId);
  return step?.screen?.title ?? pair.stepId;
}

/** Right panel (spec §4.6): journeys with their proven share, before/after pairs, recent activity. */
export function RightPanel({ activity, onOpenActivity, journeys = [], beforeAfter = [], onOpenJourney, onOpenPair, botName = (id) => id }: RightPanelProps) {
  return (
    <aside className="right-panel" aria-label="Run side panel">
      <section className="right-section" aria-label="Journeys">
        <h2>Journeys</h2>
        {journeys.length === 0 ? <p className="right-empty">No journeys mapped yet.</p> : (
          <ul className="right-journeys">
            {journeys.map((journey) => {
              const s = journeySummary(journey);
              const label = `${s.proven} of ${s.total} steps proven`;
              return (
                <li key={journey.contractId}>
                  <button type="button" className="right-journey" onClick={() => onOpenJourney?.(journey.contractId)}>
                    <span className="right-journey-title">{journey.title}</span>
                    <span className="right-bar" role="img" aria-label={label}>
                      {s.proven > 0 && <span className="right-bar-proven" style={{ flexGrow: s.proven }} />}
                      {s.stale > 0 && <span className="right-bar-stale" style={{ flexGrow: s.stale }} />}
                      {s.other > 0 && <span className="right-bar-other" style={{ flexGrow: s.other }} />}
                    </span>
                    <span className="right-when">{label}</span>
                  </button>
                </li>
              );
            })}
          </ul>
        )}
      </section>
      <section className="right-section" aria-label="Before and after">
        <h2>Before / after</h2>
        {beforeAfter.length === 0 ? <p className="right-empty">No before/after screenshots yet.</p> : (
          <ul className="right-pairs">
            {beforeAfter.map((pair) => (
              <li key={`${pair.contractId}:${pair.stepId}:${pair.pr}`}>
                <button type="button" onClick={() => onOpenPair?.(pair)}>
                  PR #{pair.pr} · {stepTitle(journeys, pair)} · {botName(pair.actorId ?? pair.observer)}
                </button>
              </li>
            ))}
          </ul>
        )}
      </section>
      <section className="right-section" aria-label="What just happened">
        <h2>What just happened</h2>
        {activity.length === 0 ? <p className="right-empty">Nothing recorded yet.</p> : (
          <ol className="right-activity">
            {activity.map((line) => (
              <li key={line.sequence}><button type="button" onClick={() => onOpenActivity(line.sequence)}>{line.text}<span className="right-when"> · {ago(line.at)}</span></button></li>
            ))}
          </ol>
        )}
      </section>
    </aside>
  );
}
