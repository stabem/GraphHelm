import { useEffect, useState } from "react";

import type { JourneyPastRun, Runtime } from "../runtime/client";
import { changedSentence, diffPixels, type CompareMode, type Pixels } from "./compare";
import type { Step } from "./steps";

const MODES: { mode: CompareMode; label: string }[] = [
  { mode: "frame", label: "Frame" },
  { mode: "side", label: "Side by side" },
  { mode: "overlay", label: "Overlay" },
  { mode: "changes", label: "Changes" },
];

const PAST = { pass: "passed", fail: "failed", drift: "drifted" } as const;

interface CompareFrameProps {
  runtime: Runtime;
  flowId: string;
  step: Step;
  steps: number;
  /** Object URL of this step's frame in the run on screen. */
  now: string | undefined;
  /** #1777: the same step's mobile frame, when the run played mobile. */
  mobileNow?: string | undefined;
  /** The earlier run this step is compared with, or `null` when none kept its frame. */
  earlier: JourneyPastRun | null;
}

/** The selected step's frame with its compare modes (docs/design/studio/Proof.dc.html): the frame
 * alone, Before and Now side by side, Now over Before with an opacity slider, or Now with every
 * pixel that changed since Before painted red. Before is the earlier run's frame, fetched with the
 * session's bearer header into an object URL. */
export function CompareFrame({ runtime, flowId, step, steps, now: webNow, mobileNow, earlier }: CompareFrameProps) {
  const [mode, setMode] = useState<CompareMode>("frame");
  const [opacity, setOpacity] = useState(50);
  const [viewport, setViewport] = useState<"web" | "mobile">("web");
  // #1777: Mobile is on when this run kept a mobile frame of this step; Before pairs the same viewport.
  const mobileOn = mobileNow !== undefined;
  // #1878: a step with only a mobile frame (a mobile-only run) shows it as mobile, never as web.
  const webOn = webNow !== undefined || !mobileOn;
  const shownViewport = mobileOn && (viewport === "mobile" || !webOn) ? "mobile" : "web";
  const now = shownViewport === "mobile" ? mobileNow : webNow;
  const before = usePastFrame(runtime, flowId, earlier?.run ?? null, step.screen, shownViewport === "mobile" ? "mobile" : undefined);
  const changes = useChanges(mode === "changes" ? before.url : null, mode === "changes" ? now ?? null : null);
  const comparable = now !== undefined && before.url !== null;
  const shown = comparable ? mode : "frame";
  const when = earlier === null ? "" : earlier.ranAt ?? earlier.run;
  const offReason = now === undefined
    ? `No frame captured for step ${step.n} in this run, so there is nothing to compare.`
    : earlier === null
      ? "No earlier run kept a frame for this step, so Side by side, Overlay and Changes are off."
      : before.error ?? (before.url === null ? "Reading the earlier run's frame…" : null);

  return (
    <div className="compare">
      <section aria-label="Compare frame" className="compare-frame">
        <div className="compare-bar">
          <span className="mono compare-step">step {step.n} · {step.label}</span>
          <div role="group" aria-label="Viewport" className="seg-group">
            <button type="button" className={`seg${shownViewport === "web" ? " on" : ""}`} aria-pressed={shownViewport === "web"} disabled={!webOn} onClick={() => setViewport("web")}>Web</button>
            <button type="button" className={`seg${shownViewport === "mobile" ? " on" : ""}`} aria-pressed={shownViewport === "mobile"} disabled={!mobileOn} aria-describedby={mobileOn ? undefined : "compare-mobile-note"} onClick={() => setViewport("mobile")}>Mobile</button>
            <span className="mono compare-size">{shownViewport === "mobile" ? "390 × 844" : "1280 × 800"}</span>
          </div>
          <div role="group" aria-label="Compare mode" className="seg-group">
            {MODES.map(({ mode: each, label }) => (
              <button key={each} type="button" className={`seg${shown === each ? " on" : ""}`} aria-pressed={shown === each}
                disabled={each !== "frame" && !comparable} onClick={() => setMode(each)}>{label}</button>
            ))}
          </div>
          {shown === "overlay" && (
            <label className="compare-slider">Before
              <input type="range" min={0} max={100} value={opacity} aria-label="Now opacity" onChange={(event) => setOpacity(Number(event.target.value))} />
              Now</label>
          )}
          <span className="mono compare-count">frame {step.n}/{steps}</span>
        </div>
        {!mobileOn && <p id="compare-mobile-note" className="compare-note">This run kept no mobile frame of this step (a flow can name only web in `viewports`, and runs before #1777 played web alone).</p>}
        {shown === "frame" && (now ? <img src={now} alt={`Step ${step.n} captured`} /> : <p>No frame captured for step {step.n} in this run.</p>)}
        {shown === "side" && before.url !== null && now && (
          <div role="group" aria-label="Before and Now side by side" className="compare-side">
            <figure><figcaption>Before · {when}</figcaption><img src={before.url} alt={`Step ${step.n} before`} /></figure>
            <figure><figcaption>Now</figcaption><img src={now} alt={`Step ${step.n} now`} /></figure>
          </div>
        )}
        {shown === "overlay" && before.url !== null && now && (
          <div className="compare-overlay">
            <img src={before.url} alt={`Step ${step.n} before`} />
            <img src={now} alt={`Step ${step.n} now`} style={{ opacity: opacity / 100 }} />
          </div>
        )}
        {shown === "changes" && (changes.url !== null
          ? <img src={changes.url} alt="Now, changed regions highlighted" />
          : <p>{changes.message ?? "Comparing the two frames…"}</p>)}
      </section>
      <section aria-label="Compare with an earlier run" className="compare-side-panel">
        <span className="section-label">{earlier === null ? "NO EARLIER RUN" : `COMPARE WITH ${when}`}</span>
        {offReason !== null && <span>{offReason}</span>}
        {earlier?.screens?.[step.screen]?.result !== undefined && <span>This step {PAST[earlier.screens[step.screen]!.result!]} then.</span>}
        {shown === "changes" && changes.sentence !== null && <span className="compare-changed">{changes.sentence}</span>}
      </section>
    </div>
  );
}

/** An earlier run's frame of one screen, as an object URL revoked when it is replaced or left. */
function usePastFrame(runtime: Runtime, flowId: string, run: string | null, screen: string, viewport?: "mobile"): { url: string | null; error: string | null } {
  // #1808: the state names the frame it was read for, so a switch of step or viewport never shows
  // the previous frame for the render before the effect clears it.
  const key = `${flowId}|${run}|${screen}|${viewport ?? "web"}`;
  const [state, setState] = useState<{ key: string; url: string | null; error: string | null }>({ key: "", url: null, error: null });
  useEffect(() => {
    setState({ key, url: null, error: null });
    if (run === null) return;
    let live = true;
    let url: string | null = null;
    runtime.journeyPastFrame(flowId, run, screen, viewport).then((blob) => {
      if (!live) return;
      if (blob === null) { setState({ key, url: null, error: "The earlier run kept no frame for this step." }); return; }
      url = URL.createObjectURL(blob);
      setState({ key, url, error: null });
    }, (cause: unknown) => { if (live) setState({ key, url: null, error: cause instanceof Error ? cause.message : "The Runtime refused the frame." }); });
    return () => { live = false; if (url !== null) URL.revokeObjectURL(url); };
  }, [runtime, key, flowId, run, screen, viewport]);
  return state.key === key ? { url: state.url, error: state.error } : { url: null, error: null };
}

function decode(src: string): Promise<Pixels> {
  return new Promise((resolve, reject) => {
    const image = new Image();
    image.onload = () => {
      const canvas = document.createElement("canvas");
      canvas.width = image.naturalWidth;
      canvas.height = image.naturalHeight;
      const context = canvas.getContext("2d");
      if (context === null || canvas.width === 0) { reject(new Error("A frame could not be decoded.")); return; }
      context.drawImage(image, 0, 0);
      const read = context.getImageData(0, 0, canvas.width, canvas.height);
      resolve({ width: read.width, height: read.height, data: read.data });
    };
    image.onerror = () => reject(new Error("A frame could not be decoded."));
    image.src = src;
  });
}

/** Changes: both frames decoded on a canvas, compared pixel by pixel, the result shown as an object
 * URL (revoked when replaced or left) with the changed share of the screen. */
function useChanges(before: string | null, now: string | null): { url: string | null; message: string | null; sentence: string | null } {
  // #1808: the result names the pair it was drawn from. A viewport switch must never leave the
  // previous diff on screen for a render, or a reader that found it sees it vanish a moment later.
  type Drawn = { before: string | null; now: string | null; url: string | null; message: string | null; sentence: string | null };
  const [state, setState] = useState<Drawn>({ before: null, now: null, url: null, message: null, sentence: null });
  const reset = { before, now, url: null, message: null, sentence: null };
  useEffect(() => {
    setState({ before, now, url: null, message: null, sentence: null });
    if (before === null || now === null) return;
    let live = true;
    Promise.all([decode(before), decode(now)]).then(([a, b]) => {
      if (!live) return;
      const diff = diffPixels(a, b);
      if (diff.kind === "refused") { setState({ before, now, url: null, message: diff.reason, sentence: null }); return; }
      const canvas = document.createElement("canvas");
      canvas.width = diff.width;
      canvas.height = diff.height;
      canvas.getContext("2d")?.putImageData(new ImageData(diff.data, diff.width, diff.height), 0, 0);
      // #1808: synchronous, so the picture never waits on a toBlob callback a hidden or headless
      // page may hold back (the mobile switch stayed on "Comparing the two frames…").
      setState({ before, now, url: canvas.toDataURL("image/png"), message: null, sentence: changedSentence(diff.changed, diff.percent) });
    }, (cause: unknown) => { if (live) setState({ before, now, url: null, message: cause instanceof Error ? cause.message : "A frame could not be decoded.", sentence: null }); });
    return () => { live = false; };
  }, [before, now]);
  const current = state.before === before && state.now === now ? state : reset;
  return { url: current.url, message: current.message, sentence: current.sentence };
}
