# Lanes: many agent sessions on one machine

A **lane** is one agent session working this repository beside others on the same machine, under
one GitHub account. This page is the lane's standing rules. How a change gets from an issue to
`main` is [DELIVERY.md](DELIVERY.md); this page only adds what sharing one machine requires.

The paths below are this team's layout: the lane root is `D:\gh`, the owner's checkout (the one the
Runtime serves) is `F:\github\GraphHelm`, and the coordinator session is `gh-claude-orquestrador`.
`<lane>` is your session name, exactly as `ListAgents` prints it (for example `gh-claude-11`).

## 1. Who decides

- Your session name is your address. Rename the chat to exactly `<lane>`.
- The coordinator gives orders and go-aheads. Report to it with `SendMessage`; never wait for the
  owner.
- On start, send `<lane> ready` plus one line on what you read first, then wait for an order.
- Read [`AGENTS.md`](../../AGENTS.md) and [DELIVERY.md](DELIVERY.md) before any work.
- **Only the coordinator assigns a reviewer**, one per PR. Do not pick one and do not review a PR
  you were not assigned; when none was named, ask the coordinator. The assigned reviewer merges
  (pinned squash, DELIVERY.md §5).
- A lane holds at most **one implementation task, one review in progress and one waiting slot
  ticket** at a time.
- Before you merge as reviewer, check the PR against **today's main**. When its base is not the
  current `origin/main` and what landed since touches the same files or symbols
  (`git log <base>..origin/main --stat`), build and run the reached tests on the PR merged with
  main, or ask the author to rebase. "Mergeable" only means no textual conflict: a changed
  signature and a new caller of the old one merge cleanly and do not compile (#520 + #533 → #551).
- As reviewer, read every comment on the PR first. When a review of record already exists for that
  head, stop and tell the coordinator instead of adding a second one.

## 2. Where a lane writes

- Everything of yours lives under `D:\gh\<lane>\`: worktrees, scripts, logs, temp files.
- Never write or edit a file in the owner's checkout (`F:\github\GraphHelm`), `.claude/launch.json`
  included, and never in the root of `F:\`. Read from it, and run its tools, freely.
- Start a dev server from your own folder and open it with `preview_start {url}`, not by adding an
  entry to the owner's `.claude/launch.json`.
- No bare `git stash`: the stash is shared by every worktree of the repository, so another lane can
  pop yours. Commit to your branch instead.
- One new worktree per task, cut from `origin/main`. Use `workspace claim` so the Runtime
  can manage its lifetime, then `workspace release` when the task is done:

      graphhelm workspace claim --root D:/gh --lane <lane> --task <issue> --repo F:/github/GraphHelm --base origin/main --branch issue-<N>-<slug>
      graphhelm workspace release --root D:/gh --lane <lane> --task <issue>

  The worktree is `<root>/<lane>/<task>/wt`. Hand-created worktrees (`git worktree add`)
  are outside the claim ledger and are never reclaimed as workspaces; their recorded slot
  targets can still qualify for the target-only rules below.
- `graphhelm serve --workspace-root <root>` runs the existing `workspace sweep --apply`
  automatically every 1800 seconds. `--workspace-sweep-seconds 0` disables it; nonzero
  values must be 60..=86400. It sleeps before each attempt and after completion; this period
  does not bound the duration of Git or filesystem operations. No cleanup rules change.
  CLI sweeps, HTTP sweeps, periodic sweeps and held-slot reclaim share
  `<root>/.graphhelm-workspaces/sweep.lock`; contention refuses that attempt without waiting.
  The lock covers the eligibility checks and delete loop, never the timer sleep or a build.
  A failed tick is logged and retried next period, without stopping the Runtime.
  `GET /v1/workspaces` adds `lastSweep` (`null` before the first tick), with Unix-seconds
  `at`, `removed`, `kept`, target results and `ok`/`diagnostics`. Each tick logs this same
  outcome on one stderr line; it is in memory and resets when the Runtime restarts.

- A worktree's `node_modules` (for example `apps\studio\node_modules`) can be a junction into the
  owner's copy. `Remove-Item -Recurse`, `rm -rf` **and `git worktree remove --force`** all follow a
  junction and delete the owner's files through it. Remove each junction first (`rmdir` removes the
  link only), check it is gone, then remove the worktree, without `--force`:

      cmd /c rmdir D:\gh\<lane>\wt-<issue>\apps\studio\node_modules
      cmd /c if exist D:\gh\<lane>\wt-<issue>\apps\studio\node_modules echo STILL THERE
      git -C F:/github/GraphHelm worktree remove D:/gh/<lane>/wt-<issue>

  When git still refuses ("contains modified or untracked files"), find what is left and deal with
  it by name. Never answer that refusal with `--force` while a junction is inside.
- Remove only what you created, by name. Never touch another lane's worktree, branch or ticket, and
  never run `git worktree prune`.

## 3. Builds: one slot for the whole machine

- Every `cargo` build or test runs under the build slot, one at a time in arrival order:

      graphhelm workspace slot --root D:\gh --lane <lane> --label <what> -- "C:\Program Files\Git\bin\bash.exe" D:\gh\<lane>\<script>.sh

- The script exports the worktree's own target and never cleans it:

      export CARGO_TARGET_DIR="<your worktree>/target"

  Never use or export `D:\gh\target-shared` (`--shared-target`): cargo can treat another worktree's
  artifacts as fresh, so the run vouches for bytes it did not build (#361). No `cargo clean`.
- The script also exports `GRAPHHELM_TEST_TIME_SCALE=3`: it multiplies the hang-catcher ceilings of
  the CLI tests that wait on a child process (`apps/cli/tests/support/time_scale.rs`, #549), which
  otherwise go red on a machine other lanes are building on. It scales test waits only, never a
  product budget; a whole number from 1 to 20, anything else is refused.
- **Where the owner set a build-directory rule, the slot picks the directory and the script does
  not export one (#360).** The rule is `<root>\.graphhelm-workspaces\slot-targets.json`:

      {"targetRoot": "C:/gh", "cap": 3, "minFreeGb": 20}

  With it the slot runs the command with
  `CARGO_TARGET_DIR=<targetRoot>\<lane>\<worktree directory name>\target`, records that
  directory, and holds each lane to `cap` of them (default 3): a run that would start one more is
  refused before it queues, naming the ones the lane holds. A build directory whose worktree no
  longer exists is reclaimed by that lane's next slot run and by `graphhelm workspace sweep
  --apply`. The sweep also reclaims only the target of an existing worktree when it is clean,
  idle and merged by content into its local `refs/remotes/origin/main`; its worktree and branch
  remain intact. Git 2.38 or newer must report that `git merge-tree --write-tree
  refs/remotes/origin/main HEAD` produces exactly `origin/main`'s tree, with no error or stderr.
  The sweep never fetches. Squash merges qualify; a later main edit that conflicts keeps the
  target. Missing refs, unreadable paths and ambiguous results keep it with a reason.
  Only the target's own path is excluded from the clean-tree check. Every target entry must be
  older than 30 minutes; links are refused. The sweep holds the build slot while checking and
  reclaiming, and refuses a target named by a waiter (older tickets without a worktree protect
  the entire lane). A held slot keeps targets as `merged_busy`.
  Without `--apply` the same checks only list candidates; eligible entries carry `reason: merged`.
  Other keep reasons include `merged_dirty`, `merged_recent`, `not_merged` and `merge_check_failed`.
  `graphhelm workspace slot status` shows how many targets each lane holds and the worktree of
  new holders and waiters. An
  `export CARGO_TARGET_DIR=...` inside the script overrides the slot and puts the build back where
  the rule says not to: leave it out. The worktree's directory name must be a workspace id
  (lowercase letters, digits, `.`, `_`, `-`).
  `minFreeGb` is an integer from 0 to 4096, defaults to 20 when absent, and 0 disables the
  free-space floor. With the floor enabled the target root's volume must report its free space.
  A run below the floor refuses before queueing when no other lane has target records to
  inspect. Otherwise it queues, then, while holding the slot and still below the floor, applies
  the target-only sweep to other lanes and rechecks free space once. It never reclaims this
  lane's targets or the worktree it is about to build in, even if another lane recorded it.
  When space remains below the floor it refuses and names the free GB, the floor and
  `graphhelm workspace sweep`; no child runs. GB here means 1024 cubed bytes. Reclaim runs only
  while holding the slot; a pre-queue refusal does not reclaim anything. Free space is a
  sample, not a reservation, so another writer can consume space after the check. Slot status
  adds `targetSpace: {targetRoot, freeGb, minFreeGb}` alongside the existing lane counts;
  unreadable free space is `null`, never a claim that enough space remains. No-rule and
  `--shared-target` runs keep their existing behavior.
- Write Git Bash's full path in the slot command, as above. A bare `bash` or `sh` resolves to WSL.
- A `cargo` run allowed outside the slot (`check`, `clippy`) uses
  `CARGO_TARGET_DIR=D:\gh\<lane>\target-check`.
- Benchmarks, load generators and store seeding run inside the slot too: they load the machine like
  a build does.
- A fix the owner is waiting to see may run outside the slot with `CARGO_BUILD_JOBS=6`, and only
  when at least 8 GB of RAM is free.
- After the machine reboots, every background run is dead. Queue it again; do not wait for it.
- **One waiting ticket per lane.** Put everything the diff needs in one script instead of queueing
  several.
- Run only what the diff reaches (DELIVERY.md §3):

      python tools/reached-tests/reached_tests.py

- Studio-only and docs-only work never uses the slot: `vitest` and `tsc` run directly.
- **Never end a turn idle because a build waits.** Write the next code, write the PR body, or read
  the PR you were assigned to review.

## 4. Task records

The owner watches the Studio Team tab, which is drawn from these records (DELIVERY.md, "Task
records"). From a lane worktree the token path must be absolute:

    T="python F:/github/GraphHelm/tools/task-record/task_record.py --repo stabem/GraphHelm --token-file F:/github/GraphHelm/.graphhelm/events.agent.token --lane <lane>"

| When | Command |
|---|---|
| Start a task | `$T --issue N claimed --branch B --journeys <contractId>` |
| Open a PR, and again after **every** push | `$T --issue N pr_opened --pr P --head SHA --reviewer <reviewer> --journeys <contractId>` |
| As reviewer, after posting the verdict | `$T --issue N review_verdict --pr P --head SHA --verdict APPROVE\|APPROVE-WITH-RISK\|BLOCK --comment-url URL` |
| As merger, only after your own `gh pr merge` exits 0 | `$T --issue N merged --pr P --merge-sha SHA --closes <issues the squash closed>` |

Name the journey the issue serves; the Studio Graph tab links the task to it. Ids are the stems of .graphhelm/journeys/*.journey.yaml.

`--closes` is recorded exactly as given: name every issue the squash closed, and pass none for a
`Refs` PR. Every lane merges under one GitHub account, so `mergedBy` cannot tell whose merge it
was; the exit code of the `gh pr merge` you ran is the evidence.

`pr_opened --reviewer` is meant to record the reviewer in the same call. Until the change that does
so lands, also run `$T --issue N review_assigned --pr P --head SHA --reviewer <reviewer>`.

Record only as yourself. Recording as another lane is a protocol violation the Runtime cannot stop.

## 5. Naming and messages

- Issue title `<Area>: <what changes for the user>`, at most 50 characters; PR title
  `type(area): <what changes for the user>`, at most 60. Both bodies start with
  `Summary: <one sentence, at most 100 characters>`. Areas and examples: DELIVERY.md §1.
- Identity line on every PR comment, review and commit body: `Session: <lane> · Head: <sha8>`.
- A message to another session: the first line is one plain sentence that stands alone; then short
  bullets; shas and paths in backticks.
- **Every verdict goes to two places:** the comment on the PR, and a one-line `SendMessage` to the
  author. A verdict the author never hears about is not delivered.

## 6. Never

- GitHub Actions: do not enable, run or rely on a workflow.
- Approving a journey. Only the owner approves journeys.
- Touching another lane's worktree or ticket.
- Recording a task step as another lane.
