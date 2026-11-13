import { act } from "react";
import type { Root } from "react-dom/client";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";

import type { ProofView as PanelType } from "./ProofView";
import type { Runtime } from "../runtime/client";

// #2913: the Proof view when the journeys read failed and there is nothing to show, through the real
// ProofView. jsdom is loaded inside beforeAll, as the neighbouring component tests do.
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
}, JSDOM_LOAD_MS);
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | undefined;
afterEach(async () => {
  if (root) await act(async () => root!.unmount());
  root = undefined;
  document.body.innerHTML = "";
});

const runtime = {} as unknown as Runtime;
const buttons = () => [...document.querySelectorAll("button")].map((button) => button.textContent);

describe("Proof journeys read failure with nothing to show (#2913)", () => {
  it("keeps the failure on screen with Retry only, so it cannot be dismissed into a loading panel", async () => {
    const onRan = vi.fn();
    const host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    await act(async () => { root!.render(<ProofView projectKey="demo" runtime={runtime} journeys={null} error="The Runtime did not answer." executionId="exec-1" onRan={onRan} />); });

    expect(document.querySelector('[role="alert"]')?.textContent).toContain("Journeys could not be read");
    expect(buttons()).toEqual(["Retry"]);
    expect(document.body.textContent).not.toContain("Reading the journeys");

    const readsBefore = onRan.mock.calls.length;
    await act(async () => { document.querySelector<HTMLButtonElement>(".action-error-retry")!.click(); });
    expect(onRan.mock.calls.length).toBe(readsBefore + 1);
    // The read is still failing: the failure stays, it does not turn into a spinner.
    expect(document.querySelector('[role="alert"]')?.textContent).toContain("The Runtime did not answer.");
  });
});
