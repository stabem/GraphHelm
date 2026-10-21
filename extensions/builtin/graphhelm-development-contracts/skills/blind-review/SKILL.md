---
name: blind-review
description: "Review one PR blind, as the single assigned reviewer: read the earlier comments, run the tests the change reaches and the Keel check, try to break the user-visible promise, and post APPROVE, APPROVE-WITH-RISK or BLOCK with the identity line. Use when you are assigned as a PR's reviewer."
---

# Blind review

Step 4 of the per-task graph. One reviewer per PR, assigned once (`docs/process/DELIVERY.md` §4).
If you helped write the change, or your brief carries the author's reasoning, a finding to look
for, a summary of the change or an expected verdict, you are not blind: say so and do not review.

**Load the project's `KEEL.md` and `keel/KEEL_REVIEW.md` first, when the project has them; their rules bind and win over this skill where they differ.**

## Emits and reads

- Reads: the PR (diff, body, every earlier comment), `AGENTS.md`, `docs/process/DELIVERY.md`,
  the task's `keel.plan` record when one exists.
- Emits: one PR comment whose first line is `Session: <name> · Head: <sha8>` (add
  `(blind subagent)` after the name for a subagent) and whose verdict word is `APPROVE`,
  `APPROVE-WITH-RISK` (name the risk) or `BLOCK` (name the defect); and the record
  `task.review_verdict`, signed as your own lane once the comment is posted:
  `python tools/task-record/task_record.py --lane <you> review_verdict --issue <N> --pr <P> --head <sha> --verdict <WORD> --comment-url <url>`
  (from a GraphHelm checkout; token and defaults: `docs/process/DELIVERY.md` "Task records").

## Method

1. At the start of each review or re-review, fetch the head and body together with
   `gh pr view <N> --json headRefOid,body`. Save that exact body as UTF-8 Markdown under the
   reviewer's lane scratch directory, outside the worktree. Record the PR number, head SHA, fetch
   time and body digest beside the saved file. Use that saved body for the Keel check; do not reuse
   a card saved during an earlier review. Pin the fetched head: everything below is about that sha.
2. Read every earlier comment. An open BLOCK on this head must be answered in your review: does it
   still hold, and why.
3. Reuse the author's reached-plan receipt (the `run_reached.py` JSON named in the PR body) when
   its head equals the pinned head and nothing in it is failed or pending; then run the
   end-to-end proof of the promise, any cell you doubt, and the lints (fmt, clippy) on touched
   code. Rerun the whole reached plan only when the receipt is missing, stale, red or pending, or
   when the diff touches persistence, permissions, security, compatibility or the plan selector
   itself. Name what you reused and what you reran, each command and its result.
4. Run the Keel check against the body saved in step 1 and paste its output:
   `graphhelm --json keel check --diff <merge-base>..<head> --card <saved-body.md>`. State the
   source PR and fetch time (or digest) in the review. Exit 2 with
   `keel.scope.path_outside_card` is a finding for the author. If the head changed, restart the
   head-dependent proof; if only the body changed on the same head, fetch and save it again and
   rerun the Keel check without rerunning unrelated tests.
5. Attack the promise, not the prose: reproduce the user-visible journey (or the invariant), try
   the error, empty and recovery paths, and look for one reproducible counterexample. Shrink it to
   the fewest steps that still fail. One severe, reproducible counterexample outweighs any number
   of passing checks.
6. Decide. A BLOCK names the defect and the smallest change that would answer it. After a BLOCK,
   the same reviewer re-checks only the fix diff and the reached tests.
7. On APPROVE, merge with the `merge` skill.

## Completion

Done when the verdict comment is posted on the pinned head with commands and results, and (on
approve) the `merge` skill has run.


### Deliberate review waiting and resumption

When you actually decide to suspend an assigned review or merge for an external condition, read the current task binding and its `reviewContextSequence` / `reviewActivityRevision`. Generate your own `task.review_activity` with `tools/task-record/task_record.py --lane <you> review_activity --issue <issue> --repo <owner/repo> --pr <pr> --head <full-head> --review-context-sequence <current-context> --revision <watermark+1> --activity waiting --reason <observed-reason> --dry-run`, then submit through MCP `signal` with your own lane credential. Confirm task readback before saying it was recorded. Preserve verdict, approval, head, author and assignment; never manufacture a BLOCK or author heartbeat for a wait.

When you actually resume, use the current binding and next revision with `--activity resumed` and no reason. Keep the exact envelope/key for response-loss retries. A historical duplicate acknowledgement does not prove its old reason remains current. On sequence conflict, read back and make at most one same-envelope retry only if the binding still applies; abandon a stale statement instead of silently rebinding it. An older Runtime refusing this kind is unsupported: report missing task evidence, with no fallback write. Do not automatically repeat waits to maintain freshness, infer dependency resolution from text, or sign as another reviewer. This records a deliberate decision; it does not wake an idle host or prove a worker is running.
