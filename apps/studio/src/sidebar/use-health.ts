import { useEffect, useState } from "react";

/** How often the Runtime's health is read again for the restart notice. */
const HEALTH_MS = 30_000;

/** What the sidebar's restart notice needs from `/health` (`previousExit`, `currentRun`). */
export interface Restart {
  /** Epoch seconds the running Runtime started. */
  startedAt: number;
  /** `clean`, `vanished`, ... as the Runtime's lifecycle record says. */
  state: string;
  /** Bytes of a torn journal tail recovered at start-up, if any. */
  recoveredBytes: number | null;
  /** #1962: how many times `serve --supervise` has restarted the Runtime on its own; null when it did not. */
  supervisorRestarts: number | null;
}

/** Reads the restart the running Runtime reports; null when it reports no previous exit. */
export function restartOf(data: unknown): Restart | null {
  if (typeof data !== "object" || data === null) return null;
  const { previousExit, currentRun } = data as { previousExit?: unknown; currentRun?: unknown };
  if (typeof previousExit !== "object" || previousExit === null) return null;
  const exit = previousExit as { state?: unknown; journalTailRecovered?: { trimmedBytes?: unknown } | null; supervisorRestart?: { count?: unknown } | null };
  const started = (currentRun as { startedAt?: unknown } | null | undefined)?.startedAt;
  if (typeof started !== "number") return null;
  const bytes = exit.journalTailRecovered?.trimmedBytes;
  const count = exit.supervisorRestart?.count;
  return { startedAt: started, state: typeof exit.state === "string" ? exit.state : "unknown", recoveredBytes: typeof bytes === "number" ? bytes : null, supervisorRestarts: typeof count === "number" ? count : null };
}

/** The one sentence under "Runtime restarted": why the previous run ended, in the owner's words. */
export function restartDetail(restart: Restart): string {
  const why = restart.state === "clean" ? "The previous run stopped cleanly." : "The previous run stopped without recording why. Its agents were released; nothing was lost.";
  const torn = restart.recoveredBytes === null ? why : `${why} ${restart.recoveredBytes} bytes of a torn last write were set aside.`;
  return restart.supervisorRestarts === null ? torn : `${torn} It was started again on its own.`;
}

/** The running project's restart, read from its Runtime's `/health` through the Studio's own origin. */
export function useRestart(base: string): Restart | null {
  const [restart, setRestart] = useState<Restart | null>(null);
  useEffect(() => {
    let live = true;
    const read = () => {
      fetch(`${base}/health`, { signal: AbortSignal.timeout(10_000) })
        .then((response) => response.json() as Promise<{ data?: unknown }>)
        .then((reply) => { if (live) setRestart(restartOf(reply.data)); }, () => { /* keep the last answer */ });
    };
    read();
    const timer = setInterval(read, HEALTH_MS);
    return () => { live = false; clearInterval(timer); };
  }, [base]);
  return restart;
}
