import { describe, expect, it } from "vitest";

import type { Bot } from "./team";
import { describeActivity, namesOf, parseMention } from "./threads";

const bot = (key: string, name: string): Bot => ({ key, actorId: key, name, hue: 0, role: null, doingNow: "", lastRecordAt: null,
  lastSequence: 0, state: "working", quietMinutes: null, shared: false, native: false, tasks: [] });
const BOTS = [bot("coordinator", "Coordinator"), bot("kit-1", "loja kit 1"), bot("kit-2", "loja kit 2")];
describe("parseMention", () => {
  it("targets one bot by its display name, longest name first", () => {
    expect(parseMention("@loja kit 2 please rebase", BOTS)).toEqual({ to: "kit-2", text: "please rebase" });
    expect(parseMention("@Coordinator  plan?", BOTS)).toEqual({ to: "coordinator", text: "plan?" });
  });
  it("leaves an unknown mention as plain text to everyone", () => {
    expect(parseMention("@nobody hi", BOTS)).toEqual({ to: null, text: "@nobody hi" });
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

  it("renders a lane operator_note with replyTo as an answer to the owner's ask", () => {
    const names = namesOf(BOTS);
    const envelopes = {
      10: { to: "kit-1", replyTo: null, text: "Owner asks: status?" },
      11: { to: "studio-operator", replyTo: "ask-10", text: "The fix is ready." },
    };
    expect(describeActivity({ sequence: 11, actorId: "kit-1", occurredAt: null, text: "The fix is ready." }, names, envelopes, "studio-operator").text)
      .toBe("loja kit 1 replied to you “The fix is ready.”");
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

  // #442: the #301 observer saw an owner alias and capture documents printed as raw JSON.
  it("describes owner records in words and never prints a JSON document", () => {
    const names = namesOf(BOTS);
    const alias = JSON.stringify({ protocol: "graphhelm-actor-alias-v1", displayName: "Loja kit 1" });
    const refusal = JSON.stringify({ protocol: "graphhelm-owner-refusal-v1", reason: "not now" });
    const env = { 1: { to: "kit-1", replyTo: null, text: alias }, 2: { to: "kit-1", replyTo: "sig-q", text: refusal } };
    expect(describeActivity({ sequence: 1, actorId: "studio-operator", occurredAt: null, text: alias }, names, env, "studio-operator").text)
      .toBe("You named kit-1 “Loja kit 1”");
    expect(describeActivity({ sequence: 2, actorId: "studio-operator", occurredAt: null, text: refusal }, names, env, "studio-operator").text)
      .toBe("You refused a question from loja kit 1");
    const unknown = JSON.stringify({ schema: "graphhelm-something-v1", value: 1 });
    expect(describeActivity({ sequence: 3, actorId: "kit-1", occurredAt: null, text: unknown }, names, {}, "studio-operator").text)
      .toBe("loja kit 1 recorded a graphhelm-something-v1 record");
    const cut = JSON.stringify({ protocol: "graphhelm-screen-capture-v1", contractId: "kit", stepId: "cart", observer: "x".repeat(300) }).slice(0, 220);
    expect(describeActivity({ sequence: 4, actorId: "kit-1", occurredAt: null, text: cut }, names, {}, "studio-operator").text)
      .toBe("loja kit 1 recorded a record");
  });
});
