import { StrictMode, act } from "react";
import type { Root } from "react-dom/client";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";

import type { ProofView as ProofViewType } from "../proof/ProofView";
import type { Topbar as TopbarType } from "../topbar/Topbar";
import type { JourneyFlow, Runtime } from "../runtime/client";
import type { PageTool, PageToolResult } from "./page-tools";

// #2839: the Studio's WebMCP page tools. A fake host stands in for the browser's `document.modelContext`
// and implements only what the Chrome in the journey driver was seen to do: `registerTool(tool, {signal})`
// returns a promise, an aborted signal removes the tool, a second tool of one name and an already aborted
// signal reject, and `execute`'s return value reaches the caller as JSON.
// jsdom is loaded inside beforeAll, as the neighbouring component tests do (see RunCheckout.test.tsx).
const JSDOM_LOAD_MS = 300_000;
let usePageTools: typeof import("./page-tools").usePageTools;
let ProofView: typeof ProofViewType;
let Topbar: typeof TopbarType;
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
  ({ usePageTools } = await import("./page-tools"));
  ({ ProofView } = await import("../proof/ProofView"));
  ({ Topbar } = await import("../topbar/Topbar"));
  URL.createObjectURL = () => "blob:none";
  URL.revokeObjectURL = () => undefined;
}, JSDOM_LOAD_MS);
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

interface Registered { name: string; description: string; inputSchema: Record<string, unknown>; execute: (input: Record<string, unknown>) => Promise<unknown> }
function fakeHost() {
  const live = new Map<string, Registered>();
  let registrations = 0;
  return {
    live,
    get registrations() { return registrations; },
    registerTool(tool: Registered, options?: { signal?: AbortSignal }) {
      registrations += 1;
      if (options?.signal?.aborted) return Promise.reject(new DOMException("signal is aborted without reason", "AbortError"));
      if (live.has(tool.name)) return Promise.reject(new DOMException("Duplicate tool name", "InvalidStateError"));
      live.set(tool.name, tool);
      options?.signal?.addEventListener("abort", () => { if (live.get(tool.name) === tool) live.delete(tool.name); });
      return Promise.resolve();
    },
    async run(name: string, input: Record<string, unknown> = {}): Promise<PageToolResult> {
      const tool = live.get(name);
      if (tool === undefined) throw new Error(`no live tool ${name}`);
      return JSON.parse(JSON.stringify(await tool.execute(input))) as PageToolResult;
    },
  };
}

let host: ReturnType<typeof fakeHost>;
let root: Root | undefined;
let mountNode: HTMLElement | null = null;
const install = (target: unknown, where: "document" | "navigator" = "document") => Object.defineProperty(where === "document" ? document : navigator, "modelContext", { configurable: true, value: target });
afterEach(async () => {
  if (root) await act(async () => root!.unmount());
  root = undefined;
  mountNode?.remove();
  mountNode = null;
  document.body.innerHTML = "";
  for (const target of [document, navigator]) delete (target as unknown as { modelContext?: unknown }).modelContext;
});
const settle = () => act(async () => { await new Promise((resolve) => setTimeout(resolve, 20)); });
async function render(node: React.ReactNode) {
  mountNode = document.createElement("div");
  document.body.append(mountNode);
  root = createRoot(mountNode);
  await act(async () => { root!.render(node); });
  await settle();
}
const schema = { type: "object", additionalProperties: false, properties: {} };
const spec = (handler: PageTool["handler"], name = "probe_tool"): PageTool => ({ name, description: "A probe.", inputSchema: schema, handler });
function Probe({ handler, name }: { handler: PageTool["handler"]; name?: string }) {
  usePageTools([spec(handler, name)]);
  return null;
}

describe("usePageTools", () => {
  it("registers its tool while mounted and removes it when the component unmounts", async () => {
    host = fakeHost(); install(host);
    await render(<Probe handler={() => ({ ok: true })} />);
    expect([...host.live.keys()]).toEqual(["probe_tool"]);
    expect(host.live.get("probe_tool")?.description).toBe("A probe.");
    expect(await host.run("probe_tool")).toEqual({ ok: true });
    await act(async () => root!.unmount());
    root = undefined;
    expect(host.live.size).toBe(0);
  });

  it("leaves exactly one live registration per tool after StrictMode mounts, unmounts and mounts again", async () => {
    host = fakeHost(); install(host);
    await render(<StrictMode><Probe handler={() => ({ ok: false, reason: "latest" })} /></StrictMode>);
    expect(host.live.size).toBe(1);
    expect(host.registrations).toBe(2);
    expect(await host.run("probe_tool")).toEqual({ ok: false, reason: "latest" });
  });

  it("always runs the latest handler the component passed, without registering again", async () => {
    host = fakeHost(); install(host);
    await render(<Probe handler={() => ({ ok: false, reason: "first" })} />);
    await act(async () => root!.render(<Probe handler={() => ({ ok: false, reason: "second" })} />));
    expect(host.registrations).toBe(1);
    expect(await host.run("probe_tool")).toEqual({ ok: false, reason: "second" });
  });

  it("does nothing and throws nothing when the browser has no WebMCP host", async () => {
    const handler = vi.fn(() => ({ ok: true } as const));
    await render(<StrictMode><Probe handler={handler} /></StrictMode>);
    expect(handler).not.toHaveBeenCalled();
  });

  it("finds a host on navigator when document has none", async () => {
    host = fakeHost(); install(host, "navigator");
    await render(<Probe handler={() => ({ ok: true })} />);
    expect([...host.live.keys()]).toEqual(["probe_tool"]);
  });

  it("answers a handler that throws or rejects with a refusal, never a rejection", async () => {
    host = fakeHost(); install(host);
    await render(<Probe handler={() => { throw new Error("secret detail"); }} />);
    const thrown = await host.run("probe_tool");
    expect(thrown.ok).toBe(false);
    expect(JSON.stringify(thrown)).not.toContain("secret detail");
    await act(async () => root!.render(<Probe handler={() => Promise.reject(new Error("secret detail"))} />));
    const rejected = await host.run("probe_tool");
    expect(rejected.ok).toBe(false);
    expect(JSON.stringify(rejected)).not.toContain("secret detail");
  });
});

const flow = (id: string) => ({ id, title: `Journey ${id}`, status: "approved", screens: [], edges: [], paths: {}, lastRun: { status: "not_run", kind: "replay", ranAt: null, reason: null, failingStep: null, flowChanged: null } }) as unknown as JourneyFlow;
const idle = { state: "none", screens: {}, edges: {} };
const running = { state: "running", screens: {}, edges: {} };
function runtimeStub(over: Record<string, unknown> = {}) {
  const startJourneyRun = (over.startJourneyRun as ReturnType<typeof vi.fn> | undefined) ?? vi.fn(async (..._args: unknown[]) => running);
  const runtime = { journeyRun: async () => idle, journeyHistory: async () => [], requests: async () => [], journeyFrame: async () => null, ...over, startJourneyRun } as unknown as Runtime;
  return { runtime, startJourneyRun };
}
const proof = (runtime: Runtime) => <StrictMode><ProofView projectKey="demo" runtime={runtime} journeys={[flow("a"), flow("b")]} error={null} executionId="exec-1" onRan={() => {}} /></StrictMode>;
const button = (label: string) => [...document.querySelectorAll<HTMLButtonElement>("button")].find((candidate) => candidate.getAttribute("aria-label") === label || candidate.textContent === label);

describe("studio_proof_run", () => {
  it("is registered once, in StrictMode, by the Proof view", async () => {
    host = fakeHost(); install(host);
    await render(proof(runtimeStub().runtime));
    expect(host.live.has("studio_proof_run")).toBe(true);
  });

  it("starts the journey through the very call the Run button makes, and selects it", async () => {
    host = fakeHost(); install(host);
    const { runtime, startJourneyRun } = runtimeStub();
    await render(proof(runtime));
    await act(async () => button("Run Journey b")!.click());
    await settle();
    const byButton = [...startJourneyRun.mock.calls];
    expect(byButton).toEqual([["b", "exec-1"]]);
    await act(async () => button("Select Journey a")!.click());
    startJourneyRun.mockClear();
    expect(button("Select Journey b")?.getAttribute("aria-pressed")).toBe("false");
    expect(await host.run("studio_proof_run", { journey: "b" })).toEqual({ ok: true });
    await settle();
    expect(startJourneyRun.mock.calls).toEqual(byButton);
    expect(button("Select Journey b")?.getAttribute("aria-pressed")).toBe("true");
  });

  // #2845: the Journeys column's search hides cards, not journeys; the tool looks up the whole list.
  it("runs a journey that the column's search currently hides", async () => {
    host = fakeHost(); install(host);
    const { runtime, startJourneyRun } = runtimeStub();
    await render(proof(runtime));
    const search = document.querySelector<HTMLInputElement>('input[type="search"][aria-label="Search journeys"]')!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(search, "Journey a");
      search.dispatchEvent(new window.Event("input", { bubbles: true }));
    });
    expect(button("Run Journey b")).toBeUndefined();
    expect(await host.run("studio_proof_run", { journey: "b" })).toEqual({ ok: true });
    await settle();
    expect(startJourneyRun.mock.calls).toEqual([["b", "exec-1"]]);
  });

  it("refuses while Run is disabled, and for a journey that does not exist", async () => {
    host = fakeHost(); install(host);
    const { runtime, startJourneyRun } = runtimeStub({ startJourneyRun: vi.fn(() => new Promise(() => {})) });
    await render(proof(runtime));
    expect((await host.run("studio_proof_run", { journey: "nope" })).ok).toBe(false);
    expect((await host.run("studio_proof_run", {})).ok).toBe(false);
    expect(startJourneyRun).not.toHaveBeenCalled();
    await act(async () => button("Run Journey a")!.click());
    expect(button("Run Journey b")?.disabled).toBe(true);
    expect((await host.run("studio_proof_run", { journey: "b" })).ok).toBe(false);
    expect(startJourneyRun).toHaveBeenCalledTimes(1);
  });

  // `busy` is React state, so two calls in one tick both read null; the column also holds a synchronous guard.
  it("starts one run when two calls arrive in the same tick, and refuses the second", async () => {
    host = fakeHost(); install(host);
    const { runtime, startJourneyRun } = runtimeStub();
    await render(proof(runtime));
    const [first, second] = await Promise.all([host.run("studio_proof_run", { journey: "a" }), host.run("studio_proof_run", { journey: "b" })]);
    expect(first).toEqual({ ok: true });
    expect(second.ok).toBe(false);
    expect(second.ok === false && second.reason.length > 0).toBe(true);
    await settle();
    expect(startJourneyRun).toHaveBeenCalledTimes(1);
  });

  it("starts a run again once the first start has settled", async () => {
    host = fakeHost(); install(host);
    const { runtime, startJourneyRun } = runtimeStub();
    await render(proof(runtime));
    expect(await host.run("studio_proof_run", { journey: "a" })).toEqual({ ok: true });
    await settle();
    expect(await host.run("studio_proof_run", { journey: "b" })).toEqual({ ok: true });
    await settle();
    expect(startJourneyRun).toHaveBeenCalledTimes(2);
  });

  it("releases the guard when a start rejects, so a later call can start a run", async () => {
    host = fakeHost(); install(host);
    const { runtime, startJourneyRun } = runtimeStub({ startJourneyRun: vi.fn().mockRejectedValueOnce(new Error("refused")).mockResolvedValue(running) });
    await render(proof(runtime));
    expect(await host.run("studio_proof_run", { journey: "a" })).toEqual({ ok: true });
    await settle();
    expect(await host.run("studio_proof_run", { journey: "a" })).toEqual({ ok: true });
    await settle();
    expect(startJourneyRun).toHaveBeenCalledTimes(2);
  });

  it("starts one run when a Run button click is followed in the same tick by a call", async () => {
    host = fakeHost(); install(host);
    const { runtime, startJourneyRun } = runtimeStub();
    await render(proof(runtime));
    let result: PageToolResult | undefined;
    await act(async () => {
      button("Run Journey a")!.click();
      result = await host.run("studio_proof_run", { journey: "b" });
    });
    await settle();
    expect(result?.ok).toBe(false);
    expect(startJourneyRun.mock.calls).toEqual([["a", "exec-1"]]);
  });
});

describe("studio_proof_set_log", () => {
  it("opens and closes the run console exactly as Show log and Hide log do", async () => {
    host = fakeHost(); install(host);
    await render(proof(runtimeStub({ journeyRun: async () => running }).runtime));
    expect(document.querySelector('[role="log"]')).toBeNull();
    expect(await host.run("studio_proof_set_log", { open: true })).toEqual({ ok: true });
    await settle();
    expect(document.querySelector('[role="log"]')).not.toBeNull();
    expect(button("Hide log")?.getAttribute("aria-expanded")).toBe("true");
    expect(await host.run("studio_proof_set_log", { open: false })).toEqual({ ok: true });
    await settle();
    expect(document.querySelector('[role="log"]')).toBeNull();
    expect(button("Show log")?.getAttribute("aria-expanded")).toBe("false");
  });

  it("refuses where the Show log button is absent, and for a value that is not a boolean", async () => {
    host = fakeHost(); install(host);
    await render(proof(runtimeStub().runtime));
    expect(button("Show log")).toBeUndefined();
    expect((await host.run("studio_proof_set_log", { open: true })).ok).toBe(false);
    expect(document.querySelector('[role="log"]')).toBeNull();
    await act(async () => root!.unmount());
    root = undefined;
    host = fakeHost(); install(host);
    await render(proof(runtimeStub({ journeyRun: async () => running }).runtime));
    expect((await host.run("studio_proof_set_log", { open: "yes" })).ok).toBe(false);
    expect(document.querySelector('[role="log"]')).toBeNull();
  });
});

describe("studio_select_view", () => {
  it("switches the tab through the handler the tabs call, and refuses a view that does not exist", async () => {
    host = fakeHost(); install(host);
    const onView = vi.fn();
    await render(<StrictMode><Topbar project="demo" run={null} view="graph" updated={null} onView={onView} onStart={null} /></StrictMode>);
    expect(await host.run("studio_select_view", { view: "lanes" })).toEqual({ ok: true });
    expect(onView).toHaveBeenCalledExactlyOnceWith("lanes");
    onView.mockClear();
    expect((await host.run("studio_select_view", { view: "settings" })).ok).toBe(false);
    expect(onView).not.toHaveBeenCalled();
  });
});
