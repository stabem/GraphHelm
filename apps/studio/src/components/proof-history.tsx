/**
 * #history: a journey's earlier runs, so the owner can see what changed on screen. The Runtime
 * keeps each finished run (verdict, time, per-screen results and frames) when a new one replaces
 * it; this reads that list, lets the owner pick one as "before", and compares each step's frame
 * now with the frame that run kept: side by side, and as a pixel diff that paints what changed.
 */
import { useEffect, useRef, useState } from "react";

import type { JourneyPastRun, JourneyRunView } from "../runtime/types";
import type { JourneyRunSource } from "./journey-flows";

/** A pixel counts as changed when any channel moved more than this (0-255): JPEG-free PNGs from
 * the same page differ by 0, antialiasing by a little. */
const CHANNEL_THRESHOLD = 32;
/** Frames are compared at this width: enough to see a moved button, cheap on every step. */
const DIFF_WIDTH = 320;
/** Below this share of changed pixels a step reads "same as before". */
export const CHANGED_AT = 0.002;

/** Compares two same-size RGBA buffers. Returns the share of pixels that changed and an RGBA
 * mask: changed pixels red, the rest the current frame faded to grey. */
export function diffPixels(now: Uint8ClampedArray, before: Uint8ClampedArray, threshold = CHANNEL_THRESHOLD): { ratio: number; mask: Uint8ClampedArray<ArrayBuffer> } {
  const mask = new Uint8ClampedArray(new ArrayBuffer(now.length));
  const pixels = Math.floor(now.length / 4);
  let changed = 0;
  for (let p = 0; p < pixels; p++) {
    const i = p * 4;
    const moved = Math.abs(now[i]! - before[i]!) > threshold || Math.abs(now[i + 1]! - before[i + 1]!) > threshold
      || Math.abs(now[i + 2]! - before[i + 2]!) > threshold;
    if (moved) {
      changed++;
      mask[i] = 230; mask[i + 1] = 40; mask[i + 2] = 60; mask[i + 3] = 255;
    } else {
      const grey = 200 + Math.round((now[i]! + now[i + 1]! + now[i + 2]!) / 3 * 55 / 255);
      mask[i] = grey; mask[i + 1] = grey; mask[i + 2] = grey; mask[i + 3] = 255;
    }
  }
  return { ratio: pixels === 0 ? 0 : changed / pixels, mask };
}

function loadImage(url: string): Promise<HTMLImageElement> {
  return new Promise((done, fail) => {
    const image = new Image();
    image.onload = () => done(image);
    image.onerror = () => fail(new Error("frame did not load"));
    image.src = url;
  });
}

/** The diff of two frame URLs drawn at `DIFF_WIDTH`, or null where the browser cannot draw it. */
export async function diffFrames(nowUrl: string, beforeUrl: string): Promise<{ ratio: number; url: string } | null> {
  const [now, before] = await Promise.all([loadImage(nowUrl), loadImage(beforeUrl)]);
  const width = DIFF_WIDTH;
  const height = Math.max(1, Math.round(width * (now.naturalHeight || 1) / (now.naturalWidth || 1)));
  const canvas = document.createElement("canvas");
  canvas.width = width; canvas.height = height;
  const ctx = canvas.getContext("2d", { willReadFrequently: true });
  if (!ctx) return null;
  ctx.drawImage(now, 0, 0, width, height);
  const a = ctx.getImageData(0, 0, width, height).data;
  ctx.clearRect(0, 0, width, height);
  ctx.drawImage(before, 0, 0, width, height);
  const b = ctx.getImageData(0, 0, width, height).data;
  const { ratio, mask } = diffPixels(a, b);
  ctx.putImageData(new ImageData(mask, width, height), 0, 0);
  return { ratio, url: canvas.toDataURL("image/png") };
}

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
export function when(iso: string | undefined): string {
  const t = iso ? new Date(iso) : null;
  if (!t || Number.isNaN(t.getTime())) return "unknown time";
  return `${t.getDate()} ${MONTHS[t.getMonth()]} ${String(t.getHours()).padStart(2, "0")}:${String(t.getMinutes()).padStart(2, "0")}`;
}

/** One past run in one word, as the rail says it. */
export function pastVerdict(run: JourneyRunView): "passed" | "failed" | "waits_for_you" {
  const results = Object.values(run.screens ?? {}).map((s) => s.result);
  const edges = Object.values(run.edges ?? {}).map((e) => e.result);
  if (run.state === "failed" || results.includes("fail") || edges.includes("fail")) return "failed";
  if (run.held || edges.includes("skipped")) return "waits_for_you";
  return "passed";
}

const MARK = { passed: "✓", failed: "✕", waits_for_you: "!" } as const;

export interface JourneyHistory {
  runs: JourneyPastRun[];
  /** The run picked as "before"; the newest by default. */
  baseline: JourneyPastRun | null;
  pick(run: string): void;
  /** A frame of the baseline run for one screen, once loaded. */
  frame(screenId: string): string | null;
  /** Per screen: the share of pixels that changed against the baseline, once measured. */
  changes: Record<string, number>;
}

/**
 * Reads the history (again each time a run ends: that run's predecessor just joined it), loads
 * the baseline's frames, and measures each screen that has a frame now and then.
 */
export function useJourneyHistory(flowId: string, source: JourneyRunSource, run: JourneyRunView | null, nowFrame: (screenId: string) => string | null, screens: readonly string[]): JourneyHistory {
  const [runs, setRuns] = useState<JourneyPastRun[]>([]);
  const [picked, setPicked] = useState<string | null>(null);
  const [frames, setFrames] = useState<Record<string, string>>({});
  const [changes, setChanges] = useState<Record<string, number>>({});
  const urls = useRef(new Map<string, string>());
  const settled = run !== null && run.state !== "running" ? `${run.startedAt ?? ""}:${run.state}` : null;
  useEffect(() => {
    if (!source.history || settled === null) return undefined;
    let cancelled = false;
    source.history(flowId).then((view) => { if (!cancelled) setRuns(view.runs ?? []); }, () => { if (!cancelled) setRuns([]); });
    return () => { cancelled = true; };
  }, [flowId, source, settled]);
  const baseline = runs.find((r) => r.run === picked) ?? runs[0] ?? null;
  const base = baseline?.run ?? null;
  useEffect(() => {
    if (!source.historyFrame || base === null) return undefined;
    let cancelled = false;
    setFrames({});
    for (const screen of screens) {
      if (!baseline?.screens?.[screen]?.frame) continue;
      const key = `${base}:${screen}`;
      const kept = urls.current.get(key);
      if (kept) { setFrames((before) => ({ ...before, [screen]: kept })); continue; }
      source.historyFrame(flowId, base, screen).then((frame) => {
        if (frame === null) return;
        const url = URL.createObjectURL(frame.blob);
        urls.current.set(key, url);
        if (!cancelled) setFrames((before) => ({ ...before, [screen]: url }));
      }, () => {});
    }
    return () => { cancelled = true; };
    // `baseline` is read through `base` and `screens`; its object changes on every history read.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [flowId, source, base, screens.join("\u0000")]);
  const pairs = screens.map((s) => [s, nowFrame(s), frames[s] ?? null] as const).filter(([, now, then]) => now && then);
  const pairKey = pairs.map(([s, now, then]) => `${s}=${now}|${then}`).join(",");
  useEffect(() => {
    let cancelled = false;
    setChanges({});
    for (const [screen, now, then] of pairs) {
      diffFrames(now!, then!).then((d) => { if (!cancelled && d) setChanges((before) => ({ ...before, [screen]: d.ratio })); }, () => {});
    }
    return () => { cancelled = true; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pairKey]);
  useEffect(() => {
    const kept = urls.current;
    return () => { for (const url of kept.values()) URL.revokeObjectURL(url); kept.clear(); };
  }, []);
  return { runs, baseline, pick: setPicked, frame: (s) => frames[s] ?? null, changes };
}

/** The row of past runs; clicking one makes it the "before" every frame is compared with. */
export function HistoryStrip({ history }: { history: JourneyHistory }) {
  if (history.runs.length === 0) return <p className="ph-none">No earlier runs yet. The next Run keeps this one to compare with.</p>;
  return (
    <div className="ph-strip" role="group" aria-label="Earlier runs">
      <span className="tc-cap">HISTORY</span>
      {history.runs.slice(0, 12).map((r) => {
        const verdict = pastVerdict(r);
        return (
          <button key={r.run} type="button" className="ph-run" data-status={verdict} aria-pressed={history.baseline?.run === r.run}
            title={`Compare with the run of ${when(r.startedAt)} (${verdict.replace(/_/g, " ")})`} onClick={() => history.pick(r.run)}>
            <span aria-hidden="true">{MARK[verdict]}</span> {when(r.startedAt)}
          </button>
        );
      })}
    </div>
  );
}

/** The inspector block: this step now beside the same step in the picked earlier run. */
export function FrameCompare({ history, screenId, nowUrl, nowStatus }: { history: JourneyHistory; screenId: string; nowUrl: string | null; nowStatus: string }) {
  const [diff, setDiff] = useState<{ ratio: number; url: string } | null>(null);
  const [showDiff, setShowDiff] = useState(true);
  const thenUrl = history.frame(screenId);
  useEffect(() => {
    setDiff(null);
    if (!nowUrl || !thenUrl) return undefined;
    let cancelled = false;
    diffFrames(nowUrl, thenUrl).then((d) => { if (!cancelled) setDiff(d); }, () => {});
    return () => { cancelled = true; };
  }, [nowUrl, thenUrl]);
  const baseline = history.baseline;
  if (!baseline) return null;
  const was = baseline.screens?.[screenId]?.result;
  const wasWord = was === "pass" ? "passed" : was === "fail" ? "failed" : was ? was : "not reached";
  const nowWord = nowStatus.replace(/_/g, " ");
  let summary: string;
  if (!thenUrl) summary = "That run kept no frame for this step.";
  else if (!nowUrl) summary = "This run has no frame for this step yet.";
  else if (diff === null) summary = "Comparing…";
  else if (diff.ratio < CHANGED_AT) summary = "Same as before: nothing changed on screen.";
  else summary = `${Math.max(1, Math.round(diff.ratio * 100))}% of the screen changed (in red).`;
  return (
    <div className="tc-ins-block ph-compare" role="group" aria-label="Compare with an earlier run">
      <span className="tc-cap">{`COMPARE WITH ${when(baseline.startedAt).toUpperCase()}`}</span>
      <p className="ph-summary" data-changed={diff !== null && diff.ratio >= CHANGED_AT}>{summary}</p>
      {was !== undefined && wasWord !== nowWord && <p className="ph-verdict">{`This step ${wasWord} then, ${nowWord} now.`}</p>}
      <div className="ph-pair">
        <figure><figcaption>Before</figcaption>{thenUrl ? <img src={thenUrl} alt={`Step as of ${when(baseline.startedAt)}`} /> : <span className="ph-blank">no frame</span>}</figure>
        <figure>
          <figcaption>
            {showDiff && diff ? "Changes" : "Now"}
            {diff && <button type="button" className="ph-toggle" onClick={() => setShowDiff((v) => !v)}>{showDiff ? "show now" : "show changes"}</button>}
          </figcaption>
          {nowUrl ? <img src={showDiff && diff ? diff.url : nowUrl} alt={showDiff && diff ? "What changed, in red" : "Step now"} /> : <span className="ph-blank">no frame</span>}
        </figure>
      </div>
    </div>
  );
}
