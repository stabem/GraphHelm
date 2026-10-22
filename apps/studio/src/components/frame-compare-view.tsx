/**
 * #1776: Before and Now compared in the big frame, not as two thumbnails in the side panel.
 *
 * Three ways to look, one small segmented control above the picture: side by side at the same
 * scale, Now laid over Before with an opacity slider (the default, at 50%), and Now with the
 * changed regions highlighted (the diff already computed for the "x% of the screen changed"
 * figure; offered only when that diff exists). Escape or "Back to frame" returns to the frame.
 */
import "./frame-compare-view.css";
import { useEffect, useRef, useState } from "react";

export type CompareMode = "side" | "overlay" | "changes";

interface Props {
  beforeUrl: string;
  nowUrl: string;
  /** The highlighted diff of Now against Before; null when it was not computed. */
  diffUrl: string | null;
  /** How the Before picture is named, e.g. "Before (Oct 20, 14:02)". */
  beforeLabel: string;
  initialMode?: CompareMode;
  onBack: () => void;
}

const MODES: { mode: CompareMode; label: string }[] = [
  { mode: "side", label: "Side by side" },
  { mode: "overlay", label: "Overlay" },
  { mode: "changes", label: "Changes" },
];

export function FrameCompareView({ beforeUrl, nowUrl, diffUrl, beforeLabel, initialMode = "overlay", onBack }: Props) {
  const [mode, setMode] = useState<CompareMode>(initialMode === "changes" && !diffUrl ? "overlay" : initialMode);
  const [opacity, setOpacity] = useState(50);
  const modes = MODES.filter((m) => m.mode !== "changes" || diffUrl !== null);
  const root = useRef<HTMLDivElement>(null);
  const back = useRef(onBack);
  back.current = onBack;
  // #1776 review: Escape must close the compare wherever focus is (the side-panel button that
  // opened it keeps focus after a click), so it listens on the document while open, and focus
  // moves into the compare so its controls are the next stops for the keyboard.
  useEffect(() => {
    root.current?.focus();
    const onKey = (event: KeyboardEvent) => { if (event.key === "Escape") back.current(); };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, []);
  return (
    <div className="fcv" role="group" aria-label="Compare Before and Now" tabIndex={-1} ref={root}>
      <div className="fcv-bar">
        <div className="fcv-modes" role="group" aria-label="Compare mode">
          {modes.map((m) => (
            <button key={m.mode} type="button" className="fcv-mode" aria-pressed={mode === m.mode} onClick={() => setMode(m.mode)}>{m.label}</button>
          ))}
        </div>
        {mode === "overlay" && (
          <label className="fcv-opacity">
            <span>Before</span>
            <input type="range" min={0} max={100} step={1} value={opacity} aria-label="Now opacity"
              onChange={(event) => setOpacity(Number(event.target.value))} />
            <span>{`Now ${opacity}%`}</span>
          </label>
        )}
        <button type="button" className="fcv-back" onClick={onBack}>Back to frame</button>
      </div>
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
}
