import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import type { RunSummary } from "../runtime/client";
import { activeRunWaits, subtitle } from "./project-summary";
import { Subtitles } from "./Sidebar";
import { summaryOf } from "./use-summaries";
import type { Session } from "../login/session";
import { noRequestsText } from "../graph/requests-text";

const runOf = (executionId: string, status: string, attention?: string): RunSummary => ({ executionId, status, attention } as RunSummary);
const summary = { activeRun: "demo", agents: 0, journeys: 1, toFix: 1 };

// #2341: a bare fixture run that waits on its owner shows it in the project rail and in the Graph's
// empty text, with the phrase Graph's Status already uses.
describe("a run that waits for you, in the rail and in the empty Graph", () => {
  it("the rail reads the run's attention, only while the run is running", () => {
    expect(activeRunWaits([runOf("demo", "running", "needs_you")], "demo")).toBe(true);
    expect(activeRunWaits([runOf("demo", "running", "can_sleep")], "demo")).toBe(false);
    expect(activeRunWaits([runOf("demo", "running")], "demo")).toBe(false);
    expect(activeRunWaits([runOf("demo", "completed", "needs_you")], "demo")).toBe(false);
    expect(activeRunWaits([runOf("other", "running", "needs_you")], "demo")).toBe(false);
    expect(activeRunWaits([runOf("demo", "running", "needs_you")], null)).toBe(false);
  });

  it("the rail line adds 'waiting for you' only for a waiting active run", () => {
    expect(subtitle({ ...summary, waiting: true })).toEqual({ run: "● demo · 0 agents", waits: "waiting for you", journeys: "1 journey · 1 to fix" });
    expect(subtitle({ ...summary, waiting: false }).waits).toBeNull();
    expect(subtitle(summary).waits).toBeNull();
    expect(subtitle({ ...summary, activeRun: null, waiting: true }).waits).toBeNull();
    const open = { key: "k", name: "synthetic", folder: null };
    expect(renderToStaticMarkup(<Subtitles project={{ ...open, summary: { ...summary, waiting: true } }} open />))
      .toContain("● demo · 0 agents · <span class=\"run-waits\">waiting for you</span>");
    expect(renderToStaticMarkup(<Subtitles project={{ ...open, summary: { ...summary, waiting: true } }} open={false} />))
      .toContain("● demo · 0 agents · waiting for you");
    expect(renderToStaticMarkup(<Subtitles project={{ ...open, summary }} open />)).not.toContain("waiting for you");
  });

  it("the empty Graph says the run waits, only for one waiting run", () => {
    expect(noRequestsText(false, 1, 0, true)).toBe("No owner request is recorded on this run yet. The run waits for you; see Status above.");
    expect(noRequestsText(false, 1, 0, false)).toBe("No owner request is recorded on this run yet.");
    expect(noRequestsText(true, 3, 0, true)).toBe("No owner request is recorded in the 3 runs of this project yet.");
    expect(noRequestsText(false, 1, 2, true)).toBe("No request has work in this journey.");
  });

  it("the summary read carries the active run's wait to the rail", async () => {
    const session = { token: "t", base: "http://x", teamRun: null } as unknown as Session;
    const read = (runs: RunSummary[]) => summaryOf(session, { runs: async () => runs, journeys: async () => [], lanes: async () => [] });
    expect((await read([runOf("demo", "running", "needs_you")])).waiting).toBe(true);
    expect((await read([runOf("demo", "running", "can_sleep")])).waiting).toBe(false);
    expect((await read([])).waiting).toBe(false);
  });
});
