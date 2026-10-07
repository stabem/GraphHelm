/**
 * The Journey tab (spec 4.7, phase 5 rulings 1-5): one card per contract step, in order, each with
 * its last screenshot and whether the code moved under it; arrows say whether the step-to-step
 * walk was recorded. Images arrive only as `blob:` URLs and are drawn only in `<img>`.
 */
import { useState } from "react";

import type { BeforeAfterPair } from "../runtime/journeys";
import { captureAge } from "../runtime/journeys";
import type { ArrowView, CaptureUnknownCause, CaptureView, JourneysView, RuntimeEvent, StepAction, StepView } from "../runtime/types";
import { ago } from "./format";
import { useImageUrl } from "./use-image-url";

export interface JourneyCanvasProps {
  view: JourneysView | null;
  /** Shown when the view could not be read and there is nothing older to keep showing. */
  failed?: boolean;
  /** The Runtime's own message for the failed read, shown under the empty state. */
  failure?: string | null;
  contractId: string | null;
  onSelectContract: (contractId: string) => void;
  /** Reads an image; `executionId` names the run that holds it (#332), else the selected run. */
  loadImage: (evidenceId: string, executionId?: string) => Promise<Blob>;
  /** The run selected in the Studio; `events` are its records. Captures may come from any run. */
  executionId?: string;
  events: RuntimeEvent[];
  botName: (actorOrObserver: string) => string;
  beforeAfter: BeforeAfterPair[];
  onOpenRecords: (sequences: number[]) => void;
  /** Optional control of the open detail (the right panel's before/after rows open one). */
  detailStepId?: string | null;
  onDetailStepChange?: (stepId: string | null) => void;
}

export const UNKNOWN_CAUSE: Record<CaptureUnknownCause, string> = {
  dirty: "taken from uncommitted code",
  no_scope_paths: "screen has no scope paths",
  no_git: "no git history",
  revision_missing: "revision not in the repository",
};

function titleOf(step: StepView): string {
  return step.screen?.title ?? step.stepId;
}

/** #379: verbs for the contract's semantic action kinds, said as what the user does. */
const ACTION_VERB: Record<string, string> = {
  navigate: "Goes to",
  activate: "Clicks",
  enter_text: "Types into",
  select: "Chooses",
  upload: "Uploads a file to",
  download: "Downloads",
  wait_for: "Waits for",
  inspect: "Looks at",
  submit: "Submits",
  recover: "Recovers with",
  approve: "Approves",
};

/** One plain sentence for a step's action: `Clicks “Checkout”`, `Opens /cart`. */
export function actionText(action: StepAction): string {
  if (action.kind === "navigate" && action.strategy === "url") return `Opens ${action.target}`;
  const verb = ACTION_VERB[action.kind] ?? action.kind.replace(/_/g, " ");
  return `${verb} “${action.target}”`;
}

/** What the user does and must see on one step, above the capture's provenance. */
function Plain({ step }: { step: StepView }) {
  const states = step.expectedStates ?? [];
  return (
    <>
      {step.action ? <span className="journey-does"><span className="journey-label">Does</span> {actionText(step.action)}</span> : null}
      {step.promises.length > 0
        ? <span className="journey-sees"><span className="journey-label">Sees</span> {step.promises.join("; ")}</span>
        : states.length > 0 ? <span className="journey-sees"><span className="journey-label">Reaches</span> {states.join(", ")}</span> : null}
    </>
  );
}

function Shot({ evidenceId, executionId, loadImage, alt, className }: { evidenceId: string; executionId?: string; loadImage: (id: string, executionId?: string) => Promise<Blob>; alt: string; className: string }) {
  const { url, error } = useImageUrl(() => (executionId === undefined ? loadImage(evidenceId) : loadImage(evidenceId, executionId)), `${executionId ?? ""}/${evidenceId}`);
  if (url !== null) return <img className={className} src={url} alt={alt} />;
  return <span className="journey-shot-missing">{error ? "Image could not be read" : "Loading image…"}</span>;
}

function Freshness({ capture }: { capture: CaptureView }) {
  if (capture.freshness === "stale") {
    const [first, ...rest] = capture.changedFiles;
    return (
      <span className="journey-stale-note">
        <span className="journey-crack" aria-hidden="true" />
        Code changed after this shot
        {first !== undefined && <span className="journey-file"> · {first}{rest.length > 0 ? ` +${rest.length} more` : ""}</span>}
      </span>
    );
  }
  if (capture.freshness === "unknown") {
    const cause = capture.unknownCause === undefined ? null : UNKNOWN_CAUSE[capture.unknownCause];
    return <span className="journey-unknown-badge">Freshness unknown{cause === null ? "" : ` — ${cause}`}</span>;
  }
  return null;
}

function Arrow({ arrow, next }: { arrow: ArrowView | undefined; next: StepView }) {
  const state = arrow?.state ?? "never_walked";
  return (
    <div className="journey-arrow" data-state={state} aria-label={`Arrow: ${state.replace("_", " ")}`}>
      {next.action ? <span className="journey-arrow-act">{actionText(next.action)}</span> : null}
      <span className="journey-arrow-line" />
      {state !== "walked" && <span className="journey-arrow-label">{state === "stale" ? "stale" : "never walked"}</span>}
    </div>
  );
}

export function JourneyCanvas(props: JourneyCanvasProps) {
  const { view, contractId, onSelectContract, loadImage, events, botName, beforeAfter, onOpenRecords } = props;
  const [ownDetail, setOwnDetail] = useState<string | null>(null);
  const detailStepId = props.detailStepId !== undefined ? props.detailStepId : ownDetail;
  const setDetail = (stepId: string | null) => { setOwnDetail(stepId); props.onDetailStepChange?.(stepId); };

  if (view === null) {
    return <section className="journey-empty" aria-label="Journey"><p>{props.failed ? "Journeys could not be read." : "Loading journeys…"}</p>{props.failed && props.failure ? <p className="journey-failure" role="alert">{props.failure}</p> : null}</section>;
  }
  if (view.journeys.length === 0) {
    return <section className="journey-empty" aria-label="Journey"><p>No journeys mapped yet. A journey appears here once its screens are captured.</p></section>;
  }
  const journey = view.journeys.find((candidate) => candidate.contractId === contractId) ?? view.journeys[0];
  const detail = detailStepId === null ? undefined : journey.steps.find((step) => step.stepId === detailStepId);
  const pair = detail === undefined ? undefined
    : beforeAfter.find((candidate) => candidate.contractId === journey.contractId && candidate.stepId === detail.stepId);
  const detailCapture = detail?.capture ?? null;
  /** A capture recorded in another run than the selected one (#332): its records live there. */
  const fromOtherRun = (capture: CaptureView) => capture.executionId !== undefined && capture.executionId !== props.executionId;
  /** When the capture was taken: its own timestamp, else its record in the selected run. */
  const takenAt = (capture: CaptureView) => capture.recordedAt !== undefined ? Date.parse(capture.recordedAt)
    : captureAge(capture.sequence, events);

  return (
    <section className="journey-canvas" aria-label="Journey">
      {view.journeys.length > 1 && (
        <label className="journey-picker">Journey{" "}
          <select value={journey.contractId} onChange={(event) => { setDetail(null); onSelectContract(event.target.value); }}>
            {view.journeys.map((candidate) => <option key={candidate.contractId} value={candidate.contractId}>{candidate.title}</option>)}
          </select>
        </label>
      )}
      <h2 className="journey-title">{journey.title}</h2>
      <ol className="journey-steps">
        {journey.steps.map((step, index) => {
          const capture = step.capture ?? null;
          const state = capture === null ? "missing" : capture.freshness;
          const at = capture === null ? null : takenAt(capture);
          const next = journey.steps[index + 1];
          const arrow = next === undefined ? undefined
            : journey.arrows.find((candidate) => candidate.fromStepId === step.stepId && candidate.toStepId === next.stepId);
          return (
            <li key={step.stepId} className="journey-step">
              <button type="button" className="journey-card" data-state={state} aria-label={`${titleOf(step)}, open detail`} onClick={() => setDetail(step.stepId)}>
                {capture === null ? <span className="journey-shot-missing">Not captured yet</span> : (
                  <Shot evidenceId={capture.imageEvidenceId} executionId={capture.executionId} loadImage={loadImage} alt={`${titleOf(step)} screenshot`} className="journey-thumb" />
                )}
                <strong className="journey-card-title">{titleOf(step)}</strong>
                <Plain step={step} />
                {capture !== null && (
                  <span className="journey-meta">
                    {botName(capture.observer)} · {capture.revision.slice(0, 7)} · {at === null ? "age unknown" : ago(new Date(at).toISOString())}
                  </span>
                )}
                {capture !== null && <Freshness capture={capture} />}
              </button>
              {next !== undefined && <Arrow arrow={arrow} next={next} />}
            </li>
          );
        })}
      </ol>
      {detail !== undefined && (
        <div className="journey-detail" role="dialog" aria-label={`${titleOf(detail)} detail`}>
          <div className="journey-detail-head">
            <h3>{titleOf(detail)}</h3>
            <button type="button" onClick={() => setDetail(null)}>Close</button>
          </div>
          {detailCapture !== null ? (
            <Shot evidenceId={detailCapture.imageEvidenceId} executionId={detailCapture.executionId} loadImage={loadImage} alt={`${titleOf(detail)} full screenshot`} className="journey-full" />
          ) : <p className="journey-shot-missing">Not captured yet</p>}
          {detailCapture?.executionId !== undefined && (
            <p className="journey-source">Captured in run <code>{detailCapture.executionId}</code>{fromOtherRun(detailCapture) ? " (not the selected run)" : ""}</p>
          )}
          {detailCapture !== null && <Freshness capture={detailCapture} />}
          {detailCapture?.freshness === "stale" && detailCapture.changedFiles.length > 0 && (
            <ul className="journey-files" aria-label="Changed files">
              {detailCapture.changedFiles.map((file) => <li key={file}>{file}</li>)}
            </ul>
          )}
          {pair !== undefined && (
            <div className="journey-pair">
              <figure><figcaption>Before · PR #{pair.pr}</figcaption>
                <Shot evidenceId={pair.before.imageEvidenceId} loadImage={loadImage} alt="Before screenshot" className="journey-full" /></figure>
              <figure><figcaption>After · PR #{pair.pr}</figcaption>
                <Shot evidenceId={pair.after.imageEvidenceId} loadImage={loadImage} alt="After screenshot" className="journey-full" /></figure>
            </div>
          )}
          {((detailCapture !== null && !fromOtherRun(detailCapture)) || pair !== undefined) && (
            <button type="button" onClick={() => onOpenRecords([
              ...(detailCapture !== null && !fromOtherRun(detailCapture) ? [detailCapture.sequence] : []),
              ...(pair !== undefined ? [pair.before.sequence, pair.after.sequence] : []),
            ])}>Records</button>
          )}
          <h4>Promises</h4>
          {detail.promises.length === 0 ? <p className="journey-shot-missing">No promise text in the contract</p> : (
            <ul className="journey-promises">{detail.promises.map((text) => <li key={text}>{text}</li>)}</ul>
          )}
        </div>
      )}
    </section>
  );
}
