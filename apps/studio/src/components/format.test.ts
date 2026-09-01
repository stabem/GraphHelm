import { describe, expect, it } from "vitest";

import { clock } from "./format";

/**
 * A time-of-day alone cannot say WHICH day. "needs you · 06:16 PM" read the same whether the
 * run moved nine hours ago or thirty-three (round-4, 3am on-call) — triage across a day
 * boundary was guesswork. Today's instants stay short; anything older carries its day.
 */
describe("clock", () => {
  it("renders a today instant as time only", () => {
    const today = new Date();
    today.setHours(9, 5, 0, 0);
    const rendered = clock(today.toISOString());
    expect(rendered).toMatch(/09:05|9:05/);
    expect(rendered).not.toMatch(/[A-Za-z]{3,}/u);
  });

  it("carries the day on an instant from another day", () => {
    const past = new Date();
    past.setDate(past.getDate() - 3);
    past.setHours(12, 1, 0, 0);
    const rendered = clock(past.toISOString());
    expect(rendered).toMatch(/12:01/);
    // Some day marker beyond the time: a month word or a numeric date.
    expect(rendered.replace(/12:01|AM|PM/g, "")).toMatch(/\p{L}{3}|\d{1,2}[/.]/u);
  });

  it("keeps the honest dash for nothing", () => {
    expect(clock(null)).toBe("—");
    expect(clock(undefined)).toBe("—");
  });
});
