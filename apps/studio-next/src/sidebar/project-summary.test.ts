import { describe, expect, it } from "vitest";

import { activeRun, agentCount, failingJourneys, subtitle } from "./project-summary";

describe("project summary", () => {
  it("names the newest running run, never a finished one", () => {
    const row = (executionId: string, status: string) => ({ executionId, status, startedAt: null, lastEventAt: null });
    expect(activeRun([row("old-done", "completed"), row("live", "running"), row("older", "running")])).toBe("live");
    expect(activeRun([row("done", "completed")])).toBeNull();
  });

  it("counts each lane the Runtime does not judge free once", () => {
    expect(agentCount([{ lane: "a", free: false }, { lane: "a", free: false }, { lane: "b", free: true }, { lane: "c", free: false }])).toBe(2);
  });

  it("fails a journey that is unreadable or carries an error finding, not a warning", () => {
    expect(failingJourneys([
      { id: "a", title: null, status: "approved", findings: [{ severity: "warning" }] },
      { id: "b", title: null, status: "draft", findings: [{ severity: "error" }] },
      { id: "c", title: null, status: "unreadable" },
    ])).toBe(2);
  });

  it("keeps an unread count apart from zero", () => {
    expect(subtitle({ activeRun: null, agents: null, journeys: null, failing: null })).toEqual({ run: "no run", journeys: null });
    expect(subtitle({ activeRun: "gh-team", agents: 8, journeys: 24, failing: 1 })).toEqual({ run: "● gh-team · 8 agents", journeys: "24 journeys · 1 failing" });
    expect(subtitle({ activeRun: "r", agents: null, journeys: 0, failing: 0 })).toEqual({ run: "● r", journeys: "0 journeys" });
  });
});
