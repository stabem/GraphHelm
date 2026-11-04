# KEEL_MERGE

Step 4. Only the reviewer who approved merges. Read [`KEEL.md`](../KEEL.md) first.

1. **Pin the head.** `gh pr view <N> --json headRefOid` must equal the sha the review named, and
   no BLOCK on that head is left unanswered. Merge exactly that commit:
   `gh pr merge <N> --squash --match-head-commit <sha>`.
2. **Main gets one full reached plan nightly.** Two changes that pass alone can break together.
   The orchestrator owns the nightly run on main, under the shared build slot and outside declared
   benchmark windows. Do not run the full plan per PR, per merge or hourly. Preserve any failed
   receipt and open a P0 linked to the responsible PR after identifying it from evidence; fix red
   main first with a revert or fix-forward PR. Scheduling and observed execution are separate
   operational work: this rule does not install a job or change an existing runner's cadence.
3. **Closing keywords** in the body and the commits must match what the change closes. Never
   write a closing keyword (`close`, `fix`, `resolve` and their forms) next to an issue you do not
   mean to close, not even inside a negation; write `Refs #N` instead.
4. **Check after.** Read what landed, then run the change's main proof on main and confirm it is
   green. If it is red, the merge is not delivered: fix main first.
5. **Clean up the PR.** After the merge lands the merger runs `graphhelm workspace sweep --apply --merged-branch <branch> --repo <repo> --pr <N> --head <sha> [--target <dir>]... [--keep-worktree <dir>]`: it stops the processes inside the PR's worktrees, unlinks links without following them, saves uncommitted work to `D:/_merge-leftovers/<N>/`, and removes every worktree and review checkout of the PR (any lane's), the named per-PR targets and the local branch (unless a pull request is stacked on it: `gh pr list --base <branch> --state open`). Exception: a lane's single reused review worktree, named by `--keep-worktree`, is kept with its shared target; only its checkout of the PR is moved to `origin/main` (#1456). Then it runs `graphhelm workspace sweep --all --apply --repo <repo>` (#1578), which removes whatever any earlier PR left behind on the machine: every worktree, local branch and build cache of a merged or closed PR (or clean, pushed and idle for 24 hours), across every slot and target root and `.claude/worktrees`, followed by `git worktree prune`. It saves dirty work to `D:/_merge-leftovers/<worktree>/` first, skips and reports a locked worktree, never touches a directory a live process holds or unmerged unpushed work, and never follows a link. Run it without `--apply` first to read what it would remove. To delete any other single worktree or directory, run `graphhelm workspace remove <dir>` (#1663), which never follows a link; never `rm -rf`, `Remove-Item -Recurse` or `git worktree remove --force` a tree that may hold a junction, because they follow `apps/studio/node_modules` into the owner checkout.

**Delivered** means merged into `main` and `main` green on the change's main proof. An open pull
request is not delivered.
