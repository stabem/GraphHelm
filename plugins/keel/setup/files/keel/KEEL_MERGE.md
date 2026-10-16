# KEEL_MERGE

Step 4. Only the reviewer who approved merges. Read [`KEEL.md`](../KEEL.md) first.

1. **Pin the head.** `gh pr view <N> --json headRefOid` must equal the sha the review named, and
   no BLOCK on that head is left unanswered. Merge exactly that commit:
   `gh pr merge <N> --squash --match-head-commit <sha>`.
2. **Main gets one whole run after merges.** Two changes that each pass alone can break
   together, so after merges land the orchestrator owns one whole-package run on main, at most
   once an hour while main moves. A red main is fixed first, by a revert or a fix-forward PR,
   before any new merge.
3. **Closing keywords** in the body and the commits must match what the change closes. Never
   write a closing keyword (`close`, `fix`, `resolve` and their forms) next to an issue you do not
   mean to close, not even inside a negation; write `Refs #N` instead.
4. **Check after.** Read what landed, then run the change's main proof on main and confirm it is
   green. If it is red, the merge is not delivered: fix main first.
5. **Clean up the PR.** After the merge lands the merger runs `graphhelm workspace sweep --apply --merged-branch <branch> --repo <repo> --pr <N> --head <sha> [--target <dir>]... [--keep-worktree <dir>]`: it stops the processes inside the PR's worktrees, unlinks links without following them, saves uncommitted work to `D:/_merge-leftovers/<N>/`, and removes every worktree and review checkout of the PR (any lane's), the named per-PR targets and the local branch (unless a pull request is stacked on it: `gh pr list --base <branch> --state open`). Exception: a lane's single reused review worktree, named by `--keep-worktree`, is kept with its shared target; only its checkout of the PR is moved to `origin/main` (#1456).

**Delivered** means merged into `main` and `main` green on the change's main proof. An open pull
request is not delivered.
