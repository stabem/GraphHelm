import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { createElement } from "react";
import { digestOf } from "./customs";
import { MissionGraph } from "../components/mission-graph";
import { foldTaskEvents, isClaudeTaskSignal, parseTaskEvent, readClaudeTasks, type TaskEventKind, type TaskEventRecord } from "./team-tasks";
import type { EvidenceContent, RuntimeEvent } from "./types";

const executionId = "run-task";

function taskRecord(sequence: number, kind: TaskEventKind, actorId: string, taskId: string, fields: Record<string, unknown>): TaskEventRecord {
  const parsed = parseTaskEvent(kind, actorId, JSON.stringify({
    schema: "graphhelm-task-event-v1", taskId, revision: sequence, at: new Date(sequence * 1000).toISOString(), ...fields,
  }));
  if (!parsed) throw new Error(`invalid ${kind} fixture`);
  return { ...parsed, sequence, occurredAt: new Date(sequence * 1000).toISOString() };
}

async function sealed(phase: "created" | "completed", id: string, teammateName: string | null = "reviewer"): Promise<EvidenceContent> {
  const actor = "claude-session-session-1";
  const kind = `agent_task_${phase}`;
  const detail = { protocol: "graphhelm-native-task-v1", executionId, host: "claude", parentSessionId: "session-1", nativeTaskId: "task-1", taskSubject: "Review the checkout flow", teammateName, phase };
  const content = JSON.stringify({ id: `signal-${phase}`, source: { type: "tool", id: actor }, type: kind, evidence: [executionId], description: JSON.stringify(detail) });
  const hash = await digestOf(new TextEncoder().encode(content).buffer, globalThis.crypto.subtle);
  return { evidenceId: id, mediaType: "application/json", sensitivity: "internal", contentSha256: hash.slice("sha256:".length), content };
}

function event(seq: number, phase: "created" | "completed", evidence: EvidenceContent): RuntimeEvent {
  const actor = "claude-session-session-1";
  return { sequence: seq, kind: "signal_recorded", payload: { kind: `agent_task_${phase}`, sourceId: actor, sourceKind: "tool", signalId: `signal-${phase}`, envelopeSha256: evidence.contentSha256 }, occurredAt: null, actorId: actor, actorType: "agent", idempotencyKey: null, eventId: `event-${seq}`, evidenceRefs: [evidence.evidenceId] };
}

describe("readClaudeTasks", () => {
  it("verifies sealed create and completion events but labels them as observations", async () => {
    const created = await sealed("created", "ev-created", "planner");
    const completed = await sealed("completed", "ev-completed");
    const result = await readClaudeTasks({ executionId, events: [event(2, "completed", completed), event(1, "created", created)], readEvidence: vi.fn(async (_run, id) => id === created.evidenceId ? created : completed) });
    expect(result.rejected).toBe(0);
    expect(result.tasks).toHaveLength(1);
    expect(result.tasks[0]).toMatchObject({ taskSubject: "Review the checkout flow", createdByTeammateName: "planner", completedByTeammateName: "reviewer", sourceId: "claude-session-session-1", parentSessionId: "session-1", createdSequence: 1, completedSequence: 2 });
  });

  it("keeps a completion without its creation and rejects tampered evidence", async () => {
    const completed = await sealed("completed", "ev-completed");
    const signal = event(2, "completed", completed);
    const result = await readClaudeTasks({ executionId, events: [signal], readEvidence: vi.fn(async () => completed) });
    expect(result.tasks[0]).toMatchObject({ createdSequence: null, createdByTeammateName: null, completedSequence: 2, completedByTeammateName: "reviewer" });
    expect(isClaudeTaskSignal(signal)).toBe(true);
    const bad = await readClaudeTasks({ executionId, events: [{ ...signal, payload: { ...(signal.payload as Record<string, unknown>), sourceId: "different-actor" } }], readEvidence: vi.fn(async () => completed) });
    expect(bad.tasks).toHaveLength(0);
    expect(bad.rejected).toBe(1);
  });

  it("preserves an unreported teammate as unknown", async () => {
    const created = await sealed("created", "ev-created", null);
    const result = await readClaudeTasks({ executionId, events: [event(1, "created", created)], readEvidence: vi.fn(async () => created) });
    expect(result.tasks[0].createdByTeammateName).toBeNull();
  });
});

describe("task review assignment recovery", () => {
  it("shows an assigned reviewer as review work when no author or PR-opened record exists", () => {
    const [task] = foldTaskEvents([
      taskRecord(1, "task.review_assigned", "coordinator", "issue-976", {
        pr: 977, headSha: "abcdef1234567", reviewer: "codexrev-1",
      }),
    ]);

    expect(task).toMatchObject({ pr: 977, step: "review", headSha: "abcdef1234567", currentReviewer: "codexrev-1" });
    render(createElement(MissionGraph, {
      mission: {
        contractId: "test", title: "Test", steps: [], tasks: [{
          key: task.key, pr: task.pr, issue: task.issue, title: "Review PR 977", lane: task.lane,
          reviewers: task.reviewers, step: task.step, blocked: false, trust: 1, blockedBy: null,
          rounds: [], headSha: task.headSha, mergeSha: null, repoUrl: null,
        }], summary: { proven: 0, total: 0, inFlight: 1, needYou: 0, readyUnclaimed: 0 },
      },
      selectedStepId: null, selectedTaskKey: null, onSelectStep: vi.fn(), onSelectTask: vi.fn(), onOpenTest: vi.fn(),
    }));
    expect(screen.getByText("codexrev-1")).toBeInTheDocument();
    expect(screen.queryByText("nobody yet")).not.toBeInTheDocument();
  });

  it("attaches a review verdict to its PR after earlier slices of the same issue ended", () => {
    const records: TaskEventRecord[] = [];
    let sequence = 0;
    const claimAndFinish = (pr: number) => {
      records.push(taskRecord(++sequence, "task.claimed", "codex-2", "issue-913", {
        issue: 913, lane: "codex-2", branch: `issue-913-${pr}`,
      }));
      records.push(taskRecord(++sequence, "task.pr_opened", "codex-2", "issue-913", {
        pr, headSha: `abcdef${String(pr).padStart(7, "0")}`, lane: "codex-2", journeys: [],
      }));
      records.push(taskRecord(++sequence, "task.merged", "coordinator", "pr-" + pr, {
        pr, mergeSha: `fedcba${String(pr).padStart(7, "0")}`, closes: [], merger: "coordinator",
      }));
    };
    for (const pr of [917, 924, 925, 939]) claimAndFinish(pr);
    records.push(taskRecord(++sequence, "task.review_assigned", "coordinator", "issue-913", {
      pr: 971, headSha: "abcdef1234567", reviewer: "codexrev-1",
    }));
    records.push(taskRecord(++sequence, "task.review_verdict", "codexrev-1", "issue-913", {
      pr: 971, headSha: "abcdef1234567", reviewer: "codexrev-1", verdict: "APPROVE",
      commentUrl: "https://github.com/stabem/GraphHelm/pull/971#review",
    }));

    const tasks = foldTaskEvents(records);
    expect(tasks.filter((task) => task.pr === 971)).toHaveLength(1);
    expect(tasks.find((task) => task.pr === 971)).toMatchObject({ step: "merge", headSha: "abcdef1234567", currentReviewer: "codexrev-1" });
    expect(tasks.find((task) => task.pr === 971)?.strayVerdicts).toEqual([]);
  });
});
