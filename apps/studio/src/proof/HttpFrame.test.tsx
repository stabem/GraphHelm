import { act } from "react";
import type { Root } from "react-dom/client";
import { afterEach, beforeAll, describe, expect, it } from "vitest";

import type { ProofView as PanelType } from "./ProofView";
import type { JourneyFlow, Runtime } from "../runtime/client";

// #2945 and #2903, through the real ProofView. jsdom is loaded inside beforeAll, as the neighbouring
// component tests do.
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
const flow = {
  id: "studio2-auth-refusal", title: "Studio auth refusal", name: "Studio auth refusal", feature: "Studio", status: "approved",
  screens: [{ id: "no-session", url: "/health" }, { id: "foreign-origin", url: "/health" }],
  edges: [{ id: "no-session.foreign-origin", from: "no-session", to: "foreign-origin", acts: [{ kind: "http", role: "GET", name: "/health" }] }],
  paths: { main: ["no-session.foreign-origin"] },
  lastRun: last,
} as unknown as JourneyFlow;

const sha = "9878f029" + "0".repeat(56);
const text = { frame: true, kind: "text", result: "pass", status: 403, contentType: "application/json", body: "{\"ok\":false,\"reason\":\"session_required\"}", bodyTruncated: false, bodySha256: sha };
const passed = (screens: Record<string, unknown>) => ({ state: "ready", result: "pass", ranAt: "2026-11-14T10:00:00Z", screens, edges: { "no-session.foreign-origin": { result: "pass" } } });

const settle = () => act(async () => { await new Promise((resolve) => setTimeout(resolve, 30)); });
const runtimeOf = (journeyRun: () => Promise<unknown>) => ({ journeyRun, journeyHistory: async () => [], requests: async () => [], journeyFrame: async () => null }) as unknown as Runtime;
async function mount(runtime: Runtime) {
  const host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => { root!.render(<ProofView projectKey="demo" runtime={runtime} journeys={[flow]} error={null} executionId="exec-1" onRan={() => {}} />); });
}
const stepTiles = () => [...document.querySelectorAll<HTMLElement>(".step-thumb")].map((node) => node.textContent);

describe("an HTTP journey shows what it saw (#2945)", () => {
  it("draws each tile's status and content type, and the panel's request, status, type and reply text", async () => {
    await mount(runtimeOf(async () => passed({ "no-session": text, "foreign-origin": text })));
    await settle();

    expect(stepTiles()).toEqual(["403application/json", "403application/json"]);
    const panel = document.querySelector("section[aria-label='HTTP reply']")!;
    expect(panel.querySelector("[aria-label='Request']")!.textContent).toBe("GET /health");
    expect(panel.querySelector("[aria-label='Status 403']")!.textContent).toBe("403");
    expect(panel.querySelector("[aria-label='Content type']")!.textContent).toBe("application/json");
    expect(panel.querySelector("[aria-label='Reply text']")!.textContent).toBe(text.body);
    expect(panel.textContent).toContain("reply text as recorded, secrets masked · sha256 9878f029…");
    expect(panel.textContent).not.toContain("cut to its last");
    // No screen: no Web / Mobile tabs and no "no mobile frame" line.
    expect(document.querySelector("button[aria-label='Web']")).toBeNull();
    expect(document.body.textContent).not.toContain("kept no mobile frame");
    expect(document.body.textContent).not.toContain("no frame");
    expect(document.querySelector("[role='img'][aria-label$='HTTP journey']")).not.toBeNull();
  });

  it("says the reply was cut when the Runtime cut it, and draws markup in the reply as text", async () => {
    const markup = "<img src=x onerror=alert(1)>";
    await mount(runtimeOf(async () => passed({ "no-session": text, "foreign-origin": { ...text, body: markup, bodyTruncated: true } })));
    await settle();
    await act(async () => { document.querySelector<HTMLElement>("li[aria-label='Step 2'] button")!.click(); });

    const panel = document.querySelector("section[aria-label='HTTP reply']")!;
    expect(panel.querySelector("[aria-label='Reply text']")!.textContent).toBe(markup);
    expect(panel.querySelector("img")).toBeNull();
    expect(panel.textContent).toContain("cut to its last 4096 characters");
  });

  it("keeps today's step when the Runtime's reply carries no status", async () => {
    const old = { frame: true, kind: "text", result: "pass" };
    await mount(runtimeOf(async () => passed({ "no-session": old, "foreign-origin": old })));
    await settle();

    expect(document.querySelector("section[aria-label='HTTP reply']")).toBeNull();
    expect(stepTiles()).toEqual(["no frame", "no frame"]);
  });
});

describe("no verdict before the first read (#2903)", () => {
  const verdictText = () => document.querySelector(".run-verdict")?.textContent ?? null;

  it("writes nothing while the first answer is awaited, then 'Not run yet' for a journey that never ran", async () => {
    let answer: (run: unknown) => void = () => undefined;
    await mount(runtimeOf(() => new Promise((resolve) => { answer = resolve; })));
    await settle();
    expect(verdictText()).toBeNull();
    expect(document.body.textContent).not.toContain("Not run yet");

    await act(async () => { answer({ state: "none" }); });
    await settle();
    expect(verdictText()).toBe("Not run yet");
  });

  it("writes the real result once the first answer is a finished run", async () => {
    await mount(runtimeOf(async () => passed({ "no-session": text, "foreign-origin": text })));
    await settle();
    expect(verdictText()).toMatch(/^Passed/);
  });
});
