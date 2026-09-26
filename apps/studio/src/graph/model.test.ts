import { describe, expect, it } from "vitest";
import { buildGraphModel, conversationFor, moodOf, nodeResult, voiceOf } from "./model";

it("distinguishes a returned model reply from a verified result and from the runtime recorder", () => {
  const event = {
    sequence: 11,
    kind: "node_outcome_recorded",
    payload: { nodeId: "review_browser_evidence", outcome: "succeeded", nextState: "succeeded" },
    occurredAt: "2026-09-26T02:45:00Z",
    actorId: "system-runtime",
    actorType: "system",
    idempotencyKey: "result-11",
    eventId: "event-11",
    evidenceRefs: ["exec-landing-review_browser_evidence-a1-reply"],
  };
  const node = buildGraphModel([event], null).nodes[0];
  expect(node.resultSource).toBe("model_reply");
  expect(nodeResult(node)).toEqual({
    executor: "Model call · model identity not recorded",
    verification: "Reply received · acceptance not verified",
    short: "Reply received · unverified",
  });
  expect(node.history[0].actorId).toBe("system-runtime");
});

it("recognizes a structured judge result as a verdict rather than a plain reply", () => {
  const model = buildGraphModel([{
    sequence: 9,
    kind: "node_outcome_recorded",
    payload: { nodeId: "judge_browser", outcome: "succeeded", nextState: "succeeded" },
    occurredAt: "2026-09-26T02:45:00Z",
    actorId: "system-runtime",
    actorType: "system",
    idempotencyKey: "result-9",
    eventId: "event-9",
    evidenceRefs: ["exec-landing-judge_browser-a1-judgment"],
  }], null);
  expect(model.nodes[0].resultSource).toBe("judge_verdict");
  expect(nodeResult(model.nodes[0])?.verification).toBe("Structured judgment passed");
});

/**
 * "Quietly reopened the part it already called done" - the reference project's best signal,
 * derived here from OUR spine alone: a node that reached a terminal settled state (succeeded,
 * waived, skipped) and was then named by a LATER event carries both coordinates, so the board
 * can show the reopening with its author. No filesystem watcher, no second truth.
 */
describe("reopened after done", () => {
  const outcome = (sequence: number, nodeId: string, nextState: string) => ({
    sequence,
    kind: "node_outcome_recorded",
    payload: { nodeId, outcome: "reported", nextState },
    occurredAt: `2026-08-30T12:00:${String(sequence).padStart(2, "0")}Z`,
    actorId: sequence % 2 === 0 ? "codex" : "system-cli",
    actorType: sequence % 2 === 0 ? "agent" : "system",
    idempotencyKey: `k${sequence}`,
    eventId: `event-${sequence}`,
    evidenceRefs: [],
  });

  it("marks a settled node that a later event named, with both coordinates", () => {
    const model = buildGraphModel(
      [
        outcome(3, "deploy", "succeeded"),
        outcome(4, "deploy", "running"),
      ] as never,
      null,
    );
    const node = model.nodes.find((candidate) => candidate.id === "deploy")!;
    expect(node.reopened).toEqual({ settledAt: 3, reopenedAt: 4, by: "codex" });
  });

  it("leaves an undisturbed settled node unmarked", () => {
    const model = buildGraphModel([outcome(3, "deploy", "succeeded")] as never, null);
    expect(model.nodes.find((candidate) => candidate.id === "deploy")!.reopened).toBeNull();
  });

  it("never marks a node that was still working", () => {
    const model = buildGraphModel(
      [outcome(3, "deploy", "running"), outcome(4, "deploy", "succeeded")] as never,
      null,
    );
    expect(model.nodes.find((candidate) => candidate.id === "deploy")!.reopened).toBeNull();
  });
});

/** The board-face lint: disagreements the fold itself can attest, each citing the log. */
describe("the model's lint", () => {
  const outcome = (sequence: number, nodeId: string, nextState: string, evidenceRefs: string[] = []) => ({
    sequence,
    kind: "node_outcome_recorded",
    payload: { nodeId, outcome: "reported", nextState },
    occurredAt: `2026-08-30T12:00:${String(sequence).padStart(2, "0")}Z`,
    actorId: "codex",
    actorType: "agent",
    idempotencyKey: `k${sequence}`,
    eventId: `event-${sequence}`,
    evidenceRefs,
  });

  it("accuses a settling that carried no evidence, citing its sequence", () => {
    const model = buildGraphModel([outcome(7, "deploy", "succeeded")] as never, null);
    expect(model.lint).toEqual([
      { kind: "done-without-evidence", detail: "deploy settled as succeeded carrying no evidence", sequence: 7 },
    ]);
  });

  it("stays silent about a settling that brought its evidence", () => {
    const model = buildGraphModel([outcome(7, "deploy", "succeeded", ["ev-7"])] as never, null);
    expect(model.lint).toEqual([]);
  });

  it("accuses a reopening with the later event's sequence and author", () => {
    const model = buildGraphModel(
      [outcome(3, "deploy", "succeeded", ["ev-3"]), outcome(9, "deploy", "running")] as never,
      null,
    );
    expect(model.lint).toEqual([
      { kind: "reopened-after-done", detail: "deploy was reopened after it settled by codex", sequence: 9 },
    ]);
  });

  it("accuses a proven edge whose endpoint is not on the roster, instead of dropping it silently", () => {
    const model = buildGraphModel([outcome(3, "deploy", "running")] as never, {
      match: "matched",
      edges: [{ id: "e1", from: "deploy", to: "ghost", type: "needs" }],
      entrypoints: [],
    } as never);
    expect(model.edges).toEqual([]);
    expect(model.lint).toEqual([
      {
        kind: "orphan-edge",
        detail: "the graph file draws deploy → ghost, but ghost is not on this run's roster",
        sequence: null,
      },
    ]);
  });
});

import type { RuntimeEvent } from "../runtime/types";

function event(sequence: number, kind: string, payload: unknown, actorType = "system"): RuntimeEvent {
  return {
    sequence,
    kind,
    payload,
    occurredAt: `2026-08-27T12:00:${String(sequence).padStart(2, "0")}Z`,
    actorId: `${actorType}-actor`,
    actorType,
    idempotencyKey: `k${sequence}`,
    eventId: `event-${sequence}`,
    evidenceRefs: [],
  };
}

const ROSTER = event(2, "execution_form_declared", {
  executionId: "demo",
  nodeIds: ["deploy", "implementation"],
});

describe("the board's model", () => {
  it("takes its roster from the declaration, not from what happened to run", () => {
    const model = buildGraphModel([ROSTER]);
    expect(model.rosterDeclared).toBe(true);
    expect(model.nodes.map((node) => node.id)).toEqual(["deploy", "implementation"]);
    // A declared node nothing has touched is `unknown`, not `ready`: claiming a state the log
    // never reported would say the scheduler had reached it.
    expect(model.nodes.map((node) => node.state)).toEqual(["unknown", "unknown"]);
  });

  /** A partial page is the normal case while a stream is still loading. The model must say that
   * the roster is missing rather than present a short list as the whole graph. */
  it("says so when it has not seen the roster", () => {
    const model = buildGraphModel([
      event(9, "node_outcome_recorded", { nodeId: "implementation", nextState: "blocked" }),
    ]);
    expect(model.rosterDeclared).toBe(false);
    expect(model.nodes.map((node) => node.id)).toEqual(["implementation"]);
  });

  it("carries the LAST state each node was moved into, not the first", () => {
    const model = buildGraphModel([
      ROSTER,
      event(3, "node_outcome_recorded", { nodeId: "implementation", nextState: "running", outcome: "started" }),
      event(4, "node_outcome_recorded", { nodeId: "implementation", nextState: "blocked", outcome: "retryable_failure" }),
    ]);
    const node = model.nodes.find((candidate) => candidate.id === "implementation")!;
    expect(node.state).toBe("blocked");
    expect(node.touches).toBe(2);
    expect(node.history.map((entry) => entry.sequence)).toEqual([3, 4]);
  });

  /** The layout is a pure function of the data, so the same run opens to the same board. An
   * arrival-ordered list would move a card because a page came back in a different order. */
  it("orders nodes by id whatever order the events arrived in", () => {
    const forwards = buildGraphModel([
      event(3, "node_outcome_recorded", { nodeId: "zulu", nextState: "ready" }),
      event(4, "node_outcome_recorded", { nodeId: "alpha", nextState: "ready" }),
    ]);
    const backwards = buildGraphModel([
      event(3, "node_outcome_recorded", { nodeId: "alpha", nextState: "ready" }),
      event(4, "node_outcome_recorded", { nodeId: "zulu", nextState: "ready" }),
    ]);
    expect(forwards.nodes.map((node) => node.id)).toEqual(["alpha", "zulu"]);
    expect(backwards.nodes.map((node) => node.id)).toEqual(["alpha", "zulu"]);
  });

  /**
   * THE CLAIM THE BOARD'S FOOTNOTE MAKES. Nothing in the public API publishes topology, so the
   * model must never report edges as known. A guard rather than a comment: the day someone adds
   * an edge source, this fails and they have to change the footnote too.
   */
  it("never claims to know the edges", () => {
    expect(buildGraphModel([ROSTER]).edgesKnown).toBe(false);
    expect(buildGraphModel([]).edgesKnown).toBe(false);
  });

  it("survives an event whose payload is not an object", () => {
    const model = buildGraphModel([event(3, "node_outcome_recorded", null), event(4, "signal_recorded", "text")]);
    expect(model.nodes).toEqual([]);
  });
});

describe("the node thread", () => {
  it("keeps only the events that name the node", () => {
    const events = [
      ROSTER,
      event(3, "node_outcome_recorded", { nodeId: "implementation", nextState: "running" }),
      event(4, "node_outcome_recorded", { nodeId: "deploy", nextState: "ready" }),
      event(5, "execution_paused", { executionId: "demo" }),
    ];
    expect(conversationFor(events, "implementation").map((entry) => entry.sequence)).toEqual([3]);
    // The roster and the pause name no node, so a node thread must not inherit them.
    expect(conversationFor(events, "deploy").map((entry) => entry.sequence)).toEqual([4]);
  });
});

describe("reading a voice and a mood", () => {
  it("associates node-sourced delivery signals without borrowing other sources", () => {
    const entries = [event(1, "signal_recorded", {sourceKind: "node", sourceId: "docs", kind: "node_delivery"}),
      event(2, "signal_recorded", {sourceKind: "user", sourceId: "docs"}),
      event(3, "signal_recorded", {sourceKind: "node", sourceId: "tests"})];
    expect(conversationFor(entries, "docs").map((entry) => entry.sequence)).toEqual([1]);
  });
  it("maps the three actor types and treats anything else as the runtime", () => {
    expect(voiceOf("owner")).toBe("owner");
    expect(voiceOf("agent")).toBe("agent");
    expect(voiceOf("system")).toBe("system");
    expect(voiceOf(null)).toBe("system");
    expect(voiceOf("something-new")).toBe("system");
  });

  /** The moods drive the card's colour, so every state that means "a person is needed" has to
   * land on `waiting` - a blocked node coloured like a running one is the whole failure. */
  it("puts every state that needs a person into waiting", () => {
    for (const state of ["blocked", "waiting_input", "waiting_capacity", "ghost"]) {
      expect(moodOf(state)).toBe("waiting");
    }
    for (const state of ["running", "queued", "linting"]) {
      expect(moodOf(state)).toBe("moving");
    }
    expect(moodOf("succeeded")).toBe("done");
    // Failed needs a person: it must never wear the finished colour.
    expect(moodOf("failed")).toBe("dead");
    expect(moodOf("unknown")).toBe("idle");
  });
});
