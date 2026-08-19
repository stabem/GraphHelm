Closes #74

A sleeper is rung, wakes, and re-arms — on the same rendezvous, because our agents use fixed
rendezvous ids. Then the sweep that woke it finally records, finds a live lease for that session
whose rendezvous matches its capture, and burns it. The lease it burns is the one the sleeper is
asleep on.

Nothing caught it. The filter compared session and rendezvous, and the capture carried no arming
identity at all, so every comparison available to it passed. The fold accepts it too — consuming
a live lease is legal, so there is no corrupt error and no refused replay. And `wake-wait`'s
timeout path never consults the receipt, so the sleeper is told its deadline passed while the
store's own record says `rung`. Two surfaces answer one question differently and the operator
acts on the calm one.

The precondition is not hypothetical: the archived pair store shows session `agente-a` arming
rendezvous `factory-a-1` at sequences 26, 30, 33 and 39 — four arms, one rendezvous, one session.
Precondition present; incident not observed there, because those pairs happen to be ordered.

## The fix

A lease carries `armed_at_sequence`, **derived in the fold from the envelope** and never stored
in the payload — which is what makes it work for history written before the field existed. The
capture carries a copy, and the filter compares session plus that sequence.

The rendezvous comparison is **not weakened, it is implied**: a lease is a pure function of its
arming event, so the same session at the same arming sequence is the same event, which has one
rendezvous.

`wakeLeaseConsumed.capturedArming` records the arming the sweep **captured**, not the one live
when it wrote. Replay already knows the live side; only the captured side was ever unknown to the
log. A mismatch is **recorded, never refused** — it lands in `wake_mis_burns` where the attention
predicate can reach it. Recorded but not yet surfaced: the wiring to attention is seeded.

## Sabotage ledger

Every confirmed row is red **at its own assertion, named by panic site**. That standard was
adopted mid-review, after a sabotage produced four reds that were all at an `append` and none at
a guard — a red nobody would have audited.

| sabotage | result |
|---|---|
| restore the second `next_sequence` read | `wake.rs:462:9` / `:482:9`, left 1 right 0 — CONFIRMED |
| drop the rendezvous compare | `wake.rs:580:9`, left 1 right 0 — CONFIRMED |
| recorder returns 0 early | `wake.rs:524:14`, "the rival's consumption is on record" — CONFIRMED |
| recorder burns with the wrong reason | `wake.rs:696:9`, left StaleRendezvous right Rung — CONFIRMED |
| idempotency key without the sequence | **no casualty** — SEEDED, and **not run-verified** (see below) |
| delete the recorder call, belt alone | **no casualty** (armed=15, consumed=1, green) — CONFIRMED |
| filter matches session only | `wake.rs:794:9` and `:675:9`, both left 1 right 0 — CONFIRMED |
| constant fold, hand-written fixtures | wrong two fell — **REFUTED**; instrument replaced, sites unrecoverable |
| constant fold, rebuilt fixtures | `execution_projection.rs:460:5` + two wake guards — CONFIRMED |
| rendezvous blade restored under constant fold | swap guard **green** — the masking, CONFIRMED |
| same-rendezvous control | swap guard falls, left 1 right 0 — CONFIRMED |
| fold records unconditionally | `execution_projection.rs:613:5` — CONFIRMED |
| fold's mis-burn detection removed | `execution_projection.rs:544:10` — CONFIRMED |
| `skip_serializing_if` removed | four reds, **all at `execution_projection.rs:96:40`** — the append helper, not any assertion — **REFUTED** |

Two refutations are in the ledger on purpose. One was mine: hand-written fixtures hardcoded the
discriminator that production copies off the projection, so a corrupted fold moved one side and
not the other and two guards passed for a reason production would never reproduce. That
refutation forced the fixture rebuild. One was the reviewer's: the wire-absence assertion was
predicted to fall and never executed, which is what established that the **schema** is that
property's live guard.

The last row is the exhibit for why the standard exists.

### Which rows prove they ran

A separate question from "did the sabotage kill something", and one this ledger can answer for
almost nothing, because the standard it already uses happens to settle it.

**Twelve rows are self-verifying.** Every confirmed row is red *at its own assertion, named by
panic site* — `wake.rs:462:9`, `execution_projection.rs:613:5`, and so on. A panic at a specific
assertion line is a receipt that the assertion executed. No harness that failed to start can
produce one. The standard adopted to stop vacuous reds turns out to also discharge the
run-verification burden, at zero extra cost. (The `skip_serializing_if` row is included here: its
four reds landed at `execution_projection.rs:96:40` — the wrong site, which is exactly why it is
REFUTED — but the panic site still proves the suite ran.)

**One zero carries its own control.** "delete the recorder call, belt alone — no casualty" reports
`armed=15, consumed=1, green`. Those are non-zero observations; a harness that never ran produces
no counts. Run-verified.

**One row does not, and is now marked.** "idempotency key without the sequence — no casualty" is a
bare zero with no positive observation recorded beside it. It is almost certainly fine — it ran in
the same session and same suite as its neighbours — but "almost certainly fine" is precisely the
reasoning that a zero from a dead instrument survives. It stays SEEDED and is labelled
**not run-verified** rather than silently counted as a real negative.

The general rule this pass produced: **a zero is the one result a completely dead instrument
reproduces perfectly.** Every other outcome is at least evidence that something happened, so
run-verification only ever needs to be argued for the zeros.

## Two things stated rather than implied

**A guard kept while dormant.** The wire-absence assertion cannot fail today — the schema rejects
a null at append. It is kept because that reason is exactly one edit deep, and the day the type is
relaxed it becomes a single-sabotage blade with nothing behind it. Its comment records that it is
currently redundant, the measurement proving it, and that its own sabotage needs two edits and
**has not been run**.

**Where measuring stopped.** The compound state — schema loosened *and* the skip removed — is
labelled expected-unmeasured in the changelog, with its backstop marked as a reading. Two
deliberate changes, each individually guarded, each meeting a written warning. Saying we drew the
line beats pretending it is not there.

## What cannot be known about the past

No consumption committed before this change carries the captured side, so no analysis can ever
decide whether this defect fired in history. Replay can say which lease was burned; nothing can
say which one the sweep meant to burn, and the discrepancy is the defect. A future reader must
not search the archives, fail to find it, and conclude it never happened.
