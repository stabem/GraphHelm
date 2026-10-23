import { describe, expect, it } from "vitest";

import { restartDetail, restartOf } from "./use-health";

describe("restartOf", () => {
  it("reads the restart from the Runtime's /health data", () => {
    const restart = restartOf({ currentRun: { startedAt: 1792709011 }, previousExit: { state: "vanished", pid: 1 } });
    expect(restart).toEqual({ startedAt: 1792709011, state: "vanished", recoveredBytes: null, supervisorRestarts: null });
    expect(restartDetail(restart!)).toContain("without recording why");
  });

  it("says the supervisor started it again on its own (#1962)", () => {
    const restart = restartOf({ currentRun: { startedAt: 1 }, previousExit: { state: "out_of_memory", supervisorRestart: { count: 2 } } });
    expect(restart?.supervisorRestarts).toBe(2);
    expect(restartDetail(restart!)).toBe("The previous run stopped without recording why. Its agents were released; nothing was lost. It was started again on its own.");
  });

  it("is null for a first start", () => {
    expect(restartOf({ currentRun: { startedAt: 1 }, previousExit: null })).toBeNull();
  });

  it("names a recovered torn tail", () => {
    const restart = restartOf({ currentRun: { startedAt: 1 }, previousExit: { state: "clean", journalTailRecovered: { trimmedBytes: 14 } } });
    expect(restartDetail(restart!)).toBe("The previous run stopped cleanly. 14 bytes of a torn last write were set aside.");
  });
});
