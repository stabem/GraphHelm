import { act } from "react";
import type { Root } from "react-dom/client";
import { afterEach, beforeAll, describe, expect, it } from "vitest";

import type { ReviewModal as ModalType } from "./ReviewModal";
import type { BoardTask, IssueJourney, Runtime } from "../runtime/client";

// jsdom is loaded inside beforeAll, not as the file's vitest environment (see RunCheckout.test.tsx).
const JSDOM_LOAD_MS = 300_000;
let ReviewModal: typeof ModalType;
let createRoot: typeof import("react-dom/client").createRoot;
const revoked: string[] = [];
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
  ({ ReviewModal } = await import("./ReviewModal"));
  URL.createObjectURL = (blob: Blob) => `blob:${(blob as Blob & { tag: string }).tag}`;
  URL.revokeObjectURL = (url: string) => { revoked.push(url); };
}, JSDOM_LOAD_MS);
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | undefined;
afterEach(async () => { if (root) await act(async () => root!.unmount()); root = undefined; document.body.innerHTML = ""; revoked.length = 0; });

const journey = {
  id: "demo", title: "Demo",
  before: { run: "r1", current: true, commit: null, ranAt: null, result: "pass", screens: [{ screen: "graph", frameUrl: "/web", viewports: { mobile: "/mobile" } }] },
  now: null,
} as unknown as IssueJourney;
const task = { key: "k", issue: 721, pr: 721 } as unknown as BoardTask;
const tick = () => act(async () => { await new Promise((resolve) => setTimeout(resolve, 20)); });

describe("review modal frames", () => {
  it("keeps the shown frame readable until the next one has arrived, and frees it after", async () => {
    let release: (blob: Blob | null) => void = () => {};
    const runtime = {
      issueJourneys: async () => [journey],
      frameAt: (url: string) => url === "/mobile" ? new Promise<Blob | null>((resolve) => { release = resolve; }) : Promise.resolve(Object.assign(new Blob(["w"]), { tag: "web" })),
    } as unknown as Runtime;
    const host = document.createElement("div"); document.body.append(host); root = createRoot(host);
    await act(async () => { root!.render(<ReviewModal runtime={runtime} executionId="demo" task={task} onClose={() => {}} />); });
    await tick();
    expect(host.querySelector("img")?.getAttribute("src")).toBe("blob:web");

    const mobile = [...host.querySelectorAll("button")].find((button) => button.textContent === "Mobile")!;
    await act(async () => { mobile.dispatchEvent(new MouseEvent("click", { bubbles: true })); });
    await tick();
    // The mobile frame is still on its way: the image on screen must still be a readable one.
    expect(host.querySelector("img")?.getAttribute("src")).toBe("blob:web");
    expect(revoked).not.toContain("blob:web");

    await act(async () => { release(Object.assign(new Blob(["m"]), { tag: "mobile" })); });
    await tick();
    expect(host.querySelector("img")?.getAttribute("src")).toBe("blob:mobile");
    expect(revoked).toContain("blob:web");

    await act(async () => root!.unmount()); root = undefined;
    expect(revoked).toContain("blob:mobile");
  });
});

describe("review modal states how the Now layer is drawn", () => {
  const both = {
    ...journey,
    now: { pr: 721, headSha: "721abcde", ranAt: null, result: "pass", screens: [{ screen: "graph", frameUrl: "/now" }] },
  } as unknown as IssueJourney;
  const open = async () => {
    const runtime = {
      issueJourneys: async () => [both],
      frameAt: async (url: string) => Object.assign(new Blob([url]), { tag: url.slice(1) }),
    } as unknown as Runtime;
    const host = document.createElement("div"); document.body.append(host); root = createRoot(host);
    await act(async () => { root!.render(<ReviewModal runtime={runtime} executionId="demo" task={task} onClose={() => {}} />); });
    await tick();
    return host;
  };
  const press = async (host: HTMLElement, label: string) => {
    const button = [...host.querySelectorAll("button")].find((each) => each.textContent === label)!;
    await act(async () => { button.dispatchEvent(new MouseEvent("click", { bubbles: true })); });
    await tick();
  };
  const facts = (host: HTMLElement) => host.querySelector("figure")?.textContent ?? "";

  it("reads the blend of Changes and the opacity of Overlay from the drawn image, and follows the slider", async () => {
    const host = await open();
    await press(host, "Changes");
    expect(facts(host)).toContain("Now layer drawn with blend difference, opacity 1");
    await press(host, "Overlay");
    expect(facts(host)).toContain("Now layer drawn with blend normal, opacity 0.5");
    const slider = host.querySelector<HTMLInputElement>('input[type="range"]')!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(slider, "25");
      slider.dispatchEvent(new window.Event("input", { bubbles: true }));
    });
    expect(facts(host)).toContain("Now layer drawn with blend normal, opacity 0.25");
  });

  it("draws the statement out of sight: one clipped pixel, no layout of its own", async () => {
    const host = await open();
    await press(host, "Changes");
    const statement = host.querySelector<HTMLElement>("figure span")!;
    expect(statement.style.position).toBe("absolute");
    expect(statement.style.width).toBe("1px");
    expect(statement.style.overflow).toBe("hidden");
  });
});
