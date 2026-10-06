import { describe, expect, it } from "vitest";

import type { Bot } from "./team";
import type { RuntimeEvent } from "./types";
import type { WorkMessage } from "./work-conversation";
import { chatThreads, describeActivity, namesOf, parseMention, sealedNotesPending, unreadCounts } from "./threads";

const bot = (key: string, name: string): Bot => ({ key, actorId: key, name, hue: 0, role: null, doingNow: "", lastRecordAt: null,
  lastSequence: 0, state: "working", quietMinutes: null, shared: false, native: false, tasks: [] });
const BOTS = [bot("coordinator", "Coordinator"), bot("kit-1", "loja kit 1"), bot("kit-2", "loja kit 2")];
const msg = (sequence: number, sender: string, to: string | null): WorkMessage => ({ id: `event-${sequence}`, sequence, sender, to,
  replyTo: null, text: `m${sequence}`, at: null, provenance: "stored", acknowledged: false });

describe("chatThreads", () => {
  const threads = chatThreads([msg(1, "coordinator", null), msg(2, "kit-1", "kit-2"), msg(3, "kit-2", "kit-1"),
    msg(4, "studio-operator", "kit-1"), msg(5, "kit-1", "studio-operator")], BOTS, "studio-operator");

  it("splits Everyone, bot pairs and direct lines with the owner", () => {
    expect(threads.map((thread) => [thread.key, thread.kind, thread.label, thread.messages.map((m) => m.sequence)])).toEqual([
      ["everyone", "everyone", "Everyone", [1]],
      ["pair:kit-1+kit-2", "pair", "loja kit 1 ↔ loja kit 2", [2, 3]],
      ["direct:kit-1", "direct", "loja kit 1", [4, 5]],
    ]);
  });

  it("always offers Everyone, even with no messages", () => {
    expect(chatThreads([], BOTS, "studio-operator").map((thread) => thread.key)).toEqual(["everyone"]);
  });

  it("counts unread messages since a tab was last opened", () => {
    expect(unreadCounts(threads, { everyone: 1, "pair:kit-1+kit-2": 2 })).toEqual({ everyone: 0, "pair:kit-1+kit-2": 1, "direct:kit-1": 2 });
  });
});

describe("parseMention", () => {
  it("targets one bot by its display name, longest name first", () => {
    expect(parseMention("@loja kit 2 please rebase", BOTS)).toEqual({ to: "kit-2", text: "please rebase" });
    expect(parseMention("@Coordinator  plan?", BOTS)).toEqual({ to: "coordinator", text: "plan?" });
  });
  it("leaves an unknown mention as plain text to everyone", () => {
    expect(parseMention("@nobody hi", BOTS)).toEqual({ to: null, text: "@nobody hi" });
  });
});

describe("sealedNotesPending", () => {
  it("counts operator notes whose envelope is not opened yet", () => {
    const note = (sequence: number): RuntimeEvent => ({ sequence, kind: "signal_recorded", payload: { kind: "operator_note" }, occurredAt: null,
      actorId: "kit-1", actorType: "agent", idempotencyKey: null, eventId: null, evidenceRefs: ["ev"] });
    expect(sealedNotesPending([note(1), note(2)], { 1: { to: null, replyTo: null, text: "x" } })).toBe(1);
  });
});

describe("describeActivity", () => {
  it("reads as bot, verb, object", () => {
    const names = namesOf(BOTS);
    const envelopes = { 7: { to: "studio-operator", replyTo: null, text: "Merge now?" }, 8: { to: "kit-2", replyTo: null, text: "Take the cart" } };
    expect(describeActivity({ sequence: 7, actorId: "kit-1", occurredAt: null, text: "Merge now?" }, names, envelopes, "studio-operator").text)
      .toBe("loja kit 1 asked you “Merge now?”");
    expect(describeActivity({ sequence: 8, actorId: "coordinator", occurredAt: null, text: "Take the cart" }, names, envelopes, "studio-operator").text)
      .toBe("Coordinator told loja kit 2 “Take the cart”");
    expect(describeActivity({ sequence: 9, actorId: "kit-9", occurredAt: null, text: null }, names, {}, "studio-operator").text)
      .toBe("kit-9 recorded a sealed note");
  });

  it("describes journey records in words, never as their JSON", () => {
    const names = namesOf(BOTS);
    const title = (contractId: string, stepId: string) => ({ "kit:cart": "Cart", "kit:home": "Home", "kit:kit": "Kit page" } as Record<string, string>)[`${contractId}:${stepId}`] ?? stepId;
    const capture = JSON.stringify({ protocol: "graphhelm-screen-capture-v1", contractId: "kit", stepId: "cart", revision: "a".repeat(40), dirty: false, viewport: { width: 1, height: 1 }, observer: "kit-1" });
    const walked = JSON.stringify({ protocol: "graphhelm-transition-walked-v1", contractId: "kit", fromStepId: "home", toStepId: "kit", revision: "a".repeat(40), observer: "kit-1", fromCaptureId: "a", toCaptureId: "b" });
    expect(describeActivity({ sequence: 1, actorId: "kit-1", occurredAt: null, text: capture }, names, {}, "studio-operator", title).text).toBe("loja kit 1 captured Cart");
    expect(describeActivity({ sequence: 2, actorId: "kit-1", occurredAt: null, text: walked }, names, {}, "studio-operator", title).text).toBe("loja kit 1 walked Home → Kit page");
    expect(describeActivity({ sequence: 3, actorId: "kit-1", occurredAt: null, text: capture.replace("cart", "pay") }, names, {}, "studio-operator").text).toBe("loja kit 1 captured pay");
  });
});
