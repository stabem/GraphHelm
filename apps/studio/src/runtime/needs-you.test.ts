import { describe, expect, it } from "vitest";

import type { EnvelopeRecord } from "../graph/ledger";
import type { ExecutionStatus, NativeChatRequest, RuntimeEvent } from "./types";
import { needsYou, RUNTIME_SILENT, type NeedsYouInput } from "./needs-you";

const STATUS: ExecutionStatus = {
  executionId: "run", mode: "supervised", status: "running", attention: "can_sleep", attentionReasons: [],
  nodeStateCounts: {}, untriagedInterruptions: [], silenceUnevaluated: [], startedAt: null, lastEventAt: null,
  nodeLastEventAt: {}, headSequence: 10,
};

function signal(sequence: number, actorId: string, signalId: string | null): RuntimeEvent {
  return { sequence, kind: "signal_recorded", payload: signalId === null ? { kind: "operator_note" } : { kind: "operator_note", signalId }, occurredAt: "2026-10-05T11:00:00Z",
    actorId, actorType: actorId === "studio-operator" ? "owner" : "agent", idempotencyKey: null, eventId: `e${sequence}`, evidenceRefs: [`ev${sequence}`] };
}

const base = (overrides: Partial<NeedsYouInput> = {}): NeedsYouInput => ({
  status: STATUS, stale: false, events: [], envelopes: {}, nativeRequests: [], pendingDraftIds: [], nodeNames: {},
  operatorId: "studio-operator", ...overrides,
});

const question = [signal(5, "kit-3", "sig-q")];
const asked: EnvelopeRecord = { 5: { to: "studio-operator", replyTo: null, text: "Merge now?", recommendations: ["Wait", "Merge"] } };

describe("needsYou", () => {
  it("is dark when the Runtime answered and nothing is open", () => {
    expect(needsYou(base())).toEqual({ state: { kind: "dark" }, items: [] });
  });

  it("is lit with one card per open question, carrying its choices", () => {
    const result = needsYou(base({ events: question, envelopes: asked }));
    expect(result.state).toEqual({ kind: "lit", count: 1 });
    expect(result.items).toEqual([{ kind: "question", key: "question:sig-q", asker: "kit-3", text: "Merge now?", signalId: "sig-q",
      recommendations: ["Wait", "Merge"], at: "2026-10-05T11:00:00Z", sequence: 5 }]);
  });

  it("gives two questions without signal ids distinct keys", () => {
    const events = [signal(5, "kit-3", null), signal(6, "kit-4", null)];
    const envelopes: EnvelopeRecord = {
      5: { to: "studio-operator", replyTo: null, text: "First?" },
      6: { to: "studio-operator", replyTo: null, text: "Second?" },
    };
    const keys = needsYou(base({ events, envelopes })).items.map((item) => item.key);
    expect(keys).toHaveLength(2);
    expect(new Set(keys).size).toBe(2);
    expect(keys).toEqual(["question:seq-6", "question:seq-5"]);
  });

  it("closes a question the owner refused, so the beacon count drops", () => {
    expect(needsYou(base({ events: question, envelopes: asked })).state).toEqual({ kind: "lit", count: 1 });
    const refused = [...question, { ...signal(6, "studio-operator", "sig-r"), payload: { kind: "owner_refusal", signalId: "sig-r" } }];
    const result = needsYou(base({ events: refused, envelopes: { ...asked,
      6: { to: "kit-3", replyTo: "sig-q", text: JSON.stringify({ protocol: "graphhelm-owner-refusal-v1" }) } } }));
    expect(result.items).toEqual([]);
    expect(result.state).toEqual({ kind: "dark" });
    const two = [signal(4, "kit-2", "sig-p"), ...question];
    const open: EnvelopeRecord = { ...asked, 4: { to: "studio-operator", replyTo: null, text: "Deploy?" } };
    const second: RuntimeEvent = { ...signal(6, "studio-operator", "sig-r"), payload: { kind: "owner_refusal", signalId: "sig-r" } };
    const after = needsYou(base({ events: [...two, second], envelopes: { ...open, 6: { to: "kit-3", replyTo: "sig-q", text: JSON.stringify({ protocol: "graphhelm-owner-refusal-v1" }) } } }));
    expect(after.state).toEqual({ kind: "lit", count: 1 });
    expect(after.items.map((item) => item.key)).toEqual(["question:sig-p"]);
  });

  it("closes a question only when the owner replies to it", () => {
    const ownerReply = [...question, signal(6, "studio-operator", "sig-a")];
    expect(needsYou(base({ events: ownerReply, envelopes: { ...asked, 6: { to: "kit-3", replyTo: "sig-q", text: "Wait" } } })).items).toEqual([]);
    const agentReply = [...question, signal(6, "kit-1", "sig-b")];
    expect(needsYou(base({ events: agentReply, envelopes: { ...asked, 6: { to: "kit-3", replyTo: "sig-q", text: "merge it" } } })).items).toHaveLength(1);
  });

  it("is unknown, never dark, when the connection is stale or the status is missing", () => {
    expect(needsYou(base({ stale: true })).state).toEqual({ kind: "unknown", reason: RUNTIME_SILENT });
    expect(needsYou(base({ stale: true, events: question, envelopes: asked })).state).toEqual({ kind: "unknown", reason: RUNTIME_SILENT });
    expect(needsYou(base({ status: null })).state.kind).toBe("unknown");
  });

  it("is unknown, never dark, when the native bridge is not answering", () => {
    expect(needsYou(base({ nativeRequests: null })).state.kind).toBe("unknown");
  });

  it("lists native requests the ledger cannot confirm and leaves confirmed ones out", () => {
    const request = (state: NativeChatRequest["state"]): NativeChatRequest => ({ requestId: `r-${state}`, nodeId: "start", threadId: "t-1", title: "loja kit 2", sourceDirectory: "C:/p", state });
    const items = needsYou(base({ nativeRequests: [request("unobserved"), request("blocked"), request("completed"), request("requested")] })).items;
    expect(items.map((item) => item.key)).toEqual(["native:r-unobserved", "native:r-blocked"]);
  });

  it("lists waiting and blocked steps by name, and pending drafts", () => {
    const status = { ...STATUS, attentionReasons: [{ kind: "waiting_input_node", node: "start" }, { kind: "blocked_node", node: "cart" }, { kind: "untriaged_interruption", node: "pay" }] };
    const items = needsYou(base({ status, nodeNames: { cart: "Cart page" }, pendingDraftIds: ["d-1"] })).items;
    expect(items).toEqual([
      { kind: "draft", key: "draft:d-1", draftId: "d-1" },
      { kind: "waiting_step", key: "waiting_input_node:start", nodeId: "start", name: "start", reason: "waiting_input_node" },
      { kind: "blocked_step", key: "blocked_node:cart", nodeId: "cart", name: "Cart page", reason: "blocked_node" },
      { kind: "blocked_step", key: "untriaged_interruption:pay", nodeId: "pay", name: "pay", reason: "untriaged_interruption" },
    ]);
  });
});
