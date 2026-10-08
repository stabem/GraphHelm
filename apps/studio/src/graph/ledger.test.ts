import { describe, expect, it } from "vitest";

import type { RuntimeEvent } from "../runtime/types";
import { address_of, openQuestions, readable_content, recommendations_of } from "./ledger";

function signal(
  sequence: number,
  actor: { id: string; type: string },
  payload: unknown = {},
): RuntimeEvent {
  return {
    sequence,
    kind: "signal_recorded",
    payload,
    occurredAt: `2026-08-31T09:00:${String(sequence).padStart(2, "0")}Z`,
    actorId: actor.id,
    actorType: actor.type,
    idempotencyKey: `k${sequence}`,
    eventId: `event-${sequence}`,
    evidenceRefs: [`ev-${sequence}`],
  } as never;
}

const OPERATOR = "studio-operator";

it("shows delivery reasons and owner edits as inert prose without inventing links", () => {
  expect(readable_content(JSON.stringify({type:"node_delivery", description:JSON.stringify({summary:"Created a rule",reason:"Purchase recovery"})}), "application/json")).toBe("Created a rule\n\nPurchase recovery");
  expect(readable_content(JSON.stringify({type:"owner_document_changed", description:JSON.stringify({path:"docs/rule.md",reason:"<script>inert</script>"})}), "application/json")).toContain("<script>inert</script>");
});

it("renders a sealed model reply as its words rather than a JSON accounting blob", () => {
  const reply = JSON.stringify({
    text: "Browser review incomplete: no screenshots or interaction evidence were supplied.",
    usage: { inputTokens: 125, outputTokens: 17 },
  });
  expect(readable_content(reply, "application/json")).toBe(
    "Browser review incomplete: no screenshots or interaction evidence were supplied.",
  );
  expect(readable_content(JSON.stringify({ text: "untrusted lookalike" }), "application/json"))
    .toContain('"text":"untrusted lookalike"');
});

it("C5 renders sealed run-team words without exposing protocol JSON in the human thread", () => {
  const sealed = JSON.stringify({ type: "run_team_message", description: JSON.stringify({
    protocol: "graphhelm-run-team-v1", executionId: "run-1", text: "Please review the view." }) });
  expect(readable_content(sealed, "application/json")).toBe("Please review the view.");
  const malformed = JSON.stringify({ type: "run_team_message", description: "{broken" });
  expect(readable_content(malformed, "application/json")).toBe("Run team record unavailable.");
});

describe("the shared ledger of unanswered questions", () => {
  it("owes a question an agent addressed to the operator, with the signalId an answer must cite", () => {
    const events = [signal(5, { id: "codex", type: "agent" }, { signalId: "sig-5" })];
    const envelopes = { 5: { to: OPERATOR, replyTo: null, text: "which region?" } };
    expect(openQuestions(events, envelopes, OPERATOR)).toEqual([
      { asker: "codex", at: "2026-08-31T09:00:05Z", text: "which region?", signalId: "sig-5", sequence: 5 },
    ]);
  });

  it("settles a debt only when the OPERATOR answers it", () => {
    const events = [
      signal(5, { id: "codex", type: "agent" }, { signalId: "sig-5" }),
      // Another AGENT replying does not settle a question addressed to the operator.
      signal(6, { id: "helper", type: "agent" }, { signalId: "sig-6" }),
      signal(7, { id: OPERATOR, type: "owner" }, { signalId: "sig-7" }),
    ];
    const withAgentReply = {
      5: { to: OPERATOR, replyTo: null, text: "which region?" },
      6: { to: "codex", replyTo: "sig-5", text: "try eu-west" },
      7: { to: "codex", replyTo: null, text: "unrelated note" },
    };
    expect(openQuestions(events, withAgentReply, OPERATOR)).toHaveLength(1);

    const withOperatorAnswer = {
      ...withAgentReply,
      7: { to: "codex", replyTo: "sig-5", text: "eu-west, use the shared VPC" },
    };
    expect(openQuestions(events, withOperatorAnswer, OPERATOR)).toEqual([]);
  });

  it("treats a reply to the operator's own signal as a receipt, not a question", () => {
    const events = [
      signal(3, { id: OPERATOR, type: "owner" }, { signalId: "sig-3" }),
      signal(4, { id: "codex", type: "agent" }, { signalId: "sig-4" }),
    ];
    const envelopes = {
      3: { to: "codex", replyTo: null, text: "deploy it" },
      4: { to: OPERATOR, replyTo: "sig-3", text: "deployed, all green" },
    };
    expect(openQuestions(events, envelopes, OPERATOR)).toEqual([]);
  });

  it("never invents a question from an unaddressed or unopened envelope", () => {
    const events = [
      signal(5, { id: "codex", type: "agent" }, { signalId: "sig-5" }),
      signal(6, { id: "codex", type: "agent" }, { signalId: "sig-6" }),
    ];
    // 5 speaks to the room (to: null); 6's envelope never opened.
    expect(openQuestions(events, { 5: { to: null, replyTo: null, text: "thinking..." } }, OPERATOR)).toEqual([]);
  });
});

describe("reading a sealed envelope", () => {
  it("reads the sentence and the address out of a JSON envelope", () => {
    const sealed = JSON.stringify({ description: "which region?", to: OPERATOR, replyTo: "sig-1" });
    expect(readable_content(sealed, "application/json")).toBe("which region?");
    expect(address_of(sealed, "application/json")).toEqual({ to: OPERATOR, replyTo: "sig-1", task: null });
  });

  it("reads the task a message belongs to, and only when it is a task id (#393)", () => {
    expect(address_of(JSON.stringify({ description: "x", task: "issue-384" }), "application/json").task).toBe("issue-384");
    expect(address_of(JSON.stringify({ description: "x", task: "../etc" }), "application/json").task).toBeNull();
  });

  it("returns non-JSON content unchanged and address-free", () => {
    expect(readable_content("plain words", "text/plain")).toBe("plain words");
    expect(address_of("plain words", "text/plain")).toEqual({ to: null, replyTo: null, task: null });
    // Claims JSON, is not: the bytes still reach the reader, and no address is guessed.
    expect(readable_content("{broken", "application/json")).toBe("{broken");
    expect(address_of("{broken", "application/json")).toEqual({ to: null, replyTo: null, task: null });
  });
});

describe("recommendations_of", () => {
  it("reads the graph-signal recommendations as choice labels", () => {
    const sealed = JSON.stringify({ to: "studio-operator", description: "Merge now?", recommendations: ["Wait", "Merge", " Merge ", ""] });
    expect(recommendations_of(sealed, "application/json")).toEqual(["Wait", "Merge"]);
  });
  it("caps the list at four and drops labels longer than 120 characters", () => {
    const sealed = JSON.stringify({ recommendations: ["a", "b", "x".repeat(121), "c", "d", "e"] });
    expect(recommendations_of(sealed, "application/json")).toEqual(["a", "b", "c", "d"]);
  });
  it("yields nothing for plain text, broken JSON or a wrong type", () => {
    expect(recommendations_of("Merge?", "text/plain")).toEqual([]);
    expect(recommendations_of("{broken", "application/json")).toEqual([]);
    expect(recommendations_of(JSON.stringify({ recommendations: "Merge" }), "application/json")).toEqual([]);
    expect(recommendations_of(JSON.stringify({ recommendations: [1, null] }), "application/json")).toEqual([]);
  });
});
