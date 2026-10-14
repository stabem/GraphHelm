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
  it("names the step a play failed on", () => {
    const f = runFailure({ state: "failed", reason: "internal", detail: { code: "driver.unsupported_act", pointer: "/result" } }, ["home", "result"]);
    expect(f.step).toBe(1);
    expect(f.text).toMatch(/^Step 2 failed: /);
  });
});
