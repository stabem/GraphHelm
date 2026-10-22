/** Where the owner is: which run and which view, kept in the URL so a link or reload lands there. */
export type View = "graph" | "lanes" | "proof";

export const VIEWS: readonly View[] = ["graph", "lanes", "proof"];

export interface Route {
  run: string | null;
  view: View;
}

const RUN_ID = /^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$/;

export function parseRoute(search: string): Route {
  const query = new URLSearchParams(search);
  const run = query.get("run");
  const view = query.get("view");
  return {
    run: run !== null && RUN_ID.test(run) ? run : null,
    view: VIEWS.includes(view as View) ? (view as View) : "graph",
  };
}

/** The query for a route. The login ticket is never carried forward: it is spent on first use. */
export function routeSearch(route: Route): string {
  const query = new URLSearchParams();
  if (route.run !== null) query.set("run", route.run);
  if (route.view !== "graph") query.set("view", route.view);
  const text = query.toString();
  return text ? `?${text}` : "";
}
