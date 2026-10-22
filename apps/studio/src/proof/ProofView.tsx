import { useState } from "react";

import type { JourneyFlow, JourneyRun, Runtime } from "../runtime/client";
import { earlierRun } from "./compare";
import { CompareFrame } from "./CompareFrame";
import { decisionOf, offersDecision, rejectNote } from "./decision";
import { journeySteps, lastRunChip, preparingStatus, type Step, type Viewport } from "./steps";
import { useJourneyRun } from "./use-journey-run";

interface ProofViewProps {
  runtime: Runtime;
  journeys: JourneyFlow[] | null;
  error: string | null;
  /** The team run that is open: an approved journey's replay records its proof there. */
  executionId: string | null;
  /** A run finished: the journeys' last results are read again. */
  onRan: () => void;
  /** The journey a `?journey=` link opens on; `null` = the first listed. */
  initial?: string | null;
}

/** The Proof region (docs/design/studio/Proof.dc.html): the journeys column, then the selected
 * journey's run bar, console and step strip with the frames its run captured. */
export function ProofView({ runtime, journeys, error, executionId, onRan, initial = null }: ProofViewProps) {
  // The journey a deep link named (`studio open --journey`), until the owner picks another.
  const [picked, setPicked] = useState<string | null>(initial);
  if (error !== null) return <section id="panel-proof" aria-label="Proof" className="view"><p className="error">{error}</p></section>;
  if (journeys === null) return <section id="panel-proof" aria-label="Proof" className="view"><p>Reading the journeys…</p></section>;
  const selected = journeys.find((flow) => flow.id === picked) ?? journeys[0] ?? null;
  return (
    <section id="panel-proof" aria-label="Proof" className="proof">
      <JourneysColumn journeys={journeys} selected={selected?.id ?? null} onPick={setPicked} />
      {selected === null
        ? <div className="proof-main"><p>This project has no journeys yet.</p></div>
        : <JourneyRunPanel key={selected.id} runtime={runtime} flow={selected} executionId={executionId} onRan={onRan} />}
    </section>
  );
}

function JourneysColumn({ journeys, selected, onPick }: { journeys: JourneyFlow[]; selected: string | null; onPick: (id: string) => void }) {
  const counts = { passed: 0, failed: 0, not_run: 0 };
  for (const flow of journeys) if (flow.lastRun?.kind === "replay" || flow.lastRun?.status === "not_run") counts[flow.lastRun.status] += 1;
  const groups = new Map<string, JourneyFlow[]>();
  for (const flow of journeys) {
    const feature = flow.feature || "Other";
    groups.set(feature, [...(groups.get(feature) ?? []), flow]);
  }
  return (
    <aside className="proof-journeys">
      <span className="section-label">JOURNEYS · {journeys.length}</span>
      {journeys.some((flow) => flow.lastRun !== undefined) && (
        <span className="proof-counts">{counts.passed} passed · {counts.failed} failed · {counts.not_run} not run</span>
      )}
      <ul className="journey-list" aria-label="Journeys">
        {[...groups].map(([feature, flows]) => (
          <li key={feature} className="journey-group">
            <strong className="journey-feature">{feature}</strong>
            <ul>
              {flows.map((flow) => {
                const chip = lastRunChip(flow.lastRun);
                return (
                  <li key={flow.id}>
                    <button type="button" className="b journey" aria-pressed={flow.id === selected} onClick={() => onPick(flow.id)}>
                      <strong>{flow.name || flow.title || flow.id}</strong>
                      {flow.purpose && <span className="journey-purpose">{flow.purpose}</span>}
                      <span className={`chip ${chip.tone}`}>{chip.text}</span>
                      {decisionOf(flow) !== "undecided" && <span className={`chip ${decisionOf(flow) === "approved" ? "pass" : "fail"}`}>{decisionOf(flow)}</span>}
                    </button>
                  </li>
                );
              })}
            </ul>
          </li>
        ))}
      </ul>
    </aside>
  );
}

const MARK: Record<Step["status"], string> = { passed: "✓", failed: "✕", drift: "≈", skipped: "–", running: "…", "not run": "○" };

function runStatus(run: JourneyRun | null, starting: boolean): { name: string; text: string } | null {
  if (starting) return { name: "Starting the run", text: "Starting the run" };
  if (run?.state !== "running") return null;
  if (run.observerSetup?.state === "installing") return { name: "Preparing the browser", text: preparingStatus(run.observerSetup.message) };
  return { name: "Running", text: run.current ? `Running · ${run.current}` : "Running" };
}

function verdict(run: JourneyRun | null): string | null {
  if (run === null || run.state === "none") return "Not run yet";
  if (run.state === "running") return null;
  if (run.state === "failed") return run.reason ? `The run could not finish: ${run.reason}` : "The run could not finish";
  const proof = run.kind === "replay" ? "" : " · Preview — not proof";
  return `${run.result === "pass" ? "Passed" : run.result === "drift" ? "Drifted" : "Failed"}${run.ranAt ? ` · ${run.ranAt}` : ""}${proof}`;
}

function viewportLine(step: Step) {
  const mark = (viewport: Viewport) => (step.viewport === viewport ? MARK[step.status] : step.viewport === null && step.status !== "not run" ? "?" : "not run");
  return <span className="step-viewports">web <b>{mark("web")}</b> · mobile <b>{mark("mobile")}</b></span>;
}

function JourneyRunPanel({ runtime, flow, executionId, onRan }: { runtime: Runtime; flow: JourneyFlow; executionId: string | null; onRan: () => void }) {
  const { run, frames, log, history, error, starting, start } = useJourneyRun(runtime, flow.id, executionId, onRan);
  const [showLog, setShowLog] = useState(false);
  const [picked, setPicked] = useState<number | null>(null);
  const steps = journeySteps(flow, run);
  const busy = starting || run?.state === "running";
  const status = runStatus(run, starting);
  const said = verdict(run);
  const selected = steps.find((step) => step.n === picked) ?? steps.find((step) => step.status === "failed") ?? steps[0] ?? null;
  const frame = selected === null ? undefined : frames[selected.screen];
  return (
    <div className="proof-main">
      <h1>{flow.name || flow.title || flow.id}</h1>
      {flow.purpose && <p><span className="section-label">PROVES</span> {flow.purpose}</p>}
      <div className="run-bar">
        <button type="button" className="b act" disabled={busy} onClick={() => { setPicked(null); start(); }}>
          {busy ? "Running…" : run === null || run.state === "none" ? "Run" : "Run again"}
        </button>
        {status !== null && <span role="status" aria-label={status.name} className="run-status"><span className="pulse" aria-hidden="true" />{status.text}</span>}
        {said !== null && <span className="run-verdict">{said}</span>}
        {(busy || log.length > 0) && (
          <button type="button" className="b ghost" aria-expanded={showLog} onClick={() => setShowLog(!showLog)}>{showLog ? "Hide log" : "Show log"}</button>
        )}
        {history.length > 0 && (
          <span className="run-history" aria-label="History">
            <span className="section-label">HISTORY</span>
            {history.map((past) => <span key={past.run} className="mono">{past.result === "pass" ? "✓" : "✕"} {past.ranAt ?? past.run}</span>)}
          </span>
        )}
      </div>
      {error !== null && <p className="error">{error}</p>}
      <DecisionBar runtime={runtime} flow={flow} runFinished={run?.state === "ready"} onDecided={onRan} />
      {showLog && (
        <pre role="log" aria-label="Run console" className="run-log">{log.length === 0 ? "Waiting for the Runtime's first line…" : log.join("\n")}</pre>
      )}
      <ol className="steps" aria-label="Steps">
        {steps.map((step) => (
          <li key={step.n} aria-label={`Step ${step.n}`} className={`step ${step.status.replace(" ", "-")}`}>
            <button type="button" className="b" aria-pressed={selected?.n === step.n} onClick={() => setPicked(step.n)}>
              <span className="step-thumb">{frames[step.screen] ? <img src={frames[step.screen]} alt={`Step ${step.n} frame`} /> : <span>{step.frame ? "loading" : "no frame"}</span>}</span>
              <span className="step-head"><span className="mono">{step.n} DOES</span><span className="step-status">{MARK[step.status]} {step.status}</span></span>
              <span className="step-label">{step.label}</span>
              {viewportLine(step)}
            </button>
          </li>
        ))}
      </ol>
      {selected !== null && (
        <figure className="step-frame">
          <CompareFrame runtime={runtime} flowId={flow.id} step={selected} steps={steps.length} now={frame} earlier={earlierRun(history, run, selected.screen)} />
          <figcaption><span className="mono">ACTION {selected.n} · DOES</span> {selected.label}</figcaption>
        </figure>
      )}
    </div>
  );
}

/** The owner's Approve and Reject on a run journey. Each act goes to its public route with the
 * session's owner token; the journeys are read again after it, so the status shown is the
 * Runtime's, and a refusal is shown as its text. */
function DecisionBar({ runtime, flow, runFinished, onDecided }: { runtime: Runtime; flow: JourneyFlow; runFinished: boolean; onDecided: () => void }) {
  const [reason, setReason] = useState("");
  const [busy, setBusy] = useState(false);
  const [refusal, setRefusal] = useState<string | null>(null);
  const decision = decisionOf(flow);
  const act = (send: () => Promise<void>) => {
    setBusy(true);
    setRefusal(null);
    send().then(() => { setReason(""); onDecided(); }, (cause: unknown) => setRefusal(cause instanceof Error ? cause.message : "The Runtime refused."))
      .finally(() => setBusy(false));
  };
  const reject = () => {
    const checked = rejectNote(reason);
    if ("refusal" in checked) { setRefusal(checked.refusal); return; }
    act(() => runtime.rejectJourney(flow.id, checked.note));
  };
  return (
    <div className="decision-bar">
      {decision === "approved" && <span role="status" aria-label="Approved" className="chip pass">Approved</span>}
      {decision === "rejected" && <span role="status" aria-label="Rejected" className="chip fail">Rejected{flow.rejected?.note ? ` · ${flow.rejected.note}` : ""}</span>}
      {offersDecision(flow, runFinished) && (
        <>
          <button type="button" className="b act" disabled={busy} onClick={() => act(() => runtime.approveJourney(flow.id))}>Approve</button>
          <input aria-label="Reason for rejecting" placeholder="Why reject?" value={reason} disabled={busy} onChange={(event) => setReason(event.target.value)} />
          <button type="button" className="b ghost" disabled={busy} onClick={reject}>Reject</button>
        </>
      )}
      {refusal !== null && <p className="error">{refusal}</p>}
    </div>
  );
}
