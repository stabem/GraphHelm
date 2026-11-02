import { createContext, useContext, useEffect, useMemo, useSyncExternalStore } from "react";

import type { AttentionReason, Runtime } from "../runtime/client";
import { cliRemedy, reasonText, remedyLead } from "./attention";

/** What `useRunWaits` knows: the reasons (empty when the run waits on nothing), `"reading"` before
 * the first read lands, `"failed"` when it did not. Neither of the last two is "nothing". */
export type RunWaits = AttentionReason[] | "reading" | "failed";

/** The reasons per execution, given instead of read. Tests render Graph and Lanes as they ship
 * through it; the app never provides it. */
export const RunWaitsGiven = createContext<Readonly<Record<string, RunWaits>> | null>(null);

/** The read behind `useRunWaits`, kept apart from React so a test drives it as it runs: one run's
 * status read at a time, what it knows for that run, and "reading" for any other run, so a run
 * switch never shows the previous run's reasons. A read that lands after a newer one started, or for
 * a run no longer shown, is dropped. */
export class RunWaitsReader {
  #runtime: Runtime | null;
  #run: string | null = null;
  #waits: RunWaits = "reading";
  #read = 0;
  #listeners = new Set<() => void>();

  constructor(runtime: Runtime | null) {
    this.#runtime = runtime;
  }

  subscribe = (listener: () => void): (() => void) => {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  };

  /** What is known for `executionId`: "reading" unless it is the run last read. */
  waitsFor = (executionId: string | null): RunWaits => (executionId !== null && executionId === this.#run ? this.#waits : "reading");

  /** Reads `executionId`'s status. A new run starts from "reading"; the same run keeps what it
   * knows until the new read lands. */
  read(executionId: string | null): void {
    const read = ++this.#read;
    if (executionId !== this.#run) this.#set(executionId, "reading");
    if (this.#runtime === null || executionId === null) return;
    this.#runtime.runAttention(executionId).then(
      (status) => { if (read === this.#read) this.#set(executionId, status.attention === "needs_you" ? status.attentionReasons ?? [] : []); },
      () => { if (read === this.#read) this.#set(executionId, "failed"); },
    );
  }

  #set(run: string | null, waits: RunWaits): void {
    this.#run = run;
    this.#waits = waits;
    for (const listener of this.#listeners) listener();
  }
}

/** What a run waits on, read from its status (`attentionReasons`) each time it moves. */
export function useRunWaits(runtime: Runtime | null, executionId: string | null, moved: string | null): RunWaits {
  const given = useContext(RunWaitsGiven);
  const reader = useMemo(() => new RunWaitsReader(runtime), [runtime]);
  useEffect(() => {
    if (given === null) reader.read(executionId);
  }, [given, reader, executionId, moved]);
  const read = useSyncExternalStore(reader.subscribe, () => reader.waitsFor(executionId), () => reader.waitsFor(executionId));
  if (given !== null) return executionId === null ? "reading" : given[executionId] ?? "reading";
  return read;
}

/** One reason as the approved design says it (docs/design/studio/2325-2324/): what waits, and for
 * a blocked node the CLI line that approves it, since node approval has no Studio surface yet. */
export function WaitsOn({ reason, executionId }: { reason: AttentionReason; executionId: string }) {
  const remedy = cliRemedy(reason, executionId);
  return <>{reasonText(reason)}{remedy !== null && <> {remedyLead(reason)} <code>{remedy}</code></>}</>;
}
