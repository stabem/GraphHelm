import type { JourneyRunView, JourneyView } from "./types";
import { actionText } from "../components/journey-canvas";
import { skippedEdgeInto } from "./mission";

export type FrameStatus = "passed" | "failed" | "waits_for_you" | "not_run";
export interface TestFrame { stepId: string; n: number; verb: "SEES" | "DOES" | "EXPECT"; text: string; status: FrameStatus; reason: string | null; expected: string[] }

export function testFrames(journey: JourneyView, run: JourneyRunView | null): TestFrame[] {
  const last = journey.steps.length - 1;
  return journey.steps.map((s, i) => {
    const title = s.screen?.title ?? s.stepId;
    const expected = s.expectedStates ?? [];
    const verb: TestFrame["verb"] = s.action ? "DOES" : i === last && expected.length > 0 ? "EXPECT" : "SEES";
    const skipped = skippedEdgeInto(run, s.stepId, journey.steps.map((x) => x.stepId));
    const screen = run?.screens?.[s.stepId];
    let status: FrameStatus = "not_run";
    let reason: string | null = null;
    if (skipped) { status = "waits_for_you"; reason = skipped[1].reason ?? null; }
    else if (screen?.result === "pass") status = "passed";
    else if (screen?.result) { status = "failed"; reason = screen.reason ?? null; }
    return { stepId: s.stepId, n: i + 1, verb, text: s.action ? actionText(s.action) : title, status, reason, expected };
  });
}
