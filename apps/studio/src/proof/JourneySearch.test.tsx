import { act } from "react";
import type { Root } from "react-dom/client";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";

import type { ProofView as PanelType } from "./ProofView";
import type { JourneyFlow, Runtime } from "../runtime/client";

// #2845: the Proof column's search field, through the real ProofView. jsdom is loaded inside beforeAll,
// as the neighbouring component tests do (see ProofCounts.test.tsx).
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
});

const last = { status: "not_run", kind: null, ranAt: null, reason: null, failingStep: null, flowChanged: null };
const flow = (id: string, name: string, feature: string, over: Record<string, unknown> = {}) => ({ id, title: name, name, feature, status: "approved", screens: [], edges: [], paths: {}, lastRun: last, ...over }) as unknown as JourneyFlow;
const flows = [
  flow("studio2-lanes-board", "See the lanes", "Studio", { approvable: true, status: "draft" }),
  flow("studio2-proof-run", "Run a journey", "Studio", { approvable: true, status: "draft" }),
  flow("keel-card-accepted", "Accept in-scope work", "Keel"),
  { id: "skill-driven-delivery", title: null, status: "draft", screens: [], edges: [], paths: {}, lastRun: last, findings: [{ severity: "error", code: "flow.bad" }] } as unknown as JourneyFlow,
];
const run = { state: "ready", runId: "11111111-1111-4111-8111-111111111111", kind: "replay", result: "pass", screens: {}, edges: {} };
function runtimeStub() {
  const startJourneyRun = vi.fn(async (..._args: unknown[]) => ({ ...run }));
  const approveJourney = vi.fn(async (..._args: unknown[]) => ({}));
  const runtime = { journeyRun: async () => ({ state: "none" }), journeyHistory: async () => [], requests: async () => [], journeyFrame: async () => null, startJourneyRun, approveJourney } as unknown as Runtime;
  return { runtime, startJourneyRun, approveJourney };
}
const settle = () => act(async () => { await new Promise((resolve) => setTimeout(resolve, 30)); });
async function mount(runtime: Runtime, journeys = flows) {
  const host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => { root!.render(<ProofView projectKey="demo" runtime={runtime} journeys={journeys} error={null} executionId="exec-1" onRan={() => {}} />); });
  await settle();
  return host;
}
const field = () => document.querySelector<HTMLInputElement>('input[type="search"][aria-label="Search journeys"]')!;
const type = async (text: string) => {
  await act(async () => {
    const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
    set.call(field(), text);
    field().dispatchEvent(new window.Event("input", { bubbles: true }));
  });
};
const cards = () => [...document.querySelectorAll("li.journey")].map((li) => li.getAttribute("aria-label"));
const status = () => document.querySelector(".journey-search-count");
const groups = () => [...document.querySelectorAll(".journey-feature strong")].map((el) => el.textContent);
const panel = () => document.querySelector("section.proof > :not(aside)")?.textContent ?? "";
const button = (label: string) => [...document.querySelectorAll<HTMLButtonElement>("button")].find((candidate) => candidate.getAttribute("aria-label") === label || candidate.textContent === label);

describe("the journeys column search (#2845)", () => {
  it("sits above the list, shows every journey while empty, and has no status or clear button", async () => {
    await mount(runtimeStub().runtime);
    expect(field().placeholder).toBe("Search journeys");
    expect(field().maxLength).toBe(80);
    expect(field().compareDocumentPosition(document.querySelector(".journey-list")!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(cards()).toHaveLength(4);
    expect(status()).toBeNull();
    expect(button("Clear search")).toBeUndefined();
  });

  it("filters the cards as the owner types, by name, id and group, and drops the empty group", async () => {
    await mount(runtimeStub().runtime);
    await type("LANES ");
    expect(cards()).toEqual(["See the lanes"]);
    expect(status()?.textContent).toBe("1 of 4 journeys");
    expect(status()?.closest("[role=status]")?.getAttribute("role")).toBe("status");
    expect(groups()).toEqual(["Studio"]);
    await type("proof-run");
    expect(cards()).toEqual(["Run a journey"]);
    await type("keel");
    expect(cards()).toEqual(["Accept in-scope work"]);
    expect(groups()).toEqual(["Keel"]);
  });

  it("hides the journey that cannot be read like any other, and finds it by its id", async () => {
    await mount(runtimeStub().runtime);
    await type("billing");
    expect(cards()).toEqual([]);
    expect(document.querySelectorAll(".journey-list li")).toHaveLength(0);
    await type("driven");
    expect(cards()).toEqual(["skill-driven-delivery"]);
  });

  it("says when nothing matches, and shows the query as text", async () => {
    await mount(runtimeStub().runtime);
    await type("<b>x</b>");
    expect(cards()).toEqual([]);
    expect(status()?.textContent).toBe("0 of 4 journeys");
    const none = document.querySelector(".journey-search-none")!;
    expect(document.querySelectorAll(".proof-journeys [role=status]")).toHaveLength(1);
    expect(none.closest("[role=status]")).toBe(status()!.closest("[role=status]"));
    expect(none.textContent).toBe("No journey matches “<b>x</b>”.");
    expect(none.querySelector("b")).toBeNull();
  });

  it("clears with the button and with Escape, and keeps focus in the field", async () => {
    await mount(runtimeStub().runtime);
    await type("keel");
    await act(async () => button("Clear search")!.click());
    expect(field().value).toBe("");
    expect(cards()).toHaveLength(4);
    expect(status()).toBeNull();
    expect(document.activeElement).toBe(field());
    await type("keel");
    field().focus();
    await act(async () => { field().dispatchEvent(new window.KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true })); });
    expect(field().value).toBe("");
    expect(cards()).toHaveLength(4);
    expect(document.activeElement).toBe(field());
  });

  it("keeps the header total, and Run all and Approve N, whatever the query", async () => {
    const { runtime, startJourneyRun, approveJourney } = runtimeStub();
    Object.defineProperty(navigator, "locks", { configurable: true, value: { request: async (_name: string, _options: unknown, callback: (lock: unknown) => Promise<void>) => callback({}) } });
    await mount(runtime);
    const labels = () => [document.querySelector(".section-label")?.textContent, ...[...document.querySelectorAll(".journeys-actions button")].map((b) => `${b.textContent}|${(b as HTMLButtonElement).disabled}`)];
    const before = labels();
    expect(before).toEqual(["JOURNEYS · 4", "Approve 2|false", "▶ Run all|false"]);
    await type("keel");
    expect(labels()).toEqual(before);
    await act(async () => button("Approve 2")!.click());
    await settle();
    expect(approveJourney.mock.calls).toEqual([["studio2-lanes-board"], ["studio2-proof-run"]]);
    await act(async () => button("▶ Run all")!.click());
    await settle();
    await settle();
    expect(startJourneyRun.mock.calls.map((call) => call[0])).toEqual(flows.map((entry) => entry.id));
  });

  it("keeps the selected journey and its panel when its card is filtered out", async () => {
    await mount(runtimeStub().runtime);
    await act(async () => button("Select Accept in-scope work")!.click());
    expect(panel()).toContain("Accept in-scope work");
    await type("lanes");
    expect(cards()).toEqual(["See the lanes"]);
    expect(panel()).toContain("Accept in-scope work");
  });

  it("starts empty again when the project changes", async () => {
    const { runtime } = runtimeStub();
    await mount(runtime);
    await type("keel");
    await act(async () => { root!.render(<ProofView projectKey="other" runtime={runtime} journeys={flows} error={null} executionId="exec-1" onRan={() => {}} />); });
    expect(field().value).toBe("");
    expect(cards()).toHaveLength(4);
  });

  it("keeps each group's score, accessible name and tint at the whole group while a query hides some of its journeys", async () => {
    const replay = (status: string) => ({ status, kind: "replay", ranAt: null, reason: null, failingStep: null, flowChanged: false });
    const grouped = [
      flow("alpha-one", "Alpha one", "Studio", { lastRun: replay("passed") }),
      flow("alpha-two", "Alpha two", "Studio", { lastRun: replay("failed") }),
      flow("alpha-three", "Alpha three", "Studio", { lastRun: replay("not_run") }),
      flow("keel-one", "Keel one", "Keel", { lastRun: replay("passed") }),
    ];
    await mount(runtimeStub().runtime, grouped);
    const headers = () => [...document.querySelectorAll(".group-score")].map((el) => `${el.getAttribute("aria-label")}|${el.textContent}|${el.className}`);
    const full = headers();
    expect(full).toEqual(["Studio: 1 of 3 passed|1/3|mono group-score fail", "Keel: 1 of 1 passed|1/1|mono group-score pass"]);
    await type("alpha three");
    expect(cards()).toEqual(["Alpha three"]);
    expect(headers()).toEqual([full[0]]);
    await type("keel");
    expect(headers()).toEqual([full[1]]);
    await type("");
    expect(headers()).toEqual(full);
  });
});
