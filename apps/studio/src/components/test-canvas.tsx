import { useState } from "react";
import "./test-canvas.css";
import type { FrameStatus, TestFrame } from "../runtime/test-frames";

const LABEL: Record<FrameStatus, string> = { passed: "passed", failed: "failed", waits_for_you: "waits for you", not_run: "not run" };
const MARK: Record<FrameStatus, string> = { passed: "✓", failed: "✕", waits_for_you: "!", not_run: "○" };

interface Props {
  frames: TestFrame[];
  selected: number;
  onSelect(n: number): void;
  frameUrl(stepId: string): string | null;
  /** May return a promise; a rejection is shown beside the decision buttons. */
  onMarkSafe(stepId: string): void | Promise<void>;
  /** Absent: no route sends a step back yet, so the button is shown disabled with that reason. */
  onSendBack?: (stepId: string) => void;
}

export function TestCanvas({ frames, selected, onSelect, frameUrl, onMarkSafe, onSendBack }: Props) {
  const cur = frames[selected];
  const [failure, setFailure] = useState<{ stepId: string; message: string } | null>(null);
  if (!cur) return <p className="tc-empty">This journey has no steps to test</p>;
  const src = frameUrl(cur.stepId);
  const count = (s: FrameStatus) => frames.filter((f) => f.status === s).length;
  return (
    <div className="tc">
      <div className="tc-head">
        <h2>{cur.text}</h2>
        <span className="tc-counts">
          <span><b data-status="passed">{count("passed")}</b> passed</span>
          <span><b data-status="waits_for_you">{count("waits_for_you")}</b> waits for you</span>
          <span><b data-status="failed">{count("failed")}</b> failed</span>
          <span><b data-status="not_run">{count("not_run")}</b> not run</span>
        </span>
      </div>
      <ol className="tc-strip" aria-label="Test actions">
        {frames.map((f, i) => (
          <li key={f.stepId} className="tc-strip-item">
            <button type="button" className="tc-card" data-status={f.status} aria-pressed={i === selected} onClick={() => onSelect(i)}
              aria-label={`${f.n} ${f.verb} ${f.text} · ${LABEL[f.status]}`}>
              <span className="tc-thumb" aria-hidden="true"><span className="tc-thumb-line" /><span className="tc-thumb-hl" data-status={f.status} /><span className="tc-thumb-line" /></span>
              <span className="tc-card-body">
                <span className="tc-card-top">
                  <span className="tc-num">{f.n}</span>
                  <span className="tc-verb">{f.verb}</span>
                  <span className="tc-chip" data-status={f.status}>{LABEL[f.status]}</span>
                </span>
                <span className="tc-card-text">{f.text}</span>
              </span>
            </button>
          </li>
        ))}
      </ol>
      <div className="tc-main">
        <section className="tc-browser" aria-label="Emulated browser">
          <div className="tc-chrome">
            <span className="tc-dots" aria-hidden="true"><i /><i /><i /></span>
            <button type="button" aria-label="Previous frame" onClick={() => onSelect(Math.max(0, selected - 1))}>‹</button>
            <button type="button" aria-label="Next frame" onClick={() => onSelect(Math.min(frames.length - 1, selected + 1))}>›</button>
            <span className="tc-url">
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="#8D929C" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                <rect x="5" y="11" width="14" height="10" rx="2" /><path d="M8 11V7a4 4 0 0 1 8 0v4" />
              </svg>
              <span className="tc-url-text">{cur.text}</span>
            </span>
            <span className="tc-count">{`emulated · frame ${cur.n}/${frames.length}`}</span>
          </div>
          <div className="tc-viewport" data-status={cur.status}>
            {src ? <img src={src} alt={`Frame ${cur.n}: ${cur.text}`} /> : <p>No frame recorded for this step</p>}
          </div>
          <div className="tc-status">
            <span className="tc-chip" data-status={cur.status}>{LABEL[cur.status]}</span>
            <span className="tc-status-verb">{cur.verb.toLowerCase()}</span>
            <span className="tc-status-text">{cur.text}</span>
          </div>
        </section>
        <aside className="tc-inspector" aria-label="Action inspector">
          <div className="tc-ins-block">
            <span className="tc-kicker">{`ACTION ${cur.n} · ${cur.verb}`}</span>
            <h3>{cur.text}</h3>
            <code className="tc-code">{`step ${cur.stepId}\n${cur.verb.toLowerCase()} ${cur.text}`}</code>
          </div>
          <div className="tc-ins-block">
            <span className="tc-cap">ASSERTIONS</span>
            {cur.expected.length > 0 ? (
              <ul aria-label="Assertions" className="tc-asserts">
                {cur.expected.map((e) => <li key={e}><span aria-hidden="true" className="tc-mark" data-status={cur.status}>{MARK[cur.status]}</span> {e}</li>)}
              </ul>
            ) : <p className="tc-muted">No assertion written for this step</p>}
            {cur.reason && <p className="tc-reason">{cur.reason}</p>}
          </div>
          <div className="tc-ins-block">
            <span className="tc-cap">CONSOLE</span>
            <p className="tc-muted">No console captured</p>
          </div>
          <div className="tc-ins-foot">
            <div className="tc-scrub" aria-label="Frames">
              {frames.map((f, i) => (
                <button key={f.stepId} type="button" className="tc-tick" data-status={f.status} aria-pressed={i === selected}
                  aria-label={`Jump to frame ${f.n}`} onClick={() => onSelect(i)} />
              ))}
            </div>
            <span className="tc-muted">click a frame to jump</span>
            {cur.status === "waits_for_you" && (
              <div className="tc-decide">
                <button type="button" className="tc-primary" onClick={() => {
                  const stepId = cur.stepId;
                  setFailure(null);
                  Promise.resolve().then(() => onMarkSafe(stepId)).catch((cause: unknown) => {
                    setFailure({ stepId, message: cause instanceof Error ? cause.message : String(cause) });
                  });
                }}>I watched it — mark safe</button>
                {onSendBack
                  ? <button type="button" className="tc-secondary" onClick={() => onSendBack(cur.stepId)}>Send back</button>
                  : <button type="button" className="tc-secondary" disabled>Send back — not available yet</button>}
                {failure?.stepId === cur.stepId && <p role="alert">{`Mark safe failed: ${failure.message}`}</p>}
              </div>
            )}
          </div>
        </aside>
      </div>
    </div>
  );
}
