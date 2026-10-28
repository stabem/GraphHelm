import { describe, expect, it } from "vitest";

import type { JourneyFlow, JourneyRun } from "../runtime/client";
import { failureNotice } from "./failure";
import { journeySteps } from "./steps";

const flow: JourneyFlow = {
  id: "buy",
  title: null,
  name: "Buy a sofa",
  status: "approved",
  screens: [{ id: "list", url: "/items" }, { id: "item", url: "/items/1" }, { id: "done", url: "/done" }],
  edges: [
    { id: "list.item", from: "list", to: "item", acts: [{ kind: "activate", role: "link", name: "Sofa" }] },
    { id: "item.done", from: "item", to: "done", acts: [{ kind: "submit", role: "button", name: "Save" }] },
  ],
  paths: { main: ["list.item", "item.done"] },
};
const notice = (run: JourneyRun) => failureNotice(run, journeySteps(flow, run));

describe("failure notice (#1298)", () => {
  it("names the failing step, its reason, its code and position, and the launcher's last lines", () => {
    const run: JourneyRun = {
      state: "ready", kind: "replay", result: "fail",
      screens: { list: { frame: true, result: "pass" }, item: { frame: true, result: "pass" }, done: { frame: true, result: "fail", reason: "act_failed" } },
      edges: { "list.item": { result: "pass" }, "item.done": { result: "fail" } },
      launchLog: { tail: "ready on :3000\n\nsubmit Save\r\nrun ended: failed\n" },
    };
    const said = notice(run)!;
    expect(said.sentence).toMatch(/^Failed at step 3 \(.+\): an action of this step could not be done\.$/);
    expect(said.code).toBe("act_failed · step 3 of 3");
    expect(said.log).toEqual({ label: "Launcher", lines: ["ready on :3000", "submit Save", "run ended: failed"] });
    expect(said.details).toContain("Launcher, last 3 lines:");
  });

  it("says a launch that never served the app stopped before step 1", () => {
    const said = notice({ state: "ready", result: "fail", reason: "watch.launch_failed", launchLog: { tail: "npm ERR! missing script: dev" } })!;
    expect(said.sentence).toBe("Stopped before step 1: the app did not start.");
    expect(said.code).toBe("watch.launch_failed");
    expect(said.log?.lines).toEqual(["npm ERR! missing script: dev"]);
  });

  it("does not claim a run that already passed a step stopped before step 1", () => {
    const said = notice({ state: "failed", reason: "internal", screens: { list: { frame: true, result: "pass" } } })!;
    expect(said.sentence).toBe("Failed; the failing step is unknown.");
    expect(said.code).toBe("internal");
  });

  it("shows nothing for a pass, a drift, a running run, or one that waits", () => {
    expect(notice({ state: "ready", result: "pass" })).toBeNull();
    expect(notice({ state: "ready", result: "drift" })).toBeNull();
    expect(notice({ state: "running" })).toBeNull();
    expect(notice({ state: "ready", result: "fail", edges: { "list.item": { result: "skipped", reason: "review_needed" } } })).toBeNull();
  });
});
