> **PROVENANCE: this draft became #74 — *a woken sleeper's fresh lease is burned by the sweep that woke it*.**
> Established by title match against `gh issue list --state all`, not from memory. Written here
> because the derived issue carries the content and never the source's name: provenance is the
> strong relation and the one grep cannot see, so only the author can record it.

## What is wrong

A sleeper is woken and re-arms, and the sweep that woke it burns the lease it just armed.

The sequence, which is this factory's own daily shape rather than a contrived one:

1. A session arms a lease on rendezvous `X`.
2. A sweep captures it as due, rings it, and the sleeper wakes.
3. The sleeper does its work and re-arms — on the **same** rendezvous, because our agents use
   fixed rendezvous ids.
4. The first sweep's phase 3 finally runs. It finds a live lease for that session whose
   rendezvous is `X`, and consumes it.

The lease burned in step 4 is the one from step 3 — the one its sleeper is asleep on at that
moment. No later append can ring it, because the lease is gone. The sleeper waits out its full
declared horizon and `wake-wait` reports that the deadline passed and nothing happened.

## Why nothing catches it

Three independent reasons, each verified in source:

- The recorder's filter compares **session and rendezvous only**, and `DueLease` carries no
  arming identity at all. Every comparison available to it passes.
- The fold accepts it. Consuming a lease that is **live** is legal, so there is no corrupt
  error and no refused replay — none of the noise the #55 family makes.
- `wake-wait`'s timeout path never consults the consumption receipt. So the sleeper is told
  "your deadline passed" while the store's own record for that session says `rung`. Two
  surfaces answer one question differently, and the operator acts on the calm one.

That is absence laundered into calm, which is the rule this milestone exists to enforce,
inverted and silent.

## What the window-3 fix already closed, and what it did not

The sequence pin landed in #71 closes the **in-window** slice for free: a re-arm that lands
*after* the recorder's decision read makes the pinned sequence stale, so the append is refused
and the replacement lease survives.

The **out-of-window** slice is untouched, and it is the one that matters here. If the re-arm
landed *before* the decision read — the sweep rang, the sleeper woke and re-armed, and only
then did the delayed phase 3 look — the decision sees a live lease matching on session and
rendezvous, and the sequence it pins from that same read is perfectly current. Nothing refuses
the burn.

## Why this is not a coverage gap that a new test alone fixes

Measured this milestone, each observed individually:

- The 15-round race test passes **green with the sweep's recorder call deleted entirely**. It
  is green when the thing it polices does not run at all.
- Under a sabotage that stops the recorder dead, the pre-existing deterministic guard for #55
  also stays **green**. Only one guard in the whole chain noticed, and it noticed by counting.
- With the consumption's idempotency key stripped of its sequence component, the belt reported
  `armed=15 consumed=1` and **passed**. Fourteen of fifteen consumptions never happened, and
  the guard named `never_double_consume` was satisfied.

So the chain can see a recorder that is **dead** and cannot see one that is **wrong** — and a
working recorder doing the wrong thing is exactly what this defect is.

## Approach

Two halves, deliberately separated because they close different things:

1. **Recorder-side discriminator (closes the hazard).** Carry the arming's sequence into
   `DueLease`, derived from the history the phase-3 read already holds, and compare it in the
   filter. A capture from before the wake then cannot match the lease armed after it. Internal
   to `apps/cli`; no event, no schema.
2. **Arming identity on `WakeLeaseConsumed` (makes the fold able to see it).** Today the event
   carries execution, session and reason, so no replay can detect this class at all. This is a
   schema change and takes the full ritual: both schema copies byte-identical, both catalog
   digests, a CHANGELOG entry, the fold arm, and the old digest reproduced before the new one
   is written.

Dead ends already measured, so nobody walks them again:

- **The cursor does not discriminate.** Re-arming at the `contentHead` the surface reports is a
  documented fixed point, so a re-armed lease can legitimately carry an identical cursor. The
  arming's sequence is strictly monotone per stream; the cursor is not.
- **The rendezvous does not discriminate here.** It is equal by construction — that is the
  whole shape of the defect — so the existing filter cannot be tightened into a fix.

## Guards

- The red, already written and **not yet run**: a capture from before the wake must not burn
  the lease armed after it. Its fixture appends both arms *before* the recorder is called,
  which is the out-of-window slice; writing the in-window variant instead would produce a test
  that passes because of #71 and proves nothing about this defect.
- The race test's per-round assertion upgraded to identity rather than counts, and its fixture
  given a distinct session per round — today it reuses one session, so an unconsumed lease is
  silently overwritten by the next round's arm and the fixture erases its own evidence.

Both halves get sabotages observed individually, per the house rule: an unrun assertion is not
a guard, it is a claim about a guard.

## Status of the claims above

The burn itself is **reasoned from source and not yet observed** — the red is typed and has
never been executed. Running it is step one, and if it comes back green the defect does not
exist as described and this issue closes without a fix rather than acquiring one.

Everything under "why this is not a coverage gap" is measured, with raw per-guard lists and
invocations recorded in `.factory/c-agent-wake-flakes-study.md`.
