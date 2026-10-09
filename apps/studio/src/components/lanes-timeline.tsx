import "./lanes-timeline.css";
import type { Lane } from "../runtime/lane-bars";
import type { MissionTask } from "../runtime/mission";

const pct = (n: number) => `${Math.round(n * 10000) / 100}%`;

/** The three cards under the lanes, read only from real data: stuck = a lane flagged silent;
 * ping-pong = a task with two or more BLOCK rounds; free hands = a lane with no open bar. */
export function laneCards(lanes: Lane[], tasks: MissionTask[]) {
  return {
    stuck: lanes.filter((l) => l.silent).map((l) => l.lane),
    pingPong: tasks.filter((t) => t.rounds.length >= 2).map((t) => `${t.pr ? `#${t.pr}` : t.title} · ${t.rounds.length} BLOCKs`),
    free: lanes.filter((l) => !l.bars.some((b) => b.open)).map((l) => l.lane),
  };
}

/** The title of the last merged task each lane implemented, in the order the tasks arrive. */
function lastDelivered(tasks: MissionTask[]) {
  const out = new Map<string, string>();
  for (const t of tasks) if (t.step === "merged" && t.lane) out.set(t.lane, `${t.pr ? `#${t.pr} ` : ""}${t.title}`);
  return out;
}

interface Props { lanes: Lane[]; now: number; windowMs: number; tasks?: MissionTask[] }

export function LanesTimeline({ lanes, now, windowMs, tasks = [] }: Props) {
  if (lanes.length === 0) return <p className="lt-empty">No agent has recorded work in this window</p>;
  const from = now - windowMs;
  const hours = Math.round(windowMs / 3_600_000);
  const step = hours > 8 ? 2 : 1;
  const ticks: string[] = [];
  for (let h = hours; h > 0; h -= step) ticks.push(`−${h}h`);
  const delivered = lastDelivered(tasks);
  const cards = laneCards(lanes, tasks);
  const card = (items: string[]) => (items.length ? items.join(", ") : "none right now");
  return (
    <section className="lt" aria-label="Lanes">
      <div className="lt-head">
        <div className="lt-title">
          <span className="lt-cap">{`Last ${hours} hours · ${lanes.length} lanes`}</span>
          <h2>Who did what, and who is stuck</h2>
        </div>
        <div className="lt-legend">
          <span className="lt-key" data-kind="implement">Implement</span>
          <span className="lt-key" data-kind="review">Review</span>
          <span className="lt-key" data-kind="merge">Merge + proof</span>
          <span className="lt-key" data-kind="silent">Silent / blocked</span>
        </div>
      </div>
      <div className="lt-row lt-axis" aria-hidden="true">
        <span>AGENT</span>
        <span className="lt-ticks">{ticks.map((t) => <span key={t}>{t}</span>)}<span className="lt-now">now</span></span>
        <span>LAST DELIVERED</span>
      </div>
      <ul className="lt-lanes" aria-label="Agent lanes">
        {lanes.map((l) => {
          const open = l.bars.some((b) => b.open);
          return (
            <li key={l.lane} className="lt-row lt-lane">
              <span className="lt-who">
                <span className="lt-dot" data-state={l.silent ? "silent" : open ? "busy" : "idle"} />
                <span className="lt-name">{l.lane}</span>
                {l.silent ? <span className="lt-flag" data-flag="silent">silent</span> : !open && <span className="lt-flag" data-flag="free">free</span>}
              </span>
              <div className="lt-track">
                {l.bars.map((b, i) => (
                  <div key={i} className="lt-bar" data-kind={b.kind} data-silent={l.silent && b.open}
                    style={{ left: pct((b.start - from) / windowMs), width: pct((b.end - b.start) / windowMs) }}>
                    <span>{`${b.label} ${b.kind}`}</span>
                  </div>
                ))}
              </div>
              <span className="lt-last">{delivered.get(l.lane) ?? "—"}</span>
            </li>
          );
        })}
      </ul>
      <div className="lt-cards">
        <div className="lt-card" data-kind="stuck"><span className="lt-card-cap">STUCK</span><span>{card(cards.stuck)}</span></div>
        <div className="lt-card" data-kind="pingpong"><span className="lt-card-cap">PING-PONG</span><span>{card(cards.pingPong)}</span></div>
        <div className="lt-card" data-kind="free"><span className="lt-card-cap">FREE HANDS</span><span>{card(cards.free)}</span></div>
      </div>
    </section>
  );
}
