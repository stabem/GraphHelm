import { describe, expect, it } from "vitest";
import { bucketKanban, kanbanColumn, KANBAN_COLUMNS } from "./work-kanban";
import { buildWorkGroups } from "./work-groups";
import type { StageHealth, HealthFlag } from "./stage-health";
import type { TaskState } from "./team-tasks";

const h = (flag: HealthFlag): StageHealth => ({ flag, text: flag, tone: "green", elapsedMs: null, elapsed: null });

describe("kanbanColumn", () => {
  it("maps each stage to its column", () => {
    expect(kanbanColumn("plan", h("moving"))).toBe("implement");
    expect(kanbanColumn("implement", h("moving"))).toBe("implement");
    expect(kanbanColumn("review", h("moving"))).toBe("review");
    expect(kanbanColumn("fix", h("blocked"))).toBe("blocked");
    expect(kanbanColumn("merge", null)).toBe("merge");
  });
  it("hides merged and proven work", () => {
    expect(kanbanColumn("merged", null)).toBeNull();
    expect(kanbanColumn("proven", null)).toBeNull();
  });
  it("waiting for build and building go to Waiting for build", () => {
    expect(kanbanColumn("implement", h("building"))).toBe("build");
    expect(kanbanColumn("fix", h("waiting_build"))).toBe("build");
  });
  it("silent wins over the stage: a stalled review shows under Silent", () => {
    expect(kanbanColumn("review", h("stalled"))).toBe("silent");
    expect(kanbanColumn("fix", h("stalled"))).toBe("silent");
    expect(kanbanColumn("merge", h("stalled"))).toBe("silent");
  });
  it("bucketKanban keeps every column, empty ones too, and counts", () => {
    const b = bucketKanban(["a", "b", "c"], (x) => (x === "c" ? null : x === "a" ? "review" : "silent"));
    expect(Object.keys(b)).toEqual(KANBAN_COLUMNS.map((c) => c.id));
    expect(b.review).toEqual(["a"]);
    expect(b.silent).toEqual(["b"]);
    expect(b.implement).toEqual([]);
  });
});

describe("released claims", () => {
  it("does not put a released claim in any kanban column", () => {
    const task: TaskState = {
      key: "issue-790:4618", taskId: "issue-790", branch: "issue-790-released-claim", issue: 790, pr: null,
      lane: "L", headSha: null, journeys: [], step: "implement", blockedBy: null, reviewers: [], mergeSha: null,
      repoUrl: null, strayVerdicts: [], title: "Released claim", summary: null, prTitle: null, prSummary: null,
      critic: null, recordedHeads: [], parent: null, rounds: [], clock: {} as TaskState["clock"], lastSequence: 4979,
      claimSequence: 4618, releasedBy: "L",
    };
    const [group] = buildWorkGroups([task], []);
    const buckets = bucketKanban(group!.rows.filter((row) => row.open), (row) => kanbanColumn(group!.stages[row.key]!, null));
    expect(Object.values(buckets).flat()).toEqual([]);
    expect(group!.tasks).toHaveLength(1);
    expect(group!.stages[task.key]).toBe("implement");
    expect(group!.open).toBe(false);
    expect(group!.releasedKeys).toEqual([task.key]);
    expect(group!.rows[0]!.open).toBe(false);
  });
});
