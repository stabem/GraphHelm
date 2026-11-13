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

function Titled({ runtime }: { runtime: Runtime }) {
  const state = useJourneyRun(runtime, "demo", null, () => undefined);
  return createElement("div", null,
    createElement("button", { type: "button", onClick: () => state.start() }, "Run"),
    createElement("output", null, state.error === null ? "" : `${state.error.title} | ${state.error.cause} | ${state.error.nextStep}`),
  );
}

describe("a failed run read is not a failed start (#2935)", () => {
  const mount = async (runtime: Runtime) => {
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
    await act(async () => { root?.render(createElement(Titled, { runtime })); });
  };

  it("says the run could not be read when opening the journey fails, with no start attempted", async () => {
    const start = vi.fn();
    const runtime = { journeyRun: vi.fn().mockRejectedValue(new Error("network down")), startJourneyRun: start, journeyHistory: vi.fn().mockResolvedValue([]) } as unknown as Runtime;
    await mount(runtime);

    expect(start).not.toHaveBeenCalled();
    expect(container?.textContent).toContain("Run could not be read | The Runtime could not return this journey's run. | Try opening this journey again.");
    expect(container?.textContent).not.toContain("could not start");
  });

  it("still says the run could not start when Run is pressed and refused", async () => {
    const runtime = {
      journeyRun: vi.fn().mockResolvedValue({ state: "none" }),
      startJourneyRun: vi.fn().mockRejectedValue(new RuntimeError("The flow is not approved.", 409, "preview.flow_invalid")),
      journeyHistory: vi.fn().mockResolvedValue([]),
    } as unknown as Runtime;
    await mount(runtime);
    await act(async () => { container?.querySelector("button")?.click(); });

    expect(container?.textContent).toContain("Run could not start | The flow is not approved. | Fix the cause and try Run again.");
  });
});
