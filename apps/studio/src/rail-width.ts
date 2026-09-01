/**
 * How wide the operator left the projects rail.
 *
 * Kept in this browser and nowhere else: a pane width is a property of the screen someone is
 * sitting at, not of the run, the project or the Runtime. Sending it anywhere would make one
 * person's window size everybody's.
 *
 * The bounds are enforced in THREE places on purpose, and none of them is redundant. CSS clamps
 * what is painted, so a bad value can never produce an unusable window. The drag clamps what is
 * stored, so the value that survives the session is already sane. And this module clamps what is
 * READ, because the stored value outlives the code that wrote it: a build that changes the bounds
 * would otherwise inherit widths from the build before it.
 */

export const RAIL_MIN = 200;
export const RAIL_MAX = 460;
export const RAIL_DEFAULT = 268;

const KEY = "graphhelm.studio.rail-width";

export function loadRailWidth(): number {
  let raw: string | null = null;
  try {
    raw = window.localStorage.getItem(KEY);
  } catch {
    // A private window, or storage the browser refuses. The default is a correct answer, not a
    // degraded one, so there is nothing here worth telling anybody about.
    return RAIL_DEFAULT;
  }
  if (raw === null) return RAIL_DEFAULT;
  const parsed = Number.parseInt(raw, 10);
  if (!Number.isFinite(parsed)) return RAIL_DEFAULT;
  return Math.max(RAIL_MIN, Math.min(RAIL_MAX, parsed));
}

export function saveRailWidth(width: number): void {
  try {
    window.localStorage.setItem(KEY, String(Math.max(RAIL_MIN, Math.min(RAIL_MAX, width))));
  } catch {
    // Failing to remember a pane width is not worth interrupting anyone over.
  }
}
