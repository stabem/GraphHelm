/**
 * Which execution verbs the dock may offer, and the REASON for each one it may not.
 *
 * A button never pretends (Phase 2, #105): a verb the API would refuse in this state renders
 * disabled with the reason a person can read - never a live button whose click bounces off a
 * refusal, and never a dead button with no explanation. This map is the single place that
 * judgement lives; the dock consumes it and the tests pin it, so the two cannot drift.
 *
 * The vocabulary is the Runtime's own `simulation_status_label` (execution/mod.rs): none |
 * running | completed | failed | paused | blocked | cancelled. "none" means no lifecycle event
 * has folded yet - a REAL state in which the Runtime accepts pause and cancel (the storm suite
 * drives pause straight off a start), so this surface must too.
 *
 * WHAT THIS MAP CAN AND CANNOT SEE (L's review of #662, fourth instance of one class): its
 * only input is the status reply. From it the map reads TWO fields - the aggregate `status`,
 * and `untriagedInterruptions`, the Runtime's own list of nodes left `Blocked` with last
 * outcome `Interrupted` (an immediate pause interrupting work in flight), which `resume.rs`
 * refuses to resume past until each is triaged. It does NOT see per-node states, the graph,
 * or the ledger; a verb whose legality depends on any of those cannot be judged here and must
 * not be added as another arm of the switch below.
 */

import type { ExecutionStatus } from "../runtime/types";

export interface ActionLegality {
  /** `undefined` means the verb is legal; a string is the reason it is not, ready to show. */
  pause?: string;
  resume?: string;
  cancel?: string;
  sweep?: string;
}

/**
 * Closed over the Runtime's seven states, one arm each, and a `default` that judges NOTHING
 * legal. The first version named four states and let the rest fall into a residual bucket that
 * answered "legal" - and `blocked`, the one state without a test cell, was the one it got wrong:
 * pause rendered live while `pause.rs:71` accepts only None | Running (L's review of #662, and
 * the bot's legality.ts:42 - the same finding twice). A status this surface does not know must
 * disable every verb with its name in the reason, never guess.
 *
 * The gates, each copied from the verb's own source rather than remembered:
 * - pause    `pause.rs:71`   accepts None | Running only
 * - cancel   `cancel.rs:62`  refuses Completed | Failed | Cancelled
 * - resume   `resume.rs:315` relays the driver's `ResumeError::NotPaused` for any other state,
 *            and `ResumeError::UntriagedInterruption` while `untriagedInterruptions` is not
 *            empty - `approve.rs:15` on such a node IS the triage act it waits for
 * - sweep    any existing stream - the journal entry it appends is a reading
 */
export function actionLegality(status: ExecutionStatus | null): ActionLegality {
  if (status === null) {
    const reason = "no run is loaded yet";
    return { pause: reason, resume: reason, cancel: reason, sweep: reason };
  }
  // ABSENT is its own state, outside the seven and judged BEFORE the switch: the status endpoint
  // answers an unknown id with an empty projection (executionId null, status null, head 0), and
  // the first map fabricated "none" out of that absence with `?? "none"` - and "none" is the
  // permissive arm, so pause, cancel and sweep lit up on a run that does not exist (PR #662
  // review, and L's addendum: never fabricate a state from an absence). A real run whose
  // lifecycle has not folded reports "running" (execution/mod.rs reported_status), so nothing
  // legitimate is refused here; and absent is not "unknown" - the reason says which.
  if (status.status == null) {
    const reason = "no such execution - the Runtime holds no run under this id";
    return { pause: reason, resume: reason, cancel: reason, sweep: reason };
  }
  const state = status.status;
  switch (state) {
    case "none":
    case "running":
      return { resume: "this run is not paused" };
    case "blocked":
      return {
        pause: "this run is blocked; pause accepts only a running (or not yet started) run",
        resume: "this run is not paused",
      };
    case "paused":
      return { pause: "already paused", resume: untriagedResumeReason(status.untriagedInterruptions) };
    case "completed":
    case "failed":
    case "cancelled":
      return {
        pause: `this run is ${state}; there is nothing left to hold`,
        resume: `this run is ${state}; a finished run does not resume`,
        cancel: `this run is already ${state}`,
      };
    default: {
      const reason = `unknown state "${state}" - this Studio cannot judge what is legal here`;
      return { pause: reason, resume: reason, cancel: reason, sweep: reason };
    }
  }
}

/**
 * Whether the run has ended - completed, failed or cancelled (#1083).
 *
 * On an ended run the dock HIDES pause, resume and cancel rather than offering them disabled: the
 * orchestrator's verification found a completed run still presenting all three as its actions.
 * What an ended run still accepts is read from the Runtime's own gates, not guessed: `sweep.rs`
 * refuses no lifecycle state (its journal entry is a reading), `signal.rs` refuses no terminal
 * state (a message is still recorded), and `approve.rs` gates on the NODE being ghost or blocked,
 * not on the run - so sweep, messages and a real approve target stay.
 */
export function hasEnded(status: ExecutionStatus | null): boolean {
  return status?.status === "completed" || status?.status === "failed" || status?.status === "cancelled";
}

/**
 * The SECOND input resume's legality reads (PR #662 review, legality.ts:66): the Runtime's
 * own triage list. An immediate pause that interrupted work in flight leaves each such node
 * `Blocked` with last outcome `Interrupted`, and `resume_preconditions` refuses with
 * `UntriagedInterruption` until every one is triaged - `approve` on the node is that act
 * (`approve.rs:15`). So while the list is non-empty resume is off, naming the nodes and the
 * remedy.
 *
 * The list is REQUIRED on the wire (execution/mod.rs `render`), a plain array of node ids. A
 * value of any other shape fails validation and is REFUSED as an answer - resume off with a
 * reason naming the field - never treated as "no interruptions" (an absence fabricated from a
 * malformed value would be the same defect as `?? "none"` was).
 */
function untriagedResumeReason(list: unknown): string | undefined {
  if (!Array.isArray(list) || list.some((node) => typeof node !== "string")) {
    return "the status reply's untriagedInterruptions is not a list of node ids - this Studio cannot judge whether resume is legal";
  }
  if (list.length === 0) return undefined;
  const nodes = (list as string[]).join(", ");
  return `interrupted work awaits triage before resume: ${nodes} - approve each node to triage it`;
}
