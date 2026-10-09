import type { Bot } from "./team";
import type { Lane, LaneBar } from "./lane-bars";
import type { MissionTask } from "./mission";

/** #591 Lanes board: who is free, who does what, and for how long. */
export type AgentStatus = "silent" | "working" | "waiting" | "free";
export interface AgentRow {
  name: string;
  status: AgentStatus;
  /** The open bar's stage, absent when free. */
  stage: LaneBar["kind"] | null;
  pr: number | null;
  title: string | null;
  /** Link to the PR when the task knows its repository. */
  href: string | null;
  /** ms in the current stage, or ms free since the last bar ended; null when unknown. */
  forMs: number | null;
  lastDelivered: string | null;
  /** Pill text override: "Reviewing" for a lane busy on an open review. */
  label?: string | null;
  /** Full "what" text override, e.g. "reviewing #7 Add board". */
  doing?: string | null;
}

const ORDER: Record<AgentStatus, number> = { silent: 0, working: 1, waiting: 2, free: 3 };

function laneFor(b: Bot, lanes: Lane[]): Lane | undefined {
  return lanes.find((l) => l.lane === b.name || l.lane === b.actorId || l.lane === b.key);
}

const blank = (n: string | undefined | null) => !n || n.trim() === "" || n.trim().toLowerCase() === "tbd";

/** A verdict on the task's current head: a BLOCK round on it, or the unanswered BLOCK on it. */
function hasVerdictOnHead(t: MissionTask, names: string[]): boolean {
  if (!t.headSha) return false;
  if (t.rounds.some((r) => names.includes(r.reviewer) && r.headSha === t.headSha)) return true;
  return !!t.blockedBy && names.includes(t.blockedBy.reviewer) && t.blockedBy.headSha === t.headSha;
}

/** One row per bot, plus a row per lane no bot claims; silent first, then working (longest
 * first), then waiting, then free. Silent is the lane-bars STALL rule (`Lane.silent`). */
export function agentBoard(bots: Bot[], lanes: Lane[], tasks: MissionTask[], now: number): AgentRow[] {
  bots = bots.filter((b) => !blank(b.name));
  lanes = lanes.filter((l) => !blank(l.lane));
  const used = new Set<Lane>();
  const pairs: { name: string; lane: Lane | undefined }[] = bots.map((b) => {
    const lane = laneFor(b, lanes);
    if (lane) used.add(lane);
    return { name: b.name, lane };
  });
  for (const l of lanes) if (!used.has(l)) pairs.push({ name: l.lane, lane: l });
  const delivered = new Map<string, string>();
  for (const t of tasks) if (t.step === "merged" && t.lane) delivered.set(t.lane, `${t.pr ? `#${t.pr} ` : ""}${t.title}`);
  const rows = pairs.map(({ name, lane }): AgentRow => {
    const ids = [name, lane?.lane].filter((n): n is string => !!n);
    const openBars = (lane?.bars ?? []).filter((b) => b.open);
    if (!openBars.some((b) => b.kind === "implement")) {
      const rt = tasks.find((t) => t.step !== "merged" && t.step !== "merge" && t.pr !== null
        && !hasVerdictOnHead(t, ids)
        && (t.reviewers.some((r) => ids.includes(r)) || openBars.some((b) => b.kind === "review" && ((b.taskId !== undefined && b.taskId === t.key) || b.label === `#${t.pr}`))));
      if (rt) {
        const rb = openBars.find((b) => b.kind === "review" && ((b.taskId !== undefined && b.taskId === rt.key) || b.label === `#${rt.pr}`)) ?? null;
        const since = rb ? (rb.since ?? rb.start) : null;
        const last0 = delivered.get(name) ?? (lane ? delivered.get(lane.lane) : undefined) ?? null;
        return {
          name, status: "waiting", stage: "review", pr: rt.pr, title: rt.title, label: "Reviewing", doing: `reviewing #${rt.pr} ${rt.title}`,
          href: rt.repoUrl ? `${rt.repoUrl}/pull/${rt.pr}` : null, forMs: since === null ? null : Math.max(0, now - since), lastDelivered: last0,
        };
      }
    }
    const bar = openBars.reduce<LaneBar | null>((a, b) => (a === null || (b.since ?? b.start) > (a.since ?? a.start) ? b : a), null);
    const last = lane ? delivered.get(lane.lane) ?? delivered.get(name) ?? null : delivered.get(name) ?? null;
    if (!bar) {
      const ended = (lane?.bars ?? []).reduce((m, b) => Math.max(m, b.end), 0) || lane?.lastEventAt || 0;
      return { name, status: "free", stage: null, pr: null, title: null, href: null, forMs: ended > 0 ? Math.max(0, now - ended) : null, lastDelivered: last };
    }
    const task = tasks.find((t) => (bar.taskId !== undefined && t.key === bar.taskId) || (t.pr !== null && bar.label === `#${t.pr}`)) ?? null;
    const pr = task?.pr ?? (bar.label.startsWith("#") ? Number(bar.label.slice(1)) : null);
    const status: AgentStatus = lane!.silent ? "silent" : bar.kind === "implement" ? "working" : "waiting";
    return {
      name, status, stage: bar.kind, pr, title: task?.title ?? null,
      href: task?.repoUrl && pr ? `${task.repoUrl}/pull/${pr}` : null,
      forMs: Math.max(0, now - (bar.since ?? bar.start)), lastDelivered: last,
    };
  });
  return rows.sort((a, b) => ORDER[a.status] - ORDER[b.status]
    || (a.status === "working" ? (b.forMs ?? 0) - (a.forMs ?? 0) : 0)
    || a.name.localeCompare(b.name));
}

/** `1h 17m`, `23m`, `<1m`. */
export function span(ms: number): string {
  const m = Math.floor(ms / 60_000);
  if (m < 1) return "<1m";
  const h = Math.floor(m / 60);
  return h > 0 ? `${h}h ${m % 60}m` : `${m}m`;
}
