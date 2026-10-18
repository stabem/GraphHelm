# Delivery process: GraphHelm + Keel

This is the only process for changing this repository. How code is written is
[Keel](../../KEEL.md); this page says how a change gets from an issue to `main`.

## 1. Issue

Every change starts from a GitHub issue. The branch is `issue-<N>-<short-kebab-description>`.
New community issues receive `needs-triage`. A maintainer assigns exactly one workflow label:
`current-wave`, `in-flight`, `tech-debt`, or `product-vision`, removing `needs-triage` when the
issue is classified. Maintainer-created issues may start with their workflow label.

### Naming: issue and PR titles the owner can read

The Team tab shows each task as its issue number, its title and one summary line, and each PR
by its title. They are read at a glance, so they follow one standard:

- **Issue title:** `<Area>: <what changes for the user>`, plain words, at most 50 characters, no
  file names, codes or issue numbers. Areas: Studio, Journeys, Keel, Runtime, Build, Skills, Docs.
  Example: `Studio: Team tab shows task titles`.
- **PR title:** `type(area): <what changes for the user>`, at most 60 characters, the same plain
  words (the squash commit takes this title, so the conventional type stays). Example:
  `feat(studio): team tab shows task titles`. The Studio shows it without the `type(area):` prefix.
- **First line of every issue and PR body:** `Summary: <one sentence, at most 100 characters, who
  gets what>`. Technical detail goes below it, and the identity line follows it.

`task_record.py` reads the title and the `Summary:` line with `gh` on `claimed` (the issue) and
`pr_opened` (the PR); `--title` / `--summary` override, `--no-github` skips. Over the standard's
length it warns and still records; the Studio clips long text and shows all of it on hover.

## 2. Keel card

How much card a change needs, and what goes in it, is [keel/KEEL_PLAN.md](../../keel/KEEL_PLAN.md).


## 3. Work

Follow the four Keel moves: start from the promise, start from the card and search on purpose, add
only the surface the promise needs, prove with the smallest adequate observer. Before opening the
PR, the author runs the tests the change can reach and lists them, with their result, in the PR
body.

**Retire the path you replace** (the rule is `keel/KEEL_EXEC.md`; how to check it here:). When a change removes, replaces or redirects
a production caller, check the displaced declarations and their exclusive helpers/types, styles,
dependencies and tests before opening the PR. A rename alone does not make its target dead. Search
from the actual entry points and check dynamic registrations, public consumers, manual commands,
other platforms and persisted/wire contracts before calling a candidate unused. Test-only calls
and a search with zero hits are leads, not deletion proof. Read source when an index is stale or
partial and report any remaining coverage gap.

When journey execution evidence is useful, [`keel test validation`](../keel/KEEL_TEST_VALIDATION.md)
can collect it in isolated fixtures. It is optional, not another gate; missing coverage is unknown.

Add a short `Retirement:` entry to the PR body (not a new card/schema field), only for changes that
trigger this rule. Name the inspected paths/symbols and the evidence at the committed head, then
record each candidate as **removed**, **retained** (name its live consumer or contract), or
**unobserved** (name the coverage gap, risk and follow-up). If nothing became orphaned, say what
was checked; an unexplained "none" is not evidence. Remove confirmed dead candidates in the same
change, widening the card to their exact paths when needed; unrelated pre-existing debt gets a
separate task. Do not keep speculative scaffolding under a generic future-use excuse. A documented
compatibility or migration obligation is a reason to retain it, with its retirement condition.

Test removal still requires the existing `test-audit` deletion record and covering observer. Keep
checks proportional: run only reached checks, do not add a test to keep a dead export alive, and
do not add a full build, whole-repository scan or a new tool dependency just for this record. For
example, replacing an old team reader names that reader and its remaining calls, removes its
exclusive parser/styles when proven unused, and retains the wire type if the active reader still
consumes that contract. This is a human/agent review check; `keel check` has no dead-code detector.

**Which tests a change reaches** is computed, not guessed:
`python tools/reached-tests/reached_tests.py [--base origin/main] [--head HEAD]` reads the diff and
the workspace graph (`cargo metadata --no-deps`, no build) and prints the commands. Its rules:
- a changed integration-test file reaches only its own target, or its module in a test bundle;
- filtered `graphhelm-cli` unit and `cli` integration selections may share one Cargo command only
  for the exact `--bin graphhelm` plus `--test cli` pair; the command carries both filter sets;
- a whole-package `graphhelm-cli` `cli` target is partitioned by registered bundle modules, with
  prior-module skips and a final `--skip` complement for inline/root tests; missing bundle metadata
  remains incomplete;
- an unfiltered single selection of that same `cli` target uses the same metadata-backed partition;
  it cannot fall back to one broad command;
- `tests/support/` or a fixture reaches its package's tests;
- a crate's source reaches that crate and every crate that depends on it, except an
  `apps/cli/src/commands/<module>` change (a directory's `mod.rs` counts as its module): it reaches
  `--bin graphhelm commands::<module>::`, the `cli` modules that name its command (a submodule no
  test names falls back to its command; `CLI_COMMAND_WORDS` in the selector lists the modules whose
  command has another name) and the audited source readers (#1608). Shared files (`main.rs`,
  `args.rs`, `commands/mod.rs`, the envelope and rendering files) and unmapped helpers still reach
  the whole package, so a gap is never silently skipped;
- the CLI help golden (`args_help_golden`) is reached by its fixture, by its source, and by any
  file that derives clap argument types; the protocols `canonical_golden` is reached by every
  `extensions/` and `schemas/` path and by any protocols source change;
- a file a crate embeds (`include_str!`) reaches that crate (a Rust source that tests embed
  reaches those test targets when each one resolves), and an extension package reaches the
  tests that read it at run time plus `graphhelm extension validate`;
- Studio files reach `npx vitest related` for those files plus `tsc`, and a `tools/<tool>/` change
  reaches that tool's own tests;
- docs reach nothing.

**One test budget for authors and reviewers:**

- Run only the checks reached by `tools/reached-tests/reached_tests.py`. Check any "not mapped"
  path by hand. If the computed set misses an obligation, name the extra check and why it is
  needed in the card before running it. A full-package or whole-suite run is allowed only when
  the card names it and explains why, including when the script prints a whole-package command.
  Lanes do not pass `--allow-whole-package` by default (#1608): the whole `graphhelm-cli` package
  runs once per main change in the scheduled main whole run (#1540), which reports a red on the
  board, so a lane runs only what its diff reaches.
- Run Studio checks (`vitest`, `tsc`, npm scripts) only when `apps/studio` or something it imports
  is reached. Run `npm ci --prefer-offline --no-audit --no-fund` in that worktree's `apps/studio`
  only when `node_modules` is missing or `package-lock.json` changed; otherwise reuse it. Use
  `vitest --maxWorkers=1`, including for related tests.
- Run journey previews only for flows whose file changed or whose screens the diff changes.
  Never preview all flows by default. A card may name additional flows, but must explain why.
- Every cargo build, test and clippy run goes through the build slot (LANES.md §3). Run clippy
  only on touched crates; name and justify any broader lint scope in the card. Rust tests use
  `-- --test-threads=2`. The fmt and workspace-source-guard checks remain reached by Rust edits.
- Docs-only changes need only the existing applicable docs guards and `git diff --check`, not
  Rust builds, Studio checks or journey previews.

Execute the plan with `python tools/reached-tests/run_reached.py` (the command, its batches, slot
admission and 900-second budget are LANES.md §3). The author runs it once on main + head and
leaves the receipt, the JSON report, as a file path in the PR body together with its summary line;
the receipt names the head sha (`headBefore`) and the base main sha (`baseSha`). A budget miss is
never green.

**Resume, summary and preflight.** Rerun the same command with the same `--output` after
any stop: a step is reused only when the previous run passed it, ended with HEAD unchanged and a
clean worktree, and its proof key still matches (commit tree, argv, cwd, tool versions and the
`CARGO*`/`RUST*`/`GRAPHHELM_*`/`NODE*`/`PYTHON*`/`PG*` environment). Anything else, including a
failure, a stale or unreadable report, reruns; `--no-resume` reruns everything. Before any step,
preflight checks free disk (10 GB with Cargo steps on the slot root and `CARGO_TARGET_DIR`, 1 GB on the checkout and report; `--min-free-gb` sets all), a relative
`CARGO_TARGET_DIR`, configured tool paths that do not exist, and the git identity; a problem stops
the run as `blocked` (`--skip-preflight` records it and goes on). Stdout is a short summary
(passed/reused, failed with log path, pending, report path); `--json` prints the full report.
The report's `measure` block records reused steps and the seconds they saved. Reached browser observers additionally require
`--include-browser` and a configured local observer toolchain; otherwise the report stays incomplete.
Broad audits and all-journey coverage remain explicit work,
not mandatory feedback for every edit.

The script prints fmt, clippy and workspace-guard commands whenever Rust is reached. Apply the
budget above to those commands, including narrowing clippy to touched crates. When `graphhelm`
is on PATH the Keel plan's class and proof are printed beside the commands.

**Without a Runtime** (a cloud or CI session with no `.graphhelm/` and no Runtime to post to), the
delivery still runs on the CLI alone: write the card in the PR body (`Promise:`, `Scope:`,
`Proof:` lines), save the body to a file, and run
`graphhelm keel check --diff origin/main..HEAD --card <body.md> [--prove-new-tests]`; paste its
verdict into the PR body. Rules that say to post records through the Runtime HTTP API do not apply
there; say so in the PR body with `OBSERVER_MISSING: Runtime records (no Runtime in this session)`
rather than skipping silently. Studio, the Stop and edit locks and the Runtime signals are not
observed in such a session, and the PR body must not claim they were.

**Journey proof for screens (a signal, not yet a gate).** A PR whose scope touches a
journey screen runs `graphhelm journey replay <id>` at the head for each journey its card names and
pastes the JSON summary (paths observed, captured signal ids). `keel check` reports
`keel.journey.card_missing_journey` and `keel.journey.replay_not_green` until that is true; both are
warnings until the first end-to-end replay on this repository has run.

**Before/after captures for screens (guidance, not a gate).** When the card's scope touches a
journey screen's `scopePaths` (`.graphhelm/journeys/<contractId>.json`), record, for each touched
screen, a `before` capture at the base and an `after` capture at the head, and link both in the PR
body:

    git checkout <base>   # then run the app
    graphhelm journey capture --events <dir> --execution <id> --keyring <dir> --key-id <id>       --contract <contractId> --step <stepId> --image before.png --pr <N> --phase before
    git checkout <head>   # then run the app
    graphhelm journey capture ... --image after.png --pr <N> --phase after

`tools/playwright-observer/playwright_observe.py --journey <contractId>` (with the same `--events`,
`--execution`, `--keyring`, `--key-id`) records the head captures and the walked transitions in one
run. Name the journeys in the card (`journeys: [contractId]`, or a `Journeys:` line in the PR body);
`graphhelm keel check` (same four flags) then warns, without blocking, about a touched screen with
no fresh capture at the head. A PR without captures is reviewed and merged like any other, and the
review says what was not observed.

### The agent verifies; the user never does

Done means an observer proved the user-visible promise, run by the agent. A reply that asks the
user to verify ("try it in the app", "please test", "vai lá e testa", "me avisa se funcionou")
is not a delivery: run the journey observer yourself and report what it saw. When the change is
deployed, replay the journey against the deployed site after the deploy, not only at the head.
With no observer that can reach it, follow `OBSERVER_MISSING` in [`keel/KEEL_EXEC.md`](../../keel/KEEL_EXEC.md). Only a product
decision (approve a design, choose an option) goes to the user. The plugin's Stop hook
`plugins/graphhelm/hooks/handoff_stop_hook.py` refuses such a reply once.

## 4. One review, by another session or a blind subagent

- One review from a reviewer that did not write the change: another session, or a **blind
  subagent**, spawned for the review only. What its brief may carry, and who is not blind, is
  [`keel/KEEL_REVIEW.md`](../../keel/KEEL_REVIEW.md).
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
- **One review for every kind of change**, including security,
  persistence, concurrency and destructive changes, until the owner raises it. A change that touches
  a `keel.yaml` invariant class still needs the isolated tests `keel/KEEL_EXEC.md` asks for, with their failure list; the review checks they are there.
  For a user-visible change the reviewer also runs `graphhelm journey replay` for each journey the
  card names, on the head, and pastes its summary.
- The reviewer **reuses the author's receipt** (§3) when its head equals the PR head and nothing in
  it is failed or pending, and then runs the end-to-end proof of the promise, any cell the reviewer
  doubts, and fmt and clippy on the touched crates. The reviewer reruns the whole reached plan
  only when the receipt is missing, stale, red or pending, or when the diff touches persistence,
  permissions, security, compatibility or the plan selector itself. The review comment names what
  was reused and what was rerun, each command and its result. The reached set is what
  `tools/reached-tests/reached_tests.py --base <merge base> --head <head>` prints, subject to the
  same test budget as the author (§3). Any extra check needs its scope and reason in the card;
  review is not a reason to run unrelated Studio checks, all journey previews or a full suite.
- **The reviewer runs the Keel check on the head under review and pastes its output** in the
  review: `graphhelm --json keel check --diff <base>..<head> --card <card.json>`, where `<base>` is
  the merge base with `main` and the card is the one in the PR body saved as JSON
  ([`docs/keel/KEEL_CHECK.md`](../keel/KEEL_CHECK.md) has the shape). Exit 2 with
  `keel.scope.path_outside_card` means the change touches a path its card does not list: the
  author widens the card in the PR body (and says why) or removes the change. The surface counts
  and warnings are signals the reviewer weighs, not a verdict. A change whose row in §2 needs no
  card is checked without `--card`, which reports the counts only.
- **A change to Rust code also runs the lints on the crates it touches**, author and reviewer
  alike: `cargo +1.97.1 fmt --all -- --check` and
  `cargo +1.97.1 clippy --locked -p <crate> --all-targets --all-features -- -D warnings` for each
  touched crate. Tests alone do not catch a lint: a change once merged with passing tests and left `main`
  red under clippy. Clippy runs through the build slot, on touched crates only.
  If a manifest, lockfile or shared-crate change needs broader lint coverage, the card names
  that scope and explains why before a workspace run (`--workspace` instead of `-p`).
- **A change to any Rust file also runs the workspace-wide source guards**, because they read every
  crate's text and so are reached by any edit: `cargo +1.97.1 test --locked -p graphhelm-protocols
  --test authored_strings_across_the_workspace` (about one second): a string literal added in a test file once
  passed its crate's tests and left `main` red on every platform.
- **Reuse preparation, not verdicts.** The author may hand the unchanged worktree and its warm
  isolated target to the reviewer for sequential use. The author stops all writes and Cargo
  commands there for the review. The reviewer verifies the remote, exact committed head, clean
  tree, toolchain/profile/features and target path, then runs the reached commands independently
  through the slot. Check the head and clean tree again afterward. If any identity changes,
  the result does not cover the requested head. Do not point a second worktree at that target,
  copy Cargo fingerprints, or execute binaries from an unverified artifact directory. A separate
  reviewer target remains valid when the handoff cannot be made, but its cold cost is explicit.
- Run tests from the **committed** head, not a dirty tree: a check that compares the branch with
  `main` (the freeze rule) sees nothing before the commit exists.
- For a change that displaces a production caller, check its `Retirement:` record against the
  pinned diff and source. Answer missing evidence or known dead leftovers before approving;
  uncertainty may receive `APPROVE-WITH-RISK` only with the specific gap and follow-up named.
  Do not infer deletion safety from a zero-hit search, green tests or a green Keel check. Use the
  existing review, not another reviewer or a new build gate.
- The verdict is a word in the comment text: `APPROVE`, `APPROVE-WITH-RISK` (name the risk) or
  `BLOCK` (name the defect). GitHub review state stays `COMMENTED`, because every session shares
  one account.
- A deadline consulted once per iteration bounds the iterations, not the wall time: before
  approving a budget, ask what the longest thing between two checks can be; an unbounded call there
  makes the deadline advisory, and the constant must say so where it is defined.
- If the head moves after the review, the review covers only the sha it named. A prose-only edit of
  the PR body does not move the head and does not void the review.

## 5. Merge, by the reviewer

The reviewer who approved merges (a blind subagent included). There is no separate gate run and no separate presser: the Keel checks the reviewer runs (`KEEL.md`) are what block, and a blocking finding is a `BLOCK`.

The steps (pin the head, merge, check after, delete the branch; main's one whole run after merges) and the
definition of delivered are [`keel/KEEL_MERGE.md`](../../keel/KEEL_MERGE.md). Two changes that each
pass alone can break together (`mcp_discovery` × #1412), which is why main gets that run. Two additions here:

1. Closing check: `ci/closing-keywords.ps1 -Number <N> -Repository stabem/GraphHelm -Closes <issues>`.
   It reads the body and the commit messages; their union must equal the intent (the
   keyword rule is `keel/KEEL_MERGE.md`).
2. Read what landed: `git log -1 --format=%B origin/main` and `gh issue view` for every closed
   issue. Corrections go in a follow-up comment on the same PR.
5. Clean up the PR: run `graphhelm workspace sweep --apply --merged-branch <branch> --repo <repo> --pr <N> --head <sha> [--target <dir>]... [--keep-worktree <dir>]`; it stops the processes inside the PR's worktrees, unlinks links without following them, saves uncommitted work to `D:/_merge-leftovers/<N>/`, and removes every worktree and review checkout of the PR (any lane's), the named per-PR targets and the local branch. The `--keep-worktree` exception is in [KEEL_MERGE.md](../../keel/KEEL_MERGE.md) step 5.

Delivered is defined once, in [`keel/KEEL_MERGE.md`](../../keel/KEEL_MERGE.md).

### Task records

`task_record.py merged` requires GitHub-confirmed `MERGED` state and a matching merge SHA (full or at least 7 hex characters, case-insensitive); `--no-github` refuses unless `--dry-run` is set, and an `OPEN` state or null merge commit retries up to three reads, two seconds apart.

Where a Runtime execution coordinates the lanes (`gh-team` here), each step that writes an identity
line also records the matching `task.*` signal, so the Studio can draw each task's graph from
records instead of prose. The signal's `type` is the kind, its `source` is
`{"type":"user","id":"<your lane>"}` and its `description` is one `graphhelm-task-event-v1`
document (`extensions/builtin/graphhelm-development-contracts/schemas/task-event.schema.json`):

| Step | Kind | Fields besides `schema`, `taskId`, `revision`, `at` |
|---|---|---|
| take the issue (§1; the single issue lock, read and confirmed as LANES.md §4 "One issue lock") | `task.claimed` | `issue`, `lane`, `branch`, optional `plan`, optional `repo`, optional `journeys`, optional `leaseSeconds` (300..86400, never below the 300 s heartbeat cooldown: how long the claim stays live without a heartbeat; `task_record.py` sends 1800 unless `--lease-seconds` says otherwise; an older Runtime that refuses the field gets the claim once more without the DEFAULT lease, never without one the lane gave); that resend happens only for a shape refusal at `/signal/description`, never for the holder refusal below). Refused (`GHCLI003`, `/signal/description/taskId`, "`<taskId>` is held by lane `<lane>` (claim `<sequence>`)…") while another lane holds the task as an open pre-PR claim whose lease has not lapsed, unless the claim carries `assignedBy: <holder>`; a lane's own re-claim, and a claim after the holder's release, PR or lapsed lease, are admitted |
| release the latest still-open pre-PR claim | `task.released` | `lane`, exact `claimSequence`, bounded nonempty `reason`; only that claim ends, and unreadable history or a claim with a PR refuses release |
| release a lapsed claim of another lane | `task.released` | the same fields naming that lane, plus `releasedBy: graphhelm`; signed only by the `graphhelm` actor (the Stop reconciler); any other signer, or `releasedBy` on a lane's own release, is refused as an identity mismatch |
| release any open slice by its key (owner) | `task.released` | `lane` (the slice's), `sliceKey` (the `key` the tasks route serves, also for a slice with no claim sequence), bounded nonempty `reason`, `releasedBy` naming the signer; recorded only by an owner actor (the owner token with `X-GraphHelm-Actor-Type: owner`, as the orchestrator holds it); a key no open slice of that task and lane holds is refused |
| retire or restore lanes (owner) | `task.lane_retired` | `lanes` (1-64), `retired` (`true` retires, `false` restores), bounded nonempty `reason`; owner only. From its sequence on, every open slice of a retired lane reads `orphaned` and released by the signer, the lane leaves `liveness`, and the Runtime refuses its records until a later record restores it |
| judge every live pre-PR claim's lease | `GET /v1/executions/<id>/tasks` | the reply carries `now` (the Runtime's clock) and `leases`: per live pre-PR claim its `lane`, `claimSequence`, `leaseSeconds`, `expiresAt` (the later of the claim and its last heartbeat plus the lease) and `overdue`; the Stop reconciler releases every `overdue` one as `graphhelm` |
| see every open PR nobody moved | `GET /v1/executions/<id>/tasks` | `stalls`: per open PR slice with no task record for 2 h its `taskId`, `pr`, `reason` (`block_unanswered`: the author owes a fix push; `awaiting_verdict`: the reviewer owes a verdict; `awaiting_merge`: the approving reviewer owes the merge), `owner` (that lane), `idleSince` and `idleSeconds`; the Stop reconcile reply carries them as `stalled`, so `tasks.reconciled` records them |
| judge every lane's liveness | `GET /v1/executions/<id>/tasks` | the reply carries `liveness`: `budgets` (`silentAfterSeconds` 1800, `freeAfterSeconds` 900 and `stallAfterSeconds` 7200, declared once in the Runtime) and per lane in the fold `lastRecordAt` (its newest event of any kind), `owes` (an open slice it authored or the current review of an open PR) with `owesSince` (when the oldest began: the claim or the review assignment), `silent` (owes and quiet for the silent budget, quiet counted from the later of the last record and `owesSince`) and `free` (a record within the free budget); a stale claim is a lease that is `overdue`. Readers use these, never a threshold of their own |
| plan it, **right after the claim** (§2) | `task.planned` | `lane`, `classes`, `reviews`, `proof`, `critic` `{mode, passScore, maxRounds}`, `summary` |
| say an open pre-PR claim is alive (the Stop reconciler sends it on every turn end, so a live lane never has to) | `task.heartbeat` | `lane` only; the Runtime refuses one without an open pre-PR claim and limits them to one per 300 s per task; `task_record.py heartbeat` sends it by hand |
| open the PR (§3), and **again after every push** to it | `task.pr_opened` | `pr`, `headSha`, `journeys`, `lane`, optional `repo` |
| ask a reviewer (§4) | `task.review_assigned` | `pr`, `headSha`, `reviewer`, `ordinal` (1 or 2) |
| post the verdict (§4) | `task.review_verdict` | `pr`, `headSha`, `reviewer`, `verdict`, `commentUrl` |
| merge and read what landed (§5) | `task.merged` | `pr`, `mergeSha`, `closes`, `merger` |
| a PR closed without a merge (the Stop reconciler records it as the lane; by hand only to repair) | `task.closed` | `pr`, `reason` (`superseded` or `abandoned`), `by` (the superseding PR, or the PR itself when abandoned) |
| grade the design, once per round, when the plan says `critic: design` | `task.critic_verdict` | `round`, `score`, `passScore`, `maxRounds`, `verdict`, `designRef`, `lane`, `reasons` |
| refresh the Lanes Backlog (the coordinator; #1469): the open `current-wave` issues no open PR closes, at most 25; `taskId` is `backlog` and the newest record replaces the earlier ones | `task.queued` | `issues` (each `issue`, `title`, `openedAt`), `repo` |

`taskId` is `issue-<N>` for the whole life of the task. An issue worked in several PRs is one
task with one slice per PR: record `task.claimed` with the new slice's own branch before its
`pr_opened`, and the Studio draws each slice as its own graph, so merging one slice never hides the
next. `repo` is the GitHub `owner/name` (here
`stabem/GraphHelm`); the Studio links the task's issue and PR from it, so name it on `task.claimed`.
Pass `--assigned-by` on `claimed` when an order came from someone; the Studio labels it as the lane's report.
Release only a claim your lane recorded before opening a PR: use `released --claim-sequence <sequence> --release-reason "<why>"`. The sequence must identify that exact `task.claimed` record; the Runtime rejects another lane's claim. A release ends only that pre-PR claim and keeps its event history.
Name the journey the issue serves; the Studio Graph tab links the task to it. Ids are the stems of .graphhelm/journeys/*.journey.yaml.

**Plan is a recorded step.** A claimed task shows **Plan** lit until its `task.planned`
arrives; then **Implement**, or first **Critic** when the plan's `critic.mode` is `design`.
`planned` copies `classes`, `reviews`, `proof` and `critic` from `graphhelm keel plan` on the
task's paths (`--paths`, or a saved `keel plan --json` reply with `--plan-file`), so the fields are
the planner's decision, never retyped. With no `--paths`, the branch's diff against `origin/main`
plus uncommitted changes are the paths, so `planned --summary "<one line>"` alone is enough.
With nothing to plan from yet, or when `keel plan` cannot run, it records a stated default (`code`,
1 review, `tests`, no critic) and says so on stderr; `--classes`/`--reviews`/`--proof`/`--critic-mode`
override any field. `summary` is the plan in one line (at most 300 characters, the same bound as the issue `Summary:` line). A
`planned` recorded after the PR opened informs the graph without moving it back.
The Runtime refuses a `task.*` signal whose `source.id`, or whose `lane` / `reviewer` (on a
verdict) / `merger`, is not the actor recording it (`GHCLI038_ACTOR_MISMATCH`).

**The lane recipe: one command per step, the actor set per command.** Lanes share one MCP
registration, so a per-lane `GRAPHHELM_ACTOR` on the MCP server is not practical. Record each step
with `tools/task-record/task_record.py` (Python 3 standard library), which POSTs
`/v1/executions/<execution>/signal` with `X-GraphHelm-Actor: <lane>`:

The command for each step is the table in LANES.md §4.

A critic round's verdict is not an argument: the script derives it from the score and the round
(`pass` at `--pass-score`, default 8; `revise` while a round is left of `--max-rounds`, default 3;
`exhausted` on the last round), which is the rule the Runtime checks. `exhausted` needs a person.

- **Token.** `--token-file` names the Runtime's agent session token path, `<events>.agent.token`
  beside its events directory: `<the Runtime's --project>/.graphhelm/events.agent.token`. The
  default is that path relative to the current directory, which is right only inside the checkout
  the Runtime serves; from a lane worktree pass the Runtime checkout's absolute path. A Runtime
  mints it at start (D-058). **The value sent for a `task.*` record is not that token but
  the lane's own credential**, `<events>.agent.<lane>.token` beside it, which the owner or
  the orchestrator mints once per lane and execution:
  `graphhelm tasks credential --lane <lane> --execution gh-team --token-file <project>/.graphhelm/events.token`.
  The Runtime derives it from the owner token, the execution and the lane and verifies it by
  recomputation; the credential presented under another lane's `X-GraphHelm-Actor` is not a
  principal (401). **Rollout:** a Runtime started with `serve --require-lane-credential` refuses a
  `task.*` record under the shared session token (`GHCLI038_ACTOR_MISMATCH`, 403, nothing
  appended); without the flag (the default) the shared token keeps recording as before. The
  owner's order is: upgrade the Runtime, mint one credential per lane, then restart with the flag.
  Without the file, `task_record.py` refuses and prints the mint command (#1459); `--allow-shared-token` sends the shared token instead, with that note on stderr;
  under the flag that record is refused. Never use the owner's `events.token` as a lane record's
  bearer.
- **Defaults.** `--url http://127.0.0.1:8793`, `--execution gh-team`, `--repo stabem/GraphHelm`;
  `revision` is the step's position (claimed 1 … merged 5; `planned` is 1, beside its claim). `closes` is exactly the `--closes`
  numbers (the issues the squash closed, the intent `ci/closing-keywords.ps1` checked); with none
  given it is empty, which is right for a `Refs` PR.
  `--dry-run` prints the request without sending it. `--url` must be a loopback host
  (`127.0.0.1`, `localhost`, `::1`); any other is refused before the token is read.
- **Every push is a `pr_opened`.** A verdict counts only on a head that has a `pr_opened`
  record; a fix head without one leaves its verdicts outside the graph (shown as a verdict on an
  unrecorded head). Record `pr_opened` after each push; it requires `--reviewer` and records
  `review_assigned` in the same call, so Review is never drawn unnamed. Record
  `review_assigned` on its own only when the
  reviewer changes.
- **Repeated steps.** The record id and its `Idempotency-Key` end with a hash of the lane, the kind
  and the record without `at`. Any change is a new record: a new head in the fix loop, a second
  reviewer, a changed verdict on the same head. Run the same commands with the new values; no
  `--revision` is needed.
- **Retry.** Re-running a step already recorded (a call that timed out and may have landed) is
  safe: the same content gives the same key, the Runtime answers `GHE003_IDEMPOTENCY_CONFLICT` and the script prints
  `already recorded <signal id>`, exit 0. A `GHE001_SEQUENCE_CONFLICT` (another writer appended
  first) is re-sent up to three times.
- **Output.** `recorded <signal id>` or `already recorded <signal id>` and exit 0, or
  `REFUSED <signal id>` plus each error diagnostic and exit 1.
- **Limit.** A lane credential proves which lane wrote the record; the Runtime refuses a
  `task.*` record from the shared agent session token and one whose lane is not the credential's.
  What it cannot stop is D-058's limit: every credential file belongs to the same OS user, so a
  session that reads another lane's file from disk can still sign as that lane. Hand each lane
  only its own path.

The MCP `signal` tool holds the shared session token, so it is no longer a door for `task.*`
records; `graphhelm execution signal` writes to a store directly as `owner-cli` and is not a lane
door either. Task records go through `task_record.py` with the lane's credential.

## 6. Housekeeping

Lanes sharing one machine follow [LANES.md](LANES.md) on top of this section: where a lane writes,
how it removes a worktree, the single build slot and the per-worktree cargo target.

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

Names that comments and specs give to notes of the old process resolve in
[ARCHIVE_NAMES.md](ARCHIVE_NAMES.md).
