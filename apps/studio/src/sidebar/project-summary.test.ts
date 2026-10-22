import { describe, expect, it } from "vitest";

import { activeRun, agentCount, failingJourneys, subtitle, teamRun } from "./project-summary";

describe("project summary", () => {
  const row = (executionId: string, status: string, lastEventAt: string | null = null) => ({ executionId, status, startedAt: null, lastEventAt });

  it("names the most recently active running run, never a finished one", () => {
    expect(activeRun([row("old-done", "completed", "2026-10-22T12:00:00Z"), row("core", "running", "2026-10-20T00:00:00Z"), row("gh-team", "running", "2026-10-22T11:00:00Z")])).toBe("gh-team");
    expect(activeRun([row("live", "running"), row("older", "running")])).toBe("live");
    expect(activeRun([row("done", "completed")])).toBeNull();
  });

  it("opens the run team.json marks, else the most recently active one (#1901)", () => {
    const runs = [row("core", "running", "2026-10-22T11:00:00Z"), row("gh-team", "running", "2026-10-21T00:00:00Z"), row("done", "completed", "2026-10-22T12:00:00Z")];
    expect(teamRun(runs, "gh-team")?.executionId).toBe("gh-team");
    expect(teamRun(runs, "gone")?.executionId).toBe("core");
    expect(teamRun([row("a", "completed", "2026-10-01T00:00:00Z"), row("b", "failed", "2026-10-02T00:00:00Z")])?.executionId).toBe("b");
    expect(teamRun([])).toBeNull();
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
