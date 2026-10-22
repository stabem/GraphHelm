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
  /** #1777: how the step stands at each viewport. A run recorded before viewports measured one
   * size, so the viewport it did not play is `not run`. */
  viewports: Record<Viewport, StepStatus>;
  frame: boolean;
}

export const VIEWPORTS: readonly Viewport[] = ["web", "mobile"];

/** #1777: the one viewport a step failed at while it passed at the other, else `null`. */
export function failsOnlyOn(step: Pick<Step, "viewports">): Viewport | null {
  const failed = VIEWPORTS.filter((viewport) => step.viewports[viewport] === "failed" || step.viewports[viewport] === "drift");
  if (failed.length !== 1) return null;
  const other = VIEWPORTS.find((viewport) => viewport !== failed[0])!;
  return step.viewports[other] === "passed" ? failed[0]! : null;
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
  const statusIn = (
    seen: { result?: string } | undefined,
    walked: { result?: string | null } | undefined,
    current: boolean,
  ): StepStatus => {
    const results = [seen?.result, walked?.result];
    if (results.includes("fail")) return "failed";
    if (results.includes("drift")) return "drift";
    if (walked?.result === "skipped") return "skipped";
    if (seen?.result === "pass") return "passed";
    if (current) return "running";
    return "not run";
  };
  return order.map(({ screen, edge }, index) => {
    const seen = run?.screens?.[screen];
    const walked = edge === null ? undefined : run?.edges?.[edge.id];
    const playing = run?.state === "running" && run.current === screen;
    const status = statusIn(seen, walked, playing);
    const viewports: Record<Viewport, StepStatus> = { web: "not run", mobile: "not run" };
    if (run?.viewports !== undefined && Object.keys(run.viewports).length > 0) {
      for (const viewport of VIEWPORTS) {
        const view = run.viewports[viewport];
        if (view !== undefined) viewports[viewport] = statusIn(view.screens?.[screen], edge === null ? undefined : view.edges?.[edge.id], false);
      }
      // The viewport playing now is the top level until it is kept under its name.
      const now = run.viewport;
      if (run.state === "running" && now !== undefined && run.viewports[now] === undefined) viewports[now] = status;
    } else if (seen !== undefined || walked !== undefined || playing) {
      viewports[viewportOf(seen?.width) ?? "web"] = status;
    }
    const url = screens.get(screen)?.url ?? screen;
    return {
      n: index + 1,
      screen,
      edge: edge?.id ?? null,
      label: edge === null ? `Opens ${url}` : does(edge),
      status,
      viewports,
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
