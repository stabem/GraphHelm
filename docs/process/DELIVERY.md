# Delivery process: GraphHelm + Keel

This is the only process for changing this repository. It replaces the old `.factory/` protocol
(two passes, a third-lane presser, the merge checklist, the lane loop) and the `superpowers`
plans. How code is written is [Keel](../keel/KEEL_SPEC.md); this page says how a change gets from
an issue to `main`.

## 1. Issue

Every change starts from a GitHub issue. The branch is `issue-<N>-<short-kebab-description>`.
New issues carry exactly one label: `current-wave`, `in-flight`, `tech-debt` or `product-vision`.

## 2. Keel card

Keel is proportional. Use only as much of it as the change needs.

| The change | What goes in the PR body |
|---|---|
| Docs, comments, config values, a one-line fix, a test-only fix | Nothing beyond the summary. |
| A bounded code change | A three-line card: the paths in scope, the promise, the command that proves it. |
| New public surface: a module, type, public function, dependency or test file | The full card, and the new surface named. |
| Persistence, permissions, compatibility, security, external effects | The full card and the JPD flow (`AGENTS.md`, Journey-Proven Development). |

A card lists paths, not globs. A new dependency is always named. Risk is read from what the change
touches, not from how big it is: a one-line change can remove a permission check.

## 3. Work

Follow the four Keel moves: start from the promise, start from the card and search on purpose, add
only the surface the promise needs, prove with the smallest adequate observer. Before opening the
PR, the author runs the tests the change can reach and lists them, with their result, in the PR
body.

## 4. One review, by another session

- One review from a session that did not write the change. The reviewer's session name and the
  head sha it read go in the first line: `Session: <ListAgents name [ref]> · Head: <sha8>`.
- The reviewer **runs the tests the change reaches** on that head and names each command and its
  result in the review. A review that does not run them is not a review.
- The verdict is a word in the comment text: `APPROVE`, `APPROVE-WITH-RISK` (name the risk) or
  `BLOCK` (name the defect). GitHub review state stays `COMMENTED`, because every session shares
  one account.
- If the head moves after the review, the review covers only the sha it named. A prose-only edit of
  the PR body does not move the head and does not void the review.

## 5. Merge, by the reviewer

The reviewer who approved merges. There is no gate and no separate presser.

1. Pin the head: `gh pr view <N> --json headRefOid` must equal the sha the review named.
2. Closing check: `ci/closing-keywords.ps1 -Number <N> -Repository stabem/GraphHelm -Closes <issues>`.
   It reads the body and the commit messages; their union must equal the intent. Never write a
   closing keyword (`close`, `fix`, `resolve` and their forms) next to an issue you do not mean to
   close, not even inside a negation. Write `Refs #N` instead.
3. `gh pr merge <N> --squash --match-head-commit <sha>`. Delete the branch unless another PR is
   stacked on it (`gh pr list --base <branch> --state open`).
4. Read what landed: `git log -1 --format=%B origin/main` and `gh issue view` for every closed
   issue. Corrections go in a follow-up comment on the same PR.

A change is delivered when it is merged into `main`, not when the PR is open.

## 6. Housekeeping

- A session removes only what it created, by name: its worktree, its branch, its scratch
  directory. Never sweep other sessions' worktrees or branches, and never run `git worktree prune`
  by hand.
- Worktrees live under `<repo>/.worktrees/<branch>` (or the tool's own root, such as
  `.claude/worktrees/`). Scratch and cargo targets go under `<drive>:/_agent-scratch/graphhelm/<lane>/`,
  never on `F:`, and on `C:` only while at least 100 GB stay free.

## 7. Traps that produced false readings

- A failed `git fetch` leaves the ref at its old value and the next `rev-parse` succeeds. Check the
  fetch's exit code.
- Bash: `$?` after a pipe belongs to the last command. Never pipe a run whose exit code matters;
  redirect to a file and read the file separately.
- MSYS mangles some `<ref>:<path>` shapes; use `MSYS_NO_PATHCONV=1` or the blob sha.
- `git ls-tree` without `--full-tree` is scoped to the current directory and returns empty with
  exit code 0.
- A zero produced by your own filter measures the filter. Put a known positive in the same command.

## History

Older code comments, ADRs and milestone notes cite design notes that lived under `.factory/`,
`.superpowers/` and `docs/superpowers/plans/`. Those files were removed on 2026-09-24; read them
at the last commit that had them: `git show c080739b:<path>`. The subsystem specifications that
lived under `docs/superpowers/specs/` moved to [`docs/specs/`](../specs/); the Keel specification
moved to [`docs/keel/KEEL_SPEC.md`](../keel/KEEL_SPEC.md).
