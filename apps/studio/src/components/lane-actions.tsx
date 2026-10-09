import { useState } from "react";
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

const minutes = (ms: number) => `${Math.max(0, Math.floor(ms / 60_000))}m`;

/**
 * #591: Nudge and Reassign for the lane that owns the current step. Both only post owner notes on
 * the run. Reassign deliberately writes NO task record (no `review_assigned`): the coordinator reads
 * the notes and records the reassignment itself; the Studio never writes a lane's task history.
 */
export function LaneActions({ lane, step, pr, roster, lastSeenAt, now, send }: {
  lane: string; step: string; pr: number | null; roster: RosterLane[]; lastSeenAt: number | null; now: number;
  send(note: LaneNote): Promise<unknown>;
}) {
  const [nudgedAt, setNudgedAt] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [target, setTarget] = useState("");
  const [requested, setRequested] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
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
  // The nudge stands until the lane records anything newer than it.
  const nudged = nudgedAt !== null && !(lastSeenAt !== null && lastSeenAt > nudgedAt);
  const choices = roster.filter((r) => r.name !== lane);
  const nudge = () => {
    const at = now;
    void run([{ type: "operator_note", to: lane, description: `Owner asks: status of ${step} on ${prText}?` }], () => setNudgedAt(at));
  };
  const reassign = () => {
    const to = target;
    void run([
      { type: "operator_note", to, description: `Take over ${step} on ${prText} from ${lane}` },
      { type: "operator_note", to: lane, description: `Hand ${step} on ${prText} to ${to}` },
    ], () => setRequested(to));
  };
  return (
    <div className="mg-section mg-lane-actions">
      <span className="mg-cap">Owner</span>
      <div className="mg-actions">
        {nudged ? <span className="mg-muted">{`nudged ${minutes(now - nudgedAt!)} ago`}</span> : (
          <button type="button" className="mg-secondary" disabled={busy} onClick={nudge}>{`Nudge ${lane}`}</button>
        )}
      </div>
      <div className="mg-actions">
        <select aria-label="Reassign to…" value={target} onChange={(e) => setTarget(e.target.value)}>
          <option value="">Reassign to…</option>
          {choices.map((r) => <option key={r.name} value={r.name}>{r.silent ? `${r.name} (silent)` : r.name}</option>)}
        </select>
        <button type="button" className="mg-secondary" aria-label="Confirm reassign" disabled={busy || target === ""} onClick={reassign}>Confirm</button>
      </div>
      {requested && <p className="mg-muted">{`reassign requested to ${requested}`}</p>}
      {error && <p role="alert" className="mg-note">{error}</p>}
    </div>
  );
}
