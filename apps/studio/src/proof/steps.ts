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
  /** #1905: the screen's title and URL, for the Action panel and the compare's address bar. */
  title: string | null;
  url: string;
  /** #1905: every act of the edge, in words (WHAT IT DID). */
  acts: string[];
  /** #1905: what the screen expects (ASSERTIONS), as the flow file declares it. */
  expects: { role: string; name: string; text?: string }[];
  /** #1905: the run's own check of each expectation, when it recorded them. */
  checks: Check[] | null;
  /** #1905: why the step stands as it does, as the Runtime recorded it (screen, else edge). */
  reason: string | null;
  /** #1905: the reason each viewport recorded for this step, when it recorded one. */
  viewportReasons: Partial<Record<Viewport, string>>;
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

type Act = NonNullable<JourneyEdge["acts"]>[number];

function sayAct(act: Act): string {
  if ((act.kind === "fill" || act.kind === "type" || act.kind === "enter_text") && act.text !== undefined) return `Types “${act.text}” into “${act.name ?? ""}”`;
  const verb = VERBS[act.kind] ?? act.kind;
  return act.name ? `${verb} “${act.name}”` : verb;
}

function does(edge: JourneyEdge): string {
  const act = edge.acts?.[0];
  return act === undefined ? edge.id : sayAct(act);
}

/** #1905: each expectation the run checked on this screen and whether it was seen, as the Runtime
 * records it under `screens.<id>.checks` (absent from the typed client); `null` when none recorded. */
export interface Check { role: string; name: string; text?: string; seen: boolean }

function checksOf(seen: unknown): Check[] | null {
  const list = (seen as { checks?: unknown } | undefined)?.checks;
  if (!Array.isArray(list)) return null;
  return list.filter((item): item is Check =>
    typeof item === "object" && item !== null && typeof (item as Check).role === "string" && typeof (item as Check).name === "string" && typeof (item as Check).seen === "boolean");
}

/** The screen's `expect` list, which the Runtime serves with the flow (the typed client omits it). */
function expectsOf(screen: unknown): { role: string; name: string; text?: string }[] {
  const list = (screen as { expect?: unknown } | undefined)?.expect;
  if (!Array.isArray(list)) return [];
  return list.filter((item): item is { role: string; name: string; text?: string } =>
    typeof item === "object" && item !== null && typeof (item as { role?: unknown }).role === "string" && typeof (item as { name?: unknown }).name === "string");
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
    // A Runtime may serve `viewports: null` while a run is still playing; null is no viewport, never a crash.
    if (run?.viewports != null && Object.keys(run.viewports).length > 0) {
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
    const viewportReasons: Partial<Record<Viewport, string>> = {};
    for (const viewport of VIEWPORTS) {
      const view = run?.viewports?.[viewport];
      const said = view?.screens?.[screen]?.reason ?? (edge === null ? undefined : view?.edges?.[edge.id]?.reason);
      if (said) viewportReasons[viewport] = said;
    }
    return {
      n: index + 1,
      screen,
      edge: edge?.id ?? null,
      label: edge === null ? `Opens ${url}` : does(edge),
      status,
      viewports,
      frame: seen?.frame === true,
      title: screens.get(screen)?.title ?? null,
      url,
      acts: (edge?.acts ?? []).map(sayAct),
      expects: expectsOf(screens.get(screen)),
      checks: checksOf(seen),
      reason: (walked?.result === "skipped" ? walked.reason : undefined) ?? seen?.reason ?? walked?.reason ?? null,
      viewportReasons,
    };
  });
}

/** #1905: the run bar's counts. A step skipped because the run waits for the owner's confirmation
 * counts as waiting for you, not as not run. */
export interface StepCounts { passed: number; waits: number; failed: number; notRun: number }

const WAITS = new Set(["confirm_needed", "cli_needs_approval"]);

export function countSteps(steps: Step[]): StepCounts {
  const counts: StepCounts = { passed: 0, waits: 0, failed: 0, notRun: 0 };
  for (const step of steps) {
    if (step.status === "passed") counts.passed += 1;
    else if (step.status === "failed" || step.status === "drift") counts.failed += 1;
    else if (step.status === "skipped" && step.reason !== null && WAITS.has(step.reason)) counts.waits += 1;
    else counts.notRun += 1;
  }
  return counts;
}

/** #1905: a journey card's score and per-step bar, from the journey list's `lastRun` alone. A pass
 * is every step; never run is none; a failure counts the steps the walk order puts before the step
 * `failingStep` names, which is drawn red. A failure naming no step in the walk, or a Runtime that
 * serves no `lastRun`, scores unknown (`null`), never a guessed number. */
export type BarCell = "pass" | "fail" | "none";

export function journeyScore(flow: JourneyFlow): { passed: number | null; total: number; bar: BarCell[] } {
  const steps = journeySteps(flow, null);
  const total = steps.length;
  const none = steps.map((): BarCell => "none");
  const last = flow.lastRun;
  if (last === undefined) return { passed: null, total, bar: none };
  if (last.status === "not_run") return { passed: 0, total, bar: none };
  if (last.status === "passed") return { passed: total, total, bar: steps.map((): BarCell => "pass") };
  const failing = last.failingStep;
  const index = failing === null ? -1 : steps.findIndex((step) => (failing.kind === "edge" ? step.edge === failing.id : step.screen === failing.id));
  if (index < 0) return { passed: null, total, bar: none };
  return { passed: index, total, bar: steps.map((_, i): BarCell => (i < index ? "pass" : i === index ? "fail" : "none")) };
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
