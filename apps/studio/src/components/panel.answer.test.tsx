import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { NodePanel } from "./panel";
import type { GraphNode, NodeStateName } from "../graph/model";
import type { ClaimEvidence } from "../runtime/types";
import type { AnswerOutcome } from "./answer";

afterEach(cleanup);

/**
 * #1187 review: the state gate at `panel.tsx` had no cell at all.
 *
 * Deleting `node.state === "waiting_input" &&` left the whole Studio suite green, because
 * `NodePanel` was never rendered by any test — so the one condition that keeps the answer form
 * off a node that is not waiting was held by nothing but its own comment. The comment forbids
 * exactly the behaviour the deletion produces, which is the shape where a reader trusts a
 * sentence and the sentence is all there is.
 */

function node(state: NodeStateName): GraphNode {
  return {
    id: "implementation",
    state,
    touches: 3,
    lastEventAt: null,
    history: [],
    reopened: null,
  };
}

const answer = {
  waitSeq: 9,
  onAnswer: vi.fn(async (_evidence: ClaimEvidence[]) => ({
    step: "claimed-and-cleared",
  }) as AnswerOutcome),
  hash: vi.fn(async (file: File) => ({
    contentHash: `sha256:${"a".repeat(64)}`,
    size: file.size,
  })),
};

describe("NodePanel's answer affordance", () => {
  it("offers it for a node that is waiting", () => {
    render(<NodePanel node={node("waiting_input")} events={[]} onClose={() => {}} answer={answer} />);
    expect(screen.getByRole("button", { name: "Answer with no evidence" })).toBeTruthy();
  });

  // THE GATE. A claim against a node that is not waiting is refused by the fold as `not_waiting`,
  // so an answer form on one exists only to produce that refusal — and a person who was offered
  // the action reasonably reads the refusal as the node's answer rather than as the screen's
  // mistake. One cell per state that could plausibly carry a stale form.
  for (const state of ["succeeded", "running", "failed", "ready", "blocked"] as NodeStateName[]) {
    it(`withholds it from a node that is ${state}`, () => {
      render(<NodePanel node={node(state)} events={[]} onClose={() => {}} answer={answer} />);
      expect(screen.queryByRole("button", { name: /^Answer/ })).toBeNull();
      expect(screen.queryByLabelText("Artefact")).toBeNull();
    });
  }

  // And the prop's ABSENCE is not a claim about the node. A panel opened without a connected
  // Runtime, or without the graph file a claim must carry, renders nothing here rather than a
  // button that cannot work — and says nothing about whether the node is waiting.
  it("renders nothing when the panel was given no way to answer", () => {
    render(<NodePanel node={node("waiting_input")} events={[]} onClose={() => {}} />);
    expect(screen.queryByRole("button", { name: /^Answer/ })).toBeNull();
    expect(screen.getByText("waiting input")).toBeTruthy();
  });
});
