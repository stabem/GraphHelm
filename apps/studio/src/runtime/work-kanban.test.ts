import { describe, expect, it } from "vitest";
import { bucketKanban, kanbanColumn, KANBAN_COLUMNS } from "./work-kanban";
import type { StageHealth, HealthFlag } from "./stage-health";

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
