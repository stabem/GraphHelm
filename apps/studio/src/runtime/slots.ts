/* #636: the build-slot queues (`GET /v1/workspaces/slots`, #612) as the Studio reads them: which lane
 * holds a slot (building) and which lanes wait for one, in serving order. Pure. */

export interface SlotHolder { lane: string; label: string | null; pid: number | null; worktree: string | null; heldSeconds: number }
export interface SlotWaiter { lane: string; label: string | null; pid: number | null; worktree: string | null; priority: number | null; waitedSeconds: number; ticket: string | null }
/** A slot the Runtime read, or one it could not (`error` set: the queue is unavailable, never empty). */
export type SlotView =
  | { root: string; ok: true; holder: SlotHolder | null; waiting: SlotWaiter[] }
  | { root: string; ok: false; errorCodes: string[] };

export type SlotStatus =
  | { kind: "building"; root: string; seconds: number }
  | { kind: "waiting"; root: string; position: number; seconds: number };

const obj = (v: unknown): Record<string, unknown> | null => (v !== null && typeof v === "object" && !Array.isArray(v) ? v as Record<string, unknown> : null);
const str = (v: unknown): string | null => (typeof v === "string" && v.trim() !== "" ? v : null);
const num = (v: unknown): number | null => (typeof v === "number" && Number.isFinite(v) ? v : null);
const secs = (v: unknown): number => Math.max(0, num(v) ?? 0);

function holder(v: unknown): SlotHolder | null {
  const o = obj(v);
  const lane = o && str(o.lane);
  if (!o || !lane) return null;
  return { lane, label: str(o.label), pid: num(o.pid), worktree: str(o.worktree), heldSeconds: secs(o.heldSeconds) };
}

function waiter(v: unknown): SlotWaiter | null {
  const o = obj(v);
  const lane = o && str(o.lane);
  if (!o || !lane) return null;
  return { lane, label: str(o.label), pid: num(o.pid), worktree: str(o.worktree), priority: num(o.priority), waitedSeconds: secs(o.waitedSeconds), ticket: str(o.ticket) };
}

/** Parses the route's `data` (or the whole envelope) defensively: entries without a root are
 * dropped; an entry with `error`, or without a `waiting` list, reads as unavailable. */
export function parseSlots(data: unknown): SlotView[] {
  let d = obj(data);
  if (d && obj(d.data)) d = obj(d.data);
  const list = d && Array.isArray(d.slots) ? d.slots : [];
  const out: SlotView[] = [];
  for (const raw of list) {
    const o = obj(raw);
    const root = o && str(o.root);
    if (!o || !root) continue;
    if (o.error !== undefined || !Array.isArray(o.waiting)) {
      const codes = Array.isArray(o.error) ? o.error.map((e) => str(obj(e)?.code)).filter((c): c is string => c !== null) : [];
      out.push({ root, ok: false, errorCodes: codes });
      continue;
    }
    out.push({ root, ok: true, holder: holder(o.holder), waiting: o.waiting.map(waiter).filter((w): w is SlotWaiter => w !== null) });
  }
  return out;
}

/** The lane's place in the queues: building when it holds a slot, else waiting (1-based position in
 * serving order) on the first slot that queues it; null when it is in no queue. */
export function slotStatus(lane: string | null | undefined, slots: SlotView[]): SlotStatus | null {
  if (!lane) return null;
  for (const s of slots) if (s.ok && s.holder?.lane === lane) return { kind: "building", root: s.root, seconds: s.holder.heldSeconds };
  for (const s of slots) {
    if (!s.ok) continue;
    const i = s.waiting.findIndex((w) => w.lane === lane);
    if (i >= 0) return { kind: "waiting", root: s.root, position: i + 1, seconds: s.waiting[i]!.waitedSeconds };
  }
  return null;
}

/** The roots whose queue could not be read: shown as "queue unavailable", never as empty. */
export const unavailableRoots = (slots: SlotView[]): string[] => slots.filter((s) => !s.ok).map((s) => s.root);

/** 1st, 2nd, 3rd, 4th, 11th, 21st. */
export function ordinal(n: number): string {
  const t = n % 100;
  if (t >= 11 && t <= 13) return `${n}th`;
  return `${n}${["th", "st", "nd", "rd"][n % 10] ?? "th"}`;
}

const mins = (s: number) => (s < 60 ? `${Math.floor(s)}s` : s < 3600 ? `${Math.floor(s / 60)}m` : `${Math.floor(s / 3600)}h ${Math.floor((s % 3600) / 60)}m`);
/** "building · 4m (D:/gh)" or "waiting for build · 2nd · 12m". */
export function slotText(s: SlotStatus): string {
  return s.kind === "building" ? `building · ${mins(s.seconds)} (${s.root})` : `waiting for build · ${ordinal(s.position)} · ${mins(s.seconds)}`;
}
export { mins as slotDuration };
