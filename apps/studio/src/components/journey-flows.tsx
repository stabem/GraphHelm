/**
 * The owner's journey approval screen (#353, reshaped by #465): a compact list of the project's
 * journey flows, and the selected one as plain steps — what it Does and what it Sees — with Watch
 * (#462: a headed browser plays the journey while its current step lights here) and Approve
 * (`POST /v1/journey-flows/{id}/approve`, the same path as `graphhelm journey approve`). Approve is
 * offered only when the Runtime says it would be accepted. Finding codes, pointers and URLs are the
 * agent's business; the owner reads what to do in words.
 */
import { useEffect, useRef, useState } from "react";

import type { JourneyFlowEdge, JourneyFlowScreen, JourneyFlowView, JourneyFlowsView, LiveSession } from "../runtime/types";

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

const VERB: Record<string, string> = {
  activate: "Clicks",
  submit: "Submits with",
  navigate: "Follows",
  enter_text: "Fills in",
  wait_for: "Waits for",
  inspect: "Looks at",
};

const ROLE: Record<string, string> = { textbox: "field", heading: "heading", button: "button", link: "link" };

function actWords(act: { kind: string; name: string }): string {
  return `${VERB[act.kind] ?? act.kind.replace(/_/g, " ")} “${act.name}”`;
}

function seesWords(screen: JourneyFlowScreen): string {
  const expect = screen.expect ?? [];
  if (expect.length === 0) return screen.title ?? screen.id;
  return expect.map((item) => `the “${item.name}” ${ROLE[item.role] ?? item.role}`).join(", ");
}

interface Step { screen: JourneyFlowScreen; arrivedBy: JourneyFlowEdge | null }

/** The flow's paths, `main` first: Approve approves every one of them, so every one is shown. */
function pathsOf(flow: JourneyFlowView): string[] {
  const names = Object.keys(flow.paths);
  return [...names.filter((name) => name === "main"), ...names.filter((name) => name !== "main").sort()];
}

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

function Detail({ flow, onApprove, onWatch, session }: { flow: JourneyFlowView; onApprove: (flowId: string) => Promise<void>; onWatch?: (flowId: string, path?: string) => Promise<void>; session?: LiveSession }) {
  const [approving, setApproving] = useState(false);
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [watchError, setWatchError] = useState<{ text: string; path?: string } | null>(null);
  const settled = flow.status === "approved" && !flow.approvable;
  const blocked = !flow.approvable && flow.findings.some((finding) => finding.severity === "error");
  const title = splitTitle(flow.title ?? flow.id);
  const paths = pathsOf(flow);
  const playing = session?.state === "playing";
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
      {(paths.length === 0 ? ["main"] : paths).map((path) => {
        const steps = stepsOf(flow, path);
        const here = session !== undefined && (session.path || "main") === path ? session : undefined;
        const current = currentStep(steps, here);
        return (
          <div key={path} className="journey-flow-path">
            {path !== "main" && (
              <div className="journey-flow-path-head">
                <h4>Also approved: the “{path}” way</h4>
                {onWatch && <button type="button" onClick={() => watch(path)} disabled={starting || playing}>{`Watch “${path}”`}</button>}
              </div>
            )}
            <ol className="journey-flow-steps" aria-label={path === "main" ? "Steps" : `Steps: ${path}`}>
              {steps.map((step, index) => (
                <li key={step.screen.id} className="journey-flow-step" aria-current={index === current ? "step" : undefined}>
                  <span className="journey-flow-step-number">{index + 1}</span>
                  <span className="journey-flow-step-text">
                    {step.arrivedBy && (step.arrivedBy.acts ?? []).length > 0 && (
                      <span className="journey-does"><span className="journey-label">Does</span> {(step.arrivedBy.acts ?? []).map(actWords).join(", then ")}</span>
                    )}
                    <span className="journey-sees"><span className="journey-label">Sees</span> {seesWords(step.screen)}</span>
                  </span>
                </li>
              ))}
            </ol>
            {here && <p className="journey-flow-watch" role="status">{watchWords(here, steps, current)}</p>}
          </div>
        );
      })}
    </article>
  );
}

/** The flow a journey id belongs to: itself, or the longest flow id it extends with `.<path>`. */
function flowOf(flows: JourneyFlowView[], journeyId: string | null | undefined): string | null {
  if (!journeyId) return null;
  const owners = flows.filter((flow) => journeyId === flow.id || journeyId.startsWith(`${flow.id}.`));
  return owners.sort((a, b) => b.id.length - a.id.length)[0]?.id ?? null;
}

export function JourneyFlows({ view, failure = null, onApprove, focusFlowId = null, sessions = null, onWatch, onSelect }: JourneyFlowsProps) {
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
        <Detail key={selected.id} flow={selected} onApprove={onApprove} {...(onWatch ? { onWatch } : {})} {...(watching ? { session: watching } : {})} />
      </div>
    </section>
  );
}
