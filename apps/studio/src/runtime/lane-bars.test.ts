import { describe, expect, it } from "vitest";
import { agentBoard } from "./agent-board";
import { LIVENESS_MS, laneBars, packBars, placeholderLane, type TimedTaskEvent } from "./lane-bars";

const T0 = Date.parse("2026-10-09T00:00:00Z");
const at = (ms: number) => new Date(T0 + ms).toISOString();
let seq = 0;
const ev = (e: Partial<TimedTaskEvent>): TimedTaskEvent => ({ actorId: "a", sequence: seq++, taskId: "t1", ...e }) as TimedTaskEvent;

describe("laneBars", () => {
  it("ends every lane's bars for a PR when it merges or closes", () => {
    for (const end of ["task.merged", "task.closed"] as const) {
      const lanes = laneBars([
        ev({ kind: "task.claimed", lane: "author", taskId: "issue-809", issue: 809, at: at(0) }),
        ev({ kind: "task.pr_opened", lane: "author", taskId: "issue-809", pr: 816, headSha: "head1", at: at(1) }),
        ev({ kind: "task.review_assigned", reviewer: "codex-review", taskId: "pr-816", pr: 816, headSha: "head1", at: at(2) }),
        ev({ kind: end, taskId: "pr-816", pr: 816, at: at(3) }),
      ], T0 + 10, 1000);
      expect(lanes.flatMap((lane) => lane.bars)).toEqual(expect.arrayContaining([
        expect.objectContaining({ kind: "implement", end: T0 + 2, open: false }),
        expect.objectContaining({ kind: "review", end: T0 + 3, open: false }),
      ]));
    }
  });

  it("ends a pool-assigned review on a verdict for the same PR head from a real reviewer", () => {
    const lanes = laneBars([
      ev({ kind: "task.pr_opened", lane: "author", taskId: "issue-809", pr: 816, headSha: "head1", at: at(0) }),
      ev({ kind: "task.review_assigned", reviewer: "codex-review", taskId: "pr-816", pr: 816, headSha: "head1", at: at(1) }),
      ev({ kind: "task.review_verdict", reviewer: "codexrev-2", taskId: "pr-816", pr: 816, headSha: "head1", verdict: "APPROVE" as never, at: at(2) }),
    ], T0 + 10, 1000);
    expect(lanes.find((lane) => lane.lane === "codex-review")!.bars[0]).toMatchObject({ end: T0 + 2, open: false });
  });
  it("uses the folded PR title on an older claim bar", () => {
    const lanes = laneBars([
      ev({ kind: "task.claimed", lane: "dev", taskId: "issue-809", issue: 809, title: "Old issue title", at: at(0) }),
      ev({ kind: "task.pr_opened", lane: "dev", taskId: "issue-809", pr: 822, title: "Close false timeline bars", at: at(1) }),
    ], T0 + 10, 1000);
    expect(lanes[0]!.bars[0]).toMatchObject({ label: "#822", displayLabel: "#822 Close false timeline bars" });
  });
  it("ends a same-lane re-claim bar when its already-open PR merges", () => {
    const lanes = laneBars([
      ev({ kind: "task.claimed", lane: "codex-4", actorId: "codex-4", taskId: "issue-86", at: at(0) }),
      ev({ kind: "task.pr_opened", lane: "codex-4", actorId: "codex-4", taskId: "issue-86", pr: 658, headSha: "head1", at: at(10) }),
      ev({ kind: "task.claimed", lane: "codex-4", actorId: "codex-4", taskId: "issue-86", at: at(20) }),
      ev({ kind: "task.pr_opened", lane: "codex-4", actorId: "codex-4", taskId: "issue-86", pr: 658, headSha: "head2", at: at(30) }),
      ev({ kind: "task.merged", actorId: "coordinator", taskId: "issue-86", pr: 658, at: at(40) }),
    ], T0 + 50, 1000);
    expect(lanes.find((lane) => lane.lane === "codex-4")!.bars.filter((bar) => bar.open)).toEqual([]);
    expect(lanes.find((lane) => lane.lane === "codex-4")!.silent).toBe(false);
    expect(lanes.find((lane) => lane.lane === "codex-4")!.bars).toHaveLength(2);
    expect(lanes.find((lane) => lane.lane === "codex-4")!.bars[1]).toMatchObject({ label: "#658", end: T0 + 40, open: false });
  });
  it.each([false, true])("keeps each PR title with its slice after reorder (ended first: %s)", (endedFirst) => {
    const records: TimedTaskEvent[] = [];
    if (endedFirst) records.push(
      ev({ kind: "task.claimed", lane: "old", taskId: "issue-0", issue: 0, title: "Old", at: at(0) }),
      ev({ kind: "task.closed", taskId: "issue-0", at: at(1) }),
    );
    records.push(
      ev({ kind: "task.claimed", lane: "a", taskId: "issue-1", issue: 1, title: "Alpha", at: at(2) }),
      ev({ kind: "task.claimed", lane: "b", taskId: "issue-2", issue: 2, title: "Beta", at: at(3) }),
      ev({ kind: "task.pr_opened", lane: "b", taskId: "issue-2", pr: 2, title: "Beta", at: at(4) }),
      ev({ kind: "task.pr_opened", lane: "a", taskId: "issue-1", pr: 1, title: "Alpha", at: at(5) }),
    );
    const bars = laneBars(records, T0 + 10, 1000).flatMap((lane) => lane.bars);
    expect(bars.find((bar) => bar.label === "#1")?.displayLabel).toBe("#1 Alpha");
    expect(bars.find((bar) => bar.label === "#2")?.displayLabel).toBe("#2 Beta");
  });
  it("keeps another slice's review open when a verdict routes to the same PR and head", () => {
    const lanes = laneBars([
      ev({ kind: "task.claimed", lane: "a", taskId: "issue-1", branch: "one", at: at(0) }),
      ev({ kind: "task.pr_opened", lane: "a", taskId: "issue-1", pr: 10, headSha: "head", at: at(1) }),
      ev({ kind: "task.review_assigned", reviewer: "pool-a", taskId: "issue-1", pr: 10, headSha: "head", at: at(2) }),
      ev({ kind: "task.claimed", lane: "b", taskId: "issue-1", branch: "two", headSha: "head", at: at(3) }),
      ev({ kind: "task.review_assigned", reviewer: "pool-b", taskId: "issue-1", headSha: "head", at: at(4) }),
      ev({ kind: "task.pr_opened", lane: "b", taskId: "issue-1", pr: 10, headSha: "head", at: at(5) }),
      ev({ kind: "task.review_verdict", reviewer: "real-a", taskId: "issue-1", pr: 10, headSha: "head", at: at(6) }),
    ], T0 + 10, 1000);
    expect(lanes.find((l) => l.lane === "pool-a")!.bars[0]).toMatchObject({ end: T0 + 6, open: false });
    expect(lanes.find((l) => l.lane === "pool-b")!.bars[0]).toMatchObject({ open: true });
  });

  it("keeps a silent gap between two records on the same claim", () => {
    const hour = 3_600_000;
    const lanes = laneBars([
      ev({ kind: "task.claimed", lane: "dev", actorId: "dev", taskId: "issue-783", at: at(0) }),
      ev({ kind: "task.planned", lane: "dev", actorId: "dev", taskId: "issue-783", at: at(5 * 60_000) }),
      ev({ kind: "task.planned", lane: "dev", actorId: "dev", taskId: "issue-783", at: at(13 * hour + 42 * 60_000) }),
      ev({ kind: "task.pr_opened", lane: "dev", actorId: "dev", taskId: "issue-783", pr: 783, at: at(13 * hour + 47 * 60_000) }),
      ev({ kind: "task.review_assigned", actorId: "coordinator", reviewer: "reviewer", taskId: "issue-783", pr: 783, at: at(13 * hour + 52 * 60_000) }),
    ], T0 + 13 * hour + 52 * 60_000, 14 * hour);
    expect(lanes.find((lane) => lane.lane === "dev")!.bars[0]!.quietSpans).toEqual([
      { start: T0 + 35 * 60_000, end: T0 + 13 * hour + 42 * 60_000 },
    ]);
  });
  it("marks a late supervisor release as quiet after the claimant's last record", () => {
    const lanes = laneBars([
      ev({ kind: "task.claimed", lane: "dev", actorId: "dev", taskId: "issue-548", sequence: 100, at: at(0) }),
      ev({ kind: "task.released", lane: "dev", actorId: "supervisor", taskId: "issue-548", claimSequence: 100, sequence: 101, at: at(27 * 3_600_000) } as never),
    ], T0 + 27 * 3_600_000, 28 * 3_600_000);
    expect(lanes.find((lane) => lane.lane === "dev")!.bars[0]).toMatchObject({
      lastActivityAt: T0, end: T0 + 27 * 3_600_000, endedBy: { kind: "released", actor: "supervisor" },
    });
    expect(lanes.find((lane) => lane.lane === "dev")!.bars[0]!.lastActivityAt! + LIVENESS_MS).toBe(T0 + LIVENESS_MS);
  });

  it("keeps a frequently recording claim solid through PR opening", () => {
    const lanes = laneBars([
      ev({ kind: "task.claimed", lane: "dev", actorId: "dev", taskId: "issue-549", at: at(0) }),
      ev({ kind: "task.planned", lane: "dev", actorId: "dev", taskId: "issue-549", at: at(10 * 60_000) }),
      ev({ kind: "task.pr_opened", lane: "dev", actorId: "dev", taskId: "issue-549", pr: 550, at: at(20 * 60_000) }),
      ev({ kind: "task.review_assigned", actorId: "coordinator", reviewer: "reviewer", taskId: "issue-549", pr: 550, at: at(25 * 60_000) }),
    ], T0 + 25 * 60_000, 30 * 60_000);
    const bar = lanes.find((lane) => lane.lane === "dev")!.bars[0]!;
    expect(bar.lastActivityAt! + LIVENESS_MS).toBeGreaterThanOrEqual(bar.end);
  });

  it("frees the author when the Stop hook records a merged PR", () => {
    const lanes = laneBars([
      ev({ kind: "task.claimed", taskId: "issue-766", lane: "codex-3", issue: 766, at: at(0) }),
      ev({ kind: "task.pr_opened", taskId: "issue-766", lane: "codex-3", pr: 783, at: at(10) }),
      ev({ kind: "task.merged", taskId: "issue-766", actorId: "graphhelm", pr: 783,
        mergeSha: "a".repeat(40), closes: [], merger: "graphhelm", at: at(20) } as never),
    ], T0 + 30, 1000);
    expect(agentBoard([{ name: "codex-3" } as never], lanes, [], T0 + 30)[0]!.status).toBe("free");
  });
  it.each(["task.merged", "task.closed"] as const)("clears every author's awaiting PR when another slice records %s", (end) => {
    const lanes = laneBars([
      ev({ kind: "task.pr_opened", taskId: "issue-814", lane: "first", pr: 811, at: at(0) }),
      ev({ kind: "task.pr_opened", taskId: "issue-814", lane: "second", pr: 811, at: at(1) }),
      ev({ kind: end, taskId: "pr-811", pr: 811, at: at(2) }),
    ], T0 + 3, 1000);
    expect(lanes.find((lane) => lane.lane === "first")!.awaiting).toEqual([]);
    expect(lanes.find((lane) => lane.lane === "second")!.awaiting).toEqual([]);
  });
  it("ends a claimed PR when it merges or closes, including its awaiting review state", () => {
    for (const end of ["task.merged", "task.closed"] as const) {
      const lanes = laneBars([
        ev({ kind: "task.claimed", lane: "dev", taskId: "issue-803", issue: 803, at: at(0) }),
        ev({ kind: "task.pr_opened", lane: "dev", taskId: "issue-803", pr: 900, at: at(10) }),
        ev({ kind: end, taskId: "pr-900", pr: 900, at: at(20) }),
      ], T0 + 30, 1000);
      const dev = lanes.find((l) => l.lane === "dev")!;
      expect(dev.bars.some((bar) => bar.open)).toBe(false);
      expect(dev.awaiting).toEqual([]);
      expect(agentBoard([{ name: "dev" } as never], lanes, [], T0 + 30)[0]!.status).toBe("free");
    }
  });

  it("ends the review on its assigned head at verdict", () => {
    const lanes = laneBars([
      ev({ kind: "task.pr_opened", lane: "dev", pr: 901, headSha: "head1", at: at(-1) }),
      ev({ kind: "task.review_assigned", reviewer: "rev", pr: 901, headSha: "head1", at: at(0) }),
      ev({ kind: "task.review_verdict", reviewer: "rev", pr: 901, headSha: "head1", verdict: "APPROVE" as never, at: at(10) }),
    ], T0 + 20, 1000);
    expect(lanes.find((l) => l.lane === "rev")!.bars.some((bar) => bar.open)).toBe(false);
    expect(agentBoard([{ name: "rev" } as never], lanes, [], T0 + 20)[0]!.status).toBe("free");
  });
  it("ends a replaced reviewer's bar when the same head is reassigned", () => {
    const lanes = laneBars([
      ev({ kind: "task.pr_opened", lane: "dev", pr: 832, headSha: "head1", at: at(0) }),
      ev({ kind: "task.review_assigned", reviewer: "codex-review", pr: 832, headSha: "head1", at: at(1) }),
      ev({ kind: "task.review_assigned", reviewer: "codexrev-2", pr: 832, headSha: "head1", at: at(2) }),
    ], T0 + 3, 1000);
    expect(lanes.find((l) => l.lane === "codex-review")!.bars).toEqual([
      expect.objectContaining({ kind: "review", end: T0 + 2, open: false }),
    ]);
    expect(lanes.find((l) => l.lane === "codexrev-2")!.bars).toEqual([
      expect.objectContaining({ kind: "review", start: T0 + 2, open: true }),
    ]);
  });
  it("ends a placeholder review after a released claim is reclaimed (#851)", () => {
    const taskId = "issue-851";
    const lanes = laneBars([
      ev({ kind: "task.claimed", lane: "old", taskId, issue: 851, claimSequence: 1, sequence: 1, at: at(0) }),
      ev({ kind: "task.released", lane: "old", taskId, claimSequence: 1, sequence: 2, at: at(1) }),
      ev({ kind: "task.claimed", lane: "dev", taskId, issue: 851, claimSequence: 3, sequence: 3, at: at(2) }),
      ev({ kind: "task.review_assigned", reviewer: "placeholder", taskId, pr: 852, headSha: "head1", sequence: 4, at: at(3) }),
      ev({ kind: "task.pr_opened", lane: "dev", taskId, pr: 852, headSha: "head1", sequence: 5, at: at(4) }),
      ev({ kind: "task.review_assigned", reviewer: "reviewer", taskId, pr: 852, headSha: "head1", sequence: 6, at: at(5) }),
    ], T0 + 6, 1000);
    expect(lanes.find((lane) => lane.lane === "placeholder")!.bars[0]).toMatchObject({ end: T0 + 5, open: false });
    expect(lanes.find((lane) => lane.lane === "reviewer")!.bars[0]).toMatchObject({ start: T0 + 5, open: true });
  });
  it("ends the older head's review bar when a new head opens", () => {
    const lanes = laneBars([
      ev({ kind: "task.pr_opened", lane: "dev", pr: 832, headSha: "head-a", at: at(0) }),
      ev({ kind: "task.review_assigned", reviewer: "old", pr: 832, headSha: "head-a", at: at(1) }),
      ev({ kind: "task.pr_opened", lane: "dev", pr: 832, headSha: "head-b", at: at(2) }),
      ev({ kind: "task.review_assigned", reviewer: "new", pr: 832, headSha: "head-b", at: at(3) }),
    ], T0 + 4, 1000);
    expect(lanes.find((l) => l.lane === "old")!.bars[0]).toMatchObject({ end: T0 + 2, open: false });
    expect(lanes.find((l) => l.lane === "new")!.bars[0]).toMatchObject({ start: T0 + 3, open: true });
  });
  it("keeps review and author waiting when the verdict head is missing or differs", () => {
    for (const verdictHead of [undefined, "head2"]) {
      const lanes = laneBars([
        ev({ kind: "task.pr_opened", lane: "dev", taskId: "issue-803", pr: 902, headSha: "head1", at: at(0) }),
        ev({ kind: "task.review_assigned", reviewer: "rev", taskId: "issue-803", pr: 902, headSha: "head1", at: at(1) }),
        ev({ kind: "task.review_verdict", reviewer: "rev", taskId: "issue-803", pr: 902, headSha: verdictHead, verdict: "APPROVE" as never, at: at(2) }),
      ], T0 + 20, 1000);
      expect(lanes.find((l) => l.lane === "rev")!.bars.some((bar) => bar.open)).toBe(true);
      expect(lanes.find((l) => l.lane === "dev")!.awaiting).toEqual([902]);
    }
  });
  it("ends a closed PR's implement bar", () => {
    const lanes = laneBars([
      ev({ kind: "task.claimed", lane: "dev", issue: 86, taskId: "issue-86", at: at(0) }),
      ev({ kind: "task.pr_opened", lane: "dev", pr: 661, taskId: "issue-86", at: at(10) }),
      ev({ kind: "task.closed", pr: 661, taskId: "issue-86", at: at(20) }),
    ], T0 + 30, 1000);
    expect(lanes[0]!.bars[0]).toMatchObject({ end: T0 + 20, open: false });
  });

  it("ends both slices holding a merged PR", () => {
    const lanes = laneBars([
      ev({ kind: "task.claimed", lane: "a", taskId: "issue-1", at: at(0) }),
      ev({ kind: "task.pr_opened", lane: "a", pr: 10, taskId: "issue-1", at: at(10) }),
      ev({ kind: "task.claimed", lane: "b", taskId: "issue-1", at: at(20) }),
      ev({ kind: "task.pr_opened", lane: "b", pr: 10, taskId: "issue-1", at: at(30) }),
      ev({ kind: "task.merged", pr: 10, taskId: "pr-10", at: at(40) }),
    ], T0 + 50, 1000);
    expect(lanes.flatMap((l) => l.bars).filter((b) => b.kind === "implement").map((b) => [b.end, b.open])).toEqual([[T0 + 40, false], [T0 + 40, false]]);
  });

  it("ends an absent reviewer's bar when its PR closes or merges", () => {
    for (const end of ["task.closed", "task.merged"] as const) {
      const lanes = laneBars([
        ev({ kind: "task.review_assigned", reviewer: "rev", pr: 10, at: at(0) }),
        ev({ kind: end, pr: 10, at: at(20) }),
      ], T0 + 30, 1000);
      expect(lanes[0]!.bars[0]).toMatchObject({ end: T0 + 20, open: false });
    }
  });

  it("ends a released claim at its release record", () => {
    const lanes = laneBars([
      ev({ kind: "task.claimed", lane: "dev", taskId: "issue-1", sequence: 1, at: at(0) }),
      ev({ kind: "task.released", lane: "dev", taskId: "issue-1", claimSequence: 1, sequence: 2, at: at(20) } as never),
    ], T0 + 30, 1000);
    expect(lanes[0]!.bars[0]).toMatchObject({ end: T0 + 20, open: false });
  });

  it("uses the PR title, or the issue title before PR, in bar labels", () => {
    const lanes = laneBars([
      ev({ kind: "task.claimed", lane: "dev", taskId: "issue-1", issue: 1, title: "Fix confusing timeline", at: at(0) } as never),
      ev({ kind: "task.pr_opened", lane: "dev", taskId: "issue-1", pr: 10, title: "Show truthful task bars", at: at(10) } as never),
    ], T0 + 20, 1000);
    expect(lanes[0]!.bars[0]!.displayLabel).toBe("#10 Show truthful task bars");
  });
  it("implement then review then merge", () => {
    const lanes = laneBars([
      ev({ kind: "task.claimed", lane: "dev", pr: 9, at: at(0) }),
      ev({ kind: "task.pr_opened", lane: "dev", pr: 9, headSha: "head9", at: at(50) }),
      ev({ kind: "task.review_assigned", reviewer: "rev", pr: 9, headSha: "head9", at: at(100) }),
      ev({ kind: "task.review_verdict", reviewer: "rev", verdict: "APPROVE" as never, pr: 9, headSha: "head9", at: at(200) }),
      ev({ kind: "task.merged", pr: 9, at: at(300) }),
    ], T0 + 400, 1000);
    const dev = lanes.find((l) => l.lane === "dev")!;
    const rev = lanes.find((l) => l.lane === "rev")!;
    expect(dev.bars).toEqual([{ kind: "implement", label: "#9", start: T0, end: T0 + 100, open: false, taskId: "t1", since: T0, lastActivityAt: T0, endedBy: { kind: "review_assigned", actor: "a" } }]);
    expect(rev.bars.map((b) => [b.kind, b.start - T0, b.end - T0, b.open])).toEqual([["review", 100, 200, false]]);
  });

  it("open bar runs to now", () => {
    const [lane] = laneBars([ev({ kind: "task.claimed", lane: "dev", at: at(0) })], T0 + 50, 1000);
    expect(lane.bars[0]).toMatchObject({ end: T0 + 50, open: true });
  });

  it("a release closes only the latest open claim bar", () => {
    const lanes = laneBars([
      ev({ kind: "task.claimed", lane: "dev", taskId: "issue-729", sequence: 10, at: at(0) }),
      ev({ kind: "task.claimed", lane: "dev", taskId: "issue-729", sequence: 11, at: at(10) }),
      ev({ kind: "task.released", lane: "dev", taskId: "issue-729", sequence: 12, claimSequence: 11, reason: "Stopped", at: at(20) } as never),
    ], T0 + 30, 1000);
    expect(lanes[0]!.bars.map((bar) => [bar.start - T0, bar.end - T0, bar.open])).toEqual([[0, 30, true], [10, 20, false]]);
  });

  it("a released sole claim leaves its lane Free on the Agents board", () => {
    const lanes = laneBars([
      ev({ kind: "task.claimed", lane: "dev", taskId: "issue-803", issue: 803, sequence: 20, at: at(0) }),
      ev({ kind: "task.released", lane: "dev", taskId: "issue-803", sequence: 21, claimSequence: 20, reason: "Stopped", at: at(10) } as never),
    ], T0 + 20, 1000);
    expect(lanes[0]!.bars).toEqual([expect.objectContaining({ open: false, end: T0 + 10 })]);
    expect(agentBoard([{ name: "dev" } as never], lanes, [], T0 + 20)[0]!.status).toBe("free");
  });

  it("silence threshold", () => {
    const events = [ev({ kind: "task.review_assigned", reviewer: "rev", at: at(0) })];
    expect(laneBars(events, T0 + LIVENESS_MS - 1, LIVENESS_MS * 2)[0].silent).toBe(false);
    expect(laneBars(events, T0 + LIVENESS_MS, LIVENESS_MS * 2)[0].silent).toBe(true);
  });

  it("an open implement bar goes silent after LIVENESS_MS too (#591: an offline author is not working)", () => {
    expect(laneBars([ev({ kind: "task.claimed", lane: "dev", at: at(0) })], T0 + LIVENESS_MS - 1, LIVENESS_MS * 4)[0].silent).toBe(false);
    expect(laneBars([ev({ kind: "task.claimed", lane: "dev", at: at(0) })], T0 + LIVENESS_MS, LIVENESS_MS * 4)[0].silent).toBe(true);
  });

  it("window clamps and drops", () => {
    const lanes = laneBars([
      ev({ kind: "task.claimed", lane: "old", taskId: "o", at: at(0) }),
      ev({ kind: "task.review_assigned", reviewer: "x", taskId: "o", at: at(10) }),
      ev({ kind: "task.claimed", lane: "dev", taskId: "n", at: at(500) }),
    ], T0 + 1000, 600);
    expect(lanes.find((l) => l.lane === "old")!.bars).toEqual([]);
    expect(lanes.find((l) => l.lane === "x")!.bars[0].start).toBe(T0 + 400);
  });

  it("a verdict ends the assigned review for either outcome", () => {
    for (const v of ["APPROVE-WITH-RISK", "BLOCK"] as const) {
      const lanes = laneBars([
        ev({ kind: "task.pr_opened", lane: "dev", pr: 902 + (v === "BLOCK" ? 1 : 0), headSha: "head2", at: at(-1) }),
        ev({ kind: "task.review_assigned", reviewer: "rev", pr: 902 + (v === "BLOCK" ? 1 : 0), headSha: "head2", at: at(0) }),
        ev({ kind: "task.review_verdict", reviewer: "rev", pr: 902 + (v === "BLOCK" ? 1 : 0), headSha: "head2", verdict: v as never, at: at(10) }),
      ], T0 + 20, 1000);
      expect(lanes.find((l) => l.lane === "rev")!.bars.some((b) => b.open)).toBe(false);
    }
  });

  it("a repeated assign closes the first bar", () => {
    const lanes = laneBars([
      ev({ kind: "task.pr_opened", lane: "dev", pr: 903, headSha: "head3", at: at(-1) }),
      ev({ kind: "task.review_assigned", reviewer: "rev", pr: 903, headSha: "head3", at: at(0) }),
      ev({ kind: "task.review_assigned", reviewer: "rev", pr: 903, headSha: "head3", at: at(10) }),
      ev({ kind: "task.review_verdict", reviewer: "rev", pr: 903, headSha: "head3", verdict: "BLOCK" as never, at: at(20) }),
    ], T0 + LIVENESS_MS * 2, LIVENESS_MS * 4);
    expect(lanes[0].bars.some((b) => b.open)).toBe(false);
    expect(lanes[0].silent).toBe(false);
  });

  it("merged closes every open bar of the task", () => {
    const lanes = laneBars([
      ev({ kind: "task.claimed", lane: "dev", at: at(0) }),
      ev({ kind: "task.merged", at: at(50) }),
    ], T0 + 100, 1000);
    expect(lanes[0].bars[0]).toMatchObject({ end: T0 + 50, open: false });
  });

  it("skips events with an invalid timestamp", () => {
    const lanes = laneBars([ev({ kind: "task.claimed", lane: "dev", at: "garbage" })], T0, 1000);
    expect(lanes).toEqual([]);
  });

  it("#549: bars are kept per PR, so another slice's merge or claim never closes #581's review", () => {
    const t = "issue-549";
    const lanes = laneBars([
      ev({ kind: "task.claimed", lane: "author", taskId: t, branch: "s1", at: at(0) } as never),
      ev({ kind: "task.pr_opened", lane: "author", taskId: t, pr: 571, headSha: "aaaa1111", at: at(10) } as never),
      ev({ kind: "task.claimed", lane: "author", taskId: t, branch: "s2", at: at(20) } as never),
      ev({ kind: "task.pr_opened", lane: "author", taskId: t, pr: 581, headSha: "66613c95", at: at(30) } as never),
      ev({ kind: "task.review_assigned", reviewer: "gh-claude-8", taskId: t, pr: 581, headSha: "66613c95", at: at(40) } as never),
      ev({ kind: "task.merged", taskId: t, pr: 571, at: at(50) }),
      ev({ kind: "task.claimed", lane: "author", taskId: t, branch: "s3", at: at(60) } as never),
    ], T0 + 100, 1000);
    const rev = lanes.find((l) => l.lane === "gh-claude-8")!;
    expect(rev.bars).toEqual([expect.objectContaining({ kind: "review", label: "#581", open: true })]);
    const author = lanes.find((l) => l.lane === "author")!;
    expect(author.bars.filter((b) => b.open).map((b) => b.label)).toEqual([t]);
    const row = agentBoard([{ name: "gh-claude-8" } as never], lanes, [], T0 + 100).find((r) => r.name === "gh-claude-8")!;
    expect(row).toMatchObject({ stage: "review", pr: 581 });
    expect(row.status).not.toBe("free");
  });
});

describe("packBars (#591)", () => {
  const bars = [
    { start: 0, end: 100 }, { start: 10, end: 50 }, { start: 20, end: 30 }, { start: 50, end: 90 }, { start: 100, end: 120 }, { start: 30, end: 60 },
  ];
  it("no two bars in one sub-row overlap", () => {
    const { row } = packBars(bars);
    for (let i = 0; i < bars.length; i++) for (let j = i + 1; j < bars.length; j++) {
      if (row[i] === row[j]) expect(bars[i]!.end <= bars[j]!.start || bars[j]!.end <= bars[i]!.start).toBe(true);
    }
  });
  it("uses the minimum number of sub-rows (the most bars open at one instant)", () => {
    // At t=25 three bars are open (0-100, 10-50, 20-30); at t=55, three (0-100, 50-90, 30-60).
    expect(packBars(bars).rows).toBe(3);
    expect(packBars([{ start: 0, end: 10 }, { start: 10, end: 20 }]).rows).toBe(1);
    expect(packBars([]).rows).toBe(1);
  });
  it("a lane named TBD or nothing is a placeholder", () => {
    expect(["TBD", "tbd ", "", "  "].map(placeholderLane)).toEqual([true, true, true, true]);
    expect(placeholderLane("gh-claude-5")).toBe(false);
  });
});
