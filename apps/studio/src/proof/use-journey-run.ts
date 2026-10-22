import { useCallback, useEffect, useRef, useState } from "react";

import type { JourneyPastRun, JourneyRun, Runtime } from "../runtime/client";

/** How often a running journey is read again. */
const POLL_MS = 1_000;
/** The console keeps the newest lines only. */
const LOG_LINES = 200;

export interface JourneyRunState {
  run: JourneyRun | null;
  /** Object URLs of the frames this run captured, by screen id. */
  frames: Record<string, string>;
  /** What the Runtime printed while preparing and running, oldest first. */
  log: string[];
  history: JourneyPastRun[];
  error: string | null;
  starting: boolean;
  start: () => void;
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : "The Runtime refused.";
}

/** The selected journey's run: read on open, started by Run, read again while it runs, and its
 * frames fetched with the session's bearer header and shown through object URLs. */
export function useJourneyRun(runtime: Runtime, flowId: string, executionId: string | null, onFinished: () => void): JourneyRunState {
  const [run, setRun] = useState<JourneyRun | null>(null);
  const [frames, setFrames] = useState<Record<string, string>>({});
  const [log, setLog] = useState<string[]>([]);
  const [history, setHistory] = useState<JourneyPastRun[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const urls = useRef(new Map<string, { key: string; url: string | null }>());
  const finished = useRef(onFinished);
  finished.current = onFinished;

  const note = useCallback((line: string | undefined) => {
    const text = line?.trim();
    if (!text) return;
    setLog((lines) => (lines[lines.length - 1] === text ? lines : [...lines, text].slice(-LOG_LINES)));
  }, []);

  useEffect(() => {
    let live = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const take = (next: JourneyRun, ran: boolean) => {
      if (!live) return;
      setRun(next);
      setError(null);
      setStarting(false);
      note(next.observerSetup?.message);
      note(next.observerSetup?.error);
      if (next.state === "running") {
        timer = setTimeout(() => runtime.journeyRun(flowId).then((again) => take(again, true), fail), POLL_MS);
        return;
      }
      note(next.launchLog?.tail);
      note(next.driverLog?.tail);
      runtime.journeyHistory(flowId).then((runs) => { if (live) setHistory(runs.slice(0, 8)); }, () => undefined);
      if (ran) finished.current();
    };
    const fail = (cause: unknown) => {
      if (!live) return;
      setStarting(false);
      setError(message(cause));
    };
    if (attempt === 0) runtime.journeyRun(flowId).then((first) => take(first, false), fail);
    else runtime.startJourneyRun(flowId, executionId).then((first) => take(first, true), fail);
    return () => { live = false; if (timer !== undefined) clearTimeout(timer); };
    // executionId is read when Run is pressed; a change to it alone does not read the run again.
  }, [runtime, flowId, attempt, note]);

  useEffect(() => {
    if (run === null) return;
    const stamp = `${attempt}:${run.startedAt ?? ""}:${run.ranAt ?? ""}`;
    for (const [screen, seen] of Object.entries(run.screens ?? {})) {
      if (!seen.frame) continue;
      const key = `${stamp}:${seen.result ?? ""}`;
      const held = urls.current.get(screen);
      if (held?.key === key) continue;
      urls.current.set(screen, { key, url: held?.url ?? null });
      runtime.journeyFrame(flowId, screen).then((blob) => {
        const entry = urls.current.get(screen);
        if (blob === null || entry === undefined || entry.key !== key) return;
        const url = URL.createObjectURL(blob);
        if (entry.url !== null) URL.revokeObjectURL(entry.url);
        entry.url = url;
        setFrames((before) => ({ ...before, [screen]: url }));
      }, () => { urls.current.delete(screen); });
    }
  }, [run, runtime, flowId, attempt]);

  const drop = useCallback(() => {
    for (const { url } of urls.current.values()) if (url !== null) URL.revokeObjectURL(url);
    urls.current.clear();
  }, []);
  useEffect(() => drop, [drop]);

  const start = useCallback(() => {
    // A new run's steps start without frames: an earlier run's picture is never shown as this one's.
    drop();
    setFrames({});
    setStarting(true);
    setError(null);
    setLog([]);
    setAttempt((n) => n + 1);
  }, [drop]);

  return { run, frames, log, history, error, starting, start };
}
