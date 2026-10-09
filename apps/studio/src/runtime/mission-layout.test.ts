import { describe, expect, it } from "vitest";
import { layoutMission } from "./mission-layout";
import type { Mission, MissionStep, MissionTask, StepStatus } from "./mission";

const step = (i: number, status: StepStatus): MissionStep => ({ stepId: `s${i}`, index: i, title: `S${i}`, status, reason: null, promise: null });
const task = (key: string, pr: number | null, s: MissionTask["step"]): MissionTask => ({
  key, pr, issue: null, title: key, lane: "l", reviewers: [], step: s, blocked: false, trust: 1,
  blockedBy: null, rounds: [], headSha: null, mergeSha: null, repoUrl: null,
});
const mission = (steps: MissionStep[], tasks: MissionTask[]): Mission => ({
  contractId: "j", title: "J", steps, tasks, summary: { proven: 0, total: steps.length, inFlight: 0, needYou: 0, readyUnclaimed: 0 },
});

describe("layoutMission", () => {
  it("open work sits in the first unproven step, merged work one step past it, stacked by PR", () => {
    const m = mission([step(0, "proven"), step(1, "not_run"), step(2, "not_run")],
      [task("c", 30, "review"), task("a", 10, "merged"), task("b", 20, "implement"), task("x", null, "implement")]);
    const at = Object.fromEntries(layoutMission(m).placed.map((p) => [p.task.key, [p.col, p.row]]));
    expect(at).toEqual({ a: [2, 0], b: [1, 0], c: [1, 1], x: [1, 2] });
  });

  it("merged work is clamped to the last step; all proven puts the frontier on the last step", () => {
    const m = mission([step(0, "proven"), step(1, "proven")], [task("a", 1, "merged"), task("b", 2, "review")]);
    expect(layoutMission(m).placed.map((p) => p.col)).toEqual([1, 1]);
  });

  it("joins consecutive tasks in PR order; solid only from merged work", () => {
    const m = mission([step(0, "not_run"), step(1, "not_run")], [task("b", 2, "review"), task("a", 1, "merged"), task("c", 3, "implement")]);
    const l = layoutMission(m);
    expect(l.edges).toEqual([{ from: "a", to: "b", done: true }, { from: "b", to: "c", done: false }]);
    expect(l.rows).toBe(2);
  });

  it("no tasks: no edges, one empty row", () => {
    expect(layoutMission(mission([step(0, "not_run")], []))).toEqual({ placed: [], edges: [], rows: 1 });
  });
});
