import type { JourneyFlow } from "../runtime/client";

/** The owner's decision on a journey as the Runtime lists it. `approved` is the flow's own status;
 * a rejection counts only while it names this exact content (the Runtime returns it per digest). A
 * stale or weak approval is neither: the owner decides again. */
export type Decision = "approved" | "rejected" | "undecided";

export function decisionOf(flow: JourneyFlow): Decision {
  if (flow.status === "approved") return "approved";
  if (flow.rejected !== undefined && flow.rejected !== null) return "rejected";
  return "undecided";
}

/** Approve and Reject are offered once the journey has a finished run and is not approved. */
export function offersDecision(flow: JourneyFlow, runFinished: boolean): boolean {
  return runFinished && decisionOf(flow) !== "approved";
}

/** The reason a Reject sends, or the refusal shown instead of sending: the Runtime needs one. */
export function rejectNote(text: string): { note: string } | { refusal: string } {
  const note = text.trim();
  return note === "" ? { refusal: "Say why the journey is rejected before rejecting it." } : { note };
}
