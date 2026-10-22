/** Where the owner is: which project, run and view, kept in the URL so a link or reload lands there. */
export type View = "graph" | "lanes" | "proof";

export const VIEWS: readonly View[] = ["graph", "lanes", "proof"];

export interface Route {
  /** The project's id (`"first"` for the dev server's own project without one); `null` = the default. */
  project: string | null;
  run: string | null;
  view: View;
  /** The journey Proof opens on (`graphhelm studio open --journey`); `null` = the first. */
  journey?: string | null;
}

const RUN_ID = /^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$/;
const PROJECT = /^(?:first|[0-9a-f]{64})$/;
/** A journey id as the journey contract schema admits it. */
const JOURNEY_ID = /^[a-z0-9][a-z0-9._-]{0,127}$/;

export function parseRoute(search: string): Route {
  const query = new URLSearchParams(search);
  const project = query.get("project");
  const run = query.get("run");
  const view = query.get("view");
  const asked = query.get("journey");
  const journey = asked !== null && JOURNEY_ID.test(asked) && !asked.includes("..") ? asked : null;
  return {
    project: project !== null && PROJECT.test(project) ? project : null,
    run: run !== null && RUN_ID.test(run) ? run : null,
    // A journey link opens Proof unless it names another view.
    view: VIEWS.includes(view as View) ? (view as View) : journey !== null ? "proof" : "graph",
    journey,
  };
}

/** The query for a route. The login ticket is never carried forward: it is spent on first use. */
export function routeSearch(route: Route): string {
  const query = new URLSearchParams();
  if (route.project !== null) query.set("project", route.project);
  if (route.run !== null) query.set("run", route.run);
  if (route.view !== "graph") query.set("view", route.view);
  if (route.journey) query.set("journey", route.journey);
  const text = query.toString();
  return text ? `?${text}` : "";
}
