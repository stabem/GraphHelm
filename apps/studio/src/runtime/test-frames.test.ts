import { describe, expect, it } from "vitest";
import { testFrames } from "./test-frames";
import type { JourneyView } from "./types";

const journey: JourneyView = {
  contractId: "j", title: "J", arrows: [],
  steps: [
    { stepId: "a", screen: { screenId: "a", title: "Journey tab", scopePaths: [] }, promises: [] },
    { stepId: "b", screen: { screenId: "b", title: "Step 4 row", scopePaths: [] }, promises: [], action: { kind: "click", role: "button", name: "Mark safe" } as never },
    { stepId: "c", screen: { screenId: "c", title: "Ladder", scopePaths: [] }, promises: [], expectedStates: ["ladder 5/5"] },
  ],
};

describe("testFrames", () => {
  it("verbs and statuses", () => {
    const f = testFrames(journey, { state: "ready", kind: "replay", screens: { a: { frame: true, result: "pass" } }, edges: { "a->b": { result: "skipped", reason: "data-changing" } } });
    expect(f.map((x) => [x.n, x.verb, x.status])).toEqual([[1, "SEES", "passed"], [2, "DOES", "waits_for_you"], [3, "EXPECT", "not_run"]]);
    expect(f[1].reason).toBe("data-changing");
    expect(f[2].expected).toEqual(["ladder 5/5"]);
  });

  it("matches the dotted edge id the explorer writes", () => {
    const f = testFrames(journey, { state: "ready", kind: "replay", edges: { "a.b": { result: "skipped" } } });
    expect(f[1].status).toBe("waits_for_you");
  });

  it("a dotted id from another pair does not match the wrong step", () => {
    const f = testFrames(journey, { state: "ready", kind: "replay", edges: { "x.y.b": { result: "skipped" }, "cart.checkout/0": { result: "skipped" } } });
    expect(f.every((x) => x.status === "not_run")).toBe(true);
  });

  it("no run: all not_run", () => {
    expect(testFrames(journey, null).every((x) => x.status === "not_run")).toBe(true);
  });
});
