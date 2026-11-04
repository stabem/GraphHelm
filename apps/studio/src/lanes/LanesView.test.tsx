/** @vitest-environment jsdom */
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";

import { Runtime } from "../runtime/client";
import { LanesView } from "./LanesView";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

it("shows a successful lane board when reads take longer than a poll", async () => {
  vi.useFakeTimers();
  vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify({ ok: true, data: {} }), { status: 200 })));
  const board = await new Runtime("synthetic").board("fixture");
  vi.unstubAllGlobals();
  const read = vi.fn(() => new Promise<typeof board>((resolve) => setTimeout(() => resolve(board), 6_000)));
  const runtime = {
    board: read, requestsRead: async () => ({ requests: [] }), runAttention: async () => ({ attention: "none" }),
    collisions: async () => ({ collisions: [] }), capacity: async () => ({
      providers: { configured: false, list: [] }, buildSlots: { configured: 0, unreadable: 0, busy: 0, queued: 0 },
      memory: { availableBytes: null, totalBytes: null, reason: null },
    }),
  } as unknown as Runtime;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);

  try {
    await act(async () => root.render(createElement(LanesView, {
      runtime, runs: ["fixture"], choices: ["fixture"], filter: null, onFilter: () => {}, journeys: [],
    })));
    await act(async () => vi.advanceTimersByTimeAsync(16_001));
    expect(host.textContent).not.toContain("Reading the lanes");
    expect(read.mock.calls.length).toBeGreaterThanOrEqual(2);
  } finally {
    await act(async () => root.unmount());
    host.remove();
    vi.useRealTimers();
    vi.unstubAllGlobals();
  }
});
