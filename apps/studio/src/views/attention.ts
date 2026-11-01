import type { AttentionReason } from "../runtime/client";

/** #2324 (approved design, docs/design/studio/2325-2324/): a running run the Runtime says needs you
 * reads "waiting for you" instead of "running". Any other verdict, or none from an older Runtime,
 * leaves the status as the Runtime sent it. */
export function runState(status: string, attention: string | undefined): string {
  return waitsForYou(status, attention) ? WAITING_FOR_YOU : status;
}

/** #2341: the one phrase for a run that waits for its owner, in Graph's Status and in the project rail. */
export const WAITING_FOR_YOU = "waiting for you";

/** #2341: whether the run reads as waiting for you: running, and the Runtime says `needs_you`. */
export function waitsForYou(status: string, attention: string | undefined): boolean {
  return status === "running" && attention === "needs_you";
}

/** #2324: one reason in words, naming the node, draft or session it is about. */
export function reasonText(reason: AttentionReason): string {
  const node = reason.node ?? "a node";
  switch (reason.kind) {
    case "blocked_node": return `${node} is blocked.`;
    case "untriaged_interruption": return `${node} was interrupted and waits for you to decide what happens next.`;
    case "failed_node": return `${node} failed.`;
    case "waiting_input_node": return `${node} waits for your answer.`;
    case "silent_node": return `${node} has said nothing past its own deadline.`;
    case "pending_draft": return reason.draft_id ? `Draft ${reason.draft_id} waits for a decision.` : "A draft waits for a decision.";
    case "wedged_quiescence": return "Nothing can move: no node is running, queued or ready.";
    case "foreign_wake_consumption": return `A wake for session ${reason.session ?? "?"} was taken by a writer that is not this Runtime.`;
    default: return `${reason.kind.replaceAll("_", " ")}${reason.node ? `: ${reason.node}` : ""}.`;
  }
}

/** #2324: the CLI line that acts on a reason the Studio cannot act on yet (node approval). */
export function cliRemedy(reason: AttentionReason, executionId: string): string | null {
  if (reason.kind === "blocked_node" && reason.node) {
    return `graphhelm execution approve --events <events> --execution ${executionId} --node ${reason.node}`;
  }
  return null;
}
