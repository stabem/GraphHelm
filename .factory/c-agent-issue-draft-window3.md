> **PROVENANCE: this draft became #71 — *a consumption can be recorded against a lease a rival already burned*.**
> Established by title match against `gh issue list --state all`, not from memory. Written here
> because the derived issue carries the content and never the source's name: provenance is the
> strong relation and the one grep cannot see, so only the author can record it.

## What is wrong

The wake sweep's recorder decides a lease is still live from one read of the stream, then
asks the store for the sequence to append at in a **second** call. A rival sweep that burns
the same lease between those two calls satisfies both checks:

- the liveness-and-rendezvous filter already passed, against a replay taken before the rival;
- the sequence returned afterwards is genuinely current, so the store's sequence check has
  nothing to refuse.

The second consumption lands on a lease that no longer exists, and the fold refuses **every
later replay of that stream**. That is the corruption from #55, reached through the one
sub-window its fix did not close.

The archived evidence from the original incident is preserved at
`.factory/archive/pair-events-corrupted-2026-08-16`.

## Mechanism

Three interleavings are possible between two sweeps racing the same lease. Two were already
guarded; the third was not:

| rival lands | outcome |
|---|---|
| before the recorder's validation read | filter drops our consumption — safe |
| after the recorder's sequence read | our expected sequence is stale, the store refuses — safe |
| **between the two reads** | filter passed, sequence is current, **both consumptions land** |

Both surviving commit messages in this area state that one store handle's exclusive lock made
the validation and the append atomic. It never did: the lock is taken and released per
operation, and an open handle holds none in between. That is true at the parent of the
read-concurrency change as much as after it, so **the window predates that change and
reverting it would restore nothing here**. The claim was a sentence standing in for a guard.

## Why it was not caught

The existing 15-round race test cannot reach this window. Measured on the base commit before
any change: **0 reproductions in 13 runs** (10 isolated, 3 in-suite). A deterministic seam
reaches it **every time**.

Two further measurements taken while fixing it, both worth their own follow-ups:

- with the sweep's recorder call deleted entirely, the race test still passes — it is green
  when the thing it protects does not run at all;
- with the recorder returning early for an unrelated reason, the pre-existing deterministic
  guard for #55 also still passes.

So the chain proving #55 had no link that fails when the protected code stops existing. The
fix was real; nothing was measuring that it stayed real.

## Approach

Pin the sequence from the **same** read the liveness decision is made against. A rival that
lands after that read makes the pin stale and the store's existing sequence check refuses the
append. The two blades then cover opposite sides of one instant — the filter catches a rival
who burned the lease before the read, the pin catches one who lands after — and neither is
redundant.

A conditional-append store API was considered and rejected: deciding inside the append's own
lock closes the window too, but it takes the test seam with it (a rival cannot be injected
inside a lock the recorder itself holds), and it adds a public store API for one call site.

## Guards

- `a_rival_consume_between_validation_and_the_sequence_pin_appends_nothing` — the deterministic
  red. Asserts nothing was recorded, that the stream still replays, and that the **rival's**
  burn is the last consumption on that session (a count alone is reachable from a dozen
  unrelated paths in this recorder, so it would go green on a dead recorder).
- `a_stale_capture_never_burns_the_lease_that_replaced_it` — pins the rendezvous half of the
  filter, which had no test and is the only thing standing between a stale capture and the
  lease that replaced it.

Sabotages, each observed to fail individually: restore the second sequence read; drop the
rendezvous comparison; make the recorder bail early. Plus the #55 race test and the two
pre-existing guards kept green, and clippy clean across the workspace.

## Out of scope, tracked separately

- A stale capture held across a delayed ring can still match a re-armed lease on session and
  rendezvous, and burn it. Silent: the log stays legal and every surface reports calm.
- `WakeLeaseConsumed` carries no arming identity, so no replay can detect that class at all.
- The race test's per-round assertion needs upgrading to identity rather than counts.
