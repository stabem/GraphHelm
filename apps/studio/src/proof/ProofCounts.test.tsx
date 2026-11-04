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
afterEach(async () => { if (root) await act(async () => root!.unmount()); root=undefined; document.body.innerHTML=""; });
describe("summary after loading a saved run", () => {
  it.each(["pass", "fail"] as const)("counts saved %s replay while the list is older", async (result) => {
    const flow = {id:"demo",title:"Demo",status:"approved",screens:[],edges:[],paths:{},lastRun:{status:"not_run",kind:null,ranAt:null,reason:null,failingStep:null,flowChanged:null}} as unknown as JourneyFlow;
    const saved: JourneyRun = {state:"ready",kind:"replay",result,screens:{},edges:{}};
    const runtime = {journeyRun:async()=>saved,journeyHistory:async()=>[],requests:async()=>[]} as unknown as Runtime;
    const host=document.createElement("div");document.body.append(host);root=createRoot(host);
    await act(async()=>{root!.render(<ProofView projectKey="demo" runtime={runtime} journeys={[flow]} error={null} executionId={null} onRan={()=>{}} />);});
    await act(async()=>{await new Promise(resolve=>setTimeout(resolve,30));});
    expect(host.querySelector("li.journey .chip")?.textContent).toContain(result === "pass" ? "passed" : "failed");
    expect(host.querySelector(".proof-counts")?.textContent).toBe(`${result === "pass" ? 1 : 0} passed \u00b7 ${result === "fail" ? 1 : 0} failed \u00b7 0 stale \u00b7 0 not run`);
    expect(host.querySelector(".group-score")?.textContent).toBe(result === "pass" ? "1/1" : "0/1");
  });
});

it("keeps previews, waits, unknown and running apart from replay proof", async () => {
  const last = {status:"passed",kind:"replay",ranAt:null,reason:null,failingStep:null,flowChanged:false};
  const states = [last, {...last,status:"failed"}, {...last,flowChanged:true}, {...last,status:"not_run",kind:null}, {...last,kind:"preview"}, {...last,kind:"preview",status:"failed"}, {...last,status:"held"}, undefined];
  const flows = states.map((lastRun,index)=>({id:`demo-${index}`,title:`Demo ${index}`,status:"approved",screens:[],edges:[],paths:{},lastRun})) as unknown as JourneyFlow[];
  flows.push({...flows[0],id:"running",running:{}} as JourneyFlow);
  const runtime={journeyRun:async()=>({state:"none"}),journeyHistory:async()=>[],requests:async()=>[]} as unknown as Runtime;
  const host=document.createElement("div");document.body.append(host);root=createRoot(host);
  await act(async()=>{root!.render(<ProofView projectKey="demo" runtime={runtime} journeys={flows} error={null} executionId={null} onRan={()=>{}} />);});
  expect([...host.querySelectorAll(".proof-counts")].map(el=>el.textContent)).toEqual([
    "1 passed \u00b7 1 failed \u00b7 1 stale \u00b7 1 not run",
    "2 previews (not proof) \u00b7 1 running \u00b7 1 waiting \u00b7 1 unknown"
  ]);
  expect(host.querySelector(".group-score")?.textContent).toBe("1/9");
  expect(host.querySelectorAll("li.journey")).toHaveLength(9);
});
