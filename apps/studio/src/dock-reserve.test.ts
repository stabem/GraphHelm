import { describe, expect, it } from "vitest";

import { dockReserve } from "./dock-reserve";

/** #1083 F9: the overview reserves what the dock actually covers, not a fixed guess. */
describe("the dock's reserve", () => {
  it("reserves the largest height-plus-offset among the docks", () => {
    // The action dock wrapped to three rows (112px, 12px off the bottom); the remedies dock
    // stands 174px up with 125.4px of rows.
    expect(dockReserve([{ height: 112, bottom: 12 }])).toBe(124);
    expect(dockReserve([{ height: 112, bottom: 12 }, { height: 125.4, bottom: 174 }])).toBe(300);
  });

  it("reserves nothing for a dock that is not laid out", () => {
    expect(dockReserve([])).toBe(0);
    expect(dockReserve([{ height: 0, bottom: 174 }])).toBe(0);
  });
});
