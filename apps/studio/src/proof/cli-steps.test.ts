import { describe, expect, it } from "vitest";

import type { JourneyFlow, JourneyRun } from "../runtime/client";
import { countSteps, distinctOutputs, journeyKind, journeySteps, liveCard, outputLines } from "./steps";

// #1912: the shape of keel-card-accepted: a command-only flow walked on two paths, so each edge's
// command is kept twice, and its entry screen recorded `not_reached` as the commands' precondition.
const flow: JourneyFlow = {
  id: "keel-card-accepted",
  title: null,
  status: "draft",
  screens: [{ id: "accepted", url: "/" }, { id: "refused", url: "/" }, { id: "start", url: "/" }],
  edges: [
    { id: "start.accept-in-scope", from: "start", to: "accepted", acts: [{ kind: "cli", role: "exec", name: "python run.py inside" }] },
    { id: "start.refuse-out-of-scope", from: "accepted", to: "refused", acts: [{ kind: "cli", role: "exec", name: "python run.py outside" }] },
  ],
  paths: { main: ["start.accept-in-scope", "start.refuse-out-of-scope"], "refusal-only": ["start.accept-in-scope", "start.refuse-out-of-scope"] },
};

const inside = { argv: "python run.py inside", exit: 0, stdout: "{\"ok\":true}\n", stderr: "", stdoutSha256: "a", stderrSha256: "e" };
const outside = { argv: "python run.py outside", exit: 2, stdout: "keel.scope.path_outside_card docs/notes.md\n", stderr: "refused\n", stdoutSha256: "b", stderrSha256: "f" };

const passed: JourneyRun = {
  state: "ready",
  result: "pass",
  screens: { accepted: { frame: false, result: "pass" }, refused: { frame: false, result: "pass" }, start: { frame: false, reason: "not_reached" } },
  edges: { "start.accept-in-scope": { result: "pass" }, "start.refuse-out-of-scope": { result: "pass" } },
  cliConfirm: { argv: ["python run.py inside", "python run.py outside"], confirmed: true, digest: "sha256:aa" },
  outputs: { "start.accept-in-scope": [inside, inside], "start.refuse-out-of-scope": [outside, outside] },
};

describe("a passed command run (#1912)", () => {
  const steps = journeySteps(flow, passed);

  it("gives each command step its command, exit code and output, each distinct output once", () => {
    expect(steps.map((step) => [step.cli, step.outputs.map((output) => [output.argv, output.exit])])).toEqual([
      [false, []],
      [true, [["python run.py inside", 0]]],
      [true, [["python run.py outside", 2]]],
    ]);
  });

  it("reads the start screen as the commands' precondition, never not run", () => {
    expect(steps.map((step) => step.status)).toEqual(["precondition", "passed", "passed"]);
    expect(countSteps(steps)).toEqual({ passed: 2, waits: 0, failed: 0, notRun: 0 });
  });

  it("keeps not run for a start screen before any run", () => {
    expect(journeySteps(flow, null)[0]!.status).toBe("not run");
  });

  it("writes the console from what the commands printed", () => {
    expect(outputLines(passed)).toEqual([
      "$ python run.py inside", "{\"ok\":true}", "exit 0",
      "$ python run.py outside", "keel.scope.path_outside_card docs/notes.md", "refused", "exit 2",
    ]);
    expect(outputLines({ state: "ready" })).toEqual([]);
  });
});

describe("one output per command (#1914)", () => {
  it("shows a command once when its runs differ only by a digest or a clock", () => {
    const again = { ...inside, stdout: "{\"ok\":true,\"at\":2}\n", stdoutSha256: "z" };
    expect(distinctOutputs([inside, outside, again]).map((output) => [output.argv, output.stdoutSha256])).toEqual([["python run.py inside", "z"], ["python run.py outside", "b"]]);
    expect(outputLines({ state: "ready", outputs: { "start.accept-in-scope": [inside, again] } }).filter((line) => line.startsWith("$ "))).toEqual(["$ python run.py inside"]);
  });
});

describe("journey kind (#1914)", () => {
  it("reads Terminal when every act is a command, Browser when none is, Both when mixed", () => {
    expect(journeyKind(flow)).toBe("terminal");
    expect(journeyKind({ ...flow, edges: [{ id: "a", from: "start", to: "accepted", acts: [{ kind: "activate", role: "button", name: "Go" }] }] })).toBe("browser");
    expect(journeyKind({ ...flow, edges: [...flow.edges!, { id: "b", from: "start", to: "accepted", acts: [{ kind: "activate", role: "button", name: "Go" }] }] })).toBe("both");
  });
});

describe("a card follows the live run (#1914)", () => {
  it("reads running with its step count, then passed when the run ends", () => {
    const running: JourneyRun = { state: "running", current: "accepted", screens: { accepted: { frame: false, result: "pass" } }, edges: { "start.accept-in-scope": { result: "pass" } } };
    const mid = liveCard(flow, running)!;
    expect([mid.text, mid.passed, mid.total]).toEqual(["● running", 1, 3]);
    const done = liveCard(flow, { ...passed, kind: "replay" })!;
    expect([done.text, done.tone, done.total]).toEqual(["✓ passed", "pass", 3]);
    expect(liveCard(flow, { state: "none" })).toBeNull();
  });
});
