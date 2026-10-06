import type { ActivityLine } from "../runtime/threads";
import { ago } from "./format";

/** Right panel (spec §4.6). Journeys and Before / after get their data in phases 4-5; until then
 * they say so instead of drawing an empty chart. */
export function RightPanel({ activity, onOpenActivity }: { activity: ActivityLine[]; onOpenActivity: (sequence: number) => void }) {
  return (
    <aside className="right-panel" aria-label="Run side panel">
      <section className="right-section" aria-label="Journeys">
        <h2>Journeys</h2>
        <p className="right-empty">No journeys mapped yet.</p>
      </section>
      <section className="right-section" aria-label="Before and after">
        <h2>Before / after</h2>
        <p className="right-empty">No before/after screenshots yet.</p>
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
