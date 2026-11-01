import type { JourneyFlow, LaneLiveness, RunSummary } from "../runtime/client";
import { WAITING_FOR_YOU, waitsForYou } from "../views/attention";

/** What the sidebar says under one project (Sidebar.dc.html): its active run with how many agents
 * work it, and how many journeys it has with how many are to fix. Every field comes from that
 * project's own public Runtime routes; `null` is "not read", never zero. */
export interface ProjectSummary {
  activeRun: string | null;
  agents: number | null;
  journeys: number | null;
  toFix: number | null;
  /** #2341: the active run waits for its owner (`needs_you`). Absent or false: nothing is said. */
  waiting?: boolean;
}

/** When a run last moved: its last event, else its start; `-Infinity` when it carries neither. */
function activity(run: RunSummary): number {
  const at = Date.parse(run.lastEventAt ?? run.startedAt ?? "");
  return Number.isNaN(at) ? -Infinity : at;
}

/** #1901: the project's team run, the one the Studio opens. The run `.graphhelm/team.json` marks
 * when the Runtime lists it, else the most recently active running run, else the most recently
 * active run; `null` when the project has no run. Ties keep the list's (newest first) order. */
export function teamRun(runs: readonly RunSummary[], marked: string | null = null): RunSummary | null {
  const named = marked === null ? undefined : runs.find((run) => run.executionId === marked);
  if (named !== undefined) return named;
  const newest = (rows: readonly RunSummary[]) => rows.reduce<RunSummary | null>((best, run) => (best === null || activity(run) > activity(best) ? run : best), null);
  return newest(runs.filter((run) => run.status === "running")) ?? newest(runs);
}

/** The run the sidebar names: the team run while it is running, else none. */
export function activeRun(runs: readonly RunSummary[], marked: string | null = null): string | null {
  const run = teamRun(runs, marked);
  return run !== null && run.status === "running" ? run.executionId : null;
}

/** Lanes the Runtime does not judge free (#1234) are the agents working the run. */
export function agentCount(lanes: LaneLiveness[]): number {
  return new Set(lanes.filter((lane) => !lane.free).map((lane) => lane.lane)).size;
}

/** #2325: journeys to fix: the Runtime cannot read them or reports an error finding on them. Not a
 * failed run (Proof counts those from `lastRun`); Proof's card chip says "N to fix" for the same ones. */
export function journeysToFix(flows: JourneyFlow[]): number {
  return flows.filter(journeyToFix).length;
}

/** One journey to fix. The rail counts these and Proof gives each a "to fix" chip, both through
 * this one predicate, so the two never disagree (an unreadable journey with no findings included). */
export function journeyToFix(flow: JourneyFlow): boolean {
  return flow.status === "unreadable" || (flow.findings ?? []).some((finding) => finding.severity === "error");
}

/** #2341: whether the project's active run waits for its owner, from the run list the sidebar read. */
export function activeRunWaits(runs: readonly RunSummary[], active: string | null): boolean {
  const run = active === null ? undefined : runs.find((row) => row.executionId === active);
  return run !== undefined && waitsForYou(run.status, run.attention);
}

/** The subtitle lines: `● <run> · N agents` (or `no run`), `waiting for you` when that run waits
 * (#2341), and `N journeys · M to fix`. */
export function subtitle(summary: ProjectSummary): { run: string; waits: string | null; journeys: string | null } {
  const run = summary.activeRun === null ? "no run"
    : `● ${summary.activeRun}${summary.agents === null ? "" : ` · ${summary.agents} ${summary.agents === 1 ? "agent" : "agents"}`}`;
  const journeys = summary.journeys === null ? null
    : `${summary.journeys} ${summary.journeys === 1 ? "journey" : "journeys"}${summary.toFix ? ` · ${summary.toFix} to fix` : ""}`;
  return { run, waits: summary.activeRun !== null && summary.waiting === true ? WAITING_FOR_YOU : null, journeys };
}
