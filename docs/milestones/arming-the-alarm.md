# Milestone 09 — Arming the Alarm

**The user sentence, end to end:** *"When I arm, I declare how long silence may last on this
node; the doorbell holds me to that bound — refuses one nobody could live to see, lets me
shorten or lengthen it without losing the wait already in flight, and answers the same way
whether I ask from the CLI or over MCP."* M08 made the glance say *when*. M09 makes the wait
*ring* instead of only being pollable, closes the CLI/MCP parity gap M08 opened, and — because
stabilizing the wake path meant staring at its tests — surfaced how much of the family's own
test coverage had been certifying less than it appeared to.

Two artifacts carry the M09 name and are not the same thing. **PR #70**, merged to `main` as
`efd85d0`, closed seed 3 from `m09-seeds.md` ("a run wedged in the queue never wakes anyone") on
its own, before the rest of this work began. **The `issue-m09-arming-the-alarm` branch** carries
seed 4 ("the doorbell cannot ring for silence") plus the flake-stabilization and audit work that
followed from touching the same file. Several more fixes landed on `main` directly, main-based
rather than part of this branch, discovered while the wake path was under scrutiny.

## What landed

| Part | Shape | Status |
|---|---|---|
| Seed 3 — silence keyed on reach, not run state | `attention` no longer filters `state == Running` only; a node retried into `Queued` after a declared bound now counts | Merged, `main`, `efd85d0` (#70) |
| Seed 4 — arming declares a bound, the doorbell honors it | A bound nobody could live to see is refused, not answered with a date (a trillion-second horizon previously overflowed into a year-33715 date); `wake-wait` reads the deadline from its own lease instead of a caller-supplied `--timeout`; the MCP half of `wake_wait` now matches the CLI half; shortening a re-armed horizon is accepted and named, never silently late; reads take a shared lock instead of the same exclusive lock as writes | Pushed, `origin/issue-m09-arming-the-alarm`, not yet on `main` — 5 commits, `abd2f2c..53d212d` |
| Flake #3, `concurrent_sweeps_never_double_consume_a_lease` | The validate/`next_sequence` window closed by pinning the consume's sequence from the read that judged the lease, not a second store read — the seam survives the fix and stays sabotage-testable | Pushed as `aac0d67`, and its follow-on discriminator (issue #74) as `d10916b` — both on the branch, not yet `main`. The `d10916b` push landed on a RED Postgres-adapter gate stage, by explicit owner decision (below) |
| Flake #2, `a_sleeper_wakes_on_a_peer_append_with_zero_requests_in_the_window` | A condition-wait replaces a timing-dependent immediate read | Pushed as `576e553` (PR #73), on the branch, not yet `main` |
| Flake #1, `the_storm_holds_under_eight_concurrent_agents` | Instrumented headroom measurement; no fix | Still OPEN — see "The storm" below |
| Cited evidence must open, not just hash | Two committed acceptance stores answered `GHE005_INTEGRITY_FAILURE` on open for their entire committed life while every checksum stayed green; a new test opens and replays every committed store by directory | Merged, `main`, `df5e431` (#77, closes #75) |
| Store-layout recovery | `.tmp/`/`active/` recoverable on open (transient workspace); `blobs/` stays strict (a blob is a tracked file; its absence means evidence is gone) | Merged, `main`, `f2efb94` (#84, closes #76) |
| A CLI that can print a schema digest | `schema digest` prints the canonical digest a ritual previously needed a throwaway test to get | Merged, `main`, `326799b` (#85, closes #78) — raised and resolved within this milestone |
| The second judge story | Ran paid; coverage goal met, narrative goal failed on real product defects the story surfaced | See "The paid judge run" below; archived `docs/acceptance/m09-judge-run-2026-08-19/` |

## The measurement that outranks the rest: the belt reports green with 14 of 15 consumptions missing

Stabilizing flake #3 meant reading its whole guard family, and the reading turned into a
measurement. `concurrent_sweeps_never_double_consume_a_lease` — the family's flagship, and
flake #3's own named guard, not a separate test — asserts `consumed <= armed` across fifteen
barrier-synchronized rounds. Two independent sabotages pass it clean: deleting the recorder's
own consumption call reports `test result: ok. 1 passed; 0 failed`, and an instrument that
prints the raw count before the assertion shows `armed=15 consumed=1` — still green.

Why, by construction and not by luck: with zero consumptions written, `consumed <= armed` holds
for any `armed`, and a component that writes nothing cannot write an illegal event, so
`ok == true` holds every round. The belt asserts "no double consume" and never asserts the
consumptions happened at all — the owner's own assert-at-the-finest-grain rule, measured
against the family's flagship test rather than argued for it.

**What this does not mean, stated as precisely as the finding itself:** the belt has *never*
carried evidence for the race window it exists to guard — H measured it 0/10 isolated and 0/3
in-suite before the fix, matching the fix's own commit body ("the stochastic belt test cannot
reach this window: the base measurement reproduced it 0 times in 13"), and the guard's
introducing commit already documented the timing as too narrow to reproduce the race under
test. **What actually certified flake #3's fix** was never this guard — it was the deterministic
seam test (red before the fix, green after) plus five sabotages each independently felling one
named guard. The belt sharing the flake's name is not the same as being its evidence. What is
dented by this measurement is the guard's own credibility, not the landing: `aac0d67` remains
evidenced by instruments this finding does not touch. The guard's own upgrade — per-round
identity, a distinct session per round, since today's fixture erases its own evidence — is
tracked as issue #74's oracle-fix half, separate from the discriminator half that already
landed.

Twenty wake guards were graded by oracle grain — not whether they pass, but what a pass is
capable of ruling out: 2 legality-only, 12 receipt-grain, 6 mixed (receipt-grain in places, but
the headline property resting on a legality check or a timeout-negative). Four root causes
recur across the family: a type-string helper that discards event payload, blinding three
guards to reason/session/count; three guards asserting something did *not* happen with no
positive control that the mechanism was alive to make it happen; a recorder function whose
return type collapses one real decision and eleven unrelated failure paths into the same `0`;
and a consumption event that carries no rendezvous or armed sequence, so the journal cannot
answer "which arming did this burn?"

The self-repair rule this family produced: an end-state oracle measures the REPAIRER, not the
fault, whenever repair latency is shorter than the observation window — so the fix is not
"avoid end-state oracles," it is "observe at a grain finer than the repair latency." And a third
state joins the guard vocabulary alongside enforced-and-declared and unenforced:
**enforced-and-undeclared** — a wall of unrelated red pointing nowhere. Measured instance: the
wake recorder's honest count depends on a schema field covering a sequence number, stated
nowhere; blinding that field fells seven tests, five of them pre-existing and none of them
having the property as their actual subject. The person who broke it on purpose could not
derive the causal chain from source alone — an accidental maintainer break has no chance.

## Six claims this milestone made and then killed

Recorded because a close doc that only reports its wins teaches the wrong lesson, and because
these are the sentences well-written enough to survive being quoted later if nobody corrects
them here.

| dead claim | what replaced it |
|---|---|
| "Nothing pins divergence-by-sequence-alone" | Enforced and undeclared — true of the names, false of the coverage |
| "The #55 chain has no link that fails when the recorder dies" | One link fails when the recorder is DEAD and asserts a COUNT — so no link fails when the recorder is WRONG |
| "No guard drives two consumptions for one session" | The belt does, fourteen times — a detection gap, not a coverage gap |
| "The recorder's count is honest by accident" | Honest because replay is restricted to where "on record" means "written by us" — undeclared, not accidental |
| "Journal count 1 confirms the mechanism" | Confirms incidence only — conflict and replay produce identical journals |
| Sleeper fails "roughly 3 in 4" (`m09-seeds.md` seed 9) | Uncitable, not disproven — the figure named no invocation, N, or base. H's measured 12/13 (9/10 isolated, 3/3 in-suite) carries all three and stands in its place |

The general form: reading an assertion tells you what it says and never what it can see, and
that gap is invisible from the inside every time.

## The storm: less solved than it reads

Flake #1, `the_storm_holds_under_eight_concurrent_agents`, closes this milestone still open.

**The central finding is that the same code produced two different failure rates on the same
machine.** A same-disk paired re-baseline — ten runs at the pre-M09 commit, ten runs at the
current tip, three minutes apart, identical free disk on every row — found ZERO failures at
both, against an earlier measured rate of 4/10 isolated, 2/3 in-suite (roughly 50%, loosely
bounded: the sample was too small to have killed a true rate anywhere from 12% to 74%, so treat
"50%" as a center point, not a confirmed rate). That result **exonerates the code** — whatever
produced the earlier failures is not something this milestone's own changes did — but it does
**not** establish that disk contention specifically explains the change, because the comparison
has no independent baseline for machine load. What it does establish, and what matters more:
**the exact same code and machine produced 4/10 and 0/10 at two different disk states.** Any
future before/after comparison that is not a fresh paired baseline, taken in the same session
and disk state, reproduces this exact confound while looking like a clean result — and the
failure mode is silent, because nobody discovers a disk-confounded comparison by looking at the
"after" alone. Conditions are recorded here alongside the numbers so a later pairing is
checkable: free disk ~19G, fsync 1.5-2.0ms/op, commit `ef51193`, 2026-08-19, pooled n=1521
store opens.

**The headroom measurement, and why "8x" is the wrong altitude to read it at.** At the median,
today's machine sits comfortably clear: 29.1-32.6ms per store-open, 2.40-2.56 opens per client
request, giving a per-request cost of roughly 70-80ms. The budget applies to the STORM's own
8-request convoy, not one request: 8 x 70-80ms totals 0.56-0.64s against a 5-second budget —
roughly 8x headroom, and neither pre-registered closing trigger fires (the convoy total does
not threaten, and the per-operation falsifier — a single open approaching seconds — does not
fire either, max observed 261ms). But the flake is a TAIL event, not a median one, and
computing headroom at the tail changes the reading: the SAME 8-deep convoy, run entirely at the
p99 per-open latency, totals 2.81s against the 5s budget — still under, but only by ≈1.8x, not
8x — and
run entirely at the observed maximum, it totals 5.22s, CROSSING the budget by 4%. **A
non-firing median with a tail this close to the budget is a live finding, not a clearance.**
The composition bound sharpens it further: reaching the budget needs the burst average itself
to reach roughly 250ms, which requires broad degradation — about 20% of opens slowed to
roughly 1.1s — not a few slow outliers (a thin 1%-at-one-second tail only moves the average to
about 40ms, nowhere close). **The sharpened verdict: consistent with a genuinely sick storage
volume, inconsistent with mild pressure.** That is the bridge the disk hypothesis lacked all
day — not proof the disk did it, but proof it plausibly could, at a specific and falsifiable
magnitude rather than a vague "the disk was worse."

All instrumentation used to take these measurements was reverted before commit; no fix, and no
code change, lands with M09 for this flake. What the lane produced instead, quantified rather
than argued: each store open costs roughly 30ms of STRUCTURAL work (journal load, anchor
validation, directory scans — fsync itself is only about 6% of an open) that serializes on an
exclusive lock regardless of handler threading, at roughly 2.5 opens per request. Removing ONE
open saves ~30ms per request directly, but ~244ms off the 8-deep convoy's tail — an 8x
amplification, because the convoy is where an open's cost is spent. Fewer opens, not more
threads, is the quantified form of M10's own D1 premise; put another way, the storm lane
produced no fix, but it empirically validated M10-D1's design premise and sized its payoff.

**Grading the lane's own predictions, honestly:** a pre-registered no-referral prediction held
10 out of 10 runs, but it was derived from a headroom model wrong by roughly an order of
magnitude (40-100x predicted, ≈8x measured at the median) — right only because both the wrong
model and the correct one land on the same side of the trigger; a near-boundary case would have
flipped it. Recorded as RIGHT-FOR-WRONG-REASON, not a successful forecast, on its own author's
principle: a number that is right for a reason its author does not have is not a measurement.
The weight-bearing findings from this lane are the measurements — the tail headroom, the
composition bound, the disk-conditional pairing — not the prediction that happened to survive
them.

**Handoff condition for M10 (binding, not a note):** the storm's before/after comparison MUST
be a fresh paired baseline taken in one session at one disk state — never a comparison against
the figures recorded here. This document would rather be superseded than misused. The
mechanism (three candidate hypotheses, none yet scored against each other) and the O(history)
cost question the headroom measurement was built to inform are both referred to M10 alongside
this condition.

## The paid judge run: coverage achieved, story failed

M08's own measurement found that seven of fourteen MCP tools had never been touched across nine
judge runs. A second story was designed, adversarially reviewed twice (an early draft was killed
before a paid run — its "stall" depended on a fixed 300-second tool-host timeout and a `pause`
mode that does not kill the underlying process, two permanent runtime properties now recorded in
the story's own "note for future stories"), rehearsed for free, and run paid.

**Coverage was met, verified two ways.** `apps/cli/src/commands/serve/mod.rs` registers every
MCP-tool-backed route before the auth and audit middleware layers are applied — the audit log
records a request even when it is refused, by construction, and only the HTML monitor dashboard
(merged in after both layers) falls outside its population. A direct count from the archived
audit log confirms all seven previously-untouched tools fired: `signal`, `approve`, `pause`,
`resume`, `cancel`, `probe`, and `wake_arm`/`wake_status`.

**The story failed, `passed: false`, 8 findings — 2 critical, 2 high, 2 medium, 2 low.** The
redesign's own forcing mechanism worked exactly as designed: the block fired deterministically
after four identical failures, and the judge triaged the resulting incident correctly (arm and
wait, signal, pause, approve, the full forced sequence). The release did not ship, because of
real defects on the MCP surface that the story's own design surfaced rather than any flaw in the
story: `resume` against the named graph file refuses deterministically on a workspace/staging
collision the MCP tool schema exposes no parameter to satisfy (issue #82), and that same refused
`resume` still commits `execution_resumed` and drops the operator's held nodes *before* reporting
the refusal — the operator is told the operation failed while their manual hold was silently
dropped underneath them (issue #83). Six lesser findings — an unusable `signal` schema, no
evidence-read tool on the MCP surface, mutation replies that blank the timestamp fields a
`status` call answers correctly moments later, an `acceptedMutations` counter still stuck at
zero, a silence-budget breach that writes no event, and `wake_wait`'s own outcome narration
disagreeing with the log for the same wait — round out the record. Archived, hash-verified on
both copy hops, as `docs/acceptance/m09-judge-run-2026-08-19/`.

**The method lesson: rehearse on the exact surface the real actor uses.** The free rehearsal's
own driver supplied a `project` parameter on every `resume` call — a parameter the real MCP tool
schema never exposes at all. The rehearsal proved the state machine's mechanics thoroughly and
correctly; it structurally could not have caught #82 or #83, because it never touched the layer
those defects live on. That is a gap in which layer was rehearsed, not in how carefully the
rehearsal was executed.

## What the milestone learned, past its own shipped code

Investigating the store-layout recovery fix (#76/#84) killed two independently-derived traces
about why an evidence guard passed, both wrong for the same reason: widening the
recoverable-layout rule to include `blobs/` could never have felled that guard, because a
missing `blobs/` dies on TWO INDEPENDENT DEFENCES, neither of which the layout rule controls.
`classify_layout` already refuses a missing `blobs/` directory outright — it is explicitly
excluded from the recoverable set. Separately, a `blobs/` directory that is present but missing
one evidence file dies inside `read_verified_blob`, on the file open, during `load_state`
(`core/events/src/local.rs:1225` at main `0fb0e66` — the function moved once already this
milestone; cite the base, not the bare number). Proving the layout-recoverability rule at all
needed the one archive that seals no evidence at all. A full-crate run separately caught an
existing invariant asserting the opposite for five other components; it was split rather than
deleted, with the reasoning rewritten in both halves. Two plausible traces, both wrong, and a
guard that had been green the whole time for two reasons nobody had actually named until it was
run.

## CITE-or-MARK

Adopted mid-milestone as the docs rule for committed milestone documents, and the reason is
inside this document: `m09-seeds.md` stated the evidence rule ("a finding we act on because it
is right must not also teach us to trust numbers nobody checked") four lines above a seed that
broke it — a rate with no invocation, no N, no base. A rule stated once and not applied to the
claims sitting beside it is decoration; the fix is a mechanism, not a second sentence. Two
clauses, kept deliberately light — a heavier version (every number spelling out full
invocation/N/base inline) was considered and killed, because that reads well in one note and
makes milestone-scale prose unreadable, and an unreadable rule is the one that gets silently
dropped:

1. An unprovenanced **number** cites its source, even loosely ("H's table," "measured in run
   7") — it needs a pointer a reader can follow, not a footnote-grade citation.
2. An unmeasured **claim** stated as settled is **marked** unmeasured ("registered prediction,"
   "unconfirmed," "estimate") rather than written in declarative voice.

Applied to this milestone's own founding numbers, not only its seeds doc: the owner's cited
48%/15→36ms/3x figures motivating M10's incremental-verified-prefix design have no source
anywhere in this repository (a stated zero-hit search). M10's own step zero, before building on
those numbers, is reproducing them with provenance.

The debt this rule exposes is smaller than "audit every rate" first read, once tested against
its own worst case: a zero is the *only* result a completely dead instrument reproduces
perfectly — every non-zero count, and every red named at its own specific panic or assertion
site, is already its own run receipt, because a harness that failed to start cannot produce
either. One 14-row sabotage ledger audited under this test found twelve rows self-verifying by
panic site, one zero-count self-verifying by a paired non-zero observation, and exactly one bare
zero with no positive observation beside it — caught and marked not-run-verified before
publishing rather than after. The honest remaining scope is narrower than a blanket "not
audited": the zeros above this rule's own adoption point in this document's drafting are not
individually re-walked against it; non-zero counts and panic-site-named reds already carry their
own receipt.

## Honest limits

1. **Nothing from this milestone's own flake-stabilization or oracle work is on `main` yet.**
   Seed 4's five commits, flake #2 (#73), and flake #3 plus its discriminator (#74) are all on
   `origin/issue-m09-arming-the-alarm`, pushed, not merged. The branch is not rebased onto
   `main`'s current tip. Closing this milestone requires that rebase, a full gate run after it,
   and a squash-merge — none of which this document performs. **Superseded pre-execution:** a
   rebase would void every SHA-pinned approval on this branch, by the board's own rule — the
   owner instead merges `main` INTO the milestone branch (preserving those SHAs as ancestors),
   runs the full gate on the combined tree, and squash-merges to `main`. `main`'s resulting
   history is identical either way; only the intermediate step changed.
2. **Issue #74's PR landed on a RED gate stage, by explicit owner decision to land on the
   evidence rather than re-roll.** The red — `admin_operator_binds_pool_profile_and_source_
   identity`, a Postgres backup/restore test under a non-C collation matrix — is judged
   unrelated to the fix (the change touches no PostgreSQL or SQL code; the same test binary
   passed in one matrix and failed in another within the same run). The rate under load stays
   **unmeasured**: isolated re-runs bound it at only ≈63% confidence, nearly no constraint.
   Investigating it surfaced that the same test file's *other* registered flake passed in the
   very run where this one failed — evidence that a shared 30-second timeout policy, not either
   individual test, is the actual defect (tracked as issue #81).
3. **The storm's mechanism is still open**, code exonerated but attribution to disk specifically
   not established — sharpened, not settled, by the tail-headroom measurement (≈8x at the
   median, ≈1.8x at the p99, crossing at the observed maximum): consistent with a genuinely
   sick volume, inconsistent with mild pressure, still not proof either way. See "The storm"
   above for the binding handoff condition: M10's own before/after must be a fresh paired
   baseline, never a comparison against these figures directly — the same code measured 4/10
   and 0/10 on this machine at two disk states, which is the confound any future comparison
   must not silently reproduce.
4. **The paid judge run's two replay attempts are byte-identical to each other and confirm
   nothing about replay.** Both errored identically (a missing scope/stream selection on a
   multi-execution store) rather than replaying anything — a deterministic failure, not a
   determinism check.
5. **Two loose threads from the storm-phase investigation are PENDING, not close blockers**: a
   status-code question on `resume`'s failure surface (owner: D) and the prediction ledger's
   final scoring pass (owner: M). Both carry into M10 as open rows rather than being resolved
   here.
6. **The three flakes named at this milestone's open leave one open at its close.** A milestone
   titled "Arming the Alarm" whose own new tests sit in the same file as a known-flaky sibling
   is not a coincidence worth burying.

## What remains for M10

- The owner's founding performance numbers (48%/15→36ms/3x) need in-repo reproduction with
  provenance before M10's incremental-verified-prefix design builds on them.
- The storm's mechanism (three candidate hypotheses, none scored against each other) and the
  disk-vs-load attribution question — under the binding handoff condition above: a fresh paired
  baseline in one session and disk state, never a comparison against this document's own
  figures.
- Flake #3's own oracle upgrade (per-round receipt-sequence identity) — separate from the
  discriminator half of issue #74 that already shipped.
- Issue #81 (the shared 30-second restore-path timeout policy) and the mis-burn-to-attention
  wiring (`wake_mis_burns` is now populated and not yet surfaced to an operator).
- Nine new seeds recorded in `m09-seeds.md`, four existing seeds closed with landing citations,
  one figure corrected — see that file for the full inventory.

## Closing rule

This milestone did not run a blind-judge closing pass against a corrected surface the way M07
and M08 did — its own judge run (the second story) served that role directly, on the actual
runtime rather than a rehearsal, and it did not pass. Closing M09 is therefore not "the judge
approved" (M07's own record establishes he does not, by rule) and not "the judge run was clean"
— it is that the milestone's SCOPED deliverables (flake stabilization, the arming mechanism, the
coverage-gap story) are complete and evidenced, the defects that story surfaced are filed as
next-milestone product work rather than close blockers (a milestone that finds a bug is not
automatically the milestone that must fix it), and every landing claim in this document names
what was checked and when it was checked, not merely that it was.

The class claimed one more instance on its way out the door: the first invocation of the close
gate itself returned a clean exit code and a completion notification from a shell that could
not find the interpreter it was told to run — a vacuous green, caught only by reading the
output rather than the exit banner, on the very last check of the milestone whose own rule is
that a check proves nothing it did not actually run.
