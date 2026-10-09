import "./lanes-timeline.css";
import type { Lane } from "../runtime/lane-bars";

const pct = (n: number) => `${Math.round(n * 10000) / 100}%`;

export function LanesTimeline({ lanes, now, windowMs }: { lanes: Lane[]; now: number; windowMs: number }) {
  if (lanes.length === 0) return <p className="lt-empty">No agent has recorded work in this window</p>;
  const from = now - windowMs;
  const hours = Math.round(windowMs / 3_600_000);
  return (
    <div className="lt">
      <div className="lt-axis" aria-hidden="true"><span>{`−${hours}h`}</span><span>now</span></div>
      <ul className="lt-lanes" aria-label="Agent lanes">
        {lanes.map((l) => (
          <li key={l.lane} className="lt-lane">
            <span className="lt-name">{l.lane}</span>
            {l.silent && <span className="lt-flag">silent</span>}
            <div className="lt-track">
              {l.bars.map((b, i) => (
                <div key={i} className="lt-bar" data-kind={b.kind} data-silent={l.silent && b.open}
                  style={{ left: pct((b.start - from) / windowMs), width: pct((b.end - b.start) / windowMs) }}>
                  <span>{`${b.label} ${b.kind}`}</span>
                </div>
              ))}
            </div>
          </li>
        ))}
      </ul>
    </div>
  );
}
