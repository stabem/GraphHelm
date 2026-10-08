import { useState } from "react";

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

/** "While you were away" (spec §4.5) as a bar in the page flow: one line until the owner asks for
 * the details, never a dialog over the canvas. It covered the Journey list and its Watch/Approve
 * buttons at 800-1440 px when it was. Only Got it (or ten visible seconds of the live view, handled
 * by the page) moves the last-seen position; Hide only puts the bar away for this session. */
export function HandoverCard({ handover, onOpen, onDismiss, onHide }: { handover: Handover; onOpen: (sequences: number[]) => void; onDismiss: () => void; onHide?: () => void }) {
  const [open, setOpen] = useState(false);
  return (
    <section className="handover-bar" aria-label="While you were away">
      <div className="handover-bar-line">
        <p className="handover-summary">
          <strong>While you were away</strong>
          <span className="handover-span"> · {handover.eventCount} records over {span(handover.gapMinutes)}</span>
          <span> · Shipped {handover.shipped.length} · Needs you {handover.needsYou.length}</span>
        </p>
        <button type="button" aria-expanded={open} onClick={() => setOpen((value) => !value)}>{open ? "Hide details" : "Show details"}</button>
        <button type="button" className="handover-dismiss" onClick={onDismiss}>Got it</button>
        {onHide && <button type="button" onClick={onHide}>Hide for now</button>}
      </div>
      {open && (
        <div className="handover-details">
          <Group title="Shipped" lines={handover.shipped} onOpen={onOpen} />
          <Group title="Needs you" lines={handover.needsYou} onOpen={onOpen} />
          <Group title="Went quiet" lines={handover.quiet} onOpen={onOpen} />
          <Group title="Nobody touched" lines={handover.untouched} onOpen={onOpen} />
        </div>
      )}
    </section>
  );
}
