import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import type { JourneyFlowView } from "../runtime/types";
import { JourneyFlowchart, MIN_FIT, fitScale } from "./journey-flowchart";

const screen = (id: string) => ({ id, title: id, url: "/", state: "stable", scopePaths: [], expect: [] });
const edge = (id: string, from: string, to: string) => ({ id, from, to, acts: [] });

/** Main row a → b → c → d; a second way leaves b for x below and rejoins the main row. */
function flow(side: string[]): JourneyFlowView {
  return {
    id: "rejoin",
    status: "draft",
    findings: [],
    title: "rejoin",
    screens: ["a", "b", "c", "d", "x", "y"].map(screen),
    edges: [edge("ab", "a", "b"), edge("bc", "b", "c"), edge("cd", "c", "d"), edge("bx", "b", "x"),
      edge("xd", "x", "d"), edge("xc", "x", "c"), edge("xy", "x", "y"), edge("yc", "y", "c")],
    paths: { main: ["ab", "bc", "cd"], side },
  } as unknown as JourneyFlowView;
}

function arrow(container: HTMLElement, id: string): string | null {
  return container.querySelector(`path[data-edge="${id}"]`)?.getAttribute("d") ?? null;
}

// #519 (the owner's sketch: the lower card feeds back into the main row). A branch that rejoins a
// card on the row above used to be drawn into that card's left side, at the height the main-row
// arrow already enters (the two merged), or as a loop under the cards when the card it rejoins is
// not to its right (it read as "go back" and ran behind the branch). Now it leaves the branch card
// from its top, crosses in the empty band between the rows, and enters the rejoined card from
// below. The grid is fixed (cards 232×252, gaps 56/40), so the paths are exact. Cost: jsdom render.
describe("JourneyFlowchart rejoin", () => {
  // #605: the second return used to cross the first branch's card and share its final segment.
  // Exact rendered SVG paths observe the fixed grid without adding a production seam.
  // Cost: three small jsdom renders, no network or browser process.
  it.each([
    ["ahead", "d", false, "M 808 412 H 830 V 266 H 974 V 252", "M 808 716 H 842 V 278 H 986 V 252"],
    ["above", "c", false, "M 808 412 H 830 V 278 H 686 V 252", "M 808 716 H 842 V 266 H 698 V 252"],
    ["behind", "c", true, "M 1096 412 H 1118 V 278 H 686 V 252", "M 1096 716 H 1130 V 266 H 698 V 252"],
  ] as const)("keeps two returns %s separate and outside the intervening card", (_shape, target, longer, first, second) => {
    const chart = flow([]);
    chart.screens = ["a", "b", "c", "d", "x", "z", ...(longer ? ["y", "w"] : [])].map(screen);
    chart.edges = [edge("ab", "a", "b"), edge("bc", "b", "c"), edge("cd", "c", "d"),
      edge("bx", "b", "x"), edge("bz", "b", "z"),
      ...(longer ? [edge("xy", "x", "y"), edge("zw", "z", "w")] : []),
      edge("first", longer ? "y" : "x", target), edge("second", longer ? "w" : "z", target)];
    chart.paths = { main: ["ab", "bc", "cd"],
      side: ["ab", "bx", ...(longer ? ["xy"] : []), "first"],
      third: ["ab", "bz", ...(longer ? ["zw"] : []), "second"] };
    const view = render(<JourneyFlowchart flow={chart} />);
    // Distinct paths can still cross: compare every pair of rendered axis-aligned segments.
    // This catches the leftward band ordering defect that exact-path snapshots missed.
    const segments = (id: string) => {
      const path = arrow(view.container, id)!;
      const commands = [...path.matchAll(/([MHV])\s+(-?\d+(?:\.\d+)?)(?:\s+(-?\d+(?:\.\d+)?))?/g)];
      expect(commands).toHaveLength(5);
      let x = 0, y = 0;
      return commands.flatMap(([, command, a, b]) => {
        const nextX = command === "V" ? x : Number(a);
        const nextY = command === "M" ? Number(b) : command === "V" ? Number(a) : y;
        const segment = { minX: Math.min(x, nextX), maxX: Math.max(x, nextX), minY: Math.min(y, nextY), maxY: Math.max(y, nextY) };
        x = nextX; y = nextY;
        return command === "M" ? [] : [segment];
      });
    };
    for (const a of segments("first")) {
      for (const b of segments("second")) {
        expect(a.minX <= b.maxX && b.minX <= a.maxX && a.minY <= b.maxY && b.minY <= a.maxY,
          `${_shape}: rejoin segments must not intersect`).toBe(false);
      }
    }
    expect(arrow(view.container, "first")).toBe(first);
    expect(arrow(view.container, "second")).toBe(second);
    // The rightmost gutter must fit in the chart rather than being clipped by its scroll box.
    if (longer) expect(view.container.querySelector(".journey-chart")).toHaveStyle({ width: "1152px" });
    view.unmount();
  });

  it("draws a branch that rejoins the main row up through the band between the rows", () => {
    // x sits under c (col 2, row 1); d is col 3 on the main row.
    const ahead = render(<JourneyFlowchart flow={flow(["ab", "bx", "xd"])} />);
    expect(arrow(ahead.container, "xd")).toBe("M 692 292 V 272 H 980 V 252");
    expect(arrow(ahead.container, "cd")).toBe("M 808 72.5 H 864");
    ahead.unmount();
    // The rejoined card straight above: a short climb, not a loop around the branch card.
    const above = render(<JourneyFlowchart flow={flow(["ab", "bx", "xc"])} />);
    expect(arrow(above.container, "xc")).toBe("M 692 292 V 272 H 692 V 252");
    above.unmount();
    // The rejoined card behind the branch's last card: still up and across, never under the cards.
    const behind = render(<JourneyFlowchart flow={flow(["ab", "bx", "xy", "yc"])} />);
    expect(arrow(behind.container, "yc")).toBe("M 980 292 V 272 H 692 V 252");
  });
});

describe("fitScale (#519)", () => {
  it("shrinks a chart wider than its column so every card shows", () => {
    expect(fitScale(560, 403)).toBeCloseTo(403 / 560);
  });
  it("never enlarges a chart that already fits", () => {
    expect(fitScale(400, 600)).toBe(1);
  });
  it("stops at MIN_FIT and lets a long journey scroll", () => {
    expect(fitScale(3000, 400)).toBe(MIN_FIT);
  });
  it("changes nothing before the column is measured", () => {
    expect(fitScale(560, 0)).toBe(1);
  });
});
