import { describe, expect, it } from "vitest";
import laneBars from "./lane-bars.ts?raw";
import agentBoard from "./agent-board.ts?raw";
import stageHealth from "./stage-health.ts?raw";
import lanesTimeline from "../components/lanes-timeline.tsx?raw";
import missionView from "../components/mission-view.tsx?raw";

/* #1234 (plan A7): lane liveness is judged once, in the Runtime, and served on the tasks route with
 * the budgets it was judged by (`liveness`; a stale claim is a lease that is `overdue`). Before,
 * three thresholds lived in two of these files (`LIVENESS_MS` 30 min, `FREE_LIVENESS_MS` 15 min,
 * `STALE_CLAIM_MS` 2 h) and every view could drift from the others and from the Runtime. This guard
 * fails if any file that draws liveness declares a threshold of its own again: the old names, or a
 * constant whose name speaks of liveness, silence, staleness, freedom or quiet and whose value is a
 * number. Pace budgets (`STAGE_ALLOWANCE_MS`: how long a stage may take) are not liveness and are
 * not named so. Cost: five raw imports, a regex each. */
const FILES: Record<string, string> = {
  "runtime/lane-bars.ts": laneBars,
  "runtime/agent-board.ts": agentBoard,
  "runtime/stage-health.ts": stageHealth,
  "components/lanes-timeline.tsx": lanesTimeline,
  "components/mission-view.tsx": missionView,
};

const OLD_NAMES = /\b(LIVENESS_MS|FREE_LIVENESS_MS|STALE_CLAIM_MS)\b/;
const THRESHOLD = /\bconst\s+([A-Z0-9_]*(?:LIVE|SILEN|STALE|FREE|QUIET)[A-Z0-9_]*)\s*(?::[^=]+)?=\s*[^;]*\d/;

/* Named exceptions, each with its reason. `GONE_QUIET_MS` (24 h) only folds agents with no record
 * for a day out of the roster into a "gone quiet" list; it drives no lane flag, column or verdict,
 * so it is a display window like `WINDOW_MS`, not a liveness threshold the Runtime judges. */
const EXCEPTIONS: Record<string, readonly string[]> = { "components/mission-view.tsx": ["GONE_QUIET_MS"] };

function offences(source: string, allowed: readonly string[] = []): string[] {
  const found: string[] = [];
  source.split(/\r?\n/).forEach((line, index) => {
    const code = line.replace(/\/\/.*$/, "");
    if (/^\s*(\*|\/\*)/.test(code)) return;
    const named = THRESHOLD.exec(code)?.[1];
    if (OLD_NAMES.test(code) || (named !== undefined && !allowed.includes(named))) found.push(`${index + 1}: ${line.trim()}`);
  });
  return found;
}

describe("no view keeps a liveness threshold of its own (#1234)", () => {
  it("the detector bites on the shapes it guards against (control)", () => {
    expect(offences("export const LIVENESS_MS = 30 * 60 * 1000;")).toHaveLength(1);
    expect(offences("export const STALE_CLAIM_MS = 2 * 3_600_000;")).toHaveLength(1);
    expect(offences("const LANE_SILENCE_MS: number = 45 * 60_000;")).toHaveLength(1);
    expect(offences("const QUIET_AFTER = 900_000;")).toHaveLength(1);
    expect(offences("export const STAGE_ALLOWANCE_MS = { plan: 30 * 60_000 };")).toHaveLength(0);
    expect(offences("  // LIVENESS_MS used to live here")).toHaveLength(0);
    expect(offences("const GONE_QUIET_MS = 24 * 3_600_000;")).toHaveLength(1);
    expect(offences("const GONE_QUIET_MS = 24 * 3_600_000;", ["GONE_QUIET_MS"])).toHaveLength(0);
  });

  for (const [path, source] of Object.entries(FILES)) {
    it(`${path} reads liveness from the Runtime's reply`, () => {
      expect(source.length, `${path} was read`).toBeGreaterThan(100);
      expect(offences(source, EXCEPTIONS[path]), `${path} declares a liveness threshold`).toEqual([]);
    });
  }
});
