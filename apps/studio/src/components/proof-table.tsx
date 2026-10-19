import "./proof-table.css";
import { useEffect, useRef, useState } from "react";
import type { Mission, StepStatus } from "../runtime/mission";
import { layoutMission } from "../runtime/mission-layout";
import { STATUS_LABEL } from "./mission-graph";
import { MiniBrowser } from "./issue-proof";
import type { JourneyFlowView, JourneyRunView } from "../runtime/types";
import { approvalBlockReason } from "../runtime/journeys";
import { useJourneyRun, type JourneyRunSource } from "./journey-flows";
import { runFailure as explain } from "../runtime/run-failure";

const HINT: Record<StepStatus, string> = {
  proven: "Proven by a machine. Seen by you closes the ladder.",
  stale: "This replay belongs to an older flow. Run it again to prove the current flow.",
  needs_you: "Only you can close this. Watch it, then mark it safe or send it back.",
  failed: "The replay failed here. Open the test to see what it saw.",
  preview_only: "A draft preview is not proof. Approve the flow to replay it.",
  not_run: "Not replayed yet.",
};

const EV: Record<StepStatus, [string, string, string]> = {
  proven: ["ok", "✓", "PASS"], stale: ["warn", "!", "STALE"], failed: ["no", "✕", "FAIL"], needs_you: ["warn", "!", "SKIPPED"], preview_only: ["warn", "!", "preview only"], not_run: ["wait", "○", "not replayed yet"],
};
const TONE: Record<StepStatus, string> = { proven: "proven", stale: "stalled", failed: "stalled", needs_you: "stalled", preview_only: "work", not_run: "ready" };

/** #1157: a finished run that failed is said, not left reading "not replayed yet". */
function runFailure(run: JourneyRunView, stepIds: string[]): string {
  if (run.reason === "driver.observer_missing" || run.detail?.code === "replay.observer_missing") {
    return "Couldn't run this journey: no browser observer is installed. Run `graphhelm setup --install-observer playwright` in this project, then Run again.";
  }
  const failed = explain(run, stepIds);
  return `Couldn't run this journey. ${failed.text}${failed.log ? ` The ${failed.logFrom === "driver" ? "browser driver" : "launcher"} said: ${failed.log.split("\n").slice(-2).join(" / ")}` : ""}`;
}

interface Props {
  mission: Mission;
  onOpenTest(stepId: string): void;
  /** The recorded frame for a step, if any; absent or null draws the placeholder browser. */
  frameUrl?: (stepId: string) => string | null;
  flow?: JourneyFlowView;
  run?: JourneyRunSource;
  onApprove?: (flowId: string) => Promise<void>;
  /** #1658: the owner's Reject; the note is trimmed and 1 to 4096 UTF-8 bytes. */
  onReject?: (flowId: string, note: string) => Promise<void>;
  onWatch?: (flowId: string) => Promise<void>;
  busy?: boolean;
  onBusy?: (value: boolean) => void;
  error?: string | null;
  onActionError?: (value: string | null) => void;
  onRun?: (run: JourneyRunView) => void;
}

export function ProofTable({ mission, onOpenTest, frameUrl, flow, run, onApprove, onReject, onWatch, busy = false, onBusy, error, onActionError, onRun }: Props) {
  const preview = useJourneyRun(mission.contractId, flow ? run : undefined, false);
  const reported = useRef<JourneyRunView | null>(null);
  useEffect(() => {
    if (preview.run !== null && preview.run.state !== "none" && preview.run !== reported.current) {
      reported.current = preview.run;
      onRun?.(preview.run);
    }
  }, [preview.run, onRun]);
  // #1635: a Run blocks until the run ends; say so at once, here and on the rail.
  const running = preview.launching || preview.run?.state === "running";
  // The rail's running mark is cleared by the run's answer; an answer of "none" is reported too.
  // `onRun` is a fresh closure each render, so it is read through a ref and never re-fires this.
  const onRunRef = useRef(onRun);
  onRunRef.current = onRun;
  const announced = useRef(false);
  useEffect(() => {
    if (preview.launching) { announced.current = true; onRunRef.current?.({ state: "running", kind: "preview" }); return; }
    if (announced.current && (preview.run === null || preview.run.state === "none")) { announced.current = false; onRunRef.current?.(preview.run ?? { state: "none" }); }
  }, [preview.launching, preview.run]);
  const act = (action: () => Promise<void>) => {
    onBusy?.(true);
    onActionError?.(null);
    void action().catch((cause: unknown) => onActionError?.(cause instanceof Error ? cause.message : String(cause))).finally(() => onBusy?.(false));
  };
  const placed = layoutMission(mission).placed;
  const draft = mission.title.match(/^(.*?)\s*\(draft:\s*(.*?)\)\s*$/i);
  const title = draft?.[1] ?? mission.title;
  const draftNote = draft?.[2];
  const approvalReason = flow ? approvalBlockReason(flow, mission, preview.run) : null;
  const [rejecting, setRejecting] = useState(false);
  const [note, setNote] = useState("");
  const noteBytes = new TextEncoder().encode(note.trim()).length;
  return (
    <section className="proof" aria-label="Proof">
      <div className="proof-head">
        <div className="proof-title">
          <span className="proof-cap">{`Journey · proof · ${mission.summary.proven}/${mission.summary.total} proven`}</span>
          <h2>{`Can I trust “${title}”?`}</h2>
          {draftNote && <span className="proof-sub proof-draft-note">{`Draft: ${draftNote}`}</span>}
          <span className="proof-sub">Every step is a promise to a person. Each row shows who built it, what checked it, and the frame the replay saw.</span>
        </div>
      </div>
      {flow && <div className="proof-flow-actions" role="group" aria-label="Journey actions">
        <span>{flow.status === "draft" ? "Waiting for your approval" : flow.status === "approval_stale" ? "Changed since approval" : flow.status === "weak" ? "Does not say what it proves" : flow.status === "approved" ? "Approved" : "Can't be read"}</span>
        {flow.status !== "approved" && <><button type="button" disabled={!flow.approvable || approvalReason !== null || busy || !onApprove}
          aria-describedby={approvalReason ? "proof-approval-reason" : undefined} onClick={() => onApprove && act(() => onApprove(flow.id))}>Approve</button>
          {approvalReason && <span id="proof-approval-reason">{approvalReason}</span>}
          {onReject && <button type="button" disabled={busy || rejecting} onClick={() => setRejecting(true)}>Reject</button>}</>}
        {onWatch && <button type="button" disabled={busy} onClick={() => act(() => onWatch(flow.id))}>Watch</button>}
        {run && <button type="button" disabled={running} onClick={preview.again}>Run again</button>}
        {preview.run?.state === "ready" && <span>{preview.run.kind === "preview" ? "Preview only — not proof" : "Replay recorded"}</span>}
        {running && <span role="status">Running…</span>}
        {flow.rejected && <p className="proof-rejected" role="note" aria-label={`Rejected: ${flow.rejected.note}`}>{`Rejected: ${flow.rejected.note}`}</p>}
        {rejecting && onReject && <div className="proof-reject">
          <textarea aria-label="Why reject" value={note} maxLength={4096} onChange={(event) => setNote(event.target.value)} />
          <button type="button" disabled={busy || noteBytes === 0 || noteBytes > 4096}
            onClick={() => act(() => onReject(flow.id, note.trim()).then(() => { setRejecting(false); setNote(""); }))}>Send reject</button>
          <button type="button" onClick={() => { setRejecting(false); setNote(""); }}>Cancel</button>
        </div>}
        {(error || preview.failure || preview.busyReason) && <p role="alert">{error ?? preview.failure ?? preview.busyReason}</p>}
        {!error && !preview.failure && !preview.busyReason && preview.run?.state === "failed" && <p role="alert">{runFailure(preview.run, mission.steps.map((s) => s.stepId))}</p>}
      </div>}
      <div className="proof-summary">
        <section className="proof-promise-list" aria-labelledby="proof-promise-heading">
          <h3 id="proof-promise-heading">Promise</h3>
          {mission.steps.map((step) => <p key={step.stepId}>{step.promise ?? step.title}</p>)}
        </section>
        <section className="proof-observer" aria-labelledby="proof-observer-heading">
          <h3 id="proof-observer-heading">Observer and evidence</h3>
          <p>Observer identity unavailable from this journey response</p>
          {preview.run?.state === "ready" && preview.run.ranAt ? (
            <p>{`${preview.run.kind === "replay" ? "Replay" : "Preview"} recorded ${new Date(preview.run.ranAt).toLocaleString()} (${preview.run.result ?? "result unavailable"})`}</p>
          ) : <p>No evidence yet</p>}
        </section>
      </div>
      <div className="proof-cols" aria-hidden="true"><span>Step</span><span>Promise</span><span>Replay saw</span><span>Chain of custody · evidence</span><span>Your call</span></div>
      <ol className="proof-rows" aria-label="Steps">
        {mission.steps.map((s) => {
          const src = frameUrl?.(s.stepId) ?? null;
          const tasks = placed.filter((p) => p.col === s.index).map((p) => p.task);
          return (
            <li key={s.stepId} className="proof-row" data-status={s.status}>
              <div className="proof-n"><span>{s.index + 1}</span><span className="proof-dot" data-status={s.status} /></div>
              <div className="proof-promise">
                <span className="proof-name" title={s.title}>{s.title}</span>
                {s.promise && <span className="proof-sub proof-clamp" title={s.promise}>{s.promise}</span>}
                <span className="proof-badge" data-status={s.status} title={s.reason ?? undefined}>{STATUS_LABEL[s.status]}</span>
              </div>
              <button type="button" className="proof-frame" data-status={s.status} aria-label={`Open the test canvas for step ${s.index + 1}`} onClick={() => onOpenTest(s.stepId)}>
                {src ? <img src={src} alt={`Replay frame of step ${s.index + 1}`} /> : <MiniBrowser tone={TONE[s.status]} />}
                <span className="proof-frame-txt"><span>{src ? "frame recorded" : s.status === "not_run" ? "not replayed yet" : "no frame recorded"}</span><span className="proof-frame-go">open test →</span></span>
              </button>
              <div className="proof-chain"><div className="proof-chiprow">
                {tasks.length === 0 ? <span className="proof-sub">No work linked to this step</span> : tasks.map((t) => (
                  <span key={t.key} className="proof-chips">
                    <span className="proof-chip" data-tone={t.step === "implement" ? "run" : "ok"}><span className="proof-stage">impl</span>{` ${t.lane ?? "—"}`}</span>
                    {t.reviewers.length > 0 && <span className="proof-chip" data-tone={t.blocked ? "no" : t.step === "review" ? "run" : "ok"}><span className="proof-stage">rev</span>{` ${t.reviewers.join(", ")}${t.blocked ? " BLOCK" : ""}`}</span>}
                    {t.step === "merged" && <span className="proof-chip" data-tone="ok"><span className="proof-stage">merge</span>{` ${t.mergeSha?.slice(0, 8) ?? (t.pr ? `#${t.pr}` : "—")}`}</span>}
                  </span>
                ))}
              </div>
                {s.reason && <div className="proof-ev"><span className="proof-ev-mark" data-mark="warn">!</span><span className="proof-ev-txt" title={s.reason}>{s.reason}</span></div>}
                <div className="proof-ev"><span className="proof-ev-mark" data-mark={EV[s.status][0]}>{EV[s.status][1]}</span><span className="proof-ev-txt">{`journey replay · step ${s.index + 1} ${EV[s.status][2]}`}</span></div>
              </div>
              <div className="proof-call">
                <button type="button" className={s.status === "needs_you" ? "proof-primary" : "proof-secondary"} aria-label={`Open test for step ${s.index + 1}`} onClick={() => onOpenTest(s.stepId)}>
                  {s.status === "needs_you" ? "Watch it, mark safe" : "Open test"}
                </button>
                <span className="proof-hint">{HINT[s.status]}</span>
              </div>
            </li>
          );
        })}
      </ol>
    </section>
  );
}
