import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { placeholderLane, type Lane } from "../runtime/lane-bars";

/** The one payload the Studio sends to a lane: an owner note on the run (client.signal). */
export interface LaneNote { type: "operator_note"; to: string; description: string }
export interface RosterLane { name: string; silent: boolean }

/** #591: the lanes the owner can hand work to: the team's agents plus every lane with records,
 * "TBD" and empty names dropped, silent lanes marked. */
export function laneRoster(agentNames: readonly string[], lanes: readonly Lane[]): RosterLane[] {
  const out = new Map<string, RosterLane>();
  for (const name of agentNames) if (!placeholderLane(name)) out.set(name.trim(), { name: name.trim(), silent: false });
  for (const l of lanes) {
    if (placeholderLane(l.lane)) continue;
    const name = l.lane.trim();
    out.set(name, { name, silent: l.silent || (out.get(name)?.silent ?? false) });
  }
  return [...out.values()];
}

const ago = (ms: number) => `${Math.max(0, Math.floor(ms / 60_000))} min ago`;

/**
 * #591: Ask for status and Hand to… for the lane that owns the current step. Both only post owner
 * notes on the run. Handing off deliberately writes NO task record (no `review_assigned`): the
 * coordinator reads the notes and records the hand-off itself; the Studio never writes a lane's
 * task history. `askedAt` lives in the caller so it survives selecting another PR.
 */
export function LaneActions({ lane, step, pr, roster, lastSeenAt, now, askedAt, onAsked, send }: {
  lane: string; step: string; pr: number | null; roster: RosterLane[]; lastSeenAt: number | null; now: number;
  askedAt: number | null; onAsked(at: number): void;
  send(note: LaneNote): Promise<unknown>;
}) {
  const [error, setError] = useState<string | null>(null);
  const [menuOpen, setMenuOpen] = useState(false);
  const [target, setTarget] = useState<string | null>(null);
  const [requested, setRequested] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const handRef = useRef<HTMLButtonElement>(null);
  const itemsRef = useRef<(HTMLButtonElement | null)[]>([]);
  const prText = pr !== null ? `PR #${pr}` : "this work";
  const run = async (notes: LaneNote[], done: () => void) => {
    setBusy(true);
    setError(null);
    try {
      for (const n of notes) await send(n);
      done();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };
  const answered = askedAt !== null && lastSeenAt !== null && lastSeenAt > askedAt;
  const choices = roster.filter((r) => r.name !== lane);
  const ask = () => {
    const at = now;
    void run([{ type: "operator_note", to: lane, description: `Owner asks: status of ${step} on ${prText}?` }], () => onAsked(at));
  };
  const handOff = () => {
    const to = target!;
    void run([
      { type: "operator_note", to, description: `Take over ${step} on ${prText} from ${lane}` },
      { type: "operator_note", to: lane, description: `Hand ${step} on ${prText} to ${to}` },
    ], () => { setRequested(to); setTarget(null); });
  };
  useEffect(() => { if (menuOpen) itemsRef.current[0]?.focus(); }, [menuOpen]);
  const closeMenu = () => { setMenuOpen(false); handRef.current?.focus(); };
  const onMenuKey = (e: KeyboardEvent<HTMLDivElement>) => {
    const items = itemsRef.current.filter((x): x is HTMLButtonElement => x !== null);
    const i = items.indexOf(document.activeElement as HTMLButtonElement);
    const go = (n: number) => { e.preventDefault(); items[(n + items.length) % items.length]?.focus(); };
    if (e.key === "ArrowDown") go(i + 1);
    else if (e.key === "ArrowUp") go(i - 1);
    else if (e.key === "Home") go(0);
    else if (e.key === "End") go(items.length - 1);
    else if (e.key === "Escape" || e.key === "Tab") { e.preventDefault(); closeMenu(); }
  };
  const pick = (name: string) => { setTarget(name); setRequested(null); closeMenu(); };
  return (
    <div className="mg-section mg-lane-actions">
      <span className="mg-cap">Owner</span>
      <div className="mg-actions">
        {askedAt !== null && !answered ? <span className="mg-muted">{`Asked ${ago(now - askedAt)} · no answer yet`}</span> : (
          <button type="button" className="mg-secondary" disabled={busy} onClick={ask}>{`Ask ${lane} for status`}</button>
        )}
        <span className="mg-menu-anchor">
          <button ref={handRef} type="button" className="mg-secondary" aria-haspopup="menu" aria-expanded={menuOpen} disabled={busy || choices.length === 0}
            onClick={() => (menuOpen ? closeMenu() : setMenuOpen(true))}>Hand to…</button>
          {menuOpen && (
            <div role="menu" aria-label={`Hand ${step.toLowerCase()} to`} className="mg-menu" onKeyDown={onMenuKey}>
              {choices.map((r, i) => (
                <button key={r.name} ref={(el) => { itemsRef.current[i] = el; }} type="button" role="menuitem" tabIndex={-1} className="mg-menu-item"
                  data-silent={r.silent} onClick={() => pick(r.name)}>
                  {r.silent ? <>{r.name}<span className="mg-menu-flag">{" · silent"}</span></> : r.name}
                </button>
              ))}
            </div>
          )}
        </span>
      </div>
      {answered && <p className="mg-muted">{`Answered ${ago(now - lastSeenAt!)}`}</p>}
      {target && (
        <div className="mg-confirm" role="group" aria-label="Confirm hand-off">
          <span>{`Hand ${step.toLowerCase()} of ${prText} to ${target}?`}</span>
          <span className="mg-actions">
            <button type="button" className="mg-primary" disabled={busy} onClick={handOff}>Confirm</button>
            <button type="button" className="mg-secondary" disabled={busy} onClick={() => setTarget(null)}>Cancel</button>
          </span>
        </div>
      )}
      {requested && <p className="mg-muted">{`Hand-off to ${requested} requested`}</p>}
      {error && <p role="alert" className="mg-note">{error}</p>}
    </div>
  );
}
