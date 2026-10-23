import { useCallback, useEffect, useRef, useState } from "react";

import type { JourneyPastRun, JourneyRun, Runtime } from "../runtime/client";
import { followRun } from "./run-all";
import { outputLines } from "./steps";

/** How often a running journey is read again. */
const POLL_MS = 1_000;
/** The console keeps the newest lines only. */
const LOG_LINES = 200;

export interface JourneyRunState {
  run: JourneyRun | null;
  /** Object URLs of the frames this run captured, by screen id: the run's first viewport. */
  frames: Record<string, string>;
  /** #1777: the mobile frames, by screen id, when the run also played mobile. */
  mobileFrames: Record<string, string>;
  /** What the Runtime printed while preparing and running, oldest first. */
  log: string[];
  history: JourneyPastRun[];
  error: string | null;
  starting: boolean;
  /** Run; with a digest, the owner's Confirm of the commands shown under that digest (#1909). */
  start: (confirmDigest?: string) => void;
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
  const confirming = useRef<string | null>(null);
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
    const ran = attempt > 0;
    const show = (next: JourneyRun) => {
      setRun(next);
      setError(null);
      setStarting(false);
      note(next.observerSetup?.message);
      note(next.observerSetup?.error);
      // #1912: a command run's console is what its commands printed, as the Runtime keeps it.
      for (const line of outputLines(next)) note(line);
    };
    const fail = (cause: unknown) => {
      if (!live) return;
      setStarting(false);
      setError(message(cause));
    };
    // #1912: the answer to Run or Confirm is followed to the run's recorded result. A read that
    // fails on the way is read again; it no longer leaves the panel on "Running" for good.
    const follow = (first: JourneyRun) => followRun(first, () => runtime.journeyRun(flowId), {
      live: () => live,
      pollMs: POLL_MS,
      onRun: show,
      onError: (cause, failures, final) => {
        setError(final
          ? `Lost the run: ${message(cause)} Reload to read it again.`
          : `Reading the run again (${failures}): ${message(cause)}`);
      },
    }).then((last) => {
      if (!live || last === null) return;
      note(last.launchLog?.tail);
      note(last.driverLog?.tail);
      runtime.journeyHistory(flowId).then((runs) => { if (live) setHistory(runs.slice(0, 8)); }, () => undefined);
      if (ran) finished.current();
    });
    const begin = attempt === 0
      ? runtime.journeyRun(flowId)
      // #1909: a Confirm reruns with the digest the owner was shown; a plain Run never confirms.
      : confirming.current !== null
        ? runtime.confirmJourneyRun(flowId, confirming.current, executionId)
        : runtime.startJourneyRun(flowId, executionId);
    begin.then((first) => { if (live) void follow(first); }, fail);
    return () => { live = false; };
    // executionId is read when Run is pressed; a change to it alone does not read the run again.
  }, [runtime, flowId, attempt, note]);

  useEffect(() => {
    if (run === null) return;
    const stamp = `${attempt}:${run.startedAt ?? ""}:${run.ranAt ?? ""}`;
    // #1777: the first viewport's frames are the top level (today's URLs); a mobile pass that came
    // after it is fetched with `?viewport=mobile` and kept under `mobile:<screen>`.
    const wanted: [string, string, { frame: boolean; result?: string }, "mobile" | undefined][] =
      Object.entries(run.screens ?? {}).map(([screen, seen]) => [screen, screen, seen, undefined]);
    if (run.viewportsPlanned?.[0] !== "mobile") {
      for (const [screen, seen] of Object.entries(run.viewports?.mobile?.screens ?? {})) wanted.push([`mobile:${screen}`, screen, seen, "mobile"]);
    }
    for (const [slot, screen, seen, viewport] of wanted) {
      if (!seen.frame) continue;
      const key = `${stamp}:${seen.result ?? ""}`;
      const held = urls.current.get(slot);
      if (held?.key === key) continue;
      urls.current.set(slot, { key, url: held?.url ?? null });
      runtime.journeyFrame(flowId, screen, viewport).then((blob) => {
        const entry = urls.current.get(slot);
        if (blob === null || entry === undefined || entry.key !== key) return;
        const url = URL.createObjectURL(blob);
        if (entry.url !== null) URL.revokeObjectURL(entry.url);
        entry.url = url;
        setFrames((before) => ({ ...before, [slot]: url }));
      }, () => { urls.current.delete(slot); });
    }
  }, [run, runtime, flowId, attempt]);

  const drop = useCallback(() => {
    for (const { url } of urls.current.values()) if (url !== null) URL.revokeObjectURL(url);
    urls.current.clear();
  }, []);
  useEffect(() => drop, [drop]);

  const start = useCallback((confirmDigest?: string) => {
    // A new run's steps start without frames: an earlier run's picture is never shown as this one's.
    confirming.current = confirmDigest ?? null;
    drop();
    setFrames({});
    setStarting(true);
    setError(null);
    setLog([]);
    setAttempt((n) => n + 1);
  }, [drop]);

  const split = splitFrames(frames, run?.viewportsPlanned?.[0]);
  return { run, frames: split.web, mobileFrames: split.mobile, log, history, error, starting, start };
}

/** #1777/#1878: a run's fetched frames by viewport. The top level is the run's first viewport and
 * `mobile:<screen>` a later mobile pass; a run that played mobile alone (first viewport `mobile`)
 * has no web frame, so its top-level frames are mobile and never shown as web. */
export function splitFrames(slots: Record<string, string>, first: "web" | "mobile" | undefined): { web: Record<string, string>; mobile: Record<string, string> } {
  const top: Record<string, string> = {};
  const later: Record<string, string> = {};
  for (const [slot, url] of Object.entries(slots)) {
    if (slot.startsWith("mobile:")) later[slot.slice("mobile:".length)] = url;
    else top[slot] = url;
  }
  return first === "mobile" ? { web: {}, mobile: { ...later, ...top } } : { web: top, mobile: later };
}
