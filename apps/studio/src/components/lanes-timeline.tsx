import "./lanes-timeline.css";
import { packBars, placeholderLane, type Lane } from "../runtime/lane-bars";
import { stageDuration } from "../runtime/stage-health";
import type { MissionTask } from "../runtime/mission";
import type { Bot } from "../runtime/team";
import { useEffect, useRef, useState, type RefObject } from "react";
import { agentBoard, type AgentRow, type AgentStatus } from "../runtime/agent-board";
import { slotDuration, type SlotView } from "../runtime/slots";

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

/** #647: the bot controls the retired Team canvas had; absent, the board shows no buttons. */
export interface BotActions {
  onOpenBotDetails?: (key: string) => void;
  onNameBot?: (actorId: string, displayName: string) => void | Promise<unknown>;
}

interface Props extends BotActions { lanes: Lane[]; now: number; windowMs: number; tasks?: MissionTask[]; agents?: Bot[]; slots?: SlotView[] }

/** #647: the Team canvas's rename control (same gate, labels and save path), on a board row. */
export function NameBot({ bot, onSave }: { bot: Bot; onSave: (actorId: string, displayName: string) => void | Promise<unknown> }) {
  const [open, setOpen] = useState(false);
  const [value, setValue] = useState("");
  if (bot.actorId === null) return null;
  const actorId = bot.actorId;
  if (!open) return <button type="button" className="ab-act" onClick={() => setOpen(true)}>Name this bot</button>;
  const name = value.trim();
  return (
    <form className="ab-name-form" onSubmit={(event) => { event.preventDefault(); if (name.length === 0) return; void onSave(actorId, name); setOpen(false); setValue(""); }}>
      <input aria-label={`Name for ${bot.name}`} placeholder="Display name" maxLength={80} value={value} onChange={(event) => setValue(event.target.value)} autoFocus />
      <button type="submit" disabled={name.length === 0}>Save</button>
      <button type="button" onClick={() => { setOpen(false); setValue(""); }}>Cancel</button>
    </form>
  );
}

/** The Team canvas showed the rename only for an unnamed, unshared, non-native actor. */
const nameable = (b: Bot) => b.actorId !== null && b.role === null && !b.native && !b.shared;

const PILL: Record<AgentStatus, string> = { silent: "Silent", stale: "Stale claim", building: "Building", build_wait: "Waiting for build", working: "Working", awaiting: "Awaiting review", waiting: "Waiting", free: "Free" };
const STAGE: Record<string, string> = { implement: "implementing", review: "reviewing", merge: "merging" };
type Filter = "all" | "working" | "free" | "silent";

function AgentBoard({ rows, agents = [], onOpenBotDetails, onNameBot }: { rows: AgentRow[]; agents?: Bot[] } & BotActions) {
  const [filter, setFilter] = useState<Filter>("all");
  const count = (f: Filter) => (f === "all" ? rows.length : rows.filter((r) => r.status === f).length);
  const shown = filter === "all" ? rows : rows.filter((r) => r.status === filter);
  return (
    <section className="ab" aria-label="Agents">
      <div className="ab-head">
        <h2>Agents</h2>
        <div className="ab-chips" role="group" aria-label="Filter agents">
          {(["all", "working", "free", "silent"] as const).map((f) => (
            <button key={f} type="button" className="ab-chip" aria-pressed={filter === f} onClick={() => setFilter(f)}>
              {`${f === "all" ? "All" : PILL[f]} ${count(f)}`}
            </button>
          ))}
        </div>
      </div>
      <ul className="ab-rows" aria-label="Agent board">
        {shown.map((r) => {
          const full = r.doing ? r.doing : r.stage ? `${STAGE[r.stage]}${r.pr ? ` PR #${r.pr}` : ""}${r.title ? ` ${r.title}` : ""}` : "—";
          // The pill carries the plain status; a longer label (stale claim · #549, awaiting review #N) is a chip.
          const tag = r.label && r.label !== PILL[r.status] ? r.label : null;
          const what = tag && full.startsWith(tag) ? full.slice(tag.length).trim() : full;
          const bot = agents.find((b) => b.name === r.name);
          return (
            <li key={bot ? `bot:${bot.key}` : `lane:${r.name}`} className="ab-row">
              <span className="ab-pill" data-status={r.status}>{PILL[r.status]}</span>
              <span className="ab-name">{r.name}</span>
              <span className="ab-what">
                {tag && <span className="ab-tag">{tag.split(" · ").map((part, i) => <span key={i}>{i > 0 ? ` · ${part}` : part}</span>)}</span>}
                {what.length > 0 && <span className="ab-doing" title={what}>{r.href ? <a href={r.href} target="_blank" rel="noreferrer">{what}</a> : what}</span>}
              </span>
              <span className="ab-for">{(r.latest ?? "—").split(" · ").map((part, i) => <span key={i}>{i > 0 ? ` · ${part}` : part}</span>)}</span>
              <span className="ab-last" title={r.lastDelivered ? `last delivered ${r.lastDelivered}` : "nothing delivered yet"}>{r.lastDelivered ? `last delivered ${r.lastDelivered}` : "nothing delivered yet"}</span>
              {bot && (onOpenBotDetails || onNameBot) && (
                <span className="ab-acts">
                  {onOpenBotDetails && <button type="button" className="ab-act" onClick={() => onOpenBotDetails(bot.key)}>Details</button>}
                  {onNameBot && nameable(bot) && <NameBot bot={bot} onSave={onNameBot} />}
                </span>
              )}
            </li>
          );
        })}
      </ul>
    </section>
  );
}

/** #636: one chip per build-slot root: the holder and the queue, or "queue unavailable" (amber). */
export function SlotStrip({ slots }: { slots: SlotView[] }) {
  if (slots.length === 0) return null;
  return (
    <section className="ab-slots" aria-label="Build slots">
      <span className="ab-slots-cap">Build slots</span>
      {slots.map((s) => (
        <span key={s.root} className="ab-slot" data-state={s.ok ? (s.holder ? "busy" : "idle") : "unavailable"}>
          <span className="ab-slot-root">{s.root}</span>
          {!s.ok ? <span>queue unavailable</span> : <>
            <span>{s.holder ? `${s.holder.lane} building · ${slotDuration(s.holder.heldSeconds)}` : "free"}</span>
            {s.waiting.length > 0 && <span className="ab-slot-queue">{`queue: ${s.waiting.map((w) => w.lane).join(", ")}`}</span>}
          </>}
        </span>
      ))}
    </section>
  );
}

/** #591: sub-row geometry; a bar narrower than MIN_LABEL_PX on a 1000px track draws no text. */
export const SUB_H = 18, SUB_GAP = 2, MIN_LABEL_PX = 44;
const TRACK_PAD = 3;

/** The drawn width of a lane track, so a bar knows whether its label fits; 1000px until measured. */
function useTrackWidth(ref: RefObject<HTMLElement | null>): number {
  const [w, setW] = useState(1000);
  useEffect(() => {
    const track = ref.current?.querySelector(".lt-track");
    if (!track || typeof ResizeObserver === "undefined") return;
    const measure = () => { const x = track.getBoundingClientRect().width; if (x > 0) setW(x); };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(track);
    return () => ro.disconnect();
  }, [ref]);
  return w;
}

export function LanesTimeline({ lanes, now, windowMs, tasks = [], agents = [], slots = [], onOpenBotDetails, onNameBot }: Props) {
  const listRef = useRef<HTMLUListElement>(null);
  const trackW = useTrackWidth(listRef);
  lanes = lanes.filter((l) => !placeholderLane(l.lane));
  if (lanes.length === 0 && agents.length === 0) return <>{<SlotStrip slots={slots} />}<p className="lt-empty">No agent has recorded work in this window</p></>;
  const board = agentBoard(agents, lanes, tasks, now, slots);
  const rank = new Map<string, number>();
  board.forEach((r, i) => rank.set(r.name, i));
  const botOf = (l: Lane) => agents.find((b) => b.actorId === l.lane || b.key === l.lane)?.name;
  const at = (l: Lane) => rank.get(l.lane) ?? rank.get(botOf(l) ?? "") ?? board.length;
  lanes = [...lanes].sort((a, b) => at(a) - at(b));
  const from = now - windowMs;
  const hours = Math.round(windowMs / 3_600_000);
  const step = hours > 8 ? 2 : 1;
  const ticks: string[] = [];
  for (let h = hours; h > 0; h -= step) ticks.push(`−${h}h`);
  const delivered = lastDelivered(tasks);
  // #591: free hands are the board's Free rows: no open slice, no PR awaiting review, no review owed.
  const cards = { ...laneCards(lanes, tasks), free: board.filter((r) => r.status === "free").map((r) => r.name) };
  const card = (items: string[]) => (items.length ? items.join(", ") : "none right now");
  return (
    <div className="lt-wrap">
    <SlotStrip slots={slots} />
    <AgentBoard rows={board} agents={agents} {...(onOpenBotDetails ? { onOpenBotDetails } : {})} {...(onNameBot ? { onNameBot } : {})} />
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
      <ul className="lt-lanes" aria-label="Agent lanes" ref={listRef}>
        {lanes.map((l) => {
          const open = l.bars.some((b) => b.open);
          const pack = packBars(l.bars);
          const trackH = pack.rows * SUB_H + (pack.rows - 1) * SUB_GAP + TRACK_PAD * 2;
          return (
            <li key={l.lane} className="lt-row lt-lane" style={{ height: Math.max(44, trackH + 14) }}>
              <span className="lt-who">
                <span className="lt-dot" data-state={l.silent ? "silent" : open ? "busy" : "idle"} />
                <span className="lt-name">{botOf(l) ?? l.lane}</span>
                {l.silent ? <span className="lt-flag" data-flag="silent">silent</span> : !open && <span className="lt-flag" data-flag="free">free</span>}
              </span>
              <div className="lt-track" style={{ height: trackH }} data-subrows={pack.rows}>
                <span className="lt-nowline" aria-hidden="true" />
                {l.bars.map((b, i) => {
                  const text = `${b.label} ${b.kind}`;
                  const frac = (b.end - b.start) / windowMs;
                  return (
                    <div key={i} className="lt-bar" data-kind={b.kind} data-silent={l.silent && b.open} data-subrow={pack.row[i]}
                      title={`${text} · ${stageDuration(b.end - b.start)}${b.open ? " so far" : ""}`}
                      style={{ left: pct((b.start - from) / windowMs), width: pct(frac), top: TRACK_PAD + pack.row[i]! * (SUB_H + SUB_GAP), height: SUB_H }}>
                      {frac * trackW >= MIN_LABEL_PX && <span>{text}</span>}
                    </div>
                  );
                })}
              </div>
              <span className="lt-last" title={delivered.get(l.lane)}>{delivered.get(l.lane) ?? "—"}</span>
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
    </div>
  );
}
