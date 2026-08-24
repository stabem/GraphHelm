# #201 first measurement — sealed BEFORE the run

Base: `issue-201-clearance-at-own-sequence` @ `4645d75` (the ten guards, preserved from #161)
plus `schemas/` taken **wholesale** from A's `b6e4595`. Wholesale on purpose: no hand-merging, no
pin recomputation, nothing of A's schema written by me. Target dir `D:/graphhelm-target-j201`.

## Why this is a PARTIAL test, measured rather than assumed

Taking A's schemas wholesale gives their six kinds and drops my two
(`clearance_identity_registered` / `_revoked`, still unapplied by A). Enumerating each guard's
fixture by what it appends:

- **5 of 10 need only A's kinds** and become runnable now.
- **5 of 10 also append identity events** and must still die at `append_atomic`.

All **three** guards that exercise ALREADY-IMPLEMENTED fold arms are in the runnable five, which is
what makes the partial test decisive rather than merely convenient.

## Sealed predictions

| guard | group | predicted |
|---|---|---|
| `a_claim_naming_a_superseded_wait_answers_nothing_and_leaves_the_node_parked` | implemented | **GREEN** |
| `the_downstream_of_a_claimed_wait_is_not_ready_until_the_claim_clears` | implemented | **GREEN** |
| `a_clearance_naming_no_claim_is_corrupt_rather_than_silently_ignored` | implemented | **GREEN** |
| `a_clearance_by_an_identity_never_registered_is_refused` | unimplemented | **RED at its own assertion** |
| `a_refused_clearance_is_recorded_and_the_log_still_replays` | unimplemented | **RED at its own assertion** |
| the other five | blocked on my two kinds | still `Err(Invalid)` at append |

## The cells that can cost me

- **Any of the runnable five still returning `Err(Invalid)`** means A's six kinds are not sufficient
  and the blocker is something else. A asked to be told exactly this, so it gets reported as a
  finding rather than folded into a pass rate.
- **The two "unimplemented" guards coming out GREEN** refutes the claim that membership-at-N is
  unimplemented — a claim I and the reviewer of #193 made independently. If that happens, #201 is
  closed as mistaken, not quietly rescoped.
- **All five going red at the SAME panic site** would mean they share a failure upstream of their
  assertions, i.e. the old vacuous red in a new place, and none of them would count.

## Adjudication, same rule as the #161 matrix

A guard counts as data only if the run prints a `test result:` line. No line, no verdict. Durations
are **not** citable: another lane's workspace gate overlaps this window, which destroys the clock
oracle and leaves the result oracle intact.

# RESULTS — 9 of 10 predictions correct, and the one refutation is mine

Run in `D:/graphhelm-target-j201`. Durations not citable (another lane's workspace gate overlapped);
verdicts and panic sites are.

## First: the run refuted my premise before it could test my prediction

The first run had **all ten failing at `:97:40`** — the append — including my own registry guard.
Under the rule sealed above ("all five red at the SAME panic site means they share a failure
upstream of their assertions"), that run counted for nothing, and I did not read it as a result.

Cause, measured rather than reasoned. `ClearanceVerifier` in `core/protocols/src/event.rs` carried
`#[serde(rename_all = "camelCase", tag = "type")]`. I made it print what it actually serialises:

```
COUNTERSIGN   = {"type":"countersign","identity":"auditor-a","key_fingerprint":"sha256:bbb..."}
MACHINEREPLAY = {"type":"machineReplay","manifest_hash":"sha256:ccc..."}
```

`rename_all` on an enum renames the **variants** — and those are correct, matching the schema's
`countersign` / `machineReplay` consts. The **fields inside struct variants** need
`rename_all_fields`, which was absent, so they ship snake_case into a `clearanceVerifier` that
requires `manifestHash` / `keyFingerprint` under `additionalProperties: false`. Bare `Invalid`.

This is why `completion_claimed` validated and `completion_cleared` did not: `CompletionClaimed`
has no struct-variant enum in its payload. **Enumerated rather than assumed**: `rename_all_fields`
appears **zero** times in `core/protocols/src/`, and a scan of every enum in `event.rs` for
camelCase-tagged enums with snake_case struct-variant fields returns **exactly one** — this.

## Then the prediction, with the one-attribute fix applied

| guard | group | predicted | observed |
|---|---|---|---|
| `a_claim_naming_a_superseded_wait...` | implemented | GREEN | **ok** |
| `a_clearance_naming_no_claim...` | implemented | GREEN | **ok** |
| `the_downstream_of_a_claimed_wait...` | implemented | GREEN | **RED at `:1657`** |
| `a_clearance_by_an_identity_never_registered_is_refused` | unimplemented | RED at own assertion | **RED at `:1797`** |
| `a_refused_clearance_is_recorded_and_the_log_still_replays` | unimplemented | RED at own assertion | **RED at `:1937`** |
| the other five | blocked on my two kinds | `Invalid` at append | **`:97:40`** |

**The two that mattered most came out exactly as sealed, and they say WHY:**

```
:1797  left: None   right: Some(Refused { reason_code: SafeCode("unknown_identity") })
:1937  the mistake is ON RECORD, readable by anyone replaying: {}
```

Both are the empty `clearances` map. Membership-at-N is unimplemented, now demonstrated **at the
guards' own assertions** rather than argued from a grep — the first time any guard in this lane has
been red at its own assertion, which is exactly what #193's reviewer said had never happened. Three
distinct panic sites (`:1657`, `:1797`, `:1937`) is the control that they are not sharing one
upstream failure.

## The refutation, and it is a defect in my CRITERION, not a near miss

`the_downstream_of_a_claimed_wait_is_not_ready_until_the_claim_clears` failed at its own assertion:

```
once cleared, the dependent is ready through the SAME derivation both drivers call
```

I grouped it as "implemented" using the predicate **"does the fold arm exist?"** — and it does. But
that guard does not ask about the fold arm. It asks whether the dependent becomes **ready through
the shared readiness derivation**, which is a different code path that my criterion never looked at.
**The classification was sound for the other nine and wrong for this one because I applied a
predicate that answers a different question than the guard asks.**

I am not diagnosing whether the readiness derivation is defective or the guard expects something
unbuilt: that is lane 1's territory and belongs to whoever owns it, reported rather than concluded.

## Scoreboard, stated so it can lose

Nine of ten correct. The claim that **membership-at-N is unimplemented survived the run that could
have killed it** — had those two come out green, this issue would have closed as mistaken. It did
not, and the evidence is now an assertion failure rather than an absence of grep hits.

# THE FULL SET, with both halves of the schema present

My two kinds re-added to A's schema (textual insertion, 40 lines, no reformat), propagated
byte-identical to `releases/1.0.0/`, both catalog pins recomputed.

**Pin method validated before use, not after.** The same canonicalisation reproduced two pins nobody
had touched — `agent` and `edge` — byte for byte, against what the catalog already recorded. Only
then was it trusted on the file I had changed. Without that control the new pin is an unfalsifiable
number that agrees with itself.

```
agent  computed sha256:1f60a931...  recorded sha256:1f60a931...
edge   computed sha256:5b6ff343...  recorded sha256:5b6ff343...
event-envelope  07b1a852... -> edc22519...
```

## Result: 29 passed, 8 failed, and NOTHING is stuck at the append

**All seven membership-at-N guards are red AT THEIR OWN ASSERTIONS, at seven distinct panic sites:**

| line | guard |
|------|-------|
| `:1769` | `a_clearance_by_an_identity_registered_after_it_is_refused` |
| `:1797` | `a_clearance_by_an_identity_never_registered_is_refused` |
| `:1830` | `a_clearance_survives_the_later_revocation_of_its_signer` |
| `:1856` | `a_revoked_identity_cannot_clear_a_later_claim` |
| `:1879` | `a_clearance_whose_fingerprint_does_not_match_the_registration_is_refused` |
| `:1911` | `interleaved_registrations_and_clearances_replay_identically` |
| `:1937` | `a_refused_clearance_is_recorded_and_the_log_still_replays` |

**Zero guards at `:97:40`.** Seven distinct sites is the control that they are not sharing one
failure upstream — the exact defect that made every earlier red in this lane worthless.

`the_identity_registry_folds_deterministically_and_revocation_removes` is **green** again, as it
must be once my kinds are declared: it failed here only while the schema lacked them, which is
attribution rather than regression.

## What this unlocks, and why it could not have been done first

#193's reviewer established that not one guard in this lane had ever been observed failing at its
own assertion, so any green would have been decoration. **That precondition is now satisfied for
all seven.** Implementing the `CompletionCleared` validation before this point would have turned
seven guards green having never been red — which is why the fold was deferred to this issue rather
than written when the schema blocker first lifted.

The eighth failure, `the_downstream_of_a_claimed_wait...` at `:1657`, is the guard that refuted my
classification and remains reported-not-diagnosed: it belongs to the shared readiness derivation,
not to this fold, and it is lane 1's to answer.

# SEALED BEFORE WRITING THE FOLD — including the cells that must NOT go green

All seven going green proves little on its own: an implementation written to satisfy assertions
does that too. The informative part is which guards must FAIL under each way of getting it subtly
wrong — and the guards' own doc comments already made advance claims about which of them do NOT
discriminate. Those claims are testable, and they were written before any of this ran.

## Phase 1 — after implementing

All seven GREEN. **And the eighth failure must STAY RED:**
`the_downstream_of_a_claimed_wait_is_not_ready_until_the_claim_clears` (`:1657`). It belongs to the
shared readiness derivation, not to this fold. **If it turns green, either my fold touched a path it
had no business touching, or my diagnosis that it is a different code path was wrong.** That is the
cell that can catch an implementation reaching further than it claims.

The 29 currently-passing tests must stay green: regression control.

## Phase 2 — the sabotage matrix, which is the part that measures

| id | mutation | must FAIL | must STAY GREEN |
|----|----------|-----------|-----------------|
| T1 | validate against the FINAL registry instead of the registry at the folded sequence | `registered_after`, `survives_revocation`, `interleaved` | `never_registered`, `revoked_cannot_clear`, `fingerprint_mismatch`, `refused_is_recorded` |
| T2 | registry ignores revocation (never removes) | `revoked_cannot_clear` **only** | all six others, `survives_revocation` **especially** |
| T3 | compare identity name only, ignore the fingerprint | `fingerprint_mismatch` **only** | all six others |

**T2 is the row that tests the author's own advance claim.** The doc on
`a_clearance_survives_the_later_revocation_of_its_signer` says it *"stays GREEN under a registry that
ignores revocation entirely, because it asserts SURVIVAL"*, and names
`a_revoked_identity_cannot_clear_a_later_claim` as the member that makes revocation bite. **If T2
fails anything other than that one guard, the pair does not divide the way its own documentation
says it does** — and the doc has been describing a property it never had.

**T1's green cells are the ones that can embarrass me.** `never_registered` and
`fingerprint_mismatch` are supposed to be caught by wrong implementations too — they are companions,
not discriminators. **If T1 fails them as well, then the fixtures are not isolating what their
comments claim**, and the discriminating cell is carrying credit that belongs to the arrangement
rather than to the property.

## What would make me distrust a clean sweep

If every mutation fails every guard, the guards are not discriminating between failure modes at all
— they are just detecting "the fold is broken", which is the coarse-grained assertion this lane
exists to avoid. A matrix with distinct FAIL sets per row is the evidence; a matrix of all-fails is
[[uniform-output-is-harness-broke]] in its slow form.

# PHASE 1 + PHASE 2 RESULTS

## Phase 1: as sealed, including the cell that could have caught me

All seven green (36 passed / 1 failed), and
`the_downstream_of_a_claimed_wait_is_not_ready_until_the_claim_clears` **STAYED RED at `:1657`**.
That was the sealed "must NOT go green" cell: a green there would have meant the fold reached into
the shared readiness derivation, which is not its business. It did not.

## Phase 2: T1 and T3 exactly as sealed, T2 REFUTED

| id | sealed FAIL set | observed FAIL set | verdict |
|----|-----------------|-------------------|---------|
| T1 (final registry, not at-sequence) | `registered_after`, `survives_revocation`, `interleaved` | **identical** | **as sealed, 7/7 cells** |
| T2 (revocation ignored) | `revoked_cannot_clear` **only** | `revoked_cannot_clear` **and** `survives_revocation` | **REFUTED** |
| T3 (name only, ignore fingerprint) | `fingerprint_mismatch` **only** | **identical** | **as sealed** |

**Three distinct FAIL sets — sizes 3, 2, 1.** The guards discriminate between failure *modes*, not
merely between "fold works" and "fold broken", which is the coarse assertion this lane exists to
avoid. A matrix of all-fails would have meant the opposite.

## The refutation, and it is a false claim in a doc comment I wrote

The doc on `a_clearance_survives_the_later_revocation_of_its_signer` states it *"stays GREEN under a
registry that ignores revocation entirely, because it asserts SURVIVAL"*. **Measured: it goes RED
under exactly that mutation.** Panic site `:1826`:

```
precondition: the signer is revoked at head, or this measures nothing
```

**It failed at its PRECONDITION, not at its property assertion.** The property claim in the doc is
correct — the `Cleared` verdict does survive T2. What the doc forgot is that the guard *also*
asserts a precondition about head state, and T2 is precisely the mutation that breaks it. The
precondition is doing its job and firing correctly: under T2 the fixture genuinely would measure
nothing.

**Transferable form: a precondition assertion widens a guard's failure surface beyond the property
the guard is named after.** Claiming "guard X is blind to mutation M" requires checking *every*
assertion in X, not the headline one. I described the guard by its title and its own setup refuted
me.

The pair's division of labour survives in a narrower form: `revoked_cannot_clear` is still the only
guard whose **property** T2 breaks. `survives_revocation` fails T2 as a *fixture*, not as a
*measurement* — a distinction the doc erased and this run restored.

## A void row, and the rule that voided it

The first attempt at this matrix produced three rows of byte-identical output. Under
[[uniform-output-is-harness-broke]] that is a broken harness, not a finding, and it was thrown away
rather than read. Cause: the mutation script was written to a relative path from a reverted working
directory and never existed, while the loop's `git checkout` reset ate the **uncommitted**
implementation before any mutation could apply. **The matrix resets with `git checkout`, so its
subject must be COMMITTED before it runs** — that is now in the fold's commit message.

The second void was narrower and the rule caught it too: T1's anchor
(`let mut projection = ExecutionProjection::default();`) appears **twice** in the file, so the script
asserted and died. The row then showed all-green, which reads exactly like a dramatic finding — *the
discriminator does not discriminate* — and was entirely an artifact of no mutation being applied.
**The script printing `APPLIED T1` is the harness proof for this matrix**, the same role
`test result:` plays for the runs.

# RESERVATION, added after the fact and before anyone reads the numbers above

**The tree these results ran on is a base that exists nowhere.** Lane 1's author reconstructed it
and I verified every step:

```
rename_all_fields:  60370b1 = 0  |  424b084 = 2  |  4fc059c = 2
424b084 is an ancestor of this branch's HEAD?  NO
merge-base(HEAD, lane 1 head) = 60370b1
```

This branch sits **one commit before** lane 1's serde fix, and I ran their **newer schema** against
my **older Rust**. Schema requiring `manifestHash` plus a type emitting `manifest_hash` is the
rejection I measured and reported as a defect. **The red was real and the thing it described does
not exist on any branch.** Same class as stale rlib plus fresh source in a shared target dir:
artifacts of different provenance assembled as if they were one state.

**Half a migration is a third base with no name.** Either take the whole tree or declare the chimera
and attribute the result to neither branch.

## What this withdraws

- The `ClearanceVerifier` finding. `b602bfd` **duplicates** a fix that already existed at `424b084`.
- The statement I gave another lane as an exhaustive enumeration — *"`rename_all_fields` = ZERO in
  all of `core/protocols/src/`"* — was **true of this tree and false of lane 1's head**, and I did
  not name the tree as the claim's scope. It made an already-fixed defect look open.
- The `the_downstream_of_a_claimed_wait` failure. Its cause landed in `424b084` too.

I nearly refuted that last point wrongly: `grep dispatchable_deploy` in `projection.rs` returned
zero at both commits, and I was one step from replying "your second claim does not verify". The
whole-tree search **with a positive control** found it in
`core/events/tests/execution_projection.rs`. **My zero was the wrong scope, not an absence** — the
same defect class, committed against the person who had just corrected me.

## What this does NOT withdraw

The T1/T2/T3 matrix mutates **this branch's own fold**, and every fixture appended successfully, so
it measures the logic written here rather than the schema↔Rust boundary. Those results stand. What
falls is exactly the two claims that crossed the boundary.

**Next: rebase onto lane 1's head once it lands without the two kinds that belong to another lane,
then re-run.** Their sealed prediction — the clearance guards pass untouched — will be measured, and
it is theirs, not mine.

# T8, sealed before running — closing the gap the reviewer found in this matrix

The reviewer showed the union of T1/T2/T3 reddens **five of seven** guards, and that
`refused_is_recorded` is reddened by **none** of them while being **declared nowhere** as a control.
The dimension it asserts — a refusal is journal data, and only an *uninterpretable* log is `Corrupt`
(the M09 rule) — has no mutation in this matrix. So the guard was carrying credit the matrix had not
earned it.

**T8: a refused clearance returns `Err(ReplayError::Corrupt)` instead of recording the outcome.**

| prediction | reasoning |
|---|---|
| `refused_is_recorded` goes RED | it replays a refusal and expects `Ok`; that is the property |
| `registered_after`, `never_registered`, `revoked_cannot_clear`, `fingerprint_mismatch` also go RED | each replays a refusal too, so each hits the same `Corrupt` |
| the two clear-path guards stay GREEN | `survives_revocation` and `interleaved` never refuse |

**The honest part, sealed with it: T8 will NOT isolate `refused_is_recorded`.** Five guards fall
together. That is a statement about the fixture family, not about T8 — every refusal fixture shares
the property, so no mutation of this fold can single that guard out. **It moves from "no mutation
reaches it" to "reddened, but never alone", and it is a weaker cell than its siblings.** Saying so is
the point; a matrix that quietly implies seven isolated cells is the thing being corrected.

**UNINFORMATIVE cell:** if `survives_revocation` or `interleaved` also go red, the mutation is
reaching the clear path and T8 measures something wider than a refusal rule.

## T8 results — shape as sealed, COUNT wrong, and the miss is my own guard

```
RED (6): refused_is_recorded, registered_after, never_registered,
         revoked_cannot_clear, fingerprint_mismatch,
         a_refused_clearance_leaves_a_pointer_in_the_nodes_timeline_not_a_copy  <- NOT PREDICTED
GREEN  : survives_revocation, interleaved      (clear path untouched, as sealed)
33 passed / 6 failed
```

**The UNINFORMATIVE cell did not trigger:** both clear-path guards stayed green, so T8 measures a
refusal rule and nothing wider.

**But I predicted five reds and got six.** The extra is
`a_refused_clearance_leaves_a_pointer_in_the_nodes_timeline_not_a_copy` — **a guard I wrote myself an
hour earlier, in this same file.** It replays a refusal like the other five, so it was always going
to fall.

**I enumerated the original seven from memory instead of enumerating what actually appends a
refusal.** That is the same defect as counting seven blocked guards when ten consumed the undeclared
kinds: *intent does not know what the fixtures append.* Twice, on the same lane, with the same cure
available and unused — list the fixtures by what they DO, never the cells by what they are called.

**What T8 buys, stated at its real size:** `refused_is_recorded` moves from *"no mutation in this
matrix can redden it"* to *"reddened, but never alone."* Six guards fall together because every
refusal fixture shares the property; **no mutation of this fold can single that guard out.** It is a
weaker cell than its siblings, and the matrix now says so instead of implying seven isolated ones.
