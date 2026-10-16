import { act, renderHook } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import { liveClocks, useNow } from "./use-now";

afterEach(() => vi.useRealTimers());

it("shares one interval per period across readers and stops with the last one", () => {
  vi.useFakeTimers();
  vi.setSystemTime(1_000);
  const first = renderHook(() => useNow(1000));
  const second = renderHook(() => useNow(1000));
  const paused = renderHook(() => useNow(null));
  const slow = renderHook(() => useNow(30_000));
  expect(liveClocks()).toEqual([1000, 30_000]);

  act(() => { vi.advanceTimersByTime(1000); });
  expect(first.result.current).toBe(2_000);
  expect(second.result.current).toBe(2_000);
  expect(paused.result.current).toBe(1_000);

  first.unmount();
  expect(liveClocks()).toEqual([1000, 30_000]);
  second.unmount();
  slow.unmount();
  paused.unmount();
  expect(liveClocks()).toEqual([]);
});
