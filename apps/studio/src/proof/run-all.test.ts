import { describe, expect, it } from "vitest";

import { followRun, isRunAllRunId, makeRunAllBatch, readRunAllBatch, reconcileRunAllJourney, RunAllIdentityFailure, runAllStorageKey, runAllUntil, runAllViewIsCurrent, updateRunAllJourney, writeRunAllBatch, type RunAllBatch, type RunAllStorage } from "./run-all";

const runId = "2a19a79e-1905-4bc8-8caa-91c82744ae75";

describe("Run all waits on a journey", () => {
  it("cancels a continuation when the Proof view changes projects and remounts", () => {
    const runStartedIn = 4;
    const newProjectView = 6;
    expect(runAllViewIsCurrent(runStartedIn, newProjectView)).toBe(false);
    expect(runAllViewIsCurrent(newProjectView, newProjectView)).toBe(true);
  });

  it("stops with timeout when the run never leaves running", async () => {
    let t = 0;
    const clock = { now: () => t, sleep: async (ms: number) => { t += ms; } };
    const outcome = await runAllUntil(runId, async () => ({ state: "running", runId }), { timeoutMs: 5_000, pollMs: 1_000, clock });
    expect(outcome.state).toBe("timeout");
    expect(t).toBe(5_000);
  });

  it("returns done as soon as the run is no longer running", async () => {
    let reads = 0;
    const clock = { now: () => 0, sleep: async () => undefined };
    const outcome = await runAllUntil(runId, async () => ({ state: ++reads < 3 ? "running" : "ready", runId }), { clock });
    expect([outcome.state, reads]).toEqual(["done", 3]);
    if (outcome.state === "done") expect(outcome.run.state).toBe("ready");
  });

  it("retries only the read and reaches the matching Runtime run", async () => {
    const answers: (() => { state: string; runId?: string })[] = [
      () => { throw new Error("temporary disconnect"); },
      () => ({ state: "running", runId }),
      () => ({ state: "failed", runId }),
    ];
    const timeouts: number[] = [];
    const clock = { now: () => 0, sleep: async () => undefined };
    const outcome = await runAllUntil(runId, async (timeoutMs) => {
      timeouts.push(timeoutMs);
      return answers.shift()!();
    }, { timeoutMs: 9_000, clock });
    expect(outcome.state).toBe("done");
    expect(timeouts).toEqual([9_000, 9_000, 9_000]);
  });

  it("caps a slow GET at the remaining batch deadline", async () => {
    let now = 0;
    const clock = { now: () => now, sleep: async (ms: number) => { now += ms; } };
    const passed: number[] = [];
    const outcome = await runAllUntil(runId, async (timeoutMs) => {
      passed.push(timeoutMs);
      now += timeoutMs;
      throw new Error("GET timed out");
    }, { timeoutMs: 4_000, pollMs: 1_000, retryMs: 1_000, clock });
    expect(outcome.state).toBe("timeout");
    expect(passed).toEqual([4_000]);
    expect(now).toBe(4_000);
  });

  it("stops after bounded repeated read failures with a safe reason", async () => {
    const clock = { now: () => 0, sleep: async () => undefined };
    await expect(runAllUntil(runId, async () => { throw new Error("private diagnostic"); }, {
      maxFailures: 3, clock,
    })).rejects.toMatchObject({ reason: "read_failed", failures: 3 });
  });

  it("never treats a missing, unknown, or mismatched run as settled", async () => {
    const clock = { now: () => 0, sleep: async () => undefined };
    for (const run of [{ state: "ready" }, { state: "none", runId }, { state: "ready", runId: "other" }]) {
      await expect(runAllUntil(runId, async () => run, { clock })).rejects.toBeInstanceOf(RunAllIdentityFailure);
    }
  });

  it("does not continue after cancellation", async () => {
    let reads = 0;
    const outcome = await runAllUntil(runId, async () => { reads += 1; return { state: "running", runId }; }, {
      live: () => false,
      clock: { now: () => 0, sleep: async () => undefined },
    });
    expect([outcome.state, reads]).toEqual(["cancelled", 0]);
  });
});

describe("Run all saved interruption records", () => {
  const batch: RunAllBatch = {
    version: 1,
    projectKey: "project-a",
    batchId: "3a19a79e-1905-4bc8-8caa-91c82744ae75",
    updatedAt: 100,
    reason: "reloaded",
    journeys: [
      { flowId: "first", phase: "running", runId, updatedAt: 100 },
      { flowId: "later", phase: "not_started", reason: "reloaded", updatedAt: 100 },
    ],
  };

  function memoryStorage(): RunAllStorage & { values: Map<string, string> } {
    const values = new Map<string, string>();
    return {
      values,
      getItem: (key) => values.get(key) ?? null,
      setItem: (key, value) => { values.set(key, value); },
      removeItem: (key) => { values.delete(key); },
    };
  }

  it("keeps interruption state isolated by project and validates run UUIDs", () => {
    const storage = memoryStorage();
    writeRunAllBatch(storage, batch);
    expect(readRunAllBatch(storage, "project-a")).toEqual(batch);
    expect(readRunAllBatch(storage, "project-b")).toBeNull();
    expect(runAllStorageKey("project-a")).not.toBe(runAllStorageKey("project-b"));
    expect(isRunAllRunId(runId)).toBe(true);
    expect(isRunAllRunId("not-an-id")).toBe(false);
  });

  it("preserves the previous complete record when an atomic storage write fails", () => {
    const storage = memoryStorage();
    writeRunAllBatch(storage, batch);
    const key = runAllStorageKey("project-a");
    const before = storage.getItem(key);
    const failing: RunAllStorage = {
      ...storage,
      setItem: () => { throw new Error("quota"); },
    };
    expect(() => writeRunAllBatch(failing, { ...batch, batchId: "new-batch" })).toThrow("quota");
    expect(storage.getItem(key)).toBe(before);
  });

  it("rejects corrupt or incomplete saved records instead of calling them interrupted", () => {
    const storage = memoryStorage();
    storage.setItem(runAllStorageKey("project-a"), "{");
    expect(() => readRunAllBatch(storage, "project-a")).toThrow();
    storage.setItem(runAllStorageKey("project-a"), JSON.stringify({ ...batch, journeys: [{ flowId: "first", phase: "running", updatedAt: 1 }] }));
    expect(() => readRunAllBatch(storage, "project-a")).toThrow("incomplete");
  });

  it("reconciles only the exact saved run and keeps the remainder not started", () => {
    const batch = updateRunAllJourney(makeRunAllBatch("project-a", ["first", "later"], "3b19a79e-1905-4bc8-8caa-91c82744ae75", 1), 0, "running", { runId, updatedAt: 2 });
    const terminal = reconcileRunAllJourney(batch, 0, { state: "failed", runId }, { updatedAt: 3 });
    expect(terminal.journeys[0]).toMatchObject({ phase: "terminal", runId, reason: "reloaded" });
    expect(terminal.journeys[1]).toMatchObject({ phase: "not_started", reason: "reloaded" });

    const mismatched = reconcileRunAllJourney(batch, 0, { state: "ready", runId: "2b19a79e-1905-4bc8-8caa-91c82744ae75" }, { updatedAt: 4 });
    expect(mismatched.journeys[0]).toMatchObject({ phase: "unknown", reason: "run_identity_unknown" });
    expect(mismatched.journeys[1]).toMatchObject({ phase: "not_started", reason: "run_identity_unknown" });
  });

  it("keeps the original wait deadline across reload and marks an expired read unknown", () => {
    const batch = makeRunAllBatch("project-a", ["first", "later"], "3d19a79e-1905-4bc8-8caa-91c82744ae75", 1);
    const running = updateRunAllJourney(batch, 0, "running", { runId, deadlineAt: 10_000, updatedAt: 2 });
    const storage = memoryStorage();
    writeRunAllBatch(storage, running);
    const restored = readRunAllBatch(storage, "project-a");
    expect(restored?.journeys[0].deadlineAt).toBe(10_000);

    const expired = reconcileRunAllJourney(restored!, 0, null, { reason: "read_deadline", updatedAt: 11_000 });
    expect(expired.journeys[0]).toMatchObject({ phase: "unknown", runId, deadlineAt: 10_000, reason: "read_deadline" });
    expect(expired.journeys[1]).toMatchObject({ phase: "not_started", reason: "read_deadline" });
  });

  it("keeps a lost start reply unknown even when a later Runtime run exists", () => {
    const batch = makeRunAllBatch("project-a", ["first", "later"], "3c19a79e-1905-4bc8-8caa-91c82744ae75", 1);
    const pending = updateRunAllJourney(batch, 0, "start_pending", { updatedAt: 2 });
    const afterReload = reconcileRunAllJourney(pending, 0, { state: "ready", runId }, { updatedAt: 3 });
    expect(afterReload.journeys[0]).toMatchObject({ phase: "unknown", reason: "run_identity_unknown" });
    expect(afterReload.journeys[1]).toMatchObject({ phase: "not_started", reason: "run_identity_unknown" });
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
