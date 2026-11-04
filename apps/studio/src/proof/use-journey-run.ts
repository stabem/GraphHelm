import { useCallback, useEffect, useRef, useState } from "react";

import type { JourneyPastRun, JourneyRun, Runtime } from "../runtime/client";
import { RuntimeError } from "../runtime/client";
import { actionErrorDetails, type ActionErrorDetails } from "../states/ActionError";
import { followRun } from "./run-all";
import { earlierVersion, outputLines } from "./steps";

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
  /** #1956: the earlier version's run shown while the current version has none, else `null`.
   * #2442: while an earlier run is being viewed (`viewing`), that run. */
  earlier: JourneyPastRun | null;
  /** #2442: the earlier run asked for with `viewing`, once the history has it; else `null`. */
  viewed: JourneyPastRun | null;
  error: ActionErrorDetails | null;
  starting: boolean;
  /** Run; with a digest, the owner's Confirm of the commands shown under that digest (#1909). */
  start: (confirmDigest?: string) => void;
  dismissError: () => void;
}

/** The selected journey's run: read on open, started by Run, read again while it runs, and its
 * frames fetched with the session's bearer header and shown through object URLs. */
export function useJourneyRun(runtime: Runtime, flowId: string, executionId: string | null, onFinished: () => void, onRun?: (run: JourneyRun) => void, viewing: string | null = null): JourneyRunState {
  const [run, setRun] = useState<JourneyRun | null>(null);
  const [frames, setFrames] = useState<Record<string, string>>({});
  const [log, setLog] = useState<string[]>([]);
  const [history, setHistory] = useState<JourneyPastRun[]>([]);
  const [error, setError] = useState<ActionErrorDetails | null>(null);
  const [starting, setStarting] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const confirming = useRef<string | null>(null);
  const urls = useRef(new Map<string, { key: string; url: string | null }>());
  const finished = useRef(onFinished);
  finished.current = onFinished;
  const seen = useRef(onRun);
  seen.current = onRun;

  const note = useCallback((line: string | undefined) => {
    const text = line?.trim();
    if (!text) return;
    setLog((lines) => (lines[lines.length - 1] === text ? lines : [...lines, text].slice(-LOG_LINES)));
  }, []);

  // History must be available even when the current run has not finished yet.
  useEffect(() => {
    let live = true;
    runtime.journeyHistory(flowId).then(
      runs => { if (live) setHistory(runs); },
      cause => { if (live) setError(actionErrorDetails("Run history could not be read", cause, "The Runtime could not return this journey's history.", "Try opening this journey again.")); },
    );
    return () => { live = false; };
  }, [runtime, flowId]);

  useEffect(() => {
    let live = true;
    const ran = attempt > 0;
    const show = (next: JourneyRun) => {
      setRun(next);
      seen.current?.(next);
      setStarting(false);
      note(next.observerSetup?.message);
      note(next.observerSetup?.error);
      // #1912: a command run's console is what its commands printed, as the Runtime keeps it.
      for (const line of outputLines(next)) note(line);
    };
    const fail = (cause: unknown) => {
      if (!live) return;
      setStarting(false);
      const code = cause instanceof RuntimeError ? cause.code : null;
      const flowCommand = code?.startsWith("flow.") === true || code?.startsWith("preview.flow_") === true
        ? "graphhelm journey validate --all"
        : undefined;
      setError(actionErrorDetails("Run could not start", cause, "The Runtime refused to start the journey run.", "Fix the cause and try Run again.", flowCommand));
    };
    // #1912: the answer to Run or Confirm is followed to the run's recorded result. A read that
    // fails on the way is read again; it no longer leaves the panel on "Running" for good.
    const follow = (first: JourneyRun) => followRun(first, () => runtime.journeyRun(flowId), {
      live: () => live,
      pollMs: POLL_MS,
      onRun: show,
      onError: (cause, failures, final) => {
        setError(actionErrorDetails(
          final ? "Run status could not be read" : "Run status read failed",
          cause,
          final ? "Studio lost the current run status." : "Studio could not read the current run status.",
          final ? "Reload Studio to read the run again." : `Studio will retry reading the run (attempt ${failures}).`,
        ));
      },
    }).then((last) => {
      if (!live || last === null) return;
      if (last.startError !== undefined && last.startError !== null) {
        const code = last.startError.code ?? null;
        const failure = new RuntimeError(last.startError.message ?? "The Runtime refused to start the journey run.", null, code);
        const flowCommand = code?.startsWith("flow.") === true || code?.startsWith("preview.flow_") === true
          ? "graphhelm journey validate --all"
          : undefined;
        setError(actionErrorDetails(
          "Run could not start",
          failure,
          "The Runtime refused to start the journey run.",
          "Fix the named issue, then try Run again.",
          flowCommand,
        ));
        return;
      }
      note(last.launchLog?.tail);
      note(last.driverLog?.tail);
      runtime.journeyHistory(flowId).then((runs) => { if (live) setHistory(runs); }, () => undefined);
      // #1914: a run that began before this panel opened (started from the column) was followed
      // here too; its end reads the journey list again, or the card stays on "not run".
      if (ran || first.state === "running") finished.current();
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
    const wanted: [string, string, { frame: boolean; kind?: "text"; result?: string }, "mobile" | undefined][] =
      Object.entries(run.screens ?? {}).map(([screen, seen]) => [screen, screen, seen, undefined]);
    if (run.viewportsPlanned?.[0] !== "mobile") {
      for (const [screen, seen] of Object.entries(run.viewports?.mobile?.screens ?? {})) wanted.push([`mobile:${screen}`, screen, seen, "mobile"]);
    }
    for (const [slot, screen, seen, viewport] of wanted) {
      if (!seen.frame || seen.kind === "text") continue;
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

  // #1956: the earlier version's frames, read by its run id. #2442: or the earlier run being viewed.
  // Each run's frames sit under their own slots, so moving from one past run to another never shows
  // the first one's frame for a screen the second kept none of.
  const viewed = viewing === null ? null : history.find((past) => past.run === viewing) ?? null;
  const earlier = viewed ?? earlierVersion(run, history);
  useEffect(() => {
    if (earlier === null) return;
    const fetchFrame = (slot: string, screen: string, viewport: "mobile" | undefined) => {
      const key = `earlier:${earlier.run}`;
      const held = urls.current.get(slot);
      if (held?.key === key) return;
      urls.current.set(slot, { key, url: held?.url ?? null });
      runtime.journeyPastFrame(flowId, earlier.run, screen, viewport).then((blob) => {
        const entry = urls.current.get(slot);
        if (blob === null || entry === undefined || entry.key !== key) return;
        const url = URL.createObjectURL(blob);
        if (entry.url !== null) URL.revokeObjectURL(entry.url);
        entry.url = url;
        setFrames((before) => ({ ...before, [slot]: url }));
      }, () => { urls.current.delete(slot); });
    };
    for (const [screen, seen] of Object.entries(earlier.screens ?? {})) {
      if (seen.frame === true && seen.kind !== "text") fetchFrame(pastSlot(earlier.run, screen), screen, undefined);
    }
    // The mobile pass, when the Runtime kept and sent it.
    if (earlier.viewportsPlanned?.[0] !== "mobile") {
      for (const [screen, seen] of Object.entries(earlier.viewports?.mobile?.screens ?? {})) {
        if (seen.frame === true && seen.kind !== "text") fetchFrame(pastSlot(earlier.run, `mobile:${screen}`), screen, "mobile");
      }
    }
  }, [earlier, runtime, flowId]);

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

  const dismissError = useCallback(() => setError(null), []);
  const own = Object.fromEntries(Object.entries(frames).filter(([slot]) => !slot.startsWith(PAST)));
  const split = splitFrames(own, run?.viewportsPlanned?.[0]);
  // A viewed run shows its own frames only; an earlier version's frames show under the run's (none yet).
  const past = earlier === null ? { web: {}, mobile: {} } : splitFrames(pastFrames(frames, earlier.run), earlier.viewportsPlanned?.[0]);
  const web = viewed !== null ? past.web : { ...past.web, ...split.web };
  const mobile = viewed !== null ? past.mobile : { ...past.mobile, ...split.mobile };
  return { run, frames: web, mobileFrames: mobile, log, history, earlier, viewed, error, starting, start, dismissError };
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

const PAST = "past:";
/** #2442: a past run's slot for a screen (`mobile:<screen>` for its mobile pass). */
function pastSlot(run: string, screen: string): string {
  return `${PAST}${run}:${screen}`;
}
/** The frames of one past run, with the run's prefix taken off the slot. */
function pastFrames(slots: Record<string, string>, run: string): Record<string, string> {
  const prefix = `${PAST}${run}:`;
  return Object.fromEntries(Object.entries(slots).filter(([slot]) => slot.startsWith(prefix)).map(([slot, url]) => [slot.slice(prefix.length), url]));
}
