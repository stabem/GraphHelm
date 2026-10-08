import type { BeforeAfterPair } from "../runtime/journeys";
import { journeyPath, journeySummary } from "../runtime/journeys";
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
  /** #446: the journeys read has not answered yet. */
  journeysLoading?: boolean;
  /** #446: screen captures exist whose envelopes are not opened yet. */
  pairsLoading?: boolean;
}

function stepTitle(journeys: JourneyView[], pair: BeforeAfterPair): string {
  const step = journeys.find((j) => j.contractId === pair.contractId)?.steps.find((s) => s.stepId === pair.stepId);
  return step?.screen?.title ?? pair.stepId;
}

/** #447: each flow's branch paths right after the flow itself, so they read as its sub-rows. */
function ordered(journeys: JourneyView[]): JourneyView[] {
  const ids = journeys.map((journey) => journey.contractId);
  const parentOf = (id: string) => journeyPath(ids, id)?.flow ?? id;
  return [...journeys].sort((a, b) => ids.indexOf(parentOf(a.contractId)) - ids.indexOf(parentOf(b.contractId))
    || Number(parentOf(a.contractId) !== a.contractId) - Number(parentOf(b.contractId) !== b.contractId));
}

/** Right panel (spec §4.6): journeys with their proven share, before/after pairs, recent activity. */
export function RightPanel({ activity, onOpenActivity, journeys = [], beforeAfter = [], onOpenJourney, onOpenPair, botName = (id) => id,
  journeysLoading = false, pairsLoading = false }: RightPanelProps) {
  return (
    <aside className="right-panel" aria-label="Run side panel">
      <section className="right-section" aria-label="Journeys">
        <h2>Journeys</h2>
        {journeys.length === 0 ? <p className="right-empty">{journeysLoading ? "Loading journeys…" : "No journeys mapped yet."}</p> : (
          <ul className="right-journeys">
            {ordered(journeys).map((journey) => {
              const s = journeySummary(journey);
              const label = `${s.proven} of ${s.total} steps proven`;
              const branch = journeyPath(journeys.map((j) => j.contractId), journey.contractId);
              return (
                <li key={journey.contractId} className={branch === null ? undefined : "right-journey-branch"}>
                  <button type="button" className="right-journey" onClick={() => onOpenJourney?.(journey.contractId)}>
                    <span className="right-journey-title">{branch === null ? journey.title : `↳ ${branch.path} path`}</span>
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
        {beforeAfter.length === 0 ? <p className="right-empty">{pairsLoading ? "Opening screenshots…" : "No before/after screenshots yet."}</p> : (
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
