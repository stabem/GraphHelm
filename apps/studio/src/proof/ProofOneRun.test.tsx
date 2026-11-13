import { act } from "react";
import type { Root } from "react-dom/client";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";

import type { ProofView as PanelType } from "./ProofView";
import type { JourneyFlow, Runtime } from "../runtime/client";

// #2939: a journey's two Run buttons (its card and the run panel), through the real ProofView. jsdom
// is loaded inside beforeAll, as the neighbouring component tests do.
const JSDOM_LOAD_MS = 300_000;
let ProofView: typeof PanelType;
let createRoot: typeof import("react-dom/client").createRoot;
beforeAll(async () => {
  const specifier = "jsdom";
  const { JSDOM } = (await import(/* @vite-ignore */ specifier)) as { JSDOM: new (html: string, options: { url: string }) => { window: Window & typeof globalThis } };
  const { window } = new JSDOM("<!doctype html><html><body></body></html>", { url: "http://127.0.0.1/" });
  const g = globalThis as Record<string, unknown>;
  for (const key of Object.getOwnPropertyNames(window)) {
    if (key in g || key.startsWith("_")) continue;
    Object.defineProperty(g, key, { configurable: true, get: () => (window as unknown as Record<string, unknown>)[key] });
  }
  for (const [key, value] of [["window", window], ["document", window.document], ["navigator", window.navigator]] as const) {
    Object.defineProperty(g, key, { configurable: true, value });
  }
  ({ createRoot } = await import("react-dom/client"));
  ({ ProofView } = await import("./ProofView"));
  URL.createObjectURL = () => "blob:none";
  URL.revokeObjectURL = () => undefined;
}, JSDOM_LOAD_MS);
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | undefined;
afterEach(async () => {
  if (root) await act(async () => root!.unmount());
  root = undefined;
  document.body.innerHTML = "";
});

const last = { status: "not_run", kind: null, ranAt: null, reason: null, failingStep: null, flowChanged: null };
const flow = (id: string, name: string) => ({ id, title: name, name, feature: "Studio", status: "approved", screens: [], edges: [], paths: {}, lastRun: last }) as unknown as JourneyFlow;
const flows = [flow("first", "See the lanes"), flow("second", "Run a journey")];

/** A Runtime whose start request stays open until `answer` is called. */
function heldStart() {
  let answer: (run: unknown) => void = () => undefined;
  const startJourneyRun = vi.fn(() => new Promise((resolve) => { answer = resolve; }));
  const runtime = { journeyRun: async () => ({ state: "none" }), journeyHistory: async () => [], requests: async () => [], journeyFrame: async () => null, startJourneyRun } as unknown as Runtime;
  return { runtime, startJourneyRun, answer: () => answer({ state: "running" }) };
}
const settle = () => act(async () => { await new Promise((resolve) => setTimeout(resolve, 30)); });
async function mount(runtime: Runtime) {
  const host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => { root!.render(<ProofView projectKey="demo" runtime={runtime} journeys={flows} error={null} executionId="exec-1" onRan={() => {}} />); });
  await settle();
}
const cardRun = (name: string) => document.querySelector<HTMLButtonElement>(`button[aria-label="Run ${name}"]`)!;
const panelRun = () => document.querySelector<HTMLButtonElement>(".run-bar button.act")!;

describe("one Run at a time for a journey, card and panel (#2939)", () => {
  it("holds the panel's Run in its busy state while the card's start request is in flight", async () => {
    const { runtime, startJourneyRun } = heldStart();
    await mount(runtime);
    expect(panelRun().disabled).toBe(false);

    await act(async () => { cardRun("See the lanes").click(); });
    await settle();

    expect(panelRun().disabled).toBe(true);
    expect(panelRun().textContent).toBe("Running…");
    await act(async () => { panelRun().click(); });
    expect(startJourneyRun).toHaveBeenCalledTimes(1);
  });

  it("disables that journey's card Run while the panel's start request is in flight, and no other card's", async () => {
    const { runtime, startJourneyRun } = heldStart();
    await mount(runtime);
    expect(cardRun("See the lanes").disabled).toBe(false);

    await act(async () => { panelRun().click(); });
    await settle();

    expect(cardRun("See the lanes").disabled).toBe(true);
    expect(cardRun("Run a journey").disabled).toBe(false);
    await act(async () => { cardRun("See the lanes").click(); });
    expect(startJourneyRun).toHaveBeenCalledTimes(1);
  });

  it("keeps that card's Run disabled when another journey is picked while the panel's start request is in flight", async () => {
    const { runtime, startJourneyRun, answer } = heldStart();
    await mount(runtime);
    await act(async () => { panelRun().click(); });
    await settle();
    await act(async () => { document.querySelector<HTMLButtonElement>('button[aria-label="Select Run a journey"]')!.click(); });
    await settle();

    expect(cardRun("See the lanes").disabled).toBe(true);
    await act(async () => { cardRun("See the lanes").click(); });
    expect(startJourneyRun).toHaveBeenCalledTimes(1);

    await act(async () => { answer(); });
    await settle();
    expect(cardRun("See the lanes").disabled).toBe(false);
  });

  it("gives both Run buttons back when the start request is refused", async () => {
    let refuse: (cause: unknown) => void = () => undefined;
    const startJourneyRun = vi.fn(() => new Promise((_resolve, reject) => { refuse = reject; }));
    const runtime = { journeyRun: async () => ({ state: "none" }), journeyHistory: async () => [], requests: async () => [], journeyFrame: async () => null, startJourneyRun } as unknown as Runtime;
    await mount(runtime);
    await act(async () => { panelRun().click(); });
    await settle();
    expect(cardRun("See the lanes").disabled).toBe(true);

    await act(async () => { refuse(new Error("refused")); });
    await settle();
    expect(cardRun("See the lanes").disabled).toBe(false);
    expect(panelRun().disabled).toBe(false);
  });

  it("gives the card's Run back once the panel's start request is answered and the run has ended", async () => {
    const { runtime, answer } = heldStart();
    (runtime as unknown as { journeyRun: () => Promise<unknown> }).journeyRun = async () => ({ state: "ready", result: "pass", screens: {}, edges: {} });
    await mount(runtime);
    await act(async () => { panelRun().click(); });
    await settle();
    expect(cardRun("See the lanes").disabled).toBe(true);

    await act(async () => { answer(); });
    await settle();
    expect(cardRun("See the lanes").disabled).toBe(false);
  });
});
