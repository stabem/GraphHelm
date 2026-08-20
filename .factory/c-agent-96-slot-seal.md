# #96 slot — predictions SEALED before any cargo

**sha-at-launch: `6e9c0fe2a0ad6db74904b0689932fb2afbb44be1`** (branch `issue-96-driver-failure-split`).
**origin/main at launch: `e60a27d`.** Written before the first cargo invocation of this slot.

## Machine baseline

| volume | free | role |
|---|---|---|
| F: | **6.6 GB (98% used)** | the repo — git only, no build output |
| C: | 83 GB | `TEMP` — where every test's tempdir and the PostgreSQL clusters land |
| D: | 506 GB | `CARGO_TARGET_DIR=D:/graphhelm-target-m10` per standing policy |

F: is **below the 9.0 GB floor** the orchestrator has previously called a breach. Reported rather
than worked around. It is likely tolerable *this time* because the build writes to D: and the tests
write to C: — F: sees git operations only. **That is a reason, not a guarantee**, and it is stated so
that if something dies on F: nobody has to reconstruct why the number was accepted.

## Condition (i) — the observed red, in natural form

**The edit:** revert the four-site swap in `prepare_drive` — `setup_failure` back to `driver_failure`
— so class (a) answers `GHCLI016` again. Nothing else touched. This is the pre-#96 shape.

### SEALED: exactly THREE casualties, named by test and by assertion line

| # | test | assertion line | why it falls |
|---|---|---|---|
| 1 | `a_resume_whose_drive_setup_fails_leaves_the_operator_hold_intact` | **:425** | asserts the setup path answers `GHCLI019_DRIVER_SETUP`; reverted it answers `GHCLI016` |
| 2 | `a_start_whose_drive_setup_fails_commits_no_execution` | **:640** | same, on the `start` route |
| 3 | `the_same_idempotency_key_after_a_failed_setup_still_executes` | **:732** | same, on the same-key retry |

Each must be red **at its own CODE assertion, named by panic site** — `:425`, `:640`, `:732`. A red
anywhere else means the arrangement broke, not that the guard saw.

### SEALED: exactly TWO survivors — and this half is the discriminating evidence

| test | why it must stay GREEN |
|---|---|
| `a_resume_whose_drive_setup_succeeds_still_commits_the_decision` | the positive control asserts a **200** and a committed decision; the revert changes only which code a FAILURE carries, so a success path is untouched |
| `a_second_resume_after_a_failed_one_is_still_accepted` (`:479`) | asserts the retry is **not** `GHCLI005_EXECUTION_STATE` and is 500; reverted it is `GHCLI016` — still 500, still not `GHCLI005` |

**A blanket 5/5 red would REFUTE this seal**, not confirm it. The claim is that #96 changed the code
a failure carries and nothing else; if the revert also takes the positive control or the
hold-survival guard, it changed more than claimed and the split is not the isolated thing the commit
says it is.

## Condition (ii) — guard-header framing verified strictly-stronger

The file header claims the guards' having to change **is the evidence the split took effect**.
Verification: the three casualties are exactly the three tests that assert the CODE, and the two
survivors assert **behaviour** (a committed decision; a retry that is not a state refusal). If a
behavioural guard fell, the header's claim would be false — the change would have altered behaviour,
not just the code a failure carries.

## UNINFORMATIVE cells — outcomes that measure nothing

- **U1** — any casualty red at a line other than `:425` / `:640` / `:732`. Arrangement broke.
- **U2** — 5/5 red. Refutes the seal (see above); the change is not isolated and must be re-derived
  before the gate is worth running.
- **U3** — a compile failure under the revert. Typed-unbuilt work has never been compiled; a build
  error is a HARNESS result, not a red, and the revert measures nothing until it builds.
- **U4** — the suite passing 5/5 under the revert. Would mean the code assertions never execute —
  the vacuous-green shape — and would kill the guards, not the seal.

## Base staleness — declared before it becomes a surprise

The branch is **12 commits behind `origin/main`** (`e60a27d`), which now carries the readers-serialize
regression fixes. **The observed red runs on the branch as B reviewed it** — it concerns only my own
guards and my own revert, so main's state is irrelevant to it. **The FULL GATE will not run on a
12-commit-stale base**: gating a tree without landed fixes measures a tree nobody has. Main gets
**merged, never rebased** (B's approval is pinned to `6e9c0fe`; a rebase voids it, a merge preserves
it — the #83 precedent), and the merge is a **delta beyond what B reviewed**, so it gets flagged for
their one-line confirm even though it changes none of my lines.

## Death condition

If the revert does not drop all three code assertions at their own lines, the guards do not measure
the split and #96's evidence is void regardless of what the gate says afterwards.
