import { describe, expect, it } from "vitest";

import type { JourneyFlow } from "../runtime/client";
import { failsOnlyOn, journeySteps, lastRunChip, preparingStatus } from "./steps";

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

describe("preparing status", () => {
  it("names the setup phase in words and keeps a percentage only when one was printed", () => {
    expect(preparingStatus("npx playwright install chromium 62%")).toBe("Preparing the browser · Downloading Chromium 62%");
    expect(preparingStatus("npm install --save-dev @playwright/test@1.56.0")).toBe("Preparing the browser · Installing test tools");
    expect(preparingStatus("something else 140%")).toBe("Preparing the browser");
  });
});
