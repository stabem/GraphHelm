import type { JourneyFlow, LaneLiveness, RunSummary } from "../runtime/client";

/** What the sidebar says under one project (Sidebar.dc.html): its active run with how many agents
 * work it, and how many journeys it has with how many fail. Every field comes from that project's
 * own public Runtime routes; `null` is "not read", never zero. */
export interface ProjectSummary {
  activeRun: string | null;
  agents: number | null;
  journeys: number | null;
  failing: number | null;
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

/** A journey fails when the Runtime cannot read it or reports an error finding on it. */
export function failingJourneys(flows: JourneyFlow[]): number {
  return flows.filter((flow) => flow.status === "unreadable" || (flow.findings ?? []).some((finding) => finding.severity === "error")).length;
}

/** The subtitle lines: `● <run> · N agents` (or `no run`) and `N journeys · M failing`. */
export function subtitle(summary: ProjectSummary): { run: string; journeys: string | null } {
  const run = summary.activeRun === null ? "no run"
    : `● ${summary.activeRun}${summary.agents === null ? "" : ` · ${summary.agents} ${summary.agents === 1 ? "agent" : "agents"}`}`;
  const journeys = summary.journeys === null ? null
    : `${summary.journeys} ${summary.journeys === 1 ? "journey" : "journeys"}${summary.failing ? ` · ${summary.failing} failing` : ""}`;
  return { run, journeys };
}
