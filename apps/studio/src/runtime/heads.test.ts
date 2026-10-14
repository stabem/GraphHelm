import { describe, expect, it } from "vitest";
import { sameHead } from "./heads";
import { agentBoard } from "./agent-board";
import { withCurrentReviewer, type MissionTask } from "./mission";
import { stageHealth } from "./stage-health";
import { parseSlots } from "./slots";
import type { Lane } from "./lane-bars";
import { TEST_FREE_MS, TEST_SILENT_MS } from "../test/task-fold";
import type { TaskState } from "./team-tasks";
import agentBoardSource from "./agent-board.ts?raw";
import missionSource from "./mission.ts?raw";
import stageHealthSource from "./stage-health.ts?raw";
import laneBarsSource from "./lane-bars.ts?raw";

/* #1275: the Runtime's fold treats a full and a short sha as one head (#1262). The views that
 * compare heads go through `sameHead`, so a `pr_opened` spelled with 8 characters and a verdict or
 * assignment spelled with 40 read the same in the fold and in every view. */
const SHORT = "b9319a3a";
const FULL = "b9319a3a7c0e5d41f2a6b8c9d0e1f2a3b4c5d6e7";

describe("sameHead (#1275)", () => {
  it("is the fold's rule: a prefix of at least 7, either way round, any case; equal is equal", () => {
    expect(sameHead(SHORT, FULL)).toBe(true);
    expect(sameHead(FULL, SHORT)).toBe(true);
    expect(sameHead(FULL.toUpperCase(), SHORT)).toBe(true);
    expect(sameHead("b9319a3", FULL)).toBe(true);
    expect(sameHead("b9319a", FULL)).toBe(false);
    expect(sameHead("b9319acf", FULL)).toBe(false);
    expect(sameHead("h2", "h2")).toBe(true);
    expect(sameHead(null, FULL)).toBe(false);
    expect(sameHead(undefined, undefined)).toBe(false);
  });
});

describe("an 8-char pr_opened and a 40-char verdict are one head in every view (#1275)", () => {
  const now = Date.parse("2026-10-14T12:00:00Z");
  const M = 60_000;
  const mission = (over: Partial<MissionTask>): MissionTask => ({ key: "issue-9#pr-9", pr: 9, issue: 9, title: "t", lane: "author",
    reviewers: ["rev"], step: "review", blocked: false, trust: 1, blockedBy: null, rounds: [], headSha: SHORT, mergeSha: null, repoUrl: null, ...over });

  it("agent board: a BLOCK recorded on the full sha frees the reviewer of the short-sha head", () => {
    const bot = { key: "rev", actorId: "rev", name: "rev", hue: 0, role: null, doingNow: "", lastRecordAt: new Date(now).toISOString(),
      lastSequence: 0, state: "working", quietMinutes: null, shared: false, native: false, tasks: [] } as never;
    // #1234: a hand-built lane carries the budgets the Runtime serves; without them nobody is free.
    const lane: Lane = { lane: "rev", bars: [], silent: false, lastEventAt: now, silentAfterMs: TEST_SILENT_MS, freeAfterMs: TEST_FREE_MS };
    const blocked = mission({ blocked: true, blockedBy: { reviewer: "rev", headSha: FULL }, currentReviewer: "rev" });
    expect(agentBoard([bot], [lane], [blocked], now)[0]!.status).toBe("free");
    const round = mission({ rounds: [{ reviewer: "rev", headSha: FULL, fixHead: null, blockedAt: null }], currentReviewer: "rev" });
    expect(agentBoard([bot], [lane], [round], now)[0]!.status).toBe("free");
  });

  it("mission: a review bar assigned on the full sha names the current reviewer of the short-sha head", () => {
    const lanes: Lane[] = [{ lane: "rev", silent: false, lastEventAt: now,
      bars: [{ kind: "review", label: "#9", taskId: "issue-9", headSha: FULL, start: now - M, end: now, open: true, since: now - M }] }];
    const t = withCurrentReviewer(mission({ reviewers: ["rev", "other"] }), lanes);
    expect(t.reviewers).toEqual(["rev"]);
  });

  it("stage health: the reviewer's build-slot wait belongs to the review of the short-sha head", () => {
    const task = { key: "issue-9", taskId: "issue-9", branch: null, issue: 9, pr: 9, lane: "author", headSha: SHORT, journeys: [], step: "review",
      blockedBy: null, reviewers: ["rev"], currentReviewer: "rev", mergeSha: null, repoUrl: null, strayVerdicts: [], title: "T", summary: null, prTitle: null,
      prSummary: null, critic: null, recordedHeads: [SHORT], parent: null, rounds: [], clock: { since: new Date(now - M).toISOString(), spent: {} }, lastSequence: 0 } as unknown as TaskState;
    const lanes: Lane[] = [{ lane: "rev", silent: false, lastEventAt: now,
      bars: [{ kind: "review", label: "#9", taskId: "issue-9", headSha: FULL, start: now - M, end: now, open: true }] }];
    const waiting = parseSlots({ slots: [{ root: "D:/gh", holder: null, waiting: [{ lane: "rev", waitedSeconds: 120 }] }] });
    expect(stageHealth(task, lanes, [task], now, false, waiting)?.flag).toBe("waiting_build");
  });
});

/* Guard: a view that compares two heads with `===`/`!==` again would disagree with the fold on a
 * mixed-length pair. Presence checks (`!== undefined`, `!== null`) are not head comparisons. */
const VIEWS: Record<string, string> = {
  "runtime/agent-board.ts": agentBoardSource,
  "runtime/mission.ts": missionSource,
  "runtime/stage-health.ts": stageHealthSource,
  "runtime/lane-bars.ts": laneBarsSource,
};
const HEAD = String.raw`(?:\w*[hH]ead(?:Sha)?\b|headOf\.get\([^)]*\))`;
const COMPARE = new RegExp(String.raw`${HEAD}\s*[!=]==\s*[\w.(]|[\w.)]\s*[!=]==\s*${HEAD}`);

function headComparisons(source: string): string[] {
  return source.split(/\r?\n/).flatMap((line, index) => {
    const code = line.replace(/\/\/.*$/, "");
    if (/^\s*(\*|\/\*)/.test(code)) return [];
    const bare = code.replace(/[\w.?()]+\s*[!=]==\s*(?:undefined|null)\b/g, "");
    return COMPARE.test(bare) ? [`${index + 1}: ${line.trim()}`] : [];
  });
}

describe("no view compares heads exactly (#1275)", () => {
  it("the detector bites on the shapes it guards against (control)", () => {
    expect(headComparisons("if (r.headSha === t.headSha) return true;")).toHaveLength(1);
    expect(headComparisons("if (b.headSha !== t.headSha) continue;")).toHaveLength(1);
    expect(headComparisons("if (headOf.get(slice) !== e.headSha) close();")).toHaveLength(1);
    expect(headComparisons("|| (head && value.bar.headSha !== head)) continue;")).toHaveLength(1);
    expect(headComparisons("if (e.headSha !== undefined && sameHead(e.headSha, x)) close();")).toHaveLength(0);
    expect(headComparisons("if (headOf.get(slice) !== undefined && t.headSha === null) return;")).toHaveLength(0);
  });
  for (const [path, source] of Object.entries(VIEWS)) {
    it(`${path} compares heads through sameHead`, () => {
      expect(source.length, `${path} was read`).toBeGreaterThan(100);
      expect(headComparisons(source), `${path} compares heads with ===`).toEqual([]);
    });
  }
});
