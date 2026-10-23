import { splitFrames } from "./use-journey-run";
import { describe, expect, it } from "vitest";

import type { JourneyFlow } from "../runtime/client";
import { CHANGED, countSteps, earlierVersion, failsOnlyOn, journeyScore, journeySteps, lastRunChip, pastAsRun, preparingStatus } from "./steps";

const flow: JourneyFlow = {
  id: "buy",
  title: null,
  status: "approved",
  screens: [{ id: "list", url: "/items" }, { id: "item", url: "/items/1" }, { id: "done", url: "/done" }],
  edges: [
    { id: "item.done", from: "item", to: "done", acts: [{ kind: "submit", role: "button", name: "Save" }] },
    { id: "list.item", from: "list", to: "item", acts: [{ kind: "activate", role: "link", name: "Sofa" }] },
  ],
  paths: { other: ["item.done"], main: ["list.item", "item.done"] },
};

describe("journey steps", () => {
  it("walks the main path and joins each screen's and edge's verdict into its step", () => {
    const steps = journeySteps(flow, {
      state: "ready",
      screens: { list: { frame: true, width: 1280, result: "pass" }, item: { frame: true, width: 390, result: "pass" } },
      edges: { "list.item": { result: "pass" }, "item.done": { result: "fail" } },
    });
    // A run recorded before viewports measured one size: the width it saw names the viewport.
    expect(steps.map((step) => [step.screen, step.label, step.status, step.viewports])).toEqual([
      ["list", "Opens /items", "passed", { web: "passed", mobile: "not run" }],
      ["item", "Clicks “Sofa”", "passed", { web: "not run", mobile: "passed" }],
      ["done", "Submits “Save”", "failed", { web: "failed", mobile: "not run" }],
    ]);
  });

  it("marks the screen being played as running and leaves unreached steps not run", () => {
    const steps = journeySteps(flow, { state: "running", current: "item", screens: { list: { frame: true, result: "pass" } } });
    expect(steps.map((step) => step.status)).toEqual(["passed", "running", "not run"]);
  });
});

// #1777: a Run proves web and mobile; each step shows both, and a mobile-only break is named.
describe("steps at web and mobile", () => {
  it("reads each viewport's own result and names a step that fails only on mobile", () => {
    const steps = journeySteps(flow, {
      state: "ready",
      viewportsPlanned: ["web", "mobile"],
      screens: { list: { frame: true, result: "pass" }, item: { frame: true, result: "fail" } },
      edges: { "list.item": { result: "pass" } },
      viewports: {
        web: { result: "pass", screens: { list: { frame: true, result: "pass" }, item: { frame: true, result: "pass" } }, edges: { "list.item": { result: "pass" } } },
        mobile: { result: "fail", screens: { list: { frame: true, result: "pass" }, item: { frame: true, result: "fail" } }, edges: { "list.item": { result: "pass" } } },
      },
    });
    expect(steps.map((step) => step.viewports)).toEqual([
      { web: "passed", mobile: "passed" },
      { web: "passed", mobile: "failed" },
      { web: "not run", mobile: "not run" },
    ]);
    expect(steps.map(failsOnlyOn)).toEqual([null, "mobile", null]);
  });

  it("names no viewport when both failed, or when the other never ran", () => {
    expect(failsOnlyOn({ viewports: { web: "failed", mobile: "failed" } })).toBeNull();
    expect(failsOnlyOn({ viewports: { web: "not run", mobile: "failed" } })).toBeNull();
    expect(failsOnlyOn({ viewports: { web: "drift", mobile: "passed" } })).toBe("web");
  });
});

describe("journeys column chip", () => {
  it("counts only a replay as proof and reads an absent lastRun as unknown, not not run", () => {
    const last = { ranAt: "t", reason: null, failingStep: null, flowChanged: false };
    expect(lastRunChip(undefined).text).toBe("? unknown");
    expect(lastRunChip({ ...last, status: "not_run", kind: null }).text).toBe("○ not run");
    expect(lastRunChip({ ...last, status: "failed", kind: "replay" })).toEqual({ text: "✕ failed", tone: "fail" });
    expect(lastRunChip({ ...last, status: "passed", kind: "preview" })).toEqual({ text: "✓ passed · preview, not proof", tone: "idle" });
  });
});

// #1956: a flow edited after its run (its digest moved) reads changed, never passed, and the panel
// shows that run's results from the history entry; a flow that never ran stays "not run".
describe("a journey changed since its last run", () => {
  const last = { ranAt: "t", reason: null, failingStep: null, kind: "replay" as const };
  const past = { run: "00000001792058400000", state: "ready" as const, result: "pass" as const, ranAt: "2026-10-22T21:53:09Z", digest: "sha256:old",
    screens: { list: { frame: true, result: "pass" as const }, item: { frame: true, result: "pass" as const }, done: { frame: false, result: "pass" as const } },
    edges: { "list.item": { result: "pass" as const }, "item.done": { result: "pass" as const } } };

  it("reads Changed since last run on the card when the digest moved, never passed", () => {
    expect(lastRunChip({ ...last, status: "passed", flowChanged: true })).toEqual({ text: CHANGED, tone: "idle" });
    expect(lastRunChip({ ...last, status: "passed", flowChanged: false }).text).toBe("✓ passed");
    expect(lastRunChip({ ...last, status: "not_run", kind: null, flowChanged: null }).text).toBe("○ not run");
  });

  it("shows the earlier version's results when the current version has no run, and none when it never ran", () => {
    expect(earlierVersion({ state: "none" }, [past])).toBe(past);
    expect(earlierVersion({ state: "none" }, [])).toBeNull();
    expect(earlierVersion({ state: "ready", result: "pass" }, [past])).toBeNull();
    expect(earlierVersion(null, [past])).toBeNull();
    const steps = journeySteps(flow, pastAsRun(past));
    expect(steps.map((step) => step.status)).toEqual(["passed", "passed", "passed"]);
    expect(steps.map((step) => step.frame)).toEqual([true, true, false]);
  });
});

describe("preparing status", () => {
  it("names the setup phase in words and keeps a percentage only when one was printed", () => {
    expect(preparingStatus("npx playwright install chromium 62%")).toBe("Preparing the browser · Downloading Chromium 62%");
    expect(preparingStatus("npm install --save-dev @playwright/test@1.56.0")).toBe("Preparing the browser · Installing test tools");
    expect(preparingStatus("something else 140%")).toBe("Preparing the browser");
  });
});

// #1878: a mobile-only run never shows its frames as web; a web-first run keeps both apart.
describe("splitFrames", () => {
  it("keeps a mobile-only run off the web view", () => {
    expect(splitFrames({ cart: "blob:m" }, "mobile")).toEqual({ web: {}, mobile: { cart: "blob:m" } });
  });
  it("keeps web at the top level and mobile under its slot", () => {
    expect(splitFrames({ cart: "blob:w", "mobile:cart": "blob:m" }, "web")).toEqual({ web: { cart: "blob:w" }, mobile: { cart: "blob:m" } });
    expect(splitFrames({ cart: "blob:w" }, undefined)).toEqual({ web: { cart: "blob:w" }, mobile: {} });
  });
});

// #1905: the run bar's counts and a journey card's score come from Runtime data only.
describe("step counts and journey score", () => {
  it("counts a step skipped for the owner's confirmation as waiting, not as not run", () => {
    const steps = journeySteps(flow, {
      state: "ready",
      screens: { list: { frame: true, result: "pass" }, item: { frame: false, reason: "not_reached" } },
      edges: { "list.item": { result: "skipped", reason: "confirm_needed" }, "item.done": { result: null, reason: "not_reached" } },
    });
    expect(countSteps(steps)).toEqual({ passed: 1, waits: 2, failed: 0, notRun: 0 });
    expect(steps.map((step) => step.status)).toEqual(["passed", "waiting", "waiting"]);
    expect(steps[1]!.reason).toBe("confirm_needed");
  });

  it("scores a failure by the step lastRun names, and an unnamed failing step as unknown", () => {
    const last = { ranAt: null, kind: "replay" as const, reason: null, flowChanged: false };
    const failed = journeyScore({ ...flow, lastRun: { ...last, status: "failed", failingStep: { kind: "edge", id: "item.done", result: "fail", reason: null } } });
    expect(failed).toEqual({ passed: 2, total: 3, bar: ["pass", "pass", "fail"] });
    expect(journeyScore({ ...flow, lastRun: { ...last, status: "failed", failingStep: null } }).passed).toBeNull();
    expect(journeyScore({ ...flow, lastRun: { ...last, status: "passed", failingStep: null } }).passed).toBe(3);
    expect(journeyScore({ ...flow, lastRun: undefined }).passed).toBeNull();
  });
});
