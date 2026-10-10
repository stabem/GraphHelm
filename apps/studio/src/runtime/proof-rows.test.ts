import { describe, expect, it } from "vitest";
import { proofRow, proofRows, type ProofInput } from "./proof-rows";
import type { TaskState } from "./team-tasks";
import type { StageHealth } from "./stage-health";

const ts = (over: Partial<TaskState> = {}): TaskState => ({
  key: "k", taskId: "k", branch: null, issue: 549, pr: 690, lane: "codex-6", headSha: "aaaaaaaa11", journeys: [], step: "implement",
  blockedBy: null, reviewers: [], mergeSha: null, repoUrl: "https://github.com/o/r", strayVerdicts: [], title: "Issue title", summary: "Lanes stop going red.",
  prTitle: "fix(tests): scale ceilings", prSummary: "PR summary", critic: null, recordedHeads: [], parent: null, rounds: [],
  clock: { since: null, spent: {} }, lastSequence: 0, ...over,
});
const input = (over: Partial<ProofInput> = {}): ProofInput => ({ task: ts(), stage: "implement", health: null, link: null, open: true, ...over });
const stalledHealth: StageHealth = { flag: "stalled", text: "codex-9 silent 2h 50m", tone: "red", elapsedMs: null, elapsed: null };
const round = { reviewer: "codexrev-5", headSha: "96874d32ff", commentUrl: "https://github.com/o/r/pull/690#c1", fixHead: null, blockedAt: null, fixedAt: null };

describe("proofRow", () => {
  it("Building: PR title, issue summary as promise, no journey linked, Open PR", () => {
    const r = proofRow(input(), 1);
    expect(r).toMatchObject({ n: 1, title: "fix(tests): scale ceilings", promise: "Lanes stop going red.", status: "Building · #690", tone: "work", alarm: false,
      prUrl: "https://github.com/o/r/pull/690" });
    expect(r.frame).toEqual({ src: null, caption: "no journey linked", stepId: null, contractId: null });
    expect(r.chips).toEqual([{ stage: "impl", who: "codex-6", tone: "run" }]);
    expect(r.evidence).toEqual([{ mark: "wait", text: "no verdict recorded yet", href: null }]);
    expect(r.call).toMatchObject({ kind: "pr", label: "Open PR" });
  });

  it("Fixing: BLOCK chip and linked evidence, the author is asked for status", () => {
    const t = ts({ step: "review", reviewers: ["codexrev-5"], rounds: [round], blockedBy: { reviewer: "codexrev-5", headSha: "96874d32ff", commentUrl: round.commentUrl } });
    const r = proofRow(input({ task: t, stage: "fix" }), 1);
    expect(r).toMatchObject({ status: "Fixing · #690", tone: "stalled", alarm: true });
    expect(r.chips.map((c) => `${c.stage} ${c.who} ${c.tone}`)).toEqual(["impl codex-6 ok", "rev codexrev-5 BLOCK no", "fix codex-6 run"]);
    expect(r.evidence[0]).toEqual({ mark: "no", text: "BLOCK at 96874d32 — comment", href: round.commentUrl });
    expect(r.call).toMatchObject({ kind: "ask", label: "Ask codex-6 for status", lane: "codex-6", primary: true });
  });

  it("In re-review after a pushed fix: re-rev chip, fix evidence, Open PR", () => {
    const t = ts({ step: "review", reviewers: ["codexrev-2"], rounds: [{ ...round, reviewer: "codexrev-2", fixHead: "15f4aef0aa" }] });
    const r = proofRow(input({ task: t, stage: "review" }), 2);
    expect(r.status).toBe("In re-review · #690");
    expect(r.chips.map((c) => c.stage)).toEqual(["impl", "rev", "fix", "re-rev"]);
    expect(r.evidence.map((e) => e.text)).toEqual(["BLOCK at 96874d32 — comment", "fix pushed 15f4aef0"]);
    expect(r.call).toMatchObject({ kind: "pr", label: "Open PR", hint: "A reviewer is on it. Nothing to validate yet." });
  });

  it("Stalled: health says so, the owning lane is asked", () => {
    const t = ts({ step: "merge", reviewers: ["codex-review"] });
    const r = proofRow(input({ task: t, stage: "merge", health: stalledHealth }), 1);
    expect(r).toMatchObject({ status: "Stalled · #690", alarm: true });
    expect(r.evidence).toContainEqual({ mark: "warn", text: "codex-9 silent 2h 50m", href: null });
    expect(r.evidence).toContainEqual({ mark: "ok", text: "APPROVE by codex-review", href: null });
    expect(r.call).toMatchObject({ kind: "ask", label: "Ask codex-6 for status" });
  });

  it("Merged, not proven, with a journey: merge sha, replay line, real frame, Open test", () => {
    const t = ts({ step: "merged", reviewers: ["gh-claude-6"], mergeSha: "4f1ead4c99" });
    const link = { contractId: "j", stepId: "s2", stepIndex: 1, status: "not_run" as const, frame: null };
    const r = proofRow(input({ task: t, stage: "merged", link, open: false }), 3);
    expect(r).toMatchObject({ status: "Merged · not proven · #690", tone: "merged", alarm: false });
    expect(r.chips.at(-1)).toEqual({ stage: "merge", who: "4f1ead4c", tone: "ok" });
    expect(r.evidence.map((e) => e.text)).toEqual(["APPROVE by gh-claude-6", "merged 4f1ead4c", "journey replay · step 2 not replayed yet"]);
    expect(r.frame).toMatchObject({ caption: "not replayed yet", contractId: "j", stepId: "s2" });
    expect(r.call).toMatchObject({ kind: "test", label: "Open test", primary: true });
    const seen = proofRow(input({ task: t, stage: "merged", link: { ...link, frame: "blob:f" }, open: false }), 3);
    expect(seen.frame).toMatchObject({ src: "blob:f", caption: "step 2 · frame recorded" });
  });

  it("Proven: prove chip and PASS line; merged without a recorded reviewer says so", () => {
    const t = ts({ step: "merged", mergeSha: "a97e76fd00" });
    const r = proofRow(input({ task: t, stage: "proven", link: { contractId: "j", stepId: "s", stepIndex: 0, status: "proven", frame: null }, open: false }), 1);
    expect(r).toMatchObject({ status: "Proven · #690", tone: "proven" });
    expect(r.chips).toContainEqual({ stage: "rev", who: "not recorded", tone: "warn" });
    expect(r.chips.at(-1)).toEqual({ stage: "prove", who: "step 1", tone: "ok" });
    expect(r.evidence).toContainEqual({ mark: "ok", text: "journey replay · step 1 PASS", href: null });
    expect(r.call).toMatchObject({ kind: "pr", hint: "Proven by the journey replay." });
  });
});

describe("proofRows", () => {
  it("open work first, merged last, numbered in that order", () => {
    const rows = proofRows([
      input({ task: ts({ key: "m", pr: 1, step: "merged" }), stage: "merged", open: false }),
      input({ task: ts({ key: "o", pr: 2 }), stage: "implement" }),
    ]);
    expect(rows.map((r) => [r.key, r.n])).toEqual([["o", 1], ["m", 2]]);
  });
});
