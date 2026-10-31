import { describe, expect, it } from "vitest";

import type { AttentionReason, Runtime } from "../runtime/client";
import { RunWaitsReader } from "./RunAttention";

// The read behind useRunWaits, driven as it runs: each status read is a promise the test settles,
// so every state between pending and settled is observed.
function runtimeWithReads() {
  const pending: { run: string; resolve: (status: unknown) => void; reject: (error: unknown) => void }[] = [];
  const runtime = {
    runAttention: (run: string) => new Promise((resolve, reject) => { pending.push({ run, resolve, reject }); }),
  } as unknown as Runtime;
  return { runtime, pending };
}

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));
const blocked: AttentionReason[] = [{ kind: "blocked_node", node: "implementation" }];

describe("RunWaitsReader (#2351)", () => {
  it("pending → failed: never 'nothing', before or after the failed read", async () => {
    const { runtime, pending } = runtimeWithReads();
    const reader = new RunWaitsReader(runtime);
    reader.read("demo");
    expect(reader.waitsFor("demo")).toBe("reading");
    pending[0].reject(new Error("503"));
    await settle();
    expect(reader.waitsFor("demo")).toBe("failed");
  });

  it("pending → loaded: 'nothing' (an empty list) only after a successful read with no reasons", async () => {
    const { runtime, pending } = runtimeWithReads();
    const reader = new RunWaitsReader(runtime);
    reader.read("demo");
    expect(reader.waitsFor("demo")).toBe("reading");
    pending[0].resolve({ attention: "can_sleep", attentionReasons: [] });
    await settle();
    expect(reader.waitsFor("demo")).toEqual([]);
    reader.read("demo");
    pending[1].resolve({ attention: "needs_you", attentionReasons: blocked });
    await settle();
    expect(reader.waitsFor("demo")).toEqual(blocked);
  });

  it("a run switch goes back to 'reading' and never shows the previous run's reasons", async () => {
    const { runtime, pending } = runtimeWithReads();
    const reader = new RunWaitsReader(runtime);
    reader.read("first");
    pending[0].resolve({ attention: "needs_you", attentionReasons: blocked });
    await settle();
    expect(reader.waitsFor("first")).toEqual(blocked);
    expect(reader.waitsFor("second")).toBe("reading");
    reader.read("second");
    expect(reader.waitsFor("second")).toBe("reading");
    expect(reader.waitsFor("first")).toBe("reading");
    pending[1].resolve({ attention: "can_sleep", attentionReasons: [] });
    await settle();
    expect(reader.waitsFor("second")).toEqual([]);
  });

  it("a read that lands after a newer one started is dropped", async () => {
    const { runtime, pending } = runtimeWithReads();
    const reader = new RunWaitsReader(runtime);
    reader.read("first");
    reader.read("second");
    pending[0].resolve({ attention: "needs_you", attentionReasons: blocked });
    await settle();
    expect(reader.waitsFor("second")).toBe("reading");
    expect(reader.waitsFor("first")).toBe("reading");
  });
});
