import { act } from "react";
import type { Root } from "react-dom/client";
import { afterEach, beforeAll, describe, expect, it } from "vitest";

import type { JourneyRunPanel as PanelType } from "./ProofView";
import type { JourneyFlow, JourneyPastRun, JourneyRun, Runtime } from "../runtime/client";

// #2442: the real run panel with a stub Runtime that keeps two recorded runs with different results.
// Clicking the older run's chip shows ITS statuses and frames, not the latest run's.
//
// jsdom is loaded inside beforeAll, not as the file's vitest environment: vitest gives a worker 60 s
// to start and loading jsdom alone can take longer on a busy machine, so the file never ran. Here the
// load is bounded by the hook's own timeout.
const JSDOM_LOAD_MS = 300_000;
let JourneyRunPanel: typeof PanelType;
let createRoot: typeof import("react-dom/client").createRoot;
beforeAll(async () => {
  // jsdom ships no types and the Studio has no @types/jsdom; only the one constructor is used here.
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
  ({ JourneyRunPanel } = await import("./ProofView"));
  // jsdom has no object URLs: the stub names a URL after the frame it was made from.
  URL.createObjectURL = (blob: Blob) => `blob:${(blob as Blob & { tag: string }).tag}`;
  URL.revokeObjectURL = () => undefined;
}, JSDOM_LOAD_MS);
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const flow = {
  id: "buy", title: "Buy a sofa", status: "approved",
  screens: [{ id: "list", url: "/items" }, { id: "item", url: "/items/1" }],
  edges: [{ id: "list.item", from: "list", to: "item", acts: [{ kind: "activate", role: "link", name: "Sofa" }] }],
  paths: { main: ["list.item"] },
} as unknown as JourneyFlow;

const LATEST = "00000000000000002000";
const OLDER = "00000000000000001000";
const latest: JourneyRun = { state: "ready", kind: "preview", result: "pass", ranAt: "2026-11-02T12:00:00Z", screens: { list: { frame: true, result: "pass" }, item: { frame: true, result: "pass" } }, edges: { "list.item": { result: "pass" } } };
const history: JourneyPastRun[] = [
  { run: LATEST, state: "ready", kind: "preview", result: "pass", ranAt: "2026-11-02T12:00:00Z", screens: latest.screens, edges: latest.edges },
  { run: OLDER, state: "failed", kind: "preview", result: "fail", ranAt: "2026-11-01T09:00:00Z", screens: { list: { frame: true, result: "pass" }, item: { frame: false, result: "fail" } }, edges: { "list.item": { result: "fail" } } },
];
const blob = (tag: string) => Object.assign(new Blob([tag]), { tag });
const runtime = {
  journeyRun: async () => latest,
  journeyHistory: async () => history,
  journeyFrame: async (_flow: string, screen: string) => blob(`live-${screen}`),
  journeyPastFrame: async (_flow: string, run: string, screen: string) => blob(`past-${run.slice(-4)}-${screen}`),
} as unknown as Runtime;

let root: Root | null = null;
let host: HTMLElement | null = null;
afterEach(() => { act(() => root?.unmount()); host?.remove(); root = null; host = null; });

const settle = () => act(async () => { await new Promise((resolve) => setTimeout(resolve, 20)); });
const statuses = () => [...host!.querySelectorAll('[aria-label^="Step "] .step-status')].map((node) => node.textContent ?? "");
const thumbs = () => [...host!.querySelectorAll(".step-thumb")].map((node) => node.querySelector("img")?.getAttribute("src") ?? node.textContent ?? "");
const chip = (older: boolean) => [...host!.querySelectorAll<HTMLButtonElement>("button.history-chip")][older ? 1 : 0]!;

async function mount(initialPast: string | null, seen: (Array<string | null>) = []) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => {
    root!.render(<JourneyRunPanel runtime={runtime} flow={flow} executionId={null} onRan={() => {}} onRun={() => {}} onGone={() => {}} initialPast={initialPast} onPast={(id) => seen.push(id)} />);
  });
  await settle();
  await settle();
  return seen;
}

describe("a history chip opens that run (#2442)", () => {
  it("shows the older run's statuses and frames, not the latest run's, and goes back", async () => {
    const seen = await mount(null);
    expect(statuses().join(" ")).toMatch(/pass.*pass/);
    expect(thumbs()).toEqual(["blob:live-list", "blob:live-item"]);
    expect(chip(true).getAttribute("aria-pressed")).toBe("false");

    await act(async () => chip(true).click());
    await settle();
    expect(seen).toEqual([OLDER]);
    expect(chip(true).getAttribute("aria-pressed")).toBe("true");
    expect(statuses()[1]).toMatch(/fail/);
    expect(thumbs()[0]).toBe("blob:past-1000-list");
    expect(thumbs()[1]).not.toContain("blob:");
    expect(host!.textContent).toContain("Showing the run of");

    const back = [...host!.querySelectorAll("button")].find((node) => node.textContent === "Back to the latest run")!;
    await act(async () => back.click());
    await settle();
    expect(seen).toEqual([OLDER, null]);
    expect(thumbs()).toEqual(["blob:live-list", "blob:live-item"]);
    expect(statuses().join(" ")).toMatch(/pass.*pass/);
  });

  it("a link that names the run opens on it", async () => {
    await mount(OLDER);
    expect(chip(true).getAttribute("aria-pressed")).toBe("true");
    expect(statuses()[1]).toMatch(/fail/);
  });
});
