import { useEffect, useRef, useState } from "react";
import type { JourneyFrame, JourneyRunView } from "../runtime/types";

export interface JourneyRunSource {
  start: (flowId: string, force: boolean, confirm?: boolean) => Promise<JourneyRunView>;
  read: (flowId: string) => Promise<JourneyRunView>;
  screenFrame: (flowId: string, screenId: string) => Promise<JourneyFrame | null>;
  liveFrame: (sessionId: string, etag: string | null) => Promise<JourneyFrame | null>;
}

const RUN_POLL_MS = 1000;
let activePreview: symbol | null = null;

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
      else setFailure(cause instanceof Error ? cause.message : String(cause));
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
