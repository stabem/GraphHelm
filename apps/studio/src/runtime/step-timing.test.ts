import { describe, expect, it } from "vitest";

import { clockStep, duration, emptyClock, pace, typicalStep } from "./step-timing";
import { foldTaskEvents, parseTaskEvent, type TaskEventRecord } from "./team-tasks";

/* #502 (owner: a timer and a bar on the lit step). These cells observe the clock the fold keeps per
 * task slice, from the Runtime's append time of each record: when the slice entered its step, what
 * it spent in each step it left, the run's typical time per step (a median, only with 3 or more
 * samples) and the pace colour. They catch a clock that restarts on records that do not change the
 * step, one that counts a step it never left, and a typical time invented from too few samples.
 * Cost: pure functions, milliseconds. */

const T0 = Date.parse("2026-10-08T10:00:00Z");
const at = (minutes: number) => new Date(T0 + minutes * 60_000).toISOString();

function record(sequence: number, minutes: number, kind: string, actorId: string, fields: Record<string, unknown>): TaskEventRecord {
  const parsed = parseTaskEvent(kind, actorId, JSON.stringify({ schema: "graphhelm-task-event-v1", taskId: "issue-9", revision: sequence, at: "x", ...fields }));
  if (parsed === null) throw new Error(`fixture ${kind} did not parse`);
  return { ...parsed, sequence, occurredAt: at(minutes) };
}

const head = "a".repeat(40);
const url = "https://github.com/o/r/pull/19#c1";

describe("step clock in the fold (#502)", () => {
  it("starts Implement at the claim, moves to Review at pr_opened and to Merge at the APPROVE", () => {
    const records = [
      record(1, 0, "task.claimed", "l1", { issue: 9, lane: "l1", branch: "b" }),
      record(2, 30, "task.pr_opened", "l1", { pr: 19, headSha: head, journeys: [], lane: "l1" }),
      record(3, 35, "task.review_assigned", "l1", { pr: 19, headSha: head, reviewer: "l2", ordinal: 1 }),
      record(4, 50, "task.review_verdict", "l2", { pr: 19, headSha: head, reviewer: "l2", verdict: "APPROVE", commentUrl: url }),
      record(5, 55, "task.merged", "l2", { pr: 19, mergeSha: "c".repeat(40), closes: [9], merger: "l2" }),
    ];
    const at = (n: number) => foldTaskEvents(records.slice(0, n))[0].clock;
    expect(at(1)).toEqual({ since: "2026-10-08T10:00:00.000Z", spent: {} });
    expect(at(3)).toEqual({ since: "2026-10-08T10:30:00.000Z", spent: { implement: 30 * 60_000 } });
    expect(at(4)).toEqual({ since: "2026-10-08T10:50:00.000Z", spent: { implement: 30 * 60_000, review: 20 * 60_000 } });
    expect(at(5)).toEqual({ since: "2026-10-08T10:55:00.000Z", spent: { implement: 30 * 60_000, review: 20 * 60_000, merge: 5 * 60_000 } });
  });

  it("keeps a BLOCK and a new head inside Review instead of restarting the clock", () => {
    const tasks = foldTaskEvents([
      record(1, 0, "task.pr_opened", "l1", { pr: 19, headSha: head, journeys: [], lane: "l1" }),
      record(2, 10, "task.review_verdict", "l2", { pr: 19, headSha: head, reviewer: "l2", verdict: "BLOCK", commentUrl: url }),
      record(3, 20, "task.pr_opened", "l1", { pr: 19, headSha: "b".repeat(40), journeys: [], lane: "l1" }),
    ]);
    expect(tasks[0].clock).toEqual({ since: "2026-10-08T10:00:00.000Z", spent: {} });
  });

  it("has no start when the records carry no append time", () => {
    const parsed = parseTaskEvent("task.claimed", "l1", JSON.stringify({ schema: "graphhelm-task-event-v1", taskId: "issue-9", revision: 1, at: "x", issue: 9, lane: "l1", branch: "b" }));
    expect(foldTaskEvents([{ ...parsed!, sequence: 1 }])[0].clock).toEqual(emptyClock());
  });
});

describe("typical step time and pace (#502)", () => {
  const spent = (minutes: number[]) => minutes.map((m) => ({ since: null, spent: { review: m * 60_000 } }));

  it("is the median of the slices that left the step, and nothing below three samples", () => {
    expect(typicalStep(spent([10, 20]), "review")).toBeNull();
    expect(typicalStep(spent([10, 40, 20]), "review")).toEqual({ ms: 20 * 60_000, samples: 3 });
    expect(typicalStep(spent([10, 40, 20, 30]), "review")).toEqual({ ms: 25 * 60_000, samples: 4 });
    expect(typicalStep(spent([10, 40, 20]), "merge")).toBeNull();
  });

  it("is green under the typical time, amber past it and red past twice it", () => {
    expect(pace(10, 20)).toBe("under");
    expect(pace(20, 20)).toBe("under");
    expect(pace(21, 20)).toBe("over");
    expect(pace(41, 20)).toBe("stuck");
  });

  it("writes a duration short enough for a node", () => {
    expect([duration(45_000), duration(12 * 60_000), duration(185 * 60_000), duration(52 * 3_600_000)]).toEqual(["45 s", "12 min", "3 h 05", "2 d 4 h"]);
  });

  it("ignores a clock that goes backwards", () => {
    const clock = { since: at(10), spent: {} };
    clockStep(clock, "implement", "review", at(5));
    expect(clock).toEqual({ since: at(5), spent: {} });
  });
});
