# #104 — slot run plan

Written before the slot opens so machine time is spent executing, not deciding. Branch
`issue-104-grounded-reads-discovery`, cut off main `07b1243`, two commits already in place
(`bb5a794` fix, `daeb8e5` pinned regression tests). Scope is ONE crate: `tools/acceptance-map`
(`lib.rs` + `tests/grounded.rs`). Nothing else touched, so `cargo clean -p` scope is that one
crate — per L's binding rule (orchestrator-board.md:322-330): clean at slot start AND before
the final result, clean invocations + exit codes recorded in the SAME file as the test/gate
exit code, or the green is unverified months later.

`CARGO_TARGET_DIR=D:/graphhelm-target-m10` (shared, warm per B). State the env in every figure.

Results directory: `.factory/f-agent-104-results/` (created).

Sealed BEFORE running — do not edit predictions after seeing output.

---

## Phase 0 — clean, slot start

```
cargo clean -p acceptance-map
```
→ `results/00-clean-start.txt` (exit code recorded in the file, not just scrollback).

## Phase 1 — the observed red, base-innocent (confirm, don't just inherit B's finding)

Stash this branch's two commits, run against plain `origin/main` content for the two changed
files only (`git stash` or a throwaway checkout of grounded.rs/lib.rs at `07b1243`), so the RED
is independently observed on my own machine, not taken on B's word alone.

```
cargo test -p acceptance-map
```
→ `results/01-red-on-main.txt`.

**Predicted (P1):** RED. Panics in `acceptance_map_is_grounded`'s per-prover suite assertion,
first hit at the FIRST clause carrying a non-`workspace tests` prover in `m05-clauses.toml`
declaration order — `real-work-to-completion`'s prover, suite `runtime_http`. Message contains
`"suite runtime_http must be on the gate's CLI suite list"`. (A single `assert!` panics the test
function immediately, so only the first-in-order failure surfaces per run — this is expected,
not a partial fix.)

Restore the two commits immediately after capturing output.

## Phase 2 — the fix, green

```
cargo test -p acceptance-map
```
→ `results/02-green-with-fix.txt`.

**Predicted (P2):** GREEN. All tests pass: `acceptance_map_is_grounded` (integration) plus the
two new unit tests (`gate_excluded_suites_ignores_its_own_illustrative_comment`,
`gate_excluded_suites_parses_a_real_entry_past_the_comment`).

```
cargo clippy -p acceptance-map --all-targets --all-features --locked -- -D warnings
```
→ `results/02b-clippy.txt`. **Predicted:** clean, exit 0.

## Phase 3 — sabotage, per the issue's own ask

Two sabotages, each restoring the tree before the next. Predicted casualties sealed here, not
after seeing red.

**Sabotage A — scratch suite out of discovery reach.** Edit `docs/acceptance/m05-clauses.toml`:
change one real prover's `suite` value (`runtime_http`, the `real-work-to-completion` clause) to
`scratch_suite_104_out_of_reach` — a name with no `apps/cli/tests/*.rs` file behind it.

```
cargo test -p acceptance-map
```
→ `results/03a-sabotage-out-of-reach.txt`.

**Predicted:** RED, at the new discovery-based assertion (`grounded.rs`, "must be on the gate's
discovered CLI suite list"), naming `scratch_suite_104_out_of_reach` and printing the real
discovered set (which will NOT contain it). Revert the TOML edit immediately after capture.

**Sabotage B — unexplained exclusion.** Edit `ci/gate.ps1`'s `$excludedSuites` map: add a real
line `'runtime_http' = 'sabotage probe #104'` (past the illustrative comment, so it is a REAL
parsed entry, not the trap the comment itself represents).

```
cargo test -p acceptance-map
```
→ `results/03b-sabotage-exclusion.txt`.

**Predicted:** RED, same assertion, now failing because `gate_cli_suites` correctly excludes
`runtime_http` per the gate's own (sabotaged) map — proving the exclusion path is actually read,
not just present in the parser's code path. Revert the `gate.ps1` edit immediately after
capture.

Confirm `git status --short` shows only the intended two committed files changed once both
sabotages are reverted.

## Phase 4 — clean before the final result, per L's rule

```
cargo clean -p acceptance-map
```
→ `results/04-clean-final.txt` (exit code recorded).

```
cargo test -p acceptance-map
```
→ `results/04b-final-green.txt`. **Predicted:** GREEN, matching Phase 2 exactly — the clean
proves this isn't an incremental-artifact false green.

---

## Reporting

PR body's validation section cites every figure to its `results/` file — nothing from memory of
a run. HIT/MISSED per prediction, in the terms sealed above, no softening a miss. `git status
--short` output after Phase 3's reverts goes in the PR body too, proving no sabotage edit
leaked into the diff.
