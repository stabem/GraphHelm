import { describe, expect, it } from "vitest";

import { cliRemedy, reasonText, runState } from "./attention";

describe("run attention (#2324, approved design)", () => {
  it("says waiting for you instead of running, and never invents a verdict an older Runtime did not send", () => {
    expect(runState("running", "needs_you")).toBe("waiting for you");
    expect(runState("paused", "needs_you")).toBe("paused");
    expect(runState("running", "can_sleep")).toBe("running");
    expect(runState("running", undefined)).toBe("running");
  });

  it("names the blocked node and the CLI line that approves it", () => {
    const blocked = { kind: "blocked_node", node: "implementation" };
    expect(reasonText(blocked)).toBe("implementation is blocked.");
    expect(cliRemedy(blocked, "demo")).toBe("graphhelm execution approve --events <events> --execution demo --node implementation");
    expect(cliRemedy({ kind: "failed_node", node: "x" }, "demo")).toBeNull();
    expect(reasonText({ kind: "some_new_reason", node: "n" })).toBe("some new reason: n.");
  });
});
