/** @vitest-environment jsdom */
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";

import { RuntimeError, type Runtime } from "../runtime/client";
import { useJourneyRun, type JourneyRunState } from "./use-journey-run";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

let root: Root | null = null;
let container: HTMLDivElement | null = null;

afterEach(async () => {
  if (root !== null) await act(async () => root?.unmount());
  root = null;
  container?.remove();
  container = null;
  vi.useRealTimers();
});

function Harness({ runtime }: { runtime: Runtime }) {
  const state = useJourneyRun(runtime, "demo", null, () => undefined);
  const cause = (error: JourneyRunState["error"]): string => {
    if (typeof error === "string") return error;
    return error?.cause ?? "";
  };
  return createElement("div", null,
    createElement("button", { type: "button", onClick: () => state.start() }, "Run"),
    createElement("output", null, cause(state.error)),
  );
}

describe("journey action error persistence (#2445)", () => {
  it("keeps the failed poll visible after the next successful read", async () => {
    vi.useFakeTimers();
    const read = vi.fn()
      .mockResolvedValueOnce({ state: "none" })
      .mockRejectedValueOnce(new RuntimeError("temporary poll fault", 503, "runtime.unavailable"))
      .mockResolvedValue({ state: "ready" });
    const start = vi.fn().mockResolvedValue({ state: "running" });
    const runtime = { journeyRun: read, startJourneyRun: start, journeyHistory: vi.fn().mockResolvedValue([]) } as unknown as Runtime;
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);

    await act(async () => { root?.render(createElement(Harness, { runtime })); });
    await act(async () => { container?.querySelector("button")?.click(); });
    await act(async () => { await vi.advanceTimersByTimeAsync(1_000); });
    expect(container.textContent).toContain("temporary poll fault");

    await act(async () => { await vi.advanceTimersByTimeAsync(2_000); });
    expect(container.textContent).toContain("temporary poll fault");
    expect(read).toHaveBeenCalledTimes(3);
  });
});
