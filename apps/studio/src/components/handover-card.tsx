import type { Handover, HandoverLine } from "../runtime/handover";

function span(minutes: number): string {
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  return hours === 0 ? `${rest} min` : rest === 0 ? `${hours} h` : `${hours} h ${rest} min`;
}

function Group({ title, lines, onOpen }: { title: string; lines: HandoverLine[]; onOpen: (sequences: number[]) => void }) {
  return (
    <section className="handover-group" aria-label={title}>
      <h3>{title}</h3>
      {lines.length === 0 ? <p className="handover-empty">Nothing.</p> : (
        <ul>{lines.map((line) => <li key={`${line.text}:${line.sequences.join(",")}`}><button type="button" onClick={() => onOpen(line.sequences)}>{line.text}</button></li>)}</ul>
      )}
    </section>
  );
}

/** "While you were away" (spec §4.5). It overlays the canvas; only Got it (or ten visible
 * seconds of the live view, handled by the page) moves the last-seen position. */
export function HandoverCard({ handover, onOpen, onDismiss }: { handover: Handover; onOpen: (sequences: number[]) => void; onDismiss: () => void }) {
  return (
    <section className="handover-card" role="dialog" aria-modal="false" aria-label="While you were away">
      <h2>While you were away</h2>
      <p className="handover-span">{handover.eventCount} records over {span(handover.gapMinutes)}</p>
      <Group title="Shipped" lines={handover.shipped} onOpen={onOpen} />
      <Group title="Needs you" lines={handover.needsYou} onOpen={onOpen} />
      <Group title="Went quiet" lines={handover.quiet} onOpen={onOpen} />
      <Group title="Nobody touched" lines={handover.untouched} onOpen={onOpen} />
      <button type="button" className="handover-dismiss" onClick={onDismiss}>Got it</button>
    </section>
  );
}
