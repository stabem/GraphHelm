import { describe, expect, it } from "vitest";

import { timeScaleFactor } from "./time-scale";

/* #549: the Studio tests' waits stretch with GRAPHHELM_TEST_TIME_SCALE, the CLI tests' knob. These
 * cells pin what a value means: unset keeps every bound (a developer's own run is unchanged), a
 * whole number from 1 to 20 multiplies, and anything else refuses loudly instead of silently
 * meaning 1, which would bring back the false reds the knob exists to remove. Cost: pure. */
describe("timeScaleFactor (#549)", () => {
  it("keeps every bound when unset or blank", () => {
    expect(timeScaleFactor(undefined)).toBe(1);
    expect(timeScaleFactor("")).toBe(1);
    expect(timeScaleFactor("  ")).toBe(1);
  });

  it("multiplies by a whole number from 1 to 20", () => {
    expect(timeScaleFactor("1")).toBe(1);
    expect(timeScaleFactor(" 3 ")).toBe(3);
    expect(timeScaleFactor("20")).toBe(20);
  });

  it("refuses anything else instead of meaning 1", () => {
    for (const raw of ["0", "21", "1.5", "-2", "three", "3x"]) {
      expect(() => timeScaleFactor(raw), raw).toThrow(/GRAPHHELM_TEST_TIME_SCALE/);
    }
  });
});
