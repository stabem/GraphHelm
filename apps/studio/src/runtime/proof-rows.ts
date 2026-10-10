import type { TaskState } from "./team-tasks";
import type { StepStatus } from "./mission";
import { openBlock, sha8 } from "./mission";
import { ownerLane, type StageHealth } from "./stage-health";
import type { WorkStage } from "./work-groups";

/**
 * #735: the Proof view of issue work, one row per PR, read only from the task's own records
 * (TaskState rounds, verdicts, merge sha) and, when the PR serves a journey step, that step's replay.
 */
export interface ProofLink { contractId: string; stepId: string; stepIndex: number; status: StepStatus; frame: string | null }
export interface ProofInput { task: TaskState; stage: WorkStage; health: StageHealth | null; link: ProofLink | null; open: boolean }

export type ProofTone = "proven" | "merged" | "work" | "stalled" | "ready";
export type ChipTone = "ok" | "no" | "run" | "warn";
export type EvidenceMark = "ok" | "no" | "warn" | "wait";
export interface ProofChip { stage: string; who: string; tone: ChipTone }
export interface ProofEvidence { mark: EvidenceMark; text: string; href: string | null }
export interface ProofCall { kind: "ask" | "test" | "pr"; label: string; hint: string; lane: string | null; primary: boolean }
export interface ProofRowData {
  key: string; n: number; pr: number | null; title: string; promise: string | null;
  status: string; tone: ProofTone; alarm: boolean;
  frame: { src: string | null; caption: string; stepId: string | null; contractId: string | null };
  chips: ProofChip[]; evidence: ProofEvidence[]; call: ProofCall; prUrl: string | null; open: boolean;
}

export const EVIDENCE_SYM: Record<EvidenceMark, string> = { ok: "✓", no: "✕", warn: "!", wait: "○" };

const STAGE_STATUS: Record<WorkStage, string> = {
  plan: "Planning", implement: "Building", review: "In review", fix: "Fixing", merge: "Merging", merged: "Merged · not proven", proven: "Proven",
};
const REPLAY: Record<StepStatus, [EvidenceMark, string]> = {
  proven: ["ok", "PASS"], failed: ["no", "FAIL"], needs_you: ["warn", "SKIPPED"], preview_only: ["warn", "preview only"], not_run: ["wait", "not replayed yet"],
};

export function proofRow(input: ProofInput, n: number): ProofRowData {
  const { task: t, stage, health, link, open } = input;
  const stalled = open && stage !== "fix" && (health?.flag === "stalled" || health?.flag === "blocked");
  const block = openBlock(t);
  const pr = t.pr;
  const prUrl = t.repoUrl && pr !== null ? `${t.repoUrl}/pull/${pr}` : null;
  const lane = t.lane ?? "—";

  // Chain of custody: impl, each BLOCK round and its fix, the live or approving review, the merge, the replay.
  const chips: ProofChip[] = [{ stage: "impl", who: lane, tone: stage === "implement" || stage === "plan" ? "run" : "ok" }];
  const evidence: ProofEvidence[] = [];
  t.rounds.forEach((r, i) => {
    chips.push({ stage: i === 0 ? "rev" : "re-rev", who: `${r.reviewer || "—"} BLOCK`, tone: "no" });
    evidence.push({ mark: "no", text: `BLOCK at ${sha8(r.headSha) ?? "—"} — comment`, href: r.commentUrl || null });
    if (r.fixHead) {
      chips.push({ stage: "fix", who: lane, tone: "ok" });
      evidence.push({ mark: "ok", text: `fix pushed ${sha8(r.fixHead)}`, href: null });
    } else if (block && i === t.rounds.length - 1) chips.push({ stage: "fix", who: lane, tone: "run" });
  });
  const revStage = t.rounds.length > 0 ? "re-rev" : "rev";
  const who = t.reviewers.join(", ") || "—";
  if (t.step === "merge" || t.step === "merged") {
    if (t.reviewers.length > 0) {
      chips.push({ stage: revStage, who: `${who} APPROVE`, tone: "ok" });
      evidence.push({ mark: "ok", text: `APPROVE by ${who}`, href: null });
    } else chips.push({ stage: revStage, who: "not recorded", tone: "warn" });
  } else if (t.step === "review" && !block) {
    chips.push({ stage: revStage, who: t.reviewers.length > 0 ? who : "unassigned", tone: stalled ? "no" : "run" });
  }
  if (t.step === "merged") {
    chips.push({ stage: "merge", who: sha8(t.mergeSha) ?? (pr !== null ? `#${pr}` : "—"), tone: "ok" });
    evidence.push({ mark: "ok", text: `merged ${sha8(t.mergeSha) ?? ""}`.trim(), href: null });
  }
  if (link) {
    const [mark, res] = REPLAY[link.status];
    if (t.step === "merged" && link.status !== "not_run") {
      chips.push({ stage: "prove", who: link.status === "needs_you" ? "needs a person" : `step ${link.stepIndex + 1}`, tone: mark === "wait" ? "run" : mark });
    }
    evidence.push({ mark, text: `journey replay · step ${link.stepIndex + 1} ${res}`, href: null });
  }
  if (stalled && health) evidence.push({ mark: "warn", text: health.text, href: null });
  if (evidence.length === 0) evidence.push({ mark: "wait", text: open ? "no verdict recorded yet" : "no verdict recorded", href: null });

  const tone: ProofTone = stalled || stage === "fix" ? "stalled" : stage === "proven" ? "proven" : stage === "merged" ? "merged" : t.lane === null ? "ready" : "work";
  const alarm = tone === "stalled" || link?.status === "failed" || (t.step === "merged" && link?.status === "needs_you");
  const label = stalled ? "Stalled" : stage === "review" && t.rounds.length > 0 ? "In re-review" : STAGE_STATUS[stage];
  const status = `${label}${pr !== null ? ` · #${pr}` : ""}`;

  let call: ProofCall;
  const owner = ownerLane(t);
  if (stage === "fix" || stalled) {
    call = { kind: "ask", label: `Ask ${owner ?? lane} for status`, lane: owner, primary: true,
      hint: stage === "fix" ? `${block?.reviewer || "A reviewer"} blocked it; ${lane} owes the fix.` : "No record for a while. Ask before you hand it to someone else." };
  } else if (stage === "merged" && link) {
    call = { kind: "test", label: "Open test", lane: null, primary: true, hint: `Merged is not done. The replay must prove step ${link.stepIndex + 1}.` };
  } else {
    const hint = stage === "proven" ? "Proven by the journey replay."
      : stage === "merged" ? "Merged. No journey names this work, so no replay can prove it."
      : stage === "review" ? "A reviewer is on it. Nothing to validate yet."
      : stage === "merge" ? "Approved; the merge is next."
      : t.lane === null ? "Nobody has claimed this yet." : "Still being written.";
    call = { kind: "pr", label: "Open PR", lane: null, primary: false, hint: prUrl ? hint : `${hint} No PR link recorded.` };
  }

  const frame = !link ? { src: null, caption: "no journey linked", stepId: null, contractId: null }
    : {
      src: link.frame, stepId: link.stepId, contractId: link.contractId,
      caption: link.frame ? `step ${link.stepIndex + 1} · frame recorded` : link.status === "not_run" ? "not replayed yet" : `step ${link.stepIndex + 1} · no frame`,
    };

  return {
    key: t.key, n, pr, title: t.prTitle ?? t.title ?? (pr !== null ? `#${pr}` : t.key), promise: t.summary ?? t.prSummary ?? null,
    status, tone, alarm, frame, chips, evidence, call, prUrl, open,
  };
}

/** Open work first (in the order given), then merged work; numbered in that order. */
export function proofRows(inputs: ProofInput[]): ProofRowData[] {
  const ordered = [...inputs.filter((i) => i.open), ...inputs.filter((i) => !i.open)];
  return ordered.map((i, k) => proofRow(i, k + 1));
}
