import { describe, expect, it } from "vitest";

import { clock, runLabel } from "./format";

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

/**
 * #1077: a run the Studio started is `run-<uuid>`, and two of them could not be told apart. The
 * objective the operator typed is the name; the id stays the address. A hand-named run keeps
 * its id as the name, and the draft graph's placeholder "New task" is never shown as one.
 */
describe("runLabel", () => {
  const generated = "run-dc7b06e3-6459-440d-9e2e-31efc3b25b18";
  it("names a generated run by its objective, truncated", () => {
    expect(runLabel(generated, { objective: "Investigate slow login on mobile", name: "New task" })).toBe("Investigate slow login on mobile");
    const long = "x".repeat(200);
    const label = runLabel(generated, { objective: long, name: null });
    expect(label.length).toBeLessThan(80);
    expect(label.endsWith("…")).toBe(true);
  });
  it("falls back to the id, never to the placeholder name", () => {
    expect(runLabel(generated, { objective: null, name: "New task" })).toBe(generated);
    expect(runLabel(generated, null)).toBe(generated);
    expect(runLabel(generated, { objective: "   ", name: null })).toBe(generated);
  });
  /** #1083 F7: a run started from the CLI or over HTTP holds an objective too, and was listed by
   * its id while the store held the sentence. Any run with an objective is named by it. */
  it("names a hand-named run by its objective too, and by its id when it has none", () => {
    expect(runLabel("exec_feature", { objective: "Locate related components", name: "Feature" })).toBe("Locate related components");
    expect(runLabel("demo-deploy", { objective: null, name: "Deploy" })).toBe("demo-deploy");
  });
});
