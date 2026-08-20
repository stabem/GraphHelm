# #90 and #94: one operator need, or two? — joint scoping note

**For the owner report. ANALYSIS ONLY — nothing here scopes work, and neither issue should be
built until the owner weighs in.** Written by D at main `0fb0e66`, 2026-08-20. Every code claim
below was read at that commit; line references are against it.

---

## The answer, up front

**They are ONE need at the level of intent and TWO defects at the level of mechanism, and the
mechanisms fail in opposite ways.** Neither falls out of the other. Building either one leaves the
other exactly as unmet as it is today.

The shared intent: *let the operator express something the lifecycle table already permits.* Both
issues are cases where the state machine allows a state that no operator can reach at the moment
they need it.

The divergence, which is the decisive finding:

| | #90 — start held | #94 — waive / skip |
|---|---|---|
| Does an emitter exist? | **YES** — `pause.rs:121` produces `Paused` | **NO** — nothing anywhere produces `Waived` or `Skipped` |
| What is actually broken | **SEQUENCING**: the emitter cannot be reached *before* the drive | **REACHABILITY**: the outcome has no surface at all |
| Repair shape | withhold a call that already exists | add a command that does not exist |
| New lifecycle state needed? | **No** (established below) | No |
| New event kind needed? | **No** | No — `NodeOutcomeRecorded` already carries both |

A missing ordering and a missing emitter are not the same defect, and a fix for one is not
evidence about the other.

---

## Why they look like one need

Both trace to the same shape, and it is the shape #93 named: **the lifecycle table says what is
LEGAL; the surfaces decide what is PRODUCIBLE, and the two have drifted apart.**

- `transition.rs:111-112` permits `Waived`/`Skipped` from any state. No command emits them.
- `Paused` is emitted by `pause.rs:121`, and `pause` accepts a status of `None | Running`
  (`pause.rs:69-72`) — so it is legal on a freshly started execution. But `start` drives to
  quiescence before returning (`start.rs:97`), so by the time an operator could call `pause`, the
  work has already run. The verb exists; the window does not.

An owner reading only the state machine would reasonably conclude both capabilities exist. That is
the same reading error that let `userOverrideAllowed` (#93) look like a feature, and it is why
these two belong in one note even though they need different repairs.

---

## Does either fall out of the other? Both directions, answered

**Does exposing waive/skip give you start-held?** No. `Waived` and `Skipped` are *terminal per
node* — they satisfy dependents (`ready.rs:27`) so downstream work proceeds. Waiving every node to
prevent a run would mark the whole graph as deliberately-not-done, which is the opposite of
"staged, to be run later". The intents are contradictory, not nested.

**Does start-held give you waive/skip?** No. Holding suppresses dispatch for the whole execution;
waiving releases *one node's dependents* without running it. Holding cannot express "proceed past
this obligation" at all.

**Do they interact?** Yes, in one direction worth recording: a held execution is the natural place
to *use* waive/skip — stage the run, waive the obligations you have decided to bypass, then
release. If both ship, that sequence is the coherent operator story. That is an argument about
ORDER OF DELIVERY, not about scope merging.

---

## #90 needs no new lifecycle state — and its own "decision needed first" is answerable by reading

#90 says: *"whether 'held' is a distinct lifecycle state or simply 'started and immediately
paused'. That is a product question."* Partly. The product question is what to CALL it; the
mechanical question is already settled by the code, and the answer is that **the existing pieces
compose without a new state**:

1. `start` appends `ExecutionStarted`, then drives. A held start would append `ExecutionStarted`
   **and** `ExecutionPaused`, and simply not call `drive_to_quiescence`.
2. Nodes are never touched, so they read `Draft` — which is honest: `Draft` means *not yet
   approved*, and nothing has been.
3. `resume` then works unmodified. `resume_preconditions` requires status `Paused` ✓. There are no
   `Running` nodes, so `recovery_plan` is empty ✓. There are no `Paused` NODES, so the redispatch
   loop is a no-op — and that is fine, because the drive's own `approve_untouched`
   (`driver.rs:283`) readies every `Draft` node at the top of each pass, and dispatch proceeds
   normally.

So "held" is mechanically *started-and-paused-without-driving*. **No new state, no new event kind,
no change to `resume`.** The naming decision remains the owner's, but it is now a naming decision
rather than an architectural one, and it should not be priced as the latter.

**One property worth the owner knowing, because it is easy to fear and is in fact correct:** a held
execution reports `CanSleep`. Status is `Paused`, so `claims_running` is false and no wedge fires;
`Draft` nodes have no judgeable silence by design. An execution the operator deliberately parked
does not nag. That is the intended behaviour, not a gap — but it does mean a held execution is
silent, so if the owner wants a *reminder* that something is staged, that is a separate ask and
should be named separately rather than assumed to come free.

---

## What each exit costs

**#94 — expose waive/skip.** Two commands (plus HTTP routes) emitting `NodeOutcome::Waived` /
`Skipped` with the operator's actor and a required reason. The transition, the projection fold and
the readiness rule already handle both — this is surface work, not semantics. The real cost is not
code: it is deciding what a waiver *obliges* (does it need a reason string? evidence? does it
appear in the completion record as distinct from success?), because `satisfies_dependents` treats
`Waived` exactly like `Succeeded` for scheduling while the two mean very different things to a
reader of the finished stream.

**#94 — the alternative exit: retract the claim.** Remove both outcomes and rewrite
`satisfies_dependents` and D-019's expression to match what the product does. Cheaper in code,
expensive in meaning: it changes a decision record, and it removes the only sanctioned way an owner
could ever proceed past an obligation.

**#90 — build it.** A flag on `start` and its route; the composition above; guards that a held
start records both events and dispatches nothing, and that `resume` releases it. Small.

**#90 — the alternative exit: decline it.** The operator keeps racing the driver. #90's own
evidence is that this cost the M09 rehearsal two full rounds of pause/approve/resume — six mutation
calls where three would do. Note that **#80 has already removed part of that cost** (the
edge-blindness that made it *two* rounds rather than one); the reaction race itself remains.

---

## D-019 impact

D-019 grants the owner sovereignty over the run. Today that sovereignty is expressed in
`ready.rs:27`'s comment — *"an owner waiving an obligation or skipping a phase is exercising the
sovereignty D-019 grants"* — and **that sentence is currently false of the product**: the lever
exists in the rule and nowhere a human can reach. #94 either makes the decision true or requires
the decision's text to change. #90 does not touch D-019; withholding dispatch is a scheduling
control, not a sovereignty claim.

**This is the sharpest single line for the owner report:** a recorded decision cites a mechanism
that no surface provides.

---

## Ordering constraints already on the record

- **#93's "delete the field" exit is INVALID until #94's "expose waive/skip" exit ships.** Deleting
  `userOverrideAllowed` first would leave operators with no route at all past an obligation they
  have decided to bypass — strictly worse than the accidental override #80 removed. Recorded in
  both issue bodies.
- **#90 is unblocked by either.** It depends on nothing here.
- If both #90 and #94 ship, deliver #94 first or together: a held execution is where waive/skip is
  most useful, and shipping hold alone gives the operator a parked run they still cannot unblock
  except by making every node succeed.

---

## What the owner is actually deciding

1. **Is proceeding-past-an-obligation a capability this product offers?** Yes → #94 Exit A, and
   D-019's expression becomes true. No → #94 Exit B, and D-019's text must change. *There is no
   third option that leaves the current state honest.*
2. **Is staging-without-running a capability worth a verb?** Yes → #90 is small and needs no new
   state. No → the reaction race stays, at a measured cost of roughly double the mutation calls for
   staged-release stories.
3. **Only if (1) is yes:** what a waiver obliges — reason, evidence, and how it reads in a finished
   stream against a genuine success.

---

## What this note does NOT establish

- **I did not measure how often either need arises.** #90 cites one rehearsal; #94 cites zero
  operator reports, because the capability has never existed to be missed. Neither has a frequency
  and I am not going to invent one.
- ~~**I did not verify the HTTP surfaces**~~ — **CLOSED, and the result is stronger than the
  claim it replaces.** This was filed as the one hole to close first if the owner leans toward
  Exit A, so it was closed immediately rather than left for them to trip over.

  **The conclusion holds. My FIRST ENUMERATION OF IT DID NOT, and the correction is recorded here
  rather than quietly fixed.** The initial sweep matched `NodeOutcome::Waived|Skipped` and
  `Outcome::Waived|Skipped` and reported "six occurrences, every one a TEST". That pattern is
  CONSTRUCTOR-QUALIFIED and misses aliased imports — this tree holds three `use NodeOutcome as O`
  sites, one of them PRODUCTION (`core/gateway/src/taxonomy.rs:88`). Re-swept on the bare
  `O::Waived|O::Skipped` form: **two further occurrences, both production** — `transition.rs:111-112`.
  The honest count is EIGHT, not six, and two are not tests.

  They are still not emitters. `transition.rs:111-112` are the transition TABLE's match arms, which
  CONSUME a waive outcome to decide the next state; nothing constructs one. The aliased production
  site was read directly: `taxonomy.rs` maps gateway errors to `O::NeedsCapacity`,
  `O::RetryableFailure`, `O::TerminalFailure`, `O::Cancelled` — no waive, no skip. And the HTTP
  path's driver was read rather than inferred: `core/runtime` emits `Approved`, `Started`,
  `Interrupted` (`driver.rs:464/525/538/599`) and `Succeeded`/`RetryableFailure`/`TerminalFailure`
  (`executor.rs`).

  **So #94's premise holds TREE-WIDE: no production emitter in any crate.** But note how it nearly
  did not: the first sweep was right BY LUCK OF NAMING rather than by coverage, and the reviewer
  independently hit the SAME constructor-qualified blind spot in their own enumeration. **Two of
  the three instruments behind this "triple-sourced" negative shared one blind spot, so the
  independence is weaker than the count suggests** — three greps that all qualify the constructor
  are one instrument run three times, not three instruments. What makes the negative trustworthy is
  the bare-pattern re-sweep plus the two direct file reads, not the number of searches.
- **I did not price the guard suites** for either, because neither is scoped for building.
- **I am the author of the semantics behind #79/#80 and of the storm figures cited around this
  area.** Nothing here decides anything — that is deliberate, and it is why authorship is safe in
  this note where it was a liability for #89.
