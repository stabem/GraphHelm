import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { LanesPanels } from "../lanes/LanesPanels";
import type { Runtime, RunSummary, TasksBoard } from "../runtime/client";
import { RunWaitsGiven, type RunWaits } from "./RunAttention";
import { GraphView } from "./Views";

// #2348: Graph and Lanes rendered as they ship, with the run's attention reasons given through
// RunWaitsGiven instead of read. Removing their wiring (the Graph "Waits on" row, the Lanes line)
// turns these red; before, only the pure helpers in attention.test.ts were observed.
const runtime = {} as Runtime;
const blocked: RunWaits = [{ kind: "blocked_node", node: "implementation" }];
const run: RunSummary = { executionId: "demo", status: "running", attention: "needs_you", objective: "Build", startedAt: null, lastEventAt: "2026-10-31T17:00:00Z" };
const board: TasksBoard = { now: "2026-10-31T17:00:00Z", tasks: [], liveness: { lanes: [] }, stalls: [], waiting: [] };

const graph = (waits: RunWaits, shown: RunSummary = run) => renderToStaticMarkup(
  <RunWaitsGiven.Provider value={{ demo: waits }}><GraphView run={shown} runId="demo" runtime={runtime} /></RunWaitsGiven.Provider>);
const lanes = (waits: RunWaits) => renderToStaticMarkup(
  <RunWaitsGiven.Provider value={{ demo: waits }}>
    <LanesPanels runtime={runtime} executionId="demo" board={board} requests={[]} actions={{}} onAnswer={() => {}} onSent={async () => {}} />
  </RunWaitsGiven.Provider>);

describe("a run that waits for you, as Graph and Lanes show it (#2324, #2348)", () => {
  it("Graph says waiting for you and what it waits on", () => {
    const html = graph(blocked);
    expect(html).toContain("waiting for you");
    expect(html).toContain("<dt>Waits on</dt>");
    expect(html).toContain("implementation is blocked.");
    expect(html).toContain("graphhelm execution approve --events &lt;events&gt; --execution demo --node implementation");
  });

  it("Graph shows no Waits on row once the run waits on nothing", () => {
    const html = graph([], { ...run, status: "paused", attention: "can_sleep" });
    expect(html).toContain("paused");
    expect(html).not.toContain("Waits on");
  });

  it("Lanes counts the run's wait and says it, never Nothing waits on you", () => {
    const html = lanes(blocked);
    expect(html).toContain("The run waits for you:");
    expect(html).toContain("implementation is blocked.");
    expect(html).toMatch(/NEEDS ACTION<\/strong>.*?<span class="mono count late">1<\/span>/);
    expect(html).not.toContain("Nothing waits on you.");
  });

  it("Lanes says Nothing waits on you only when it knows", () => {
    expect(lanes([])).toContain("Nothing waits on you.");
    const reading = lanes("reading");
    expect(reading).not.toContain("Nothing waits on you.");
    expect(reading).toContain("Reading…");
    const failed = lanes("failed");
    expect(failed).not.toContain("Nothing waits on you.");
    expect(failed).toContain("The run&#x27;s status could not be read.");
  });
});
