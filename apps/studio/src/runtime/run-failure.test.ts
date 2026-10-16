import { describe, expect, it } from "vitest";
import { runFailure } from "./run-failure";

describe("runFailure (#1296)", () => {
  it("says a launch failure stopped before step 1 and shows the launcher's last lines", () => {
    const f = runFailure({ state: "failed", reason: "watch.launch_failed", detail: { code: "watch.launch_failed", pointer: "/launcher" },
      launchLog: { path: "C:\\tmp\\graphhelm-launch-1.log", tail: "starting\nError: Cannot find module 'vite'\n" } }, ["home", "result"]);
    expect(f.step).toBeNull();
    expect(f.text).toBe("It stopped before step 1: the app under test did not start.");
    expect(f.log).toContain("Cannot find module 'vite'");
    expect(f.logPath).toBe("C:\\tmp\\graphhelm-launch-1.log");
  });
  it("shows the browser driver's last lines when it died before answering (#1448)", () => {
    const f = runFailure({ state: "failed", reason: "internal", detail: { code: "replay.driver_frame_invalid", pointer: "/entry/eof" },
      driverLog: { tail: "node:internal\nError [ERR_MODULE_NOT_FOUND]: Cannot find module 'judge.mjs'\n" } }, ["gate", "run"]);
    expect(f.text).toBe("It stopped before step 1: the browser driver stopped before it answered.");
    expect(f.log).toContain("Cannot find module 'judge.mjs'");
    expect(f.logFrom).toBe("driver");
    expect(f.logPath).toBeNull();
  });
  it("names the step a play failed on", () => {
    const f = runFailure({ state: "failed", reason: "internal", detail: { code: "driver.unsupported_act", pointer: "/result" } }, ["home", "result"]);
    expect(f.step).toBe(1);
    expect(f.text).toMatch(/^Step 2 failed: /);
  });
  it("does not claim a run stopped before step 1 after an edge failed", () => {
    const f = runFailure({ state: "failed", reason: "internal",
      detail: { code: "driver.unsupported_act", pointer: "/edges/next" },
      screens: { a: { frame: true, result: "pass" } },
      edges: { next: { result: "fail" } } }, ["a", "b"]);
    expect(f.step).toBeNull();
    expect(f.text).toContain("failing step is unknown");
    expect(f.text).toContain("the journey uses a step the browser cannot do");
    expect(f.text).not.toContain("before step 1");
  });
});
