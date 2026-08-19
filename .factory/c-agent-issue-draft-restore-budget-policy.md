The restore path's fixed per-step timeouts make the gate flaky, and report the timeout as corruption

Split out of #19. That issue registers flaky *tests*; this one is about the *policy* that makes them
flaky, because registering sites one at a time will not converge.

## The victim rotates, which is how we know it is not the tests

`adapters/postgres-event-store/tests/backup_restore.rs` has exactly two `#[ignore]`d tests. In the
M09 gate run of 2026-08-19:

| test | matrix 1 (C locale) | matrix 2 (non-C, machine hot) |
|---|---|---|
| `admin_operator_binds_pool_profile_and_source_identity` (`:2148`) | ok | **FAILED** |
| `constructor_bounds_reconciliation_catalog_locks` (`:2268`) — the one registered in #19 | ok | ok |

The flake already registered in #19 **passed** in the same run where its unregistered sibling failed.
Same file, same stage, same budgets, and the loser swapped. A defect that moves between tests under
load is not a property of either test. It is a property of what they share.

## What they share

In `adapters/postgres-event-store/src/backup.rs`:

- **11 sites** bound a step with `self.process_timeout.min(...)` — 10 at `Duration::from_secs(30)`,
  1 at `Duration::from_secs(10)`.
- 6 of those are `tokio::time::timeout` wrappers; 2 more are production
  `ProcessWatchdog::start_with_cancellation` calls (`backup.rs:1152`, `backup.rs:1273`) driving
  `pg_dump` / `pg_restore` children.
- Each bound is an **independent** fixed wall-clock cap. The operation as a whole has **no**
  deadline.

So a single restore has ~11 independent chances to trip, each on a fixed cap, against step
durations that scale with machine load. The probability that at least one trips compounds across
all of them — which is exactly the behaviour observed: reliable when idle, intermittent under the
second matrix of a long gate.

Measured for scale: `admin_operator_binds_pool_profile_and_source_identity` takes **~80 s on an
idle machine** (80.52 / 79.38 / 81.95 s across three isolated runs). Its individual steps are
therefore already a meaningful fraction of a 30 s cap before any contention exists.

## The second defect: the timeout is reported as corruption

Every one of those elapsed paths maps to `BackupError::InvalidRestore` — the same value returned
when the archive is truncated, the MAC fails, or the marker contract will not parse. (`InvalidRestore`
appears 221 times in `backup.rs`, tests included.)

This is a flattening: two causes with opposite responses are fused into one legal value, and the
distinction is destroyed at the boundary. "The machine was busy" and "this backup is not
trustworthy" are the two readings, and an operator holding the error cannot tell them apart. The
wrong reading is the dangerous one in both directions — treating a slow machine as a corrupt backup
triggers a needless restore-from-elsewhere; treating a corrupt backup as a slow machine gets it
retried until it appears to work.

Note this is strictly worse than the sibling case in #19, which at least fails with `Elapsed` — a
value that names timing. Here timing is laundered into a corruption verdict.

## Proposed fix

1. **Give elapsed its own error.** A distinct variant (or at minimum a distinguishable code
   alongside `GHB002_RESTORE_INVALID`) for "a bounded step exceeded its budget". This is the small,
   high-value half: it makes every future occurrence self-diagnosing and ends the re-diagnosis cost
   #19 exists to complain about.
2. **Make the budget a deadline for the operation, not a cap per step** — or make the per-step caps
   derive from one operation-level budget, so that adding a step stops silently adding another
   chance to fail.
3. **Test budgets are anti-hang devices, not behavioural bounds.** Where a timeout exists only to
   stop a test hanging, it should be generous enough that tripping it is a real signal, and the
   assertion should not be able to pass or fail based on it.

## Why this is worth its own issue rather than another #19 entry

#19's own text: "A gate that cries wolf under load trains the operator to re-run until green, which
is how a real regression eventually slips through." Every milestone that adds a bounded step to this
path adds a new candidate victim. Registering them one at a time treats the symptom and guarantees
the list keeps growing; item 1 above makes each occurrence diagnose itself, and item 2 stops
manufacturing new ones.

## Provenance

Surfaced by the M09 gate red on `issue-m09-arming-the-alarm` (`d10916b`). That red was established
as unrelated to the branch: the gate log names the byte-identical test artifact
(`backup_restore-c2c49007dce7b021.exe`) in both matrices — it passed in the first and failed in the
second with no recompile — and the branch diff touches no PostgreSQL code and no SQL. Isolation
under the identical locale passed 3/3, which refutes determinism under the non-C collation but does
**not** establish a low rate (0 failures in 3 bounds it only at ~63% at 95% confidence). **The rate
under load remains unmeasured**, and is not claimed here to be small.
