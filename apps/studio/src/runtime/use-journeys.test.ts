import { act, renderHook, waitFor } from "@testing-library/react";
import { expect, it, vi } from "vitest";

import { RuntimeError, type RuntimeClient } from "./client";
import { useJourneys, type JourneyInputs } from "./use-journeys";

function stub() {
  return {
    journeyFlows: vi.fn(async () => ({ flows: [{ id: "checkout" }] })),
    journeys: vi.fn(async () => ({ journeys: [{ contractId: "checkout", steps: [] }] })),
    approveJourneyFlow: vi.fn(async () => undefined),
    markJourneyEdgeSafe: vi.fn(async () => undefined),
    watchFlow: vi.fn(async () => undefined),
    journeyRun: vi.fn(async () => ({ state: "none" })),
  };
}

function mount(client: ReturnType<typeof stub>, selected = "run-a", lastJpdSequence = 0) {
  const clientRef = { current: client as unknown as RuntimeClient };
  const selectedRef = { current: selected };
  return renderHook((props: Pick<JourneyInputs, "selected" | "lastJpdSequence">) => useJourneys({ clientRef, selectedRef, ...props }), {
    initialProps: { selected, lastJpdSequence },
  });
}

// #1389: the project's journey reads moved out of App with nothing pinning when they re-read.
it("re-reads flows and journeys on a run switch, a new jpd record, and after each owner action", async () => {
  const client = stub();
  const hook = mount(client);
  await waitFor(() => expect(hook.result.current.journeysView?.journeys[0]?.contractId).toBe("checkout"));
  expect(hook.result.current.flowsRead.view?.flows[0]?.id).toBe("checkout");
  const reads = () => [client.journeyFlows.mock.calls.length, client.journeys.mock.calls.length];
  expect(reads()).toEqual([1, 1]);

  hook.rerender({ selected: "run-b", lastJpdSequence: 0 });
  await waitFor(() => expect(reads()).toEqual([2, 2]));
  hook.rerender({ selected: "run-b", lastJpdSequence: 7 });
  await waitFor(() => expect(reads()).toEqual([3, 3]));

  await act(async () => { await hook.result.current.approveFlow("checkout"); });
  await waitFor(() => expect(reads()).toEqual([4, 4]));
  await act(async () => { await hook.result.current.watchFlow("checkout"); });
  await waitFor(() => expect(reads()).toEqual([5, 5]));
  await act(async () => { await hook.result.current.markFlowEdgeSafe("checkout", "edge-1"); });
  await waitFor(() => expect(reads()).toEqual([6, 6]));

  // A refused action still re-reads: the flows show what the Runtime holds now.
  client.approveJourneyFlow.mockRejectedValueOnce(new RuntimeError("refused", 409, []));
  await act(async () => { await expect(hook.result.current.approveFlow("checkout")).rejects.toThrow("refused"); });
  await waitFor(() => expect(reads()).toEqual([7, 7]));
});

it("keeps the last good journeys on a failed re-read and reports a failure only when there is none", async () => {
  const client = stub();
  client.journeys.mockRejectedValueOnce(new RuntimeError("no journeys route", 404, []));
  client.journeyFlows.mockRejectedValueOnce(new RuntimeError("no flows route", 404, []));
  const hook = mount(client);
  await waitFor(() => expect(hook.result.current.journeysFailed).toBe(true));
  expect(hook.result.current.flowsRead).toEqual({ view: null, failure: "no flows route" });

  hook.rerender({ selected: "run-a", lastJpdSequence: 1 });
  await waitFor(() => expect(hook.result.current.journeysView?.journeys).toHaveLength(1));
  expect(hook.result.current.journeysFailed).toBe(false);

  client.journeys.mockRejectedValueOnce(new RuntimeError("transient", 503, []));
  client.journeyFlows.mockRejectedValueOnce(new RuntimeError("transient", 503, []));
  hook.rerender({ selected: "run-a", lastJpdSequence: 2 });
  await waitFor(() => expect(client.journeys).toHaveBeenCalledTimes(3));
  await waitFor(() => expect(hook.result.current.flowsRead.failure).toBeNull());
  expect(hook.result.current.journeysView?.journeys).toHaveLength(1);
  expect(hook.result.current.journeysFailed).toBe(false);
});

it("starts a journey run against the selected run, and only passes confirm when the owner confirmed", async () => {
  const client = { ...stub(), startJourneyRun: vi.fn(async () => ({ state: "running" })) };
  const clientRef = { current: client as unknown as RuntimeClient };
  const selectedRef = { current: "run-a" };
  const hook = renderHook(() => useJourneys({ clientRef, selectedRef, selected: "run-a", lastJpdSequence: 0 }));
  await hook.result.current.journeyRun.start("checkout", false);
  await hook.result.current.journeyRun.start("checkout", true, true);
  expect(client.startJourneyRun.mock.calls).toEqual([["checkout", false, "run-a"], ["checkout", true, "run-a", true]]);
});
