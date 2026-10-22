import type { JourneyPastRun, JourneyRun } from "../runtime/client";

/** The four ways the frame panel shows a step (docs/design/studio/Proof.dc.html). */
export type CompareMode = "frame" | "side" | "overlay" | "changes";

/** The largest frame Changes decodes: a 4K screenshot. A bigger pair is refused, not scaled. */
export const MAX_DIFF_PIXELS = 3840 * 2160;

/** How far two pixels may differ (sum over R, G and B, 0-765) before the pixel counts as changed,
 * so compression noise and anti-aliasing do not read as a change. */
export const CHANGE_THRESHOLD = 48;

/** The earlier run a step is compared with: the newest finished run, other than the one on screen,
 * that kept a frame for this step's screen. `null` when no earlier run did. */
export function earlierRun(history: JourneyPastRun[], current: JourneyRun | null, screen: string): JourneyPastRun | null {
  return history.find((past) =>
    past.screens?.[screen]?.frame === true
    && !(current?.ranAt !== undefined && past.ranAt === current.ranAt)) ?? null;
}

export interface Pixels { width: number; height: number; data: Uint8ClampedArray }

export type Diff =
  | { kind: "diff"; width: number; height: number; changed: number; percent: number; data: Uint8ClampedArray<ArrayBuffer> }
  | { kind: "refused"; reason: string };

/** Marks every pixel of `now` that differs from `before`: changed pixels red, the rest a dimmed
 * grey of `now`. Frames of different sizes are refused with both sizes named, never stretched or
 * cropped into a percentage that would mean something else; frames over MAX_DIFF_PIXELS too. */
export function diffPixels(before: Pixels, now: Pixels): Diff {
  if (before.width !== now.width || before.height !== now.height) {
    return { kind: "refused", reason: `The frames have different sizes (before ${before.width}×${before.height}, now ${now.width}×${now.height}), so no pixel comparison is made.` };
  }
  const total = now.width * now.height;
  if (total === 0) return { kind: "refused", reason: "The frames are empty." };
  if (total > MAX_DIFF_PIXELS) return { kind: "refused", reason: `The frames are too large to compare here (${now.width}×${now.height}).` };
  if (before.data.length < total * 4 || now.data.length < total * 4) return { kind: "refused", reason: "A frame could not be decoded." };
  const out = new Uint8ClampedArray(total * 4);
  let changed = 0;
  for (let i = 0; i < total * 4; i += 4) {
    const delta = Math.abs(before.data[i] - now.data[i]) + Math.abs(before.data[i + 1] - now.data[i + 1]) + Math.abs(before.data[i + 2] - now.data[i + 2]);
    if (delta > CHANGE_THRESHOLD) {
      changed += 1;
      out[i] = 255; out[i + 1] = 59; out[i + 2] = 59;
    } else {
      const grey = Math.round(0.3 * now.data[i] + 0.59 * now.data[i + 1] + 0.11 * now.data[i + 2]);
      const dim = Math.round(grey * 0.5 + 64);
      out[i] = dim; out[i + 1] = dim; out[i + 2] = dim;
    }
    out[i + 3] = 255;
  }
  return { kind: "diff", width: now.width, height: now.height, changed, percent: (changed / total) * 100, data: out };
}

/** The sentence under the compare: a change too small to round to 1% is still said, never "0%",
 * and a screen not wholly changed never reads "100%". */
export function changedSentence(changed: number, percent: number): string {
  if (changed === 0) return "Nothing changed on the screen.";
  const shown = percent < 1 ? "Less than 1%" : percent > 99 && percent < 100 ? "More than 99%" : `${Math.round(percent)}%`;
  return `${shown} of the screen changed (in red).`;
}
