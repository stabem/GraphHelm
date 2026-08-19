# M09 close doc — wake-family section (INPUT to F's skeleton, not the close doc itself)

Written by N Agent as parked material. F holds the close-doc pen; this is source text, to be cut,
reordered or rejected. Every number below carries its source (CITE); every claim not measured is
marked (MARK). Nothing in this section was run by me — I ran nothing all milestone.

Source file: `.factory/n-agent-oracle-audit.md` (frozen, md5 622cab91f7e70ac86ea552785c683aa2).
Other inputs cited inline: `.factory/c-agent-wake-flakes-study.md` (C), `.factory/h-agent-base-measurements.md` (H).

---

## 1. What the wake family was tested by, and what that testing could see

The wake guards were audited by oracle GRAIN — not by whether they pass, but by what a passing
result is capable of ruling out. Twenty entries: 18 named guards plus two grouped blocks.

| oracle class | count | what it can see |
|---|---|---|
| (a) legality | 2 | only failures that make the log ILLEGAL |
| (b) receipt-grain | 12 | the store's own answer: lease state, receipt reason, receipt sequence |
| (c) mixed | 6 | receipt-grain in places, but the headline property rests on a legality or a timeout-negative |

CITE: full inventory, per-guard blind spot and per-guard minimal closing assertion in
`n-agent-oracle-audit.md`, section "Inventory".

The finding the classification serves, originally C's: **burning a live lease is LEGAL.** The fold
accepts it, replay succeeds, the operator surface stays calm, and the store's receipt says `rung`.
A legality oracle cannot detect the failure mode the milestone exists to prevent. CITE: C,
`c-agent-wake-flakes-study.md`, "the #55 family's oracle is blind to its own worst case".

## 2. The measured result — the belt is hollow

`concurrent_sweeps_never_double_consume_a_lease` is the family's flagship belt: fifteen rounds of
barrier-synchronised concurrent mutations. Its oracle is `ok == true` per round (legality) plus
`consumed <= armed` over the whole run (an aggregate inequality).

Two measurements, both green:

| sabotage | result | source |
|---|---|---|
| sweep's call to `record_consumptions` deleted | `test result: ok. 1 passed; 0 failed; 37.94s` | CITE: C, N=1, `cargo test -p graphhelm-cli --test wake_http --locked concurrent_sweeps_never_double_consume_a_lease` |
| across-batch key defence `{next}` removed | `BELT COUNT: armed=15 consumed=1`, GREEN, 28.55s | CITE: A's instrument, count printed before the assert, reported via C |

**The belt reports success with fourteen of fifteen consumptions missing.**

Why it cannot see them, by construction and not by luck: with no consumptions written, `consumed
= 0` and `0 <= armed` holds for every value of `armed`; and a component that writes nothing cannot
write an illegal event, so `ok == true` holds every round. The run confirms the reading rather
than being its only support.

This is the owner's existing **assert-at-the-finest-grain** rule, measured: the belt asserts "no
double consume" and never asserts the consumptions happened at all, so the assertion sits one
level above the thing it is read as representing. CITE: M's ledger, sealed as the measured
instance.

Consequence for the close: **every green that belt has ever produced means less than it reads.**

## 3. The three root causes behind the class-(a) and class-(c) guards

- **RC1 — the type-string helper is blind by construction.** `kinds_after`, `kinds_snapshot` and
  the sleeper's at-ring snapshot map events to `kind["type"]` and discard the payload, so reason,
  session and count are invisible. Three guards are (c) because of one helper. CITE:
  `wake_http.rs:231-257, 303-318, 259-297`.
- **RC2 — negatives measured by timeout with no positive control.** Three guards assert that
  something did NOT happen in a fixture where nothing proves the mechanism was alive to make it
  happen.
- **RC3 — `recorded: usize` is a lossy oracle surface.** `record_consumptions` collapses one guard
  decision and eleven unrelated failure paths into the same `0` (CITE: twelve `return 0;` sites
  counted at `serve/wake.rs:150-238`). Guards on it can only ever be class (c).
- **RC4 — the receipt cannot attribute a burn to an arming.** `WakeLeaseConsumed` carries
  execution, session and reason — no rendezvous, no armed sequence — so the journal cannot answer
  "which arming did this burn?".

## 4. The two diseases, which need different cures

A distinction that emerged mid-audit and is the operative one for the fix phase:

- **DETECTION gap** — the path is driven, repeatedly, and the oracle cannot see the result.
  Adding a test that drives the path cures nothing. *Example: the belt drives repeat consumption
  for one session fourteen times and reports green.* Cure: the ORACLE (assert the receipt's
  sequence advances per round).
- **COVERAGE gap** — nothing drives the path at all. *Example: two sessions due in one sweep,
  where duplicate keys inside one batch make the store refuse the WHOLE batch — every consumption
  in that sweep dies together, including a first-ever burn.* Nothing reaches it: the only ARMED
  session anywhere in `wake_http.rs` is `session-sleeper-1` (CITE: verified; `session-nobody`
  :574 and `session-somebody-else` :1367 are deliberately unarmed refusal fixtures). Cure: a
  FIXTURE arming two sessions on one execution.

And the vocabulary correction that governs both, B's: **a sabotage that fells many tests proves
the SUITE covers a property; it does not prove any single test MEASURES it.** "Guarded" and
"incidentally covered" are not the same word.

## 5. The self-repair rule (milestone vocabulary candidate)

> A fault followed by automatic repair produces an end state indistinguishable from no fault. An
> end-state oracle therefore measures the REPAIRER, not the fault. The operative condition is a
> latency comparison: **repair latency < observation window => the oracle is blind.** So the
> instruction is not "avoid end-state oracles" but *observe at a grain finer than the repair
> latency.*

Two legs, both mechanisms verified in source:
- **storage layer** — the sweep's due list is every live lease past `content_head` (CITE:
  `serve/wake.rs` phase 1), so a round that consumes nothing is repaired by the next round's
  sweep and the end state is clean.
- **execution layer** — `ToolDisposition::TimedOut => NodeOutcome::RetryableFailure` (CITE:
  `core/runtime/src/executor.rs:241-242`), and a node blocks only after three identical outcomes
  (CITE: `core/execution/src/bounds.rs:10`, enforced `core/execution/src/progress.rs:43`), so a
  successful second attempt ends the run clean.

MARK: the execution-layer leg's MECHANISM is verified by me; E's specific measurement (the judge
losing the race, 6/7 degrading to 4/7) is E's and is NOT verified by me. The rule does not rest
on it.

Why this shape gets built and kept (B's refinement): the oracle is not uniformly blind — if the
REPAIRER breaks, the end state goes red and the oracle catches it loudly. It detects faults in the
healer while missing faults in the healed, and demonstrates that it "works" often enough to
survive review.

What a green end state licenses: consistency with BOTH *no fault* and *fault plus repair*, never
evidence for either branch, plus the bound that the fault was within repair capacity.

## 6. Enforced and undeclared — a third state worth naming

A property can be in one of three states, and the middle one was missing from our vocabulary:

| state | failure mode |
|---|---|
| enforced + declared | the healthy case |
| unenforced | a missing guard |
| **enforced + UNDECLARED** | **a wall of unrelated red pointing nowhere** |

The wake recorder's honest count depends on `request_digest` covering `expected_next_sequence`
(CITE: `core/events/src/integrity.rs:270-277`) — a dependency two crates away that nothing states.

MEASURED: blinding the digest to that field fells seven tests, five of them pre-existing guards
(CITE: C, sabotage B, N=1). So the property is ENFORCED. Verified by reading, all five: none of
those guards has the property as its SUBJECT — cross-stream/cross-scope key independence at a held
sequence, sequence CLAIMING via the CAS, artifact re-registration, artifact identity across
producer streams, and failpoint-injected publication failures. CITE: two read by C, three read by
me.

And the argument that makes it a finding rather than an observation: C sabotaged that field ON
PURPOSE, knowing exactly what they had changed, and could not derive the causal chain from source
alone — stated as a limit of their reading. **If the person who broke it deliberately cannot
explain the reds, the maintainer who breaks it by accident has no chance.**

## 7. Claims this milestone made and then killed

Listed because they are the sentences that read well enough to be quoted, and because a close doc
that only reports its wins teaches the wrong lesson. Five of the six were mine; the sixth I
propagated.

| dead claim | what replaced it |
|---|---|
| "Nothing pins divergence-by-sequence-alone" | Enforced and undeclared. True of the NAMES, false of the COVERAGE. |
| "The chain proving #55 has no link that fails when the recorder dies" | Exactly one link fails when the recorder is DEAD, and it asserts a COUNT — so no link fails when the recorder is WRONG. |
| "No guard drives two consumptions for one session" | The belt does, fourteen times. Detection gap, not coverage gap. |
| "The recorder's count is honest by accident" | Honest for a reason: replay is restricted to the case where "on record" and "written by us" mean the same thing. Undeclared, not accidental. |
| "Journal count 1 confirms the mechanism" | Confirms INCIDENCE only — conflict and replay produce identical journals. |
| The sleeper guard fails "roughly 3 in 4" | Unciteable (no N, no base, no invocation), inherited from `docs/milestones/m09-seeds.md` seed 9. H measured 12/13. CITE: `h-agent-base-measurements.md`. |

The general form, which unifies most of this section: **reading an assertion tells you what it
says and never what it can see — and that gap is invisible from the inside every time.** It is
assert-at-the-finest-grain from the reader's side.

## 8. Seeds — named, measured, and the cure each needs

| # | seed | named | measured | cure |
|---|---|---|---|---|
| 1 | Belt cannot see a missing consumption | yes | YES — green with recorder deleted; green at `armed=15 consumed=1` | ORACLE: per-round receipt-sequence advance. Detection gap; a new driving test cures nothing. |
| 2 | Two due sessions in one sweep kill the whole batch | yes | no (MARK: reasoned from `integrity.rs:107-113` + the recorder's batch build) | FIXTURE: arm two sessions on one execution; assert both consumptions land. Genuine coverage gap. |
| 3 | Receipt cannot attribute a burn to an arming (RC4) | yes | no (MARK: structural, from the event's field set) | FIELD: carry the arming's sequence into `WakeLeaseConsumed`. Closes C's discriminator, the belt's per-round attribution, and RC1's helper at once. Schema ritual — a later PR, not a minimal one. |
| 4 | Type-string helper blinds three guards (RC1) | yes | no (MARK: from reading the helpers) | HELPER: one function returning `wake_leases` + `wake_last_consumed`; three guards fixed at once. |
| 5 | Timeout-negatives with no positive control (RC2) | yes | no (MARK: from reading three fixtures) | Pair every "did not happen" with an "and here it does". |
| 6 | `recorded: usize` is lossy (RC3) | yes | partially — a dead recorder leaves entry #1 green (CITE: C, S6) | Typed outcome. Owner/design call, not a test fix. |
| 7 | Digest dependency enforced but undeclared | yes | YES — 7 fell, 5 pre-existing (CITE: C, sabotage B) | DECLARATION at the edit site (comment naming the dependent + the executable statement), riding the named test's PR. Draft wording supplied by C. |
| 8 | Replay-reachability coverage | yes | **NO — UNMEASURED, but an instrument is REGISTERED with cells (C, via M)** | Third sabotage, distinct from the two already run: force the key-intersection branch to NEVER return the prior batch, so the replay exit becomes unreachable. Cells declared before any run. Flagged because "the neighbour test pins it" (N) and "my test relies on the neighbour" (C) are the SAME unmeasured claim from opposite sides — M caught both. |

MARK on seed 8 specifically: an instrument existing is not a measurement. The row stays open
until someone runs it, and its most interesting cell is the one none of us predicted — if the
NEIGHBOUR comes back green, nothing excludes the vacuous-pass mode and the gap is "nobody covers
it" rather than "one test leans on a sibling". C registered the prediction (neighbour RED, mine
GREEN) and registered that the prediction rests on READING, which is the class of evidence this
whole section is about.

MARK on the whole table: seeds 2–5 are reasoned from source, not run. Seeds 1, 6 and 7 carry
measurements with invocation and N in the source file. Seed 8 is the audit's own unmeasured
dismissal, recorded so no reader inherits it as fact.
