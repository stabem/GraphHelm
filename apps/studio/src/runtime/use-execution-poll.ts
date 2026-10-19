import { useCallback, useEffect, useRef, useState, type Dispatch, type MutableRefObject, type SetStateAction } from "react";

import type { RuntimeClient } from "./client";
import { sameValue } from "./same-value";
import type { EventPage, ExecutionStatus, ExecutionSummary } from "./types";

/** What the live tail reads and writes in the Studio's state; App owns all of it. */
export interface ExecutionPollInputs {
  connected: boolean;
  selected: string;
  pollIntervalMs: number;
  clientRef: MutableRefObject<RuntimeClient | null>;
  selectedRef: MutableRefObject<string>;
  eventsRef: MutableRefObject<EventPage | null>;
  executionsRef: MutableRefObject<ExecutionSummary[]>;
  restoredOutsidePage: MutableRefObject<Set<string>>;
  listPageSize: number;
  readEvents: (client: RuntimeClient, id: string, after: number) => Promise<EventPage>;
  observeStatus: (next: ExecutionStatus) => void;
  setEvents: Dispatch<SetStateAction<EventPage | null>>;
  setExecutions: Dispatch<SetStateAction<ExecutionSummary[]>>;
}

/** App's live tail (#1385, moved out of App.tsx unchanged). `markFresh` is what an explicit
 * reload does after it succeeds: the miss count and the stale badge start over. */
export function useExecutionPoll({
  connected, selected, pollIntervalMs, clientRef, selectedRef, eventsRef, executionsRef, restoredOutsidePage,
  listPageSize: LIST_PAGE_SIZE, readEvents, observeStatus, setEvents, setExecutions,
}: ExecutionPollInputs): { stale: boolean; markFresh: () => void; readNativeRequests: MutableRefObject<(() => void) | null> } {
  /**
   * The live tail: the selected run is re-read on an interval so the conversation MOVES.
   *
   * A person sent a message from this page and the reply - already durable in the log - stayed
   * invisible until they pressed refresh (2026-08-30). A chat that needs manual refresh is a log
   * viewer wearing a chat costume. Quiet by design: no busy flag (a spinner every four seconds
   * reads as the page being broken) and read failures are swallowed rather than surfaced (a
   * transient miss on a background read is not something the operator can act on; the next tick
   * either heals it or the explicit actions surface the real error).
   */
  /** Consecutive background reads that failed; three in a row flips the rail to "stale" —
   * a dead Runtime was indistinguishable from a quiet room under a badge stuck on "live". */
  const pollMisses = useRef(0);
  const [stale, setStale] = useState(false);
  const pollGeneration = useRef(0);
  const railRefreshBusy = useRef(false);
  const successfulRailRefreshes = useRef(0);
  /** The selected run's native chat request read; the tick below calls it, so it has no interval
   * of its own (#1380). Set by the effect that owns `nativeRequests`. */
  const readNativeRequests = useRef<(() => void) | null>(null);
  useEffect(() => {
    if (!connected || selected === "") return;
    const generation = ++pollGeneration.current;
    let tickBusy = false;
    let hiddenTicks = 0;
    successfulRailRefreshes.current = 0;
    pollMisses.current = 0;
    setStale(false);
    const timer = setInterval(() => {
      const client = clientRef.current;
      if (!client || tickBusy) return;
      // #503: nobody reads a hidden tab; it polls every fourth tick until it is shown again.
      if (document.visibilityState === "hidden") {
        hiddenTicks += 1;
        if (hiddenTicks % 4 !== 0) return;
      } else {
        hiddenTicks = 0;
      }
      readNativeRequests.current?.();
      tickBusy = true;
      void (async () => {
        try {
          // Incremental: only what the log grew since the last read. A quiet tick leaves the
          // events identity UNTOUCHED, so nothing downstream re-derives or re-fetches. The
          // RAIL is re-read on the same tick: it claims to be a live triage surface, and a
          // non-selected run flipping to needs-you stayed frozen until the next click
          // (round-3, two reviewers).
          const loaded = eventsRef.current;
          const lastSeen = loaded?.events.at(-1)?.sequence ?? 0;
          // Refresh page one on quiet ticks, then refresh the loaded prefix every fifth
          // successful rail read. Deep pages still update without making every poll traverse
          // the entire loaded rail (issue #1010, Card 2).
          const loadedRows = executionsRef.current.filter((run) => !restoredOutsidePage.current.has(run.executionId)).length;
          const readList = async (fullRefresh: boolean) => {
            const first = await client.listExecutions({ limit: LIST_PAGE_SIZE });
            const rows = [...first.executions];
            let cursor = first.hasMore ? first.nextCursor : null;
            if (fullRefresh) {
              for (let hops = 1; cursor !== null && rows.length < loadedRows && hops < 50; hops += 1) {
                const next = await client.listExecutions({ after: cursor, limit: LIST_PAGE_SIZE });
                rows.push(...next.executions);
                cursor = next.hasMore ? next.nextCursor : null;
              }
            }
            return rows;
          };
          // #1560: until the run's first load lands there is no tail to grow from, and reading
          // "since 0" re-paged the whole history beside the load (~55k events on gh-team). The first
          // load owns the history; the status is still read, so a slow load never hides a newer state.
          const [nextStatus, fresh] = await Promise.all([
            client.getStatus(selected, true),
            loaded === null ? Promise.resolve<EventPage>({ head: 0, events: [] }) : readEvents(client, selected, lastSeen),
          ]);
          // The connection too, not just the run: Runtime B can hold the same execution id, and
          // a tick that started against A must not paint B (PR #467 review, P1).
          if (pollGeneration.current !== generation || clientRef.current !== client || selectedRef.current !== selected) return;
          observeStatus(nextStatus);
          // The duplicate filter runs INSIDE the updater, against the array it appends to: a
          // full reload landing mid-tick already contains this tick's events, and filtering
          // against the tick-start cursor alone appended them twice (round-3 speedrunner).
          setEvents((previous) => {
            const base = previous?.events.at(-1)?.sequence ?? 0;
            const novel = fresh.events.filter((event) => event.sequence > base);
            if (novel.length === 0) return previous;
            return previous === null
              ? { head: fresh.head, events: novel }
              : { head: fresh.head, events: [...previous.events, ...novel] };
          });
          pollMisses.current = 0;
          setStale(false);
          if (!railRefreshBusy.current) {
            railRefreshBusy.current = true;
            const fullRefreshDue = successfulRailRefreshes.current >= 4;
            void readList(fullRefreshDue).then((listRows) => {
              if (pollGeneration.current !== generation || clientRef.current !== client || selectedRef.current !== selected) return;
              successfulRailRefreshes.current = fullRefreshDue ? 0 : successfulRailRefreshes.current + 1;
              for (const run of listRows) restoredOutsidePage.current.delete(run.executionId);
              setExecutions((previous) => {
                const refreshed = new Map(listRows.map((run) => [run.executionId, run]));
                const kept = previous.map((run) => {
                  const fresh = refreshed.get(run.executionId);
                  return fresh === undefined || sameValue(fresh, run) ? run : fresh;
                });
                const known = new Set(previous.map((run) => run.executionId));
                const added = listRows.filter((run) => !known.has(run.executionId));
                return added.length === 0 && kept.every((run, index) => run === previous[index])
                  ? previous
                  : [...added, ...kept];
              });
            }).catch(() => {
              // Rail refresh is best-effort; it must not suppress the selected-run poll.
            }).finally(() => {
              railRefreshBusy.current = false;
            });
          }
        } catch {
          // A single transient miss stays quiet on purpose - but persistent failure must not:
          // the screen aging under a "live" badge happened for real (server died mid-session).
          if (pollGeneration.current !== generation || clientRef.current !== client) return;
          pollMisses.current += 1;
          if (pollMisses.current >= 3) setStale(true);
        } finally {
          tickBusy = false;
        }
      })();
    }, pollIntervalMs);
    return () => {
      clearInterval(timer);
      tickBusy = false;
    };
  }, [connected, selected, pollIntervalMs, readEvents, observeStatus]);
  const markFresh = useCallback(() => {
    pollMisses.current = 0;
    setStale(false);
  }, []);
  return { stale, markFresh, readNativeRequests };
}
