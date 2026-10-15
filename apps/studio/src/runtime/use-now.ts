import { useEffect, useState } from "react";

/**
 * The Studio's one wall clock (#1380). Every component that renders "how long ago" or a live
 * timer reads `Date.now()` through this hook, and all readers of the same period share ONE
 * interval: a board, a task graph and a proof run open together cost one timer per period, not
 * one per component. The timer starts with its first reader and stops with its last.
 */
type Ticker = { timer: ReturnType<typeof setInterval>; listeners: Set<(now: number) => void> };
const TICKERS = new Map<number, Ticker>();

function subscribe(intervalMs: number, listener: (now: number) => void): () => void {
  let ticker = TICKERS.get(intervalMs);
  if (ticker === undefined) {
    const listeners = new Set<(now: number) => void>();
    const timer = setInterval(() => {
      const now = Date.now();
      for (const each of [...listeners]) each(now);
    }, intervalMs);
    ticker = { timer, listeners };
    TICKERS.set(intervalMs, ticker);
  }
  ticker.listeners.add(listener);
  const held = ticker;
  return () => {
    held.listeners.delete(listener);
    if (held.listeners.size === 0 && TICKERS.get(intervalMs) === held) {
      clearInterval(held.timer);
      TICKERS.delete(intervalMs);
    }
  };
}

/** `Date.now()`, refreshed every `intervalMs`. `null` stops the refresh (a finished run, a fixed
 * time in a test) and the last value stays. */
export function useNow(intervalMs: number | null): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (intervalMs === null) return undefined;
    setNow(Date.now());
    return subscribe(intervalMs, setNow);
  }, [intervalMs]);
  return now;
}

/** How many shared clock intervals are live, by period. For tests that count timers. */
export function liveClocks(): number[] {
  return [...TICKERS.keys()].sort((a, b) => a - b);
}
