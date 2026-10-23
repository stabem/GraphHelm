import { describe, expect, it } from "vitest";

import { followRun, runAllUntil } from "./run-all";

describe("Run all waits on a journey", () => {
  it("stops with timeout when the run never leaves running", async () => {
    let t = 0;
    const clock = { now: () => t, sleep: async (ms: number) => { t += ms; } };
    const outcome = await runAllUntil(async () => ({ state: "running" }), { timeoutMs: 5_000, pollMs: 1_000, clock });
    expect(outcome).toBe("timeout");
    expect(t).toBe(5_000);
  });

  it("returns done as soon as the run is no longer running", async () => {
    let reads = 0;
    const clock = { now: () => 0, sleep: async () => undefined };
    const outcome = await runAllUntil(async () => ({ state: ++reads < 3 ? "running" : "ready" }), { clock });
    expect([outcome, reads]).toEqual(["done", 3]);
  });
});

describe("A confirmed run is followed to its result (#1912)", () => {
  const clock = { now: () => 0, sleep: async () => undefined };

  it("keeps reading after a failed read and reaches the recorded pass", async () => {
    const answers: (() => { state: string; result?: string })[] = [
      () => ({ state: "running" }),
      () => { throw new Error("The Runtime is not answering."); },
      () => ({ state: "ready", result: "pass" }),
    ];
    const seen: string[] = [];
    const errors: number[] = [];
    const last = await followRun<{ state: string; result?: string }>({ state: "running" }, async () => answers.shift()!(), {
      onRun: (run) => seen.push(run.state), onError: (_, failures) => errors.push(failures), clock,
    });
    expect(last).toEqual({ state: "ready", result: "pass" });
    expect(seen).toEqual(["running", "running", "ready"]);
    expect(errors).toEqual([1]);
  });

  it("stops and says so after too many failed reads in a row", async () => {
    const finals: boolean[] = [];
    const last = await followRun({ state: "running" }, async () => { throw new Error("down"); }, {
      onRun: () => undefined, onError: (_, __, final) => finals.push(final), maxFailures: 3, clock,
    });
    expect(last).toBeNull();
    expect(finals).toEqual([false, false, true]);
  });

  it("never reads again once the panel has moved on", async () => {
    let reads = 0;
    const last = await followRun({ state: "running" }, async () => { reads += 1; return { state: "running" }; }, {
      onRun: () => undefined, onError: () => undefined, live: () => false, clock,
    });
    expect([last, reads]).toEqual([null, 0]);
  });
});
