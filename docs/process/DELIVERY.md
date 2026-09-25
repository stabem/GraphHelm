# Delivery process: GraphHelm + Keel

This is the only process for changing this repository. It replaces the old `.factory/` protocol
(two passes, a third-lane presser, the merge checklist, the lane loop) and the `superpowers`
plans. How code is written is [Keel](../keel/KEEL_SPEC.md); this page says how a change gets from
an issue to `main`.

This public repository starts with one import commit. Issue and pull-request numbers cited as
historical examples below belong to the private development archive. New work uses issues in this
repository; see [source provenance](../open-source/SOURCE_PROVENANCE.md).

## 1. Issue

Every change starts from a GitHub issue. The branch is `issue-<N>-<short-kebab-description>`.
New community issues receive `needs-triage`. A maintainer assigns exactly one workflow label:
`current-wave`, `in-flight`, `tech-debt`, or `product-vision`, removing `needs-triage` when the
issue is classified. Maintainer-created issues may start with their workflow label.

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

## 4. One review, by another session or a blind subagent

- One review from a reviewer that did not write the change: another session, or a **blind
  subagent** (owner order, 2026-09-24). A blind subagent is spawned for the review only. Its brief
  may carry: the PR number, the identity line to sign with, where the rules live (`AGENTS.md`,
  this document), the build directory, and questions that apply to every review (correctness,
  security, fail-closed behaviour, test quality). It must not carry the author's reasoning, a
  finding to look for, a summary of the change, or an expected verdict. A subagent that helped
  write the change, or was given any of those, is not blind and cannot review it.
- The review's first line names the reviewer and the head sha it read:
  `Session: <ListAgents name [ref]> · Head: <sha8>`, with `(blind subagent)` after the session
  name when a subagent reviews.
- **A BLOCK stands for the head it names.** The author answers it by pushing a fix (a new head) or
  by replying on the PR with why it does not hold; only then may a new reviewer be asked. Asking
  another reviewer about the same head until one approves is not a review.
- **Every review reads the PR's earlier comments first.** When a BLOCK is open on the head under
  review, the review names that BLOCK and says whether it still holds and why; an APPROVE that does
  not answer an open BLOCK on the same head is not a pass, and the merge (§5) waits for that answer.
  Reading the PR's own comments is part of the review, not a finding handed over in a brief.
- The reviewer **runs the tests the change reaches** on that head and names each command and its
  result in the review. A review that does not run them is not a review.
- **A change to Rust code also runs the lints on the crates it touches**, author and reviewer
  alike: `cargo +1.97.1 fmt --all -- --check` and
  `cargo +1.97.1 clippy --locked -p <crate> --all-targets --all-features -- -D warnings` for each
  touched crate. Tests alone do not catch a lint: #1269 merged with passing tests and left `main`
  red under clippy until #1274. When `Cargo.toml`, `Cargo.lock` or a crate many others depend on
  changes, run the workspace form (`--workspace` instead of `-p`).
- **A change to any Rust file also runs the workspace-wide source guards**, because they read every
  crate's text and so are reached by any edit: `cargo +1.97.1 test --locked -p graphhelm-protocols
  --test authored_strings_across_the_workspace` (about one second). #1281 added a string literal in
  a test file, passed its crate's tests, and left `main` red on every platform until #1311.
- Run tests from the **committed** head, not a dirty tree: a check that compares the branch with
  `main` (the freeze rule) sees nothing before the commit exists.
- The verdict is a word in the comment text: `APPROVE`, `APPROVE-WITH-RISK` (name the risk) or
  `BLOCK` (name the defect). GitHub review state stays `COMMENTED`, because every session shares
  one account.
- If the head moves after the review, the review covers only the sha it named. A prose-only edit of
  the PR body does not move the head and does not void the review.

## 5. Merge, by the reviewer

The reviewer who approved merges (a blind subagent included). There is no gate and no separate presser. `ci/gate.ps1` still
exists as an optional full local check (it takes about 20 minutes); a merge never waits on it.

1. Pin the head: `gh pr view <N> --json headRefOid` must equal the sha the review named, and no
   BLOCK on that head is left unanswered by the review (§4).
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
  `.claude/worktrees/`). Put scratch and cargo targets in a task-specific directory outside the
  repository. Follow any stricter local disk policy supplied by the owner.

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

The old process kept design notes under `.factory/`, `.superpowers/` and `docs/superpowers/plans/`.
They were removed on 2026-09-24, and so were the gate runner, the gate queue, `merge-proof` and the
committed gate receipts (`.factory/gate-runs/`). Code comments and documents now cite those notes by
the short names below. Read one at the last commit that had it: `git show c080739b:<path>`.

| Name used in comments | Path at `c080739b` |
|---|---|
| gateway-slice plan | `docs/superpowers/plans/2026-08-14-gateway-slice.md` |
| architect-judgments plan | `docs/superpowers/plans/2026-09-16-architect-judgments.md` |
| real-executor plan | `docs/superpowers/plans/2026-08-14-real-executor.md` |
| chat-surface plan | `docs/superpowers/plans/2026-08-14-chat-surface.md` |
| public-runtime-api plan | `docs/superpowers/plans/2026-08-13-public-runtime-api.md` |
| gate-under-five-minutes plan | `docs/superpowers/plans/2026-09-19-gate-under-five-minutes.md` |
| program plan index | `docs/superpowers/plans/2026-08-08-graphhelm-program-plan-index.md` |
| foundation-graph-kernel plan | `docs/superpowers/plans/2026-08-08-graphhelm-foundation-graph-kernel.md` |
| setup-backup-restore plan | `docs/superpowers/plans/2026-09-21-setup-backup-restore.md` |
| journey-proof-pilot plan | `docs/superpowers/plans/2026-09-21-journey-proof-pilot.md` |
| structured-agent-context plan | `docs/superpowers/plans/2026-09-21-structured-agent-context.md` |
| economic-route-selection plan | `docs/superpowers/plans/2026-09-21-economic-route-selection.md` |
| event-evidence-store SDD record | `.superpowers/sdd/2026-08-09-production-event-evidence-store/` |
| merge checklist | `.factory/MERGE-CHECKLIST.md` |
| lane loop | `.factory/lane-loop.md` |
| design note #107 | `.factory/b-agent-107-blueprint.md` |
| design note #119 | `.factory/c-agent-119-design.md` |
| design note #159 | `.factory/b-agent-159-blueprint.md` |
| design note #200 | `.factory/e-agent-200-design.md` |
| design note #211 | `.factory/h-agent-211-surface-blueprint.md` |
| design note #213 | `.factory/e-agent-213-blueprint.md` |
| design note #219 | `.factory/h-agent-219-blueprint.md` |
| design note #221 | `.factory/e-agent-221-refusal-code-gap.md` |
| scope amendment 5 (#222) | `.factory/b-agent-222-scope-amendment-5.md` |
| design note #285 | `.factory/e-agent-285-blueprint.md` |
| mode-semantics note (#89) | `.factory/d-agent-mode-semantics.md` |

Two cited notes were never committed to `main`, so no commit holds them: the owner-output
blueprint (`.factory/e-agent-221-blueprint.md`) and the #226 sabotage expectation record
(`.factory/n-agent-226-sabotage-expectations.md`). The findings drawn from them are in the
issues and pull requests those comments name.

The subsystem specifications that lived under `docs/superpowers/specs/` moved to
[`docs/specs/`](../specs/); the Keel specification moved to
[`docs/keel/KEEL_SPEC.md`](../keel/KEEL_SPEC.md). Historical records (ADRs, the decision register,
milestone and acceptance notes, the changelog) keep their original paths as written.
