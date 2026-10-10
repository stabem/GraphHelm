import { describe, expect, it } from "vitest";

import type { GraphModel } from "../graph/model";
import type { EnvelopeRecord } from "../graph/ledger";
import type { ClaudeTaskReadModel, TaskState } from "./team-tasks";
import type { RuntimeEvent } from "./types";
import { digestOf } from "./customs";
import { readTaskEventRecords } from "./team-tasks";
import { teamLinks, teamModel, type TeamInput } from "./team";

const NOW = Date.parse("2026-10-05T12:00:00Z");
const minutesAgo = (m: number) => new Date(NOW - m * 60_000).toISOString();

function record(sequence: number, actorId: string, at: string, kind = "operator_note"): RuntimeEvent {
  return {
    sequence, kind: "signal_recorded", payload: { kind, signalId: `sig-${sequence}` }, occurredAt: at,
    actorId, actorType: actorId === "studio-operator" ? "owner" : "agent",
    idempotencyKey: null, eventId: `e-${sequence}`, evidenceRefs: [`ev-${sequence}`],
  };
}

function input(overrides: Partial<TeamInput> = {}): TeamInput {
  return {
    events: [], envelopes: {}, personas: {}, nativeLinks: {}, aliases: {}, model: null,
    claudeTasks: null, waitingAskers: new Set(), now: NOW, ...overrides,
  };
}

// 40 merge actors that recorded five hours ago (no note, no task record), then the live crew.
const oldCrowd = Array.from({ length: 40 }, (_, i) => record(i + 1, `merge-${i}`, minutesAgo(300), "node_completed"));
const live: RuntimeEvent[] = [
  record(100, "coordinator", minutesAgo(3)),
  record(101, "kit-1", minutesAgo(2)),
  record(102, "kit-2", minutesAgo(50)),
  record(103, "kit-3", minutesAgo(5)),
  record(104, "codex", minutesAgo(4)),
];
const envelopes: EnvelopeRecord = {
  100: { to: null, replyTo: null, text: "Splitting Leva 5 into three slices\nmore detail" },
  101: { to: "coordinator", replyTo: null, text: "Cart page done" },
  103: { to: "studio-operator", replyTo: null, text: "Merge now or wait?" },
};

describe("teamModel", () => {
  it("makes bots of personas and recent actors and folds the rest into other recorders", () => {
    const team = teamModel(input({ events: [...oldCrowd, ...live], envelopes, personas: { coordinator: "Coordinator of Leva 5\nKeeps the plan" } }));
    expect(team.bots.map((bot) => bot.key)).toEqual(["coordinator", "codex", "kit-1", "kit-2", "kit-3"]);
    expect(team.otherRecorders).toHaveLength(40);
    expect(team.otherRecorders[0]).toEqual({ actorId: "merge-0", count: 1, lastRecordAt: minutesAgo(300) });
  });

  it("names the shared codex actor and never maps it to a thread", () => {
    const codex = teamModel(input({ events: live })).bots.find((bot) => bot.key === "codex")!;
    expect(codex.name).toBe("Codex (shared)");
    expect(codex.shared).toBe(true);
    expect(codex.native).toBe(false);
  });

  it("says what each bot is doing from the first line of its newest opened note", () => {
    const team = teamModel(input({ events: live, envelopes }));
    expect(team.bots.find((bot) => bot.key === "coordinator")!.doingNow).toBe("Splitting Leva 5 into three slices");
    // #446: kit-2's note is still sealed. It exists, so the card says it is opening, never that
    // there is no note (the #301 observer saw "No note yet" on every bot for ~5 s after a load).
    expect(team.bots.find((bot) => bot.key === "kit-2")!.doingNow).toBe("Opening…");
    // A bot whose records carry no note at all still says so.
    const quiet = teamModel(input({ events: [record(200, "kit-4", minutesAgo(1), "agent_status")] }));
    expect(quiet.bots.find((bot) => bot.key === "kit-4")!.doingNow).toBe("No note yet");
  });

  it("derives state from records only", () => {
    const team = teamModel(input({ events: live, envelopes, waitingAskers: new Set(["kit-3"]) }));
    const by = (key: string) => team.bots.find((bot) => bot.key === key)!;
    expect(by("kit-1").state).toBe("working");
    expect(by("kit-2").state).toBe("quiet");
    expect(by("kit-2").quietMinutes).toBe(50);
    expect(by("kit-3").state).toBe("waiting_for_you");
  });

  it("marks a bot done when its last task completed and nothing newer was recorded", () => {
    const claudeTasks: ClaudeTaskReadModel = {
      executionId: "run", rejected: 0,
      tasks: [{
        executionId: "run", nativeTaskId: "t1", taskSubject: "Build cart", createdByTeammateName: null,
        completedByTeammateName: null, sourceId: "kit-1", parentSessionId: "p", createdSequence: 90,
        createdAt: minutesAgo(30), createdEvidenceId: "ev-90", completedSequence: 101, completedAt: minutesAgo(2),
        completedEvidenceId: "ev-101",
      }],
    };
    const kit1 = teamModel(input({ events: live, claudeTasks })).bots.find((bot) => bot.key === "kit-1")!;
    expect(kit1.state).toBe("done");
    expect(kit1.tasks).toEqual([{ id: "t1", title: "Build cart", done: true, source: "task_record", nodeId: null, sequence: 90, doneSequence: 101 }]);
  });

  it("takes tasks from assigned graph nodes and never from note text", () => {
    const model = { nodes: [{ id: "cart", declaredName: "Cart page", state: "waiting_input", assignedActor: { type: "agent", id: "kit-1" }, touches: 1, lastEventAt: null, history: [{ sequence: 60 }] }] } as unknown as GraphModel;
    const kit1 = teamModel(input({ events: live, envelopes, model })).bots.find((bot) => bot.key === "kit-1")!;
    expect(kit1.tasks.map((task) => task.title)).toEqual(["Cart page"]);
    expect(kit1.state).toBe("waiting_for_you");
    expect(teamModel(input({ events: live, envelopes })).bots.find((bot) => bot.key === "coordinator")!.tasks).toEqual([]);
  });

  it("puts coordinator-like bots first and keeps a stable colour per key", () => {
    const a = teamModel(input({ events: live })).bots;
    const b = teamModel(input({ events: [...live].reverse() })).bots;
    expect(a[0].key).toBe("coordinator");
    expect(a.map((bot) => [bot.key, bot.hue])).toEqual(b.map((bot) => [bot.key, bot.hue]));
  });

  it("uses a native persona's chat title as its name", () => {
    const thread = "0d9a6f5e-1111-4222-8333-444455556666";
    const team = teamModel(input({ nativeLinks: { [thread]: { chat: { id: thread, title: "loja kit 2", projectDirectory: "C:/p", updatedAt: 0 }, charter: "Builds checkout", nodeId: "start" } } }));
    expect(team.bots[0]).toMatchObject({ key: thread, name: "loja kit 2", native: true, actorId: null, role: "Builds checkout", state: "quiet", quietMinutes: null });
  });
});

describe("teamLinks", () => {
  it("joins two bots that addressed each other and animates only the last minute", () => {
    const events = [record(1, "kit-1", minutesAgo(10)), record(2, "kit-2", new Date(NOW - 30_000).toISOString())];
    const env: EnvelopeRecord = { 1: { to: "coordinator", replyTo: null, text: "a" }, 2: { to: "kit-1", replyTo: null, text: "b" } };
    const bots = teamModel(input({ events: [...events, record(3, "coordinator", minutesAgo(1))], envelopes: env })).bots;
    expect(teamLinks(events, env, bots, NOW)).toEqual([
      { a: "coordinator", b: "kit-1", count: 1, lastAt: minutesAgo(10), live: false },
      { a: "kit-1", b: "kit-2", count: 1, lastAt: new Date(NOW - 30_000).toISOString(), live: true },
    ]);
  });

  it("never draws a line to the operator", () => {
    const events = [record(1, "kit-1", minutesAgo(1))];
    const env: EnvelopeRecord = { 1: { to: "studio-operator", replyTo: null, text: "q" } };
    const bots = teamModel(input({ events, envelopes: env })).bots;
    expect(teamLinks(events, env, bots, NOW)).toEqual([]);
  });

  it("#532: keeps every lane the run knows, idle or not, and shows its claimed task", () => {
    const task = { lane: "gh-claude-9", reviewers: ["gh-claude-5"], title: "Every lane shows", issue: 532, lastSequence: 7 } as unknown as TaskState;
    const older = { lane: "gh-claude-9", reviewers: [], title: "Older work", issue: 500, lastSequence: 3 } as unknown as TaskState;
    const team = teamModel(input({
      events: [record(1, "gh-claude-9", minutesAgo(600), "task.claimed"), record(2, "old-noter", minutesAgo(600)), record(3, "merge-x", minutesAgo(600), "node_completed")],
      envelopes: { 2: { to: null, replyTo: null, text: "still here" } },
      taskStates: [older, task],
    }));
    expect(team.bots.map((bot) => bot.key).sort()).toEqual(["gh-claude-5", "gh-claude-9", "old-noter"]);
    expect(team.otherRecorders.map((other) => other.actorId)).toEqual(["merge-x"]);
    const lane = team.bots.find((bot) => bot.key === "gh-claude-9");
    expect(lane?.doingNow).toBe("#532 Every lane shows");
    expect(lane?.state).toBe("quiet");
    expect(team.bots.find((bot) => bot.key === "old-noter")?.doingNow).toBe("still here");
  });

  it("#536: a lane whose newest slice is merged reads as last delivered", () => {
    const merged = { lane: "gh-claude-9", reviewers: [], title: "Every lane shows", issue: 532, lastSequence: 7, step: "merged" } as unknown as TaskState;
    const team = teamModel(input({ events: [record(1, "gh-claude-9", minutesAgo(600), "task.merged")], taskStates: [merged] }));
    expect(team.bots.find((bot) => bot.key === "gh-claude-9")?.doingNow).toBe("Last delivered #532 Every lane shows");
  });

  it("#737: counts an admitted shared-signer assignment for its reviewer lane", async () => {
    const detail = { schema: "graphhelm-task-event-v1", revision: 1, taskId: "issue-737", pr: 738,
      headSha: "a".repeat(40), reviewer: "codex-8" };
    const content = JSON.stringify({ type: "task.review_assigned", source: { id: "codex" }, description: JSON.stringify(detail) });
    const hash = (await digestOf(new TextEncoder().encode(content).buffer, globalThis.crypto.subtle)).slice("sha256:".length);
    const evidence = { evidenceId: "ev-201", contentSha256: hash, content, mediaType: "application/json", sensitivity: "internal" } as const;
    const event = { ...record(201, "codex", minutesAgo(2), "task.review_assigned"), payload: { kind: "task.review_assigned", envelopeSha256: hash } };
    const taskRecords = await readTaskEventRecords({ executionId: "run", events: [event], readEvidence: async () => evidence });
    expect(taskRecords).toHaveLength(1);
    const team = teamModel(input({ events: [event], taskRecords,
      taskStates: [{ lane: "codex-8", reviewers: [], title: "Active work", issue: 737, lastSequence: 201 } as unknown as TaskState] }));
    expect(team.bots.find((bot) => bot.key === "codex-8")).toMatchObject({ state: "working", quietMinutes: 2, lastRecordAt: minutesAgo(2) });
  });
});
