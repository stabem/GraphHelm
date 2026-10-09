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
  return (
    <div className="tc">
      <ol className="tc-strip" aria-label="Test actions">
        {frames.map((f, i) => (
          <li key={f.stepId}>
            <button type="button" className="tc-card" data-status={f.status} aria-pressed={i === selected} onClick={() => onSelect(i)}>
              {`${f.n} ${f.verb} ${f.text} · ${LABEL[f.status]}`}
            </button>
          </li>
        ))}
      </ol>
      <div className="tc-main">
        <section className="tc-browser" aria-label="Emulated browser">
          <div className="tc-chrome">
            <button type="button" aria-label="Previous frame" onClick={() => onSelect(Math.max(0, selected - 1))}>‹</button>
            <button type="button" aria-label="Next frame" onClick={() => onSelect(Math.min(frames.length - 1, selected + 1))}>›</button>
            <span className="tc-url">{cur.text}</span>
            <span className="tc-count">{`frame ${cur.n}/${frames.length}`}</span>
          </div>
          <div className="tc-viewport" data-status={cur.status}>
            {src ? <img src={src} alt={`Frame ${cur.n}: ${cur.text}`} /> : <p>No frame recorded for this step</p>}
          </div>
          <div className="tc-status">{`${LABEL[cur.status]} · ${cur.verb.toLowerCase()} · ${cur.text}`}</div>
        </section>
        <aside className="tc-inspector" aria-label="Action inspector">
          <span className="tc-kicker">{`ACTION ${cur.n} · ${cur.verb}`}</span>
          <h3>{cur.text}</h3>
          {cur.expected.length > 0 && (
            <ul aria-label="Assertions">
              {cur.expected.map((e) => <li key={e}><span aria-hidden="true">{MARK[cur.status]}</span> {e}</li>)}
            </ul>
          )}
          {cur.reason && <p className="tc-reason">{cur.reason}</p>}
          {cur.status === "waits_for_you" && (
            <div className="tc-decide">
              <button type="button" onClick={() => {
                const stepId = cur.stepId;
                setFailure(null);
                Promise.resolve().then(() => onMarkSafe(stepId)).catch((cause: unknown) => {
                  setFailure({ stepId, message: cause instanceof Error ? cause.message : String(cause) });
                });
              }}>I watched it — mark safe</button>
              {onSendBack
                ? <button type="button" onClick={() => onSendBack(cur.stepId)}>Send back</button>
                : <button type="button" disabled>Send back — not available yet</button>}
              {failure?.stepId === cur.stepId && <p role="alert">{`Mark safe failed: ${failure.message}`}</p>}
            </div>
          )}
        </aside>
      </div>
    </div>
  );
}
