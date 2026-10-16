import { act, renderHook } from "@testing-library/react";
import { expect, it, vi } from "vitest";

import type { RuntimeClient } from "./client";
import { useModelsRoutes } from "./use-models-routes";

const listing = { configured: true, routes: [{ id: "judge", provider: "typesafe", transport: "direct_api", enabled: true, model: "m" }] };

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

function mount(client: Record<string, unknown>) {
  const clientRef = { current: client as unknown as RuntimeClient };
  const connectionGeneration = { current: 0 };
  const hook = renderHook(() => useModelsRoutes({ clientRef, connectionGeneration }));
  return { hook, connectionGeneration };
}

// #1399: the models state moved out of App; nothing pinned what a connect or a disconnect forgets.
it("forgets the routes and the models screen on reset, and only the probes on a new connection", async () => {
  const { hook } = mount({ probeRoute: vi.fn(async () => ({ health: "available" })) });
  act(() => {
    hook.result.current.setRoutes(listing as never);
    hook.result.current.setModelsError("boom");
  });
  await act(async () => { await hook.result.current.probeRoute("judge"); });
  expect(hook.result.current.probes).toEqual({ judge: { state: "available" } });
  expect(hook.result.current.activeJudgeRoute).toBe("judge");

  act(() => hook.result.current.clearProbes());
  expect(hook.result.current.probes).toEqual({});
  expect(hook.result.current.routes).not.toBeNull();
  expect(hook.result.current.modelsError).toBe("boom");

  await act(async () => { await hook.result.current.probeRoute("judge"); });
  act(() => hook.result.current.resetModels());
  expect(hook.result.current).toMatchObject({ routes: null, modelsBusy: false, modelsError: "", probes: {}, activeJudgeRoute: null });
});

it("drops a probe reply that a newer probe or another connection superseded", async () => {
  const first = deferred<{ health: string }>();
  const second = deferred<{ health: string }>();
  const probeRoute = vi.fn().mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
  const { hook, connectionGeneration } = mount({ probeRoute });

  let a!: Promise<void>;
  let b!: Promise<void>;
  act(() => { a = hook.result.current.probeRoute("judge"); });
  act(() => { b = hook.result.current.probeRoute("judge"); });
  await act(async () => { second.resolve({ health: "available" }); await b; });
  await act(async () => { first.resolve({ health: "key missing" }); await a; });
  expect(hook.result.current.probes.judge).toEqual({ state: "available" });

  const third = deferred<{ health: string }>();
  probeRoute.mockReturnValueOnce(third.promise);
  let c!: Promise<void>;
  act(() => { c = hook.result.current.probeRoute("judge"); });
  connectionGeneration.current += 1;
  await act(async () => { third.resolve({ health: "key missing" }); await c; });
  expect(hook.result.current.probes.judge).toEqual({ state: "checking" });
});
