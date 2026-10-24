import type { CSSProperties } from "react";

import type { RequestProgress as Progress } from "./requests";

/** #1922: the request's status chip, percent delivered, one 6px segment per issue (static: proven,
 * merged, review, implementing, stalled) and the legend with a count per state. Shared by the Graph's
 * YOU ASKED card and the Lanes WHAT I ASKED card. */
export function RequestProgress({ progress }: { progress: Progress }) {
  const { chip, pct, segs, legend, done, total } = progress;
  return (
    <>
      <span className="req-head">
        <span className={`mono request-chip chip-${chip.key}`} style={{ "--chip": chip.color } as CSSProperties}>{chip.label}</span>
        <span className="mono req-pct">{pct}</span>
      </span>
      <span role="img" aria-label={`${done} of ${total} issues delivered`} className="req-bar">
        {segs.map((seg) => <span key={seg.issue} title={`#${seg.issue} ${seg.state}`} style={{ background: seg.color }} />)}
      </span>
      {legend.length > 0 && (
        <span className="req-legend">
          {legend.map((entry) => <span key={entry.label} className="mono"><i style={{ background: entry.color }} />{entry.n} {entry.label}</span>)}
        </span>
      )}
    </>
  );
}
