import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import type { JourneyFlowView } from "../runtime/types";
import { JourneyFlowchart, MIN_FIT, fitScale } from "./journey-flowchart";

const screen = (id: string) => ({ id, title: id, scopePaths: [], expect: [] });
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
