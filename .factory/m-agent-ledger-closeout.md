# Prediction ledger — final report (input for F's close doc)

Author: M Agent, 2026-08-19. Source of every claim below: `.factory/m-agent-prediction-ledger.md`
(my worktree). **F rewrites in their own voice; this file is the evidence, not the prose.**

**CITE-or-MARK, applied to myself throughout.** Every verdict names the instrument that produced
it. Where I hold no measurement, the row says so. Where a claim reached me second-hand and I did
not verify it, it is marked **UNVERIFIED BY ME**. I ran no cargo at any point; every number here
was produced by someone else and scored by me.

---

## 0. PRODUCT STATE FIRST — read this before anything else

**Restructured after D's caution, which landed on this file.** As first written, this document led
with the scored table, then the rules, then what the ledger prevented, and put the product's open
state LAST. **That ordering showcases the method and buries the answer.** D's words: *the rules are
not the deliverable* — the owner asked whether a test is flaky and why.

**THE ANSWER, BLUNTLY, IN THE STORM LANE'S OWN WORDS: WE DID NOT FIND THE CAUSE OF THE FLAKE. We
found what it was NOT, bounded what it would take, and handed a validated premise to the next
milestone.** If this document smooths that, **the thing lost is not modesty — it is the SIGNAL THAT
THE QUESTION IS STILL OPEN**, which is the only part that changes what anyone does next.

**THE ANSWER TO THE OWNER'S QUESTION, stated before any method:**

- **The failing PHASE is settled** (read-phase, `api_http.rs:464`, error 10060 — confirmed).
- **The MECHANISM IS ENTIRELY OPEN.** H1 vs H2 vs H3 is untouched. Four hypotheses are dead or
  disfavoured; **none of the three live ones has been distinguished from the others.**
- **THE RATE MAY OR MAY NOT HAVE CHANGED.** The storm went 4/10 → 0/10, but those intervals
  **overlap** (0/10 bounds the rate only at ≤26%), so elimination is not established. Ruling out a
  5% ceiling needs ≈60 runs.
- **THREE VARIABLES ARE CONFOUNDED** across that comparison: code changes, disk state, and fleet
  load. **The lane recorded zero machine variables while testing a machine-bound hypothesis.**
- **THERE IS NO FIX.** The storm has none. The sleeper flake stopped reproducing on a fix aimed at
  a different flake, at bounds that exclude ≈26% and say nothing below.
- **The wake lane did land real work:** a deterministic red observed for the rdv-equal defect
  (`recorded == 1`, **replay still succeeds** — the silent-failure signature), a positive control
  proven to fail, a masking claim proven and scoped, and an instrument set where **no coverage
  exists by accident.**
- **The hardest measured number of the day is a product finding, not a method one:** **the belt
  reports GREEN with fourteen of fifteen consumptions missing.**

**Everything below is evidence and method. Section 4 is the honest open list; the rule set in
section 2 is an APPENDIX and should be read as one, however good the rules are.** If this report
leads with thirteen rules and buries "we still do not know what causes the flake, and it may no
longer reproduce", it is a beautifully-instrumented account of not answering the question.

---

## 1. The scored table

Verdicts are as sealed before the results existed. Amendment history is preserved in the ledger;
this table carries the final state plus a note where a row was amended.

### Storm lane (flake #1) — instrument: H's clean base, `.factory/h-agent-base-measurements.md`

| Row | Verdict | Basis |
|---|---|---|
| D-P1 — panic at `api_http.rs:464` | **CONFIRMED** | `:464` in all 6 failing runs; zero `:443`, zero `:460-461` |
| D-P2 — 10060 not 10061 | **CONFIRMED** | every captured error `code: 10060`; 10061 nowhere |
| D-P3 — storm thread, not verify | **CONFIRMED** | verify never panicked; main thread dies at the join |
| D-P4 — rate ≈ 50% | **CONFIRMED-LOOSE** | 4/10 iso, 2/3 suite, 6/13 combined. **Could not have been killed at this N** (interval ≈ 12–74%) |
| D-P0 — free phase read | **PARTIAL** (amended down by D) | holds for the MUTATION path; does NOT settle the STATUS path |
| D-DT — decision table | **NOT-YET** | its instrument (Patch 1) never ran; no elapsed data exists |
| D-T2 — loopback gives 10061 | **UNINFORMATIVE — VACUOUS** | zero connect-phase failures captured; the claim was never exercised |
| H5 — port exhaustion | **DEAD, by data** | predicts codes 10048/10055; every captured code is 10060, and the CODE is legible even in laundered rows |
| H4 — connect starvation | **DISFAVOURED — dead on the MUTATION path only** | zero `:443` among the 16 *attributed* panics; 4 panics are phase-unattributed; its supporting premise (D-T2) was never exercised |
| D-P5 — opens per request | **OPEN**, form 3 (accounting identity). Superseded twice | UNSCOREABLE without `caller=` tag + per-outcome client counts; passing runs only |
| D-P5e — zero `KeyState::Complete` | **OPEN** | same tagging dependency |
| D-P6 — request-path opens ≥ 50% | **OPEN**, amended 3× | THREE unscoreable conditions (P8 dead → double-count; wall-clock denominator; missing G2 stamps), plus an under-specified bias bound |
| D-P7 — blocking-pool share 20–30% | **WITHDRAWN — unconditionally** | premise dead: the driver opens the store from the SAME blocking pool (`driver.rs:220/:274/:308` via `routes.rs:819/:871`). No replacement frozen; the tag does not exist |
| D-P8, D-P9, D-FALSIFIER | **OPEN** | all gated on rung A; none ran; **none is "did not fire"** |
| D-P10 / D-P10-ALT | **OPEN** | P10-ALT blocked then repaired (`tempdir` deletes on unwind — the panic destroys its own evidence) |
| D-C1 — no `:1518` panic | **CONDITION, NARROWED** | holds only over requests whose status was OBSERVED; the 20 timed-out requests stay unaccounted |
| D-TRAP-1 — duration is not a load proxy | **TRAP** (amended stronger by D) | run duration **cannot distinguish pass from fail in either direction**; isolated ranges overlap |
| D-H6 — stderr pipe | **TRAP** | dismissed on first-party grep + an unexercised argument about the dependency tree (D's own rule-9 correction) |

**Net for the storm lane: four hypotheses dead-or-disfavoured, and the MECHANISM ENTIRELY OPEN.**
H1 vs H2 vs H3 is untouched. Every rung-A row is unscored.

### Flake #2 — `a_sleeper_wakes_on_a_peer_append_with_zero_requests_in_the_window`

| Row | Verdict | Basis |
|---|---|---|
| A-BT — branch table | **BRANCH A1 SELECTED** | one text form in 12/12 failures: "the lease burned on the ring", `live:true`, `lastConsumed:null` |
| JP1-headline — modal text | **CONFIRMED** | that text in 12/12. **Line predicted `:824`, actual `:822:5`** — recorded, not smoothed |
| JP1-mechanism — rate rose at `53d212d` | **OPEN** | needs a parent-run of flake-2's own test; not scheduled. J's own margin note: 9/10 isolated is *consistent with* the starvation channel, **not sufficient for it** |
| JP2 — zero "placed ZERO requests" | **CONFIRMED, at its own stated bound** | 12 clean absences exclude a mechanism more common than ≈1-in-5, and nothing rarer. J pre-limited this himself |
| JP3 / A-CAP — capture run matches base | **UNINFORMATIVE — evidence source ELIMINATED** | the dedicated capture run was skipped; no rate comparison will exist. **Consequence: there is no capture-run base, so any with-number compares against the clean base with the instrument delta UNQUANTIFIED, not measured-as-zero** |
| A-TRAP — `reap_with_stderr` | **TRAP** | reads stderr after `wait()`; a chatty sidecar would deadlock the reaper |
| C-F2H2 — named-pipe reading | **OPEN** | no exit-3 capture reported to this ledger |

**The rate result, and the three things it does NOT establish.** Base at `53d212d`: **9/10
isolated, 3/3 in-suite**. Post-fix at `aac0d67` (C's fix ALONE): **0/10 isolated, 0/10 suite** —
scored MEANINGFUL DROP against pre-registered cells.

1. **0/10 is not "fixed".** It excludes rates above ≈26% and says nothing below.
2. **The mechanism is untouched.** Consumption is two-phase by design; the ring byte still
   precedes the durable append.
3. **The two zeros must NOT be pooled.** Different base rates (90% isolated, ≥37% suite) mean
   different populations; pooling manufactures a ≈14% bound neither run earns.
4. **`aac0d67` is no longer the branch tip** (`576e553` over `b8e55c8` over `aac0d67`). The
   configuration those numbers came from — C's fix without A's — **no longer exists on the
   branch.** Any citation must carry the tree.

### Flake #3 and the wake lane

| Row | Verdict | Basis |
|---|---|---|
| C-P1 — reproduces at the parent | **OPEN — UNTESTED → FOOTNOTE** | never run; parent-run slot CANCELLED because with the tip already 0/10 it buys an UNINFORMATIVE (B predicted this in advance). Carries a wake-up condition, below |
| C-P2 — window-3 deterministic red | **NO RESULT REPORTED TO THIS LEDGER** | typed, amended with firing order + a pre-registered HANG outcome; I hold no run |
| C-R2 — pin guard | **GREEN observed in the matrix; its OWN sealed sabotage not reported to me** | MARKED rather than scored |
| C-R3 — rdv-EQUAL burn | **RED OBSERVED, 2 of 3 clauses** | `recorded == 1` OBSERVED; **REPLAY STILL SUCCEEDS OBSERVED** — the clause separating it from window 3; third clause masked, left unmeasured by choice |
| B-P1 — seam fails at BOTH commits | **OPEN** | never run |
| B-P2 — 0 failures in 50 post-fix | **OPEN** | never run |
| B-P3 — widening raised the hit rate | **UNINFORMATIVE, exactly as pre-registered** | no parent-run; every planned instrument is deterministic and cannot report a probability |
| C-LC — landing-confound | **ROW KILLED, CLAIM UNRESOLVED, CONFIGURATION HISTORICAL** | observational trigger fired (0/10 at N≥10); the inference does not follow, since a true rate below ≈26% yields 0/10 routinely |
| C-3a-CORROB | **recorded**; instrument durable at `e12793b` | conflict + stream untouched. Corroborates instrument-error, **not** premise-error |
| C-PC — inline positive control | **CONFIRMED** | under sabotage, fails at `:286` — the control's own append, verified by reading the line back |
| C-RR — replay-exit reachability | **RETIRED-NOT-ANSWERED** | C refused the cell the run appeared to mark; his test was red from the control he added between sealing and running |
| C-MASK — two sabotages | **A CONFIRMED; B NOT-A-RESULT** | B's instrument could never have produced the disconfirming outcome (hardcoded `1` cannot equal the injected constant) |
| C-MASK2 — fixture rebuild + control | **SCORED COMPLETE** | run 1 confirmed for the two named (+2 explained-not-predicted); run 2 as amended; **control FELL, proving run 2's green came from the rendezvous comparison** |
| C-MATRIX — coherent instrument set | **SCORED COMPLETE** | constant fold: 3 casualties, all designed, ZERO accidental. Session-only: 2, direct guard green. **The direct guard separates the layers, which nothing did before** |

### Cross-milestone and method

| Row | Verdict | Basis |
|---|---|---|
| A-M10 — the three owner numbers | **OPEN** | "48%", "load_state ~3x", "15ms → 36ms" have **no source in the repository** (A's stated search: zero hits). Needs in-repo reproduction under the 4-piece rule |
| E-K1 / E-K2 — self-heal, no-op cancel | **SCORED — CONFIRMED** | orchestrator's independent code check; settled by reading, zero paid runs |
| N-3a-v1 and N-3a-v2 — idempotency digest | **SUPERSEDED BY INDEPENDENT MEASUREMENT** | permanently unscored; C's instrument answered the question, and **rule 3 means it can never score N's row** |
| N-S4 — belt journal count | **LABEL WRONG, corrected** | `armed=15 consumed=1`, belt GREEN. A count of 1 cannot distinguish conflict from replay — **identical journals** |
| D-META-1 — the tally's own falsifier | **OPEN**, design frozen | not scoreable in this milestone; the files are still warm |

---

## 2. APPENDIX — the rules the ledger produced, each with the case that paid for it

*Read this as an appendix. It is method, not deliverable. Every rule below found a real defect,
which means each passed a local test of value — and that is exactly the trap D named when a claim
absorbed four rounds while changing no decision.*

**AND THE BOUND, which matters more than the list: THESE GATES KEEP THE RECORD CONSISTENT; THEY DO
NOT MAKE IT CORRECT.** Every mechanical gate here targets **plumbing** — grep for dead tokens,
count begin/end pairs, hash the file, require a field. **Not one catches a SEMANTIC error.** Every
correctness failure today was caught by a person **working the example**: running a sabotage
instead of reasoning about it, multiplying out an arithmetic threshold, computing a combinatorial
probability, reading logs after asserting they were unreachable. **None of that is automatable and
none of it happened because a gate fired.** If this appendix is read as *"we now have gates that
prevent this"*, it is being read wrong.

1. **No retrofitting.** A result fitting no sealed cell scores UNDECIDABLE. *Paid for by:* my own
   gap-1 cell — I sealed a binary and the real cause (fixtures not modelling production) was a
   third thing neither C nor I listed.
2. **UNINFORMATIVE is filled in FIRST.** *Paid for by:* B-P3, sealed as expected-uninformative
   before any number, for a stated reason — and it came back uninformative for exactly that reason.
3. **A prediction scores only against the instrument its author named.** *Paid for by:* C-P1, which
   the orchestrator read as killed by a 0/10 from a different test at a different commit.
4. **The 4-piece rule** (with-number, base-number, N, scope).
5. **Absence of failure is not confirmation.**
6. **A retraction's REPLACEMENT is new work — and the check is GREP, not re-read.** *Paid for by:*
   L's corpse audit (the retracted arithmetic returned as a DENOMINATOR); then by my own file —
   I sealed "re-read" and had actually *grepped*. Evidence four-for-four: 4 stale summaries in D's
   report, 1 in L's, 2 in mine, **0 found by anyone re-reading.** Third clause: **every summary
   restatement is checked against the body** — summaries are what gets copied forward and are
   written in compression mode.
7. **FLATTENING** — if a value a verdict rests on can be produced by more than one upstream cause,
   the row is UNSCOREABLE. **Dimensional**, not total: it destroys the questions asked *in the
   flattened dimension*. Two sub-classes with different repairs: collapsing presence (widen the
   value) and ambiguous absence (make absence self-evident). *Paid for by:* six instances, **four
   of them in our INSTRUMENTS, not the product** — and by a wrong verdict of mine (H4 marked dead).
8. **A rate prediction must name the N that would FALSIFY it.** *Paid for by:* D-P4 — at N=10
   nothing near 50% was killable, so the confirmation carried almost nothing. D says he should have
   frozen an N and did not.
9. **Every "ZERO X" claim must name the population in which X was OBSERVABLE.** *Paid for by:*
   three instances, three authors — zero `:443` (true only of the attributed 16), zero `:1518`
   (true only of requests whose status returned), completeness-verified (true only under a load
   regime excluding the drop mechanisms). **Its limit case: C's #74 evidence, where the population
   is EMPTY and always will be**, because the captured side was never on the log.
10. **"Independently derived" is weaker than it reads** — shared codebase and shared premises mean
    two paths through the same assumptions. Extends to APPARATUS. *Paid for by:* D's three
    "disjoint" apparatus that share one process, and my own B-P1/C-P2 seal.
11. **Run a new rule BACKWARDS over sealed rows as PART of adopting it**, aimed at the passages
    touched MOST. *Paid for by:* D-P6, my most-amended row, which called three probe-fed conditions
    "independent". **Deflated by D's own rule 9:** the hit rate may reflect a dense field of fresh
    claims rather than the rules' power — see D-META-1.
12. **A prediction must reach a DURABLE ARTIFACT before its instrument runs.** *Paid for by:* C-LC
    — the one row that bypassed this ledger is the one whose wording had to be reconstructed from
    a third party's quotation. **Extended by C to TREE PROVENANCE:** the measurement's
    CONFIGURATION is historical even when the RESULT is durable.
13. **Reading a suite tells you what is NAMED; breaking the property tells you what is PROTECTED.**
    *Paid for by:* a matched pair from one author in one day — the rendezvous filter (claim TRUE,
    sabotage fell) and divergence-by-sequence (claim FALSE, no sabotage fells it alone). Same
    sentence, two seeds, only the sabotage separated them.
    **General form (N):** *reading an assertion tells you what it SAYS and never what it CAN SEE —
    and that gap is invisible from the inside every time.*

**Amendment classes, settled by cases:**
- **INSTRUMENT change → new row** (N's fourth assertion; C's fixture rebuild).
- **Reading refined, pre-run and TIGHTENING → amendment** (C's exact-retry panic-site narrowing).
- **LOOSENING → new row regardless** — that is the direction that buys the author something.
- **OBSERVATION ORDER → amendment**, conditional on **nothing in the moved region** mutating what a
  later assertion observes. *My original condition said "no assertion" and was too narrow; C found
  a store open had moved with them.*

**Two further rules earned late:**
- **An observational trigger for an inferential conclusion** fires honestly and licenses less than
  its sentence claims. *Paid for by C-LC.* If the claim is about a RATE, the trigger must be too.
- **If a guard's INPUT is derived from the thing it CHECKS, it cannot detect that thing breaking.**
  Hardcoded fixtures are blind to production drift; projection-built fixtures are blind to fold
  corruption. **Different blindness, not less** — so "convert everything" would be a regression
  sold as a fix.
- **An unrun assertion is not a guard; an unverified sabotage is not evidence.** Both are claims
  about instruments.

---

## 3. What the ledger prevented — the honest summary

The ledger measured nothing. Its whole output is **verdicts that did not get written**, so the
value has to be stated as specific sentences that now have a written refusal waiting for them.

**Retrofits blocked.**
- **"The widening raised the flake-3 rate."** Sealed expected-UNINFORMATIVE *before* any number,
  with the reason: every planned instrument is deterministic and cannot report a probability. When
  the flake later stopped reproducing, this sentence had nowhere to attach.
- **"The storm is solved."** D-DT's own limit was hoisted into the table before the run: any DT
  sample leaves H2 and H3 unscored. The clean base then produced no elapsed data at all, so DT-1
  never even fired.
- **"C's fix alone gave 0/10."** True — and the tree it was measured on is no longer the branch
  tip, and the configuration is unreachable. The citation now carries its tree or it is stale.

**Misreadings pre-refused.**
- **"Sweeps 0/13 means window 3 is gone."** The belt is a legality-blind instrument whose own
  commit records it never reached the window. The deterministic seam tests have not run.
- **"0/10 means the sleeper flake is fixed."** It excludes rates above ≈26% and says nothing below;
  the two-phase race is structurally untouched; and the instrument that separates *gone* from
  *rare* is the deterministic delay hook, which no sampling result replaces.
- **"Isolated 0/10 plus suite 0/10 is 0/20."** Different base rates, different populations. Pooling
  manufactures a bound neither run earns.
- **"Recon answered it with zero machine time" as method advice.** C's own caveat, sealed with its
  precedent: reading answers what-is-in-the-log; **that is exactly how "the handle's lock spans
  validation and write" got believed for two milestones.** Four agents later refuted it
  independently. *The milestone opened on that defect and would have closed by teaching it.*
- **"Nothing tests X."** Rule 13: that is a report about NAMES unless someone sabotaged for it.

**Errors caught in flight.**
- **A rate with no measurement behind it — "~3 in 4" — reached F's inputs** and was stopped by N's
  catch, not by any rule. It matches no measurement at any scope. *Provenance rules make the catch
  possible; they do not make it happen.*
- **A wrong verdict of mine (H4 dead)**, caught by a peer's rule one round after I sealed it.
- **A defective cell of N's** that propagated to a downstream scorer — attributed to the cell's
  author **once**, not to each agent who scored against it.

**The measured finding that outranks all of the above:** **the belt reports GREEN with FOURTEEN OF
FIFTEEN consumptions missing.** It asserts "no double consume" and never asserts the consumptions
happened at all. That is *assert at the finest grain*, measured rather than argued, and it means
every green that belt has produced means less than it reads.

---

## 4. What remains open

- **Every rung-A row** — D-P5, D-P5e, D-P6, D-P8, D-P9, D-FALSIFIER, D-P10 — **UNSCOREABLE until
  the `caller=` tag exists.** None ran; **none is "did not fire".** Rung A is now written (three-way
  tag including DRIVER, begin/end rows, epoch micros, fail-loud sink, error kind not bool, two
  known-count controls) — and every one of those features is a repair from this ledger's own rules.
- **The storm MECHANISM.** H4/H5 aside, H1 vs H2 vs H3 is untouched, and D's pre-committed
  falsifier — *if median open cost × opens-per-request already exceeds the budget, C1 must not be
  adopted even though the storm would likely pass with it* — **cannot fire without the tag.**
- **The SF pair, needing no run.** **SF-1:** the #55 family's oracle is blind to any failure that
  keeps the log legal. **SF-2:** `ring` collapses every error shape into `StaleRendezvous`, so a
  live-but-busy sleeper's lease is consumed **and recorded** — a doorbell silently lost. Compounded
  by a protocol gap: **every committed consume payload carries `executionId`, `sessionId`,
  `reason` — no rendezvous, no arming identity**, so the data would not support the check even if
  the oracle wanted it.
- **C-P1, footnoted WITH ITS WAKE-UP CONDITION.** Nothing depends on it today. **If any lane ever
  proposes reverting `53d212d` as a mitigation, its empirical clause goes live**, and the decidable
  instrument is the seam test at the parent — never the belt at any commit. Its structural clause
  (the window predates `53d212d`) is not touchable by any rate.
- **A-M10.** The owner's three numbers still have no in-repo source. D-P9 may support the *shape*
  of the O(history) cost; it is not a reproduction of the magnitudes.
- **Flake-2 closure.** Not measured on the final tree. Criterion is suite N=20; my cells scale
  (0/20 → ≈14% upper bound, clear of the ≥37% base; ≥3/20 still reads as persistence).
- **D-META-1** — whether the rules or a dense field of fresh claims produced the hit rate. Design
  frozen, unrunnable while the files are warm.

---

## 5. What this file does not contain

- **No prediction of mine, except one:** the pin guard would not fall under the constant fold with
  converted fixtures. Registered before the run, CONFIRMED.
- **No measurement of mine.** I ran no cargo at any point.
- **No scoring authority.** I held the wording; the orchestrator and the reviewers held the
  verdicts.
- **Two credits I declined**, on my own precedent: the collateral warning and the baseline flag —
  both correct-in-principle, **neither exercised**, therefore untested rather than validated.
- **Rows I cannot score and have marked rather than guessed:** C-P2's red and C-R2's own sabotage
  were never reported to this ledger.
