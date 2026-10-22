import { describe, expect, it } from "vitest";

import { runAllUntil } from "./run-all";

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
