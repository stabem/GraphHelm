---
name: merge
description: "Merge an approved PR at the exact head the review read, check its closing keywords, read back what landed, and release the workspace. Use only as the PR's approving reviewer, right after the approval."
---

# Merge

Step 5 of the per-task graph (`docs/process/DELIVERY.md` §5). Only the approving reviewer merges.

## Emits and reads

- Reads: the approval comment (its head sha), the PR body and commit messages.
- Emits: the squash merge, and the record `task.merged` (`pr`, `mergeSha`, `closes`, `merger`)
  after reading what landed. Record it as a signal whose `type` is the kind and whose `description` is one
  `graphhelm-task-event-v1` document, with your own actor (`GRAPHHELM_ACTOR=<lane>`): the
  Runtime refuses a lane that is not the recorder (`GHCLI038_ACTOR_MISMATCH`). Fields and
  doors: `docs/process/DELIVERY.md` "Task records".

## Method

1. Pin the head: `gh pr view <N> --json headRefOid` must equal the sha the approval named, and no
   BLOCK on that head is left unanswered. If the head moved, the approval does not cover it.
2. Closing check: the union of closing keywords in the body and commits must equal the intent
   (`ci/closing-keywords.ps1 -Number <N> -Repository <owner/repo> -Closes <issues>` in GraphHelm).
   Use `Refs #N` for an issue that must stay open.
3. `gh pr merge <N> --squash --match-head-commit <sha>`. Delete the branch unless another PR is
   stacked on it (`gh pr list --base <branch> --state open`).
4. Read what landed: `git log -1 --format=%B origin/main` and `gh issue view` for every closed
   issue. Corrections go in a follow-up comment on the same PR.
5. Release the workspace the task used: `graphhelm workspace release --root <root> --lane <lane>
   --task <n>`. It records the head and deletes nothing; never sweep other sessions' worktrees.

## Completion

Delivered means merged into `main` and read back, not an open PR.
