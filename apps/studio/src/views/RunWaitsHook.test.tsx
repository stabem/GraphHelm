// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it } from "vitest";

import { LanesPanels } from "../lanes/LanesPanels";
import type { AttentionReason, Runtime, TasksBoard } from "../runtime/client";

// #2356: Lanes rendered with the real useRunWaits (no RunWaitsGiven), so its effect and its
// subscription run. Without the reader.read(...) call the panel stays on "Reading…" forever.
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

type Status = { attention?: string; attentionReasons?: AttentionReason[] };
const board: TasksBoard = { now: "2026-10-31T17:00:00Z", tasks: [], liveness: { lanes: [] }, stalls: [], waiting: [] };

function runtimeAnswering() {
  let answer: (status: Status) => void = () => {};
  const reads: string[] = [];
  const runtime = {
    runAttention: (executionId: string) => { reads.push(executionId); return new Promise<Status>((resolve) => { answer = resolve; }); },
    collisions: () => new Promise(() => {}),
    capacity: () => new Promise(() => {}),
  } as unknown as Runtime;
  return { runtime, reads, answer: (status: Status) => answer(status) };
}

let root: Root | null = null;
let host: HTMLElement | null = null;
afterEach(() => { act(() => root?.unmount()); host?.remove(); root = null; host = null; });

async function mount(runtime: Runtime) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => {
    root!.render(<LanesPanels runtime={runtime} executionId="demo" board={board} requests={[]} actions={{}} onAnswer={() => {}} onSent={async () => {}} />);
  });
  return () => host!.querySelector('[aria-label="Needs action"]')!.textContent ?? "";
}

describe("Lanes reads what the run waits on through useRunWaits (#2356)", () => {
  it("moves from Reading… to what the run's status says", async () => {
    const { runtime, reads, answer } = runtimeAnswering();
    const needs = await mount(runtime);
    expect(reads).toEqual(["demo"]);
    expect(needs()).toContain("Reading…");
    await act(async () => answer({ attention: "needs_you", attentionReasons: [{ kind: "blocked_node", node: "implementation" }] }));
    expect(needs()).toContain("The run waits for you:");
    expect(needs()).toContain("implementation is blocked.");
    expect(needs()).not.toContain("Reading…");

    // A run that waits on nothing: Nothing waits on you, once the status says so.
    act(() => root?.unmount());
    host?.remove();
    const quiet = runtimeAnswering();
    const nothing = await mount(quiet.runtime);
    await act(async () => quiet.answer({ attention: "can_sleep" }));
    expect(nothing()).toContain("Nothing waits on you.");
    expect(nothing()).not.toContain("Reading…");
  });
});
