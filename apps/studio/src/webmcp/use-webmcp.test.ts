import { act, renderHook } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";

import type { RuntimeClient } from "../runtime/client";
import type { AdapterHooks, RegisteredTools } from "./adapter";
import { useWebMcp } from "./use-webmcp";

const registrations: Array<RegisteredTools & { settle: () => void }> = [];

// A fake register, passed in: the suite runs with `isolate: false`, so a module mock would leak.
const register = vi.fn(() => {
  let settle!: () => void;
  const registration = {
    ready: new Promise<void>((done) => { settle = done; }),
    availability: "unavailable" as const,
    names: [] as string[],
    unregister: vi.fn(),
    settle: () => settle(),
  };
  registrations.push(registration as never);
  return registration;
}) as never;
const client = {} as RuntimeClient;
const hooks: AdapterHooks = { onSelect: () => {}, onMutation: () => {}, onActivity: () => {} };

beforeEach(() => { registrations.length = 0; });

// #1408: the WebMCP lifecycle moved out of App; nothing pinned the async refresh, the teardown's
// chips or the unmount.
it("shows the names an asynchronous host confirms once its registration settles", async () => {
  const hook = renderHook(() => useWebMcp(null, register));
  act(() => hook.result.current.registerTools(client, hooks));
  expect(hook.result.current).toMatchObject({ webmcp: "unavailable", tools: [] });

  const registration = registrations[0];
  Object.assign(registration, { availability: "available", names: ["graphhelm_status"] });
  await act(async () => { registration.settle(); await registration.ready; });
  expect(hook.result.current).toMatchObject({ webmcp: "available", tools: ["graphhelm_status"] });
});

it("removes the tools and their chips on teardown, and unregisters on unmount", async () => {
  const hook = renderHook(() => useWebMcp(null, register));
  act(() => hook.result.current.registerTools(client, hooks));
  const first = registrations[0];
  Object.assign(first, { availability: "available", names: ["graphhelm_status"] });
  await act(async () => { first.settle(); await first.ready; });

  act(() => hook.result.current.unregisterTools());
  expect(first.unregister).toHaveBeenCalledTimes(1);
  expect(hook.result.current).toMatchObject({ webmcp: "unavailable", tools: [] });

  act(() => hook.result.current.registerTools(client, hooks));
  const second = registrations[1];
  hook.unmount();
  expect(second.unregister).toHaveBeenCalledTimes(1);
});
