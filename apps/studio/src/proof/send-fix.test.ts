import { describe, expect, it } from "vitest";

import type { JourneyFlow, JourneyRun } from "../runtime/client";
import { ASK_MAX, failingStep, fixAsk, fixBody, fixRequestId, offersFix } from "./send-fix";
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
const failed: JourneyRun = {
  state: "ready", kind: "replay", result: "fail", ranAt: "2026-10-23T10:15:00Z",
  screens: { list: { frame: true, width: 1280, result: "pass" }, item: { frame: true, width: 1280, result: "pass" }, done: { frame: true, width: 1280, result: "fail", reason: "the Saved heading is absent" } },
  edges: { "list.item": { result: "pass" }, "item.done": { result: "fail" } },
};

describe("send to fix", () => {
  it("is offered only for a finished run that failed or drifted", () => {
    expect(offersFix(failed)).toBe(true);
    expect(offersFix({ ...failed, result: "drift" })).toBe(true);
    expect(offersFix({ ...failed, result: "pass" })).toBe(false);
    expect(offersFix({ state: "running" })).toBe(false);
    expect(offersFix({ state: "failed", reason: "app down" })).toBe(false);
    expect(offersFix(null)).toBe(false);
  });

  it("names one request per run, so the same run is never sent twice", () => {
    expect(fixRequestId("buy", failed)).toBe("fix-buy-20261023t101500z");
    expect(fixRequestId("buy", failed)).toBe(fixRequestId("buy", { ...failed }));
    expect(fixRequestId("buy", { ...failed, ranAt: "2026-10-23T11:00:00Z" })).not.toBe(fixRequestId("buy", failed));
  });

  it("carries the journey, its flow file, the run time, the failing step with its reason, and the note", () => {
    const steps = journeySteps(flow, failed);
    expect(failingStep(steps)?.screen).toBe("done");
    const ask = fixAsk(flow, failed, steps, "  the button moved  ");
    expect(ask).toContain("Fix the journey buy (Buy a sofa): its last run failed.");
    expect(ask).toContain("Flow file: .graphhelm/journeys/buy.journey.yaml");
    expect(ask).toContain("Run: replay at 2026-10-23T10:15:00Z");
    expect(ask).toMatch(/Failing step: 3 .*screen done, edge item\.done\) at web/);
    expect(ask).toContain("Reason: the Saved heading is absent");
    expect(ask).toContain("Frame: GET /v1/journey-flows/buy/preview, screen done");
    expect(ask).toContain("Owner's note: the button moved");
    expect(fixAsk(flow, failed, steps, "x".repeat(5000)).length).toBeLessThanOrEqual(ASK_MAX);
  });

  it("records task.request_recorded as the owner, who names itself", () => {
    const body = fixBody("demo", "fix-buy-1", "Fix Buy a sofa", "ask", "2026-10-23T10:16:00Z", "studio-operator") as { signal: Record<string, unknown> };
    expect(body.signal).toMatchObject({ id: "fix-buy-1", type: "task.request_recorded", source: { type: "user", id: "studio-operator" } });
    expect(JSON.parse(body.signal.description as string)).toEqual({
      schema: "graphhelm-task-event-v1", taskId: "request-fix-buy-1", revision: 1, at: "2026-10-23T10:16:00Z",
      requestId: "fix-buy-1", ask: "ask", title: "Fix Buy a sofa", recordedBy: "studio-operator",
    });
  });
});
