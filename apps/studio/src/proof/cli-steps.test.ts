import { describe, expect, it } from "vitest";

import type { JourneyFlow, JourneyRun } from "../runtime/client";
import { countSteps, journeySteps, outputLines } from "./steps";

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
