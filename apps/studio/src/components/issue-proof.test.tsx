import { describe, expect, it, vi } from "vitest";
import { render, screen, within } from "@testing-library/react";
import { fastUserEvent } from "../test/user-event";
import { IssueProof } from "./issue-proof";
import { proofRows, type ProofInput } from "../runtime/proof-rows";
import type { TaskState } from "../runtime/team-tasks";

const userEvent = fastUserEvent();
const ts = (over: Partial<TaskState>): TaskState => ({
  key: "k", taskId: "k", branch: null, issue: 549, pr: 1, lane: "codex-6", headSha: "aaaaaaaa", journeys: [], step: "implement",
  blockedBy: null, reviewers: [], mergeSha: null, repoUrl: "https://github.com/o/r", strayVerdicts: [], title: "T", summary: "The promise.",
  prTitle: null, prSummary: null, critic: null, recordedHeads: [], parent: null, rounds: [], clock: { since: null, spent: {} }, lastSequence: 0, ...over,
});
const block = { reviewer: "rev-5", headSha: "33f23336aa", commentUrl: "https://github.com/o/r/pull/690#c", fixHead: null, blockedAt: null, fixedAt: null };
const inputs: ProofInput[] = [
  { task: ts({ key: "f", pr: 690, prTitle: "Fix ceilings", step: "review", reviewers: ["rev-5"], rounds: [block], blockedBy: { reviewer: "rev-5", headSha: block.headSha, commentUrl: block.commentUrl } }),
    stage: "fix", health: null, link: null, open: true },
  { task: ts({ key: "m", pr: 655, prTitle: "Merged work", step: "merged", reviewers: ["gh-claude-6"], mergeSha: "a1d6e72800" }),
    stage: "merged", health: null, link: { contractId: "j", stepId: "s", stepIndex: 0, status: "not_run", frame: null }, open: false },
];

function setup() {
  const onOpenTest = vi.fn(), onAsk = vi.fn(() => Promise.resolve());
  render(<IssueProof label="#549 Build" cap="Issue #549 · proof" rows={proofRows(inputs)} onOpenTest={onOpenTest} onAsk={onAsk} />);
  return { onOpenTest, onAsk };
}

describe("IssueProof", () => {
  it("header asks the question; open rows show custody, evidence link and the call", async () => {
    const { onAsk } = setup();
    expect(screen.getByRole("heading", { name: "Can I trust “#549 Build”?" })).toBeInTheDocument();
    const row = within(screen.getByRole("list", { name: "PRs" })).getByRole("listitem");
    expect(row).toHaveAttribute("data-alarm", "true");
    expect(row).toHaveTextContent("Fix ceilings");
    expect(row).toHaveTextContent("The promise.");
    expect(row).toHaveTextContent("Fixing · #690");
    expect(row).toHaveTextContent("no journey linked");
    expect(within(row).getByRole("link", { name: "BLOCK at 33f23336 — comment" })).toHaveAttribute("href", block.commentUrl);
    await userEvent.click(within(row).getByRole("button", { name: "Ask codex-6 for status" }));
    expect(onAsk).toHaveBeenCalledWith(expect.objectContaining({ key: "f" }));
    expect(await within(row).findByRole("button", { name: "Asked" })).toBeDisabled();
  });

  it("merged rows fold under Merged · N; a linked journey opens the test from frame and button", async () => {
    const { onOpenTest } = setup();
    expect(screen.queryByRole("list", { name: "Merged PRs" })).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "▸ Merged · 1" }));
    const row = within(screen.getByRole("list", { name: "Merged PRs" })).getByRole("listitem");
    expect(row).toHaveTextContent("Merged · not proven · #655");
    expect(row).toHaveTextContent("merged a1d6e728");
    expect(row).toHaveTextContent("not replayed yet");
    await userEvent.click(within(row).getByRole("button", { name: "Open the test canvas for Merged work" }));
    await userEvent.click(within(row).getByRole("button", { name: "Open test" }));
    expect(onOpenTest).toHaveBeenCalledTimes(2);
  });

  it("with nothing open, the merged rows start unfolded", () => {
    render(<IssueProof label="#684 X" cap="c" rows={proofRows([inputs[1]!])} onOpenTest={vi.fn()} />);
    expect(screen.getByRole("button", { name: "▾ Merged · 1" })).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("list", { name: "Merged PRs" })).toBeInTheDocument();
  });
});
