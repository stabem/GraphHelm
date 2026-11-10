import { act } from "react";
import type { Root } from "react-dom/client";
import { afterEach, beforeAll, describe, expect, it } from "vitest";

import type { ProofView as PanelType } from "./ProofView";
import type { JourneyFlow, JourneyRun, Runtime } from "../runtime/client";

// Regression: stored run reads can arrive before the journey list refresh.
// jsdom is loaded inside beforeAll, not as the file's vitest environment: vitest gives a worker 60 s
// to start and loading jsdom alone can take longer on a busy machine, so the file never ran. Here the
// load is bounded by the hook's own timeout.
const JSDOM_LOAD_MS = 300_000;
let ProofView: typeof PanelType;
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
  ({ ProofView } = await import("./ProofView"));
  // jsdom has no object URLs: the stub names a URL after the frame it was made from.
  URL.createObjectURL = (blob: Blob) => `blob:${(blob as Blob & { tag: string }).tag}`;
  URL.revokeObjectURL = () => undefined;
}, JSDOM_LOAD_MS);
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;


let root: Root | undefined;
afterEach(async () => { if (root) await act(async () => root!.unmount()); root = undefined; document.body.innerHTML = ""; });

const WARNING = "This run used a checkout behind origin/main (as of the last fetch). Update the project (git pull), then run again.";
async function show(run: JourneyRun): Promise<HTMLElement> {
  const flow = { id: "demo", title: "Demo", status: "approved", screens: [], edges: [], paths: {}, lastRun: { status: "passed", kind: "preview", ranAt: null, reason: null, failingStep: null, flowChanged: false } } as unknown as JourneyFlow;
  const runtime = { journeyRun: async () => run, journeyHistory: async () => [], requests: async () => [] } as unknown as Runtime;
  const host = document.createElement("div"); document.body.append(host); root = createRoot(host);
  await act(async () => { root!.render(<ProofView projectKey="demo" runtime={runtime} journeys={[flow]} error={null} executionId={null} onRan={() => {}} />); });
  await act(async () => { await new Promise((resolve) => setTimeout(resolve, 30)); });
  return host;
}
const ran = { state: "ready", kind: "preview", result: "pass", screens: {}, edges: {} } as const;

describe("run checkout (#2469)", () => {
  it("shows the project HEAD and no warning when the checkout is up to date", async () => {
    const host = await show({ ...ran, head: "0123456789abcdef0123456789abcdef01234567", behindOrigin: 0 });
    const line = host.querySelector(".run-checkout");
    expect(line?.textContent).toContain("0123456");
    expect(line?.getAttribute("title")).toBe("0123456789abcdef0123456789abcdef01234567");
    expect(host.querySelector(".run-behind")).toBeNull();
  });
  it("warns, with the count and what to do, when the checkout was behind origin/main", async () => {
    const host = await show({ ...ran, head: "0123456789abcdef0123456789abcdef01234567", behindOrigin: 2, warning: WARNING });
    const warn = host.querySelector(".run-behind");
    expect(warn?.getAttribute("role")).toBe("status");
    expect(warn?.textContent).toContain("2 commits behind");
    // #2710: the Runtime's sentence already says "behind origin/main"; the header must not repeat it.
    expect(warn?.textContent?.match(/behind origin\/main/g)).toHaveLength(1);
    expect(warn?.textContent?.match(/as of the last fetch/g)).toHaveLength(1);
    expect(warn?.textContent).toContain("git pull");
  });
  it("says nothing about the checkout when the Runtime did not record it", async () => {
    const host = await show({ ...ran });
    expect(host.querySelector(".run-checkout")).toBeNull();
    expect(host.querySelector(".run-behind")).toBeNull();
  });
});
