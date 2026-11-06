/** @vitest-environment jsdom */
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it } from "vitest";

import type { OwnerRequest } from "../graph/requests";
import type { BoardTask, Runtime, TasksBoard } from "../runtime/client";
import { LanesView } from "./LanesView";

// #2530: selecting a request leaves only its agents in IMPLEMENT / REVIEW / BLOCKED; clearing it
// (the for: line again, the ask card again, or Escape) brings every agent back. Read in the DOM.
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

const task = (lane: string, issue: number, step: BoardTask["step"], blockedBy: BoardTask["blockedBy"] = null): BoardTask => ({
  key: `${lane}-${issue}`, issue, pr: null, lane, currentReviewer: null, step, blockedBy, title: `task ${issue}`, prTitle: null,
  journeys: [], lastSequence: 1,
} as BoardTask);
const request = (requestId: string, title: string, issues: number[]): OwnerRequest => ({
  requestId, ask: title, title, recordedBy: "owner", sequence: 1, status: "in_progress",
  issues: issues.map((issue) => ({ issue, slices: [] })),
} as unknown as OwnerRequest);

const board = {
  now: "2026-11-05T12:00:00Z",
  tasks: [task("alpha", 7, "implement"), task("bravo", 8, "implement"), task("charlie", 9, "review"), task("echo", 9, "implement", { reviewer: "x" })],
  liveness: { lanes: [] },
  stalls: [],
  waiting: [],
} as unknown as TasksBoard;
const requests = [request("r1", "Old ask", [7]), request("r2", "Preflight ask", [8, 9])];

const runtime = {
  board: async () => board,
  requestsRead: async () => ({ requests }),
  runAttention: async () => ({ attention: "none" }),
  collisions: async () => ({ collisions: [] }),
  capacity: async () => ({
    providers: { configured: false, list: [] }, buildSlots: { configured: 0, unreadable: 0, busy: 0, queued: 0 },
    memory: { availableBytes: null, totalBytes: null, reason: null },
  }),
} as unknown as Runtime;

let root: ReturnType<typeof createRoot> | null = null;
let host: HTMLDivElement | null = null;
afterEach(async () => {
  if (root !== null) await act(async () => root!.unmount());
  host?.remove();
  root = null; host = null;
  history.replaceState(null, "", "/");
});

const flush = async () => {
  for (let index = 0; index < 6; index += 1) await act(async () => { await new Promise((resolve) => setTimeout(resolve, 5)); });
};
const visible = () => [...host!.querySelectorAll(".agent-card .agent-name strong")].map((node) => node.textContent ?? "").sort();
const column = (name: string) => [...host!.querySelectorAll(`ul[aria-label="${name}"] .agent-card .agent-name strong`)].map((node) => node.textContent ?? "");
const askCard = (title: string) => [...host!.querySelectorAll<HTMLButtonElement>("button.asked-card")].find((node) => node.textContent?.includes(title))!;
const forLine = (lane: string) => [...host!.querySelectorAll<HTMLButtonElement>(".agent-card")].find((card) => card.querySelector("strong")?.textContent === lane)!.querySelector<HTMLButtonElement>(".serves-ask")!;

it("selecting a request narrows the board to its agents, and clearing it restores every agent", async () => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root!.render(createElement(LanesView, { runtime, runs: ["fixture"], choices: ["fixture"], filter: null, onFilter: () => {}, journeys: [] })));
  await flush();

  // Nothing selected: all four agents on the board.
  expect(visible()).toEqual(["alpha", "bravo", "charlie", "echo"]);

  // Click the WHAT I ASKED card for "Preflight ask" (#8, #9): only its agents remain.
  await act(async () => askCard("Preflight ask").click());
  expect(visible()).toEqual(["bravo", "charlie", "echo"]);
  expect(column("Implement")).toEqual(["bravo"]);
  expect(column("Review")).toEqual(["charlie"]);
  expect(column("Blocked")).toEqual(["echo"]);
  expect(column("Implement")).not.toContain("alpha");
  expect(location.search).toContain("ask=fixture%2Fr2");

  // The ask card toggles off: every agent is back.
  await act(async () => askCard("Preflight ask").click());
  expect(visible()).toEqual(["alpha", "bravo", "charlie", "echo"]);
  expect(location.search).not.toContain("ask=");

  // The 'for:' line on bravo selects its request, and a second click clears it.
  await act(async () => forLine("bravo").click());
  expect(visible()).toEqual(["bravo", "charlie", "echo"]);
  expect(forLine("bravo").getAttribute("aria-pressed")).toBe("true");
  await act(async () => forLine("bravo").click());
  expect(visible()).toEqual(["alpha", "bravo", "charlie", "echo"]);

  // Escape clears a selection made from the ask card.
  await act(async () => askCard("Old ask").click());
  expect(visible()).toEqual(["alpha"]);
  await act(async () => { window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" })); });
  expect(visible()).toEqual(["alpha", "bravo", "charlie", "echo"]);
});

it("a request in the URL is applied on load", async () => {
  history.replaceState(null, "", "/?ask=fixture%2Fr1");
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root!.render(createElement(LanesView, { runtime, runs: ["fixture"], choices: ["fixture"], filter: null, onFilter: () => {}, journeys: [] })));
  await flush();
  expect(visible()).toEqual(["alpha"]);
});

it("a lane holding slices on an older and a newer request stays on the board for the older one", async () => {
  // "foxtrot" works #7 (the older request r1) and #9 (the newer r2): its card keeps counting in both.
  const twoRequests = { ...board, tasks: [task("alpha", 7, "implement"), task("foxtrot", 7, "review"), task("foxtrot", 9, "review")] } as unknown as TasksBoard;
  const twoRuntime = { ...runtime, board: async () => twoRequests } as unknown as Runtime;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root!.render(createElement(LanesView, { runtime: twoRuntime, runs: ["fixture"], choices: ["fixture"], filter: null, onFilter: () => {}, journeys: [] })));
  await flush();

  expect(askCard("Old ask").textContent).toContain("2 agents");
  await act(async () => askCard("Old ask").click());
  expect(visible()).toEqual(["alpha", "foxtrot"]);
  expect(column("Implement")).toEqual(["alpha"]);
  expect(column("Review")).toEqual(["foxtrot"]);
});
