import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { pollWhileVisible } from "./poll-while-visible";

describe("a Studio tab the owner is not looking at reads nothing (#2762)", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("reads at once and on each tick while visible, skips ticks while hidden, reads on return, stops after cleanup", () => {
    const doc = Object.assign(new EventTarget(), { hidden: false });
    let reads = 0;
    const stop = pollWhileVisible(() => { reads += 1; }, 5_000, doc);
    expect(reads).toBe(1);
    vi.advanceTimersByTime(10_000);
    expect(reads).toBe(3);

    doc.hidden = true;
    doc.dispatchEvent(new Event("visibilitychange"));
    vi.advanceTimersByTime(60_000);
    expect(reads).toBe(3);

    doc.hidden = false;
    doc.dispatchEvent(new Event("visibilitychange"));
    expect(reads).toBe(4);
    vi.advanceTimersByTime(5_000);
    expect(reads).toBe(5);

    stop();
    vi.advanceTimersByTime(60_000);
    doc.dispatchEvent(new Event("visibilitychange"));
    expect(reads).toBe(5);
  });
});
