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

/** The newest running run (the list is newest first), or null. */
export function activeRun(runs: RunSummary[]): string | null {
  return runs.find((run) => run.status === "running")?.executionId ?? null;
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
