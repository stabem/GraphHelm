/**
 * #746: opening a journey's test RUNS it. The run comes from `useJourneyRun` (#519), the same
 * start/poll/frame logic the Journey tab's flowchart uses: a POST that starts the run or reads the
 * one already going (never a second while one runs), a read once a second while it runs, and each
 * reached screen's real frame. A held destructive step (#548) shows the same confirm as JourneyFlows.
 */
import { useEffect, useRef, useState } from "react";

import type { JourneyRunView, JourneyView } from "../runtime/types";
import { testFrames } from "../runtime/test-frames";
import { useJourneyRun, type JourneyRunSource } from "./journey-flows";
import { TestCanvas } from "./test-canvas";

interface Props {
  journey: JourneyView;
  source: JourneyRunSource;
  selected: number;
  onSelect(n: number): void;
  /** Frames the Studio already holds for this journey (the Graph tab's reads); the run's own win. */
  frameUrl(stepId: string): string | null;
  onMarkSafe(stepId: string): void | Promise<void>;
  onSendBack?: (stepId: string) => void;
  /** Each new run state, so the Proof rows read it too. */
  onRun?: (run: JourneyRunView) => void;
}

const clock = (ms: number) => {
  const s = Math.max(0, Math.floor(ms / 1000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
};

export function ProofRun({ journey, source, selected, onSelect, frameUrl, onMarkSafe, onSendBack, onRun }: Props) {
  const { run, frames: runFrames, failure, again, confirm, confirming } = useJourneyRun(journey.contractId, source);
  const screenOf = (stepId: string) => journey.steps.find((s) => s.stepId === stepId)?.screen?.screenId ?? stepId;
  const running = run?.state === "running";
  // The step being played: the Runtime's `current` screen, else (before it names one) the first
  // step without a result yet.
  const named = running && run?.current
    ? journey.steps.findIndex((s) => s.stepId === run.current || s.screen?.screenId === run.current)
    : -1;
  const currentIdx = named >= 0 || !running ? named
    : journey.steps.findIndex((s) => !run?.screens?.[s.screen?.screenId ?? s.stepId]?.result);
  const currentId = currentIdx >= 0 ? journey.steps[currentIdx]!.stepId : null;
  const frames = testFrames(journey, run, currentId);
  // The selection follows the step being played, until the owner clicks a frame.
  const [pinned, setPinned] = useState(false);
  useEffect(() => { if (!pinned && currentIdx >= 0) onSelect(currentIdx); }, [pinned, currentIdx, onSelect]);
  const reported = useRef<JourneyRunView | null>(null);
  useEffect(() => { if (run !== null && run !== reported.current) { reported.current = run; onRun?.(run); } }, [run, onRun]);
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!running) return undefined;
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, [running]);

  const count = (status: string) => frames.filter((f) => f.status === status).length;
  const preview = run?.kind === "preview" ? <span className="tc-run-kind">Preview — not proof</span> : null;
  let line;
  if (failure !== null) {
    line = <p role="alert" className="tc-run-error">{`Couldn't run this journey: ${failure}`}</p>;
  } else if (run?.state === "failed") {
    const why = run.reason ?? "the Runtime refused the run";
    const detail = run.detail ? ` (${run.detail.code} at ${run.detail.pointer})` : "";
    line = <p role="alert" className="tc-run-error">{`Couldn't run this journey: ${why}${detail}`}</p>;
  } else if (run === null || running) {
    const step = currentIdx >= 0 ? currentIdx + 1 : Math.min(frames.length, count("passed") + count("failed") + 1);
    const since = run?.startedAt ? Date.parse(run.startedAt) : NaN;
    line = (
      <p role="status" className="tc-run-progress">
        {run === null ? "Starting the run…" : `Running · step ${step} of ${frames.length}${Number.isNaN(since) ? "" : ` · ${clock(now - since)}`}`}
        {preview}
      </p>
    );
  } else {
    const parts = [`${count("passed")} passed`];
    if (count("failed") > 0) parts.push(`${count("failed")} failed`);
    if (count("waits_for_you") > 0) parts.push(`${count("waits_for_you")} waits for you`);
    line = <p role="status" className="tc-run-progress">{parts.join(" · ")}{preview}</p>;
  }
  const canRunAgain = (run !== null && !running) || (run === null && failure !== null);
  const runLine = (
    <div className="tc-run">
      {line}
      {canRunAgain && <button type="button" className="tc-secondary" onClick={() => { setPinned(false); again(); }}>Run again</button>}
    </div>
  );
  // #548: the same words and the same handler as JourneyFlows' held step.
  const held = run?.state === "ready" && run.held ? (
    <div className="tc-ins-block journey-flow-held" role="group" aria-label="Step waiting for you">
      <p>{`The next step changes data at ${run.held.base}: “${run.held.act}”. Run it?`}</p>
      <button type="button" className="tc-primary" disabled={confirming} onClick={confirm}>{confirming ? "Running…" : "Run it"}</button>
    </div>
  ) : null;
  return (
    <TestCanvas frames={frames} selected={selected} onSelect={(n) => { setPinned(true); onSelect(n); }}
      frameUrl={(id) => runFrames[screenOf(id)] ?? frameUrl(id)} onMarkSafe={onMarkSafe}
      {...(onSendBack ? { onSendBack } : {})} runLine={runLine} inspectorTop={held} />
  );
}
