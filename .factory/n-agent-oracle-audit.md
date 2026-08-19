# STATE OF THIS FILE - read before citing anything in it

This file is append-only: corrections were added at the end and the superseded text left in
place with inline pointers, so nothing was quietly rewritten. That is honest but it means a
reader who stops early can cite a claim that is already dead. This section is the index of what
is live.

**Anything below this section that is not listed here as LIVE has been superseded, refined, or
falsified further down. Check before quoting.**

## MEASURED (someone ran it; raw output and invocation in the file)
- **The belt is hollow.** `concurrent_sweeps_never_double_consume_a_lease` reported GREEN with
  the recorder deleted (S0, C), and GREEN with `armed=15 consumed=1` - fourteen of fifteen
  consumptions missing (A's instrument). The hardest number in this file.
- **The deterministic #55 red stays green with the recorder dead.** Entry #1 cannot separate "the
  guard dropped the stale capture" from "the recorder failed before reaching it".
- **Entry #2's close works.** The wrong-reason sabotage fells it (left StaleRendezvous, right
  Rung). Shipped and measured, per B's requirement that it be observed rather than argued.
- **The mechanism: conflict, not replay.** A key reused at a later sequence takes the conflict
  path; the louder version I killed by checking stays dead. C's instrument.
- **The sleeper guard is not a flake.** H measured 9/10 isolated, 3/3 in-suite, one stable
  failure form: the lease reads back `live:true, lastConsumed:null` after the ring.

## LIVE, from reading (not run - flagged as such where they appear)
- The oracle-class inventory of 20 guards: (a) 2, (b) 12, (c) 6, with a named legal-but-wrong
  outcome for each (a) and (c) row, and a minimal closing assertion for each.
- RC1 (the type-string helper is blind by construction), RC2 (timeout-negatives with no positive
  control), RC3 (`recorded: usize` is a lossy oracle surface), RC4 (the receipt cannot attribute
  a burn to an arming).
- The second S4 hole: a sweep with two due sessions builds a batch with duplicate keys and the
  store refuses the WHOLE batch. Nothing in the family can reach it - the only armed session
  anywhere is `session-sleeper-1`. This one is a genuine COVERAGE gap.
- The self-repair rule, both legs' mechanisms verified in source: repair latency < observation
  window => the oracle is blind.

## DEAD - do not cite these
- ~~"Nothing pins divergence-by-sequence-alone."~~ FALSIFIED by C's sabotage: the digest blinded
  to sequence fells seven tests, five of them pre-existing guards. True of the NAMES, false of
  the COVERAGE. What survives is narrower: the property is **enforced and undeclared**.
- ~~"The chain proving #55 has NO link that fails when the recorder dies."~~ Too strong. Entry #2
  does fail. Corrected: exactly one link fails when the recorder is DEAD, and it asserts a COUNT,
  so no link at all fails when the recorder is WRONG.
- ~~"No guard drives two consumptions for one session."~~ False - the belt does, fourteen times.
  It is a DETECTION gap, not a coverage gap.
- ~~"The recorder's count is honest by accident."~~ Too strong. Honest for a reason: replay is
  restricted to the case where "on record" and "written by us" mean the same thing. Undeclared,
  not accidental.
- ~~"Journal count 1 confirms the mechanism."~~ My own sealed cell, over-reached. A count of 1
  confirms INCIDENCE only; conflict and replay produce identical journals.
- ~~The sleeper guard fails "roughly 3 in 4."~~ Inherited from `m09-seeds.md` seed 9 and
  unciteable (no N, no base, no invocation). H's measured figure is 12/13.

## UNMEASURED - flagged so nobody inherits it as fact
- **"The positive control is redundant because the neighbour test pins it."** My own dismissal
  when withdrawing my instrument. It is a coverage claim inferred from READING, and nobody has
  sabotaged for it - C measured coverage of the DIVERGENCE property, not of REPLAY-REACHABILITY.
  Marked unmeasured, not established. One sabotage settles it: break replay-reachability, see
  whether any test falls. (M caught this inside the sentence that conceded the previous instance.)

## The distinction B drew, which is the file's own vocabulary corrected
A sabotage that fells many tests proves the SUITE covers a property. It does not prove any single
test MEASURES it. Five guards enforcing a property none of them names is exactly the state this
audit exists to find - and it is why "guarded" and "incidentally covered" are not the same word.

## Provenance
Every number in this file cites its source. Every unverified claim carries a marker (PREDICTION,
reasoned-not-run, block judgement, or "not verified by me"). Runs are credited to whoever ran
them: A (the belt count), C (the sabotage series and the store test), H (the base measurements).
I ran nothing.

---

# N Agent - ORACLE AUDIT of the wake guard family

Written by N Agent. Read-only study: NO code changed, NO cargo run, nothing staged.
ASCII-only. Base: worktree at `efd85d0` (main, M09 A merged). Branch-only guards read via
`git show origin/issue-m09-arming-the-alarm:<path>` - no checkout, no branch switch.

## Provenance of every claim here
Each row below is derived from source I read at the line ranges named in it. Where I make a
claim about what a guard would PASS, the claim is a reading of the assertion text, NOT a
measurement - none of these were run (H holds the cargo queue). Every such claim is written
as a prediction and marked PREDICTION, so it can be killed by one run instead of debated.

## The finding this audit serves (C, ratified)
The #55-family oracle is "the stream still replays". That sees only failures that make the
log ILLEGAL. Burning a live lease is LEGAL: the fold accepts it, replay succeeds, the
operator surface stays calm, and the store's receipt says `rung`. A legality oracle cannot
detect the failure mode the product exists to prevent.

## Oracle classes used
- **(a) legality** - the assertion is satisfied by "the stream replays / the call returned
  ok". Sees illegal logs only.
- **(b) receipt-grain** - the assertion reads the store's own answer about the lease:
  `wake_leases`, `wake_last_consumed` (reason + sequence), a named surface field, or the
  presence/absence of a SPECIFIC event with its payload checked.
- **(c) mixed** - some receipt-grain assertions, but the guard's headline property rests on
  a legality or a by-timeout negative.

Model of (b) done right: C's parked `a_capture_from_before_the_wake_never_burns_the_lease_armed_after_it`
(`.factory/c-agent-rdv-equal-red-draft.rs`, C worktree). It asserts `recorded == 0` AND that
the sleeper's current lease is still in `projection.wake_leases` - and says in its own
comment why the first assertion alone would not measure the property.

---

## Inventory

| # | Guard | File:line | Class | Blind spot named |
|---|---|---|---|---|
| 1 | `a_rival_consume_between_read_and_record_appends_nothing` | serve/wake.rs:251 | **c** | `recorded == 0` is the return value of a function that returns 0 on eleven unrelated `return 0;` sites; the second oracle is pure legality. A recorder that never records anything is green. |
| 2 | `a_live_lease_consumption_still_records` | serve/wake.rs:337 | **c** | Asserts the COUNT (`== 1`) and never what was appended. A consumption written with the wrong reason, or against the wrong session, still counts 1. |
| 3 | `concurrent_sweeps_never_double_consume_a_lease` (15-round belt) | tests/wake_http.rs:870 | **a** | Per round: `value["ok"] == true` (legality). Whole run: `consumed <= armed` (aggregate inequality). A sweep that consumes NOTHING, ever, satisfies both. The test named `never_double_consume` is green when the wake sweep is dead. |
| 4 | `an_append_beyond_the_cursor_rings_one_byte_only_after_the_trigger_is_durable` | tests/wake_http.rs:319 | **c** | The consumption is detected by event TYPE only (`kinds_after`). Reason, session and count are invisible. A burn recorded as `stale_rendezvous` on a lease that actually rang is green. |
| 5 | `a_burned_lease_never_rings_twice_and_no_ring_without_a_fresh_append` | tests/wake_http.rs:366 | **c** | The headline property is a negative measured by TIMEOUT: `Sleeper::wait()` returns an empty vec after 10s for any cause. No positive control proves the ringer was still alive. |
| 6 | `a_missing_rendezvous_consumes_the_lease_without_a_serve_error` | tests/wake_http.rs:405 | **c** | Same type-only detection as #4. A stale cleanup recorded with reason `rung` - the surface then telling a dead sleeper's successor that it was woken - is green. |
| 7 | `wake_wait_exits_zero_on_ring_and_no_hostile_byte_reaches_stdout` | tests/wake_http.rs:481 | **b** | Exit code plus the exact stdout bytes ARE the property; nothing above the grain. |
| 8 | `wake_wait_exits_three_on_timeout_and_two_on_a_bad_id` | tests/wake_http.rs:511 | **b** | Exit codes are the property itself. |
| 9 | `a_sleeper_wakes_on_a_peer_append_with_zero_requests_in_the_window` | tests/wake_http.rs:638 | **b** | Strongest in the family: `live == false`, `lastConsumed.reason == "rung"`, `atSequence > armedCursor`, `head >= rang_at`, plus proxy connection-count equality. No blind spot found at receipt grain. |
| 10 | `arming_moves_the_stream_head_but_never_the_doorbells_head` | tests/wake_http.rs:783 | **b** | Asserts `live`, `head > armed`, `contentHead <= armed`, `lastConsumed == null`. Sound. |
| 11 | `a_dead_serve_degrades_to_timeout_and_a_plain_read_never_to_wrong` | tests/wake_http.rs:823 | **c** | Exit 3 is timeout-by-any-cause (a sidecar that never created its pipe exits 3 too), and the fallback assertion only echoes back `executionId`. Nothing asserts the lease was NOT burned by the dying serve. |
| 12 | `arming_reports_the_head_the_doorbell_compares_and_re_arming_with_it_is_a_fixed_point` | tests/wake_http.rs:961 | **b** | Reads `contentHead`, `armedCursor`, then `live == true` after the echo. Sound. |
| 13 | `arming_a_lease_never_moves_the_execution_clock` | tests/wake_http.rs:1026 | **c** | Asserts a field did NOT change. Nothing in the fixture proves that field CAN change - a `lastEventAt` frozen for an unrelated reason passes. Negative with no positive control. |
| 14 | `arming_twice_replaces_the_lease_and_consumption_burns_it` | events/tests/execution_projection.rs:718 | **b** | Field-grain: lease count, cursor, rendezvous, then `wake_leases.is_empty()`, plus byte-exact wire round-trip. Sound. |
| 15 | `consuming_an_unarmed_lease_is_a_replay_integrity_refusal` | events/tests/execution_projection.rs:784 | **a** | Legality is CORRECT as its subject (it is a refusal test), but `matches!(Err(Corrupt))` does not say WHICH invariant refused: any unrelated corruption in the fixture passes. No positive control that the same fixture minus the ghost consume replays Ok. |
| 16 | `a_burned_lease_leaves_its_receipt_behind` | events/tests/execution_projection.rs:987 | **b** | The template: empty receipt before, then `reason` AND exact `sequence` after, then last-wins across a second cycle. No blind spot found. |
| 17 | `wake_arm_arms_this_session_and_wake_status_reads_it_back` | tests/mcp_stdio.rs:1396 | **b** | Reads back `live`, `rendezvousId`, `cursor`, `sessionId`, and counts the two durable `wake_lease` events. Sound. |
| 18 | `a_session_may_block_only_on_the_lease_it_holds` | tests/mcp_stdio.rs:1475 | **b** | `isError == true` PLUS the refusal text naming both rendezvous ids - so a blanket refusal cannot pass for the right reason. Sound, and its docstring explains why the armed fixture is required. |
| 19 | Branch horizon family: `arming_with_a_declared_bound_...`, `arming_without_a_declared_bound_...`, `a_bound_nobody_will_live_to_see_...`, `a_horizon_already_past_...`, `a_waiter_with_no_lease_of_its_own_...`, `waiting_on_a_lease_that_declared_no_bound_...`, `shortening_a_horizon_...`, `lengthening_a_horizon_...` | branch tests/wake_http.rs | **b** | All read named fields back off the surface (the stored instant, both instants on a shortening, the refusal codes). Not audited line-by-line: none is a race guard, so the legal-but-wrong burn is outside their subject. |
| 20 | Branch fold: `a_horizon_in_the_past_folds_exactly_like_one_in_the_future`, `a_horizon_half_a_second_later_is_stored_as_later` | branch events/tests/execution_projection.rs | **b** | Field-grain on the folded instant. |

Counts: **(a) 2 - (b) 12 - (c) 6** over 20 entries. Entry 19 groups 8 branch guards and
entry 20 groups 2; both are classed as a block from assertion shape, not line-by-line.

---

## The (a) and (c) rows, each with ONE legal-but-wrong outcome it would pass

Each is a state the product must not reach, in which the log stays legal, replay succeeds,
and the guard is green.

**#3 - the 15-round belt (the biggest).** Every round arms through `arm_lease`, which
hardcodes `session_id = "session-sleeper-1"` (wake_http.rs:192-224), so round N+1's arming
REPLACES round N's lease by the one-live-lease rule. The per-round oracle is
`execution status -> ok == true`; the run-level oracle is `consumed <= armed`.
LEGAL-BUT-WRONG: the sweep records ZERO consumptions for all 15 rounds - ringer dead,
recorder returning 0 on an early error path, or the post-append hook never registered.
`armed = 15`, `consumed = 0`, `0 <= 15` holds; no illegal event is ever written, so every
round replays. PREDICTION: removing the sweep's call to `record_consumptions` leaves this
guard GREEN. That is the cheapest decisive sabotage in this audit and it is one line.

**#1 - the rival-consume red.** `record_consumptions` has TWELVE `return 0;` sites
(serve/wake.rs:150-238, counted): store open, `list_streams`, stream lookup,
`read_replay_stream`, `replay`, `next_sequence`, actor parse, two id parses,
`PreparedAppend::new`, `append_atomic` - and the guard's own drop
(`still_live.is_empty()`, serve/wake.rs:178-187). Eleven of the twelve are unrelated to the
property under test, and all twelve are indistinguishable at the call site.
LEGAL-BUT-WRONG: the `still_live` filter is deleted and the function instead fails earlier
for an unrelated reason on this fixture. `recorded == 0` holds, and the replay assertion -
pure legality - holds too. The guard names the rival window but measures "the recorder wrote
nothing", one floor above.

**#2 - the live-lease positive.** LEGAL-BUT-WRONG: the recorder appends a consumption with
`reason: Rung` when the sweep asked for `StaleRendezvous`. `recorded == 1`, green. The
receipt the sleeper later reads then says it was rung when nothing rang - the exact
two-surfaces-disagree shape #9 exists to prevent, and #2 is the only guard on that write path.

**#4 - ring-implies-durable.** Detection is `kinds_after(...).any(|k| k == "wake_lease_consumed")`,
a projection of events to their TYPE STRING with the payload discarded (wake_http.rs:231-257).
LEGAL-BUT-WRONG: the burn for the ring is recorded with `reason: StaleRendezvous`, or against
a different session that happens to be live. Green either way.

**#6 - stale cleanup.** Same type-only detector. LEGAL-BUT-WRONG: the cleanup of a lease
whose pipe never existed is recorded with `reason: Rung`. Green - and the store now asserts a
ring that never happened.

**#5 - never rings twice.** LEGAL-BUT-WRONG: the serve's sweep panics, or the ringer thread
dies, after the first trigger. The second `Sleeper` times out at 10s and returns an empty
vec, which is exactly what the assertion demands. Green while the doorbell is dead.

**#11 - dead-serve degradation.** LEGAL-BUT-WRONG: the dying serve burns the lease on its way
down. The sidecar still exits 3, `execution status` still succeeds and still echoes the
`executionId`, so the guard is green - while the sleeper's next read says `live: false` with a
receipt it never earned.

**#13 - the observer's clock.** LEGAL-BUT-WRONG: `lastEventAt` stops advancing for ALL events.
The fixture asserts only that the stamp is a string before arming and identical after; a
permanently frozen clock satisfies both. The guard cannot tell "arming does not move the
clock" from "nothing moves the clock".

**#15 - unarmed-consume refusal.** LEGAL-BUT-WRONG: the fixture's `ExecutionStarted` event
starts being refused for an unrelated reason. `Err(Corrupt)` matches, green, while the
property under test - an unmatched consumption is refused - is no longer exercised at all.

---

## Minimal receipt-grain assertion that closes each

The smallest addition that makes the named legal-but-wrong outcome RED. These are proposals,
not changes: nothing here was applied.

- **#3** Per round, after the 600ms settle, replay and pin the receipt's SEQUENCE:
  `let r = projection.wake_last_consumed["session-sleeper-1"]; assert!(r.sequence > previous_round_sequence)`,
  carrying `previous_round_sequence` across the loop. Field-grain, available today
  (`wake_last_consumed` is on the projection, execution_projection.rs:987), and red the
  instant a round consumes nothing. Keep `ok == true` and `consumed <= armed` - they are not
  wrong, only insufficient. Per-round attribution to a SPECIFIC arming is not expressible
  today; see RC4.
- **#1** Add the discriminating positive to the same fixture: after `recorded == 0`, replay
  and assert `wake_last_consumed["session-r"].sequence == 2` - the RIVAL's consumption is the
  one on record and ours did not land on top of it. A dead recorder no longer passes, because
  the rival's own append is still required to be the last word.
- **#2** After `recorded == 1`, replay and assert `wake_leases.is_empty()` and
  `wake_last_consumed["session-l"].reason == WakeConsumeReason::Rung` - the reason asked for
  is the reason stored.
- **#4** Replace the type-string poll with a receipt poll: wait until
  `wake_last_consumed["session-sleeper-1"]` exists, then assert `.reason == Rung`.
- **#6** Same shape, asserting `.reason == StaleRendezvous`.
- **#5** Add a positive control at the end: arm a FRESH lease, fire a third trigger, assert
  that one rings. A dead ringer then fails the guard that claims a burned lease is why
  nothing rang.
- **#11** After the sidecar exits 3, read the store and assert `wake_leases` still contains
  the session and `wake_last_consumed` has no entry for it - nothing was burned by the
  serve's death.
- **#13** Add a positive control: after the arm-and-compare, append one real content event
  and assert `lastEventAt` DID move. The frozen-clock world is then red.
- **#15** Add the control replay: the same fixture WITHOUT the ghost consumption must
  `replay(...).is_ok()`. Corruption arriving from anywhere else is then red instead of green.

---

## Cross-cutting findings (root causes, not per-guard)

**RC1 - the type-string projection is structurally blind.** `kinds_after`, `kinds_snapshot`
and `Sleeper::arm`'s `at_ring` snapshot (wake_http.rs:231-257, 303-318, 259-297) all map
events to `kind["type"]` as a String and discard the payload. Every guard built on them is
incapable of receipt grain BY CONSTRUCTION - #4, #5 and #6 are all downstream of this one
helper choice. Minimal systemic fix: one helper beside them returning the replayed
projection's `wake_leases` and `wake_last_consumed`, and let the wake guards assert on that.
Fixing the helper fixes three guards at once.

**RC2 - negatives measured by timeout, with no positive control.** #5, #11 and #13 each
assert that something did NOT happen, in a fixture where nothing proves the mechanism was
alive to make it happen. Same defect three times, same remedy three times: pair every
"did not" with an "and here it does".

**RC3 - `recorded: usize` is a lossy oracle surface.** `record_consumptions` collapses one
guard decision and eleven unrelated failure modes into the same `0`. Guards on it can only ever be class
(c), however they are written. Worth naming to the owner as a design observation, not a test
bug: a typed outcome (`Recorded(n)` / `DroppedNotLive` / `StoreError`) would let #1 assert the
DECISION rather than the count. NOT proposed as a change here - it touches C's file and C
holds that pen.

**RC4 - the receipt cannot attribute a burn to an arming.** `WakeLeaseConsumed` carries
`execution_id`, `session_id` and `reason` - no rendezvous, no armed-sequence. So no guard,
and no operator, can ask the journal "which arming did this burn?". #3's per-round
attribution gap and C's rendezvous-EQUAL burn are the same missing discriminator seen from
two directions.

---

## Convergence with the fix phase (why this feeds PR-2, not only the report)

C's parked red needs a discriminator separating two armings that share session AND
rendezvous, and names the arming's SEQUENCE as the only monotone candidate. RC4 says the
stored receipt lacks exactly that field. If the PR-2 fix puts the arming's sequence into the
lease identity, carrying it into `WakeLeaseConsumed` as well:

1. gives C's red that discriminator at the STORE surface, not only in memory;
2. closes #3's per-round attribution gap, upgrading the 15-round belt from a global
   inequality to a per-round identity check;
3. lets RC1's proposed helper answer "which arming", not only "how many".

One field, three holes. That is the strongest argument I have for the field belonging to the
fix rather than to a follow-up.

## What this audit did NOT establish

- Nothing here was RUN. Every "would pass" is a reading of assertion text, registered above
  as a PREDICTION so one sabotage run can kill it rather than a debate settling it. The
  cheapest decisive one: remove the sweep's call to `record_consumptions` and run #3 alone -
  I predict GREEN.
- I did not audit non-wake guards, the attention/monitor guards, or the branch's
  `read_concurrency.rs`.
- Entries 19 and 20 are block judgements from assertion shape, not line-by-line reads of all
  ten guards.

---

# ADDENDUM - cross-check against C's inventory (written after the audit above)

C sent an independent inventory of the same family and asked for it to be attacked rather
than trusted. This section records where the two reads AGREE, where they DISAGREE, and what
C's message contained that the audit above did not have. C's line refs are at `53d212d`
(branch); mine are at `efd85d0` (main). Where a number differs it is the same guard in two
frames, not a disagreement about facts - e.g. the belt is wake_http.rs:870 (main) /
wake_http.rs:947 (branch), the sleeper guard is :638 (main) / :690 (branch). Verified by
reading both.

## AGREEMENT, independently derived
C and I reached the same verdict on the two guards that matter most, from different
directions: the belt (#3) is legality-only, and the sleeper guard (#9) is the family's
strongest oracle. Neither of us read the other's notes first.

## DISAGREEMENT 1 - `a_burned_lease_never_rings_twice...` is NOT behavior-grain
C classes it receipt/behavior-grain because it asserts the second sleeper receives no bytes.
I keep class **(c)**. "Receives no bytes" is not an observation of the burn; it is the
ABSENCE of an observation, produced identically by any failure. `Sleeper::arm` returns
`(Vec::new(), Vec::new())` on BOTH the connect timeout and the read timeout
(wake_http.rs:259-297), so a dead ringer, a dead serve, or a pipe-creation race are all
indistinguishable from a correctly-refused second ring.
WRONG STATE IT CALLS GREEN: the sweep panics after the first trigger. The second `Sleeper`
times out, returns empty, guard green, doorbell dead.
A negative is behavior-grain only when something in the same fixture proves the mechanism was
alive to produce the positive. Nothing here does. Same defect as #11 and #13 (RC2).

## DISAGREEMENT 2 - `a_missing_rendezvous_consumes_the_lease...` is NOT behavior-grain
C says it waits for the consumption to actually land, which is true and is why it is (c)
rather than (a). But the wait is on `kinds_after(...)` - event TYPE strings with the payload
discarded (wake_http.rs:231-257). The guard's subject is a STALE cleanup, and the field that
makes a cleanup stale rather than a ring is `reason`, which is exactly the field the detector
cannot see.
WRONG STATE IT CALLS GREEN: the cleanup is recorded with `reason: Rung`. The store then
asserts a ring that never happened, for a session whose pipe never existed.

## PARTIAL - `an_append_beyond_the_cursor...`: C is right about a different assertion
C praises the technique and is correct: snapshotting the store from INSIDE the sleeper's read
completion measures the ordering claim itself instead of a proxy, and it is the best
measurement technique in the family. My (c) is not about that assertion. It is about the
SECOND half of the same test, which polls `kinds_after` for the consumption and so cannot see
reason, session, or count. The guard measures its ordering claim at fine grain and its
consumption claim one floor up. Both readings stand; they are about different lines.

## NEW, from C, verified here - and it is a PRODUCT hole, not a test hole
C states the waiter never consults the receipt (`wake_wait.rs:85-101`). CONFIRMED at branch
by reading: `wake_wait.rs` mentions `wake_last_consumed` ZERO times. `read_own_lease` reads
`projection.wake_leases.get(session_id)` (:116) once, BEFORE blocking, and the timeout path
(`matured`, :85-101) answers `timedOut: true / matured: true` from the lease alone.
Consequence: on timeout the waiter cannot distinguish
  (i) nothing happened - my lease is still live, from
  (ii) my lease was BURNED with reason `rung` while I sat on a doorbell nobody rang.
No test in this family can close that, because the surface never computes the distinction.
This is the mechanism behind the two-surfaces-disagree state: the operator reads "deadline
passed, nothing happened" while the store's receipt for that same session says `rung`.
MINIMAL CLOSE (product, not test): on the timeout path, re-read the projection and, if
`wake_last_consumed[session]` exists with a sequence above the arming, answer with the receipt
instead of with `matured`. Owner/PRD call, out of my lane - recorded, not proposed.

## NEW, from C - the family's best oracle is its least trusted one
C reported #9, which this audit called the strongest guard in the family, as A's flake at
roughly 3 in 4. CORRECTED FROM H'S PER-RUN DATA IN THE FINAL SECTION: the measured rate is 9/10
isolated and 3/3 in-suite (12/13), which is not a flake but a near-deterministic red with one
stable failure form. The "3 in 4" figure matches nothing in H's table. That is a structural finding, not a scheduling detail: #9 is the ONLY
end-to-end guard asserting `lastConsumed.reason == "rung"` with the burn sequence. If it is
ever quarantined, retried-until-green, or marked ignore, the family keeps its blind guards and
loses its only sighted one. RECOMMENDATION for the fix phase: whatever happens to #9, it must
not be weakened into a retry - the flake is in its harness, not in its oracle, and the oracle
is the asset.

## COMPOUNDING - what the two inventories together do to the #55 proof
Read commit `0df65da` (the belt's own fix commit, #56). It records, honestly, that the belt's
window "was too narrow to reproduce the race under test timing - the deterministic test is the
guard's proof". Put that next to this audit:
- the belt (#3) never reproduced the race AND cannot see a sweep that consumes nothing;
- so the whole #55 proof rests on the deterministic red, #1;
- and #1 asserts `recorded == 0` plus replay legality, which cannot separate "the guard
  dropped the stale capture" from "the recorder failed before reaching the guard" - eleven of
  its twelve `return 0;` sites are unrelated to the property.
Neither guard is worthless, but the chain has no link that fails when the recorder dies. That
is one sentence the owner report should carry, and it is stronger than either inventory alone.

## ON C'S OWN TYPED REDS (C asked to hear disagreement before they land)

**`a_rival_consume_between_validation_and_the_sequence_pin_appends_nothing`** - C calls it
count-grain plus legality and argues that is acceptable because window 3's failure is LOUD (it
corrupts). As a RED, agreed without reservation: the defect is illegal by construction, the
legality oracle sees it, and a red that fails today for the stated reason has done its job.
My disagreement is about its SECOND life. The moment the fix lands, this guard stops being a
red and becomes a regression guard, and as a regression guard it inherits #1's defect exactly:
it will stay green if `record_consumptions` breaks anywhere in its eleven unrelated
`return 0;` paths. The failure it was built to catch is loud; the failure that will silently
retire it is not.
MINIMAL CLOSE, one line, no design change: after `recorded == 0`, replay and assert the
RIVAL's consumption is the last word - `wake_last_consumed[session].sequence == <the rival's
sequence>`. A dead recorder cannot satisfy that, because the rival's own append still has to be
on record. It costs one assertion and it survives the fix.

**`a_stale_capture_never_burns_the_lease_that_replaced_it`** - agreed, no objection. Its
second assertion is the right shape for the right stated reason.

**`.factory/c-agent-rdv-equal-red-draft.rs`** - agreed, and it is the model this audit used.
One note, not an objection: its receipt-grain assertion is on `wake_leases` (the replacement
lease survives). It cannot also assert WHICH arming a burn belonged to, because the receipt
carries no such field - RC4. That is a limit of the store, not of the test.

## C's requested column
Already present above: the "Blind spot named" column plus the per-row legal-but-wrong section.
C's framing is worth recording as the rule behind it - a guard where nobody can name a wrong
state it would call green is either genuinely tight or measuring nothing, and which one it is
deserves the sentence. Under that rule the family's tight guards are #7, #8, #9, #10, #12,
#14, #16, #17, #18; the ones measuring less than their names claim are #1, #2, #3, #4, #5, #6,
#11, #13, #15.

## Status of this addendum
Read-only, like the audit. Nothing run, nothing staged, no code touched. The `wake_wait.rs`
and `0df65da` claims above were verified by reading at the refs named; everything else remains
a reading of assertion text, and the predictions in the audit stand unrun.

---

# OBSERVED - the predictions were run (by C, not by me)

Two predictions registered in the audit above have been MEASURED. C ran them; I did not run
anything. Everything in this section is C's reported raw output, recorded here because this
file is the family's reference doc and it must not keep calling a measured fact a prediction.

Provenance: results reported by C Agent over the pair channel. I did not observe the runs.
By my own evidence rule (now a board rule) a sabotage report owes: sabotage applied
(file:line + what changed), exact invocation, N, raw per-guard pass/fail. C supplied the
sabotage and the raw per-guard list; the exact cargo invocation and N-per-condition were not
in the first message. Both were supplied on request and are recorded verbatim in the CITATIONS
section at the end of this file, so the results below are cited, not pending.

## OBSERVED 1 - the belt is hollow (audit entry #3, S0)
Sabotage: the sweep's call to `record_consumptions` deleted. The belt run alone.

    test concurrent_sweeps_never_double_consume_a_lease ... ok
    test result: ok. 1 passed; 0 failed; finished in 37.94s

The guard named `never_double_consume` is GREEN with the recorder gone entirely. The
prediction registered above is CONFIRMED. This is no longer a reading of assertion text; it
is a measured property of the family's flagship belt.

## OBSERVED 2 - the twelve-return-0 finding is a property of the guard, not a hypothesis
Sabotage: the recorder returns 0 immediately (an unrelated bail, upstream of the guard).

    a_live_lease_consumption_still_records ....................... FAILED (left 0, right 1)
    a_rival_consume_between_read_and_record_appends_nothing ...... ok
    a_rival_consume_between_validation_and_the_sequence_pin ...... FAILED ("the rival's
                                                                    consumption is on record")
    a_stale_capture_never_burns_the_lease_that_replaced_it ....... ok

Audit entry #1 (`a_rival_consume_between_read_and_record_appends_nothing`) stays GREEN with
the recorder completely dead. Predicted above from the twelve `return 0;` sites; now observed.
C's window-3 guard fails ONLY because it carries the minimal close proposed above (the rival's
burn must be the last consumption on that session). Without that one assertion it would have
been green too - so the close is load-bearing, measured, not argued.

## CORRECTION - my "chain" sentence was too strong, and the measurement is what corrects it
I wrote, and the orchestrator routed to the owner report: *"the chain proving #55 has NO link
that fails when the recorder dies."* OBSERVED 2 shows that is wrong as written.
`a_live_lease_consumption_still_records` (audit entry #2) FAILED under the dead-recorder
sabotage - left 0, right 1. It is part of the #56 guard set (commit `0df65da` names it: "A
live lease still records normally"), so it is part of the chain, and it does fail when the
recorder dies.

The corrected sentence, which the measurement supports exactly and which is the sharper claim
anyway:

> The chain proving #55 has exactly ONE link that fails when the recorder is DEAD - entry #2 -
> and that link asserts a COUNT (`recorded == 1`). So the chain has no link at all that fails
> when the recorder is WRONG: a recorder that consumes the wrong lease, or writes the right
> lease with the wrong reason, satisfies every guard in the set.

Dead is the failure mode the chain can see. Wrong is the failure mode #55 is about, and the
one the rdv-equal burn actually produces. That distinction survives the correction and is
stronger with it: "no link fails at all" was a claim about a broken recorder; "no link fails
when the recorder is wrong" is a claim about a WORKING recorder doing the wrong thing, which is
the product failure the milestone exists to prevent.

Both the belt's per-round identity close and the receipt's arming-sequence field (RC4) attack
the WRONG axis, not the DEAD axis. Entry #2's minimal close above (assert the stored reason,
not the count) is the cheapest step onto that axis and needs no schema change.

## Status of the audit's other predictions
Unchanged and still unrun: every per-guard legal-but-wrong outcome for entries #4, #5, #6, #11,
#13, #15 remains a reading of assertion text. Two of eight predictions have been measured; six
have not, and this file does not claim otherwise.

---

# CITATIONS for the OBSERVED section - supplied by C, recorded verbatim

The two fields my own evidence rule owed are now here, so the OBSERVED results above are no
longer OBSERVED-PENDING-CITATION. Runs were made by C, not by me.

**Machine and base.** C's worktree `c-agent-e82f40`, branch `c-study-arming-the-alarm`, base
`53d212d`. NOTE THE FRAME: the audit's line refs are at `efd85d0` (main); these runs are at
`53d212d` plus C's own edits. Same guards, different base - which is why the sabotages below
are recorded BY CHANGE rather than by line number.

**N = 1 per condition.**

**Invocations.**

    # four wake guards
    cargo test -p graphhelm-cli --bins serve::wake::tests -- --test-threads=1
    # full CLI unit suite
    cargo test -p graphhelm-cli --bins --locked
    # belt alone (S0)
    cargo test -p graphhelm-cli --test wake_http --locked concurrent_sweeps_never_double_consume_a_lease
    # lint gate
    cargo clippy --workspace --all-targets --locked -- -D warnings

**Incantation note, from C, worth a line here because it cost a run:** `--lib` FAILS on this
crate - `graphhelm-cli` is a bin crate and cargo answers "no library targets found". Use
`--bins`.

**Sabotages, by change (lines moved under C's own edits).**
- S0: in the sweep, the phase-3 `spawn_blocking(record_consumptions(...))` replaced with a
  discard of its arguments.
- S6: `if true { return 0; }` inserted at the top of `record_consumptions_inner`, before the
  store open.
- S3: in the still-live filter, rendezvous-equality replaced with
  `projection.wake_leases.contains_key(&lease.session_id)`.
- S1': in `record_consumptions_inner`, the pinned `next` (taken from the decision's own history
  read) replaced with a `store.next_sequence(...)` call placed AFTER `after_validation()` - the
  pre-fix two-read shape.

## DOWNGRADE, at C's request - and the distinction that survives it

C flags S0 as the weakest cell: N=1 supports "the belt CAN pass with the recorder deleted", not
"the belt ALWAYS passes in that state", and asks that this file under-claim rather than over-
claim. Adopted. Wherever the OBSERVED section reads as the stronger version, the claim is the
weaker one.

But the two halves of that finding have DIFFERENT evidence, and collapsing them into one
N-limited claim would under-claim in the other direction:

- **The RUN outcome is N=1.** One green run establishes non-detection and nothing more. The
  belt sits in a known-flaky family; a later run could fail for harness reasons entirely
  unrelated to the recorder, and that would not be the oracle catching anything.
- **The ORACLE blindness is structural and needs no N at all.** With the recorder gone, no
  `wake_lease_consumed` event is ever written. `consumed = 0` and `0 <= armed` holds for every
  possible value of `armed`; and no illegal event can be written by a component that writes
  nothing, so `ok == true` holds every round. Both assertions are satisfied BY CONSTRUCTION,
  not by luck. That half is provable by reading and C's run is a confirmation of it, not its
  only support.

So the honest pair of sentences, and what this file claims:
1. The belt's oracle CANNOT detect a deleted recorder - structural, by construction, no N.
2. The belt was OBSERVED green in that state once - N=1, C's run, which confirms 1 rather than
   establishing it.

A future red belt run under S0 would therefore NOT refute finding 1. It would mean the harness
flaked - and, usefully, it would be the family proving my RC2 point about negatives on itself.

---

# FIXTURE BLINDNESS - the belt's second blind spot (found by C, verifying my split)

C traced my by-construction argument against the belt's real fixture, confirmed both halves,
and found something the audit above did not draw out. Credit is C's; the consequence for the
proposed closes is worked out here.

## The finding
`arm_lease` hardcodes `session_id = "session-sleeper-1"` and the belt varies only the
rendezvous (`rdv-race-{round}`). All 15 rounds therefore arm the SAME session, and the fold
REPLACES on re-arm (entry #14 pins that invariant). So an unconsumed lease never accumulates:
round N's arming silently overwrites round N-1's.

**The belt's fixture destroys its own evidence every round.** Even an oracle that inspected the
live-lease set at the end would find at most ONE stale lease instead of fifteen.

So the belt is blind twice over, and the two are independent:
- **oracle blindness** - `ok == true` plus `consumed <= armed` cannot see a missing consumption
  (audit entry #3, observed under S0);
- **fixture blindness** - single-session re-arming erases the evidence a better oracle would
  read.

Fixing either one alone leaves the other standing. That is the thing for whoever upgrades the
belt, and it is why the upgrade is bigger than one assertion.

## Correction to C's consequence, and it cuts in my own proposal's favour, so state it carefully
C writes that a per-round identity assertion alone would not fix this while the fixture reuses
one session. For IDENTITY assertions that is exactly right. But the close this audit actually
proposed for entry #3 is NOT an identity assertion - it is a receipt-SEQUENCE advance:

    per round: assert wake_last_consumed["session-sleeper-1"].sequence > previous_round_sequence

That close survives single-session reuse, because `wake_last_consumed` is last-wins per session
and the sequence is strictly monotone per stream. If round 5 consumes nothing, round 5's check
sees the same sequence round 4 left behind, and goes red - reuse or no reuse. So:

- to DETECT a round that consumed nothing: the sequence-advance close is sufficient today, no
  fixture change, no schema change;
- to ATTRIBUTE a consumption to a specific arming: distinct sessions are necessary, and the
  arming-identity field (RC4) on top of that.

Both are worth having, and they are not the same job. Saying "the fixture must change first"
would delay the cheap detector behind the expensive attributor.

## The distinct-session upgrade earns something extra, for free
If each round arms a DISTINCT session (`session-race-{round}`), unconsumed leases stop
overwriting each other and accumulate. That hands the belt an end-of-run oracle it cannot have
today, with no schema change and no new event field:

    at the end: projection.wake_leases must contain no session-race-* key
    (every armed lease of the run was consumed)

Under S0 that assertion is red immediately - fifteen leases stand unconsumed.
CORRECTED BELOW: this is a WEAK end-of-run backstop, not the strongest oracle - the sweep
repairs its own misses, so it sees total failure only. See the self-repair correction at the
end of this file before acting on this paragraph.

## Sharpening taken, from C
My sentence "a component that writes nothing cannot write an illegal event" is true but
narrower than the situation. Other writers (the signal mutations) ARE still writing under S0.
The precise form, which is what this file now claims: **S0 removes the only writer capable of
producing the illegal event, and every remaining writer produces legal ones.** Same conclusion,
no gap for a reader who notices the other writers.

C also enumerated what could actually redden the belt under S0 - the `assert_eq!(status, 200)`
on the POSTs, the `ok == true` read, and the journal count. The last two hold by construction,
so a red could only come from the mutation or the status read failing: transport, storage,
environment. None of those is the belt detecting a missing recorder. My extension stands as
written.

## Disposition of entry #2 - B ruled, it SHIPS
B ruled the entry #2 close IN SCOPE for PR 1, in the same commit as the S4 outcome, and adopted
C's self-imposed condition as a REVIEW REQUIREMENT rather than a courtesy: the wrong-reason
sabotage must be observed felling the new assertion before it lands. So the assertion is not
taken on argument - it has to be watched failing first, which is this audit's own standard
applied to this audit's own proposal.

B sharpened the grain further and C took it: assert `wake_leases.get("session-l").is_none()`
rather than the map being empty. Empty is one future fixture away from passing or failing for a
reason that is not the property; absent-by-key IS the property. That is a finer grain than this
audit proposed, and it is the correct one - recorded so the reference doc carries B's version,
not mine.

Status: edit in, uncommitted and unverified; A holds the cargo slot. Raw lists to follow. The
audit's entry #2 close therefore needs NO seed - it ships.

---

# CORRECTION - the accumulate check is NOT the strongest belt oracle (C, verified here)

The section above filed C's distinct-session upgrade with an end-of-run check ("no
`session-race-*` key remains") and called it the strongest belt oracle available without RC4.
That label is WRONG. C caught it; I verified the mechanism in source before accepting it.

## Verified in source
`serve/wake.rs` phase 1 builds the due list as

    projection.wake_leases.iter().filter(|(_, lease)| lease.cursor < content_head)

- EVERY live lease past the content head, not this round's. `content_head` only grows, and the
belt arms every round at cursor 1. So each round's sweep sees ALL outstanding leases from ALL
previous rounds and consumes them.

Consequence: round 3 consumes nothing (the regression). Round 4's signal POST fires a sweep;
that sweep finds round 3's lease still due, rings it, consumes it. The residue is CLEANED UP by
the next round. End of run: zero leftover keys. GREEN, with round 3 having failed.

**The sweep repairs its own misses.** The accumulate check therefore detects only TOTAL failure
(S0, where nothing consumes ever) and is blind to a transient miss - which is the shape an
actual regression is most likely to take.

## Corrected relationship: complementary, not ranked
- **Per-round sequence advance (this audit's close): the DETECTOR.** Catches a single round
  that consumed nothing, INCLUDING one a later sweep repairs, because it checks at the end of
  the round rather than the end of the run. Strictly the sharper of the two.
- **Distinct sessions (C's upgrade): the PREREQUISITE for attribution.** Without them there is
  nothing to attribute a burn to. That is its real value, not the end-state check.
- **The accumulate check: a weak end-of-run backstop** that a self-repairing sweep defeats.
  Worth having, worth knowing the limit of, not worth reaching for first.

Whoever upgrades the belt should take the sequence advance FIRST. The label this file carried
before would have sent them at the weakest of the three.

## THE GENERAL RULE, which is the part worth keeping
This is bigger than the belt, and it is the sharpest thing to come out of the whole exchange:

> **In a subsystem that repairs its own faults, an END-STATE oracle measures the repairer, not
> the fault.** Only a per-event or per-window oracle can see a miss that something later heals.
> A green end state is evidence the healer works, and evidence of nothing else.
> REFINED IN THE FINAL SECTION OF THIS FILE: the precise form is that a green end state is
> evidence for the DISJUNCTION (no-fault OR fault-plus-repair), never for either branch. Read
> that section before quoting this sentence.

The wake sweep is such a subsystem by design - a missed lease is picked up by the next content
append, which is exactly the accelerator-never-correction property the milestone wants. The
same property that makes the product forgiving makes end-state testing of it worthless.

## CONVERGENCE with E Agent's story finding (noted from the board, not verified by me)
The board records E's confirmed kill on the second-story spec: a hung tool self-heals - 300s
timeout, `RetryableFailure`, auto-requeue, attempt 2 sees the marker and succeeds - so the
judge realistically loses the race, the miss is SILENT, and 6/7 degrades to 4/7 with nothing
reporting a fault.

Same shape, different subsystem: **self-repair converts a fault into a silence, and every
end-state oracle reads that silence as success.** E found it in the story harness, this audit
found it in the wake sweep. That is two independent instances of one rule, which suggests it
belongs in the milestone's own vocabulary rather than in two separate agent notes. Flagged for
the orchestrator; not mine to place.

## Ownership
Neither C nor I is touching `wake_http.rs` - it is A's file this milestone. Everything in this
section is analysis for whoever takes the belt upgrade.

---

# THE SELF-REPAIR RULE, final form (N + C, with C's three refinements)

The section above stated this rule loosely and leaned on E's instance without either C or me
having checked it. C called that out - two unverified instances must not be merged into a rule
that inherits confidence from both. Correct, and acted on: I verified E's MECHANISM in source
before letting it hold any weight. What follows is the rule with C's refinements folded in and
the evidence for each leg named separately.

## The rule

> **A fault followed by automatic repair produces an end state indistinguishable from no fault.**
> An end-state oracle therefore measures the REPAIRER, not the fault.
>
> The operative condition is a latency comparison:
>
>     repair latency < observation window  =>  the oracle is blind to the fault
>
> So the instruction is not "avoid end-state oracles" but **observe at a grain finer than the
> repair latency.**

C's refinement 1, and it is what makes the rule usable. It is exactly why per-ROUND checking
works on the belt and per-RUN does not: the per-round check shrinks the observation window
below the repair window. A reader told "avoid end-state oracles" cannot act; a reader told
"observe finer than the repair latency" can.

## Why this shape gets built and kept (C's refinement 2)
The oracle is NOT uniformly blind. If the REPAIRER breaks, the end state goes red and the
oracle catches it - loudly. So it detects faults in the healer while missing faults in the
healed: it measures one component while appearing to measure the system, and it demonstrates
that it "works" often enough to survive review.

That asymmetry is the reason a legality-shaped guard, or an end-of-run check, gets written by
careful people and kept by careful reviewers. The rule is much weaker without this half,
because without it the shape looks like carelessness, and it is not.

## What a green end state actually licenses (C's refinement 3)
My earlier form - "evidence the healer works and evidence of nothing else" - overshoots. The
precise statement:

> A green end state is consistent with BOTH *no fault* and *fault plus repair*, and cannot
> separate them. It is evidence for the DISJUNCTION, never for either branch. It additionally
> bounds the fault as having been within repair capacity.

Same conclusion, one grain finer, and the finer version is the one that survives an argument.

## Evidence, per leg, stated separately

**Leg 1 - the wake sweep. VERIFIED BY ME, in source.** `serve/wake.rs` phase 1 builds
`due = wake_leases.iter().filter(|(_, lease)| lease.cursor < content_head)` - every live lease
past the content head, not the current round's - and `content_head` only grows. A round that
consumes nothing is repaired by the next round's sweep. Found by C, mechanism confirmed by me
at the filter.

**Leg 2 - the executor's retry path. MECHANISM VERIFIED BY ME; E's MEASUREMENT NOT.** The
board attributes to E a confirmed kill in which a hung tool self-heals and the miss goes
silent. I checked the mechanism rather than cite E's conclusion:
- `core/runtime/src/executor.rs:241-242` - `ToolDisposition::TimedOut => NodeOutcome::RetryableFailure`
  (and `HostError` likewise at :246);
- `core/execution/src/bounds.rs:10` - `MAX_IDENTICAL_OUTCOMES = 3`, enforced at
  `core/execution/src/progress.rs:43`, so a node retries and only BLOCKS once the same outcome
  repeats three times.
So a timeout becomes a retryable failure and is retried automatically; if attempt 2 SUCCEEDS,
the outcomes are not identical, nothing blocks, and the run ends clean. The silence comes from
a successful second attempt - exactly the shape of leg 1.
NOT verified by me: E's specific measurement (the judge losing the race, 6/7 degrading to 4/7).
That is E's result, cited as E's, and the rule does not rest on it.

So the rule stands on two legs whose MECHANISMS are both verified in source, in two unrelated
subsystems, by me, at the refs named. E's numbers remain E's.

## The rule bites C's own fix, and C is the one who said so
C's window-3 fix deliberately ADDS self-repair surface: a consumption dropped by the stale pin
is retried by the next mutation's sweep, which is the benign-drop argument in the commit body
that B and the orchestrator accepted. Under this rule that carries a consequence for whoever
tests the drop path next: it must be observed PER EVENT, because an end-state test of it would
measure the retry and call the drop correct regardless of what the drop did.

C checked their own guards against this before reporting it, and they survive - they observe a
single recorder call and assert on that call's outcome plus the receipt it left, which is
per-event, not end-state. Recorded here because it is the rule's first live application, it was
applied by its co-author against their own landed reasoning, and that is the strongest evidence
in this file that the rule is usable rather than decorative.

Stated plainly, as C put it: the fix made the product more forgiving and therefore harder to
test at end state. That is a real cost of the change, not only a property of the sweep's
design - and naming it is what keeps the trade honest rather than free.

## Where else this predicts blindness (unverified, offered as a search list)
Any component with retry, requeue, sweep-again, or next-append-picks-it-up semantics cannot be
tested at its end state. From what this audit touched: the wake sweep (verified), the
executor's retry path (mechanism verified), and whatever harness E's story runs on (E's lane).
Anything else is a guess and is labelled as one.

---

# CLOSING CITATIONS, and a NEW blind spot the S4 run exposed

## Citations for the last two runs (C, closing the debt)
Same frame: worktree `c-agent-e82f40`, base `53d212d` + C's edits, **N = 1 per condition**.

    # wrong-reason sabotage and the S4 guards
    cargo test -p graphhelm-cli --bins serve::wake::tests -- --test-threads=1
    # S4 belt half
    cargo test -p graphhelm-cli --test wake_http --locked concurrent_sweeps_never_double_consume_a_lease
    # final green
    cargo test -p graphhelm-cli --bins --locked
    cargo clippy --workspace --all-targets --locked -- -D warnings

Sabotages by change:
- **wrong-reason**: in `record_consumptions_inner`'s batch build, `reason: *reason` replaced with
  a hardcoded `WakeConsumeReason::StaleRendezvous`.
- **S4**: the idempotency key `format!("wake-consume-{next}-{index}-{}", session)` replaced with
  `format!("wake-consume-{}", session)` - sequence and index dropped.

Raw:

    wrong-reason -> a_live_lease_consumption_still_records FAILED (left StaleRendezvous, right Rung)
                    other three ok
    S4           -> all four ok; belt ok (27.34s). NOTHING falls.

## Entry #2's close is now MEASURED, not argued
The wrong-reason line is this audit's entry #2 proposal being felled by the sabotage it was
written for. Before the close, that guard asserted `recorded == 1` and a wrong reason passed it;
with the close, the wrong reason fells it. B required the sabotage be OBSERVED rather than
argued, and it was. Entry #2 moves from proposal to shipped-and-measured.

## NEW BLIND SPOT - the idempotency key has no guard at all (S4)
S4 degrades the consumption's idempotency key to `wake-consume-{session}` and **nothing in the
family falls**. That is a hole this audit did not cover: my inventory graded oracle GRAIN and
never asked whether the key's composition was guarded by anything. It is not.

**Mechanism, verified by me in source** (C ran the sabotage; the explanation below is my
reading, not C's claim):
- `core/events/src/integrity.rs:270-277` - the request digest covers `expected_next_sequence`
  along with the events.
- `core/events/src/local.rs:664-673` - on a colliding idempotency key, the store returns the
  ORIGINAL batch only when `batch.request_digest == request_digest` AND the key sets match;
  otherwise it returns `IdempotencyConflict`.
- Therefore a session's SECOND consumption under the degraded key lands at a later
  `expected_next_sequence`, so the digests differ, so it is refused with `IdempotencyConflict`.
- `serve/wake.rs` swallows it: `if store.append_atomic(&request).is_err() { return 0; }`. No
  event, no diagnostic, no surface. The recorder reports 0 and moves on.

**Consequence (REASONED from the above, not run - flagged as such):** for any session that is
consumed twice in the life of a stream, the second burn never lands. No consume event means the
lease is never burned, so it stays live with `cursor < content_head`, so every later content
append finds it due, rings it, and fails to record again. A ring loop with no receipt, and the
sleeper's receipt frozen at the first burn forever.

**Why no existing guard sees it:** FALSE AS WRITTEN - CORRECTED IN THE FINAL SECTION OF THIS
FILE. The belt DOES drive fourteen repeat consumptions for one session and reports green, which
makes this a DETECTION gap, not the coverage gap this paragraph claims. The rest of the
paragraph stands: the belt's `consumed <= armed` still holds (the count simply stops
growing); and legality still holds (nothing illegal is written by a write that never happens).
All three of this audit's named root causes line up behind one hole.

**Which proposed close catches it:** the per-round **sequence-advance** close does - the
receipt's sequence stops advancing the moment the second burn is refused. The reason-equality
close does NOT: the frozen receipt still carries a valid reason. That is the THIRD independent
argument for sequence grain over reason grain, and the first one that came from a run rather
than from reading.

Filed as a named seed, not a fix: it needs a guard driving two consumptions for one session
through the recorder and asserting the second one lands. Nobody's pen here - `serve/wake.rs` is
C's and the change is not C's PR.

## A claim I nearly wrote and killed by checking, recorded on purpose
My first reading of S4 was that the colliding key would take the store's REPLAY path - return
the original batch, append nothing, and let `record_consumptions` report `recorded = batch.len()`
anyway, since it computes that count before the append and only tests `is_err()`. That would
have been a much louder finding: a count that actively lies about events that were never
written.

It is wrong. `expected_next_sequence` is inside the digest (`integrity.rs:273`), so the second
attempt cannot match the first batch's digest and takes the conflict path instead of the replay
path. The count reports 0, honestly.

Recorded because it is exactly the failure mode this audit exists to name: a plausible,
mechanism-shaped, confidently-writable claim that one file read killed. The rule this file
applies to test oracles applies to its own findings.

## C's structural point, taken - and it is the argument for RULE over coincidence
The two verified legs of the self-repair rule fail for the same structural reason at DIFFERENT
LAYERS: mine at the storage layer (a sweep repairing a missed consumption on the next append),
the executor's at the execution layer (a retry policy repairing a timed-out attempt on the next
attempt). Neither is a bug in its repairer. Both turn the end state into a measurement of the
repairer.

One shape appearing independently at the storage layer and the execution layer, inside one
milestone, is the case that it is a RULE and not a quirk of one subsystem's design. That
framing is C's and it is the strongest single sentence for putting this in milestone
vocabulary.

---

# CORRECTION - it is a DETECTION gap, not a coverage gap (C, and my error contradicted my own file)

The S4 section above says "no guard in the family drives TWO consumptions for ONE session
through the recorder". **That sentence is false**, C caught it, and the worse part is that this
same file states the contradicting fact twice already.

## Verified
`arm_lease_bounded` hardcodes `session_id: "session-sleeper-1"` (wake_http.rs:221) and the belt
calls it every round. So the belt arms and consumes THE SAME SESSION fifteen times in one run.
This audit already recorded that fact in entry #3's legal-but-wrong paragraph and again in the
fixture-blindness section - and then the S4 section asserted the opposite. My error, and the
kind an audit is least entitled to make: contradicting its own evidence three sections apart.

## What that changes, and it makes the finding WORSE
Under S4 the belt does not merely touch the scenario; it executes it fourteen times over. Round
1's consumption lands under key `wake-consume-session-sleeper-1`; rounds 2..15 collide on that
key with a different digest and take the conflict path.

**And the belt passed.** C's measured S4 result: belt ok, 27.34s.

So the hole is not "nothing exercises this path". It is: **the one guard that exercises it
fourteen times reports green.** The three root causes are not lining up behind an untested path
- they are demonstrated by a run that drove the path fourteen times and saw nothing.

## The distinction, which is C's and is the operative one
- A **coverage gap** is cured by adding a test that drives the path.
- A **detection gap** is not: the path is driven, repeatedly, and the oracle cannot see the
  result.

This is a detection gap. Writing a test that "covers" a repeat consumption would fix nothing on
its own - the belt already does that. The cure is the ORACLE: assert the receipt's sequence
advances per round. Different disease, different medicine, and the audit had the diagnosis
wrong.

## Under S4 the counting oracle holds for a NEW reason, and it is the sharpest version yet
`consumed` stops at 1 and `1 <= 15` holds. Sequence-advance goes red at round 2 and stays red
for fourteen rounds. So this is the third independent argument for sequence grain over reason
grain - and the first in which the failing scenario was ACTUALLY EXECUTED rather than reasoned
about. C supplied the green; the mechanism is mine; the conclusion is joint.

## REGISTERED PREDICTION - C's, and it can kill the whole seed
Neither of us has dumped the journal under S4, so "rounds 2..15 hit the conflict path" is
inference from my mechanism plus C's green, NOT observation.

The run: apply S4, run the belt alone, count `wake_lease_consumed` events in the journal.

Pre-declared discriminating outcomes, so the result decides rather than gets interpreted:
- **1** - predicted. OVER-REACHED AS WRITTEN, corrected in the MEASURED section at the end of
  this file: a count of 1 confirms INCIDENCE only. It cannot arbitrate MECHANISM, because the
  conflict path and the replay path produce identical journals.
- **2** - does NOT refute. Round 1 fires two concurrent lanes; a second consumption there is
  explainable without touching the mechanism (though the #55 guard makes it unlikely, since the
  exclusive lock spans validate-and-append and the second sweep's still-live filter should drop
  it).
- **15** - REFUTES. Every consumption landed, so the key collision is not happening as
  described, and this correction plus the seed both die.
- **0** - refutes differently: nothing consumed at all, which would mean S4 broke the recorder
  outright rather than colliding keys, and the finding would need re-deriving from scratch.

C runs it when the machine frees; me cannot (no cargo in my lane). Pre-declaring which numbers
mean what is the point - a prediction that can absorb any result measures nothing, which is the
same disease as an oracle that can pass in any world.

## C's addition, verified: the honest count is ACCIDENTAL
My killed claim (that the recorder would report a count for events never written) is
unreachable, but C checked the other branch and is right that the code shape is genuinely there:
the replay arm returns `Ok(batch.events.clone())` (local.rs:668-671), and `record_consumptions`
computes `recorded = batch.len()` BEFORE the append and then only tests `is_err()`. So if the
digest did not happen to cover `expected_next_sequence`, the count WOULD lie.

**The recorder's count is honest because of an invariant in another crate, not because the
recorder is careful.** REFINED BY B AT THE END OF THIS FILE: honest for a REASON, not by
accident - replay is permitted only when the digest is fully equal, and in that case the replayed
batch is byte-identical to what we would have written, so "on record" and "written by us" mean
the same thing. What survives: the recorder does not know this, and nothing states the dependency. That is a fragility worth naming on its own: nothing in `serve/wake.rs`
would notice if `request_digest`'s field set changed, and RC3's typed-outcome proposal would
close it by construction rather than by luck.

---

# TABLE FIXED, and the S4 seed is TWO holes with different diseases

## C's table correction, taken
My discriminating table declared 0, 1, 2 and 15 and left **3..14 undeclared** - an open cell is
exactly where a result gets rationalized after the fact, which is the disease the table exists
to prevent. C is right, and right about the reasoning: `local.rs:661-673` refuses on ANY key
overlap (`!requested_keys.is_disjoint(&batch_keys)`) and only the arm where digest AND key set
both match returns the prior batch. Under S4 every round shares the key and every round has a
different `expected_next_sequence`, which the digest covers. So the collision is TOTAL from
round 2 - not timing-dependent, not partial.

Corrected table, C's version:

| journal count | verdict |
|---|---|
| 1 | CONFIRMS **INCIDENCE** only - see the MEASURED section: conflict and replay give identical journals, so this cell says nothing about mechanism. As originally written ("CONFIRMED") it over-reached. |
| 2 | CONFIRMED, with the round-1 two-lane race as the explanation |
| 0 | REFUTES - S4 broke the recorder outright rather than colliding keys; re-derive |
| 3..15 | REFUTES, and worse than 15 alone - a middling number means the collision happens SOMETIMES, which no part of the mechanism can produce. Mechanism wrong in a way neither of us has thought of. Re-derive from the journal, not from the argument. |

C's refinement on cell 2 taken as well: with C's fix in the tree there are now TWO independent
reasons a second round-1 consumption is unlikely - the still-live filter drops a sweep that
validated late, and the pinned sequence drops one that validated early. Both blades. So a 2
should be rare, and if it appears it is worth asking WHICH blade failed rather than shrugging.

## THE SECOND HOLE - and this one IS a coverage gap
Chasing C's "total from round 2" argument turned up a case with a different mechanism, which
neither of us had considered. **Verified in source, both halves:**

- `integrity.rs:107-113` - `validate_prepared_append` collects the batch's idempotency keys into
  a `BTreeSet` and refuses the whole request as `Invalid` when `keys.len() != events.len()`.
  Duplicate keys WITHIN one batch are rejected outright.
- `serve/wake.rs` - the recorder builds ONE batch containing a consumption per still-live lease,
  keyed `wake-consume-{next}-{index}-{session}`. The `{index}` exists precisely to keep those
  keys distinct inside the batch.

So under S4, which drops BOTH `{next}` and `{index}`: a sweep that finds **two or more due
sessions at once** builds a batch with duplicate keys, the store refuses the entire batch as
`Invalid`, and `record_consumptions` swallows it (`is_err() -> return 0`). **Every consumption
in that sweep is dropped together - including the first-ever burn of a session that had never
been consumed before.**

That is strictly worse than the repeat-consumption case, and it has a different shape: the
repeat case loses one session's later burns; this loses ALL sessions in the sweep, immediately.

**No fixture in the family can reach it.** Verified: the only ARMED session anywhere in
`wake_http.rs` is `session-sleeper-1`; `session-nobody` (:574) and `session-somebody-else`
(:1367) are deliberately UNARMED, used by the refusal guards. The in-module tests use one
session each (`session-l`, `session-p`, `session-r`), one per tempdir. Nothing in the wake family
ever produces two due leases in a single sweep.

**This one is a genuine COVERAGE gap** - the counterpart to the detection gap above, and the
distinction C drew is what makes them separable:
- repeat consumption for one session: the belt DRIVES it fourteen times and cannot see it ->
  detection gap -> cured by the oracle (sequence advance);
- two due sessions in one sweep: nothing drives it at all -> coverage gap -> cured by a fixture
  that arms two sessions on one execution.

And the multi-session shape is not exotic. It is this factory's own daily state: several agent
sessions armed on one execution, which is what the pair loop does.

## What that does to the key's design, precisely
`{next}` and `{index}` are not redundant with each other - they defend different collisions:
- `{next}` defends ACROSS batches: the same session consumed again at a later sequence;
- `{index}` defends WITHIN one batch: several sessions consumed by one sweep.

S4 removed both, and only the first is reachable by any existing test. So the sabotage's
"nothing falls" result under-reports its own damage: it measured the loss of `{next}` and could
not have measured the loss of `{index}` at all.

## Seed, restated with both shapes
The seed now needs TWO guards, not one:
1. drive two consumptions for ONE session through the recorder and assert the second LANDS
   (detection - the oracle is the receipt's sequence);
2. arm TWO sessions on one execution, make both due in a single sweep, and assert BOTH
   consumptions land (coverage - nothing reaches this today).
Still nobody's pen here: `serve/wake.rs` is C's file and neither guard is in C's PR.

## C on RC3
C calls "the recorder's count is honest because of an invariant in another crate, not because
the recorder is careful" the strongest sentence in this file, and adds the generalisation: the
dependency is stated NOWHERE in either crate. That makes it a coupling with no name and no test
- the same species as the atomicity claim this whole PR started from, which was also a
true-sounding sentence standing in for a guarantee nobody had written down. Recorded as C's
framing.

---

# H'S BASE MEASUREMENTS - the audit's thesis, measured by someone who was not testing it

Two numbers for the sleeper guard's failure rate reached me across this exchange - "~3 in 4"
early, "9/10" later. Disagreeing measurements go to the source that normalises nothing, so I
read H's per-run table rather than choosing between two summaries.

Source: `.factory/h-agent-base-measurements.md`, H's base measurement at `53d212d`. Read by me;
the runs are H's.

## The numbers, from H's table

| guard | isolated | in-suite | distinct failure forms |
|---|---|---|---|
| `the_storm_holds_under_eight_concurrent_agents` | 4/10 | 2/3 | two (read-phase 10060 always, get_status 10060 sometimes) |
| `a_sleeper_wakes_on_a_peer_append_with_zero_requests_in_the_window` | **9/10** | **3/3** | **ONE** (wake_http.rs:822:5) |
| `concurrent_sweeps_never_double_consume_a_lease` | **0/10** | **0/3** | none observed |

The "~3 in 4" figure matches nothing in H's data and should not be quoted; 9/10 isolated and
3/3 in-suite are the measured values. H's own note: at 12/13 total the sleeper "is not behaving
as a flake at this base - it is near-deterministically red, isolated and in-suite, with a single
stable assertion form. Only isolated run 7 passed."

## The failure payload, and why it lands on this audit
H quotes the failing assertion exactly, stable across all twelve failures modulo timestamp and
session id:

    assertion `left == right` failed: the lease burned on the ring:
    {"contentHead":15,"cursor":13,"head":15,"lastConsumed":null,"live":true,...}
      left: Bool(true)   right: false

The lease reads back **`live: true` with `lastConsumed: null` after the ring**. In the guard's
own vocabulary: the consumption did not land.

**This is the audit's thesis measured by someone who was not testing the audit's thesis.** In
the same suite runs where the sleeper reports a consumption that did not land, the belt
(`concurrent_sweeps`) passed 13/13 - 0/10 isolated, 0/3 in-suite. The one guard in the family
that asserts at receipt grain (`live == false`, `lastConsumed.reason == "rung"`) is the only one
reporting anything at all, and the guards around it - which this audit graded (a) and (c) - are
green in the very runs where a burn is missing.

## What I am NOT claiming
Whether the missing consumption is a PRODUCT fault (the burn genuinely never lands) or a HARNESS
fault (the test reads before the burn lands) is not settled by this payload, and it is A's lane,
not mine. I take no position.

The audit-relevant claim does not depend on which it is: **the guard that can distinguish
"burned" from "not burned" is the one producing signal, and the blind guards cannot tell which
world they are in.** That holds under either root cause.

## What it does to the oracle-preservation constraint
The constraint now on the board ("A's fix repairs the harness, never weakens the oracle") stops
being a principle and becomes concrete: `wake_http.rs:822` is, right now, the only assertion in
the wake family reading a fault. Weakening it - retry-until-green, `#[ignore]`, or relaxing
`live == false` / `lastConsumed.reason` - would not quiet a flake; it would remove the suite's
only working instrument while the reading it produces is still unexplained. That is a stronger
argument for the constraint than the one I gave when I proposed it, and it comes from H's data,
not from my reasoning.

## What it does to C's gate observation
C reported one wake_http pass on the milestone branch (C's fix in, A's flake-2 fix out) and
flagged it weak on purpose rather than claiming it as support. H's data sharpens the arithmetic:
against 12/13 failures at base, a single pass is a ~1-in-13 event, not 1-in-10. C's refusal to
quote it as their prediction coming true is correct, and by this file's own standard it is the
same shape as everything else here - **a green consistent with both "the fix worked" and "the
1-in-13 came up", and unable to separate them.** Refinement 3 of the self-repair rule, applied
to a gate result rather than to a test oracle.

The orchestrator's N>=10 double-duty run is what scores it. Nothing before that does.

## And the belt's 0/13 is now doubly explained
This audit predicted structurally that the belt cannot see a consumption that fails to land.
H measured the belt at 0/13 in the same window where the sleeper reports exactly that class of
failure twelve times. The belt's clean sheet is not evidence the sweep is healthy; it is
evidence the belt is not looking. Two independent supports for that now: the S0 sabotage (green
with the recorder deleted) and H's base measurement (green while the sighted guard reads a
missing burn).

---

# THE STALE NUMBER HAS A SOURCE, and it is a committed doc

C traced the "~3 in 4" they passed me: it is not C's estimate. It comes from
`docs/milestones/m09-seeds.md`, seed 9, and C's study inherited it verbatim. Verified by me at
the file:

    docs/milestones/m09-seeds.md:99-101
    "...fails by assertion - not the transport-level timeout of the known flake family - at
     roughly three runs in four, and it fails at the parent commit too."

So the figure is not a slip in a message. It is a number in a durable, committed artifact, and
the next planner inherits it exactly as C did.

## The precise claim, stated so it is defensible
H measured 9/10 isolated and 3/3 in-suite at `53d212d` - 12/13, with invocation, N, base and
raw per-run table recorded. Seed 9's "roughly three runs in four" carries NO N, NO base commit
and NO invocation.

I am NOT claiming the doc's figure was wrong when it was written - rates can differ across
commits, and I do not know what base it was taken at, because the doc does not say. The claim
is narrower and does not depend on that:

> Under the board's own evidence rule, seed 9's rate is UNCITEABLE - it is missing the three
> fields every sabotage report owes. H's figure carries all of them. Where they disagree, the
> one with provenance wins, and the one without should not be propagated.

That is the same rule the board applied to agent reports, applied to a committed doc.

## The second claim in the same sentence
Seed 9 also states "and it fails at the parent commit too" as settled fact. Whether the SLEEPER
was measured at the parent I do not know. What I can see is that H's base-measurement mandate
was base-only, and H explicitly records the parallel case as open: "C Agent's registered
prediction targets the PARENT of 53d212d - untested here (base-only mandate)." So at minimum,
a parent-commit claim in that doc is not corroborated by the measurement run that exists, and it
reads as settled when the scheduled work to settle it is still scheduled.

## The part worth the orchestrator's attention
The paragraph immediately ABOVE seed 9, closing seed 8, reads (m09-seeds.md:92-93):

    "A finding we act on because it is right must not also teach us to trust numbers nobody
     checked."

Seed 9 then states an unchecked number, and that number travelled - into C's study, into C's
message to me, into my audit, and it was still moving when H's per-run table stopped it. The
doc diagnosed the disease one paragraph before demonstrating it, which is not a criticism of
whoever wrote it so much as the strongest available evidence that the rule needs a mechanism
rather than a sentence.

## Disposition
Not my pen: `docs/milestones/` is shared and this audit does not widen into it. C reached the
same conclusion independently and flagged it rather than quietly fixing their own copy. Recorded
here with the refs so whoever owns the doc can act on evidence instead of on two agents' word.

## Gate, for the record (C's run, C's report)
C confirmed the PostgreSQL matrix ACTUALLY RAN - throwaway cluster on 127.0.0.1:57292, 43
Postgres tests, all passing - so it is a full gate rather than a gate with an unrun stage. C had
committed in advance to saying explicitly if it could not run, and then said explicitly that it
did. Recorded because a promise to report an absence is only worth anything when the presence
gets reported in the same terms.

C also notes `constructor_bounds_reconciliation_catalog_locks` passed at 86.44s - the OTHER
registered flake in issue #19, whose 2-second budget was suspected of eloping under load - and
declines to draw anything from N=1 beyond "it did not fire here". Same standard C applied to
their own wake_http pass, applied again unprompted.

---

# THE MECHANISM HALF NEEDS NO SABOTAGE AND NO PEN - and the invariant it rests on is untested

C reports the S4 journal count blocked: it needs `serve/wake.rs` (to apply the sabotage) and
`wake_http.rs` (to make the belt's count visible before the tempdir dies), and both are A's.
That is correct for the COUNT. It is not correct for the MECHANISM, and the two halves of my
seed can be settled separately.

## What the seed actually rests on
Two claims, and only one of them needs the belt:
1. **MECHANISM** - under a key that drops `{next}`, a session's second consumption diverges from
   the first by `expected_next_sequence` alone, so it takes the CONFLICT path rather than the
   store's replay path, and is refused.
2. **INCIDENCE** - the belt drives that collision fourteen times and reports green.

Claim 2 needs the journal count and therefore A's files. Claim 1 is a property of the STORE. It
can be settled in `core/events`, with no sabotage anywhere, no wake file touched, and N=1
sufficient because it is deterministic.

## What the store's own tests already pin - and the gap
`core/events/tests/local_atomicity.rs:231`,
`exact_retry_is_resolved_before_sequence_and_divergent_reuse_fails_closed`, covers the
neighbouring case. Read at the file:
- an EXACT retry (same request object) returns the first batch verbatim - the replay path;
- a DIVERGENT reuse fails closed with `GHE003_IDEMPOTENCY_CONFLICT`, and the stream is unchanged.

But the divergent request there varies the EVENT CONTENT (`Sensitivity::Restricted`) while
keeping `expected_next_sequence` at 1 (:238-246). So the guard pins divergence-by-content.

**Nothing pins divergence-by-sequence-alone** - FALSIFIED BY MEASUREMENT, see the final
section: C sabotaged the digest blind to sequence and FIVE pre-existing guards fell besides the
neighbour. The claim was true of the NAMES and false of the COVERAGE. Read the correction before
citing this paragraph. As originally written: same key, same events, different
`expected_next_sequence`. That is precisely the property my mechanism claim depends on, and
precisely the property that makes the wake recorder's count honest (the earlier finding: the
count is honest because of an invariant in another crate, not because the recorder is careful).

So the invariant `serve/wake.rs` silently depends on is untested in the one dimension that
matters to it.

## The pen-free run this suggests (proposal, not a claim on anyone's file)
A test in `core/events/tests/local_atomicity.rs`: append a batch under key K at
`expected_next_sequence` 1; then append the SAME events under the SAME key at
`expected_next_sequence` 2; assert `GHE003_IDEMPOTENCY_CONFLICT` and that the stream is
unchanged.

- Touches neither `serve/wake.rs` nor `wake_http.rs` - no collision with A.
- Needs no sabotage: it tests the store's real behaviour, not a degraded key.
- Deterministic, so N=1 is the whole measurement.
- Settles seed claim 1 outright, and closes the untested dimension of an invariant another crate
  already leans on.

If it comes back as a REPLAY rather than a conflict, my mechanism is wrong, my S4 explanation
collapses, and the killed-claim section of this file becomes the live one instead. That is the
outcome I would most want to know about, and this run is the cheapest way to get it.

Not my pen and not a request: `core/events/tests/` is unassigned on the board as far as this
audit can see, and bench does not claim files. Recorded as an option so the seed is not held
entirely behind a queue it does not need to be behind.

## Status of the seed after this
- claim 1 (mechanism): settleable now, pen-free, deterministic. UNRUN.
- claim 2 (incidence, the 14 collisions): still blocked on A's files, still REASONED, still
  under the sealed discriminating table (1 confirms, 2 confirms with the round-1 race, 0 and
  3..15 refute).

# C'S SELF-REPORT, recorded at C's own request

C reports that they released the `serve/wake.rs` pen to A and then edited that file anyway to
apply the S4 sabotage, caught it mid-task, reverted, and verified the revert (worktree clean of
tracked changes, file untouched in the main checkout, tip still `aac0d67`). No collision
occurred - and C's own account of why is the part worth keeping: only because A happened not to
be in the file at that moment, and A was unblocked there **by C's own release**.

C asked that this sit next to their process claims, since this audit has taken those at face
value all day. It does, and C's framing is the accurate one: **the outcome was fine and the
mechanism was not** - worth exactly what the belt's clean sheet is worth. A rule that held by
luck was not enforced by anything.

Recorded also because it is the third time today the same rule has been applied by its own
author against their own work (the count-grain argument against C's guard, the CITE-or-MARK
check against C's landed commit, and now this). That pattern is the reason this file has taken
C's reports as evidence rather than as assertions - and this entry is what keeps that judgement
honest rather than assumed.

Nothing in the audit's findings changes: the window-3 fix, its red, and the five sabotages are
landed and gated; the repeat-consumption mechanism remains REASONED, not run, until someone
counts.

---

# MEASURED: `armed=15 consumed=1`, BELT GREEN - and my own sealed table over-reached

A ran the belt with the across-batch defence removed. Raw, reported by C: **`BELT COUNT: armed=15
consumed=1`, test GREEN, 28.55s**, the count printed before the assert.

## Claim 2 (INCIDENCE) - CONFIRMED, and nothing else in this file lands harder
The belt passed green with **fourteen of fifteen consumptions missing**. Not reasoned - measured.
A guard named `concurrent_sweeps_never_double_consume_a_lease` reports success while 93% of the
consumptions it exists to police never happen.

Everything this audit argued about legality-shaped oracles is in that one line.

## Claim 1 (MECHANISM) - NOT settled, and MY OWN SEALED TABLE SAID IT WAS
C caught this, and the error is mine before it is A's. A scored the run "mechanism confirmed".
So did my pre-registered table, twice:

    line 874: "**1** - predicted. Mechanism confirmed: one consumption landed, fourteen were
               refused, the belt saw nothing."
    line 918: "| 1 | CONFIRMED - one landed, fourteen refused, the belt blind |"

**A journal count of 1 cannot distinguish the conflict path from the replay path I killed.** Both
produce exactly one consumption event and identical journals:
- CONFLICT: `Err(IdempotencyConflict)` -> recorder returns 0 -> nothing appended -> journal 1.
- REPLAY: digest and key set both match -> `Ok(batch.events.clone())` (local.rs:668-671),
  `publish_active_marker(..., false)` -> nothing appended -> journal 1.

The difference lives in the recorder's RETURN VALUE, which the belt never observes, and in which
store branch was taken, which no count can see. So `consumed=1` is consistent with my mechanism
AND with the version I killed - the one where `recorded` reports a count for events never
written.

**This is the finest-grain rule landing on the seed that produced it, and it caught my own
pre-registration.** The cell should have read: *confirms INCIDENCE; cannot arbitrate MECHANISM.*
I conflated the two in the very table I wrote to stop results being interpreted after the fact.
A scored the run against my label, and my label was wrong - so the mis-scoring is inherited, not
independent.

Corrected cell, replacing both lines above:

| journal count | verdict |
|---|---|
| 1 | Confirms **INCIDENCE** (the belt is blind to fourteen missing consumptions). Says NOTHING about MECHANISM - conflict and replay produce identical journals. |

The other cells stand as written: 2 confirms incidence with the round-1 two-lane race; 0 refutes
(recorder broken outright); 3..15 refutes (collision not total).

## Provenance correction: the instrument was A's, not C's S4
C's S4 removed BOTH `{next}` and `{index}`. A's sabotage removed only `{next}`
(`wake-consume-{next}-{index}-{session}` -> `wake-consume-{index}-{session}`), keeping `{index}`.

A's variant is the BETTER instrument for this seed and should be credited as A's: it isolates the
across-batch defence without also removing the within-batch one - precisely the conflation this
audit flagged in S4. So `armed=15 consumed=1` measures the loss of `{next}` cleanly.

The `{index}` half - the multi-session batch, where duplicate keys inside one batch make the store
refuse the WHOLE batch - remains unreachable by any existing fixture. That finding is unchanged
and still needs a fixture arming two sessions.

## So seed 3a is now the ONLY open half, and it is the half that can kill me
The pen-free store test (same key, same events, `expected_next_sequence` 1 then 2) is the only
instrument that can arbitrate conflict-vs-replay, because it observes the store's RETURN rather
than the journal's count. Sealed with M, assertions under review by B, unrun.

# M'S UNCOVERED CELL - added before the run, which is the only time it counts

M flagged a gap in my sealed registration and is right that it is the most alarming outcome on
the list:

> **CONFLICT (GHE003) but STREAM CHANGED.** My CONFIRMS cell requires BOTH the conflict AND the
> stream unchanged, so a refusal that nevertheless left something written matches NO cell - and
> under the no-retrofitting rule it would score UNDECIDABLE.

Added as a declared cell, and its reading matters:

| outcome | verdict |
|---|---|
| GHE003 + stream unchanged | CONFIRMS mechanism |
| GHE003 + stream CHANGED | **REFUTES THE STORE, not my mechanism.** A returned refusal alongside a mutated stream is an ATOMICITY failure, not an idempotency one - the error becomes a report rather than a guarantee. Bigger than either of my REFUTES cells, and it escalates out of seed 3 entirely. |
| replay (Ok, first batch returned) | REFUTES my mechanism; S4 explanation collapses; killed-claim section goes live |
| Ok + second batch appended | REFUTES differently - key overlap not detected at all |
| any other error code | REFUTES as stated - refused, but not for my reason |
| compile failure, hang, or panic | NOT A RESULT. Re-register; no cell scored. |

My third assertion (stream unchanged, asserted separately from the error) was written to catch
exactly that case - I built the instrument for it and then failed to declare the cell it feeds.
The assertion without the cell is half the discipline: the test would have shown it and the
ledger would have had nowhere to put it.

M also sealed, unprompted, why N=1 is correct here and cannot be attacked later with the wrong
rule: the divergence is a fixed field, not a race, so determinism does the work sample size would
otherwise do. A rate rule does not apply to a row that is not a rate prediction.

---

# B'S REVIEW - assertions approved, one refinement taken, and a refinement to the FINDING

B read the test in full at `efd85d0` and verified all three of my cited claims independently
rather than accepting them: the digest carries `expected_next_sequence`; `local_atomicity.rs:231`
varies EVENT CONTENT at a held sequence of 1; and the replay-vs-conflict decision inside the
key-intersection branch is solely (digest equal AND keyset equal).

## The confound-freedom claim I could not establish alone
I had asked B to attack the one deliberate fixture choice - `expected_next_sequence` 2 is the TRUE
next, so a refusal cannot be a stale-sequence precondition failure. B closed it:

> GHE003 is produced ONLY inside the key-intersection branch. An identical single key enters that
> branch unconditionally. The branch runs BEFORE any sequence comparison. Inside it, the only two
> exits are replay (digest equal) or GHE003 (digest unequal).

So a GHE003 in this fixture measures exactly the digest's field coverage, with no confound. That
is the claim that makes the test worth running, and it is B's, not mine.

## The refinement, taken - and it is my own rule found in my own test
B: `page.events == first` reads one page at limit 100. Exact today; a PAGINATION COINCIDENCE the
moment a fixture grows past the page - the assertion would pass for a reason that is not the
property. Added a fourth assertion:

    assert_eq!(repository.next_sequence(&scope(), "stream-1").unwrap(), 2)

The sequence cannot drift: had anything been appended the stream would be at 3. Re-registered
with M as a NEW ROW, not an edit, per the rule that an assertion change after sealing is a new
registration.

## B'S LIVE-SCENARIO NOTE - this refines the FINDING, not just the test
This is the sharpest correction to my "the count is honest by accident" claim, and it comes from
the case my test does NOT pin.

My test pins *same key, DIFFERENT sequence*. The wake recorder's real twin-race is *same key,
SAME sequence*: two post-fix sweeps pin the same `next`, the rival lands first, and ours then
resolves as a REPLAY - because with the same sequence and the same reason the digest is FULLY
equal. `recorded` then counts the RIVAL's write as ours.

**That is benign, and the reason it is benign is the part worth keeping.** The consumption exists
in the stream. The count is TRUE ABOUT THE WORLD even though our append wrote nothing. It is the
one place where `recorded` means "on record" rather than "written by us" - and those two readings
are only ever different when the replayed batch is byte-identical to what we would have written.

So the honest statement of the earlier finding, corrected:

> The digest protects the count's honesty precisely because the EQUAL-DIGEST case is the only
> replay it permits, and in that case the replayed batch IS the truth. The count is not honest by
> luck - it is honest because replay is restricted to the case where "someone else wrote exactly
> this" and "we wrote this" have the same meaning.

What survives from my earlier framing: the recorder does not KNOW any of that, nothing in either
crate states the dependency, and a change to `request_digest`'s field set would break the
equivalence silently. Honest for a reason is not the same as honest by design. But "by accident"
was too strong, and B is right to take it off me.

## And B's note names the divergent-reason case, which lands in my GHE003 arm
If the two racing sweeps carry DIFFERENT reasons (rung vs stale), the digest differs, the same
race lands in the GHE003 arm, and the recorder returns 0 with the rival's consumption standing.
That is also correct behaviour - the lease is burned, once, by whoever got there first - and it
is the live-traffic twin of exactly what my test pins synthetically.

Cited: B as second reader, by their own offer.

---

# MECHANISM CONFIRMED - and my COVERAGE claim FALSIFIED by the same run

C ran the store test. Two results, and the second one kills a claim of mine that is currently on
the board.

## 1. MECHANISM (seed 3a) - CONFIRMED
`the_same_key_at_a_later_sequence_conflicts_rather_than_replaying`, same key, same events, only
`expected_next_sequence` moved 1 -> 2:

    cargo test -p graphhelm-events --test local_atomicity the_same_key_at_a_later_sequence -- --nocapture
    test ... ok    (GHE003_IDEMPOTENCY_CONFLICT, stream unchanged)

So the second consumption takes the CONFLICT path, the recorder returns 0, and the louder version
I killed by checking stays dead. N=1, deterministic, no sabotage needed to reach it.

Scoring, precisely: **INCIDENCE** was confirmed by A's belt run (`armed=15 consumed=1`, green);
**MECHANISM** is confirmed by this test and NOT by A's count. Two instruments, two claims, and
neither could have done the other's job - which is the whole reason the seed was split.

## 2. MY CLAIM "nothing pins divergence-by-sequence-alone" IS FALSE
C refused to bank a passing test and sabotaged for it. Raw:

    SABOTAGE A - replay arm ignores the digest (`if batch_keys == requested_keys`):
      FAILED: the_same_key_at_a_later_sequence_conflicts_rather_than_replaying
      FAILED: exact_retry_is_resolved_before_sequence_and_divergent_reuse_fails_closed
      27 passed, 2 failed.

    SABOTAGE B - digest blind to the sequence (`expected_next_sequence: 0` in request_digest):
      FAILED: the_same_key_at_a_later_sequence_conflicts_rather_than_replaying
      FAILED: exact_retry_is_resolved_before_sequence_and_divergent_reuse_fails_closed
      FAILED: concurrent_writers_serialize_and_only_one_claims_the_expected_sequence
      FAILED: identical_idempotency_key_is_independent_across_streams
      FAILED: artifact_catalog_rejects_divergent_reregistration_before_mutation
      FAILED: artifact_identity_cannot_be_reused_from_a_different_producer_stream
      FAILED: injected_publication_failures_never_expose_dangling_committed_references
      22 passed, 7 failed.

C could not construct a sabotage that fells the new test ALONE. Sabotage B is the targeted one
for my claim - it makes the digest blind to sequence while leaving it sensitive to content - and
it takes down **five pre-existing guards** besides the neighbour and the new test.

**So my claim was true of the NAMES and false of the COVERAGE.** C's phrasing, and it is exactly
right. I inferred a coverage hole from reading ONE neighbouring test and generalised to "nothing
pins it". C measured it instead of reading it, and the measurement says otherwise.

This is the same error I have been naming all day, committed by me: I asserted what an oracle
CANNOT see from reading its assertions, in a case where the answer was cheaply measurable, and I
did not measure it.

## What survives the correction, stated narrowly
Not "unguarded" - that is dead. What remains true, and it is weaker:
- **No test NAMES the property.** A maintainer who changes `request_digest`'s field set gets five
  to seven red tests whose names are about concurrent writers, cross-stream keys, artifact
  identity and dangling references - none about sequence divergence. They must diagnose backwards
  from unrelated-looking failures to a field they removed.
- **The dependency is stated nowhere in either crate.** `serve/wake.rs` leans on it silently.
So: a DIAGNOSABILITY and NAMING problem, not a coverage hole. Those are different diseases and
only one of them was mine.

## One precision, offered to C rather than as a walk-back
Sabotage B shows the digest's sequence field is LOAD-BEARING crate-wide - break it and much
breaks. It does not show that any existing test TARGETS divergence-by-sequence; the five extra
failures look incidental to the field's removal rather than aimed at the property. For a
maintainer the practical effect is identical (red is red), so the correction to my claim stands
in full. But in this audit's own vocabulary there is a difference between *guarded* and
*incidentally covered*, and what C measured is the second. That distinction changes nothing about
who was wrong here.

## Consequence for the board, and I would rather say it than let it stand
The orchestrator put my cross-crate observation on the board as the INDEPENDENT LANDING RATIONALE
for my test - "the dependency is stated nowhere, this test is the warning tripwire". The tripwire
half is now measured false: the tripwire already exists, it is just unnamed. The naming and
documentation half survives.

My own sealed test is therefore **redundant as protection** and worth at most its documentation
value - which is the same judgement C reached about theirs, unprompted, in the same message that
confirmed my mechanism. I recommend my run be withdrawn rather than take a machine slot behind
H's N=20 to re-prove what C has now measured twice over.

---

# B'S AMENDMENT - the declaration at the edit site, and where it should now ride

B answered on the sentence I kept ("nothing in either crate states the dependency") and improved
it. Recorded because it is the fix for the exact half of my claim that SURVIVED falsification.

## B's point
The moment any named test lands, "nothing states the dependency" becomes false by one
instrument - the test IS the statement, executable, and it fails the day `request_digest`'s field
set drops the sequence. What remains true is narrower:

> No code NEAR the digest names the wake recorder as a dependent. The person editing
> `RequestDigest`'s field set meets the dependency only as a distant test failure, never as a
> sentence at the edit site.

B's cheap close: one comment line at the `RequestDigest` struct literal in `integrity.rs` naming
the dependent and pointing at the executable statement, landing in the SAME PR as the test, so
declaration and instrument arrive together. Then "honest by design" is earned rather than
asserted - the design includes both the restriction and its declaration.

## Why this matters MORE after C's measurement, not less
C's sabotage showed the dependency was ALREADY enforced executably, by five pre-existing guards
that do not name it. So the situation was never "unstated and unenforced" - it was **enforced and
undeclared**. B's comment line is precisely the missing declaration, and it is now the whole
remedy rather than half of it:

- enforcement: already there (measured, C's sabotage B, 5 guards fell);
- naming: C's test, if it lands;
- declaration at the edit site: B's comment, which nothing else provides.

The editor of `RequestDigest` still meets the dependency as a wall of unrelated red unless that
line exists. That is the diagnosability problem stated as a fix.

## Where it should ride
NOT with my test. My row is recommended SUPERSEDED - C's independent instrument answered the
question and my run would re-prove a measured fact. So B's comment should ride with **C's** test
in whatever PR carries it, pointing at C's test name rather than mine.

Flagged to B, who proposed it while holding the earlier state in which my test was the one
landing. The amendment survives the change of vehicle intact; only the file it points at moves.

## B on the corrected finding
B calls "replay is restricted to the case where the two readings mean the same thing" the keeper
and better than what B sent, and confirms striking "by accident" was right: the digest's field set
is a decision someone made, not luck - what was missing was only the declaration. Recorded as
B's endorsement of a correction B caused.

---

# FOURTH INSTANCE, and it is inside the sentence that corrected the third (M)

M caught this and it is the sharpest catch of the day, because of where it sits.

When I withdrew my instrument I argued its marginal value was near zero on this ground: *"mine
carries an inline positive control, but the neighbour test pins that same control one test over,
so it is coverage-across-tests."*

**That is a coverage claim inferred from reading, and nobody has sabotaged for it.** C measured
coverage of the DIVERGENCE property. **No one has measured whether anything pins
REPLAY-REACHABILITY.** So the dismissal rests on exactly the move that had just been caught in
me one property over - and I made it in the same breath as conceding the earlier one.

M's disposition, which is right: this is NOT a reason to reverse the supersede. Marginal value is
plausibly low, and running an instrument to protect a control that is probably covered is a poor
use of a contested machine. It IS a reason the dismissal is marked **UNMEASURED rather than
established**, so no future reader inherits "the positive control is redundant" as a fact.

Cost to settle: one sabotage - break replay-reachability, see whether any test falls. Cheap to
test, expensive to assume, by the rule I adopted this morning.

Four instances today, same shape. The pattern is not that I keep being wrong about coverage; it
is that **reading an assertion tells you what it says and never what it can see**, and that gap
is invisible from the inside every single time.

# C READ THE TWO GUARDS - collateral confirmed, and the diagnosability case got much stronger

C checked my "load-bearing crate-wide, not targeted" precision rather than conceding it, and read
both suspect guards:

- `identical_idempotency_key_is_independent_across_streams` (local_atomicity.rs:495) - appends the
  same key to stream-1, then stream-2, then a different scope, ALL at expected sequence 1. Its
  subject is cross-stream and cross-scope key independence, and the idempotency scan skips other
  streams' batches before any digest comparison. Sequence-in-digest is not its subject at any
  point.
- `concurrent_writers_serialize_and_only_one_claims_the_expected_sequence` (:939) - two threads,
  DIFFERENT keys, same stream, same expected sequence. Its subject is sequence CLAIMING, which is
  the CAS - a different mechanism from the idempotency digest entirely.

**Collateral, not aimed.** The precision holds: the field is load-bearing crate-wide; no existing
test targets the property.

## And C could not explain the reds from reading - which is the finding
C states it as a limit rather than a mystery: having sabotaged the field ON PURPOSE, knowing
exactly what they changed, C traced both paths and **could not derive the causal chain from
source alone**.

That is the diagnosability argument measured rather than asserted. A maintainer who removes that
field by accident gets five to seven reds about concurrent writers, cross-stream keys, artifact
identity and dangling references, and must work backwards to a field nobody named - when the
person who broke it deliberately, with full knowledge, could not.

**That is stronger evidence for keeping the named record than "no test names it" ever was**, and
it is the argument that replaced my dead tripwire claim. C's, from C's own failed attempt to
explain their own result.

## The positive control: required, not made, and C's reason is right
C will NOT blind-add the twelve lines. They are on zero-cargo until milestone close, so they could
add them but not RUN them - and shipping an assertion nobody has watched even PASS is worse than
the gap it closes. Recorded as **the required addition on whoever lands that test**: fold in the
inline positive control (exact retry at seq 1 replays first, proving reachability) and run it.

Handing over twelve unrun lines and calling it done is the thing this whole audit exists to
refuse. C refusing it about their own test is the fourth time today they applied a rule against
their own work.

---

# CODA — the same shape, one layer up (#76 review, K)

Reviewing #76 turned up the audit's own subject in a new place, and K named it better than I did.

The PR's pair-guard declared a sabotage in its own docstring: "add `blobs` to the recoverable set
and this test goes red." Traced against the implementation, it does not — the recovery arm creates
only `.tmp`/`active`, so the re-classification still sees `blobs` missing and the open still ends
`Err(Integrity)`, which is exactly what the test asserts. The guard passes for a reason unrelated
to the decision it claims to hold. **A refusal arriving for the wrong reason**, which is the one
thing this milestone trained me to look for.

K traced it independently rather than on relay, then wrote the trap into the docstring instead of
merely removing it, and added an assertion I had not asked for (the fixture must PROVE the
directory existed before removing it — a silent no-op removal can no longer pass as a removal).

K's formulation, which generalises past that PR and belongs with the rest of this file:

> **A declared-but-unrun sabotage is worth nothing — the same failure as a citation nobody
> re-derives, one layer up.**

An unrun sabotage in a docstring reads exactly like a measurement to anyone skimming. That is the
same trick the belt's green played on this milestone: a sentence that looks like evidence,
carrying none. The audit found it in guards, the seeds doc had it in prose, and #76 had it in a
docstring — three altitudes, one disease.

REGISTERED, both arms on K's run list, reported either way: predicate-only sabotage -> I predict
GREEN (does not fall); both-edits sabotage -> I predict RED (the store opens). If the
predicate-only arm comes back RED, my trace is wrong and the #76 approval should be revisited
rather than the prediction quietly dropped.

## CODA, CONTINUED — the run refuted the reviewer too, and in his own subject

Both sabotage arms ran. Arm 1 (predicate only): test 2 GREEN — my registered prediction confirmed,
the trace exactly right. **Arm 2 (predicate AND creation loop): test 2 STILL GREEN — which neither
K nor I predicted.**

Mechanism, verified in source afterwards: `load_state` resolves every evidence reference through
`read_verified_blob`, so an archive whose history seals Evidence refuses on the missing blob FILE
during the load, whatever the layout rule says. **No layout sabotage could ever have felled that
test.** A second, independent defence was answering the whole time.

So the pair-guard I reviewed and approved was itself INCIDENTALLY COVERED — the exact distinction
I coined earlier in this file, landing on my own review. And my finding 3 aimed at the right place
and stopped one step short: I told K to pin "does this archive reference evidence?" and never
asked "does that reference make something ELSE refuse first?" The property I demanded be pinned is
precisely the property that made the test unable to measure its subject.

The fix (K, measured both ways): a new test on the one committed archive that seals NO Evidence —
the only place where the layout rule is the sole defence — RED under arm 2, GREEN under arm 1. The
old test kept and RENAMED to the defence it actually pins, its docstring recording that two
plausible claims about it, K's and mine, were both wrong. And the new test pins its own fixture
property (every event's `evidence_refs` empty, with the reason inline), which is finding 3 applied
correctly this time.

**What this costs me, stated plainly:** I was wrong twice in the same direction about the same
pair, both times by reading. The reading was careful, the trace was right, and it was still an
inference about what a guard can see. That is this file's own thesis, and being its author bought
no immunity at all — which is the strongest evidence in here that the cure is a run and never
more care.
