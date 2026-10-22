/**
 * #1776: Before and Now compared in the big frame, not as two thumbnails in the side panel.
 *
 * The mode control lives in the frame bar, like a browser's toolbar (the approved Proof design):
 * Frame (the step's own picture), Side by side at the same scale, Overlay (Now laid over Before,
 * with a Before–Now opacity slider at 50%), and Changes (Now with the changed regions highlighted;
 * offered only when that diff exists). Frame or Escape returns to the step's picture.
 */
import "./frame-compare-view.css";
import { useEffect, useRef, useState, type ReactNode } from "react";

export type CompareMode = "side" | "overlay" | "changes";

export interface CompareSubject {
  beforeUrl: string;
  nowUrl: string;
  /** The highlighted diff of Now against Before; null when it was not computed. */
  diffUrl: string | null;
  /** How the Before picture is named, e.g. "Before (Oct 20, 14:02)". */
  beforeLabel: string;
  mode: CompareMode;
}

const MODES: { mode: CompareMode; label: string }[] = [
  { mode: "side", label: "Side by side" },
  { mode: "overlay", label: "Overlay" },
  { mode: "changes", label: "Changes" },
];

/**
 * The compare for one opened subject: `tools` go in the frame bar, `view` in place of the picture.
 * Both are null while nothing is open. A new subject starts again in the mode it asks for.
 */
export function useFrameCompare(subject: CompareSubject | null, onFrame: () => void): { tools: ReactNode; view: ReactNode } {
  const asked = subject && subject.mode === "changes" && !subject.diffUrl ? "overlay" : subject?.mode ?? "overlay";
  const [state, setState] = useState<{ of: CompareSubject | null; mode: CompareMode; opacity: number }>({ of: subject, mode: asked, opacity: 50 });
  if (state.of !== subject) setState({ of: subject, mode: asked, opacity: 50 });
  const mode = state.of === subject ? state.mode : asked;
  const opacity = state.of === subject ? state.opacity : 50;
  const root = useRef<HTMLDivElement>(null);
  const back = useRef(onFrame);
  back.current = onFrame;
  // #1776 review: Escape must close the compare wherever focus is (the side-panel button that
  // opened it keeps focus after a click), so it listens on the document while open, and focus
  // moves into the compare.
  useEffect(() => {
    if (subject === null) return;
    root.current?.focus();
    const onKey = (event: KeyboardEvent) => { if (event.key === "Escape") back.current(); };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [subject]);
  if (subject === null) return { tools: null, view: null };
  const { beforeUrl, nowUrl, diffUrl, beforeLabel } = subject;
  const modes = MODES.filter((m) => m.mode !== "changes" || diffUrl !== null);
  const tools = (
    <>
      <span className="fcv-modes" role="group" aria-label="Compare mode">
        <button type="button" className="fcv-mode" aria-pressed={false} onClick={onFrame}>Frame</button>
        {modes.map((m) => (
          <button key={m.mode} type="button" className="fcv-mode" aria-pressed={mode === m.mode}
            onClick={() => setState({ of: subject, mode: m.mode, opacity })}>{m.label}</button>
        ))}
      </span>
      {mode === "overlay" && (
        <label className="fcv-opacity">
          <span>Before</span>
          <input type="range" min={0} max={100} step={1} value={opacity} aria-label="Now opacity"
            onChange={(event) => setState({ of: subject, mode, opacity: Number(event.target.value) })} />
          <span>Now</span>
        </label>
      )}
    </>
  );
  const view = (
    <div className="fcv" role="group" aria-label="Compare Before and Now" tabIndex={-1} ref={root}>
      {mode === "side" && (
        <div className="fcv-side" role="group" aria-label="Before and Now side by side">
          <figure><figcaption>{beforeLabel}</figcaption><img src={beforeUrl} alt={beforeLabel} /></figure>
          <figure><figcaption>Now</figcaption><img src={nowUrl} alt="Now" /></figure>
        </div>
      )}
      {mode === "overlay" && (
        <div className="fcv-stack">
          <img src={beforeUrl} alt={beforeLabel} />
          <img className="fcv-top" src={nowUrl} alt="Now" style={{ opacity: opacity / 100 }} />
        </div>
      )}
      {mode === "changes" && diffUrl && (
        <div className="fcv-single"><img src={diffUrl} alt="Now, changed regions highlighted" /></div>
      )}
    </div>
  );
  return { tools, view };
}
