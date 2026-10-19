import { useEffect, useRef, useState } from "react";
import type { JourneyFrame, JourneyHistoryView, JourneyRunView } from "../runtime/types";

export interface JourneyRunSource {
  start: (flowId: string, force: boolean, confirm?: boolean) => Promise<JourneyRunView>;
  read: (flowId: string) => Promise<JourneyRunView>;
  screenFrame: (flowId: string, screenId: string) => Promise<JourneyFrame | null>;
  liveFrame: (sessionId: string, etag: string | null) => Promise<JourneyFrame | null>;
  /** #history: earlier finished runs, and one screen's frame as such a run kept it. Absent: no history. */
  history?: (flowId: string) => Promise<JourneyHistoryView>;
  historyFrame?: (flowId: string, run: string, screenId: string) => Promise<JourneyFrame | null>;
}

const RUN_POLL_MS = 1000;
let activePreview: symbol | null = null;

/** What to do about each preview refusal the Runtime names; the bare message said only "cannot be previewed". */
const PREVIEW_NEXT: Record<string, string> = {
  "preview.busy": "Another run of this flow is still going. Wait for it to finish, then Run again.",
  "preview.unwritable": "The project's .graphhelm folder cannot be written. Check that it exists and is yours, then Run again.",
  "preview.launcher_not_isolated": 'Recording proof needs an isolated launcher. Set `"isolated": true` in .graphhelm/journey-fixture.json, then Run again.',
  "preview.flow_unknown": "No journey flow has this id. Check the id in .graphhelm/journeys.",
  "preview.project_invalid": "The Runtime cannot read the project folder. Restart it with `graphhelm studio start` in the project.",
  "preview.keyring_missing": "Start the Runtime with its sealed keyring (`graphhelm studio start`), then Run again.",
  "flow.scope_path_missing": "A screen's `scope` names a path this project does not have. Fix that path in .graphhelm/journeys/<flow>.journey.yaml (`graphhelm journey validate` lists it), then Run again.",
  "flow.scope_path_outside_project": "A screen's `scope` names a path outside this project. Point it at a file inside the project (`graphhelm journey validate` lists it), then Run again.",
};

/** #1448: every other `flow.*` refusal is a problem in the flow file itself; the validator names it. */
const FLOW_NEXT = "The journey flow file has a problem. Run `graphhelm journey validate` on .graphhelm/journeys/<flow>.journey.yaml to see it, fix it, then Run again.";

/** The refusal as the owner reads it: the Runtime's words, its code, and the next step when known. */
export function refusalText(cause: unknown): string {
  const message = cause instanceof Error ? cause.message : String(cause);
  const code = typeof cause === "object" && cause !== null && typeof (cause as { code?: unknown }).code === "string" ? (cause as { code: string }).code : "";
  if (!code) return message;
  const next = PREVIEW_NEXT[code] ?? (code.startsWith("flow.") ? FLOW_NEXT : undefined);
  return `${message} (${code}).${next ? ` ${next}` : ""}`;
}

export function useJourneyRun(flowId: string, source: JourneyRunSource | undefined, autoStart = true): { offered: boolean; run: JourneyRunView | null; frames: Record<string, string>; failure: string | null; busyReason: string | null; launching: boolean; again: () => void; confirm: () => void; confirming: boolean } {
  const [run, setRun] = useState<JourneyRunView | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const [absent, setAbsent] = useState(false);
  const [frames, setFrames] = useState<Record<string, string>>({});
  // Which request this is: the initial cached read, a Run (`force`), or a confirmed held step.
  const [attempt, setAttempt] = useState<{ n: number; confirm: boolean }>({ n: 0, confirm: false });
  // A start is in flight: set with the click itself, cleared by that start's own answer or
  // failure. Not derived from what the answer says, so two equal failures in a row still clear it.
  const [starting, setStarting] = useState(false);
  const [busyReason, setBusyReason] = useState<string | null>(null);
  const starts = useRef<{ key: string; token: symbol | null; promise: Promise<JourneyRunView> } | null>(null);
  const held = useRef(new Map<string, { key: string; url: string | null }>());
  useEffect(() => {
    if (source === undefined) return undefined;
    const key = `${flowId}:${attempt.n}:${attempt.confirm}`;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    if (starts.current?.key !== key) {
      starts.current = { key, token: attempt.n === 0 ? null : activePreview, promise: attempt.n === 0 ? autoStart ? source.start(flowId, false) : source.read(flowId) : attempt.confirm ? source.start(flowId, false, true) : source.start(flowId, true) };
    }
    const token = starts.current.token;
    const release = () => { if (token !== null && activePreview === token) activePreview = null; };
    const fail = (cause: unknown) => {
      release();
      if (cancelled) return;
      setStarting(false);
      if (typeof cause === "object" && cause !== null && (cause as { httpStatus?: unknown }).httpStatus === 404) setAbsent(true);
      else setFailure(refusalText(cause));
    };
    const take = (next: JourneyRunView) => {
      if (next.state !== "running") release();
      if (cancelled) {
        if (next.state === "running" && token !== null) timer = setTimeout(() => { source.read(flowId).then(take, fail); }, RUN_POLL_MS);
        return;
      }
      setStarting(false);
      setRun(next);
      setFailure(null);
      if (next.state === "running") timer = setTimeout(() => { source.read(flowId).then(take, fail); }, RUN_POLL_MS);
    };
    starts.current.promise.then(take, fail);
    return () => { cancelled = true; if (timer !== undefined && token === null) clearTimeout(timer); };
  }, [flowId, source, attempt, autoStart]);
  useEffect(() => {
    if (source === undefined || run === null) return;
    for (const [screenId, screen] of Object.entries(run.screens ?? {})) {
      if (!screen.frame) continue;
      const key = `${run.digest ?? ""}:${run.commit ?? ""}:${attempt.n}:${screen.result ?? ""}`;
      if (held.current.get(screenId)?.key === key) continue;
      held.current.set(screenId, { key, url: held.current.get(screenId)?.url ?? null });
      source.screenFrame(flowId, screenId).then((frame) => {
        const entry = held.current.get(screenId);
        if (frame === null || entry === undefined || entry.key !== key) return;
        const url = URL.createObjectURL(frame.blob);
        if (entry.url !== null) URL.revokeObjectURL(entry.url);
        entry.url = url;
        setFrames((before) => ({ ...before, [screenId]: url }));
      }, () => { held.current.delete(screenId); });
    }
  }, [flowId, source, run, attempt]);
  useEffect(() => {
    const kept = held.current;
    return () => {
      for (const entry of kept.values()) if (entry.url !== null) URL.revokeObjectURL(entry.url);
      kept.clear();
    };
  }, []);
  const launch = (confirm: boolean) => {
    if (activePreview !== null) { setBusyReason("Can't run this journey: another journey is still running in this tab."); return; }
    activePreview = Symbol(flowId);
    setBusyReason(null);
    setStarting(true);
    if (!confirm) { setRun(null); setFrames({}); }
    setFailure(null);
    if (!confirm) {
      for (const entry of held.current.values()) if (entry.url !== null) URL.revokeObjectURL(entry.url);
      held.current.clear();
    }
    setAttempt((before) => ({ n: before.n + 1, confirm }));
  };
  return { offered: source !== undefined && !absent, run, frames, failure, busyReason, launching: starting, again: () => launch(false), confirm: () => launch(true), confirming: attempt.confirm && starting };

}

/** What a journey's last run says, folded from its steps (#run-all): one word for the rail, like a
 * test explorer. `stale` = the cached run belongs to an older flow and needs a rerun. */
export type JourneyVerdict = "passed" | "failed" | "stale" | "needs_you" | "not_run" | "running";
export function journeyVerdict(statuses: readonly string[], run: JourneyRunView | null, draft = false, steps: readonly { id: string; needsFrame: boolean }[] = []): JourneyVerdict {
  if (run?.state === "running") return "running";
  if (run?.state === "failed" || statuses.includes("failed")) return "failed";
  if (statuses.includes("stale")) return "stale";
  if (statuses.includes("needs_you")) return "needs_you";
  if (statuses.length > 0 && statuses.every((s) => s === "proven")) return "passed";
  // #1635: a draft has no captures until it is approved; its passing preview is what the rail shows.
  // An approved journey passes only on proven steps, never on a preview.
  // #1653: the preview must hold a passing result for every step, and a frame for every browser
  // step (a backend step has none); a record that says `pass` with no step results is not a pass,
  // and the list agrees with the detail's "Not run".
  if (draft && run?.state === "ready" && run.result === "pass" && steps.length > 0
    && steps.every((step) => run.screens?.[step.id]?.result === "pass" && (!step.needsFrame || run.screens[step.id]?.frame === true))) return "passed";
  return "not_run";
}

/**
 * Run all (#run-all): replay each journey once, in order, one at a time (the tab's one-run lock is
 * held for the whole batch, so a single click never races a row's Run again). Each journey is a
 * forced run - every step in one go - polled until it ends and handed to `onRun`, which is the same
 * cache the rail and Proof read. A journey that fails to start is reported and the batch goes on.
 */
/**
 * #1309 slice 2: the owner's batch approve. Each flow is approved on its own, one after another, so
 * each gets its own owner record and the Runtime's gate decides each one. A refused flow keeps its
 * reason and the rest still run.
 */
/** #1653: a screen whose expectations are all process output (exit, stdout, stderr) is a backend
 * step: its preview captures no frame. Every other screen is a browser screen and needs one. */
const PROCESS_ROLES = new Set(["exit", "stdout", "stderr"]);
export function stepNeedsFrame(screen: { expect?: ReadonlyArray<{ role: string }> } | undefined): boolean {
  return !(screen?.expect?.length && screen.expect.every((item) => PROCESS_ROLES.has(item.role)));
}

export async function approveJourneys(ids: readonly string[], approve: (id: string) => Promise<void>,
  onCurrent: (id: string | null) => void = () => {}): Promise<{ approved: string[]; errors: Record<string, string> }> {
  const approved: string[] = [];
  const errors: Record<string, string> = {};
  try {
    for (const id of ids) {
      onCurrent(id);
      try {
        await approve(id);
        approved.push(id);
      } catch (cause) {
        errors[id] = refusalText(cause);
      }
    }
  } finally {
    onCurrent(null);
  }
  return { approved, errors };
}

export async function runJourneys(ids: readonly string[], source: JourneyRunSource, onRun: (id: string, run: JourneyRunView) => void,
  onCurrent: (id: string | null) => void = () => {}, wait: (ms: number) => Promise<void> = (ms) => new Promise((done) => setTimeout(done, ms))): Promise<{ busy: boolean; errors: Record<string, string> }> {
  if (activePreview !== null) return { busy: true, errors: {} };
  const token = Symbol("run-all");
  activePreview = token;
  const errors: Record<string, string> = {};
  try {
    for (const id of ids) {
      onCurrent(id);
      try {
        let run = await source.start(id, true);
        onRun(id, run);
        while (run.state === "running") {
          await wait(RUN_POLL_MS);
          run = await source.read(id);
          onRun(id, run);
        }
      } catch (cause) {
        errors[id] = cause instanceof Error ? cause.message : String(cause);
      }
    }
  } finally {
    onCurrent(null);
    if (activePreview === token) activePreview = null;
  }
  return { busy: false, errors };
}
