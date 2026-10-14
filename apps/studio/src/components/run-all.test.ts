import { describe, expect, it } from "vitest";
import { journeyVerdict, runJourneys, type JourneyRunSource } from "./journey-flows";
import type { JourneyRunView } from "../runtime/types";

const source = (script: Record<string, JourneyRunView[]>, calls: string[]): JourneyRunSource => ({
  start: async (id, force) => { calls.push(`start ${id} ${force}`); if (!script[id]) throw new Error(`no ${id}`); return script[id]!.shift()!; },
  read: async (id) => { calls.push(`read ${id}`); return script[id]!.shift()!; },
  screenFrame: async () => null, liveFrame: async () => null,
});

describe("Run all", () => {
  it("replays every journey once, in order, polling each until it ends", async () => {
    const calls: string[] = [];
    const seen: string[] = [];
    const out = await runJourneys(["a", "b"], source({ a: [{ state: "running" }, { state: "ready" }], b: [{ state: "ready" }] }, calls), (id, r) => seen.push(`${id}:${r.state}`), undefined, async () => {});
    expect(out).toEqual({ busy: false, errors: {} });
    expect(calls).toEqual(["start a true", "read a", "start b true"]);
    expect(seen).toEqual(["a:running", "a:ready", "b:ready"]);
  });
  it("goes on past a journey that fails to start", async () => {
    const out = await runJourneys(["x", "b"], source({ b: [{ state: "ready" }] }, []), () => {}, undefined, async () => {});
    expect(Object.keys(out.errors)).toEqual(["x"]);
  });
  it("folds step statuses into one verdict; a changed flow reads stale", () => {
    expect(journeyVerdict(["proven", "proven"], { state: "ready" })).toBe("passed");
    expect(journeyVerdict(["proven", "stale"], { state: "ready" })).toBe("stale");
    expect(journeyVerdict(["proven", "failed"], { state: "ready" })).toBe("failed");
    expect(journeyVerdict(["not_run"], null)).toBe("not_run");
    expect(journeyVerdict([], { state: "running" })).toBe("running");
  });
});
