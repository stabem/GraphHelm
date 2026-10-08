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
  onWatch?: (flowId: string) => Promise<void>;
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

/** The main path as steps: its first screen, then each edge's destination with the edge that leads
 * there. A flow without a main path lists its screens. */
function stepsOf(flow: JourneyFlowView): Step[] {
  const byId = new Map(flow.screens.map((screenView) => [screenView.id, screenView]));
  const edges = new Map(flow.edges.map((edge) => [edge.id, edge]));
  const main = (flow.paths.main ?? []).map((id) => edges.get(id)).filter((edge): edge is JourneyFlowEdge => edge !== undefined);
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

function Detail({ flow, onApprove, onWatch, session }: { flow: JourneyFlowView; onApprove: (flowId: string) => Promise<void>; onWatch?: (flowId: string) => Promise<void>; session?: LiveSession }) {
  const [approving, setApproving] = useState(false);
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const settled = flow.status === "approved" && !flow.approvable;
  const blocked = !flow.approvable && flow.findings.some((finding) => finding.severity === "error");
  const steps = stepsOf(flow);
  const current = currentStep(steps, session);
  const playing = session?.state === "playing";
  const approve = () => {
    setApproving(true);
    setError(null);
    onApprove(flow.id).catch((cause: unknown) => setError(cause instanceof Error ? cause.message : String(cause))).finally(() => setApproving(false));
  };
  const watch = () => {
    if (!onWatch) return;
    setStarting(true);
    setError(null);
    onWatch(flow.id).catch((cause: unknown) => setError(cause instanceof Error ? cause.message : String(cause))).finally(() => setStarting(false));
  };
  return (
    <article className="journey-flow" data-status={flow.status} aria-label={`Journey ${flow.title ?? flow.id}`}>
      <h3 className="journey-title">{flow.title ?? flow.id}</h3>
      <p className="journey-flow-status">{STATUS_LABEL[flow.status]}</p>
      <div className="journey-flow-approve">
        {onWatch && (
          <button type="button" onClick={watch} disabled={starting || playing}>{starting || playing ? "Playing…" : "Watch"}</button>
        )}
        {!settled && <button type="button" disabled={!flow.approvable || approving} onClick={approve}>{approving ? "Approving…" : "Approve"}</button>}
      </div>
      {flow.drift.length > 0 && <p className="journey-flow-note">The app changed since this journey was recorded; watch it to see where.</p>}
      {blocked && <p className="journey-flow-note">The agent still has to fix this journey before you can approve it.</p>}
      <ol className="journey-flow-steps" aria-label="Steps">
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
      {session && <p className="journey-flow-watch" role="status">{watchWords(session, steps, current)}</p>}
      {error !== null && <p className="journey-failure" role="alert">{error}</p>}
    </article>
  );
}

/** The flow a journey id belongs to: itself, or the longest flow id it extends with `.<path>`. */
function flowOf(flows: JourneyFlowView[], journeyId: string | null | undefined): string | null {
  if (!journeyId) return null;
  const owners = flows.filter((flow) => journeyId === flow.id || journeyId.startsWith(`${flow.id}.`));
  return owners.sort((a, b) => b.id.length - a.id.length)[0]?.id ?? null;
}

export function JourneyFlows({ view, failure = null, onApprove, focusFlowId = null, sessions = null, onWatch }: JourneyFlowsProps) {
  const focused = view === null ? null : flowOf(view.flows, focusFlowId);
  const [picked, setPicked] = useState<string | null>(null);
  const detail = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (focused === null) return;
    setPicked(focused);
    detail.current?.scrollIntoView?.({ block: "start" });
    detail.current?.focus();
  }, [focused]);
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
              <span className="journey-flow-row-title">{flow.title ?? flow.id}</span>
              <span className="journey-flow-row-status">{STATUS_LABEL[flow.status]}</span>
              <span className="journey-flow-row-steps">{stepsOf(flow).length} steps</span>
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
