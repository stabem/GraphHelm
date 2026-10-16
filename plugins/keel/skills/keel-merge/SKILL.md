---
name: keel-merge
description: "Use when merging a change you approved under Keel: what to recheck at the merge and what to record after it."
---

How to use this skill:

- If the project has its own `KEEL.md` at its root, follow that file and the `keel/` files it names.
  The text below is the bundled copy, for projects that have none.
- In the text below, `keel/KEEL_PLAN.md`, `keel/KEEL_EXEC.md`, `keel/KEEL_REVIEW.md` and
  `keel/KEEL_MERGE.md` are this plugin's `keel-plan`, `keel-exec`, `keel-review` and `keel-merge`
  skills, and `KEEL.md` is its `keel` skill. `docs/keel/KEEL_SPEC.md` exists only in the GraphHelm
  repository.
- If the `graphhelm` CLI is on PATH, run `graphhelm keel check --diff <base>..<head> --card <card>`
  for the rules marked as measured. If it is not, the reviewer checks those rules by reading and says
  so in the review.
- Nothing here needs a GraphHelm Runtime, MCP server, task records or journeys. Where the text names
  them, they apply only inside GraphHelm: elsewhere, prove the promise with the project's own test
  runner.

<!-- keel source: keel/KEEL_MERGE.md (generated; edit the source, then run tools/keel-plugin/sync_keel_plugin.py) -->

# KEEL_MERGE

Step 4. Only the reviewer who approved merges. Read [`KEEL.md`](../KEEL.md) first.

1. **Pin the head.** Merge exactly the commit you reviewed:
   `gh pr merge <N> --squash --match-head-commit <sha>`.
2. **Main moved, run again.** If main moved after your review, rerun the reached plan on the new
   main plus the head before you merge. Two changes that each pass alone can break together.
3. **Closing keywords** in the body and the commits must match what the change closes.
4. **Check after.** Read what landed, then run the change's main proof on main and confirm it is
   green. If it is red, the merge is not delivered: fix main first.
5. **Clean up what you created.** The author deletes the branch (unless a pull request is stacked
   on it), the worktree and the build directories. Nobody deletes another lane's.

Delivered means merged into main and green there.
