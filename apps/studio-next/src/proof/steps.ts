import type { JourneyEdge, JourneyFlow, JourneyLastRun, JourneyRun } from "../runtime/client";

/** How one step stands in the open run. `running` is the screen the Runtime is playing now. */
export type StepStatus = "passed" | "failed" | "drift" | "skipped" | "running" | "not run";

export type Viewport = "web" | "mobile";

export interface Step {
  n: number;
  /** The screen this step lands on; its frame is that screen's frame. */
  screen: string;
  /** The edge that leads to the screen; `null` for the entry screen. */
  edge: string | null;
  label: string;
  status: StepStatus;
  /** Which viewport the run measured this step at, or `null` when the run did not say. */
  viewport: Viewport | null;
  frame: boolean;
}

/** The design's break between the web and the mobile layout (web 1280, mobile 390). */
const MOBILE_MAX_WIDTH = 767;

export function viewportOf(width: number | undefined): Viewport | null {
  if (typeof width !== "number" || !Number.isFinite(width) || width <= 0) return null;
  return width <= MOBILE_MAX_WIDTH ? "mobile" : "web";
}

const VERBS: Record<string, string> = { activate: "Clicks", click: "Clicks", fill: "Types into", type: "Types into", submit: "Submits", select: "Selects", check: "Checks", wait_for: "Waits for", press: "Presses", navigate: "Opens" };

function does(edge: JourneyEdge): string {
  const act = edge.acts?.[0];
  if (act === undefined) return edge.id;
  const verb = VERBS[act.kind] ?? act.kind;
  return act.name ? `${verb} “${act.name}”` : verb;
}

/** The order the run walks: the `main` path (else the first path), entry screen first, then each
 * edge's target. Without paths, the screens in file order. */
export function journeySteps(flow: JourneyFlow, run: JourneyRun | null): Step[] {
  const screens = new Map((flow.screens ?? []).map((screen) => [screen.id, screen]));
  const edges = new Map((flow.edges ?? []).map((edge) => [edge.id, edge]));
  const paths = flow.paths ?? {};
  const path = (paths.main ?? Object.values(paths)[0] ?? []).map((id) => edges.get(id)).filter((edge): edge is JourneyEdge => edge !== undefined);
  const order: { screen: string; edge: JourneyEdge | null }[] = path.length > 0
    ? [{ screen: path[0]!.from, edge: null }, ...path.map((edge) => ({ screen: edge.to, edge }))]
    : [...screens.keys()].map((screen) => ({ screen, edge: null }));
  return order.map(({ screen, edge }, index) => {
    const seen = run?.screens?.[screen];
    const walked = edge === null ? undefined : run?.edges?.[edge.id];
    const results = [seen?.result, walked?.result];
    let status: StepStatus;
    if (results.includes("fail")) status = "failed";
    else if (results.includes("drift")) status = "drift";
    else if (walked?.result === "skipped") status = "skipped";
    else if (seen?.result === "pass") status = "passed";
    else if (run?.state === "running" && run.current === screen) status = "running";
    else status = "not run";
    const url = screens.get(screen)?.url ?? screen;
    return {
      n: index + 1,
      screen,
      edge: edge?.id ?? null,
      label: edge === null ? `Opens ${url}` : does(edge),
      status,
      viewport: seen === undefined ? null : viewportOf(seen.width),
      frame: seen?.frame === true,
    };
  });
}

/** The journeys column's chip. Only a replay is proof; a preview's verdict is labelled as such, and
 * a Runtime that does not serve `lastRun` reads unknown, never `not run`. */
export function lastRunChip(lastRun: JourneyLastRun | undefined): { text: string; tone: "pass" | "fail" | "idle" } {
  if (lastRun === undefined) return { text: "? unknown", tone: "idle" };
  if (lastRun.status === "not_run") return { text: "○ not run", tone: "idle" };
  const mark = lastRun.status === "passed" ? "✓ passed" : "✕ failed";
  if (lastRun.kind !== "replay") return { text: `${mark} · preview, not proof`, tone: "idle" };
  return { text: mark, tone: lastRun.status === "passed" ? "pass" : "fail" };
}

/**
 * The run bar while Run provisions the browser observer (#1763). The Runtime reports the command
 * it runs or a phase line; the owner reads the phase in plain words, with a percentage only when
 * the installer printed one. The phase list follows apps/studio/src/runtime/observer-setup.ts
 * (#1775), the same Runtime messages.
 */
const PHASES: ReadonlyArray<readonly [RegExp, string]> = [
  [/playwright install|chromium/i, "Downloading Chromium"],
  [/npm install|@playwright\/test/i, "Installing test tools"],
  [/writing the current browser observer|driver/i, "Writing the driver"],
];

export function preparingStatus(message: string): string {
  const phase = PHASES.find(([pattern]) => pattern.test(message))?.[1];
  const percent = /\b(\d{1,3})\s?%/.exec(message)?.[1];
  const shown = percent !== undefined && Number(percent) <= 100 ? ` ${Number(percent)}%` : "";
  return phase ? `Preparing the browser · ${phase}${shown}` : `Preparing the browser${shown}`;
}
