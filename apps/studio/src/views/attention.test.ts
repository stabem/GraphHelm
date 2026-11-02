import { describe, expect, it } from "vitest";

import { cliRemedy, reasonText, remedyLead, runState } from "./attention";

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

  // #2408: a drive that failed after the Runtime accepted the run says where, why, and the answer.
  it("names where a failed drive broke, why, and the CLI line that resumes the run", () => {
    const failed = { kind: "drive_failed", site: "workspace_release", cause: "the worktree is locked", at_sequence: 41 };
    expect(reasonText(failed)).toBe("The run's drive failed while releasing the workspace: the worktree is locked.");
    expect(reasonText({ kind: "drive_failed", site: "drive", cause: "no executor." })).toBe("The run's drive failed while driving the run: no executor.");
    expect(reasonText({ kind: "drive_failed" })).toBe("The run's drive failed.");
    expect(reasonText({ kind: "drive_failed", site: "somewhere_new", cause: "x" })).toBe("The run's drive failed: x.");
    expect(cliRemedy(failed, "demo")).toBe("graphhelm execution resume --events <events> --execution demo");
    expect(remedyLead(failed)).toBe("Resume it from the CLI:");
    expect(remedyLead({ kind: "blocked_node", node: "n" })).toBe("Approve it from the CLI:");
  });
});
