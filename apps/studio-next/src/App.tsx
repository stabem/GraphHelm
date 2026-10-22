import { useCallback, useEffect, useMemo, useState } from "react";

import { login, type Session } from "./login/session";
import { parseRoute, routeSearch, type Route, type View } from "./routing/route";
import { Runtime, type JourneyFlow, type RunSummary } from "./runtime/client";
import { Sidebar } from "./sidebar/Sidebar";
import { StartRun } from "./start-run/StartRun";
import { Topbar } from "./topbar/Topbar";
import { GraphView, LanesView, ProofView } from "./views/Views";

/** How often the run list is read again: one page of at most 50 rows per poll. */
const POLL_MS = 5_000;

type Login = { state: "pending" } | { state: "refused" } | { state: "in"; sessions: Session[] };

function clock(date: Date): string {
  return `${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`;
}

export function App() {
  const [auth, setAuth] = useState<Login>({ state: "pending" });
  const [route, setRoute] = useState<Route>(() => parseRoute(location.search));

  useEffect(() => {
    let live = true;
    void login(location.search).then((sessions) => {
      if (!live) return;
      setAuth(sessions === null ? { state: "refused" } : { state: "in", sessions });
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
  const projects = auth.sessions.map((known) => ({ key: known.key, name: known.project ?? "this project" }));
  // Keyed by project: switching starts that project's own reads from empty, never the last one's rows.
  return <Shell key={session.key} session={session} projects={projects} route={{ ...route, project: session.key }} go={go} />;
}

interface ShellProps {
  session: Session;
  projects: { key: string; name: string }[];
  route: Route;
  go: (route: Route) => void;
}

function Shell({ session, projects, route, go }: ShellProps) {
  const runtime = useMemo(() => new Runtime(session.token, session.base), [session.token, session.base]);
  const [runs, setRuns] = useState<RunSummary[]>([]);
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

  useEffect(() => {
    if (route.view !== "proof" || journeys !== null) return;
    runtime.journeys().then(setJourneys, (error: unknown) => setJourneysError(error instanceof Error ? error.message : "The journeys could not be read."));
  }, [route.view, runtime, journeys]);

  const project = session.project ?? "this project";
  const run = runs.find((row) => row.executionId === route.run) ?? null;

  return (
    <div className="shell">
      <Sidebar
        projects={projects}
        current={session.key}
        runs={runs}
        selected={route.run}
        view={route.view}
        live={live}
        onProject={(key) => go({ project: key, run: null, view: route.view })}
        onSelect={(id) => go({ ...route, run: id })}
      />
      <div className="main">
        <Topbar
          project={project}
          run={route.run}
          view={route.view}
          updated={live ? updated : null}
          onView={(view: View) => go({ ...route, view })}
          onStart={route.run === null && !(composing && route.view === "graph") ? () => { setComposing(true); go({ ...route, view: "graph" }); } : null}
        />
        {route.view === "graph" && (
          <GraphView run={run} runId={route.run}>
            {composing && route.run === null && (
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
        {route.view === "lanes" && <LanesView />}
        {route.view === "proof" && <ProofView journeys={journeys} error={journeysError} />}
      </div>
    </div>
  );
}
