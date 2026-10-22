/**
 * #746: opening a journey's test RUNS it. The run comes from `useJourneyRun` (#519), the same
 * start/poll/frame logic the Journey tab's flowchart uses: a POST that starts the run or reads the
 * one already going (never a second while one runs), a read once a second while it runs, and each
 * reached screen's real frame. A held destructive step (#548) shows the same confirm as JourneyFlows.
 */
import { useEffect, useRef, useState } from "react";
import { useNow } from "../runtime/use-now";

import type { JourneyFlowView, JourneyRunView, JourneyView } from "../runtime/types";
import { testFrames } from "../runtime/test-frames";
import { runFailure } from "../runtime/run-failure";
import { useJourneyRun, type JourneyRunSource } from "./journey-flows";
import { TestCanvas } from "./test-canvas";
import { CHANGED_AT, FrameCompare, HistoryStrip, useJourneyHistory, type CompareOpen } from "./proof-history";
import { FrameCompareView } from "./frame-compare-view";
import "./proof-history.css";

interface Props {
  journey: JourneyView;
  /** #1287: the flow source, so each frame shows its inputs and checks. */
  flow?: JourneyFlowView | undefined;
  source: JourneyRunSource;
  selected: number;
  onSelect(n: number): void;
  /** Frames the Studio already holds for this journey (the Graph tab's reads); the run's own win. */
  frameUrl(stepId: string): string | null;
  onMarkSafe(stepId: string): void | Promise<void>;
  /** Each new run state, so the Proof rows read it too. */
  onRun?: (run: JourneyRunView) => void;
}

const clock = (ms: number) => {
  const s = Math.max(0, Math.floor(ms / 1000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
};

export function ProofRun({ journey, flow, source, selected, onSelect, frameUrl, onMarkSafe, onRun }: Props) {
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
  const frames = testFrames(journey, run, currentId, flow);
  const screenIds0 = journey.steps.map((s) => s.screen?.screenId ?? s.stepId);
  const failedRun = run?.state === "failed" ? runFailure(run, screenIds0) : null;
  // #1296: the step the run failed on reads failed, even when it never reported a screen result.
  if (failedRun?.step != null && frames[failedRun.step]?.status === "not_run") {
    frames[failedRun.step] = { ...frames[failedRun.step]!, status: "failed", reason: failedRun.text };
  }
  const nowFrame = (stepId: string) => runFrames[screenOf(stepId)] ?? frameUrl(stepId);
  const screenIds = screenIds0;
  const history = useJourneyHistory(journey.contractId, source, run, (screen) => runFrames[screen] ?? null, screenIds);
  const badges: Record<string, string> = {};
  for (const step of journey.steps) {
    const ratio = history.changes[step.screen?.screenId ?? step.stepId];
    if (ratio !== undefined && ratio >= CHANGED_AT) badges[step.stepId] = "changed";
  }
  // The selection follows the step being played, until the owner clicks a frame.
  const [pinned, setPinned] = useState(false);
  const [comparing, setComparing] = useState<(CompareOpen & { stepId: string }) | null>(null);
  useEffect(() => { if (!pinned && currentIdx >= 0) onSelect(currentIdx); }, [pinned, currentIdx, onSelect]);
  const reported = useRef<JourneyRunView | null>(null);
  useEffect(() => { if (run !== null && run !== reported.current) { reported.current = run; onRun?.(run); } }, [run, onRun]);
  const now = useNow(running ? 1000 : null);

  const count = (status: string) => frames.filter((f) => f.status === status).length;
  const preview = run?.kind === "preview" ? <span className="tc-run-kind">Preview — not proof</span> : null;
  let line;
  if (failure !== null) {
    line = <p role="alert" className="tc-run-error">{`Couldn't run this journey: ${failure}`}</p>;
  } else if (failedRun !== null) {
    line = (
      <div role="alert" className="tc-run-error">
        <p>{`Couldn't run this journey. ${failedRun.text}`}</p>
        {failedRun.log && <pre className="tc-run-log" aria-label={failedRun.logFrom === "driver" ? "What the browser driver said" : "What the launcher said"}>{failedRun.log}</pre>}
        {failedRun.logPath && <p className="tc-run-logpath">{`Full log: ${failedRun.logPath}`}</p>}
      </div>
    );
  } else if (run === null || running) {
    const step = currentIdx >= 0 ? currentIdx + 1 : Math.min(frames.length, count("passed") + count("failed") + 1);
    const since = run?.startedAt ? Date.parse(run.startedAt) : NaN;
    line = (
      <p role="status" className="tc-run-progress" aria-busy="true">
        <span className="tc-activity" aria-hidden="true" />
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
      {source.history && run !== null && !running && <HistoryStrip history={history} />}
    </div>
  );
  // #548: the same words and the same handler as JourneyFlows' held step.
  const held = run?.state === "ready" && run.held ? (
    <div className="tc-ins-block journey-flow-held" role="group" aria-label="Step waiting for you">
      <p>{`The next step changes data at ${run.held.base}: “${run.held.act}”. Run it?`}</p>
      <button type="button" className="tc-primary" disabled={confirming} onClick={confirm}>{confirming ? "Running…" : "Run it"}</button>
    </div>
  ) : null;
  // #1776: the compare belongs to the step it was opened on; moving to another step closes it.
  const openStep = frames[selected]?.stepId ?? null;
  const shown = comparing && comparing.stepId === openStep && !running ? comparing : null;
  return (
    <TestCanvas frames={frames} purpose={flow?.purpose ?? null} selected={selected} onSelect={(n) => { setPinned(true); onSelect(n); }}
      frameUrl={nowFrame} onMarkSafe={onMarkSafe}
      runLine={runLine} inspectorTop={held} badges={badges}
      compare={shown ? <FrameCompareView key={`${shown.stepId}:${shown.mode}`} beforeUrl={shown.beforeUrl} nowUrl={shown.nowUrl} diffUrl={shown.diffUrl}
        beforeLabel={shown.beforeLabel} initialMode={shown.mode} onBack={() => setComparing(null)} /> : undefined}
      inspectorStep={(stepId) => (history.baseline && !running
        ? <FrameCompare history={history} screenId={screenOf(stepId)} nowUrl={runFrames[screenOf(stepId)] ?? null} nowStatus={frames.find((f) => f.stepId === stepId)?.status ?? "not_run"}
          onOpen={(open) => setComparing({ ...open, stepId })} />
        : null)} />
  );
}
