import { describe, expect, it } from "vitest";

import type { Bot } from "./team";
import type { TaskState } from "./team-tasks";
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

  // #393 (spec §8): agent-to-agent pair tabs are gone; a message between two agents with no task is
  // said to the room, and only the owner's own line with one agent keeps a direct thread.
  it("splits Everyone and direct lines with the owner, with no pair thread between two agents", () => {
    expect(threads.map((thread) => [thread.key, thread.kind, thread.label, thread.messages.map((m) => m.sequence)])).toEqual([
      ["everyone", "everyone", "Everyone", [1, 2, 3]],
      ["direct:kit-1", "direct", "loja kit 1", [4, 5]],
    ]);
  });

  it("always offers Everyone, even with no messages", () => {
    expect(chatThreads([], BOTS, "studio-operator").map((thread) => thread.key)).toEqual(["everyone"]);
  });

  it("counts unread messages since a tab was last opened", () => {
    expect(unreadCounts(threads, { everyone: 1 })).toEqual({ everyone: 2, "direct:kit-1": 2 });
  });
});

/* #393 (spec §8): threads follow the tasks of the Team tab. These cells catch a fold that keeps
 * threading by actor pair, loses a tagged message to Everyone, or never retires a finished task.
 * Cost: pure functions, milliseconds. */
describe("task threads", () => {
  const tagged = (sequence: number, sender: string, to: string | null, task: string, at: string | null = null): WorkMessage =>
    ({ ...msg(sequence, sender, to), task, at });
  const task = (taskId: string, fields: Partial<TaskState>): TaskState => ({ taskId, issue: null, pr: null, lane: null, headSha: null,
    journeys: [], step: "implement", blockedBy: null, reviewers: [], mergeSha: null, repoUrl: null, lastSequence: 0, ...fields });
  const now = Date.parse("2026-10-08T12:00:00Z");

  it("folds messages tagged with one task into one thread labelled by its PR, and untagged ones into Everyone", () => {
    const result = chatThreads(
      [tagged(1, "kit-1", "kit-2", "issue-384", "2026-10-08T11:00:00Z"), msg(2, "kit-2", "kit-1"), tagged(3, "kit-2", null, "issue-384", "2026-10-08T11:30:00Z")],
      BOTS, "studio-operator", [task("issue-384", { issue: 384, pr: 388, lastSequence: 3 })], now);
    expect(result.map((thread) => [thread.key, thread.kind, thread.label, thread.messages.map((m) => m.sequence), thread.older ?? false])).toEqual([
      ["everyone", "everyone", "Everyone", [2], false],
      ["task:issue-384", "task", "PR #388 · issue #384", [1, 3], false],
    ]);
  });

  it("opens a task's thread from its records before its first message, titled by the issue", () => {
    const result = chatThreads([], BOTS, "studio-operator", [task("issue-390", { issue: 390 })], now);
    expect(result.map((thread) => [thread.key, thread.label])).toEqual([["everyone", "Everyone"], ["task:issue-390", "Issue #390"]]);
  });

  it("marks a merged task, or one quiet for the threshold, as older", () => {
    const result = chatThreads(
      [tagged(1, "kit-1", null, "issue-1", "2026-10-08T01:00:00Z"), tagged(2, "kit-1", null, "issue-2", "2026-10-08T11:00:00Z")],
      BOTS, "studio-operator",
      [task("issue-1", { issue: 1 }), task("issue-2", { issue: 2 }), task("issue-3", { issue: 3, step: "merged" })], now);
    expect(Object.fromEntries(result.filter((thread) => thread.kind === "task").map((thread) => [thread.key, thread.older]))).toEqual({
      "task:issue-1": true, "task:issue-2": false, "task:issue-3": true,
    });
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
