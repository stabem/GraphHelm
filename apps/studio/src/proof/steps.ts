import type { CliOutput, JourneyEdge, JourneyFlow, JourneyLastRun, JourneyPastRun, JourneyRun } from "../runtime/client";

/** How one step stands in the open run. `running` is the screen the Runtime is playing now. */
export type StepStatus = "passed" | "failed" | "drift" | "skipped" | "running" | "waiting" | "precondition" | "not run";

/** #1909: what a step's status says on its card. */
export const STATUS_TEXT: Record<StepStatus, string> = {
  passed: "passed", failed: "failed", drift: "drift", skipped: "skipped", running: "running",
  waiting: "waits for your confirmation", precondition: "precondition", "not run": "not run",
};

/** #1909: the run stopped before a draft's commands: the owner has not confirmed this digest yet. */
export function isHeld(run: JourneyRun | null): boolean {
  if (run === null || run.state === "running") return false;
  if (run.cliConfirm !== undefined && run.cliConfirm !== null && run.cliConfirm.confirmed) return false;
  return Object.values(run.edges ?? {}).some((edge) => edge.reason === "confirm_needed");
}

/** #1911: the run stopped before a draft's commands because its flow differs from the reviewed one
 * on main. Nothing ran and there is nothing to confirm: the flow waits for review. */
export function isWaitingReview(run: JourneyRun | null): boolean {
  if (run === null || run.state === "running") return false;
  return Object.values(run.edges ?? {}).some((edge) => edge.reason === "review_needed");
}

/** #1911: a waiting step's card says what it waits for. */
export function statusText(status: StepStatus, run: JourneyRun | null): string {
  return status === "waiting" && isWaitingReview(run) ? "waits for review" : STATUS_TEXT[status];
}

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
  /** #1955: what the edge into this step proves for the user (WHY THIS STEP); `null` when the
   * flow gives no reason or the step is the entry screen. */
  why: string | null;
  /** #1905: what the screen expects (ASSERTIONS), as the flow file declares it, each with the
   * flow's reason for it (#1955) when it gives one. */
  expects: Expect[];
  /** #1905: the run's own check of each expectation, when it recorded them. */
  checks: Check[] | null;
  /** #1905: why the step stands as it does, as the Runtime recorded it (screen, else edge). */
  reason: string | null;
  /** #1905: the reason each viewport recorded for this step, when it recorded one. */
  viewportReasons: Partial<Record<Viewport, string>>;
  /** #1912: the edge into this step runs a command (`cli`): it has no screen, so no frame. */
  cli: boolean;
  /** #1912: what the edge's commands printed in this run, each distinct output once. */
  outputs: CliOutput[];
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

/** One expectation a screen declares; `because` (#1955) is why its value is the right one. */
export interface Expect { role: string; name: string; text?: string; because?: string }

/** The screen's `expect` list, which the Runtime serves with the flow (the typed client omits it). */
function expectsOf(screen: unknown): Expect[] {
  const list = (screen as { expect?: unknown } | undefined)?.expect;
  if (!Array.isArray(list)) return [];
  return list.filter((item): item is Expect =>
    typeof item === "object" && item !== null && typeof (item as { role?: unknown }).role === "string" && typeof (item as { name?: unknown }).name === "string");
}

/** #1955: the flow's reason for an expectation the run checked, matched on role, name and text;
 * `null` when the flow gives none. */
export function becauseOf(expects: Expect[], check: { role: string; name: string; text?: string }): string | null {
  const said = expects.find((want) => want.role === check.role && want.name === check.name && (want.text ?? "") === (check.text ?? ""))?.because;
  return typeof said === "string" && said.trim() !== "" ? said : null;
}

/** #1955: an expectation's name as a person reads it. A `/pattern/` name is a regular expression:
 * the slashes go, escapes such as `\s` and `\$` become their characters, and `.+` / `.*` read
 * as "…". `pattern` is true for such a name, so the panel says "like" and keeps the raw pattern
 * for a tooltip; a plain name is returned unchanged. */
export function readableName(name: string): { text: string; pattern: boolean } {
  const match = /^\/(.+)\/[a-z]*$/s.exec(name);
  if (match === null) return { text: name, pattern: false };
  const text = match[1]!
    .replace(/^\^|\$$/g, "")
    .replace(/\s*\.[+*]\??\s*/g, " … ")
    .replace(/\\s[+*]?/g, " ")
    .replace(/\\d[+*]?/g, "0")
    .replace(/\\(.)/g, "$1")
    .replace(/ {2,}/g, " ")
    .trim();
  return { text, pattern: true };
}

/** #1955: the sentence a run's reason code reads as; a code with no sentence reads as itself. */
const REASONS: Record<string, string> = {
  page_error: "The page reported an error during this step.",
  act_failed: "An action of this step could not be done.",
  not_reached: "The run stopped before this step.",
  confirm_needed: "This step waits for your confirmation of its commands.",
  review_needed: "This step waits for review: its flow differs from the reviewed one on main.",
  cli_needs_approval: "This step waits for your approval of its commands.",
  timeout: "The step took too long and was stopped.",
  internal: "The run stopped on an internal error.",
};

export function reasonSentence(code: string): string | null {
  return REASONS[code] ?? null;
}

/** #1955: the flow's reason for a step, when it gives one. */
function whyOf(edge: JourneyEdge | null): string | null {
  const said = edge?.why;
  return typeof said === "string" && said.trim() !== "" ? said : null;
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
  // #1909: a draft held on `confirm_needed` waits for the owner's confirmation; every step it did
  // not play waits with it, never reads `not run` or skipped.
  const held = isHeld(run) || isWaitingReview(run);
  // #1912: a flow whose acts are all commands never observes its entry screen: the Runtime records
  // it `not_reached` because it is the commands' precondition, not a step that failed to run.
  const allEdges = flow.edges ?? [];
  const commandsOnly = allEdges.length > 0 && allEdges.every((edge) => (edge.acts ?? []).length > 0 && (edge.acts ?? []).every((act) => act.kind === "cli"));
  const statusIn = (
    seen: { result?: string } | undefined,
    walked: { result?: string | null } | undefined,
    current: boolean,
  ): StepStatus => {
    const results = [seen?.result, walked?.result];
    if (results.includes("fail")) return "failed";
    if (results.includes("drift")) return "drift";
    if (walked?.result === "skipped") return held ? "waiting" : "skipped";
    if (seen?.result === "pass") return "passed";
    if (current) return "running";
    return held ? "waiting" : "not run";
  };
  return order.map(({ screen, edge }, index) => {
    const seen = run?.screens?.[screen];
    const walked = edge === null ? undefined : run?.edges?.[edge.id];
    const playing = run?.state === "running" && run.current === screen;
    const precondition = commandsOnly && edge === null && index === 0 && run?.state === "ready" && seen?.result === undefined && !held;
    const status = precondition ? "precondition" : statusIn(seen, walked, playing);
    const viewports: Record<Viewport, StepStatus> = { web: "not run", mobile: "not run" };
    // A Runtime may serve `viewports: null` while a run is still playing; null is no viewport, never a crash.
    if (run?.viewports != null && Object.keys(run.viewports).length > 0) {
      for (const viewport of VIEWPORTS) {
        const view = run.viewports[viewport];
        if (view !== undefined) viewports[viewport] = precondition ? "precondition" : statusIn(view.screens?.[screen], edge === null ? undefined : view.edges?.[edge.id], false);
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
      why: whyOf(edge),
      expects: expectsOf(screens.get(screen)),
      checks: checksOf(seen),
      reason: precondition ? "The commands' starting point: a command flow does not observe it." : (walked?.result === "skipped" ? walked.reason : undefined) ?? seen?.reason ?? walked?.reason ?? null,
      viewportReasons,
      cli: (edge?.acts ?? []).some((act) => act.kind === "cli"),
      outputs: edge === null ? [] : distinctOutputs(run?.outputs?.[edge.id]),
    };
  });
}

/** #1914: a run walks an edge once per path, and the Runtime keeps the command again each time (its
 * output can differ by a digest, a temp path or a clock). A command is one act: it is shown once,
 * where its first run stood, with what its last run printed. */
export function distinctOutputs(list: CliOutput[] | undefined): CliOutput[] {
  if (!Array.isArray(list)) return [];
  const byCommand = new Map<string, CliOutput>();
  for (const output of list) {
    if (typeof output !== "object" || output === null || typeof output.argv !== "string") continue;
    byCommand.set(output.argv, output);
  }
  return [...byCommand.values()];
}

/** #1914: what a journey drives. Terminal when every act is a command, Browser when none is, Both
 * when it mixes the two; a flow with no act reads as Browser, the Studio's default. */
export type JourneyKind = "terminal" | "browser" | "both";

export function journeyKind(flow: JourneyFlow): JourneyKind {
  const acts = (flow.edges ?? []).flatMap((edge) => edge.acts ?? []);
  const cli = acts.filter((act) => act.kind === "cli").length;
  if (cli === 0) return "browser";
  return cli === acts.length ? "terminal" : "both";
}

/** #1912: the console lines a command run gives: each command, what it printed, and its exit code,
 * in the order the run walked its edges. */
export function outputLines(run: JourneyRun | null): string[] {
  const lines: string[] = [];
  for (const list of Object.values(run?.outputs ?? {})) {
    for (const output of distinctOutputs(list)) {
      lines.push(`$ ${output.argv}`);
      if (output.stdout.trim()) lines.push(output.stdout.trimEnd());
      if (output.stderr.trim()) lines.push(output.stderr.trimEnd());
      lines.push(`exit ${output.exit ?? "unknown"}`);
    }
  }
  return lines;
}

/** #1905: the run bar's counts. A step skipped because the run waits for the owner's confirmation
 * counts as waiting for you, not as not run. */
export interface StepCounts { passed: number; waits: number; failed: number; notRun: number }

const WAITS = new Set(["confirm_needed", "cli_needs_approval", "review_needed"]);

export function countSteps(steps: Step[]): StepCounts {
  const counts: StepCounts = { passed: 0, waits: 0, failed: 0, notRun: 0 };
  for (const step of steps) {
    if (step.status === "passed") counts.passed += 1;
    else if (step.status === "failed" || step.status === "drift") counts.failed += 1;
    else if (step.status === "precondition") continue;
    else if (step.status === "waiting" || (step.status === "skipped" && step.reason !== null && WAITS.has(step.reason))) counts.waits += 1;
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

/** #1914: the live runs without the one of a panel that went away, so its card reads the list again. */
export function withoutLive<T>(live: Record<string, T>, id: string): Record<string, T> {
  return Object.fromEntries(Object.entries(live).filter(([key]) => key !== id));
}

/** #1950: the journeys whose run the Proof view reads from the Runtime itself: every one it last
 * saw running, except the selected one, whose open panel already follows it. Sorted, so the list
 * is a stable key for the polling effect. */
export function watchedIds(live: Record<string, { state: string }>, selected: string | null): string[] {
  return Object.keys(live).filter((id) => id !== selected && live[id]?.state === "running").sort();
}

/** #1953: the live runs after the journey list is read again. A run the view still saw playing
 * stays until its own next answer; a finished one gives way to the list's result; and a journey
 * the list says is running (a reload, or a run started from another Studio or the CLI) is seeded
 * as running, so the watch follows it to its result. */
export function liveAfterList(live: Record<string, JourneyRun>, flows: readonly Pick<JourneyFlow, "id" | "running">[] | null): Record<string, JourneyRun> {
  const kept: Record<string, JourneyRun> = Object.fromEntries(Object.entries(live).filter(([, run]) => run.state === "running"));
  for (const flow of flows ?? []) {
    if (flow.running != null && kept[flow.id] === undefined) kept[flow.id] = { state: "running" };
  }
  return kept;
}

/** #1950: a panel that went away keeps its journey's entry only while the Runtime said running:
 * the view then reads that run until it ends, so the card neither forgets it nor stays stuck. */
export function afterPanelGone<T extends { state: string }>(live: Record<string, T>, id: string): Record<string, T> {
  return live[id]?.state === "running" ? live : withoutLive(live, id);
}

/** #1914: a card's chip, score and bar from the run the open panel is following, so the card
 * reads `running` with its live step count while the run plays and the result the moment it ends,
 * before the journey list is read again. `null` when the run says nothing yet (state `none`). */
export function liveCard(flow: JourneyFlow, run: JourneyRun | null): { text: string; tone: "pass" | "fail" | "idle" | "wait"; passed: number; total: number; bar: BarCell[] } | null {
  if (run === null || run.state === "none") return null;
  const steps = journeySteps(flow, run);
  const passed = steps.filter((step) => step.status === "passed").length;
  const bar = steps.map((step): BarCell => (step.status === "passed" ? "pass" : step.status === "failed" || step.status === "drift" ? "fail" : "none"));
  const total = steps.length;
  if (run.state === "running") return { text: "● running", tone: "wait", passed, total, bar };
  const ok = run.state === "ready" && run.result === "pass";
  const mark = ok ? "✓ passed" : "✕ failed";
  const tail = run.kind === "replay" ? "" : " · preview, not proof";
  return { text: `${mark}${tail}`, tone: run.kind === "replay" ? (ok ? "pass" : "fail") : "idle", passed, total, bar };
}

/** The journeys column's chip. Only a replay is proof; a preview's verdict is labelled as such, and
 * a Runtime that does not serve `lastRun` reads unknown, never `not run`. */
export function lastRunChip(lastRun: JourneyLastRun | undefined): { text: string; tone: "pass" | "fail" | "idle" } {
  if (lastRun === undefined) return { text: "? unknown", tone: "idle" };
  if (lastRun.status === "not_run") return { text: "○ not run", tone: "idle" };
  // #1956: a run of an earlier version of the flow proves nothing about this one.
  if (lastRun.flowChanged === true) return { text: CHANGED, tone: "idle" };
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

/** #1956: what a journey edited since its last run reads, on its card and in the panel header. */
export const CHANGED = "Changed since last run";

/** #1956: the run of an earlier version of the flow the panel shows when the current version has
 * none. The preview read answers `none` once the flow's digest (or scope commit) moved, and the
 * Runtime then lists the earlier version's run as the newest history entry; with no history the
 * journey never ran. A run of the current version (any state but `none`) is always shown instead. */
export function earlierVersion(run: JourneyRun | null, history: JourneyPastRun[]): JourneyPastRun | null {
  if (run?.state !== "none") return null;
  return history[0] ?? null;
}

/** #1956: an earlier version's run as the step grid reads a run: its recorded results, never live. */
export function pastAsRun(past: JourneyPastRun): JourneyRun {
  return {
    state: past.state === "failed" ? "failed" : "ready",
    kind: past.kind,
    ranAt: past.ranAt,
    result: past.result,
    reason: past.reason,
    screens: Object.fromEntries(Object.entries(past.screens ?? {}).map(([id, seen]) => [id, { ...seen, frame: seen.frame === true }])),
    edges: past.edges ?? {},
  };
}
