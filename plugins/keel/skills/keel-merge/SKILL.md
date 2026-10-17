---
name: keel-merge
description: "Use when merging a change you approved under Keel: what to recheck at the merge and what to record after it."
---

How to use this skill:

- First run: if the project has no `KEEL.md` at its root, install Keel into it with
  `python <this plugin>/setup/keel_setup.py <project root>` (the plugin folder is two levels above
  this skill's folder). It copies `KEEL.md` and the `keel/` step files into the project, offline,
  never overwrites an existing one, and adds a marked pointer line to `CLAUDE.md` and `AGENTS.md`.
- Read the project's own `KEEL.md` at its root when one exists, and the `keel/` step files it names.
  Before that step has run, read https://github.com/stabem/GraphHelm/blob/main/KEEL.md and the step
  files under https://github.com/stabem/GraphHelm/blob/main/keel/. The text below is the bundled copy.
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

Step 4. Only the reviewer who approved merges. Read [`KEEL.md`](https://github.com/stabem/GraphHelm/blob/main/KEEL.md) first.

1. **Pin the head.** `gh pr view <N> --json headRefOid` must equal the sha the review named, and
   no BLOCK on that head is left unanswered. Merge exactly that commit:
   `gh pr merge <N> --squash --match-head-commit <sha>`.
2. **Main gets one whole run after merges.** Two changes that each pass alone can break
   together, so after merges land the orchestrator owns one whole-package run on main, at most
   once an hour while main moves. A red main is fixed first, by a revert or a fix-forward PR,
   before any new merge. A hidden scheduled task runs it unattended:
   `python tools/reached-tests/main_whole_run.py` (skips unless main moved and an hour passed;
   records a `main.whole_run` signal, severity `high` with the red steps named when it failed).
3. **Closing keywords** in the body and the commits must match what the change closes. Never
   write a closing keyword (`close`, `fix`, `resolve` and their forms) next to an issue you do not
   mean to close, not even inside a negation; write `Refs #N` instead.
4. **Check after.** Read what landed, then run the change's main proof on main and confirm it is
   green. If it is red, the merge is not delivered: fix main first.
5. **Clean up the PR.** After the merge lands the merger runs `graphhelm workspace sweep --apply --merged-branch <branch> --repo <repo> --pr <N> --head <sha> [--target <dir>]... [--keep-worktree <dir>]`: it stops the processes inside the PR's worktrees, unlinks links without following them, saves uncommitted work to `D:/_merge-leftovers/<N>/`, and removes every worktree and review checkout of the PR (any lane's), the named per-PR targets and the local branch (unless a pull request is stacked on it: `gh pr list --base <branch> --state open`). Exception: a lane's single reused review worktree, named by `--keep-worktree`, is kept with its shared target; only its checkout of the PR is moved to `origin/main` (#1456).

**Delivered** means merged into `main` and `main` green on the change's main proof. An open pull
request is not delivered.
