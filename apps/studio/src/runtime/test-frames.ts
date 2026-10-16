import type { JourneyFlowView, JourneyRunCheck, JourneyRunView, JourneyView, StepAction } from "./types";
import { skippedEdgeInto } from "./mission";

export type FrameStatus = "running" | "passed" | "failed" | "waits_for_you" | "not_run";
export interface TestFrame { stepId: string; n: number; verb: "SEES" | "DOES" | "EXPECT"; text: string; status: FrameStatus; reason: string | null; expected: string[];
  /** #1287, from the flow source when the Studio has it: what the user did to reach this screen
   * (typed text, choices), why, and every element the screen must show. */
  inputs: string[]; intent: string | null; checks: string[];
  /** #1286: each of `checks`, judged on its own; "not_evaluated" when the run says nothing about it. */
  checkStatus: CheckStatus[];
  /** #1286: the first failed assertion in words, "Expected … — the screen shows …", or null. */
  failure: string | null;
  /** #1286: what was typed on the edge that entered this step, read from the flow (never from the
   * screenshot), so a masked block on the frame has a caption: `Typed "x" into "Field"`. */
  typed: string[];
  /** #1286: what the screen this step lands on is for, from the flow; null when it does not say. */
  screenIntent: string | null }

export type CheckStatus = "passed" | "failed" | "not_evaluated";

/** The fixed phrase stays English; role and names are the flow's and the page's, as written. */
export function checkFailure(check: JourneyRunCheck): string {
  const shown = check.closest ? `${check.closest.role} '${check.closest.name}'` : `no ${check.role}`;
  return `Expected ${check.role} '${check.name}' — the screen shows ${shown}`;
}

/** Each expected element's own verdict: from the run's per-assertion `checks` when it has them; a
 * passed step without them still means every element was seen (the observer fails otherwise). */
function judged(expect: { role: string; name: string }[], status: FrameStatus, checks: JourneyRunCheck[] | undefined) {
  const statuses: CheckStatus[] = expect.map((e, i) => {
    const check = checks?.[i];
    if (check && check.role === e.role && check.name === e.name) return check.seen ? "passed" : "failed";
    return status === "passed" ? "passed" : "not_evaluated";
  });
  const first = checks?.find((check, i) => statuses[i] === "failed" && !check.seen);
  return { statuses, failure: first ? checkFailure(first) : null };
}

const ACTION_VERB: Record<string, string> = {
  navigate: "Goes to", activate: "Clicks", enter_text: "Types into", select: "Chooses",
  upload: "Uploads a file to", download: "Downloads", wait_for: "Waits for", inspect: "Looks at",
  submit: "Submits", recover: "Recovers with", approve: "Approves",
};

export function actionText(action: StepAction): string {
  if (action.kind === "navigate" && (action.strategy === "url" || /^(\/|https?:)/.test(action.target))) return `Opens ${action.target}`;
  const verb = ACTION_VERB[action.kind] ?? action.kind.replace(/_/g, " ");
  return `${verb} “${action.target}”`;
}

/** `running`: the step id a live run is on (#746); that step reads "running" until it has a result. */
const ACT_VERB: Record<string, string> = { ...ACTION_VERB, http: "Sends", cli: "Runs" };

function typedCaption(act: NonNullable<JourneyFlowView["edges"][number]["acts"]>[number]): string | null {
  if (act.kind !== "enter_text") return null;
  const value = act.secret ? `the secret ${act.secret}` : act.text ? `"${act.text}"` : "nothing";
  return `Typed ${value} into "${act.name}"`;
}

function actInput(act: NonNullable<JourneyFlowView["edges"][number]["acts"]>[number]): string {
  const verb = ACT_VERB[act.kind] ?? act.kind.replace(/_/g, " ");
  if (act.kind === "enter_text") {
    const value = act.secret ? `the secret ${act.secret}` : act.text ? `“${act.text}”` : "nothing (left empty)";
    return `Types ${value} into “${act.name}”`;
  }
  return `${verb} “${act.name}”`;
}

/** #1287: the flow's own words for one step: the acts of the edge that entered it and what its
 * screen must show. A composed contract id `<flow>.<path>` names its path; the flow id is `main`. */
function flowDetails(flow: JourneyFlowView | undefined, contractId: string, stepId: string, index: number) {
  if (!flow) return { inputs: [], intent: null, checks: [], typed: [] as string[], screenIntent: null as string | null, expect: [] as { role: string; name: string }[] };
  const pathName = contractId === flow.id ? "main" : contractId.slice(flow.id.length + 1);
  const edgeId = index > 0 ? flow.paths[pathName]?.[index - 1] : undefined;
  const edge = edgeId ? flow.edges.find((e) => e.id === edgeId) : undefined;
  const screen = flow.screens.find((s) => s.id === stepId);
  return {
    inputs: (edge?.acts ?? []).map(actInput),
    typed: (edge?.acts ?? []).map(typedCaption).filter((line): line is string => line !== null),
    intent: edge?.intent ?? null,
    screenIntent: screen?.intent ?? null,
    checks: (screen?.expect ?? []).map((e) => `shows ${e.role} “${e.name}”`),
    expect: screen?.expect ?? [],
  };
}

export function testFrames(journey: JourneyView, run: JourneyRunView | null, running: string | null = null, flow?: JourneyFlowView): TestFrame[] {
  const last = journey.steps.length - 1;
  return journey.steps.map((s, i) => {
    const title = s.screen?.title ?? s.stepId;
    const expected = s.expectedStates ?? [];
    const verb: TestFrame["verb"] = s.action ? "DOES" : i === last && expected.length > 0 ? "EXPECT" : "SEES";
    const skipped = skippedEdgeInto(run, s.stepId, journey.steps.map((x) => x.stepId));
    const screen = run?.screens?.[s.stepId];
    let status: FrameStatus = "not_run";
    let reason: string | null = null;
    if (flow?.status === "weak") { reason = "Not proof: this journey does not say what it proves"; }
    else if (skipped) { status = "waits_for_you"; reason = skipped[1].reason ?? null; }
    else if (screen?.result === "pass") status = "passed";
    else if (screen?.result) { status = "failed"; reason = screen.reason ?? null; }
    else if (running === s.stepId) status = "running";
    const { expect, ...details } = flowDetails(flow, journey.contractId, s.screen?.screenId ?? s.stepId, i);
    const { statuses, failure } = judged(expect, status, flow?.status === "weak" ? undefined : screen?.checks);
    return { stepId: s.stepId, n: i + 1, verb, text: s.action ? actionText(s.action) : title, status, reason, expected,
      ...details, checkStatus: statuses, failure };
  });
}
