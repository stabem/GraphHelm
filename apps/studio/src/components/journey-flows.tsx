/**
 * Journey-flow review (#353, spec 2026-10-06-journey-explore-design.md §10 and §12 row 5): each
 * `.journey.yaml` flow with its status, drift and validate findings, its screens and edges drawn
 * like the journey canvas, and the owner's Approve. Approve calls `POST
 * /v1/journey-flows/{id}/approve`, the same path as `graphhelm journey approve`; it is offered only
 * when the Runtime says approval would be accepted, and a refusal shows the Runtime's own message.
 */
import { useEffect, useRef, useState } from "react";

import type { JourneyFlowView, JourneyFlowsView } from "../runtime/types";

export interface JourneyFlowsProps {
  view: JourneyFlowsView | null;
  /** The Runtime's message for a failed read; shown instead of the list. */
  failure?: string | null;
  onApprove: (flowId: string) => Promise<void>;
  /** The journey a task graph's chip named (#421): a flow id, or a path's contract id
   * `<flow>.<path>`. Its flow is marked current, focused and scrolled into view. */
  focusFlowId?: string | null;
}

const STATUS_LABEL: Record<JourneyFlowView["status"], string> = {
  draft: "Draft — waiting for your approval",
  approved: "Approved",
  approval_stale: "Approval stale — edited after it was approved",
  unreadable: "Unreadable",
};

function driftText(entry: unknown): string {
  if (entry !== null && typeof entry === "object") {
    const { edge, code } = entry as { edge?: unknown; code?: unknown };
    if (typeof edge === "string" || typeof code === "string") return [edge, code].filter((part) => typeof part === "string").join(" · ");
  }
  return JSON.stringify(entry);
}

function Flow({ flow, onApprove, current }: { flow: JourneyFlowView; onApprove: (flowId: string) => Promise<void>; current: boolean }) {
  const [approving, setApproving] = useState(false);
  const article = useRef<HTMLElement>(null);
  useEffect(() => {
    if (!current || article.current === null) return;
    article.current.scrollIntoView?.({ block: "start" });
    article.current.focus();
  }, [current]);
  const [error, setError] = useState<string | null>(null);
  const errors = flow.findings.filter((finding) => finding.severity === "error");
  const settled = flow.status === "approved" && !flow.approvable;
  const approve = () => {
    setApproving(true);
    setError(null);
    onApprove(flow.id).catch((cause: unknown) => setError(cause instanceof Error ? cause.message : String(cause))).finally(() => setApproving(false));
  };
  return (
    <article ref={article} className="journey-flow" data-status={flow.status} aria-label={`Flow ${flow.id}`}
      tabIndex={-1} aria-current={current ? "true" : undefined}>
      <h3 className="journey-title">{flow.title ?? flow.id}</h3>
      <p className="journey-flow-status">{STATUS_LABEL[flow.status]}</p>
      {flow.drift.length > 0 && (
        <ul className="journey-files" aria-label="Drift">{flow.drift.map((entry, index) => <li key={index}>{driftText(entry)}</li>)}</ul>
      )}
      {flow.findings.length > 0 && (
        <ul className="journey-files" aria-label="Findings">
          {flow.findings.map((finding, index) => <li key={index}><code>{finding.code}</code> {finding.pointer} — {finding.message}</li>)}
        </ul>
      )}
      <ol className="journey-steps">
        {flow.screens.map((screenView) => (
          <li key={screenView.id} className="journey-step">
            <div className="journey-card" data-state={screenView.state === "success" ? "fresh" : "missing"}>
              <strong className="journey-card-title">{screenView.id}</strong>
              <span className="journey-meta">{screenView.url}</span>
            </div>
            {flow.edges.filter((edge) => edge.from === screenView.id).map((edge) => (
              <div key={edge.id} className="journey-arrow" data-state="never_walked" aria-label={`Edge ${edge.id}`}>
                <span className="journey-arrow-line" />
                <span className="journey-arrow-label">{edge.id} → {edge.to}</span>
              </div>
            ))}
          </li>
        ))}
      </ol>
      {!settled && (
        <div className="journey-flow-approve">
          <button type="button" disabled={!flow.approvable || approving} onClick={approve}>{approving ? "Approving…" : "Approve"}</button>
          {!flow.approvable && errors.length > 0 && (
            <p className="journey-failure">Fix before approving: {errors.map((finding) => finding.code).join(", ")}</p>
          )}
          {error !== null && <p className="journey-failure" role="alert">{error}</p>}
        </div>
      )}
    </article>
  );
}

/** The flow a journey id belongs to: itself, or the longest flow id it extends with `.<path>`. */
function flowOf(flows: JourneyFlowView[], journeyId: string | null | undefined): string | null {
  if (!journeyId) return null;
  const owners = flows.filter((flow) => journeyId === flow.id || journeyId.startsWith(`${flow.id}.`));
  return owners.sort((a, b) => b.id.length - a.id.length)[0]?.id ?? null;
}

export function JourneyFlows({ view, failure = null, onApprove, focusFlowId = null }: JourneyFlowsProps) {
  if (view === null) {
    return failure === null ? null : <section className="journey-flows" aria-label="Journey flows"><p className="journey-failure" role="alert">{failure}</p></section>;
  }
  if (view.flows.length === 0) return null;
  const focused = flowOf(view.flows, focusFlowId);
  return (
    <section className="journey-flows" aria-label="Journey flows">
      <h2>Journey flows</h2>
      {view.flows.map((flow) => <Flow key={flow.id} flow={flow} onApprove={onApprove} current={flow.id === focused} />)}
    </section>
  );
}
