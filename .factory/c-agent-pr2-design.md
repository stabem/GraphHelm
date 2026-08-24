> **PROVENANCE: this document became the design behind PR 2, which closed #74.**
> Provenance is the strong relation and the one grep cannot see: the derived artefact carries the
> content and never the source's name, so only the author can record it. Established against
> `gh issue list --state all` and the PRs, not from memory.

# PR 2 design: the arming-identity discriminator

Issue #74. Written before any code, so the shape can be attacked while attacking it is cheap.

## The discriminator is DERIVED, never stored

`WakeLeaseState` gains `armed_at_sequence`, set in the fold from **the envelope's own sequence**
— `event.sequence`, the same value the sibling arm already records into the consumption receipt.
`DueLease` carries a copy, read off the projection phase 1 already holds.

The filter then compares session, rendezvous **and arming sequence**.

Why derived rather than stored on the `WakeLease` payload, which was the obvious first shape:

- **It deletes a question instead of answering it.** A stored field is absent on every lease
  armed before the change, which forces a live-transition decision — fail closed and drop a
  legitimate consumption, or fall back to the old comparison and keep the blindness for those
  leases' lifetime. Derived, the value is recomputed from the log on every replay, so a lease
  armed years ago has one. The transition case does not exist.
- **No schema change on the arm side.** The envelope already carries the sequence.
- **O(1) at the filter.** B's improvement on the version N and I first discussed: no history
  scan in either phase, because the projection already holds it. The older scan-based text
  should not be resurrected from the seed.

## Why it works, and the sentence that is a canary

The fold's `insert` on `wake_leases` **overwrites**. So a re-arm replaces the lease and
`armed_at_sequence` becomes the new arming's sequence, while a capture held from before the
re-arm still carries the old one. They mismatch by construction.

**The discriminator works precisely because re-arm is an overwrite.** If re-arm semantics ever
become merge-in-place, this filter is what breaks — and it breaks silently, because a merged
lease would keep the old sequence and the stale capture would match again. That sentence belongs
in the code beside the comparison, not only here.

## Attacked before written

| case | behaviour | right? |
|---|---|---|
| re-arm, same session, same rendezvous, same cursor | sequences differ, capture dropped | yes — this is the defect |
| no re-arm; capture and live lease are one arming | sequences equal, consumption recorded | yes |
| lease consumed legitimately, then re-armed, stale capture arrives | old sequence vs new, dropped | yes |
| two sweeps capture the *same* arming | both match; the first burns it, the second finds no live lease | yes — and PR 1's pin catches it too |
| re-arm lands between phase 1 and phase 3 | phase 3's live lease carries the new sequence, capture carries the old, dropped | yes — the in-window slice, already benign post-PR-1 |

No case found where a legitimate consumption is dropped that PR 1's own benign-drop chain does
not already cover.

## The rendezvous comparison comes OUT, and not merely because it is redundant

The filter becomes **session + arming sequence**. The rendezvous stays on `DueLease` — phase 2
still rings by it — but it is no longer a blade.

B's argument, verified rather than accepted: a lease's state is a pure function of its arm
event, and sequences are unique per stream, so a live lease for session S and a captured
`armed_at_sequence` matching it can only have come from the same arm event — which has one
rendezvous. Same arming implies same rendezvous. The rendezvous line can therefore never be the
deciding blade, so post-PR-2 the sabotage "drop the rendezvous compare, keep the sequence" fells
**nothing**. An unfellable check is decoration, and this week killed that three times.

**The stronger reason, which the redundancy argument misses: keeping it would MASK a sabotage.**

PR 1's guard arms `rdv-old`, re-arms `rdv-new`, and feeds the recorder the stale `rdv-old`
capture. Now break the fold so `armed_at_sequence` is a constant:

- with the rendezvous line kept — the sequences match (both constant), the rendezvous still
  differs, the capture is dropped, and the guard stays **GREEN**. The broken discriminator is
  invisible in exactly the guard that should see it.
- with the rendezvous line removed — the constant sequences match, nothing else differs, the
  capture is recorded, and the guard **FALLS**.

So removing it converts an existing behavioural guard into a detector for the new
discriminator's own failure. Belt-and-braces would have bought a second blade at the cost of
blinding the first one's sabotage.

### The general form, because it will recur

**A redundant blade does not merely buy nothing — it blinds the sabotage of the blade that
matters.** The guard stays green under exactly the corruption it should surface, so
belt-and-braces converts *observed* safety into *unobserved* dependence. Removing the redundant
blade is what turns an existing behavioural guard into a free detector for the primary's failure.

B's placement of it: this is the self-healing-hides-failure lesson one level up. Redundancy is
self-healing at the check layer — the second blade repairs the first blade's failure before any
oracle can see it, exactly as a retry repairs a fault before an end-state oracle can see it.

That guard stays and stays green post-PR-2, on the sequence blade instead of the rendezvous one.
It pins BEHAVIOUR — stale capture records nothing, replacement survives — not the mechanism that
delivers it, which is what behavioural pins are for.

The commit must say the subsumption out loud: **the rendezvous comparison is not weakened, it is
implied — same arming means same rendezvous, and the sequence is the finer grain.** A reader
diffing PR 1 against PR 2 otherwise sees a protection deleted.

## What half 2 is FOR, stated so a later simplification deletes the right half

With the discriminator derived, the `WakeLeaseConsumed` field is **not load-bearing for the
recorder**. It is:

- the **receipt** — the glance can say "burned the arming at seq N";
- a **forward-only fold check** for events written after the change.

The recorder would work without it. A reader who concludes otherwise will delete the wrong half
when simplifying. Committed history keeps its own rule: omit when absent, never null, and the
fold stays permissive for pre-change consumptions.

## Half 2 must record the CAPTURED arming, not the live one

This is the detail that is easy to get backwards, and getting it backwards makes the field
useless while looking correct.

The defect is a mismatch between two things: the arming the sweep **captured**, and the arming
that is **live** when it records. Replay already knows the live side — the fold rebuilds it. It
has never known the captured side, because the consumption event carries execution, session and
reason and nothing else.

So `WakeLeaseConsumed` must carry **the arming sequence the sweep captured**. Then the fold can
compare it against the lease it is about to burn and see a mismatch. If it instead recorded the
live lease's sequence, the comparison would be against itself — tautological, always equal, and
a check that cannot fail, which is the decoration this design just removed elsewhere.

### The epistemic limit this puts on existing history

Because the captured side was never recorded, **no future analysis can determine whether this
defect ever fired in committed history.** Replay can say which lease was burned; nothing can say
which one the sweep meant to burn. The discrepancy between them is the defect, and only one side
is on the log.

That is why the #74 evidence stops at "precondition present, incident not observed" and can
never be strengthened for the past — not because nobody has looked hard enough, but because the
data cannot answer it. Going forward, the field is what makes the question answerable at all.

Stated beside M's standing finding, which it compounds: the oracle does not check, **and** the
data would not support the check even if the oracle wanted to. Silence at the oracle, absence at
the payload, on the same defect.

## The fold does NOT refuse on a mismatch — proposed, against the reviewer's lean

The question: post-PR-2, when a consumption names a captured arming and the live lease carries a
different one, what does the fold do? Refuse the replay, or record it and surface it?

**Proposal: record it faithfully; do not refuse.** The reasoning turns on a distinction the
options collapse.

### An impossible log and a faithfully-recorded mistake are not the same thing

The fold's existing refusal fires on a consumption with **no live lease**. That log is
*uninterpretable* — it describes a state that cannot have existed, so replay genuinely cannot
reconstruct it, and refusing is the only honest answer.

A captured-versus-live mismatch is different in kind. The log is internally consistent and fully
interpretable: a lease was armed, another was armed after it, and a sweep burned the wrong one.
That is **a wrong action faithfully recorded**, not a corrupt record. Replay can reconstruct it
exactly.

A product whose thesis is that history reproduces should reproduce its mistakes too. Refusing
here would mean the log becomes unreadable *because it recorded something bad* — which is a
different failure from the log being impossible, and conflating them costs the ability to
examine the very incident we would most want to examine.

### The subsystem's own rule points the same way

`serve/wake.rs` states it directly: **a wake failure never fails the route that triggered it.**
The doorbell is an accelerator, never a correction. Bricking a stream's replay over wake
bookkeeping would enforce the opposite at the storage layer — a wake-side mistake taking down
every surface that reads the execution, including the ones an operator needs to diagnose it.

### But quiet is not acceptable either, and that is the real objection

"Surface-only" as usually meant — record a flag, wait for someone to read it — reproduces the
defect's own shape inside the detector. That objection is correct and it is why refusal is
tempting.

The answer is that **loudness belongs where loudness already lives**: the attention verdict. A
lease burned against the wrong arming is precisely a "the operator must look" condition, and
this milestone built a surface whose entire job is to say so. Wiring it there makes the
detection loud without making the system unusable.

Whether that wiring lands in this PR or is seeded is a scope call for the reviewer and the
orchestrator. The fold decision does not depend on the answer: **do not refuse, either way.**

### Where the mismatch lands — named before writing, per the reviewer's requirement

It goes in **projection state**, not merely into an event a reader could notice. Attention reads
the projection through one predicate — no surface recomputes the verdict — so a discrepancy that
only exists in the raw events is a discrepancy attention structurally cannot see, which would
reproduce the defect inside its own detector.

Proposed field on `ExecutionProjection`, following the existing `wake_leases` /
`wake_last_consumed` naming and their per-session last-wins shape:

    /// Consumptions that named an arming other than the one that was live.
    ///
    /// Keyed by session because the session is who is stranded: the burned lease is gone, so
    /// that session has no live lease and no later append can ring it.
    ///
    /// An entry here NEVER implies replay refused. This map is the record-don't-refuse ruling
    /// made visible — the log stays legal and readable, which is exactly what makes this
    /// failure silent and why the loud channel has to be attention rather than the fold. A
    /// reader who finds an entry must not infer the stream was ever unreadable.
    pub wake_mis_burns: BTreeMap<String, WakeMisBurn>,

    pub struct WakeMisBurn {
        /// Sequence of the consumption that did it.
        pub at_sequence: u64,
        /// The arming the sweep captured.
        pub captured_arming: u64,
        /// The arming that was actually live and got burned.
        pub live_arming: u64,
    }

Both armings are kept rather than a bare flag. "Something was wrong here" is not actionable; a
reader needs to see that a capture from arming N burned arming M, because that pair is the whole
diagnosis and reconstructing it from raw events is the work the predicate must not have to do.

Keyed by session because that is who is stranded: the burned lease is gone, so that session has
no live lease and no later append can ring it. Last-wins matches the two neighbouring maps.

**If the attention wiring is seeded rather than landed here**, the commit says the condition is
recorded but not yet surfaced, and the seed names `wake_mis_burns` explicitly — so whoever wires
it connects a pipe that already exists instead of inventing one.

### The frozen projection digest, checked rather than discovered at gate time

`tools/acceptance-map` freezes an `expected_projection_digest` — sha256 over the replayed
projection's canonical JSON — and `grounded.rs` fails if the current build cannot reproduce it.
So ANY field added to a projection struct can break a frozen demonstration, and the codebase
already says so: `declared_form` carries a note that its `skip_serializing_if` is "load-bearing,
not tidiness", precisely because a field emitting `null` for older histories would change all
their digests.

`armed_at_sequence` cannot use that escape. It is DERIVED at replay, so every lease gets a value
on every replay including the oldest history — there is no "absent" state to skip.

Checked instead of assumed: the frozen demonstration
(`docs/acceptance/demos/m06-fixture-journey/events/journal.jsonl`) contains **zero** `wake_lease`
and zero `wake_lease_consumed` events. Its `wake_leases` map is empty, `WakeLeaseState` is never
serialized in it, and the new field therefore cannot appear in that canonical JSON. **The digest
is unchanged and no re-record is needed.**

FIXTURE-DEPENDENT, exactly like the assertion-ordering confirmation: if a future demonstration
ever records a wake lease, adding fields to `WakeLeaseState` starts changing its digest and the
recording has to be regenerated. Noted here because the next person adding a field will not
think to look.

One boundary worth stating: a projection read back from storage rather than replayed would carry
`0` for pre-change rows, and a capture with a real sequence would then never match. The recorder
does not take that path — it replays from the log every time — but a future caller that reads a
stored projection and feeds it to this filter would silently drop every consumption.

### The masking experiment, and the caveat that makes it reusable

Three runs, same fixture, same constant-fold sabotage, one variable moved at a time:

| run | blade | capture rendezvous | swap guard |
|---|---|---|---|
| 1 | sequence only | differs | falls |
| 2 | sequence + rendezvous | differs | **green — masked** |
| control | sequence + rendezvous | **matches** | falls |

Run 2 against run 1 varies the blade with the rendezvous held; run 2 against the control varies
the rendezvous with the blade held. Together they show the green needs BOTH the blade present AND
the rendezvous differing — so it is caused by the rendezvous comparison and not by something else
the blade does.

**THE CONTROL IS SINGLE-VARIABLE ONLY BECAUSE THE FOLD IS CONSTANT.** It changes the capture's
rendezvous by taking the capture after the re-arm, and taking it later normally changes
`armed_at_sequence` too. Under the constant fold both are the constant either way, so the two
collapse and rendezvous is genuinely the only live variable. Lift this control into an
intact-fold context and it becomes a two-variable experiment that looks identical to the one that
worked.

I reasoned "the rendezvous is the only thing that differs" without noticing that only the
sabotage made that true. M caught it. The next person will make the same reading for the same
reason.

Two further limits, so the experiment is described accurately rather than generously:

- **The fourth cell is unrun**: single blade with the rendezvous matching. It predicts a trivial
  fall, and it is the only cell that could reveal an interaction nobody has posited. Not needed
  for the claim; named so nobody calls this a complete 2x2.
- **Scope is "under a broken fold"**, since all three runs use the constant. That is the only
  condition where masking is meaningful, but it belongs in the commit rather than being inferred
  later.

And what the proven claim is FOR, which is smaller than the work it took: the ruling already
stands on subsumption alone, so this decides nothing. Its only consumer is the commit prose —
telling the accurate, partial story instead of the general one I first argued. That is a real
consumer and a small one, and naming it stops the proof being cited later as load-bearing for a
decision it did not make.

### What this owes

A named test for the mismatch path — the fold records it, replay still succeeds, and the
condition is visible rather than swallowed — plus a sabotage in which the detection is removed
and that test falls. A fold check nobody has watched fire is the tautology's second cousin, and
this decision is not exempt from the rule that produced it.

Permissive when either side lacks the field, unchanged: committed history has no captured side
at all.

## Guards owed

- The red: a capture from before the wake must not burn the lease armed after it —
  out-of-window slice, both arms appended before the recorder runs.
- Pre-change arms discriminate correctly. This is **trivially true by reading** under the derived
  shape, which is exactly why it gets a guard and a sabotage rather than a pass.
  Sabotage (B's): have the fold copy a constant, or the payload cursor, into `armed_at_sequence`
  instead of the envelope sequence — the guard must fall.
- Sequence-grain assertions throughout, per the week's measured finding that count-grain and
  reason-grain are both blind.
- The belt's per-round identity upgrade, extending its oracle rather than replacing it, with
  sabotage evidence that it still catches what the old belt caught.
