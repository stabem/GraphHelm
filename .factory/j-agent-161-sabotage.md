# #161 — sabotage of the identity-registry guard: predictions sealed BEFORE the runs

Base: `issue-161-clearance-identity-registry` @ 4645d75 + the fixture strengthening in the same
commit as this file. Subject: `the_identity_registry_folds_deterministically_and_revocation_removes`
in `core/events/tests/execution_projection.rs`.

## Why this exists

H's review of PR #193 landed the finding that the append failure sits UPSTREAM of all seven
clearance assertions, so none of them has ever been observed failing at its own assertion. The
registry guard is the one cell on the free side of that line — it runs. But a guard that has only
ever been green is decoration by exactly the same rule. This file is the attempt to make it fail
on purpose, one assertion at a time.

## What I found BEFORE running anything, by reading the fold against the fixture

The guard as written at 4645d75 had four assertions and was **blind to two of the three mutations
below**. Both blindnesses come from the same fixture shape: every overwrite in it followed a
`remove`, and the only revoked identity was later re-registered.

  - a `remove` that does nothing is invisible on `auditor-a`, because the later registration
    overwrites to the same value whether or not the remove happened.
  - a first-write-wins `insert` is invisible on `auditor-a` for the same reason: the `remove`
    leaves an empty slot, which first-write-wins fills identically.

So the guard's own NAME ("...and_revocation_removes") claimed a property its assertions could not
see. Two identities were added to close that: `auditor-c` (revoked, never restored) and
`auditor-d` (rotated with NO revocation between the two registrations).

## Sealed predictions

Each row names the mutation, the assertion I expect to go red, and the cell that would make the
run UNINFORMATIVE about the claim it is supposed to settle.

| id | mutation in `core/events/src/projection.rs` | predicted RED assertion | UNINFORMATIVE if |
|----|---------------------------------------------|-------------------------|------------------|
| S0 | none (control) | none — GREEN | any red at all: the fixture is broken, not the fold |
| S1 | `ClearanceIdentityRevoked` arm: delete the `remove` call | `auditor-c` absent | red lands on a DIFFERENT assertion — then my blindness reading is wrong |
| S2 | `ClearanceIdentityRegistered` arm: `insert` -> `entry().or_insert()` | `auditor-d` == last fingerprint | red lands on a DIFFERENT assertion — same |
| S3 | `ClearanceIdentityRegistered` arm: delete the `insert` call | `auditor-b` present | GREEN — then no assertion reads the registry at all |
| S1' | S1 run against the PRE-strengthening assertions only | none — GREEN | red — then the strengthening was not load-bearing and I should say so |
| S2' | S2 run against the PRE-strengthening assertions only | none — GREEN | red — same |

S1' and S2' are the ones that can cost me: they are the rows where the scoreboard says the two
new identities added nothing. They are in the table for that reason.

The determinism assertion (`first == second`) has **no row**, and that is a statement, not an
omission: I could not construct a mutation of this fold that breaks it. The registry is a
`BTreeMap` and the fold reads no clock and no randomness. It is a regression net against a future
container swap, not a live discriminator today. Counted as unproven, not as proven.

## Results

Run at `97b2030`, target dir `D:/graphhelm-target-j161`, raw output in
`.factory/j-161-sabotage-results.txt`. Assertion line map, from the restored file:

| line | assertion |
|------|-----------|
| 1551 | determinism (`first == second`) |
| 1555 | `auditor-b` present |
| 1560 | `auditor-c` absent |
| 1566 | `auditor-d` holds the LAST fingerprint |

| id | predicted | observed | panic site | verdict |
|----|-----------|----------|------------|---------|
| S0 | GREEN | `ok, 1 passed` in 0.17s | — | as predicted |
| S1 | red at `auditor-c` | FAILED | `:1560` = `auditor-c` | **as predicted** |
| S2 | red at `auditor-d` | FAILED | `:1566` = `auditor-d` | **as predicted** |
| S3 | red at `auditor-b` | FAILED | `:1555` = `auditor-b` | **as predicted** |
| S1' | GREEN | `ok, 1 passed` | — | **as predicted** |
| S2' | GREEN | `ok, 1 passed` | — | **as predicted** |
| S0-restored | GREEN | `ok, 1 passed` | — | mutations reverted cleanly |

**Six for six, and no cell went UNINFORMATIVE.** Each mutation fell on the one assertion named as
the only one that could catch it, and no mutation fell on a different one.

**S2 printed its own mechanism**, which is worth more than the pass mark:

```
left: Some("sha256:ffff...")
```

Under `entry().or_insert()`, `auditor-d` kept `f` — the FIRST registration — where the fold must
keep `0`, the last. That is first-write-wins caught in the act, not merely inferred from a red.

**S1' and S2' are the rows that could have cost me, and they paid instead.** The same two mutations
that fail the strengthened guard leave the pre-strengthening assertions **green**. So the two added
identities are load-bearing, not decoration: without them the fold could lose its `remove` entirely,
or silently become first-write-wins, and this test would have reported success both times.

## The two controls that make the numbers readable

**Harness proof.** Every cell carries a `test result:` line, so under the adjudication rule sealed
above every cell is DATA and none is HARNESS-BROKE. `26 filtered out` in each is the second half of
that proof: the binary was the real suite with the filter applied, not an empty target.

**Contamination.** Sealed before the output existed: five rows identical would condemn the run,
because the target dir was not virgin (`debug/` held artifacts stamped 09:46-10:09, from before the
one-dir-per-lane rule). **The rows are not identical** — three distinct panic sites, two greens, and
a control that returns to green. Artifacts that ignored my edits could not produce that spread, so
the build tracked the source and the run stands.

## What this still does NOT establish

The determinism assertion (`:1551`) **never fired**, exactly as its missing table row predicted. It
remains unproven: no mutation of this `BTreeMap` fold breaks it, so it is a regression net against a
future container swap, not a discriminator. Counted as unproven, not as passed.

And the whole matrix is about the **accumulator**. Membership-at-N is untouched here, because it
only becomes observable when a clearance is validated at N — which is #201's work.

## A DEFECT IN THIS INSTRUMENT, found while the run was in flight and recorded BEFORE the numbers

The runner filters cargo's output through:

```
grep -E "^(test |thread |assertion|  left|  right|test result|error)"
```

Measured by piping known cargo lines through that exact pattern: **`Blocking waiting for file lock
on package cache`, `Compiling`, and `Finished` are all dropped.** Only `test result:` survives.

**So the results file cannot distinguish a cell that RAN from a cell that never started.** A cargo
blocked on the global package-cache lock — which is what stalled this very run, with another
agent's `cargo test --workspace` holding the machine — produces an EMPTY cell, and an empty cell
next to a green one reads like a green one. That is `harness-must-prove-the-test-ran` committed
inside the instrument built to enforce it, twenty minutes after I described the same failure to a
reviewer.

It is the same shape as the `Shell cwd was reset` warning that bit me earlier: **a filter that is
silent in the dangerous case.** I wrote the filter to keep the output readable, and readability
cost me the one line that explains a stall.

**Adjudication rule, sealed before the output exists:**

- A cell counts as DATA only if it carries a `test result:` line. That line is the harness proving
  the binary ran.
- A cell with no `test result:` line is **HARNESS-BROKE**, never a pass and never a red, regardless
  of what else it shows.
- If any cell is HARNESS-BROKE, the row is re-run with the filter widened; the surrounding cells
  are not promoted to fill the gap.

The script is **not** being edited while it runs — a shell reads a script incrementally, so editing
in place can corrupt execution mid-run. The filter is widened afterwards, and this paragraph stays
as the record of why.
