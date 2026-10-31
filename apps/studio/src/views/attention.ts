import type { AttentionReason } from "../runtime/client";

/** #2324: the run's state as one line: its status, and `needs you` when the Runtime says so. An
 * attention the Runtime did not report (older Runtime) adds nothing; `unknown` is said. */
export function runState(status: string, attention: string | undefined): string {
  if (attention === "needs_you") return `${status} · needs you`;
  if (attention === "unknown") return `${status} · whether it needs you is unknown`;
  return status;
}

/** #2324: one reason in words, naming the node, draft or session it is about. */
export function reasonText(reason: AttentionReason): string {
  const node = reason.node ?? "a node";
  switch (reason.kind) {
    case "blocked_node": return `${node} is blocked and waits for your approval.`;
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
