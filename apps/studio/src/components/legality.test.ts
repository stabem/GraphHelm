import { describe, expect, it } from "vitest";

import { actionLegality, hasEnded } from "./legality";
import type { ExecutionStatus } from "../runtime/types";

/**
 * A button never pretends (Phase 2, #105): a verb the API would refuse in this state renders
 * disabled WITH THE REASON. This map is the single place that judgement lives, so the dock and
 * its tests cannot drift on what "legal" means. The vocabulary is the Runtime's own
 * `simulation_status_label`: none | running | completed | failed | paused | blocked | cancelled.
 */
const statusOf = (status: string | null): ExecutionStatus =>
  ({
    executionId: "demo",
    status,
    mode: null,
    attention: "can_sleep",
    attentionReasons: [],
    untriagedInterruptions: [],
    silenceUnevaluated: [],
    startedAt: null,
    lastEventAt: null,
    nodeLastEventAt: {},
    headSequence: 3,
    nodeStateCounts: {},
  }) as never;

describe("what each verb is allowed to claim", () => {
  it("lets a running execution be paused, cancelled and swept, but not resumed", () => {
    const legality = actionLegality(statusOf("running"));
    expect(legality.pause).toBeUndefined();
    expect(legality.cancel).toBeUndefined();
    expect(legality.sweep).toBeUndefined();
    expect(legality.resume).toMatch(/not paused/i);
  });

  it("lets a paused execution be resumed or cancelled, but not paused again", () => {
    const legality = actionLegality(statusOf("paused"));
    expect(legality.resume).toBeUndefined();
    expect(legality.cancel).toBeUndefined();
    expect(legality.pause).toMatch(/already paused/i);
  });

  it("offers nothing but sweep on a finished run, each refusal naming the state", () => {
    for (const terminal of ["completed", "failed", "cancelled"]) {
      const legality = actionLegality(statusOf(terminal));
      expect(legality.pause).toContain(terminal);
      expect(legality.resume).toContain(terminal);
      expect(legality.cancel).toContain(terminal);
      expect(legality.sweep).toBeUndefined();
    }
  });

  it("treats a status nothing has set yet as pausable and cancellable, not as finished", () => {
    // "none" is a real answer - no lifecycle event has folded - and the storm suite proves the
    // Runtime accepts a pause straight off a start. A surface refusing what the API accepts
    // would be the inverted half of the honesty rule.
    const legality = actionLegality(statusOf("none"));
    expect(legality.pause).toBeUndefined();
    expect(legality.cancel).toBeUndefined();
    expect(legality.resume).toMatch(/not paused/i);
  });

  /** The seventh state - and the one the first map never named. `pause.rs:71` accepts only
   * None | Running, so a blocked run's pause button must be off with the reason; cancel stays
   * legal (`cancel.rs:62` refuses only the three terminals). L's review of #662: the single
   * state without a cell was the single state the map got wrong. */
  it("keeps pause off a blocked run and names why, while cancel stays legal", () => {
    const legality = actionLegality(statusOf("blocked"));
    expect(legality.pause).toMatch(/blocked/i);
    // The default arm's reason also names the state, so /blocked/ alone is a vacuous match;
    // deleting the blocked arm must redden THIS line, the one that states the finding.
    expect(legality.pause).not.toMatch(/unknown state/i);
    expect(legality.cancel).toBeUndefined();
    expect(legality.sweep).toBeUndefined();
    expect(legality.resume).toMatch(/not paused/i);
  });

  /** A status outside the seven is a Runtime this surface does not know. The only honest
   * answer is to judge nothing legal - a residual bucket that guesses "legal" is how the
   * blocked defect happened, and a future eighth state must not repeat it. */
  it("refuses to guess for a status outside the vocabulary - every verb off, reason named", () => {
    const legality = actionLegality(statusOf("hibernating"));
    for (const reason of [legality.pause, legality.resume, legality.cancel, legality.sweep]) {
      expect(reason).toMatch(/unknown state/i);
      expect(reason).toContain("hibernating");
    }
  });

  /** The status endpoint answers an unknown id with an EMPTY projection (executionId null,
   * status null, head 0). That is not a lifecycle state, it is no run at all - every verb off,
   * and the reason says so (PR #662 review). */
  it("treats a null status as ABSENT - every verb off, and absence is not 'unknown'", () => {
    const legality = actionLegality(statusOf(null));
    for (const reason of [legality.pause, legality.resume, legality.cancel, legality.sweep]) {
      expect(reason).toMatch(/no such execution/i);
      // Absence is its own state: it must never read as the default arm's "unknown state",
      // and it must never fabricate "none" (the permissive arm) out of nothing.
      expect(reason).not.toMatch(/unknown state/i);
    }
  });

  /** The SECOND input (PR #662 review, legality.ts:66): a paused run whose immediate pause
   * interrupted work in flight carries those nodes in `untriagedInterruptions`, and
   * `resume_preconditions` refuses until each is triaged - so resume is off, naming the nodes
   * and the remedy (approve). An empty list leaves resume legal; a malformed list is REFUSED
   * as an answer, never read as "nothing to triage". */
  it("keeps resume off a paused run with untriaged interruptions, naming the nodes and the remedy", () => {
    const withInterruptions = { ...statusOf("paused"), untriagedInterruptions: ["implementation", "deploy"] };
    const legality = actionLegality(withInterruptions);
    expect(legality.resume).toMatch(/triage/i);
    expect(legality.resume).toContain("implementation");
    expect(legality.resume).toContain("deploy");
    expect(legality.resume).toMatch(/approve/i);
    expect(legality.cancel).toBeUndefined();
    // Triaged: the list empties and resume is legal again - the arm above did not close it for good.
    expect(actionLegality({ ...statusOf("paused"), untriagedInterruptions: [] }).resume).toBeUndefined();
  });

  it("refuses to judge resume when untriagedInterruptions is not a list of node ids", () => {
    for (const malformed of [undefined, null, "implementation", [42]]) {
      const legality = actionLegality({ ...statusOf("paused"), untriagedInterruptions: malformed } as never);
      expect(legality.resume).toMatch(/untriagedInterruptions/);
      expect(legality.resume).not.toMatch(/unknown state/i);
    }
  });

  /** #1083: the dock hides what an ended run cannot do. Exactly the three terminal states end a
   * run; paused, blocked, running, "none", an absent run and an unknown state do not. */
  it("names exactly the three terminal states as ended", () => {
    for (const terminal of ["completed", "failed", "cancelled"]) expect(hasEnded(statusOf(terminal))).toBe(true);
    for (const live of ["running", "paused", "blocked", "none", "hibernating", null]) expect(hasEnded(statusOf(live))).toBe(false);
    expect(hasEnded(null)).toBe(false);
  });

  it("says why everything is off when no run is loaded", () => {
    const legality = actionLegality(null);
    for (const reason of [legality.pause, legality.resume, legality.cancel, legality.sweep]) {
      expect(reason).toMatch(/no run/i);
    }
  });
});
