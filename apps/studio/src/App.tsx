import { useCallback, useEffect, useMemo, useState } from "react";

import { addFolder, login, servedProjects, type Session } from "./login/session";
import { parseRoute, routeSearch, type Route, type View } from "./routing/route";
import { Runtime, type JourneyFlow, type RunSummary } from "./runtime/client";
import { Sidebar, type ProjectEntry } from "./sidebar/Sidebar";
import { useSummaries } from "./sidebar/use-summaries";
import { teamRun } from "./sidebar/project-summary";
import { StartRun } from "./start-run/StartRun";
import { Topbar } from "./topbar/Topbar";
import { ProofView } from "./proof/ProofView";
import { LanesView } from "./lanes/LanesView";
import { GraphView } from "./views/Views";
import { RequestsView } from "./graph/RequestsView";
import { ViewBoundary } from "./views/ViewBoundary";

/** How often the run list is read again: one page of at most 50 rows per poll. */
const POLL_MS = 5_000;

const NO_SESSIONS: Session[] = [];

type Login = { state: "pending" } | { state: "refused" } | { state: "in"; sessions: Session[]; closed: string[] };

/** The joined projects this tab holds no key for: listed, not openable here. */
function closedProjects(sessions: Session[], served: { first: string | null; all: string[] } | null): string[] {
  if (served === null) return [];
  const open = new Set(sessions.map((session) => (session.key === "first" ? served.first : session.key)));
  return served.all.filter((id) => !open.has(id));
}

function clock(date: Date): string {
  return `${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`;
}

export function App() {
  const [auth, setAuth] = useState<Login>({ state: "pending" });
  const [route, setRoute] = useState<Route>(() => parseRoute(location.search));

  useEffect(() => {
    let live = true;
    void login(location.search).then(async (sessions) => {
      const served = sessions === null ? null : await servedProjects();
      if (!live) return;
      setAuth(sessions === null ? { state: "refused" } : { state: "in", sessions, closed: closedProjects(sessions, served) });
      // The ticket is spent; it does not stay in the address bar or the history. The URL keeps the
      // project the ticket opened, so a reload lands on it.
      const landed = parseRoute(location.search);
      const next = sessions !== null && landed.project === null && sessions[0]!.key !== "first" ? { ...landed, project: sessions[0]!.key } : landed;
      history.replaceState(null, "", `${location.pathname}${routeSearch(next)}`);
      setRoute(next);
    });
    return () => { live = false; };
  }, []);

  useEffect(() => {
    const onPop = () => setRoute(parseRoute(location.search));
    addEventListener("popstate", onPop);
    return () => removeEventListener("popstate", onPop);
  }, []);

  const go = useCallback((next: Route) => {
    setRoute(next);
    history.pushState(null, "", `${location.pathname}${routeSearch(next)}`);
  }, []);

  const sessions = auth.state === "in" ? auth.sessions : NO_SESSIONS;
  const summaries = useSummaries(sessions);

  if (auth.state === "pending") return <main className="gate"><p>Logging in…</p></main>;
  if (auth.state === "refused") {
    return (
      <main className="gate">
        <h1>Open the Studio from your project</h1>
        <p>This page has no login ticket, or its ticket was already used. Run <code className="mono">graphhelm studio open</code> in the project folder to open a logged-in Studio.</p>
      </main>
    );
  }
  const session = auth.sessions.find((known) => known.key === route.project) ?? auth.sessions[0]!;
  const projects: ProjectEntry[] = auth.sessions.map((known) => ({
    key: known.key, name: known.project ?? "this project", folder: known.folder, summary: summaries[known.key] ?? null,
  }));
  // Keyed by project: switching starts that project's own reads from empty, never the last one's rows.
  return <Shell key={session.key} session={session} projects={projects} closed={auth.closed} route={{ ...route, project: session.key }} go={go} />;
}

interface ShellProps {
  session: Session;
  projects: ProjectEntry[];
  closed: string[];
  route: Route;
  go: (route: Route) => void;
}

function Shell({ session, projects, closed, route, go }: ShellProps) {
  const runtime = useMemo(() => new Runtime(session.token, session.base), [session.token, session.base]);
  const [runs, setRuns] = useState<RunSummary[] | null>(null);
  const [live, setLive] = useState(false);
  const [updated, setUpdated] = useState<string | null>(null);
  const [journeys, setJourneys] = useState<JourneyFlow[] | null>(null);
  const [journeysError, setJourneysError] = useState<string | null>(null);
  const [composing, setComposing] = useState(false);

  const readRuns = useCallback(async () => {
    try {
      setRuns(await runtime.runs());
      setLive(true);
      setUpdated(clock(new Date()));
    } catch {
      setLive(false);
    }
  }, [runtime]);

  useEffect(() => {
    void readRuns();
    const timer = setInterval(() => void readRuns(), POLL_MS);
    return () => clearInterval(timer);
  }, [readRuns]);

  const readJourneys = useCallback(() => {
    runtime.journeys().then((flows) => { setJourneys(flows); setJourneysError(null); }, (error: unknown) => setJourneysError(error instanceof Error ? error.message : "The journeys could not be read."));
  }, [runtime]);

  useEffect(() => {
    // Proof lists the journeys; Lanes and Graph read their last result beside a slice's journeys.
    if (journeys !== null) return;
    readJourneys();
  }, [route.view, journeys, readJourneys]);

  const project = session.project ?? "this project";
  // #1901: one project, one team run. The URL's ?run= still deep-links; without it the Studio
  // opens the run team.json marks, else the most recently active one, and the URL stays clean.
  const runId = route.run ?? (runs === null ? null : teamRun(runs, session.teamRun)?.executionId ?? null);
  const run = runs?.find((row) => row.executionId === runId) ?? null;

  return (
    <div className="shell">
      <Sidebar
        projects={projects}
        closed={closed}
        current={session.key}
        view={route.view}
        live={live}
        onProject={(key) => go({ project: key, run: null, view: route.view })}
        onAdd={session.sessionKey === null ? null : async (path, ticket) => {
          const added = await addFolder(path, session.sessionKey!, ticket);
          if ("refused" in added) return added.refused;
          // The new project's key is in this tab's keys now; a fresh login opens it.
          location.assign(`${location.pathname}${routeSearch({ project: added.projectId, run: null, view: route.view })}`);
          return null;
        }}
      />
      <div className="main">
        <Topbar
          project={project}
          run={runId}
          view={route.view}
          updated={live ? updated : null}
          onView={(view: View) => go({ ...route, view })}
          onStart={runs !== null && runId === null && !(composing && route.view === "graph") ? () => { setComposing(true); go({ ...route, view: "graph" }); } : null}
        />
        <ViewBoundary key={`${route.view}:${runId ?? ""}`} name={route.view}>
        {runs === null && route.run === null ? <section className="view" aria-label="Reading"><p>Reading the project's team run…</p></section> : <>
        {route.view === "graph" && (
          <GraphView run={run} runId={runId} requests={runId === null ? undefined : <RequestsView runtime={runtime} executionId={runId} journeys={journeys} onOpenProof={(journey) => go({ project: route.project, run: route.run, view: "proof", journey })} />}>
            {composing && runId === null && (
              <StartRun
                runtime={runtime}
                onCancel={() => setComposing(false)}
                onStarted={(id) => {
                  setComposing(false);
                  go({ project: route.project, run: id, view: "graph" });
                  void readRuns();
                }}
              />
            )}
          </GraphView>
        )}
        {route.view === "lanes" && <LanesView runtime={runtime} executionId={runId} journeys={journeys} />}
        {route.view === "proof" && <ProofView runtime={runtime} journeys={journeys} error={journeysError} executionId={runId} onRan={readJourneys} initial={route.journey ?? null} />}
        </>}
        </ViewBoundary>
      </div>
    </div>
  );
}
