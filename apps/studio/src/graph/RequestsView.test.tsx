/** @vitest-environment jsdom */
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";

import { Runtime } from "../runtime/client";
import { RequestsView } from "./RequestsView";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

it("shows a successful board when each read is slower than the poll interval", async () => {
  vi.useFakeTimers();
  vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify({ ok: true, data: {} }), { status: 200 })));
  const board = await new Runtime("synthetic").board("fixture");
  vi.unstubAllGlobals();
  let completed = 0;
  const read = vi.fn(() => new Promise<typeof board>((resolve) => setTimeout(() => { completed += 1; resolve(board); }, 6_000)));
  const runtime = { board: read, requests: async () => [] } as unknown as Runtime;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);

  try {
    await act(async () => root.render(createElement(RequestsView, { runtime, executionIds: ["fixture"], journeys: [] })));
    await act(async () => vi.advanceTimersByTimeAsync(16_001));
    expect(completed).toBeGreaterThanOrEqual(2);
    expect(host.textContent).toContain("All journeys");
  } finally {
    await act(async () => root.unmount());
    host.remove();
    vi.useRealTimers();
    vi.unstubAllGlobals();
  }
});
