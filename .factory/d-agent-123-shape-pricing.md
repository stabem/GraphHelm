# #123 — pricing note for the three shapes. PRICED, NOT BUILT.

D, 2026-08-20, at main `c357a5e`. Authorized to price; no shape is built and none should be until
the orchestrator rules the trade. Every code fact below was read at that commit; every number is
labelled measured, derived, or estimated.

## The floor the shapes are priced against

**#103's cost, measured** (`.factory/d-agent-storm-rate-results.txt`, L's pre-registered rule):

| | storm failures | events appended (median) |
|---|---|---|
| PRE `07b1243` | **0 / 10** | 49 |
| POST `0fb0e66` | **7 / 10** | 60 (**+22%**) |

Arms differ by `git rev-list --count 07b1243..0fb0e66` = **1**. Instrument byte-identical in both
arms. Verdict **MOVED** at the 10-pair look (rule: ≥6 of 10).

**And the cost compounds — this is the number I would put in front of the trade.** Measured on the
storm's own graph and verbs, a full `resume` whose drive finds nothing dispatchable, over ten
successive rounds as the journal grows:

    476  527  574  666  672  763  881  838  876  1083   (ms)

**It more than doubles.** The journal goes from ~19 to ~57 events across those rounds and every
open pays the longer `O(head)` load. So the amplifier's cost is not linear in appends: more
appends → longer journal → every subsequent open costs more. That is the convoy term this whole
lane is about, made visible on the exact scenario.

*(Caveat, stated: those are whole-CLI-process timings — they include process start and argument
parsing, so they are an UPPER BOUND on the drive itself. The growth trend is the robust part; the
absolute level is not.)*

---

## (a) L's variant — resume hands the release list to the drive, driver appends under the OWNER's actor

**Mechanism.** `resume` stops force-`Started`ing paused nodes. It passes its paused-node list into
the drive as "release these when their edges allow". The driver, which already re-evaluates every
pass and already holds the spec, releases each one at the first pass where `edges_satisfied` is
true — and appends that `Started` **under the owner's actor**, so the release stays the owner's
act and only its TIMING moves. One drive. Timing solved because the evaluation moved into the loop
that repeats.

**ACTOR FACT, VERIFIED NOT ASSERTED — and it is half good news.** Attribution genuinely is data on
the event: both drivers take an actor as a parameter and pass it to every append
(`apps/cli/.../driver.rs:39` → `:54, :122, :137, :150`; `core/runtime/src/driver.rs:428` → `:462,
:523, :536`). Nothing derives the actor from which loop wrote the event.

**BUT EACH DRIVE ACCEPTS EXACTLY ONE ACTOR TODAY.** There is no per-append actor choice. So the
change is not "does the append path accept a caller-supplied actor" (it does) but "can it accept a
SECOND actor for a subset of appends" — and that is the change itself.

**Files touched:** `apps/cli/src/commands/execution/driver.rs` (two params: release set + release
actor; use at the dispatch site), `core/runtime/src/driver.rs` (same, at `:523`/`:536`),
`apps/cli/src/commands/execution/resume.rs` (stop force-starting; pass the list), plus whatever
threads `PreparedDrive` to the async path. `core/execution` unchanged — `edges_satisfied` is
already public from #103.

**UNPRICED DETAIL I FOUND, and it is the kind that turns "one condition" into a defect:**
`dispatch_hops` (`driver.rs:249-279`) handles only `Ready` and `Queued`. For `current == Paused`
it would emit ONE `Started` (`Paused → Queued`) and the loop would then record an executor outcome
against a node that never reached `Running`. It needs `matches!(current, Ready | Paused)` so the
paused node gets both hops. One line — but nobody had costed it, and it is silent if missed.

**Opens delta: ZERO.** No extra drive, no extra reread; `edges_satisfied` is pure over spec and
states already in hand.

## (b) Two-phase resume — drive, release, drive again

**Mechanism.** Drive; then evaluate still-`Paused` nodes against the POST-drive state; `Started`
the newly-satisfied ones; drive again only if any were released.

**Opens delta: MEASURED-ADJACENT, and it is not free.** A no-op drive is at minimum **three store
opens** — `approve_untouched`'s reread, the loop's reread, and `complete_if_quiesced`'s reread
(`driver.rs:54, :56, :150`) — plus any appends. At the storm lane's **measured** ~30 ms median open
that is **~90 ms per resume that needs the second drive**, and more as the journal grows, per the
curve above. The whole-process upper bound measured here is 476–1083 ms, which brackets it loosely.

**Sovereignty:** preserved (resume still appends the release under the owner's actor) but the
BOUNDARY moves: today `execute_prepared` (owner) completes before `drive_to_quiescence` (system)
begins; two-phase makes it drive → decide → drive. **That is the 04e/05d actor split, drawn on
purpose**, and moving it is the orchestrator's call, not mine.

## (c) Live with the amplifier until #90/#94 reshape this area

**Cost, now measured rather than shrugged at:** +22% storm-native appends, **0 → 7 of 10** storm
failures, and a per-resume cost that more than doubles over ten rounds because the journal keeps
growing. The storm test stays red and stays attributed.

---

## What I would say if asked (I am not the decider)

(a) is the cheapest correct shape: **zero opens added**, sovereignty preserved by construction
rather than by argument, timing solved by moving evaluation into the loop that already repeats,
and no new state, event kind or wire vocabulary. Its cost is a second-actor concept threaded
through two drivers plus a one-line `dispatch_hops` fix. (b) is strictly more expensive for the
same outcome and moves a deliberate boundary. (c) is honest but now carries a measured 7-in-10
failure rate on a test that is in the gate.

## What this note does NOT establish

- **I have not built or run any of the three.** Every claim about (a) and (b) is read from code or
  derived from measurements of the CURRENT code, never from a working version.
- **The async path's `PreparedDrive` threading is unread.** I priced the two driver files and
  `resume.rs`; whatever carries the list to the serve route is named but not inspected.
- **Whether an ordinary (non-immediate) HTTP `pause` trips the async driver's cancel channel is
  unknown to me.** It matters for any shape that lets the driver release nodes; L raised it and I
  confirmed only that the CLI drive loop observes no pause at all (its single `simulation_status`
  read is at `:347`, inside `complete_if_quiesced`, after the loop).
