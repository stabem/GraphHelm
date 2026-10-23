import { useEffect, useRef, useState } from "react";

import type { JourneyFlow, JourneyRun, Runtime } from "../runtime/client";
import { earlierRun } from "./compare";
import { CompareFrame } from "./CompareFrame";
import { decisionOf, offersDecision, rejectNote } from "./decision";
import { fixAsk, fixBody, fixRequestId, fixTitle, NOTE_MAX, offersFix } from "./send-fix";
import { askedWhen } from "../lanes/asked";
import { OWNER } from "../runtime/client";
import { RUN_ALL_TIMEOUT_MS, runAllUntil } from "./run-all";
import { countSteps, failsOnlyOn, isHeld, journeyKind, journeyScore, journeySteps, lastRunChip, liveCard, pastAsRun, CHANGED, withoutLive, afterPanelGone, watchedIds, liveAfterList, preparingStatus, STATUS_TEXT, type JourneyKind, type Step, type Viewport } from "./steps";
import { useJourneyRun } from "./use-journey-run";
import { Empty, Loading, ReadFailure } from "../states/ViewState";
import { When } from "../time/Stamp";
import { when } from "../time/when";

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


/** #1950: how often a running journey no panel follows is read, and how many failed reads in a
 * row hand its card back to the journey list. */
const WATCH_POLL_MS = 2_000;
const WATCH_MAX_FAILURES = 5;

function message(cause: unknown, fallback: string): string {
  return cause instanceof Error ? cause.message : fallback;
}

function nameOf(flow: JourneyFlow): string {
  return flow.name || flow.title || flow.id;
}

/** The Proof region (docs/design/studio/Proof.dc.html): the journeys column, then the selected
 * journey's run bar, console, step grid and the selected step's compare and Action panel. */
export function ProofView({ runtime, journeys, error, executionId, onRan, initial = null }: ProofViewProps) {
  // The journey a deep link named (`studio open --journey`), until the owner picks another.
  const [picked, setPicked] = useState<string | null>(initial);
  // #1905: bumped after a run is started from the column, so the open panel reads the run again.
  const [reread, setReread] = useState(0);
  // #1914: the run the open panel follows, by journey, so its card reads it before the list is read again.
  const [live, setLive] = useState<Record<string, JourneyRun>>({});
  // A list read again carries the result; a run still playing stays until its next answer.
  // #1953: a journey the list says is running is seeded, so a reload still shows it running.
  useEffect(() => { setLive((before) => liveAfterList(before, journeys)); }, [journeys]);
  const selectedId = journeys === null ? null : (journeys.find((flow) => flow.id === picked) ?? journeys[0] ?? null)?.id ?? null;
  // #1950: a running journey that no open panel follows is read from the Runtime here, so its card
  // keeps reading running after the owner selects another journey, and shows the result at its end.
  const watched = watchedIds(live, selectedId).join(",");
  const ran = useRef(onRan);
  ran.current = onRan;
  useEffect(() => {
    if (watched === "") return;
    let on = true;
    const failures = new Map<string, number>();
    const timer = setInterval(() => {
      for (const id of watched.split(",")) {
        runtime.journeyRun(id).then((run) => {
          if (!on) return;
          failures.delete(id);
          setLive((before) => (before[id]?.state === "running" ? { ...before, [id]: run } : before));
          if (run.state !== "running") ran.current();
        }, () => {
          if (!on) return;
          const n = (failures.get(id) ?? 0) + 1;
          failures.set(id, n);
          // #1915: an unreadable run never leaves a card on running for good; it reads the list.
          if (n >= WATCH_MAX_FAILURES) setLive((before) => withoutLive(before, id));
        });
      }
    }, WATCH_POLL_MS);
    return () => { on = false; clearInterval(timer); };
  }, [watched, runtime]);
  if (error !== null && journeys === null) return <section id="panel-proof" aria-label="Proof" className="view"><ReadFailure what="journeys" route="/v1/journey-flows" message={error} onRetry={onRan} /></section>;
  if (journeys === null) return <section id="panel-proof" aria-label="Proof" className="view"><Loading text="Reading the journeys…" /></section>;
  const selected = journeys.find((flow) => flow.id === picked) ?? journeys[0] ?? null;
  const started = (id: string) => {
    // #1950: a run started here is known running until the Runtime says otherwise.
    setLive((before) => ({ ...before, [id]: { state: "running" } }));
    if (id === selected?.id) setReread((n) => n + 1);
  };
  return (
    <section id="panel-proof" aria-label="Proof" className="proof">
      <JourneysColumn runtime={runtime} journeys={journeys} selected={selected?.id ?? null} executionId={executionId} onPick={setPicked} onStarted={started} onRan={onRan} live={live} />
      {selected === null
        ? <div className="proof-main"><Empty title="No journeys yet" hint="This project has no journeys yet. A journey appears here once one is drafted for a request." /></div>
        : <JourneyRunPanel key={`${selected.id}:${reread}`} runtime={runtime} flow={selected} executionId={executionId} onRan={onRan} onRun={(run) => setLive((before) => ({ ...before, [selected.id]: run }))} onGone={() => setLive((before) => afterPanelGone(before, selected.id))} />}
    </section>
  );
}

interface ColumnProps {
  runtime: Runtime;
  journeys: JourneyFlow[];
  selected: string | null;
  executionId: string | null;
  onPick: (id: string) => void;
  onStarted: (id: string) => void;
  onRan: () => void;
  live: Record<string, JourneyRun>;
}

function JourneysColumn({ runtime, journeys, selected, executionId, onPick, onStarted, onRan, live }: ColumnProps) {
  const [busy, setBusy] = useState<string | null>(null);
  // #1914: the journey Run all is playing now; its card reads running even when no panel follows it.
  const [active, setActive] = useState<string | null>(null);
  const [refusal, setRefusal] = useState<string | null>(null);
  const counts = { passed: 0, failed: 0, stale: 0, not_run: 0 };
  for (const flow of journeys) {
    const last = flow.lastRun;
    if (last === undefined) continue;
    if (last.status === "not_run") counts.not_run += 1;
    else if (last.flowChanged === true) counts.stale += 1;
    else if (last.kind === "replay") counts[last.status] += 1;
  }
  const groups = new Map<string, JourneyFlow[]>();
  for (const flow of journeys) {
    const feature = flow.feature || "Other";
    groups.set(feature, [...(groups.get(feature) ?? []), flow]);
  }
  // #1905: Approve N is the single Approve, one journey at a time, on every journey the Runtime says
  // it would accept now. The Runtime decides who may approve (owner credential only).
  const approvable = journeys.filter((flow) => flow.approvable === true && decisionOf(flow) !== "approved");

  const runOne = async (flow: JourneyFlow): Promise<void> => {
    await runtime.startJourneyRun(flow.id, executionId);
    onStarted(flow.id);
  };
  const runSingle = (flow: JourneyFlow) => {
    setRefusal(null);
    setBusy(`Starting ${nameOf(flow)}`);
    onPick(flow.id);
    runOne(flow).catch((cause: unknown) => setRefusal(message(cause, "The Runtime refused the run."))).finally(() => setBusy(null));
  };
  const runAll = async () => {
    setRefusal(null);
    try {
      for (const [index, flow] of journeys.entries()) {
        setBusy(`Running all · ${index + 1}/${journeys.length}`);
        setActive(flow.id);
        await runOne(flow);
        if (await runAllUntil(() => runtime.journeyRun(flow.id)) === "timeout") {
          setRefusal(`Run all stopped: ${nameOf(flow)} was still running after ${RUN_ALL_TIMEOUT_MS / 60_000} minutes.`);
          return;
        }
        // #1914: each card shows its result as its run ends, not when the whole batch has.
        setActive(null);
        onRan();
      }
    } catch (cause) {
      setRefusal(message(cause, "The Runtime refused the run."));
    } finally {
      setActive(null);
      setBusy(null);
      onRan();
    }
  };
  const approveAll = async () => {
    setRefusal(null);
    try {
      for (const [index, flow] of approvable.entries()) {
        setBusy(`Approving ${index + 1}/${approvable.length}`);
        await runtime.approveJourney(flow.id);
      }
    } catch (cause) {
      setRefusal(message(cause, "The Runtime refused."));
    } finally {
      setBusy(null);
      onRan();
    }
  };

  return (
    <aside className="proof-journeys" aria-label="Journeys column">
      <span className="section-label">JOURNEYS · {journeys.length}</span>
      <div className="journeys-actions">
        <button type="button" className="b ghost" disabled={busy !== null || approvable.length === 0} onClick={() => { void approveAll(); }}>Approve {approvable.length}</button>
        <button type="button" className="b ghost" disabled={busy !== null || journeys.length === 0} onClick={() => { void runAll(); }}>▶ Run all</button>
      </div>
      {busy !== null && <span role="status" className="run-status"><span className="pulse" aria-hidden="true" />{busy}</span>}
      {refusal !== null && <p className="error">{refusal}</p>}
      {journeys.some((flow) => flow.lastRun !== undefined) && (
        <span className="proof-counts">{counts.passed} passed · {counts.failed} failed · {counts.stale} stale · {counts.not_run} not run</span>
      )}
      <ul className="journey-list" aria-label="Journeys">
        {[...groups].map(([feature, flows]) => {
          const passed = flows.filter((flow) => flow.lastRun?.status === "passed").length;
          const failed = flows.some((flow) => flow.lastRun?.status === "failed");
          return (
            <li key={feature} className="journey-group">
              <div className="journey-feature">
                <span aria-hidden="true">▾</span><strong>{feature}</strong>
                <span className={`mono group-score${failed ? " fail" : passed > 0 ? " pass" : ""}`} aria-label={`${feature}: ${passed} of ${flows.length} passed`}>{passed}/{flows.length}</span>
              </div>
              <ul>
                {flows.map((flow) => <JourneyCard key={flow.id} flow={flow} selected={flow.id === selected} disabled={busy !== null} onPick={onPick} onRun={runSingle} run={live[flow.id] ?? null} playing={active === flow.id} />)}
              </ul>
            </li>
          );
        })}
      </ul>
    </aside>
  );
}

function JourneyCard({ flow, selected, disabled, onPick, onRun, run, playing }: { flow: JourneyFlow; selected: boolean; disabled: boolean; onPick: (id: string) => void; onRun: (flow: JourneyFlow) => void; run: JourneyRun | null; playing: boolean }) {
  // #1914: the run the open panel follows beats the list's last result until the list is read again.
  const now = liveCard(flow, run) ?? (playing ? liveCard(flow, { state: "running" }) : null);
  // #1919: a playing run's chip blinks blue (static under reduced motion), as in the header.
  const chip: { text: string; tone: string } = now !== null ? { text: now.text, tone: now.tone === "wait" ? "running" : now.tone } : lastRunChip(flow.lastRun);
  const base = journeyScore(flow);
  const score = now !== null ? { passed: now.passed, total: now.total, bar: now.bar } : base;
  const decision = decisionOf(flow);
  return (
    <li className={`journey${selected ? " on" : ""}`} aria-label={nameOf(flow)}>
      <button type="button" className="b journey-pick" aria-label={`Select ${nameOf(flow)}`} aria-pressed={selected} onClick={() => onPick(flow.id)}>
        <span className="journey-top"><strong>{nameOf(flow)}</strong><KindBadge flow={flow} card /></span>
        {flow.purpose && <span className="journey-purpose">{flow.purpose}</span>}
      </button>
      <div className="journey-row">
        <span className={`chip ${chip.tone}`}>{chip.text}</span>
        {decision !== "undecided" && <span className={`chip ${decision === "approved" ? "pass" : "fail"}`}>{decision}</span>}
        <span className="mono journey-score">{score.passed === null ? "?" : score.passed}/{score.total}</span>
        <button type="button" className="b ghost journey-run" aria-label={`Run ${nameOf(flow)}`} disabled={disabled} onClick={() => onRun(flow)}>▶ Run</button>
      </div>
      {score.total > 0 && (
        <span className="journey-bar" aria-hidden="true">{score.bar.map((cell, index) => <span key={index} className={cell} />)}</span>
      )}
    </li>
  );
}

const MARK: Record<Step["status"], string> = { passed: "✓", failed: "✕", drift: "≈", skipped: "–", running: "…", waiting: "⏸", precondition: "·", "not run": "○" };

function runStatus(run: JourneyRun | null, starting: boolean): { name: string; text: string } | null {
  if (starting) return { name: "Starting the run", text: "Starting the run" };
  if (run?.state !== "running") return null;
  if (run.observerSetup?.state === "installing") return { name: "Preparing the browser", text: preparingStatus(run.observerSetup.message) };
  // #1808: on a host whose observer is already installed the "installing" phase lasts less than
  // one poll, so the run bar keeps saying the browser is being prepared until a first screen is
  // reached: until then the observer is checked, Chromium launched and the app opened.
  if (Object.keys(run.screens ?? {}).length === 0) return { name: "Preparing the browser", text: "Preparing the browser" };
  return { name: "Running", text: run.current ? `Running · ${run.current}` : "Running" };
}

/** #1912: said in the console when a finished run kept no line for it. */
const NO_LOG = "This run kept no log lines: the Runtime keeps the browser's and the app's tail only when one of them failed, and a command run's output shows on each step.";

/** #1914: a command step's thumbnail is a dark terminal tile: the ">_" glyph, the command, and the
 * exit chip (green for 0, red otherwise). */
function CliThumb({ step }: { step: Step }) {
  const last = step.outputs[step.outputs.length - 1];
  const command = last?.argv ?? step.label;
  return (
    <span className="cli-thumb">
      <TerminalIcon />
      <span className="mono cli-thumb-cmd">$ {command}</span>
      {last !== undefined && <span className={`mono cli-thumb-exit ${last.exit === 0 ? "pass" : "fail"}`}>exit {last.exit ?? "?"}</span>}
    </span>
  );
}

function TerminalIcon() {
  return (
    <svg className="kind-icon" width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M3 4.5 7 8l-4 3.5" /><path d="M9 12h4" />
    </svg>
  );
}

function BrowserIcon() {
  return (
    <svg className="kind-icon" width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <rect x="1.5" y="2.5" width="13" height="11" rx="1.5" /><path d="M1.5 5.5h13" /><circle cx="3.6" cy="4" r=".4" />
    </svg>
  );
}

const KIND_TEXT: Record<JourneyKind, string> = { terminal: "Terminal", browser: "Browser", both: "Terminal and Browser" };

/** #1914: what the journey drives, as icons: the terminal glyph, the browser window, or both. */
function KindBadge({ flow, card = false }: { flow: JourneyFlow; card?: boolean }) {
  const kind = journeyKind(flow);
  // The header's badge is "<Kind> journey"; a card's names its journey so the two never share a name.
  return (
    <span role="img" aria-label={card ? `${nameOf(flow)}: ${KIND_TEXT[kind]} journey` : `${KIND_TEXT[kind]} journey`} className={`kind-badge ${kind}`}>
      {kind !== "browser" && <TerminalIcon />}
      {kind !== "terminal" && <BrowserIcon />}
      <span aria-hidden="true">{KIND_TEXT[kind]}</span>
    </span>
  );
}

function verdict(run: JourneyRun | null): string | null {
  if (run === null || run.state === "none") return "Not run yet";
  if (run.state === "running") return null;
  if (isHeld(run)) return "Waits for your confirmation";
  if (run.state === "failed") return run.reason ? `The run could not finish: ${run.reason}` : "The run could not finish";
  return `${run.result === "pass" ? "Passed" : run.result === "drift" ? "Drifted" : "Failed"}${run.ranAt ? ` · ${when(run.ranAt).text}` : ""}`;
}

/** #1777: each viewport's mark on a step card, and the line that says a break is mobile-only. */
function viewportLine(step: Step) {
  const mark = (viewport: Viewport) => (step.viewports[viewport] === "not run" ? "not run" : MARK[step.viewports[viewport]]);
  const tone = (viewport: Viewport) => step.viewports[viewport].replace(" ", "-");
  const only = failsOnlyOn(step);
  return (
    <>
      <span className="step-viewports">web <b className={tone("web")}>{mark("web")}</b> · mobile <b className={tone("mobile")}>{mark("mobile")}</b></span>
      {only !== null && <span className="fails-only">Fails only on {only}</span>}
    </>
  );
}

function JourneyRunPanel({ runtime, flow, executionId, onRan, onRun, onGone }: { runtime: Runtime; flow: JourneyFlow; executionId: string | null; onRan: () => void; onRun: (run: JourneyRun) => void; onGone: () => void }) {
  const { run, frames, mobileFrames, log, history, earlier, error, starting, start } = useJourneyRun(runtime, flow.id, executionId, onRan, onRun);
  const [showLog, setShowLog] = useState(false);
  // #1914: a panel that goes away (another journey picked) stops reporting; its card must not stay "running".
  useEffect(() => onGone, []); // eslint-disable-line react-hooks/exhaustive-deps
  const [picked, setPicked] = useState<number | null>(null);
  // #1956: with no run of the current version, the earlier version's run is shown, marked stale.
  const stale = earlier !== null && !starting;
  const steps = journeySteps(flow, stale ? pastAsRun(earlier) : run);
  const counts = countSteps(steps);
  const busy = starting || run?.state === "running";
  const status = runStatus(run, starting);
  const said = stale ? CHANGED : verdict(run);
  const selected = steps.find((step) => step.n === picked) ?? steps.find((step) => step.status === "failed")
    // #1912: a command flow opens on its first command, not on the start screen it never observes.
    ?? steps.find((step) => step.status !== "precondition") ?? steps[0] ?? null;
  const frame = selected === null ? undefined : frames[selected.screen];
  // #1878: a mobile-only run has no web frame; its step thumbnail is the mobile one.
  const thumb = (screen: string): string | undefined => frames[screen] ?? mobileFrames[screen];
  const finished = run?.state === "ready";
  return (
    <div className="proof-main">
      <section aria-label="Journey run" className="journey-run">
        <div className="run-head">
          <h1>{nameOf(flow)}</h1>
          <KindBadge flow={flow} />
          {busy && <span className="chip running" aria-label="Journey running">● running</span>}
          <span className="mono run-counts" aria-label="Step counts">
            <span className="pass">{counts.passed} passed</span> <span className="wait">{counts.waits} waits for you</span> <span className="fail">{counts.failed} failed</span> <span className="idle">{counts.notRun} not run</span>
          </span>
        </div>
        {flow.purpose && <p><span className="section-label">PROVES</span> {flow.purpose}</p>}
        <div className="run-bar">
          {finished && <span className="run-score">{counts.passed} passed · {counts.failed} failed</span>}
          {finished && run?.kind !== "replay" && <span className="preview-tag">Preview — not proof</span>}
          <button type="button" className="b act" disabled={busy} onClick={() => { setPicked(null); start(); }}>
            {busy ? "Running…" : run === null || run.state === "none" ? "Run" : "Run again"}
          </button>
          {busy && (
            // #1905: the Runtime serves no route that cancels a running journey (only GET and POST
            // /v1/journey-flows/{id}/preview), so Stop is shown and says why it cannot act.
            <button type="button" className="b ghost" disabled aria-describedby="proof-stop-note">Stop</button>
          )}
          {busy && <span id="proof-stop-note" className="run-note">The Runtime has no route to stop a run.</span>}
          {run !== null && offersFix(run) && <SendToFix key={fixRequestId(flow.id, run)} runtime={runtime} flow={flow} run={run} steps={steps} executionId={executionId} />}
          {status !== null && <span role="status" aria-label={status.name} className="run-status"><span className="pulse" aria-hidden="true" />{status.text}</span>}
          {said !== null && <span role="status" aria-label={said.split(" · ")[0]} className="run-verdict" title={when(run?.ranAt).full || undefined}>{said}</span>}
          {stale && <span className="run-note stale-note">Results and frames from the earlier version ({earlier.ranAt ? when(earlier.ranAt).text : earlier.run}). Run to prove the current version.</span>}
          {(busy || log.length > 0) && (
            <button type="button" className="b ghost" aria-expanded={showLog} onClick={() => setShowLog(!showLog)}>{showLog ? "Hide log" : "Show log"}</button>
          )}
        </div>
        {showLog && (
          <pre role="log" aria-label="Run console" className="run-log">{log.length > 0 ? log.join("\n") : busy ? "Waiting for the Runtime's first line…" : NO_LOG}</pre>
        )}
        {history.length > 0 && (
          <span className="run-history" aria-label="History">
            <span className="section-label">HISTORY</span>
            {history.map((past, index) => (
              <span key={past.run} className={`history-chip${index === 0 ? " newest" : ""}`}><b className={past.result === "pass" ? "pass" : "fail"}>{past.result === "pass" ? "✓" : "✕"}</b> {past.ranAt ? <When at={past.ranAt} /> : past.run}</span>
            ))}
          </span>
        )}
        {steps.length > 0 && (
          <span className="run-segments" aria-hidden="true">{steps.map((step) => <span key={step.n} className={step.status.replace(" ", "-")} />)}</span>
        )}
      </section>
      {error !== null && <p className="error">{error}</p>}
      {isHeld(run) && run?.cliConfirm != null && !busy && (
        // #1909: the commands the draft would run and the digest they are confirmed under. Confirm
        // sends exactly this digest; the Runtime refuses it when the flow changed since, and
        // refuses anyone but the owner.
        <section aria-label="Run these commands?" className="cli-confirm">
          <strong>Run these commands?</strong>
          <span className="muted">This draft runs commands on this machine. They run only once you confirm this exact list.</span>
          <ol aria-label="Commands" className="cli-argv">{run.cliConfirm.argv.map((line, index) => <li key={index} className="mono">{line}</li>)}</ol>
          <span className="mono cli-digest" aria-label="Digest">{run.cliConfirm.digest}</span>
          <button type="button" className="b act" onClick={() => { setPicked(null); start(run.cliConfirm!.digest); }}>Confirm and run</button>
        </section>
      )}
      <DecisionBar runtime={runtime} flow={flow} runFinished={finished} onDecided={onRan} />
      <div className="steps-card">
        <ol className="steps" aria-label="Steps">
          {steps.map((step) => (
            <li key={step.n} aria-label={`Step ${step.n}`} className={`step ${step.status.replace(" ", "-")}${stale ? " stale" : ""}`}>
              <button type="button" className="b" aria-pressed={selected?.n === step.n} onClick={() => setPicked(step.n)}>
                <span className="step-thumb">{step.cli ? <CliThumb step={step} /> : thumb(step.screen) ? <img src={thumb(step.screen)} alt={`Step ${step.n} frame`} /> : <span>{step.frame ? "loading" : step.status === "precondition" ? "start" : "no frame"}</span>}</span>
                <span className="step-text">
                  <span className="step-head"><span className="mono">{step.n} DOES</span><span className="step-status">{STATUS_TEXT[step.status]}</span>{stale && <span className="stale-mark" title="From the earlier version of this journey">stale</span>}</span>
                  <span className="step-label">{step.label}</span>
                  {viewportLine(step)}
                </span>
              </button>
            </li>
          ))}
        </ol>
      </div>
      {selected !== null && (
        <CompareFrame runtime={runtime} flowId={flow.id} step={selected} steps={steps.length} now={frame} mobileNow={mobileFrames[selected.screen]} earlier={earlierRun(history, run, selected.screen)} />
      )}
    </div>
  );
}

/** #1919: Send to fix on a failed or drifted run. It records the owner's request on the team run
 * (`task.request_recorded`, the existing signal route) with the journey, its flow file, the failing
 * step, the run time and the note. The request id is the run's, so a run already sent reads
 * "Sent · <time>" (from the Runtime's requests) and is never sent twice. */
function SendToFix({ runtime, flow, run, steps, executionId }: { runtime: Runtime; flow: JourneyFlow; run: JourneyRun; steps: Step[]; executionId: string | null }) {
  const id = fixRequestId(flow.id, run);
  const [note, setNote] = useState("");
  const [sent, setSent] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [refusal, setRefusal] = useState<string | null>(null);
  // A resend after a lost answer carries the first press's instant, so its bytes are the same.
  const [at] = useState(() => new Date().toISOString().replace(/\.\d{3}Z$/, "Z"));
  const find = async (): Promise<boolean> => {
    if (executionId === null) return false;
    const found = (await runtime.requests(executionId)).find((request) => request.requestId === id);
    if (found !== undefined) setSent(found.recordedAt ?? at);
    return found !== undefined;
  };
  useEffect(() => { find().catch(() => undefined); }, [id, executionId]); // eslint-disable-line react-hooks/exhaustive-deps
  if (executionId === null) return <span className="run-note">Open a team run to send this to fix.</span>;
  if (sent !== null) return <span role="status" aria-label="Sent to fix" className="chip pass">Sent · {askedWhen(sent) ?? "recorded"}</span>;
  const send = async () => {
    setBusy(true);
    setRefusal(null);
    try {
      await runtime.recordRequest(executionId, id, fixBody(executionId, id, fixTitle(flow), fixAsk(flow, run, steps, note), at, OWNER.id));
      if (!(await find().catch(() => false))) setSent(at);
    } catch (cause) {
      // The first press may have landed and only its answer been lost: then it is sent.
      if (!(await find().catch(() => false))) setRefusal(message(cause, "The Runtime did not record the request."));
    } finally {
      setBusy(false);
    }
  };
  return (
    <span className="send-fix">
      <input aria-label="Note for the fix" placeholder="Note (optional)" maxLength={NOTE_MAX} value={note} disabled={busy} onChange={(event) => setNote(event.target.value)} />
      <button type="button" className="b ghost" disabled={busy} onClick={() => { void send(); }}>{busy ? "Sending…" : "Send to fix"}</button>
      {refusal !== null && <span role="alert" className="error">{refusal}</span>}
    </span>
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
    send().then(() => { setReason(""); onDecided(); }, (cause: unknown) => setRefusal(message(cause, "The Runtime refused.")))
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
