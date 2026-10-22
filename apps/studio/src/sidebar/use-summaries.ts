import { useEffect, useState } from "react";

import type { Session } from "../login/session";
import { Runtime } from "../runtime/client";
import { activeRun, agentCount, failingJourneys, type ProjectSummary } from "./project-summary";

/** How often every project's subtitle is read again: at most three reads per project. */
const SUMMARY_MS = 15_000;

async function summaryOf(session: Session): Promise<ProjectSummary> {
  const runtime = new Runtime(session.token, session.base);
  const [runs, flows] = await Promise.allSettled([runtime.runs(), runtime.journeys()]);
  const run = runs.status === "fulfilled" ? activeRun(runs.value, session.teamRun) : null;
  let agents: number | null = null;
  if (run !== null) {
    try { agents = agentCount(await runtime.lanes(run)); } catch { /* not read */ }
  }
  return {
    activeRun: run,
    agents,
    journeys: flows.status === "fulfilled" ? flows.value.length : null,
    failing: flows.status === "fulfilled" ? failingJourneys(flows.value) : null,
  };
}

/** Each opened project's summary, each read from that project's own Runtime with its own token. */
export function useSummaries(sessions: Session[]): Record<string, ProjectSummary> {
  const [summaries, setSummaries] = useState<Record<string, ProjectSummary>>({});
  useEffect(() => {
    let live = true;
    const read = () => {
      for (const session of sessions) {
        void summaryOf(session).then((summary) => {
          if (live) setSummaries((prior) => ({ ...prior, [session.key]: summary }));
        });
      }
    };
    read();
    const timer = setInterval(read, SUMMARY_MS);
    return () => { live = false; clearInterval(timer); };
  }, [sessions]);
  return summaries;
}
