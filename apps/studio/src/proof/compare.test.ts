import { describe, expect, it } from "vitest";

import type { JourneyPastRun } from "../runtime/client";
import { changedSentence, diffPixels, earlierRun, MAX_DIFF_PIXELS, type Pixels } from "./compare";

function solid(width: number, height: number, rgb: [number, number, number]): Pixels {
  const data = new Uint8ClampedArray(width * height * 4);
  for (let i = 0; i < data.length; i += 4) { data[i] = rgb[0]; data[i + 1] = rgb[1]; data[i + 2] = rgb[2]; data[i + 3] = 255; }
  return { width, height, data };
}

describe("diffPixels", () => {
  it("counts the changed pixels and paints exactly those red", () => {
    const before = solid(10, 10, [200, 200, 200]);
    const now = solid(10, 10, [200, 200, 200]);
    for (const pixel of [0, 37, 99]) now.data.set([10, 20, 30], pixel * 4);
    const diff = diffPixels(before, now);
    if (diff.kind !== "diff") throw new Error(diff.reason);
    expect(diff.changed).toBe(3);
    expect(diff.percent).toBeCloseTo(3);
    expect([...diff.data.slice(37 * 4, 37 * 4 + 3)]).toEqual([255, 59, 59]);
    expect(diff.data[36 * 4]).not.toBe(255);
  });

  it("does not count a difference within the noise threshold", () => {
    const diff = diffPixels(solid(4, 4, [100, 100, 100]), solid(4, 4, [110, 110, 110]));
    expect(diff.kind === "diff" && diff.changed).toBe(0);
  });

  it("refuses frames of different sizes and names both", () => {
    const diff = diffPixels(solid(4, 3, [0, 0, 0]), solid(3, 4, [0, 0, 0]));
    expect(diff).toEqual({ kind: "refused", reason: expect.stringContaining("before 4×3, now 3×4") });
  });

  it("refuses frames over the bound before allocating", () => {
    const huge = { width: MAX_DIFF_PIXELS + 1, height: 1, data: new Uint8ClampedArray(0) };
    expect(diffPixels(huge, huge)).toEqual({ kind: "refused", reason: expect.stringContaining("too large") });
  });
});

describe("changedSentence", () => {
  it("never rounds a real change to 0% or a partial change to 100%", () => {
    expect(changedSentence(1, 0.01)).toBe("Less than 1% of the screen changed (in red).");
    expect(changedSentence(999, 99.9)).toBe("More than 99% of the screen changed (in red).");
    expect(changedSentence(4, 4.4)).toBe("4% of the screen changed (in red).");
    expect(changedSentence(0, 0)).toBe("Nothing changed on the screen.");
  });
});

describe("earlierRun", () => {
  const runs: JourneyPastRun[] = [
    { run: "00000000000000000003", ranAt: "c", screens: { a: { frame: true } } },
    { run: "00000000000000000002", ranAt: "b", screens: { a: { frame: false } } },
    { run: "00000000000000000001", ranAt: "a", screens: { a: { frame: true } } },
  ];
  it("skips the run on screen and runs without this step's frame", () => {
    expect(earlierRun(runs, { state: "ready", ranAt: "c" }, "a")?.run).toBe("00000000000000000001");
    expect(earlierRun(runs, { state: "ready", ranAt: "z" }, "a")?.run).toBe("00000000000000000003");
    expect(earlierRun(runs, { state: "ready", ranAt: "c" }, "missing")).toBeNull();
  });
});
