import type { JourneyFlowView, JourneyRunView, JourneyView, StepAction } from "./types";
import { skippedEdgeInto } from "./mission";

export type FrameStatus = "running" | "passed" | "failed" | "waits_for_you" | "not_run";
export interface TestFrame { stepId: string; n: number; verb: "SEES" | "DOES" | "EXPECT"; text: string; status: FrameStatus; reason: string | null; expected: string[];
  /** #1287, from the flow source when the Studio has it: what the user did to reach this screen
   * (typed text, choices), why, and every element the screen must show. */
  inputs: string[]; intent: string | null; checks: string[] }

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
  if (!flow) return { inputs: [], intent: null, checks: [] };
  const pathName = contractId === flow.id ? "main" : contractId.slice(flow.id.length + 1);
  const edgeId = index > 0 ? flow.paths[pathName]?.[index - 1] : undefined;
  const edge = edgeId ? flow.edges.find((e) => e.id === edgeId) : undefined;
  const screen = flow.screens.find((s) => s.id === stepId);
  return {
    inputs: (edge?.acts ?? []).map(actInput),
    intent: edge?.intent ?? null,
    checks: (screen?.expect ?? []).map((e) => `shows ${e.role} “${e.name}”`),
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
    return { stepId: s.stepId, n: i + 1, verb, text: s.action ? actionText(s.action) : title, status, reason, expected,
      ...flowDetails(flow, journey.contractId, s.screen?.screenId ?? s.stepId, i) };
  });
}
