> **PROVENANCE: this document became a comment on #19 (third instance at an unregistered site).**
> Provenance is the strong relation and the one grep cannot see: the derived artefact carries the
> content and never the source's name, so only the author can record it. Established against
> `gh issue list --state all` and the PRs, not from memory.

## Third instance of class 1, at an unregistered site

Observed 2026-08-19 on the M09 gate (`issue-m09-arming-the-alarm`, merged tip `d10916b`).

**Failing test:** `admin_operator_binds_pool_profile_and_source_identity`
(`adapters/postgres-event-store/tests/backup_restore.rs:2148`) — a different test from the
`constructor_bounds_reconciliation_catalog_locks` case at `:2268` already registered above.

```
[gate] FAILED: PostgreSQL matrix under a non-C collation (exit 101)

test admin_operator_binds_pool_profile_and_source_identity ... FAILED
thread 'admin_operator_binds_pool_profile_and_source_identity' panicked at
adapters\postgres-event-store\tests\backup_restore.rs:2148:14:
called `Result::unwrap()` on an `Err` value: InvalidRestore

test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 6 filtered out; finished in 92.65s
```

Same conditions this issue already names for class 1: **non-C collation matrix, second matrix of
the run, machine hot** (and, this time, under disk pressure with other work building concurrently).

### Mechanism

Line 2148 is `restore_from_path(&destination).unwrap()`. The operator on line 2141 is constructed
with `Duration::from_secs(30)` as its process timeout, and the restore path bounds many steps on
that budget — repeated `tokio::time::timeout(self.process_timeout.min(Duration::from_secs(30)), …)`
plus a `ProcessWatchdog::start_with_cancellation(child, self.process_timeout, …)` — with elapsed
mapped to `BackupError::InvalidRestore` at every one of them.

This is the same shape as class 1 (a timing budget that exists to stop a hang, not as a behavioural
bound, elapsing first under load) with one extra difficulty worth recording: `InvalidRestore` is a
deliberately redacted, heavily-overloaded error value. Class 1 at least fails with `Elapsed`, which
names timing. Here the timeout is laundered into the same error a genuinely corrupt archive
produces, so **the red carries no diagnosis at all** — the operator cannot tell a slow machine from
a bad backup by reading the failure.

### The victim rotates — evidence this is a class, not a site

`backup_restore` has exactly **two** `#[ignore]`d tests, and they are the two this issue is about:

| | matrix 1 (C locale) | matrix 2 (non-C, hot) |
|---|---|---|
| `admin_operator_binds_pool_profile_and_source_identity` (`:2148`) | ok (L373) | **FAILED** (L1238) |
| `constructor_bounds_reconciliation_catalog_locks` (`:2268`, registered above) | ok (L374) | ok (L1239) |

The already-registered flake **passed** in the very run where its unregistered sibling failed. Under
load the loser rotates between the two heaviest, most timeout-bounded tests in the file. That is the
signature of a shared, load-sensitive resource — wall-clock against fixed budgets — rather than a
defect belonging to either test. Registering sites one at a time will keep producing "new" flakes;
the budget policy is the thing with the defect.

### Isolation, per this issue's own method

Single test, `ci/postgres.ps1 -TestArgs`, under the identical locale
(`GRAPHHELM_PG_LOCALE='English_United States.1252'`):

| iteration | verdict | runner line |
|---|---|---|
| 1 | PASS | `test result: ok. 1 passed; 0 failed; 7 filtered out; finished in 80.52s` |
| 2 | PASS | `test result: ok. 1 passed; 0 failed; 7 filtered out; finished in 79.38s` |
| 3 | PASS | `test result: ok. 1 passed; 0 failed; 7 filtered out; finished in 81.95s` |

**PASS 3 / 3 · FAIL 0 / 3 · HARNESS-BROKE 0 / 3.** The `1 passed` on every line is the receipt that
the test executed; see the methodological note below for why that column is not decoration.

**What 3/3 does and does not establish.** It does NOT establish a low failure rate. Zero failures in
three runs bounds the isolation failure rate at roughly **≤63%** at 95% confidence
(`1 − 0.05^(1/3)`), which is almost no constraint at all — the same weak bar this project has been
burned by before. Quoting it as "passes in isolation, therefore flaky" would be exactly the
cry-wolf reflex this issue was opened to stop.

What it does establish, with no statistics required, is a clean refutation: **the failure is not
deterministic under the non-C locale.** A collation-dependent defect in the ordinary sense would
have failed 3/3 under the identical `GRAPHHELM_PG_LOCALE`. It failed 0/3. That kills the hypothesis
which mattered most, because that is the one that would have meant a real pre-existing bug on the
milestone branch.

Still alive and NOT separated by this measurement: a genuinely load-sensitive failure, including one
that needs load *and* collation together. Isolation on an idle machine cannot reach that state, so
the rate under load remains unmeasured.

Note the duration: **~80 s for this one test on an otherwise idle machine** (80.52 / 79.38 / 81.95 —
tight spread). Under full-gate load the internal 30 s budgets have correspondingly less headroom,
which is the whole hypothesis.

### Not caused by the branch it surfaced on

Same as the history already recorded here — both prior cases pre-dated the branches that surfaced
them. Two facts, both readable from the gate log without re-running anything:

1. The same test **passed at L373** in the "PostgreSQL ignored matrix" and **failed at L1238** in
   the non-C matrix, in the same run. Both stages execute the identical command
   (`cargo +1.97.1 test --workspace --all-features --locked -- --ignored --test-threads=1`) against
   the **byte-identical artifact** — the log names `backup_restore-c2c49007dce7b021.exe` in both
   matrices, so nothing was recompiled between them. Deterministic, collation-independent breakage
   takes both; it took one.
2. `git diff --name-only 576e553 d10916b` touches no PostgreSQL code and no SQL. The only new
   ordering structure in the diff is a `BTreeMap<String, _>`, which orders by Rust byte order and
   is locale-independent by construction.

### Suggested addition to the fix

Alongside the remedy already proposed for class 1, this site argues for **not collapsing timeout
into `InvalidRestore`**. A distinct variant (or at minimum a distinguishable code) for "a bounded
step elapsed" would let the gate tell load from corruption without a human re-deriving it from the
panic line each milestone — which is the re-diagnosis cost this issue exists to stop paying.

### Methodological note, recorded because it nearly produced a false finding

The first isolation harness written for this returned 0/3, which under the reading declared before
the run meant "deterministic under the non-C locale → real collation-dependent bug." The test had
never executed: the argument vector reached `cargo` as a single comma-joined token and was parsed
as a toolchain name. A broken harness and a genuine deterministic bug are indistinguishable from an
exit code, so the harness now classifies on the runner's own `<test> ... (ok|FAILED)` line and has
three outcomes — PASS / FAIL / **HARNESS-BROKE** — refusing to compute a rate over iterations that
measured nothing. Anyone re-measuring these flakes should assert the test ran before reading a rate.
