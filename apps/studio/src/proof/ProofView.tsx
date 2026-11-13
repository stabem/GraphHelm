import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { usePageTools } from "../webmcp/page-tools";

import { filterJourneys, groupJourneys, JOURNEY_SEARCH_MAX } from "./journey-search";
import { journeyName, RuntimeError, type JourneyFlow, type JourneyRun, type Runtime } from "../runtime/client";
import { earlierRun } from "./compare";
import { CheckMarks, CompareFrame } from "./CompareFrame";
import { FrameWaiting } from "./FrameWaiting";

/** #2028: the step thumbnail's shape (`.step-thumb` `aspect-ratio`), for placing its check boxes. */
const THUMB_ASPECT = 1694 / 946;
import { decisionOf, offersDecision, rejectNote } from "./decision";
import { failureNotice, type FailureNotice } from "./failure";
import { fixAsk, fixBody, fixRequestId, fixTitle, NOTE_MAX, offersFix } from "./send-fix";
import { askedWhen } from "../lanes/asked";
import { OWNER } from "../runtime/client";
import { clearRunAllBatch, isRunAllRunId, makeRunAllBatch, readRunAllBatch, reconcileRunAllJourney, RunAllIdentityFailure, RunAllReadFailure, RUN_ALL_TIMEOUT_MS, runAllUntil, runAllViewIsCurrent, updateRunAllJourney, writeRunAllBatch, type RunAllBatch, type RunAllJourneyState, type RunAllReason } from "./run-all";
import { countSteps, failsOnlyOn, outputLines, waitsForFrame, staleCountsText, stepStatusText, isHeld, isWaitingReview, journeyKind, journeyScore, journeySteps, lastRunChip, historyChip, liveCard, pastAsRun, CHANGED, withoutLive, afterPanelGone, watchedIds, liveAfterList, preparingStatus, type JourneyKind, type Step, type Viewport } from "./steps";
import { useJourneyRun } from "./use-journey-run";
import { Empty, Loading } from "../states/ViewState";
import { ActionError, actionErrorDetails, type ActionErrorDetails } from "../states/ActionError";
import { When } from "../time/Stamp";
import { when } from "../time/when";
import { journeyToFix } from "../sidebar/project-summary";

interface ProofViewProps {
  projectKey: string;
  runtime: Runtime;
  journeys: JourneyFlow[] | null;
  error: string | null;
  /** The team run that is open: an approved journey's replay records its proof there. */
  executionId: string | null;
  /** A run finished: the journeys' last results are read again. */
  onRan: () => void;
  /** The journey a `?journey=` link opens on; `null` = the first listed. */
  initial?: string | null;
  /** The owner picked a journey: the URL keeps it, so a Proof view opened again lands on it. */
  onPicked?: (id: string) => void;
  /** #2442: the earlier run of the opened journey a `?past=` link names; `null` = the latest run. */
  initialPast?: string | null;
  /** #2442: the owner opened (or left) an earlier run: the URL keeps it, so it can be linked and reloaded. */
  onPast?: (run: string | null) => void;
}


/** #1950: how often a running journey no panel follows is read, and how many failed reads in a
 * row hand its card back to the journey list. */
const WATCH_POLL_MS = 2_000;
const WATCH_MAX_FAILURES = 5;

function nameOf(flow: JourneyFlow): string {
  return journeyName(flow);
}

function runAllReasonText(reason: RunAllReason | undefined): string {
  switch (reason) {
    case "start_rejected": return "The Runtime rejected the start request.";
    case "post_outcome_unknown": return "The Runtime may have accepted the start, but Studio did not get its reply.";
    case "read_deadline": return "Studio could not confirm the run before the wait limit.";
    case "read_failed": return "Studio could not read the Runtime; this run may still be active.";
    case "run_identity_unknown": return "The Runtime did not confirm this exact run.";
    case "storage_failed": return "Studio could not safely save the batch state.";
    case "view_left": return "The Proof view closed while this batch was running.";
    case "reloaded": return "Studio was reloaded before this batch finished.";
    default: return "The batch stopped before this journey started.";
  }
}

function withInterruptedReason(batch: RunAllBatch, reason: RunAllReason, updatedAt = Date.now()): RunAllBatch {
  return { ...batch, reason, updatedAt, journeys: batch.journeys.map((journey) => journey.phase === "not_started"
    ? { ...journey, reason, updatedAt }
    : journey) };
}

function lockName(projectKey: string): string {
  return `graphhelm:studio:run-all:v1:${encodeURIComponent(projectKey)}`;
}

export function runAllJourneyNotice(journey: RunAllJourneyState | undefined, reason: RunAllReason | undefined): string | null {
  if (journey === undefined || reason === undefined) return null;
  const detail = runAllReasonText(journey.reason ?? reason);
  if (journey.phase === "not_started") return `Not started: batch interrupted. ${detail}`;
  if (journey.phase === "start_pending") return "Observation interrupted; run state unknown. Studio did not get the start reply and cannot say whether the Runtime accepted it.";
  if (journey.phase === "running") return "Observation interrupted; run state unknown. The Runtime last confirmed it was running; it may still be active, and Studio did not record a result.";
  if (journey.phase === "unknown") return `Observation interrupted; run state unknown. ${detail}`;
  return `This run finished, but the batch remainder was not resumed. ${detail}`;
}

/** The Proof region (docs/design/studio/Proof.dc.html): the journeys column, then the selected
 * journey's run bar, console, step grid and the selected step's compare and Action panel. */
/** #2939: `list` without one occurrence of `id`. */
function withoutOne(list: readonly string[], id: string): readonly string[] {
  const at = list.indexOf(id);
  return at < 0 ? list : [...list.slice(0, at), ...list.slice(at + 1)];
}

/** #2939: `runtime`, with each start or confirm of a journey run told to `tell` when it is sent and
 * again when it is answered or refused. The request outlives the panel that sent it, so the telling
 * rides on the request, not on the panel. Every other call goes to `runtime` unchanged. */
function tellingStarts(runtime: Runtime, tell: (flowId: string, moment: "sent" | "settled") => void): Runtime {
  const held = <T,>(flowId: string, send: () => Promise<T>): Promise<T> => {
    tell(flowId, "sent");
    return send().finally(() => tell(flowId, "settled"));
  };
  return new Proxy(runtime, {
    get(target, key) {
      if (key === "startJourneyRun") return (flowId: string, executionId: string | null) => held(flowId, () => target.startJourneyRun(flowId, executionId));
      if (key === "confirmJourneyRun") return (flowId: string, digest: string, executionId: string | null) => held(flowId, () => target.confirmJourneyRun(flowId, digest, executionId));
      const value: unknown = Reflect.get(target, key, target);
      return typeof value === "function" ? (value as (...args: unknown[]) => unknown).bind(target) : value;
    },
  });
}

export function ProofView({ projectKey, runtime, journeys, error, executionId, onRan, initial = null, onPicked, initialPast = null, onPast }: ProofViewProps) {
  // The journey a deep link named (`studio open --journey`), until the owner picks another.
  const [picked, setPickedState] = useState<string | null>(initial);
  const setPicked = (id: string) => { setPickedState(id); onPicked?.(id); };
  // #1905: bumped after a run is started from the column, so the open panel reads the run again.
  const [reread, setReread] = useState(0);
  // #1914: the run the open panel follows, by journey, so its card reads it before the list is read again.
  const [live, setLive] = useState<Record<string, JourneyRun>>({});
  // #2939: the journeys whose start request is in flight, from a card and from the run panel. Each
  // side reads the other's, so one journey's two Run buttons cannot both send a start. The panel's
  // are held here, not in the panel, so picking another journey does not forget a request still open.
  const [cardStarting, setCardStarting] = useState<string | null>(null);
  const [panelStarts, setPanelStarts] = useState<readonly string[]>([]);
  const panelRuntime = useMemo(() => tellingStarts(runtime, (flowId, moment) => {
    setPanelStarts((before) => (moment === "sent" ? [...before, flowId] : withoutOne(before, flowId)));
  }), [runtime]);
  // A list read again carries the result; a run still playing stays until its next answer.
  // #1953: a journey the list says is running is seeded, so a reload still shows it running.
  useEffect(() => { setLive((before) => liveAfterList(before, journeys)); }, [journeys]);
  // #1953: a Proof view opened again reads the journey list afresh, so a run started from an
  // earlier view (the list says running) is seen even when another journey is selected.
  useEffect(() => { onRan(); }, []); // eslint-disable-line react-hooks/exhaustive-deps
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
  // #2913: with no journeys there is nothing behind this alert, so it has Retry and no Dismiss: a
  // failed read stays a failure until a read succeeds, never a loading panel with no read in flight.
  if (error !== null && journeys === null) return <section id="panel-proof" aria-label="Proof" className="view"><ActionError error={actionErrorDetails("Journeys could not be read", error, error, "Retry reading the journeys.")} onRetry={onRan} /></section>;
  if (journeys === null) return <section id="panel-proof" aria-label="Proof" className="view"><Loading text="Reading the journeys…" /></section>;
  const selected = journeys.find((flow) => flow.id === picked) ?? journeys[0] ?? null;
  const started = (id: string) => {
    // #1950: a run started here is known running until the Runtime says otherwise.
    setLive((before) => ({ ...before, [id]: { state: "running" } }));
    if (id === selected?.id) setReread((n) => n + 1);
  };
  const observedRun = (id: string, run: JourneyRun) => { setLive((before) => ({ ...before, [id]: run })); };
  return (
    <section id="panel-proof" aria-label="Proof" className="proof">
      <JourneysColumn projectKey={projectKey} runtime={runtime} journeys={journeys} selected={selected?.id ?? null} executionId={executionId} onPick={setPicked} onStarted={started} onStarting={setCardStarting} panelStarts={panelStarts} onObserved={observedRun} onRan={onRan} live={live} />
      {selected === null
        ? <div className="proof-main"><Empty title="No journeys yet" hint="This project has no journeys yet. A journey appears here once one is drafted for a request." /></div>
        : <JourneyRunPanel key={`${selected.id}:${reread}`} runtime={panelRuntime} flow={selected} initialPast={selected.id === initial ? initialPast : null} onPast={onPast} executionId={executionId} onRan={onRan} onRun={(run) => setLive((before) => ({ ...before, [selected.id]: run }))} onGone={() => setLive((before) => afterPanelGone(before, selected.id))} startingFromCard={cardStarting === selected.id} />}
    </section>
  );
}

interface ColumnProps {
  projectKey: string;
  runtime: Runtime;
  journeys: JourneyFlow[];
  selected: string | null;
  executionId: string | null;
  onPick: (id: string) => void;
  onStarted: (id: string) => void;
  /** #2939: the journey whose start request this column has in flight, `null` once it is answered. */
  onStarting: (id: string | null) => void;
  /** #2939: the journeys whose start request the run panel has in flight. */
  panelStarts: readonly string[];
  onObserved: (id: string, run: JourneyRun) => void;
  onRan: () => void;
  live: Record<string, JourneyRun>;
}

function JourneysColumn({ projectKey, runtime, journeys, selected, executionId, onPick, onStarted, onStarting, panelStarts, onObserved, onRan, live }: ColumnProps) {
  const [busy, setBusy] = useState<string | null>(null);
  // #1914: the journey Run all is playing now; its card reads running even when no panel follows it.
  const [active, setActive] = useState<string | null>(null);
  const [refusal, setRefusalState] = useState<ActionErrorDetails | null>(null);
  const setRefusal = (value: string | ActionErrorDetails | null) => setRefusalState(value === null || typeof value !== "string" ? value : actionErrorDetails("Journey action failed", null, value, "Dismiss this message or retry the action."));
  const [interruptedBatch, setInterruptedBatch] = useState<RunAllBatch | null>(null);
  // #2845: the search text is not kept anywhere: a reload, or another project, starts it empty.
  const [search, setSearch] = useState({ projectKey, text: "" });
  const searchField = useRef<HTMLInputElement>(null);
  const setQuery = (text: string) => setSearch({ projectKey, text });
  const mounted = useRef(true);
  const viewGeneration = useRef(0);
  const observed = useRef(onObserved);
  observed.current = onObserved;
  useEffect(() => {
    const generation = ++viewGeneration.current;
    let live = true;
    mounted.current = true;
    let storage: Storage;
    try { storage = window.localStorage; }
    catch { if (live) setRefusal("Studio could not read saved Run all status in this browser."); return () => { live = false; }; }
    const locks = typeof navigator === "undefined" ? undefined : navigator.locks;
    const apply = (batch: RunAllBatch | null) => { if (live) setInterruptedBatch(batch); };
    const restore = async () => {
      let saved: RunAllBatch | null;
      try {
        saved = readRunAllBatch(storage, projectKey);
      } catch {
        if (live) setRefusal("Studio could not read the saved Run all status. Start a new batch to replace it.");
        return;
      }
      if (saved === null) { apply(null); return; }
      if (locks === undefined) {
        apply(saved);
        if (live) setRefusal("This browser cannot safely coordinate Run all between Studio tabs.");
        return;
      }
      const hasPending = saved.journeys.some((journey) => journey.phase !== "terminal");
      if (!hasPending) {
        apply(null);
        try {
          await locks.request(lockName(projectKey), { mode: "exclusive", ifAvailable: true }, async (lock) => {
            if (lock === null) return;
            const current = readRunAllBatch(storage, projectKey);
            if (current?.batchId === saved!.batchId) clearRunAllBatch(storage, projectKey);
          });
        } catch { /* Terminal results remain valid even if cleanup cannot run. */ }
        return;
      }
      saved = withInterruptedReason(saved, saved.reason ?? "reloaded");
      apply(saved);
      const index = saved.journeys.findIndex((journey) => ["start_pending", "running", "unknown"].includes(journey.phase));
      if (index < 0) {
        // The batch was saved before its first POST; every journey is still safely not started.
        try {
          await locks.request(lockName(projectKey), { mode: "exclusive", ifAvailable: true }, async (lock) => {
            if (lock === null) return;
            const current = readRunAllBatch(storage, projectKey);
            if (current?.batchId !== saved!.batchId) return;
            writeRunAllBatch(storage, saved!);
          });
        } catch { if (live) setRefusal(runAllReasonText("storage_failed")); }
        return;
      }
      const active = saved.journeys[index];
      if (active.phase === "start_pending" || !isRunAllRunId(active.runId)) {
        const unknown = withInterruptedReason(updateRunAllJourney(saved, index, "unknown", {
          reason: "post_outcome_unknown", updatedAt: Date.now(),
        }), "post_outcome_unknown");
        apply(unknown);
        try {
          await locks.request(lockName(projectKey), { mode: "exclusive", ifAvailable: true }, async (lock) => {
            if (lock === null) return;
            const current = readRunAllBatch(storage, projectKey);
            if (current?.batchId === unknown.batchId) writeRunAllBatch(storage, unknown);
          });
        } catch { if (live) setRefusal(runAllReasonText("storage_failed")); }
        return;
      }
      try {
        await locks.request(lockName(projectKey), { mode: "exclusive", ifAvailable: true }, async (lock) => {
          if (lock === null) {
            if (live) setRefusal("Run all is active in another Studio tab. This tab will not replace or continue it.");
            return;
          }
          const current = readRunAllBatch(storage, projectKey);
          if (current?.batchId !== saved!.batchId) return;
          const remainingMs = active.deadlineAt === undefined ? 0 : active.deadlineAt - Date.now();
          if (remainingMs <= 0) {
            const next = reconcileRunAllJourney(current, index, null, { reason: "read_deadline" });
            try { writeRunAllBatch(storage, next); }
            catch { if (live) setRefusal(runAllReasonText("storage_failed")); }
            apply(next);
            return;
          }
          let run: JourneyRun | null = null;
          let readFailed = false;
          try { run = await runtime.journeyRun(active.flowId, Math.min(30_000, remainingMs)); }
          catch { readFailed = true; }
          const next = reconcileRunAllJourney(current, index, run, {
            readFailed,
            ...(readFailed && active.deadlineAt !== undefined && active.deadlineAt <= Date.now() ? { reason: "read_deadline" as const } : {}),
          });
          try { writeRunAllBatch(storage, next); }
          catch { if (live) setRefusal(runAllReasonText("storage_failed")); }
          apply(next);
          if (run !== null && run.runId === active.runId && ["running", "ready", "failed"].includes(run.state) && live) observed.current(active.flowId, run);
        });
      } catch {
        if (live) setRefusal("Studio could not reconcile the saved Run all status with the Runtime.");
      }
    };
    void restore();
    return () => {
      live = false;
      mounted.current = false;
      if (viewGeneration.current === generation) viewGeneration.current += 1;
    };
  }, [projectKey, runtime]);
  const statuses = new Map(journeys.map((flow) => [flow.id,
    liveCard(flow, live[flow.id] ?? null)
      ?? (active === flow.id ? liveCard(flow, { state: "running" }) : null)
      ?? lastRunChip(flow.lastRun)]));
  const counts = { passed: 0, failed: 0, stale: 0, not_run: 0, preview: 0, running: 0, waiting: 0, unknown: 0 };
  for (const status of statuses.values()) counts[status.category] += 1;
  const otherCounts = [
    counts.preview > 0 ? `${counts.preview} previews (not proof)` : null,
    counts.running > 0 ? `${counts.running} running` : null,
    counts.waiting > 0 ? `${counts.waiting} waiting` : null,
    counts.unknown > 0 ? `${counts.unknown} unknown` : null,
  ].filter((text) => text !== null).join(" \u00b7 ");
  // #2845: the search field only picks which cards the list shows. Everything below that acts on
  // journeys (Run all, Approve N, the counts, the page tool) reads `journeys`, never `shown`.
  const query = search.projectKey === projectKey ? search.text : "";
  const searching = query.trim() !== "";
  const shown = filterJourneys(journeys, query);
  const groups = groupJourneys(shown);
  // The scores in a group header are the whole group's, whatever the query; `groups` only picks what is drawn.
  const wholeGroups = new Map(groupJourneys(journeys));
  // #1905: Approve N is the single Approve, one journey at a time, on every journey the Runtime says
  // it would accept now. The Runtime decides who may approve (owner credential only).
  const approvable = journeys.filter((flow) => flow.approvable === true && decisionOf(flow) !== "approved");

  const runOne = async (flow: JourneyFlow, isLive = () => mounted.current): Promise<JourneyRun> => {
    const run = await runtime.startJourneyRun(flow.id, executionId);
    if (isLive()) onStarted(flow.id);
    return run;
  };
  // #2841: `busy` is state, so a second call in the same tick still reads null. This ref is set before the
  // run starts and cleared where `busy` is cleared, so the button and the page tool share one single-flight.
  const starting = useRef(false);
  const runSingle = (flow: JourneyFlow) => {
    if (starting.current || panelStarts.includes(flow.id)) return;
    starting.current = true;
    onStarting(flow.id);
    setRefusal(null);
    setBusy(`Starting ${nameOf(flow)}`);
    onPick(flow.id);
    runOne(flow).catch((cause: unknown) => setRefusal(actionErrorDetails("Journey run failed", cause, "The Runtime refused the run.", "Check the Runtime response, then try Run again."))).finally(() => { starting.current = false; setBusy(null); onStarting(null); });
  };
  // #2839: the function behind the Journeys column's `Run <journey name>` button (`runSingle`: it
  // selects the journey and starts its run), offered to a WebMCP agent. It is not the run panel's
  // `Run` button; both reach the same preparing screen. It refuses exactly where the card button is disabled.
  usePageTools([{
    name: "studio_proof_run",
    description: "Select a journey and start its run, as the Run <journey name> button on its card in the Journeys column does.",
    inputSchema: { type: "object", properties: { journey: { type: "string" } }, required: ["journey"], additionalProperties: false },
    handler: ({ journey }) => {
      const flow = typeof journey === "string" ? journeys.find((candidate) => candidate.id === journey) : undefined;
      if (flow === undefined) return { ok: false, reason: "journey must be the id of a journey listed in Proof." };
      if (busy !== null || starting.current) return { ok: false, reason: "Run is disabled while the journeys column is busy." };
      if (panelStarts.includes(flow.id)) return { ok: false, reason: "Run is disabled while this journey's run is starting." };
      runSingle(flow);
      return { ok: true };
    },
  }]);
  // #2901: Run all and Approve N take the same single-flight as the single Run, and release it in `finally`,
  // so no early return or thrown error inside the action can leave the column locked.
  const exclusive = async (action: () => Promise<void>) => {
    if (starting.current) return;
    starting.current = true;
    try { await action(); } finally { starting.current = false; }
  };
  const runAll = () => exclusive(runAllAction);
  const approveAll = () => exclusive(approveAllAction);
  const runAllAction = async () => {
    const generation = viewGeneration.current;
    const isCurrentView = () => mounted.current && runAllViewIsCurrent(generation, viewGeneration.current);
    setRefusal(null);
    if (typeof navigator === "undefined" || navigator.locks === undefined) {
      setRefusal("This browser cannot safely coordinate Run all between Studio tabs.");
      return;
    }
    let storage: Storage;
    let batchId: string;
    try {
      storage = window.localStorage;
      batchId = crypto.randomUUID();
    } catch {
      setRefusal("Studio could not prepare a safe Run all record in this browser.");
      return;
    }
    setBusy("Starting Run all");
    try {
      await navigator.locks.request(lockName(projectKey), { mode: "exclusive", ifAvailable: true }, async (lock) => {
        if (!isCurrentView()) return;
        if (lock === null) {
          if (isCurrentView()) setRefusal("Run all is active in another Studio tab. This tab did not start or replace a batch.");
          return;
        }
        let batch = makeRunAllBatch(projectKey, journeys.map((flow) => flow.id), batchId);
        try { writeRunAllBatch(storage, batch); }
        catch {
          if (isCurrentView()) setRefusal(runAllReasonText("storage_failed"));
          return;
        }
        if (isCurrentView()) setInterruptedBatch(batch);
        const save = (next: RunAllBatch) => {
          const stored = readRunAllBatch(storage, projectKey);
          if (stored?.batchId !== batchId) throw new Error("Run all batch ownership changed.");
          writeRunAllBatch(storage, next);
          batch = next;
          if (isCurrentView()) setInterruptedBatch(next);
        };
        const showStorageFailure = (index: number, phase: RunAllJourneyState["phase"], runId?: string) => {
          if (!isCurrentView()) return;
          const state = updateRunAllJourney(batch, index, phase, { ...(runId === undefined ? {} : { runId }), reason: "storage_failed" });
          setInterruptedBatch(withInterruptedReason(state, "storage_failed"));
          setRefusal(runAllReasonText("storage_failed"));
        };
        for (const [index, flow] of journeys.entries()) {
          if (!isCurrentView()) {
            const stopped = withInterruptedReason(updateRunAllJourney(batch, index, batch.journeys[index].phase, {
              ...(batch.journeys[index].runId === undefined ? {} : { runId: batch.journeys[index].runId }), reason: "view_left",
            }), "view_left");
            try { save(stopped); } catch { /* Keep the last complete record. */ }
            return;
          }
          setBusy(`Running all · ${index + 1}/${journeys.length}`);
          setActive(flow.id);
          try { save(updateRunAllJourney(batch, index, "start_pending")); }
          catch {
            if (isCurrentView()) {
              setInterruptedBatch(withInterruptedReason(batch, "storage_failed"));
              setRefusal(runAllReasonText("storage_failed"));
            }
            return;
          }
          let started: JourneyRun;
          try { started = await runOne(flow, isCurrentView); }
          catch (cause) {
            const rejected = cause instanceof RuntimeError && (cause.status === 401 || cause.status === 403);
            const reason: RunAllReason = rejected ? "start_rejected" : "post_outcome_unknown";
            const phase = rejected ? "not_started" : "unknown";
            try { save(withInterruptedReason(updateRunAllJourney(batch, index, phase, { reason }), reason)); }
            catch { showStorageFailure(index, phase); return; }
            if (isCurrentView()) setRefusal(runAllReasonText(reason));
            return;
          }
          if (!isRunAllRunId(started.runId) || !["running", "ready", "failed"].includes(started.state)) {
            const reason = "run_identity_unknown" as const;
            try { save(withInterruptedReason(updateRunAllJourney(batch, index, "unknown", { reason }), reason)); }
            catch { showStorageFailure(index, "unknown"); return; }
            if (isCurrentView()) setRefusal(runAllReasonText(reason));
            return;
          }
          const deadlineAt = started.state === "running" ? Date.now() + RUN_ALL_TIMEOUT_MS : undefined;
          try { save(updateRunAllJourney(batch, index, started.state === "running" ? "running" : "terminal", { runId: started.runId, deadlineAt })); }
          catch { showStorageFailure(index, started.state === "running" ? "unknown" : "terminal", started.runId); return; }
          if (started.state !== "running" && isCurrentView()) onObserved(flow.id, started);
          if (started.state === "running") {
            try {
              const outcome = await runAllUntil(started.runId, (timeoutMs) => runtime.journeyRun(flow.id, timeoutMs), {
                timeoutMs: Math.max(0, deadlineAt! - Date.now()),
                live: isCurrentView,
              });
              if (outcome.state === "cancelled") {
                const stopped = withInterruptedReason(updateRunAllJourney(batch, index, "running", { runId: started.runId, reason: "view_left" }), "view_left");
                try { save(stopped); } catch { /* Keep the last complete record. */ }
                return;
              }
              if (outcome.state === "timeout") {
                const reason = "read_deadline" as const;
                try { save(withInterruptedReason(updateRunAllJourney(batch, index, "unknown", { runId: started.runId, reason }), reason)); }
                catch { showStorageFailure(index, "unknown", started.runId); return; }
                if (isCurrentView()) setRefusal(`${runAllReasonText(reason)} (${RUN_ALL_TIMEOUT_MS / 60_000} minute limit).`);
                return;
              }
              onObserved(flow.id, outcome.run);
              try { save(updateRunAllJourney(batch, index, "terminal", { runId: started.runId })); }
              catch { showStorageFailure(index, "terminal", started.runId); return; }
            } catch (cause) {
              const reason: RunAllReason = cause instanceof RunAllIdentityFailure ? "run_identity_unknown" : cause instanceof RunAllReadFailure ? cause.reason : "read_failed";
              try { save(withInterruptedReason(updateRunAllJourney(batch, index, "unknown", { runId: started.runId, reason }), reason)); }
              catch { showStorageFailure(index, "unknown", started.runId); return; }
              if (isCurrentView()) setRefusal(runAllReasonText(reason));
              return;
            }
          }
          if (isCurrentView()) {
            setActive(null);
            onRan();
          }
        }
        try {
          const stored = readRunAllBatch(storage, projectKey);
          if (stored?.batchId === batchId) clearRunAllBatch(storage, projectKey);
          if (isCurrentView()) setInterruptedBatch(null);
        } catch {
          if (isCurrentView()) setRefusal("All runs finished, but Studio could not clear the saved batch marker.");
        }
      });
    } catch {
      if (isCurrentView()) setRefusal("Studio could not coordinate or save this Run all batch.");
    } finally {
      if (isCurrentView()) {
        setActive(null);
        setBusy(null);
        onRan();
      }
    }
  };
  const approveAllAction = async () => {
    setRefusal(null);
    try {
      for (const [index, flow] of approvable.entries()) {
        setBusy(`Approving ${index + 1}/${approvable.length}`);
        await runtime.approveJourney(flow.id);
      }
    } catch (cause) {
      setRefusal(actionErrorDetails("Journeys could not be approved", cause, "The Runtime refused this approval.", "Check the journey state, then try again."));
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
      {interruptedBatch?.reason !== undefined && <p role="status" className="run-status">Run all stopped: {runAllReasonText(interruptedBatch.reason)} {interruptedBatch.journeys.filter((journey) => journey.phase === "not_started").length} journeys not started.</p>}
      {busy !== null && <span role="status" className="run-status"><span className="pulse" aria-hidden="true" />{busy}</span>}
      {refusal !== null && <ActionError error={refusal} onDismiss={() => setRefusal(null)} />}
      {journeys.length > 0 && (
        <span className="proof-counts">{counts.passed} passed · {counts.failed} failed · {counts.stale} stale · {counts.not_run} not run</span>
      )}
      {otherCounts !== "" && <span className="proof-counts">{otherCounts}</span>}
      <div className="journey-search">
        <input ref={searchField} type="search" aria-label="Search journeys" placeholder="Search journeys" maxLength={JOURNEY_SEARCH_MAX} value={query} onChange={(event) => setQuery(event.target.value)} onKeyDown={(event) => {
          if (event.key === "Escape" && query !== "") { event.preventDefault(); event.stopPropagation(); setQuery(""); }
        }} />
        {query !== "" && <button type="button" className="clear" aria-label="Clear search" onClick={() => { setQuery(""); searchField.current?.focus(); }}>×</button>}
      </div>
      {searching && (
        <div role="status" aria-label="Journeys shown" className="journey-search-status">
          <span className="proof-counts journey-search-count">{shown.length} of {journeys.length} journeys</span>
          {shown.length === 0 && <p className="journey-search-none">No journey matches “{query}”.</p>}
        </div>
      )}
      <ul className="journey-list" aria-label="Journeys">
        {groups.map(([feature, flows]) => {
          const whole = wholeGroups.get(feature) ?? flows;
          const passed = whole.filter((flow) => statuses.get(flow.id)?.category === "passed").length;
          const failed = whole.some((flow) => statuses.get(flow.id)?.category === "failed");
          return (
            <li key={feature} className="journey-group">
              <div className="journey-feature">
                <span aria-hidden="true">▾</span><strong>{feature}</strong>
                <span className={`mono group-score${failed ? " fail" : passed > 0 ? " pass" : ""}`} aria-label={`${feature}: ${passed} of ${whole.length} passed`}>{passed}/{whole.length}</span>
              </div>
              <ul>
                {flows.map((flow) => <JourneyCard key={flow.id} flow={flow} selected={flow.id === selected} disabled={busy !== null || panelStarts.includes(flow.id)} onPick={onPick} onRun={runSingle} run={live[flow.id] ?? null} playing={active === flow.id} interruption={runAllJourneyNotice(interruptedBatch?.journeys.find((journey) => journey.flowId === flow.id), interruptedBatch?.reason)} />)}
              </ul>
            </li>
          );
        })}
      </ul>
    </aside>
  );
}

export function JourneyCard({ flow, selected, disabled, onPick, onRun, run, playing, interruption = null }: { flow: JourneyFlow; selected: boolean; disabled: boolean; onPick: (id: string) => void; onRun: (flow: JourneyFlow) => void; run: JourneyRun | null; playing: boolean; interruption?: string | null }) {
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
      {interruption !== null && <span role="status" className="error run-status">{interruption}</span>}
      {score.total > 0 && (
        <span className="journey-bar" aria-hidden="true">{score.bar.map((cell, index) => <span key={index} className={cell} />)}</span>
      )}
      <JourneyFindings flow={flow} />
    </li>
  );
}

/** #2325: what `journey validate` reports on the flow, shown as findings, never as a failed run:
 * errors first, each with its code, where in the file, and what to write there. */
function JourneyFindings({ flow }: { flow: JourneyFlow }) {
  const listed = [...(flow.findings ?? [])].sort((a, b) => Number(a.severity !== "error") - Number(b.severity !== "error"));
  const toFix = journeyToFix(flow);
  if (listed.length === 0 && !toFix) return null;
  const errors = listed.filter((finding) => finding.severity === "error").length;
  // The approved words (docs/design/studio/2325-2324/). The rail's "to fix" counts the same journeys
  // through journeyToFix; an unreadable journey with no listed finding is one thing to fix.
  const label = toFix ? `${Math.max(errors, 1)} to fix` : `${listed.length} ${listed.length === 1 ? "warning" : "warnings"}`;
  return (
    <details className="journey-findings">
      <summary className={`chip ${toFix ? "fix" : "idle"}`}>{label}</summary>
      <ul aria-label={`Findings for ${nameOf(flow)}`}>
        {listed.map((finding, index) => (
          <li key={index}>
            <code>{finding.code ?? "finding"}</code>{finding.pointer ? <> at <code>{finding.pointer}</code></> : null}{finding.message ? `: ${finding.message}` : ""}
          </li>
        ))}
      </ul>
    </details>
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
  if (isWaitingReview(run)) return "Waits for review";
  if (run.state === "failed") return run.reason ? `The run could not finish: ${run.reason}` : "The run could not finish";
  return `${run.result === "pass" ? "Passed" : run.result === "drift" ? "Drifted" : "Failed"}${run.ranAt ? ` · ${when(run.ranAt).text}` : ""}`;
}

/** #1777: each viewport's mark on a step card, and the line that says a break is mobile-only. */
function viewportLine(step: Step, stale: boolean) {
  // #2086: a stale run's marks are the earlier version's; this version ran on neither viewport.
  const mark = (viewport: Viewport) => (stale || step.viewports[viewport] === "not run" ? "not run" : MARK[step.viewports[viewport]]);
  const tone = (viewport: Viewport) => (stale ? "not-run" : step.viewports[viewport].replace(" ", "-"));
  const only = failsOnlyOn(step);
  return (
    <>
      <span className="step-viewports">web <b className={tone("web")}>{mark("web")}</b> · mobile <b className={tone("mobile")}>{mark("mobile")}</b></span>
      {only !== null && !stale && <span className="fails-only">Fails only on {only}</span>}
    </>
  );
}

export function JourneyRunPanel({ runtime, flow, executionId, onRan, onRun, onGone, initialPast = null, onPast, startingFromCard = false }: { runtime: Runtime; flow: JourneyFlow; executionId: string | null; onRan: () => void; onRun: (run: JourneyRun) => void; onGone: () => void; initialPast?: string | null; onPast?: (run: string | null) => void; startingFromCard?: boolean }) {
  // #2442: the earlier run on screen (its history id), else the latest or live run.
  const [viewing, setViewing] = useState<string | null>(initialPast);
  const view = (id: string | null) => { setViewing(id); onPast?.(id); };
  const { run, frames, mobileFrames, log, history, earlier, viewed, error, starting, start, dismissError } = useJourneyRun(runtime, flow.id, executionId, onRan, onRun, viewing);
  const [showLog, setShowLog] = useState(false);
  const showLogAs = (open: boolean) => setShowLog(open);
  // #1914: a panel that goes away (another journey picked) stops reporting; its card must not stay "running".
  useEffect(() => onGone, []); // eslint-disable-line react-hooks/exhaustive-deps
  const [picked, setPicked] = useState<number | null>(null);
  // #1956: with no run of the current version, the earlier version's run is shown, marked stale.
  const stale = earlier !== null && viewed === null && !starting;
  // #2442: a viewed earlier run is read as a finished run of its own: its steps, frames and results, never the live ones.
  const shown = viewed !== null ? pastAsRun(viewed) : stale ? pastAsRun(earlier) : run;
  const steps = journeySteps(flow, shown);
  const counts = countSteps(steps);
  // #2939: a start the journey's card has in flight is this journey starting, whoever was clicked.
  const busy = starting || startingFromCard || run?.state === "running";
  const status = runStatus(run, starting || startingFromCard);
  const said = stale ? CHANGED : verdict(shown);
  const selected = steps.find((step) => step.n === picked) ?? steps.find((step) => step.status === "failed")
    // #1912: a command flow opens on its first command, not on the start screen it never observes.
    ?? steps.find((step) => step.status !== "precondition") ?? steps[0] ?? null;
  const frame = selected === null ? undefined : frames[selected.screen];
  // #1878: a mobile-only run has no web frame; its step thumbnail is the mobile one.
  const thumb = (screen: string): string | undefined => frames[screen] ?? mobileFrames[screen];
  const finished = shown?.state === "ready";
  // #1298: a failed run says where and why, above the console; its Send to fix moves into that alert.
  const failure = (busy && viewed === null) || stale ? null : failureNotice(shown, steps);
  // #2442: an earlier run's console is what the Runtime kept of it; the live run's is what it printed so far.
  const consoleLines = viewed === null ? log : [...outputLines(shown), shown?.launchLog?.tail, shown?.driverLog?.tail].filter((line): line is string => typeof line === "string" && line.trim() !== "");
  const fix = viewed === null && run !== null && offersFix(run) ? <SendToFix key={fixRequestId(flow.id, run)} runtime={runtime} flow={flow} run={run} steps={steps} executionId={executionId} /> : null;
  // The Show log / Hide log button is there while a run is going, once it has printed, or for an earlier run.
  const hasLogButton = busy || log.length > 0 || viewed !== null;
  // #2839: that button's state setter, offered to a WebMCP agent; refuses where the button is absent.
  usePageTools([{
    name: "studio_proof_set_log",
    description: "Open or close the run console of the selected journey, as its Show log or Hide log button does.",
    inputSchema: { type: "object", properties: { open: { type: "boolean" } }, required: ["open"], additionalProperties: false },
    handler: ({ open }) => {
      if (typeof open !== "boolean") return { ok: false, reason: "open must be true or false." };
      if (!hasLogButton) return { ok: false, reason: "The log button is not shown: no run is going and none has printed." };
      showLogAs(open);
      return { ok: true };
    },
  }]);
  return (
    <div className="proof-main">
      <section aria-label="Journey run" className="journey-run">
        <div className="run-head">
          <h1>{nameOf(flow)}</h1>
          <KindBadge flow={flow} />
          {busy && <span className="chip running" aria-label="Journey running">● running</span>}
          <span className="mono run-counts" aria-label="Step counts">
            {stale
              ? <span className="idle">{staleCountsText(steps)}</span>
              : <><span className="pass">{counts.passed} passed</span> <span className="wait">{counts.waits} waits for you</span> <span className="fail">{counts.failed} failed</span> <span className="idle">{counts.notRun} not run</span></>}
          </span>
        </div>
        {typeof shown?.head === "string" && shown.head !== "" && (
          // #2469: the project commit this run used, and a warning when it was behind origin/main.
          <p className="run-checkout mono" title={shown.head}>HEAD {shown.head.slice(0, 7)}</p>
        )}
        {typeof shown?.behindOrigin === "number" && shown.behindOrigin > 0 && (
          <p role="status" className="run-behind">
            {/* #2710: the Runtime's sentence already says "behind origin/main (as of the last fetch)"; say it once. */}
            {shown.warning !== undefined
              ? `${shown.warning} (${shown.behindOrigin} ${shown.behindOrigin === 1 ? "commit" : "commits"} behind)`
              : `${shown.behindOrigin} ${shown.behindOrigin === 1 ? "commit" : "commits"} behind origin/main (as of the last fetch). Update the project (git pull), then run again.`}
          </p>
        )}
        {flow.purpose && <p><span className="section-label">PROVES</span> {flow.purpose}</p>}
        <div className="run-bar">
          {finished && <span className="run-score">{counts.passed} passed · {counts.failed} failed</span>}
          {finished && run?.kind !== "replay" && <span className="preview-tag">Preview — not proof</span>}
          <button type="button" className="b act" disabled={busy} onClick={() => { setPicked(null); view(null); start(); }}>
            {busy ? "Running…" : run === null || run.state === "none" ? "Run" : "Run again"}
          </button>
          {busy && (
            // #1905: the Runtime serves no route that cancels a running journey (only GET and POST
            // /v1/journey-flows/{id}/preview), so Stop is shown and says why it cannot act.
            <button type="button" className="b ghost" disabled aria-describedby="proof-stop-note">Stop</button>
          )}
          {busy && <span id="proof-stop-note" className="run-note">The Runtime has no route to stop a run.</span>}
          {failure === null && fix}
          {status !== null && <span role="status" aria-label={status.name} className="run-status"><span className="pulse" aria-hidden="true" />{status.text}</span>}
          {said !== null && <span role="status" aria-label={said.split(" · ")[0]} className="run-verdict" title={when(run?.ranAt).full || undefined}>{said}</span>}
          {stale && <span className="run-note stale-note">Results and frames from the earlier version ({earlier.ranAt ? when(earlier.ranAt).text : earlier.run}). Run to prove the current version.</span>}
          {hasLogButton && (
            <button type="button" className="b ghost" aria-expanded={showLog} onClick={() => showLogAs(!showLog)}>{showLog ? "Hide log" : "Show log"}</button>
          )}
        </div>
        {failure !== null && <FailureAlert notice={failure} fix={fix} />}
        {showLog && (
          <pre role="log" aria-label="Run console" className="run-log">{consoleLines.length > 0 ? consoleLines.join("\n") : busy && viewed === null ? "Waiting for the Runtime's first line…" : NO_LOG}</pre>
        )}
        {viewed !== null && (
          // #2442: an earlier run is on screen; the latest or live run is one click away.
          <p role="status" className="viewing-past">
            <span>Showing the run of {viewed.ranAt ? when(viewed.ranAt).full || viewed.run : viewed.run}{busy ? "; a run is live now" : ""}.</span>
            <button type="button" className="b ghost" onClick={() => view(null)}>{busy ? "Show the live run" : "Back to the latest run"}</button>
          </p>
        )}
        {viewing !== null && viewed === null && history.length > 0 && (
          <p role="status" className="viewing-past">
            <span>That run is no longer in this journey's history.</span>
            <button type="button" className="b ghost" onClick={() => view(null)}>{busy ? "Show the live run" : "Back to the latest run"}</button>
          </p>
        )}
        {history.length > 0 && (
          <span className="run-history" role="group" aria-label="History">
            <span className="section-label">HISTORY</span>
            {history.map((past, index) => {
              const chip = historyChip(past, stale);
              const on = viewed?.run === past.run;
              return <button type="button" key={past.run} aria-pressed={on} aria-label={`Open the run of ${past.ranAt ? when(past.ranAt).full || past.run : past.run}, ${chip.label}`} className={`b history-chip${index === 0 ? " newest" : ""}${on ? " on" : ""}`} title={chip.label} onClick={() => view(on ? null : past.run)}><b className={chip.tone}>{chip.mark}</b>{chip.tone === "idle" ? ` ${chip.label}` : ""} {past.ranAt ? <When at={past.ranAt} /> : past.run}</button>;
            })}
          </span>
        )}
        {steps.length > 0 && (
          <span className="run-segments" aria-hidden="true">{steps.map((step) => <span key={step.n} className={stale ? "not-run" : step.status.replace(" ", "-")} />)}</span>
        )}
      </section>
      {error !== null && <ActionError error={error} onDismiss={dismissError} />}
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
      {/* A selected history run describes the past; only the current run can authorize a decision
          on this version. Keep its approval controls hidden until returning to the current run. */}
      <DecisionBar runtime={runtime} flow={flow} runFinished={viewed === null && run?.state === "ready" && !isHeld(run) && !isWaitingReview(run)} stale={stale || viewed !== null} onDecided={onRan} />
      <div className="steps-card">
        <ol className="steps" aria-label="Steps">
          {steps.map((step) => (
            <li key={step.n} aria-label={`Step ${step.n}`} className={`step ${stale ? "not-run stale" : step.status.replace(" ", "-")}`}>
              <button type="button" className="b" aria-pressed={selected?.n === step.n} onClick={() => setPicked(step.n)}>
                <span className="step-thumb">{step.cli ? <CliThumb step={step} /> : thumb(step.screen) ? <><img src={thumb(step.screen)} alt={`Step ${step.n} frame`} /><CheckMarks step={step} aspect={THUMB_ASPECT} /></> : waitsForFrame(step, thumb(step.screen) !== undefined, shown, starting && viewed === null, stale) ? <FrameWaiting /> : <span>{step.frame ? "loading" : step.status === "precondition" ? "start" : "no frame"}</span>}</span>
                <span className="step-text">
                  <span className="step-head"><span className="mono">{step.n} DOES</span><span className="step-status">{stepStatusText(step.status, run, stale)}</span></span>
                  {stale && <span className="stale-mark" title="From the earlier version of this journey">stale</span>}
                  <span className="step-label" title={step.label}>{step.label}</span>
                  {viewportLine(step, stale)}
                </span>
              </button>
            </li>
          ))}
        </ol>
      </div>
      {selected !== null && (
        <CompareFrame runtime={runtime} flowId={flow.id} step={selected} steps={steps.length} now={frame} mobileNow={mobileFrames[selected.screen]} earlier={earlierRun(history, shown, selected.screen, viewed)} staleRun={stale} waiting={waitsForFrame(selected, thumb(selected.screen) !== undefined, shown, starting && viewed === null, stale)} />
      )}
    </div>
  );
}

/** #1298: the failed run's alert: the sentence, the code and step, the launcher's last lines
 * folded open, Copy details, and Send to fix when the run offers it. */
function FailureAlert({ notice, fix }: { notice: FailureNotice; fix: ReactNode }) {
  const [copied, setCopied] = useState(false);
  const copy = () => { void navigator.clipboard?.writeText(notice.details).then(() => setCopied(true), () => setCopied(false)); };
  return (
    <div role="alert" className="run-failure">
      <strong className="run-failure-sentence">{notice.sentence}</strong>
      {notice.code !== null && <span className="mono run-failure-code">{notice.code}</span>}
      {notice.log !== null && (
        <details open className="run-failure-log">
          <summary className="mono">{notice.log.label}, last {notice.log.lines.length} lines</summary>
          <pre className="mono">{notice.log.lines.join("\n")}</pre>
        </details>
      )}
      <div className="run-failure-actions">
        <button type="button" className="b ghost" onClick={copy}>Copy details</button>
        {copied && <span role="status" className="sent">Copied</span>}
        {fix}
      </div>
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
  const [refusal, setRefusal] = useState<ActionErrorDetails | null>(null);
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
      if (!(await find().catch(() => false))) setRefusal(actionErrorDetails("Request could not be sent", cause, "The Runtime did not record the request.", "Check the run's request list, then try again."));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="send-fix">
      <input aria-label="Note for the fix" placeholder="Note (optional)" maxLength={NOTE_MAX} value={note} disabled={busy} onChange={(event) => setNote(event.target.value)} />
      <button type="button" className="b ghost" disabled={busy} onClick={() => { void send(); }}>{busy ? "Sending…" : "Send to fix"}</button>
      {refusal !== null && <ActionError error={refusal} onDismiss={() => setRefusal(null)} />}
    </div>
  );
}

/** The owner's Approve and Reject on a run journey. Each act goes to its public route with the
 * session's owner token; the journeys are read again after it, so the status shown is the
 * Runtime's, and a refusal is shown as its text. */
function DecisionBar({ runtime, flow, runFinished, stale = false, onDecided }: { runtime: Runtime; flow: JourneyFlow; runFinished: boolean; stale?: boolean; onDecided: () => void }) {
  const [reason, setReason] = useState("");
  const [busy, setBusy] = useState(false);
  const [refusal, setRefusal] = useState<ActionErrorDetails | null>(null);
  const decision = decisionOf(flow);
  const act = (send: () => Promise<void>) => {
    setBusy(true);
    setRefusal(null);
    send().then(() => { setReason(""); onDecided(); }, (cause: unknown) => setRefusal(actionErrorDetails("Decision could not be recorded", cause, "The Runtime refused this decision.", "Check the journey state, then try again.")))
      .finally(() => setBusy(false));
  };
  const reject = () => {
    const checked = rejectNote(reason);
    if ("refusal" in checked) { setRefusal(actionErrorDetails("Decision could not be recorded", null, checked.refusal, "Correct the reason, then try Reject again.")); return; }
    act(() => runtime.rejectJourney(flow.id, checked.note));
  };
  return (
    <div className="decision-bar">
      {decision === "approved" && <span role="status" aria-label="Approved" className="chip pass">Approved</span>}
      {decision === "rejected" && <span role="status" aria-label="Rejected" className="chip fail">Rejected{flow.rejected?.note ? ` · ${flow.rejected.note}` : ""}</span>}
      {!stale && offersDecision(flow, runFinished) && (
        <>
          <button type="button" className="b act" disabled={busy} onClick={() => act(() => runtime.approveJourney(flow.id))}>Approve</button>
          <input aria-label="Reason for rejecting" placeholder="Why reject?" value={reason} disabled={busy} onChange={(event) => setReason(event.target.value)} />
          <button type="button" className="b ghost" disabled={busy} onClick={reject}>Reject</button>
        </>
      )}
      {refusal !== null && <ActionError error={refusal} onDismiss={() => setRefusal(null)} />}
    </div>
  );
}
