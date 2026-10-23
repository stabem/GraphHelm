import { when } from "../time/when";
import { useEffect, useRef, useState } from "react";

import type { BoardTask, IssueJourney, Runtime } from "../runtime/client";
import { modesOn, type ReviewMode } from "./review";

const MODES: { mode: ReviewMode; label: string }[] = [
  { mode: "frame", label: "Frame" },
  { mode: "side", label: "Side by side" },
  { mode: "overlay", label: "Overlay" },
  { mode: "changes", label: "Changes" },
];

interface Props {
  runtime: Runtime;
  executionId: string;
  task: BoardTask;
  onClose: () => void;
}

/** A frame fetched with the session's bearer header into an object URL, released when it changes. */
function useFrame(runtime: Runtime, url: string | null, executionId: string): { src: string | null; error: string | null } {
  const [state, setState] = useState<{ src: string | null; error: string | null }>({ src: null, error: null });
  useEffect(() => {
    if (url === null) { setState({ src: null, error: null }); return; }
    let alive = true;
    let made: string | null = null;
    runtime.frameAt(url, executionId).then((blob) => {
      if (!alive) return;
      made = blob === null ? null : URL.createObjectURL(blob);
      setState({ src: made, error: blob === null ? "That run kept no frame for this screen." : null });
    }, (error: unknown) => alive && setState({ src: null, error: error instanceof Error ? error.message : "The frame could not be read." }));
    return () => { alive = false; if (made !== null) URL.revokeObjectURL(made); };
  }, [runtime, url, executionId]);
  return state;
}

/** The screens to pick from: Before's in its order, then any only Now kept. */
function screensOf(journey: IssueJourney | null): string[] {
  const names = (journey?.before?.screens ?? []).map((row) => row.screen);
  for (const row of journey?.now?.screens ?? []) if (!names.includes(row.screen)) names.push(row.screen);
  return names;
}

/**
 * #1802 (S4): the review screen of an In review or Merged card (docs/design/studio/Graph.dc.html):
 * a near-full-screen dialog with a browser bar, the compare modes and the viewport switch. Before is
 * the journey's newest finished run on main (#1795 R4a); Now is the PR head's newest captured run
 * (#1839, shown since #1867). Side by side, Overlay and Changes are on only when both frames exist;
 * otherwise they stay off and say why. Mobile shows the main run's mobile frame when it kept one
 * (#1777); a PR head capture is web only. Escape and Close review shut it.
 */
export function ReviewModal({ runtime, executionId, task, onClose }: Props) {
  const [journeys, setJourneys] = useState<IssueJourney[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [journeyId, setJourneyId] = useState<string | null>(null);
  const [screen, setScreen] = useState<string | null>(null);
  const [mode, setMode] = useState<ReviewMode>("frame");
  const [opacity, setOpacity] = useState(50);
  const [viewport, setViewport] = useState<"web" | "mobile">("web");
  const closeRef = useRef<HTMLButtonElement | null>(null);

  useEffect(() => {
    if (task.issue === null) { setJourneys([]); return; }
    let alive = true;
    runtime.issueJourneys(executionId, task.issue).then((rows) => {
      if (!alive) return;
      setJourneys(rows);
      setJourneyId(rows[0]?.id ?? null);
      setScreen(screensOf(rows[0] ?? null)[0] ?? null);
    }, (reason: unknown) => alive && setError(reason instanceof Error ? reason.message : "The issue's journeys could not be read."));
    return () => { alive = false; };
  }, [runtime, executionId, task.issue]);

  useEffect(() => {
    closeRef.current?.focus();
    const onKey = (event: KeyboardEvent) => { if (event.key === "Escape") onClose(); };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const journey = journeys?.find((row) => row.id === journeyId) ?? null;
  const shot = journey?.before?.screens.find((row) => row.screen === screen) ?? null;
  // #1777: Mobile is on when the main run kept a mobile frame of this screen.
  const mobileUrl = shot?.viewports?.mobile ?? null;
  const shownViewport = viewport === "mobile" && mobileUrl !== null ? "mobile" : "web";
  const before = useFrame(runtime, shownViewport === "mobile" ? mobileUrl : shot?.frameUrl ?? null, executionId);
  const nowShot = journey?.now?.screens.find((row) => row.screen === screen) ?? null;
  const now = useFrame(runtime, shownViewport === "web" ? nowShot?.frameUrl ?? null : null, executionId);
  const on = modesOn(before.src !== null, now.src !== null);
  const shown: ReviewMode = on[mode] ? mode : "frame";
  const screens = screensOf(journey);
  const beforeFigure = before.src !== null
    ? <figure><figcaption>Before · {journey?.before?.ranAt ? when(journey.before.ranAt).text : journey?.before?.run} · {journey?.before?.result ?? ""}</figcaption><img src={before.src} alt={`${screen ?? "screen"} before`} /></figure>
    : null;
  const nowFigure = now.src !== null
    ? <figure><figcaption>Now · PR #{journey?.now?.pr} at {journey?.now?.headSha?.slice(0, 8)} · {journey?.now?.ranAt ? when(journey.now.ranAt).text : ""} · {journey?.now?.result ?? ""}</figcaption><img src={now.src} alt={`${screen ?? "screen"} now`} /></figure>
    : null;

  return (
    <div className="review-backdrop" onClick={(event) => { if (event.target === event.currentTarget) onClose(); }}>
      <div role="dialog" aria-modal="true" aria-label="Review" className="review-modal">
        <header className="review-head">
          <span className="mono small">ISSUE #{task.issue ?? "?"}{task.pr === null ? "" : ` · PR #${task.pr}`} · {task.lane ?? "no lane"}</span>
          <strong>{task.prTitle ?? task.title ?? ""}</strong>
          <button ref={closeRef} type="button" className="b ghost" aria-label="Close review" onClick={onClose}>✕</button>
        </header>
        {error !== null && <p role="alert">{error}</p>}
        {journeys === null && error === null && <p>Reading the issue's journeys.</p>}
        {journeys !== null && journeys.length === 0 && <p>No journey is named by this issue's slices, so there is no screen to review.</p>}
        {journeys !== null && journeys.length > 0 && (
          <>
            <div className="review-pickers">
              <label>Journey{" "}
                <select aria-label="Review journey" value={journeyId ?? ""} onChange={(event) => {
                  const next = journeys.find((row) => row.id === event.target.value) ?? null;
                  setJourneyId(next?.id ?? null);
                  setScreen(screensOf(next)[0] ?? null);
                }}>
                  {journeys.map((row) => <option key={row.id} value={row.id}>{row.title ?? row.id}</option>)}
                </select>
              </label>
              {screens.length > 0 && (
                <label>Screen{" "}
                  <select aria-label="Review screen" value={screen ?? ""} onChange={(event) => setScreen(event.target.value)}>
                    {screens.map((name) => <option key={name} value={name}>{name}</option>)}
                  </select>
                </label>
              )}
            </div>
            <section aria-label="Review frame" className="review-frame">
              <div className="compare-bar">
                <span className="mono small review-url">{(shownViewport === "mobile" ? mobileUrl : shot?.frameUrl) ?? "no frame"}</span>
                <div role="group" aria-label="Viewport" className="seg-group">
                  <button type="button" className={`seg${shownViewport === "web" ? " on" : ""}`} aria-pressed={shownViewport === "web"} onClick={() => setViewport("web")}>Web</button>
                  <button type="button" className={`seg${shownViewport === "mobile" ? " on" : ""}`} aria-pressed={shownViewport === "mobile"} disabled={mobileUrl === null} aria-describedby={mobileUrl === null ? "review-mobile-note" : undefined} onClick={() => setViewport("mobile")}>Mobile</button>
                  <span className="mono compare-size">{shownViewport === "mobile" ? "390 × 844" : "1280 × 800"}</span>
                </div>
                <div role="group" aria-label="Compare mode" className="seg-group">
                  {MODES.map(({ mode: each, label }) => (
                    <button key={each} type="button" className={`seg${shown === each ? " on" : ""}`} aria-pressed={shown === each}
                      disabled={!on[each]} onClick={() => setMode(each)}>{label}</button>
                  ))}
                </div>
                {shown === "overlay" && <input type="range" min={0} max={100} value={opacity} aria-label="Now opacity" onChange={(event) => setOpacity(Number(event.target.value))} />}
              </div>
              {mobileUrl === null && <p id="review-mobile-note" className="compare-note">The main run kept no mobile frame of this screen (the flow names only web, or the run is from before #1777).</p>}
              {!on.side && (
                <p className="compare-note">
                  {journey?.now == null
                    ? "The PR head has no captured run of this journey yet, so Side by side, Overlay and Changes are off."
                    : nowShot === null
                      ? "The PR head's run kept no frame of this screen, so only one side can be shown."
                      : shownViewport === "mobile"
                        ? "A PR head capture is web only; switch to Web to compare."
                        : journey?.before === null || shot === null
                          ? "Main has no Before frame of this screen, so only Now can be shown."
                          : "Reading the frames."}
                </p>
              )}
              {shown === "side" && <div className="compare-side">{beforeFigure}{nowFigure}</div>}
              {(shown === "overlay" || shown === "changes") && before.src !== null && now.src !== null && (
                <figure style={{ position: "relative" }}>
                  <figcaption>{shown === "overlay" ? `Now over Before at ${opacity}%` : "Changes: Now against Before (unchanged pixels are black)"}</figcaption>
                  <img src={before.src} alt={`${screen ?? "screen"} before`} />
                  <img src={now.src} alt={`${screen ?? "screen"} now`}
                    style={{ position: "absolute", left: 0, bottom: 0, opacity: shown === "overlay" ? opacity / 100 : 1, mixBlendMode: shown === "changes" ? "difference" : "normal" }} />
                </figure>
              )}
              {shown === "frame" && (
                journey?.before === null && journey?.now == null
                  ? <p>This journey has not finished a run on main or on the PR head yet, so there is no frame.</p>
                  : beforeFigure ?? nowFigure ?? <p>{before.error ?? now.error ?? "Reading the frame."}</p>
              )}
            </section>
          </>
        )}
      </div>
    </div>
  );
}
