# #119 — surfacing `wake_mis_burns`

**Design only. Base: main `6193f5c`.** Every file:line re-derived against that commit. Boundary with
#88 settled with J directly (their code at `67f12d8`), not inferred.

## What I measured before designing

| claim | how checked | result |
|---|---|---|
| the field is populated | `git grep wake_mis_burns origin/main` | written once, `projection.rs:1077` (the fold) |
| it is read by nobody | same grep, production files only | **ZERO production readers.** Every other hit is a test |
| the F4 wake surface is blind to it | grep in `execution/wake.rs` | reads `wake_last_consumed` (1), `wake_mis_burns` **0** |
| #88 does not already cover it | J, verified against `wake_wait.rs` | `misBurn` emitted only in `matured()` — the **timeout** path |

So #119's premise holds at main: the fold records it faithfully and no operator surface reads it.

## THE REFRAME — and it changes the issue's severity, not its validity

**Post-#74, the serve path cannot populate this field.** The recorder's filter
(`serve/wake.rs:262-270`, mine) keeps only consumptions whose captured arming equals the live
lease's:

```rust
.is_some_and(|live| live.armed_at_sequence == lease.armed_at_sequence)
```

A stale capture is dropped, so no consumption carrying a mismatched `captured_arming` is ever
appended by serve. The fold's mis-burn arm (`projection.rs:1077`) therefore fires only for
consumptions written by **something other than the current serve**: a direct append, an older
binary, or a future bug. Confirmed by J against the same code, independently.

**This does not make #119 invalid — it makes it a different kind of issue, and the design must say
so or the surface will be misread.** What `wake_mis_burns` actually is, today:

> A faithful recorder of a consumption **the current writer cannot produce.** It is an integrity
> detector for rogue, legacy or future-buggy writers — not a live defect in the serve path.

Two consequences that drive every choice below:

1. **It should be rare.** A surface that shouts about a common event becomes noise; one that shouts
   about a should-never-happen event is exactly what attention is for.
2. **It must not read as "the wake path is broken right now."** The issue text — *"a mis-burned
   lease is recorded and invisible"* — invites that reading. The operator contract has to name it as
   *someone wrote a consumption that does not match the live lease*, which is a provenance problem,
   not a wake-timing one.

## Boundary with #88 — settled, not assumed

J's answers, each verified or explicitly taken as theirs:

- **`misBurn` is emitted only in `matured()`.** #88 covers exactly one operator: the one who called
  `wake-wait` and whose wait matured. The `Rung` reply carries nothing and reads no store.
- **Rung + mis-burn: PROVED non-gap for the serve path** (cite `serve/wake.rs:268`), reachable only
  via rogue writers. **Design nothing on the rung path** — its contract is "re-read your log", it
  deliberately does zero store reads (hot path), and the record lives in the projection where
  #119's surfaces read anyway. *Taken from J; the exclusivity follows from my own filter, which is
  why I accept it.*
- **My gap 3 collapses into gap 2.** Once the waiter has exited, the projection is the only reader
  path. One gap, not two.

**So #119 owns exactly: the operator who never called `wake-wait`, or whose waiter is gone.**

## The design

**Two surfaces, one derivation.**

**A — the attention predicate** (`core/execution/src/attention.rs`). A new
`AttentionReason::MisBurnedWakeLease { session: String }`, alongside the existing node-centric
variants and the execution-level `WedgedQuiescence` (the precedent for a reason that is not about
one node). This is the surface that makes it *unignorable* rather than merely *available*.

**B — the F4 wake status surface** (`execution/wake.rs`). It already reads `wake_last_consumed`; it
should carry the mis-burn beside it. This is the forensic surface — the operator who went looking.

**A is the one that matters.** B without A is the current situation with extra JSON: present, and
seen only by someone who already suspected.

## Grain: EXISTENCE, not history — and therefore no third walk

`wake_mis_burns` is `BTreeMap<String, WakeMisBurn>` keyed by session and **overwriting**
(`projection.rs:240`, `:1077`). J's framing of what survives that shape is the deciding input:

> the map cannot answer *every mis-burn on this execution* (last per session), but it can answer
> *has this session ever mis-burned at least once* — **existence survives overwrite; count and
> history do not.**

**The attention predicate needs existence only.** "Something wrote a consumption that does not match
the live lease" is a boolean question per session, and the answer is not improved by knowing it
happened four times. So:

- **the map suffices — zero new machinery, no log walk, no projection reshape.**
- **This deliberately declines J's coordination flag rather than answering it.** They noted that a
  walk here would make three hand-copies of one derivation (wake_wait product-side, their #118 belt
  test-side, mine) and suggested an extracted in-crate helper. **Correct concern, and the best
  response is not to become the third consumer.** The helper question stays open for whoever
  genuinely needs history.
- **The trigger that revives the walk, named in advance:** if an operator question arrives that
  needs *how many* or *which armings*, existence stops sufficing — and at that point the walk, the
  shared helper, and possibly the `Vec` reshape (fold change + digest ritual) get decided together,
  not one at a time.

**What existence-only costs, stated rather than hidden:** a session that mis-burned three times
reports the same as one that mis-burned once, and surfacing shows the LAST mis-burn's details. For a
should-never-happen integrity signal that is the right trade; for forensics it is not, and the
forensic path is B plus the raw journal, which loses nothing.

## Operator contract

The reason must answer *what do I do now*, and the honest answer is not "your wake is broken":

> **A consumption named an arming that was not the live one.** The current server cannot write that,
> so it came from somewhere else — a direct append, an older binary, or a bug. Your lease was taken
> by it. Check who is writing to this store.

That is a provenance/integrity remedy, not a wake remedy, and it is why this belongs in attention
rather than in the wake surfaces alone.

## Attention is the home — and I decided that in #74, not here

My question "is attention the right home for an integrity signal" was already answered by my own
fold doc (`projection.rs:235-238`, verified at main):

> It lives in the projection rather than only in the events **because the attention verdict is
> computed from the projection through one predicate.** A discrepancy visible only in raw events is
> one that surface structurally cannot see — **which would reproduce this defect inside its own
> detector.**

That is a stronger argument than the one I was going to use ("attention is the surface operators
actually read"), and it is load-bearing: putting the record only in events would recreate the exact
invisibility #119 exists to fix, *inside the detector*. **The widening of attention's meaning
happened when the field was placed. #119 collects a decision already made** — which also means the
right disposal is a **distinct variant**, never a reused progress-shaped one. Folding "who wrote to
this store" into "how is my execution going" would flatten it at the operator level, the same shape
the reframe above kills.

House precedent agrees independently: refusal-is-for-uninterpretable-logs routes the loudness of a
faithfully-recorded-wrong-action to the channel that already exists — attention, by name.

## The rate limit is bounded by the fold, not by hope

Existence-per-session caps attention at one reason per session. **A rogue writer cannot escape that
cap even deliberately**, and the proof is in the fold (`projection.rs:1059-1062`, verified):

```rust
let Some(burned) = projection.wake_leases.remove(payload.session_id.as_str()) else {
    return Err(ReplayError::Corrupt);   // history cannot burn a lease that was never armed
};
```

A mis-burn requires a session with a **live lease**. A consume without one is `Corrupt` — it breaks
the stream and trips the existing refusal machinery, which is louder than attention. So a loop
either targets genuinely armed sessions (bounded by session count, not event count) or destroys its
own cover. *Argument J's; verified against the code by me.*

## THE LATCH — and why it stays one

**Nothing in the fold ever removes a `wake_mis_burns` entry.** The signal, once up, is up forever.
J raised this unprompted and it is the gap in the first draft: *a latch with a documented release is
a signal; a latch without one is future noise.*

**Decision: the latch is correct here, and the reason is not that clearing is hard.** It is that the
compromise is permanent. A foreign writer touched this store; the execution's provenance is in
question from that moment and does not become un-questioned by anyone acknowledging it. An execution
that looked compromised and now looks clean is worse than one that stays flagged.

The wallpaper objection is real but applies to signals that fire routinely. This one **cannot fire
from the serve path at all** (see the reframe), so its expected lifetime rate is zero; a
never-firing alarm does not train blindness, and the first firing is meant to be an event someone
investigates rather than clears.

**The release, named so it is not discovered under pressure:** there is deliberately no in-band
clear in this design. If one is ever needed, it must be an **acknowledgement EVENT** the fold reads
— not a mutation — so the acknowledgement is itself auditable and the store keeps saying what
happened. The `CalmedByAmendment` verdict is the house precedent for a recorded act suppressing a
reason. **Trigger for building it: the first time an operator legitimately needs a flagged
execution to read clean again.** Not before — building a clearing mechanism for an alarm that has
never fired is machinery designed against a guess.

## Also on record, not designed

**`doctor` is the other natural reader** — it is the integrity surface by charter, and existence is
exactly what it would want. Noted to keep the door open; deliberately not designed here, because
#119's weight is the operator who is not looking, and `doctor` is used by one who is.

## Open, for the reviewer

1. **`{ session: String }` or payload-free?** The session is the actionable identifier; the
   `WedgedQuiescence` precedent is payload-free. Session-carrying, unless the reviewer sees a
   cardinality problem I do not.
2. **Does the latch decision survive contact with a reviewer who has run an on-call rotation?** My
   argument is that permanent compromise deserves a permanent flag. Someone who has lived with
   un-clearable alarms may weigh the wallpaper risk higher than I do, and that is a judgement about
   operators rather than about code.

## Sabotage plan (sketch, to be sealed before any run)

- Remove the mis-burn arm from the fold → the attention reason must vanish. (Proves the surface
  reads the fold, not a re-derivation.)
- A consumption whose `captured_arming` MATCHES → no reason. (The positive control's mirror: proves
  the reason is about mismatch, not about consumption.)
- **The presence member, per the absence-guard rule:** an execution with a mis-burn must produce the
  reason. Without it, "no reason raised" passes trivially if the predicate is never reached.
- A second mis-burn on the same session → still exactly one reason. (Pins the existence grain, and
  fails loudly if someone later "improves" it into a count the map cannot support.)
