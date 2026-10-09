/**
 * The owner's journey approval screen (#353, reshaped by #465): a compact list of the project's
 * journey flows, and the selected one as a flowchart of its screens (#519) — each card the screen
 * as the journey's run rendered it, how that step fared, what it Does and what it Sees — with Watch
 * (#462: the journey plays while its current step lights here and shows the live page) and Approve
 * (`POST /v1/journey-flows/{id}/approve`, the same path as `graphhelm journey approve`). Approve is
 * offered only when the Runtime says it would be accepted. Finding codes, pointers and URLs are the
 * agent's business; the owner reads what to do in words.
 */
import { useEffect, useRef, useState } from "react";

import type { JourneyFlowEdge, JourneyFlowScreen, JourneyFlowView, JourneyFlowsView, JourneyFrame, JourneyRunView, LiveSession } from "../runtime/types";
import { JourneyFlowchart, pathsOf } from "./journey-flowchart";

/** Where a journey's run and its pictures come from (#519): the Runtime client's four reads. */
export interface JourneyRunSource {
  /** Start the run, or read the cached one; `force` is Run again. */
  start: (flowId: string, force: boolean) => Promise<JourneyRunView>;
  read: (flowId: string) => Promise<JourneyRunView>;
  screenFrame: (flowId: string, screenId: string) => Promise<JourneyFrame | null>;
  liveFrame: (sessionId: string, etag: string | null) => Promise<JourneyFrame | null>;
}

export interface JourneyFlowsProps {
  view: JourneyFlowsView | null;
  /** The Runtime's message for a failed read; shown instead of the list. */
  failure?: string | null;
  onApprove: (flowId: string) => Promise<void>;
  /** The journey a task graph's chip named (#421): a flow id, or a path's contract id
   * `<flow>.<path>`. Its flow is selected and focused. */
  focusFlowId?: string | null;
  /** The live sessions the Runtime holds; a `mode: "watch"` row lights the step being played. */
  sessions?: LiveSession[] | null;
  /** Play the flow in a headed browser (#462). Resolves when the play ends. Absent: no Watch. */
  onWatch?: (flowId: string, path?: string) => Promise<void>;
  /** The journey the owner selected (and its status), so the proof map below follows it. */
  onSelect?: (flowId: string, status: JourneyFlowView["status"]) => void;
  /** Opening a journey runs its test and shows each screen as it was rendered (#519). Absent: the
   * flowchart is drawn without pictures or results. */
  run?: JourneyRunSource;
}

/** A title's trailing parenthetical is a note for the reader (`Name (draft: why)`), not the name. */
export function splitTitle(title: string): { name: string; note: string | null } {
  const match = /^(.*\S)\s*\(([^()]*)\)\s*$/.exec(title);
  if (match === null) return { name: title, note: null };
  const note = match[2]!.replace(/^draft\s*[:—-]\s*/i, "").trim();
  return { name: match[1]!, note: note === "" ? null : note };
}

const STATUS_LABEL: Record<JourneyFlowView["status"], string> = {
  draft: "Waiting for your approval",
  approved: "Approved",
  approval_stale: "Changed since you approved it",
  unreadable: "Can't be read",
};

interface Step { screen: JourneyFlowScreen; arrivedBy: JourneyFlowEdge | null }

/** One path as steps: its first screen, then each edge's destination with the edge that leads
 * there. A flow without that path lists its screens. */
function stepsOf(flow: JourneyFlowView, path = "main"): Step[] {
  const byId = new Map(flow.screens.map((screenView) => [screenView.id, screenView]));
  const edges = new Map(flow.edges.map((edge) => [edge.id, edge]));
  const main = (flow.paths[path] ?? []).map((id) => edges.get(id)).filter((edge): edge is JourneyFlowEdge => edge !== undefined);
  if (main.length === 0) return flow.screens.map((screenView) => ({ screen: screenView, arrivedBy: null }));
  const first = byId.get(main[0]!.from);
  const steps: Step[] = first ? [{ screen: first, arrivedBy: null }] : [];
  for (const edge of main) {
    const to = byId.get(edge.to);
    if (to) steps.push({ screen: to, arrivedBy: edge });
  }
  return steps;
}

/** The step a watch is on: the destination of the edge being played, else the screen just seen. */
function currentStep(steps: Step[], session: LiveSession | undefined): number {
  if (!session) return -1;
  if (typeof session.edge === "string") return steps.findIndex((step) => step.arrivedBy?.id === session.edge);
  const seen = typeof session.screen === "string" ? session.screen : session.stepId;
  return steps.findIndex((step) => step.screen.id === seen);
}

const RUN_POLL_MS = 1000;
const LIVE_FRAME_MS = 200;

/** Why a run could not start or finish, in words; the Runtime's closed list of codes picks them. */
const RUN_REASON: Record<string, string> = {
  "watch.app_down": "the app it opens isn't running",
  "watch.launcher_invalid": "the project's app launcher isn't set up right",
  "driver.observer_missing": "the browser player isn't installed in this project yet",
  "preview.busy": "another run is still going",
  "preview.budget_exceeded": "it took longer than allowed",
};

function runWords(run: JourneyRunView): string {
  if (run.state === "running") return "Running this journey's test…";
  if (run.state === "failed") return `Couldn't run this journey's test: ${RUN_REASON[run.reason ?? ""] ?? "something went wrong in the Runtime"}.`;
  if (run.state !== "ready") return "This journey's test has not run yet.";
  const result = run.result === "pass" ? "Test passed" : run.result === "drift" ? "The app no longer matches this journey" : run.result === "fail" ? "Test failed" : "Test ran";
  return run.kind === "preview" ? `${result} (a preview: a draft's run is never proof)` : result;
}

/** The journey's run (#519): started when the journey is opened, read once a second while it
 * runs, and each reached screen's picture fetched once per result. Object URLs are revoked when a
 * picture is replaced and when the journey is left. A Runtime that answers 404 predates the run
 * routes: `offered` turns false and the flowchart is drawn without a run line. */
function useJourneyRun(flowId: string, source: JourneyRunSource | undefined): { offered: boolean; run: JourneyRunView | null; frames: Record<string, string>; failure: string | null; again: () => void } {
  const [run, setRun] = useState<JourneyRunView | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const [absent, setAbsent] = useState(false);
  const [frames, setFrames] = useState<Record<string, string>>({});
  const [attempt, setAttempt] = useState(0);
  const held = useRef(new Map<string, { key: string; url: string | null }>());
  useEffect(() => {
    if (source === undefined) return undefined;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const fail = (cause: unknown) => {
      if (cancelled) return;
      if (typeof cause === "object" && cause !== null && (cause as { httpStatus?: unknown }).httpStatus === 404) setAbsent(true);
      else setFailure(cause instanceof Error ? cause.message : String(cause));
    };
    const take = (next: JourneyRunView) => {
      if (cancelled) return;
      setRun(next);
      setFailure(null);
      if (next.state === "running") timer = setTimeout(() => { source.read(flowId).then(take, fail); }, RUN_POLL_MS);
    };
    source.start(flowId, attempt > 0).then(take, fail);
    return () => { cancelled = true; if (timer !== undefined) clearTimeout(timer); };
  }, [flowId, source, attempt]);
  useEffect(() => {
    if (source === undefined || run === null) return;
    for (const [screenId, screen] of Object.entries(run.screens ?? {})) {
      if (!screen.frame) continue;
      const key = `${run.digest ?? ""}:${run.commit ?? ""}:${attempt}:${screen.result ?? ""}`;
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
  return { offered: source !== undefined && !absent, run, frames, failure, again: () => setAttempt((before) => before + 1) };
}

/** The page a Watch is playing (#519): read five times a second while it plays, and once more
 * when it stops, because the Runtime keeps the last frame until the session closes. */
function useLiveFrame(session: LiveSession | undefined, source: JourneyRunSource | undefined): string | null {
  const [url, setUrl] = useState<string | null>(null);
  const sessionId = session?.frame === true ? session.sessionId : null;
  const playing = session?.state === "playing";
  useEffect(() => {
    if (source === undefined || sessionId === null) { setUrl(null); return undefined; }
    let cancelled = false;
    let busy = false;
    let etag: string | null = null;
    const tick = () => {
      if (busy) return;
      busy = true;
      source.liveFrame(sessionId, etag).then((frame) => {
        if (cancelled || frame === null) return;
        etag = frame.etag;
        setUrl(URL.createObjectURL(frame.blob));
      }, () => { /* the next tick retries */ }).finally(() => { busy = false; });
    };
    tick();
    const timer = playing ? setInterval(tick, LIVE_FRAME_MS) : undefined;
    return () => { cancelled = true; if (timer !== undefined) clearInterval(timer); };
  }, [source, sessionId, playing]);
  useEffect(() => () => { if (url !== null) URL.revokeObjectURL(url); }, [url]);
  return url;
}

function watchWords(session: LiveSession, steps: Step[], current: number): string {
  const total = session.stepCount ?? steps.length;
  const at = current >= 0 ? current + 1 : (session.stepIndex ?? 0) + 1;
  switch (session.state) {
    case "playing": return `Playing step ${at} of ${total}…`;
    case "pass": return "Played to the end — everything it should show was there.";
    case "drift": return `Stopped at step ${at}: the app no longer matches this step.`;
    case "fail": return `Stopped at step ${at}: something it should show was not there.`;
    default: return `Step ${at} of ${total}`;
  }
}

/** Why a Watch could not start, in words the owner can act on; the Runtime's code picks them. */
function watchFailure(cause: unknown): string {
  const code = typeof cause === "object" && cause !== null && "code" in cause ? String((cause as { code: unknown }).code) : "";
  const message = cause instanceof Error ? cause.message : String(cause);
  if (code.endsWith("observer_missing")) return "Can't play this journey: the browser player isn't installed in this project yet.";
  if (code === "driver.host_refused" || code.endsWith("app_unreachable") || code.endsWith("app_down") || code.endsWith("launch_failed")) return "Can't play this journey: the app it opens isn't running.";
  if (code.endsWith("busy") || code.endsWith("session_busy")) return "Can't play this journey right now: another play is still running.";
  return `Can't play this journey: ${message}`;
}

function Detail({ flow, onApprove, onWatch, session, source }: { flow: JourneyFlowView; onApprove: (flowId: string) => Promise<void>; onWatch?: (flowId: string, path?: string) => Promise<void>; session?: LiveSession; source?: JourneyRunSource }) {
  const { offered, run, frames, failure: runFailure, again } = useJourneyRun(flow.id, source);
  const liveFrame = useLiveFrame(session, source);
  const [approving, setApproving] = useState(false);
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [watchError, setWatchError] = useState<{ text: string; path?: string } | null>(null);
  const settled = flow.status === "approved" && !flow.approvable;
  const blocked = !flow.approvable && flow.findings.some((finding) => finding.severity === "error");
  const title = splitTitle(flow.title ?? flow.id);
  const paths = pathsOf(flow);
  const playing = session?.state === "playing";
  // The path a Watch is playing, as steps, to say "step 2 of 3" and to light its card.
  const watched = stepsOf(flow, session?.path || "main");
  const current = currentStep(watched, session);
  const approve = () => {
    setApproving(true);
    setError(null);
    onApprove(flow.id).catch((cause: unknown) => setError(cause instanceof Error ? cause.message : String(cause))).finally(() => setApproving(false));
  };
  const watch = (path?: string) => {
    if (!onWatch) return;
    setStarting(true);
    setWatchError(null);
    (path === undefined ? onWatch(flow.id) : onWatch(flow.id, path))
      .catch((cause: unknown) => setWatchError({ text: watchFailure(cause), ...(path === undefined ? {} : { path }) }))
      .finally(() => setStarting(false));
  };
  return (
    <article className="journey-flow" data-status={flow.status} aria-label={`Journey ${title.name}`}>
      <h3 className="journey-title">{title.name}</h3>
      <p className="journey-flow-status">{STATUS_LABEL[flow.status]}</p>
      {title.note !== null && <p className="journey-flow-note">Why it is a draft: {title.note}</p>}
      <div className="journey-flow-approve">
        {onWatch && (
          <button type="button" onClick={() => watch()} disabled={starting || playing}>{starting || playing ? "Playing…" : "Watch"}</button>
        )}
        {!settled && <button type="button" disabled={!flow.approvable || approving} onClick={approve}>{approving ? "Approving…" : "Approve"}</button>}
      </div>
      {/* Next to the buttons, never below a long step list the owner would have to scroll to. */}
      {watchError !== null && (
        <div className="journey-flow-watch-failure" role="alert">
          <p>{watchError.text}</p>
          <button type="button" onClick={() => watch(watchError.path)} disabled={starting || playing}>Retry</button>
        </div>
      )}
      {error !== null && <p className="journey-failure" role="alert">{error}</p>}
      {flow.drift.length > 0 && <p className="journey-flow-note">The app changed since this journey was recorded; watch it to see where.</p>}
      {blocked && <p className="journey-flow-note">The agent still has to fix this journey before you can approve it.</p>}
      {offered && (
        <div className="journey-flow-run">
          <p role="status" data-state={run?.state} data-result={run?.result}>
            {runFailure !== null ? `Couldn't run this journey's test: ${runFailure}` : run === null ? "Starting this journey's test…" : runWords(run)}
            {run?.state === "ready" && run.ranAt !== undefined && <> · ran <time dateTime={run.ranAt}>{new Date(run.ranAt).toLocaleString()}</time></>}
          </p>
          <button type="button" onClick={again} disabled={run?.state === "running" || (run === null && runFailure === null)}>Run again</button>
        </div>
      )}
      {paths.filter((path) => path !== "main").map((path) => (
        <div key={path} className="journey-flow-path-head">
          <h4>Also approved: the “{path}” way</h4>
          {onWatch && <button type="button" onClick={() => watch(path)} disabled={starting || playing}>{`Watch “${path}”`}</button>}
        </div>
      ))}
      <JourneyFlowchart flow={flow} run={run} frames={frames} current={watched[current]?.screen.id ?? null} liveFrame={liveFrame} />
      {session !== undefined && <p className="journey-flow-watch" role="status">{watchWords(session, watched, current)}</p>}
    </article>
  );
}

/** The flow a journey id belongs to: itself, or the longest flow id it extends with `.<path>`. */
function flowOf(flows: JourneyFlowView[], journeyId: string | null | undefined): string | null {
  if (!journeyId) return null;
  const owners = flows.filter((flow) => journeyId === flow.id || journeyId.startsWith(`${flow.id}.`));
  return owners.sort((a, b) => b.id.length - a.id.length)[0]?.id ?? null;
}

export function JourneyFlows({ view, failure = null, onApprove, focusFlowId = null, sessions = null, onWatch, onSelect, run }: JourneyFlowsProps) {
  const focused = view === null ? null : flowOf(view.flows, focusFlowId);
  const [picked, setPicked] = useState<string | null>(null);
  const detail = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (focused === null) return;
    setPicked(focused);
    detail.current?.scrollIntoView?.({ block: "start" });
    detail.current?.focus();
  }, [focused]);
  const chosen = view === null || view.flows.length === 0 ? null
    : view.flows.find((flow) => flow.id === picked) ?? view.flows.find((flow) => flow.approvable) ?? view.flows[0]!;
  useEffect(() => {
    if (chosen !== null) onSelect?.(chosen.id, chosen.status);
  }, [chosen?.id, chosen?.status, onSelect]);
  if (view === null) {
    return failure === null ? null : <section className="journey-flows" aria-label="Journey flows"><p className="journey-failure" role="alert">{failure}</p></section>;
  }
  if (view.flows.length === 0) return null;
  const waiting = view.flows.find((flow) => flow.approvable);
  const selectedId = view.flows.some((flow) => flow.id === picked) ? picked : (waiting ?? view.flows[0]!).id;
  const selected = view.flows.find((flow) => flow.id === selectedId)!;
  const watching = (sessions ?? []).find((session) => session.mode === "watch" && session.flowId === selected.id);
  return (
    <section className="journey-flows" aria-label="Journey flows">
      <h2>Journeys to approve</h2>
      <ul className="journey-flow-list" aria-label="Journeys">
        {view.flows.map((flow) => (
          <li key={flow.id}>
            <button type="button" className="journey-flow-row" aria-pressed={flow.id === selectedId} data-status={flow.status} onClick={() => setPicked(flow.id)}>
              <span className="journey-flow-row-title">{splitTitle(flow.title ?? flow.id).name}</span>
              <span className="journey-flow-row-status">{STATUS_LABEL[flow.status]}</span>
              <span className="journey-flow-row-steps">{stepsOf(flow).length} steps{pathsOf(flow).length > 1 ? ` · ${pathsOf(flow).length} ways` : ""}</span>
            </button>
          </li>
        ))}
      </ul>
      <div ref={detail} tabIndex={-1} className="journey-flow-detail">
        <Detail key={selected.id} flow={selected} onApprove={onApprove} {...(onWatch ? { onWatch } : {})} {...(watching ? { session: watching } : {})} {...(run ? { source: run } : {})} />
      </div>
    </section>
  );
}
