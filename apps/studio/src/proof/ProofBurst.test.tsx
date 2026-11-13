import { act } from "react";
import type { Root } from "react-dom/client";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";

import type { ProofView as PanelType } from "./ProofView";
import type { JourneyFlow, Runtime } from "../runtime/client";

// #2901: Run all, Approve N and the single Run share one synchronous single-flight, so a burst of calls in
// one tick (before React has re-rendered the buttons disabled) starts one action. Real ProofView; jsdom is
// loaded inside beforeAll, as the neighbouring component tests do (see JourneySearch.test.tsx).
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
  delete (navigator as unknown as { locks?: unknown }).locks;
  window.localStorage.clear();
});

const last = { status: "not_run", kind: null, ranAt: null, reason: null, failingStep: null, flowChanged: null };
const flow = (id: string) => ({ id, title: `Journey ${id}`, name: `Journey ${id}`, feature: "Studio", status: "draft", approvable: true, screens: [], edges: [], paths: {}, lastRun: last }) as unknown as JourneyFlow;
const flows = [flow("a"), flow("b")];
const run = { state: "ready", runId: "11111111-1111-4111-8111-111111111111", kind: "replay", result: "pass", screens: {}, edges: {} };
function runtimeStub(over: { approveJourney?: ReturnType<typeof vi.fn> } = {}) {
  const startJourneyRun = vi.fn(async (..._args: unknown[]) => ({ ...run }));
  const approveJourney = over.approveJourney ?? vi.fn(async (..._args: unknown[]) => ({}));
  const runtime = { journeyRun: async () => ({ state: "none" }), journeyHistory: async () => [], requests: async () => [], journeyFrame: async () => null, startJourneyRun, approveJourney } as unknown as Runtime;
  return { runtime, startJourneyRun, approveJourney };
}
// `lock` is what the browser hands the callback: an object when this tab got the lock, null when another tab holds it.
const withLocks = (lock: unknown = {}) => Object.defineProperty(navigator, "locks", { configurable: true, value: { request: async (_name: string, _options: unknown, callback: (lock: unknown) => Promise<void>) => callback(lock) } });
const settle = () => act(async () => { await new Promise((resolve) => setTimeout(resolve, 30)); });
async function mount(runtime: Runtime) {
  const host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => { root!.render(<ProofView projectKey="demo" runtime={runtime} journeys={flows} error={null} executionId="exec-1" onRan={() => {}} />); });
  await settle();
}
const button = (label: string) => [...document.querySelectorAll<HTMLButtonElement>("button")].find((candidate) => candidate.getAttribute("aria-label") === label || candidate.textContent === label)!;
const burst = (...labels: string[]) => act(async () => { for (const label of labels) button(label).click(); });
const started = (startJourneyRun: ReturnType<typeof vi.fn>) => startJourneyRun.mock.calls.map((call) => call[0]);

describe("one action at a time in the Journeys column", () => {
  it("runs one batch for two same-tick Run all calls, and sends one approval round for two Approve N calls", async () => {
    const { runtime, startJourneyRun, approveJourney } = runtimeStub();
    withLocks();
    await mount(runtime);
    await burst("▶ Run all", "▶ Run all");
    await settle(); await settle();
    expect(started(startJourneyRun)).toEqual(["a", "b"]);
    await burst("Approve 2", "Approve 2");
    await settle();
    expect(approveJourney.mock.calls).toEqual([["a"], ["b"]]);
  });

  it("starts only the first of a same-tick mix of Run all, Approve N and a single Run", async () => {
    const { runtime, startJourneyRun, approveJourney } = runtimeStub();
    withLocks();
    await mount(runtime);
    await burst("▶ Run all", "Run Journey b");
    await settle(); await settle();
    expect(started(startJourneyRun)).toEqual(["a", "b"]);
    startJourneyRun.mockClear();
    await burst("Run Journey b", "▶ Run all");
    await settle(); await settle();
    expect(started(startJourneyRun)).toEqual(["b"]);
    startJourneyRun.mockClear();
    await burst("Approve 2", "▶ Run all");
    await settle(); await settle();
    expect(approveJourney).toHaveBeenCalledTimes(2);
    expect(startJourneyRun).not.toHaveBeenCalled();
    await burst("▶ Run all", "Approve 2");
    await settle(); await settle();
    expect(started(startJourneyRun)).toEqual(["a", "b"]);
    expect(approveJourney).toHaveBeenCalledTimes(2);
  });

  it("lets a new action start after one fails, returns early, finds the lock held, or settles", async () => {
    const approveJourney = vi.fn().mockRejectedValueOnce(new Error("refused")).mockResolvedValue({});
    const { runtime, startJourneyRun } = runtimeStub({ approveJourney });
    await mount(runtime);
    await burst("▶ Run all"); // early return: this browser has no navigator.locks
    await settle();
    expect(startJourneyRun).not.toHaveBeenCalled();
    expect(document.body.textContent).toContain("cannot safely coordinate Run all");
    await burst("Approve 2"); // thrown error
    await settle();
    await burst("Approve 2");
    await settle();
    expect(approveJourney.mock.calls).toEqual([["a"], ["a"], ["b"]]);
    withLocks(null);
    await burst("▶ Run all"); // early return: another tab holds the lock
    await settle();
    expect(startJourneyRun).not.toHaveBeenCalled();
    withLocks();
    await burst("▶ Run all"); // success
    await settle(); await settle();
    await burst("Run Journey a");
    await settle();
    expect(started(startJourneyRun)).toEqual(["a", "b", "a"]);
  });
});
