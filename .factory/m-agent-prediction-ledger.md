# Prediction ledger — M09 flake stabilization

Author: M Agent. Sealed 2026-08-19, BEFORE H Agent's base measurement numbers existed.
Worktree `claude/m-agent-76c232`, branch tip `53d212d`; main at seal time `efd85d0` (#70).

## Why this file exists

Four agents registered falsifiable predictions in their own reports. Nobody owns scoring
them. Once H posts base numbers, every prediction can be re-read in the light of the
result — and a prediction re-read after the fact is not a prediction, it is a story. This
ledger fixes the wording, the source, and above all the DISCONFIRMING result, while the
result is still unknown.

Nothing here is measured by me. I ran no cargo. Every quote is verbatim from the file and
author named in its row; where I compressed, the compression is marked (M) and the verbatim
text is quoted separately.

## Scoring rules (binding on me, and on anyone who scores from this file)

1. **No retrofitting.** A row's CONFIRMS / KILLS / UNINFORMATIVE cells are frozen as
   written here. If a result arrives that fits none of the three, that is itself the
   finding: the prediction was not decidable, and it scores UNDECIDABLE — never
   retro-fitted into CONFIRMS.
2. **UNINFORMATIVE is a real outcome, not a failure to try.** It is filled in first,
   before the numbers, precisely so "the run was inconclusive" cannot be quietly
   re-described later as weak support.
3. **A prediction scores only against the instrument its own author named.** If the
   evidence arrives from a different instrument, record it, but score the row NOT-YET.
4. **The 4-piece rule applies to every scoring entry**: with-number, base-number, N,
   scope. A row scored without all four is returned unread, same as any other claim.
5. **Absence of failure is not confirmation.** "0 in N" scores only with N stated, and
   only against a prediction that named a rate.
6. **A retraction's REPLACEMENT is new work, and is reviewed as new work.** Contributed by
   L Agent from the storm-study review; adopted here as a scoring rule. When a claim is
   retracted, the question's shape changes and the measurement gets rebuilt to fit — and the
   rebuild can import a premise nobody reviewed, because reviewers are looking at the corpse.
   So for every retraction this ledger touches, two questions are asked, and a row whose
   instrument descends from a retraction cannot be scored until both are answered:
   (a) **what measurement or experiment changed because of the retraction?** Re-review that as
   new work, not as a correction.
   (b) **what did the retraction RETIRE?** An experiment killed as "now redundant" is where the
   loss hides — a retired experiment may have been valid under BOTH the dead and the
   replacement model, while its replacement is valid only under the new one.
   L's worked case is recorded at D-P7 above: retracting "the sweep's async half contends for
   the runtime thread" moved the sweep's cost onto pool-side contention, the deciding
   measurement was re-aimed at blocking-pool thread-id share, and that rebuild silently adopted
   "blocking pool == sweep" — false at `53d212d`. The sweep-ablation experiment retired as
   "redundant" is valid under either threading model; the thread-id share is not. The prose
   retraction was complete and correct; the defect is one step downstream of it.
   **AMENDMENT — THE PRESCRIBED TOOL HAS ITS OWN SILENT-FAILURE MODE (L, reproduced independently
   by F; verified by me).** On a HARD-WRAPPED document, grep for a hyphenated row ID fails whenever
   the wrap lands on the hyphen. **This is rule 7's AMBIGUOUS-ABSENCE sub-class sitting inside the
   instrument rule 6 prescribes** — and on exactly the document class the rule targets: long,
   hard-wrapped, full of hyphenated IDs. Seventh instance of the flattening family, **fifth in an
   instrument rather than the product.**
   - **My own verification, and it found a SHARPER variant than reported.** On
     `.factory/f-agent-m09-close-skeleton.md`: plain `grep -c "D-TRAP-1"` returns **3**, while
     `grep -c "TRAP-1"` returns **6**. L reported a zero; I could not reproduce the zero, but the
     mechanism is worse than a zero — **a PARTIAL silent loss.** A zero at least prompts "did I
     search right?"; **3-of-6 looks like a complete answer.**
   - **The obvious repair also fails** (F ran it): after `tr '\n' ' '` the token reads `D- TRAP-1`
     **with a join space**, so `D-[A-Z]+` still misses it. Working forms:
     `tr '\n' ' ' | grep -oE 'D-\s*[A-Z]+-?[0-9]*'`, or grep the distinctive **tail** (`TRAP-1`).
   - **AMENDED RULE:** rule 6's grep must be run over **whitespace-normalised text with a
     wrap-tolerant pattern**, and **a zero-hit result on a token that SHOULD appear is treated as
     INSTRUMENT FAILURE until the pattern is proven able to find a known-present instance.** That
     last clause is C's positive-control discipline, moved from sabotages to searches: **prove the
     instrument can see the thing before trusting it not to.**
   - **I RAN THE AMENDED CHECK ON MY OWN TWO FILES AND IT FOUND A REAL MISS (M).** No wrap
     failures — but my close-out wrote the row as `N-3a-v1 / v2`, so **a grep for the canonical
     `N-3a-v2` returned ZERO while the row was present.** Different mechanism (ID variant, not
     wrap), same silent absence, in the file I had just handed to F as evidence. Fixed to
     `N-3a-v1 and N-3a-v2`. **The amendment paid for itself on its first use, against its author.**
   - **THE SHARPEST FORM, and it beats both "a zero" and "a partial hit" — L's, verified by me at
     line level.** L's zero and my 3-of-6 were both real; the file moved between us, and **F's own
     write-up of the bug is what created every greppable instance.** Where the hits sit is the
     finding:
     - **Line 265 ends with the literal `**D-` and line 266 begins `TRAP-1 stays separate and
       LIVE...`** — that is **the substantive occurrence**, the standing-constraint sentence
       itself, and it is **still invisible to plain grep.** (Verified by me: `sed -n '265,266p'`.)
     - **Lines 332, 338, 341 are the three plain-visible hits — and all three sit inside the
       ship-gate paragraph F wrote to DOCUMENT the wrap blind spot.** (Verified: each line reads as
       prose *about* greppability.)
     - **So the visible hits are all self-referential prose about greppability, and the one hit
       carrying the constraint is the invisible one.** A reader grepping *"is D-TRAP-1 handled
       correctly?"* gets three confident hits, every one discussing how hard the token is to grep,
       **and never reaches the sentence saying the trap is LIVE and forbids fixed-vs-unfixed
       wall-clock comparison. The answer to the question is the row the search cannot see.**
     - **Why this generalises (L):** the failure is **ANTI-CORRELATED WITH SUBSTANCE** — prose
       *about* a token tends to be short and unwrapped, while a token used *inside a long argument*
       tends to land mid-line and wrap.
   - **THE LEAD GENERALISATION IS THE ID-VARIANT ONE, not wrapping (L endorses my framing):** **any
     transformation of a row ID between the file that DEFINES it and the file that CITES it yields
     the same silent zero.** Wrapping is merely the variant that **needs no author decision — it
     happens to you.** Abbreviation-at-the-citation-site (my `N-3a-v1 / v2`), case, and punctuation
     are the others.
   - **POSITIVE-CONTROL REFINEMENT (L), and I applied it to my own audit.** The control must use **a
     token from the passage MOST LIKELY TO WRAP** — an ID used mid-argument, not one from a heading
     or a table cell. **A control drawn from a short line proves the pattern works exactly where the
     failure does not occur** — L's analogy: *the same trap as pointing a latency guard at an empty
     store.*
     - **Checked against my own earlier audit:** of the IDs I tested, `D-TRAP-1` (3), `D-P10-ALT`
       (7) and `D-META-1` (1) do occur in mid-prose, so **my control was drawn from wrap-prone text
       and the clean result stands for those.** `C-MASK2` occurs **only in headings/tables (0
       mid-prose)** — that one proved nothing.
     - **VERDICT RESCOPED, per L, and it is my own rule 5 applied to my audit rather than to
       someone's guard.** My "no wrap failures in either of my two files" carried **one untested
       cell**: for heading-and-table-only IDs I had **absence of failure without an instrument
       shown able to produce failure there.** Corrected statement: **the clean verdict is scoped to
       IDs with at least one mid-prose occurrence; heading-only IDs are UNTESTED, not clean.** Same
       handling as C-P2 and C-R2 elsewhere in this file — **marked, not guessed.**
   - **THE SHAPE, three times in one thread, and its distinguishing property (L).** The control
     audit is **itself an instrument, and it inherited the defect it was built to find**: L's prep
     file (his own correction of D's overreach carried its own strong form), my close-out
     (`N-3a-v1 / v2`), and F's ship-gate paragraph (its own visible hits are the self-referential
     ones). **Distinguishing property: WRITING THE FIX IS ONE OF THE OCCASIONS THAT PRODUCES IT.**
   - **Attribution, as L set it and I accept:** the anti-correlation instance is his; **the class —
     any ID transformed between the defining file and the citing file yields a silent zero, with
     wrapping as the variant requiring no author decision — is the generalisation, and he credits
     it to me.** Recorded both ways so neither reads as sole authorship.
   **SIXTH INSTANCE, AND IT ISOLATES THE VARIABLE: THE REGISTER, NOT THE TOPIC.** D mis-stated an
   anchor in a message to L (claiming a PR shifted a line that was already at that line; it had
   inserted a line *inside* the block). **His ARTIFACT has it right; his MESSAGE did not.** Second
   instance today where the rot appeared in a message rather than a document.
   - **Generalised (D's, and it is the sharpest form the class has reached): THE REGISTER IS THE
     VARIABLE, NOT THE TOPIC.** Compression-mode writing carries the error regardless of subject —
     summaries, standing lines, chat messages, board rows. **Six instances across four authors, and
     what they share is the register they were written in, not what they were about.**
   **FIFTH INSTANCE, AND A NEW VARIANT: THE RETRACTED CLAIM CAME BACK THROUGH SOMEONE ELSE.** The
   orchestrator favoured adding a driver-exercising control, and **their premise was D's OWN
   SUPERSEDED CLAIM quoted back at him.** The survivor reached the board and **nearly bought a
   scenario.**
   - **Why this variant is worse than a survivor in your own file (M):** the author has a reason to
     grep his own documents, and none at all to grep someone else's message. **A retracted claim
     that round-trips through a third party returns LAUNDERED — it arrives carrying the third
     party's authority rather than the author's, and the author no longer recognises it as his.**
   - **Consequence for rule 6's grep clause:** grepping your own artifacts is necessary and not
     sufficient. **A retraction is not contained until the people you told it to have been told the
     retraction**, and there is no mechanical check for that — which is why D catching it required
     recognising his own dead sentence in someone else's proposal.
   - **THE ONE MITIGATION THAT EXISTS (D), added because "no mechanical check" invites giving up:**
     **when retracting something already broadcast, SEND THE RETRACTION TO EVERYONE WHO RECEIVED THE
     CLAIM. Propagation is part of the retraction, not a courtesy.** It is not a gate and will not
     catch a laundered return — **it shrinks the population that can launder one**, which is the
     most available here.
   - **FILED AS A LIMIT ON RULE 6, NOT AS A FOURTEENTH RULE — D's deliberate choice, and it binds
     me.** His reason ties to the rule-count caution: **a limit on an existing rule is worth more
     than another rule, and the count should stop growing.** I have been the one adding rules all
     day; this is the first entry that subtracts from that instinct rather than feeding it. Durable
     in project memory under `retraction-descendants.md`, attributed.
   **Structural reason this belongs to a reviewer, not an author (L's framing, kept):** D asked
   for survivors to be hunted precisely because he wrote both the claim and its retraction and
   is worst-placed to spot its descendants.
   **(c) A descendant need not be a claim or an experiment — it can be a DENOMINATOR.** Added
   by L after the full corpse audit, because without it the rule reads as "audit the prose and
   the experiments" and misses arithmetic that survives quietly inside a reading instruction.
   L's case is recorded at D-P6 below: the retracted 8×600ms arithmetic disqualified run wall
   clock as a measure of request time, and rung A's reading instruction then divides by run wall
   clock anyway. The corpse stayed buried; its arithmetic came back as the denominator.

7. **FLATTENING — if a value a verdict rests on can be produced by more than one upstream
   cause, and the consumer treats it as one, the row is UNSCOREABLE regardless of how the
   number comes out.** Contributed by L Agent, who identified it as the general form of the two
   ad-hoc kills already in this file rather than as a third rule: D-P7 died because a thread id
   means two callers, and D-P5 form 2 died because "3 opens" means two outcomes. Same test,
   twice.
   **The shape, in L's words:** a boundary maps MANY distinct causes onto ONE legal value, and
   nothing downstream can recover which. Not "an error was swallowed" — the value is well-formed,
   in range, and every surface stays calm. **The loss is the DISTINCTION, not the signal.**
   **Four instances found in this milestone, in three lanes, two of them PRODUCT surfaces and
   two INSTRUMENT surfaces:**
   - SF-1 (C's, wake/#55 family) — the replay oracle is blind to any failure that keeps the log
     legal. *Product.*
   - SF-2 (L's, `serve/wake.rs:57`/`:59`) — every error shape becomes `StaleRendezvous`, so a
     missing pipe and a busy pipe are the same value. *Product.*
   - `get_status` phase laundering (D's, `api_http.rs:219-222`) — connect, write and read
     failures become one panic string, and on Windows the `ErrorKind` is `TimedOut` for two of
     them. *Instrument.*
   - rung A's `ok=bool` (L's G8) — open-failed and open-slow share a field. *Instrument.*
   **DIMENSIONAL, not total (M's addition, adopted by L into the rule).** Flattening destroys
   exactly the questions asked IN THE FLATTENED DIMENSION and leaves the others legible.
   `get_status` destroys the PHASE and keeps the CODE, which is why H5 (a code-shaped hypothesis)
   still dies by data while H4 (a phase-shaped one) cannot. **Payoff: after finding a flattened
   boundary, re-audit the verdicts asked in that dimension, not every verdict downstream.** This
   converts the rule from a warning into a bounded work item.
   **TWO SUB-CLASSES, because the repairs differ (L, round 4):**
   - **COLLAPSING PRESENCE** — a value arrives and many causes could have produced it.
     Instances: `get_status` phase, `ring` → `StaleRendezvous`, thread-id-as-caller, `ok=bool`,
     3-opens-two-outcomes. **Repair: WIDEN the value** — add `caller=`, add phase, add error kind.
   - **AMBIGUOUS ABSENCE** — NOTHING arrives, and "nothing happened" is indistinguishable from
     "the evidence was destroyed". Instances: D-P10-ALT's tempdir deleting on unwind; L's G1 on
     rung A, where `record()` runs only after `open` RETURNS, so an open parked forever on the
     timeout-free `lock_exclusive` writes NO ROW — a run with no long-open rows reads identically
     whether opens were fast or one hung forever. **Repair: make absence IMPOSSIBLE or
     SELF-EVIDENT** — begin/end rows so a begin with no end IS the evidence; preserve the events
     dir on every run and print the path.
   L notes both repairs were proposed independently before the class existed, which is mild
   evidence the split is real rather than imposed.
   **Tally by surface: SIX instances, FOUR of them in the INSTRUMENTS, not the product.**
   **The half worth carrying to the owner report (L's flag, and I agree):** we have been
   treating flattening as a PRODUCT defect class, and half the instances are in the TOOLS WE
   JUDGE THE PRODUCT WITH. An instrument that flattens does not fail loudly — it produces a
   confident wrong reading, which is strictly worse than no reading. **This rule has already
   cost this ledger a wrong verdict: see the H4 self-correction in Scoring round 1.**

8. **A rate prediction must name the N that would FALSIFY it, or it is nearly free.** D's rule,
   taken from D-P4's scoring: at N=10 nothing near 50% was killable, so a confirmation there was
   worth very little. He notes he should have frozen an N with P4 and did not. Applies to every
   future rate row in this file. C-P1's late-registered N>=10 is the model.

9. **Every "ZERO X" claim must name the POPULATION in which X was OBSERVABLE.** If the
   hypothesis at stake lives outside that population, the zero is **SILENCE, NOT EVIDENCE.**
   D's formalisation of a pattern that hit this lane three times, three different authors:
   - *zero `:443` connect panics* — true of the 16 **phase-attributed** panics, while H4 lived in
     the 4 laundered ones.
   - *zero `:1518`* — true of requests **whose status returned**, while committed-then-failed
     hides among the 20 that timed out.
   - *completeness verified* — true under a **load regime that excludes** the very drop
     mechanisms it was meant to rule out.
   **Compressed form: ABSENCE IS EVIDENCE ONLY INSIDE THE WINDOW THAT COULD HAVE SHOWN
   PRESENCE.**

10. **"Independently derived" is weaker than the phrase reads, and the ledger uses the deflated
    form.** D's strengthening of a precision I had stated too gently. Two agents who share a code
    base, a set of rules they built together the same day, and every prior finding are **not
    independent in the sense the word usually carries.** What such agreement gives is **two paths
    through the same shared premises** — worth something for catching arithmetic, **and nothing
    at all for catching a wrong premise both hold.**
    - **Applied retroactively, per rule 6's grep clause.** I grepped every independence claim in
      this file. The one that matters is the **B-P1 / C-P2 cross-check**: I had sealed that they
      are "not independent evidence of each other's correctness, but ARE independent derivations
      of the same window." The second half now carries this deflation — B and C read the same
      code and worked inside the same shared premises, so their agreement does not license
      confidence in the window's existence beyond what either derivation licenses alone. The
      seal's operative half (a result killing one must kill the other) is unaffected.
    - **Same deflation applies to D-P10-ALT's two routes** (L's drop-detection and D's
      evidence-preservation), already recorded there, and it is why that row's corroboration is
      of an INSTRUMENT CHOICE only.
    - **What survives undeflated:** convergence still shows a finding is **FINDABLE**. D's own
      example — he and I narrowed D-C1 identically without contact — is right for that reason and
      no other. The narrowing is correct because the doc comment at `api_http.rs:1514-1516` says
      so, which either of us alone would have established.

    - **EXTENDS TO APPARATUS, not just reasoning (D).** He had called his three apparatus
      "DISJOINT failure modes"; they are not. All three are produced downstream of the same
      harness and the same server process, so a failure early enough — server never binds, storm
      never starts — blinds all three at once. What differs is how each fails ONCE THE RUN IS
      UNDER WAY. Corrected to **"different failure modes, one shared upstream"**, which still
      supports the cross-checks and claims less. **Two apparatus fed by one process are less
      independent than the word suggests.**

11. **Run a newly adopted rule BACKWARDS over already-sealed rows as PART of adopting it.** Not
    after someone notices. Evidence: three consecutive times, across three files and three
    authors, a rule adopted here immediately found an error in the file adopting it — rule 7
    found my H4 verdict; rule 6's grep clause found two stale summaries of mine; rules 9 and 10
    found two hits in D's report and one in this ledger the same hour.
    - **D's amendment, and it is the operative half: aim backward application at the passages you
      have touched MOST, not least.** Both of his hits were in claims he had ALREADY corrected
      once — H6 was raised, dismissed, then re-examined for the instrumentation constraint; the
      apparatus set was built, reviewed and narrowed twice. **Attention creates confidence, and
      confidence is where the unexamined premise hides.**
    - **Tested on this file immediately, per the amendment.** The most-touched row here is D-P6
      (amended four times). Applying rule 10 to it found the deflated word: its three conditions
      were called "independent" and share one upstream, the probe. Recorded at that row. The
      amendment predicted where to look and the prediction held on first try.
    - **DEFLATION OF THE EVIDENCE FOR THIS RULE — D's, and it is his own rule 9 aimed at our own
      tally. Adopted.** "Four consecutive rules each found an error in the adopting file" is being
      read as evidence the rules are good. **It is at least as much evidence that we have just
      produced a DENSE FIELD OF UNEXAMINED CLAIMS** — most of this material was written today, at
      speed, under review pressure. The hit rate may say more about the density of fresh,
      lightly-examined premises than about the rules' power.
      **"Four consecutive hits" names a population: claims written in the last few hours, under
      active revision.** Absence of hits in a stable file would not refute the rules; presence of
      hits here does not establish them.
      Rule 11's targeting amendment carries the same deflation: one confirmation, on the
      most-touched row, of a file written today. **It should be tested once on an old, cold file
      before being treated as general.**

### D-META-1 — the tally's own falsifiable form (D's prediction, sealed by M)

- **Prediction (D, on record):** if the RULES are doing the work, the hit rate stays roughly
  constant as the files stabilise. If the DENSITY was doing the work, it falls sharply once we
  stop writing new claims. **D predicts it FALLS**, and pre-commits to reading that as the rules
  working correctly rather than failing.
- **CONFIRMS (density explanation):** a rule adopted over a settled, cold file finds nothing.
- **KILLS (density explanation):** hit rate holds up on files nobody has touched in weeks.
- **UNINFORMATIVE (M, and this is the cell D could not fill for himself):**
  - **Search effort not held constant.** When we stop writing new claims we also stop *reviewing*
    — so a fallen hit rate is confounded with *having looked less*. The comparison is only valid
    if the backward pass over the cold file is as deliberate as the ones run today. Without that,
    "the rate fell" and "we searched less" are the same observation, which is **rule 7 at the
    design altitude**: one number, two upstream causes.
  - **The tester is the file's own author.** Rule 11's amendment says attention breeds confidence;
    an author re-reading a cold file of his own carries the same blind spot that made the passage
    stale. A cold-file test run by a peer and one run by the author are different experiments.
  - **The adopted rule is weak or off-topic for that file.** A rule finding nothing in a file it
    does not apply to measures nothing.
- **Ledger note (M):** this is the first row in the file whose subject is **the file's own
  method** rather than the product. It belongs here for exactly the reason every other row does —
  it was written before the result, with the disconfirming outcome named by its author, who also
  pre-committed to how he would read the outcome he expects.
- **DESIGN FIXED BY D after I flagged confound 1 as fatal — the confound is engineered OUT, not
  caveated.** He accepted that "hit rate per rule adoption" is one number with two upstream causes
  (rule power, search effort) and that effort collapses to zero exactly when the files stabilise,
  making a null result uninterpretable. The repair changes the UNIT so effort sits INSIDE it:
  - **Unit:** hits per **BACKWARD PASS**, where one pass = grep a **pre-registered token list**
    over one file and re-derive every hit. Not hits per adoption, not hits per hour.
  - **Effort held constant by construction:** fixed token-list size, fixed instruction, and
    **PASSES ATTEMPTED recorded alongside hits, always** — so a zero reads as "we looked and found
    nothing" rather than "we stopped looking".
  - **Pre-registration turns my confound 3 into a GATE:** the file, the token list, and an
    applicability statement are written before looking. If the rule could bite no claim in that
    file, the file is **OUT OF SCOPE and the run is VOID** — not a null result.
  - **My confound 2 becomes an ARM, not a caveat:** run the cold file **PEER-FIRST, then author**,
    same token list, reported separately and never pooled.
- **The arm is worth more than the original question (D's judgement, and I agree).** If peer-run
  finds hits where author-run found none, that **measures the blind spot directly** instead of us
  reasoning about it — it turns rule 11's attention-breeds-confidence claim from a heuristic into
  something with an experimental arm. D would keep this design even if D-META-1 is never scored.
- **Additional pre-commitment D volunteered:** if PEER-run finds hits where AUTHOR-run found none,
  that is evidence for rule 11's amendment **and against his own "it falls" prediction being about
  rule power at all** — it would mean the rate tracks WHO LOOKS, not what is there. Named in
  advance rather than argued about later.
- **Status:** OPEN, design frozen. Not scoreable in this milestone; the files are still warm.

### NAMED GAP IN THE RULE SET — logged deliberately unfilled

**Every rule in this file detects a claim that is WRONG. Not one detects a finding that is
CORRECT BUT MIS-CATEGORISED.**

- **How it surfaced.** D noticed that recording "removes a guarantee" and "locates a guarantee
  that was never articulated" both as *corrections* is a scoring defect that **predates today's
  rules and that none of them would have caught.** It is a property of the rule set, not a single
  miss.
- **Why it matters:** a ledger optimised entirely for wrong-claims accumulates mis-categorised
  ones **silently**. Mis-categorisation is exactly the failure that makes a review look more
  damaging than it was, which distorts what the next reader believes the lane's state to be.
- **One structural observation of mine, offered as a pointer and NOT as a rule (M).** The shape of
  the gap is visible in the file's own vocabulary: this ledger has a rich, carefully-argued set of
  words for PREDICTION outcomes — CONFIRMED, KILLS, UNINFORMATIVE, NOT-YET, UNDECIDABLE,
  UNSCOREABLE, WITHDRAWN, VOID — and exactly **one undifferentiated word for FINDINGS**:
  "correction". The asymmetry is where a future rule would go. I am not writing that rule here.
- **Deliberately unfilled, at D's call and with my agreement:** better a named gap than a rule
  invented badly at the end of a long day. Recorded so the next reader inherits the gap as a known
  hole rather than discovering it as a surprise.

12. **A prediction must reach a DURABLE ARTIFACT before its instrument runs, or it has no
    provenance at all.** Found by C, from the wrong end of his own rule: *cite-or-mark*, as he
    proposed it, covers numbers in durable docs — **it does not cover a PREDICTION that never
    reaches a durable doc.** C-LC lived in two chat messages, in two languages, with differing
    elaborations, and entered his study only after it died. It is citable today only because
    someone happened to ask him for the wording.
    - **The cheap fix already exists and this file is it.** C, D and J registered directly here
      and their rows are verifiable; C-LC is the one row that bypassed the ledger, and it is the
      one row whose wording had to be reconstructed from a third party's quotation. **The ledger's
      value was demonstrated by the single row that skipped it** — which is better evidence for
      the mechanism than any of the rows that used it.
    - **Chat is not registration.** A prediction sent to a peer so they can "hold you to it" is a
      promise, not a record — the peer's copy, the author's memory, and the eventual quotation can
      all drift, and nobody notices while the prediction is still alive.

13. **Reading a suite tells you what is NAMED; breaking the property tells you what is PROTECTED.**
    C's rule, from a sabotage that cost him his own contribution. **Anyone writing "nothing tests
    X" is reporting NAMES unless they sabotaged for it.** A gap in test names is not a gap in
    coverage, and only a sabotage list separates them.
    - **The evidence is a matched pair from one author, one day.** C made the same claim twice.
      About the **rendezvous filter** it was TRUE — the sabotage fell and no other guard caught it.
      About **divergence-by-sequence-alone** it was FALSE — he could not build a sabotage that
      felled his test alone (2 failed on one arm, 7 on the other; the invariant was already held
      by guards that never mention it). **Same sentence, two seeds, and only the sabotage
      separated them.** Neither could have been told from the suite's names.
    - **THE PAIR IS NOT SYMMETRIC — caveat added by C, who is credited with the rule and verified
      this in source rather than conceding it when N raised the distinction.**
      The TRUE half (rendezvous filter) is clean: sabotage fell, exactly one guard.
      The FALSE half establishes **"the property's removal is DETECTED"**, NOT **"a guard TARGETS
      it"**. C checked the five extra failures and they are **COLLATERAL, not aimed**:
      `identical_idempotency_key_is_independent_across_streams` is about cross-stream/scope key
      independence, all at sequence 1, digest never reached;
      `concurrent_writers_serialize_and_only_one_claims_the_expected_sequence` is about sequence
      claiming via the CAS, with DIFFERENT keys, and the idempotency digest is not on its path.
      **For rule 13's purpose that is still enough — reading names failed to predict red — but a
      reader taking the pair as symmetric would overstate the second half.**
    - **Sharpening that follows (M), building on C's own correction rather than against it:** an
      UNEXPLAINED red establishes that **something depends on the field**, not that **the property
      is protected**. A test can fail through a coupling unrelated to the invariant. So "the
      invariant is already protected by guards that never mention it" is the strong reading, and
      **"removal is detected" is the one the evidence carries** — which is exactly where C landed
      once he looked.
    - **THE GENERAL FORM, and it is the best statement anyone reached today — N's, written after
      his fourth instance:** **reading an assertion tells you what it SAYS and never what it CAN
      SEE — and that gap is invisible from the inside every time.**
      His own framing of why the count matters less than the pattern: it is not that he kept being
      wrong about coverage; it is that he spent the day proving this about other people's guards
      and hit it four times in his own reasoning, each time from the inside.
      This unifies most of the file: rule 13 (names vs protection), the belt green with 14 of 15
      consumptions missing, C's five reds he cannot trace, and the owner's existing
      *assert-at-the-finest-grain* — which is this same statement from the writer's side rather
      than the reader's.
    - **The consequence, in N's words and sharper than mine:** *if the person who broke it on
      purpose cannot explain the reds, a maintainer who breaks it by accident has no chance.*
      That is the diagnosability argument MEASURED rather than argued, and N notes it replaces a
      tripwire claim of his that died — stronger than the thing it replaced.
    - **Operative for whoever scopes work off a seed:** a seed that says "nothing pins X" is a
      hypothesis about coverage, and it is cheap to test and expensive to assume.

**RULES 7 AND 8 ARE ONE RULE, asked at three altitudes** (L's unification, relayed by D;
adopted, with one amendment of mine). The single question is: **does this observation admit more
than one state of the world?**
- Asked of a **FIELD** — can more than one upstream cause produce this value? (rule 7, flattening)
- Asked of a **DESIGN** — could this instrument have produced the other answer? (D-P0's laundered
  rows; L's G1, where a hung open writes no row at all)
- Asked of a **STATISTIC** — at this N, does the interval admit both the predicted and the
  contrary rate? (rule 8; D-P4's ~12-74% span admitted effectively one answer, so its
  confirmation carried no information)
**My amendment, and it is why they stay numbered separately below the unification:** the
diagnostic question is one, but **the REPAIRS are not interchangeable.** A flattened field is
repaired by WIDENING THE VALUE and no amount of extra sampling fixes it; an underpowered
statistic is repaired by MORE N and no amount of widening fixes it; an absence-blind design is
repaired by making absence SELF-EVIDENT. Collapsing the repairs would be the error — the same
shape as L's own presence/absence split, which is one class with two repairs. So: **one test,
three altitudes, three repairs.**

## Status legend

`OPEN` — sealed, no result yet. `SCORED` — result in, cell matched, evidence cited.
`NOT-YET` — evidence exists but from the wrong instrument. `UNDECIDABLE` — result fit
no sealed cell. `SUPERSEDED BY INDEPENDENT MEASUREMENT` — the row's own instrument never ran and
never will score it (rule 3), but the question it asks has been answered by someone else's; the
row stays permanently unscored and the slot is released. `VOID` — a run whose positive control or
premise failed, so it scores nothing and is not a result. `RETIRED-NOT-ANSWERED` — a fix removed
the condition the row asked about, so the question can no longer be measured and no longer needs
to be; distinct from SUPERSEDED, where someone else's instrument answered it.

---

## A. Flake #3 — `concurrent_sweeps_never_double_consume_a_lease`

### C-P1 — the window predates the widening

- **Author:** C Agent. **Source:** `.factory/c-agent-wake-flakes-study.md` (c-agent-e82f40
  worktree), stash-assessment section.
- **Verbatim:** "prediction registered: the flake reproduces on the parent of 53d212d"
- **Context (verbatim, same paragraph):** "The window exists at the parent too — consistent
  with the flake being measured on base. The fix is right; the story about when the bug was
  born is wrong, and it matters because it implies reverting 53d212d would restore safety,
  which reading says it would not."
- **CONFIRMS:** the flake reproduces at the parent of `53d212d` at a rate whose confidence
  interval overlaps the `53d212d` rate, N stated for both.
- **KILLS:** 0 failures in N at the parent, with N large enough to have caught the
  branch-tip rate (i.e. N such that P(0 failures | branch rate) is small). That result says
  the widening DID create the exposure, and both C's and B's structural analyses lose their
  shared premise.
- **UNINFORMATIVE:** any N too small to distinguish the two rates; or a parent run that
  fails for a DIFFERENT assertion text than the branch-tip failures (different mechanism,
  same test name, tells us nothing about this window).
- **Re-frozen by C in his ledger registration (2026-08-19), with a sharper kill than my own
  (verbatim):** "the concurrent_sweeps flake reproduces at the PARENT of 53d212d. If it does
  not, the stash's story that widening the read lock reopened #55 survives and my reading of
  it as false is wrong."
- **AMENDED after C's pushback — my original consequence was sealed TOO BROADLY, and C is
  right.** I had sealed: a C-P1 kill revives the stash history note and forces gate G7
  rewritten. That merged two claims with different evidence classes, and as sealed it invited
  a scorer to strike a verified structural finding on the strength of a timing measurement —
  the exact inversion this milestone exists to prevent. Corrected split, C's wording:
  - **"The window PREDATES 53d212d" is STRUCTURAL.** It comes from reading — `with_lock` takes
    and releases per call; `open` releases both locks before returning — derived independently
    by C and by B, at both commits. **No flake rate can refute it. A run cannot make a released
    lock held.** C-P1 cannot touch this clause.
  - **"Therefore reverting 53d212d restores nothing" is EMPIRICAL**, and is the clause C-P1 can
    hit. If the widening materially raised the hit rate, reverting WOULD reduce exposure while
    still not closing the window.
  - **Sealed consequence, corrected:** a C-P1 kill forces the SECOND clause rewritten before
    landing — from "restores nothing" to "reduces exposure without closing the window" — and
    leaves the first standing. **G7 does not become false; it becomes INCOMPLETE.**
- **RULING (M, asked for by the orchestrator): C-P1 is NOT killed, and was not measured.** The
  orchestrator read H's 0/10 as firing C-P1's kill cell. It does not, on two independent grounds,
  and I ruled CORRECT rather than confirm:
  1. **Instrument mismatch (rule 3).** C-P1's instrument is the CONCURRENT_SWEEPS test at the
     PARENT of `53d212d`, N>=10. H's latest 0/10 is the SLEEPER test at `aac0d67` — different
     test, different commit. H's earlier sweeps 0/10 is at `53d212d`, the branch TIP, not the
     parent; H said so himself in his report ("targets the PARENT of 53d212d — untested here").
     **C-P1 is OPEN and UNRUN** — not killed, and not unscoreable-by-ambiguity either.
  2. **"Kills my structural reading" inverts a correction C already won.** No rate can touch the
     structural clause; only the empirical one is hittable. Even a genuine parent 0/10 leaves G7
     INCOMPLETE, not false. See the amendment immediately above, which C argued against my own
     over-broad seal.
- **AND C-P1 MAY NOW BE UNSCOREABLE IN PRACTICE — B predicted this before anything ran.** C-P1's
  CONFIRMS cell requires the flake to REPRODUCE at the parent, but the comparison side at the
  branch tip is already **0/10** (H, round 1). B's memo said it in advance: statistical re-running
  of `concurrent_sweeps` "would compare 0/N against 0/N and prove nothing". My sealed UNINFORMATIVE
  cell — "any N too small to distinguish the two rates" — covers it: with 0/10 at the tip there is
  **no rate to distinguish**. A parent run at N=10 most likely returns UNINFORMATIVE, not KILL.
  **If C-P1 is to be decidable, the instrument is not the belt at any commit — it is C's
  DETERMINISTIC SEAM TEST at both `efd85d0` and `53d212d`, which is B-P1's sealed prediction and
  has never been run.** That answers the same question with no rate at all.
- **LATE AMENDMENT — N registered by C after sealing, before any run (flagged as late, per
  rule 1).** C-P1 shipped with no N, so a non-reproduction could not be scored at all: absence
  of reproduction is not evidence of absence at an unstated sample size. C registered the
  missing number himself, at the only honest moment for it. Sealed: **N >= 10 at the parent is
  the minimum for a C-P1 kill.** His arithmetic, kept: with the flake near 1 in 3, 0 hits in 10
  is roughly 1.7% and scores KILL; 0 hits in 3 is about 30% and scores UNINFORMATIVE, never
  kill. Recorded as a late amendment rather than folded in silently — the author supplying a
  missing decidability condition before the numbers exist is exactly what this file is for, and
  hiding that it arrived late would defeat the point.
- **Status:** OPEN.

### C-P2 — the deterministic red, predicted before its first run

- **Author:** C Agent. **Source:** same file, "The red this must produce BEFORE any fix
  code (predicted, to be OBSERVED)".
- **Verbatim:** "Predicted failure: `recorded == 1` AND replay returns the fold's corrupt
  error — both assertions fail, unambiguously. If the red does not appear, F1-H1/window-3
  is wrong and the whole design stops there."
- **CONFIRMS:** both assertions fail together — `recorded == 1` AND
  `graphhelm_events::replay` returns the fold's corrupt error.
- **KILLS:** the test is GREEN. Explicitly: green means window 3 does not exist and the
  planned fix has no premise. It does NOT mean "the seam needs more tuning".
- **UNINFORMATIVE:** exactly one of the two assertions fails; or the test fails for a
  build/harness reason (panic outside the assertions, timeout, the rival's mid-call open
  hanging — C flagged this hang mode himself in the typed-test note). Also uninformative if
  the run is on a tree carrying any fix code, since the prediction is scoped to the CURRENT
  two-read shape.
- **AMENDED by C's frozen registration (2026-08-19, before any run; recorded as an amendment,
  not a rewrite — it adds precision and does not move the claim).** Test now named:
  `a_rival_consume_between_validation_and_the_sequence_pin_appends_nothing`, in
  `apps/cli/src/commands/serve/wake.rs` in-module tests; worktree c-agent-e82f40, branch
  `c-study-arming-the-alarm`, base `53d212d`; status typed, unbuilt, unrun. Fixture
  (verbatim): "lease armed at seq 1; a seam hook fires after the still-live filter and before
  the next_sequence pin; the hook appends a rival WakeLeaseConsumed at seq 2; our recorder
  then pins and appends at seq 3."
  - **Firing order now pinned (verbatim):** "`recorded == 1`, so assert_eq!(recorded, 0) fires
    first, left 1 right 0. Secondary, if that assertion were removed: replay returns the
    fold's corrupt error". This upgrades the row: my sealed UNINFORMATIVE cell said "exactly
    one of the two assertions fails" — that is now sharpened, because the recorded-count
    assertion is expected to fire FIRST and mask the replay assertion. A run showing ONLY the
    recorded-count failure is therefore CONFIRMS, not UNINFORMATIVE. A run showing the replay
    failure WITHOUT the recorded-count failure remains UNINFORMATIVE.
  - **THIRD OUTCOME, pre-registered by C (verbatim):** "a HANG. That would mean a live store
    handle does hold a lock between operations, refuting both my reading and B's. It gets
    REPORTED as a finding. It must never be patched with a timeout or a sleep." Sealed here
    as its own outcome: a hang scores **KILLS the lock-release reading shared by C's study and
    B's memo** — it does not score as a flaky/inconclusive run, and the ledger refuses any
    result where a hang was made to disappear by adding a timeout or sleep.
- **Status:** OPEN.

### C-R2 — the pin-guard, predicted GREEN today (a guard, not a defect)

- **Author:** C Agent, frozen registration 2026-08-19, unbuilt and unrun. **Test:**
  `a_stale_capture_never_burns_the_lease_that_replaced_it`, same file as C-P2.
- **Fixture (verbatim):** "session-p armed on rdv-old at seq 1, re-armed on rdv-new at seq 2;
  the recorder is fed a stale capture naming rdv-old."
- **Verbatim prediction:** "PASSES TODAY. `recorded == 0`, and the projection still holds
  session-p live under rdv-new. This one pins existing behavior, so green-first is correct —
  red-first is for defect-driven tests only."
- **Its red is the sabotage (verbatim):** "drop the rendezvous comparison and match on session
  alone. PREDICT under that sabotage: FAILS with `recorded == 1`. Nothing else in the suite
  falls — that is precisely why the guard is needed; the rendezvous half of that filter is
  unpinned today."
- **CONFIRMS:** green today AND red under the sabotage, with the sabotage reported per the
  board's sabotage-evidence rule (file:line, what changed, exact cargo invocation, N,
  per-guard pass/fail list).
- **KILLS (C's own wording):** "red today (the filter does not work as read), or green under
  the sabotage (the guard measures nothing and must be rewritten, not kept)."
- **UNINFORMATIVE (sealed by M):** green today with the sabotage NOT run. A green-first guard
  with no observed red is exactly the failure mode the board's sabotage rule exists to stop —
  the assertion may be passing for a reason unrelated to the rendezvous comparison. Also
  uninformative if the sabotage turns some OTHER test red first, since C's claim is
  specifically that "Nothing else in the suite falls"; another test falling means the guard's
  uniqueness claim is unproven even if this test goes red.
- **Status:** OPEN.

### C-R3 — the rdv-EQUAL burn, staged for the SECOND PR

- **Author:** C Agent, frozen registration 2026-08-19, typed, uncompiled, unrun. **Test:**
  `a_capture_from_before_the_wake_never_burns_the_lease_armed_after_it`, parked at
  `.factory/c-agent-rdv-equal-red-draft.rs` — **outside the crate on purpose** (verbatim:
  "defect-driven, so red by construction; in-crate it would fail PR 1's own gate on a defect
  PR 1 does not fix").
- **Fixture (verbatim):** "session-p armed on rdv-fixed at seq 1; rung; sleeper re-arms on the
  SAME rdv-fixed at seq 2; the first sweep's delayed phase 3 runs with the pre-wake capture."
- **Verbatim prediction:** "FAILS. `recorded == 1` (assert fires first, left 1 right 0), and
  session-p is GONE from the projection — the fresh lease burned." Plus the distinguishing
  detail (verbatim): "replay still SUCCEEDS. No corrupt error, no refused stream. Consuming a
  LIVE lease is legal, which is exactly why this failure is silent while window 3 is loud."
- **CONFIRMS:** `recorded == 1`, session-p absent from the projection, AND replay succeeds —
  all three. The third is what separates this defect from window 3.
- **KILLS (C's wording):** "GREEN. Then the seed dies rather than being patched."
- **UNINFORMATIVE (sealed by M):** it fails AND replay returns the fold's corrupt error. That
  outcome does not distinguish this seed from C-P2's window 3 — the two defects would be
  indistinguishable by their signature, and the claim that this failure is silent while window
  3 is loud goes unproven. Record it, score NOT-YET, and re-derive the fixture before
  treating the seed as a separate defect.
- **Ledger note (M):** C-R3 is the only row on the board predicting a **silent** failure —
  one that leaves the stream replayable and therefore raises no operator signal at all. If it
  confirms, the milestone has a defect class that no existing oracle catches, which is a
  finding independent of whether it ships in this PR or a later one.
- **SCORED — C-R3 RED OBSERVED, two clauses of three.**
  - `a_capture_from_before_the_wake_never_burns_the_lease_armed_after_it` FAILED, panicked at
    `apps/cli/src/commands/serve/wake.rs:719:9`, **left: 1, right: 0**. Suite: 4 passed, 1 failed —
    the three existing #55 guards and the pin-guard all stayed green.
  - **`recorded == 1` → OBSERVED.**
  - **`replay STILL SUCCEEDS` → OBSERVED.** The panic is at the COUNT assertion, so the legality
    assertion above it PASSED. **This is NOT NOT-YET: it is the silent defect, not window 3.**
    That was the clause sealed as the one separating the two, and it separated them.
  - **`session gone from the projection` → NOT OBSERVED.** It sits after the count and was masked.
    C can infer it from `recorded == 1` plus the fold's remove-on-consume arm, **and refuses to log
    an inference as measured.** He also refuses to reorder further to chase it, because the count
    is the primary claim and belongs where a reader expects it. Both calls kept.
- **RULING ON THE ASSERTION-ORDER CHANGE — mine, as C asked, and it goes his way for a reason he
  did not give.** He moved the legality check BEFORE the count in the transplanted test, pre-run,
  because in the parked order the count panicked first and **the clause separating this defect
  from window 3 could never be observed on a red run.** Nothing added, weakened, or removed. He
  declined to self-classify because "amendment" is the reading that favours him.
  - **Neither existing category fits, and forcing it would be wrong.** *Instrument change* is for
    measuring a different thing; the properties here are identical. *Amendment* was written for
    refinements to the READING; assertion order sits inside the instrument.
  - **Named as a third category: OBSERVATION ORDER.** It changes neither what is measured nor how a
    result is read — only **which of several simultaneously-true facts survives to be seen**, since
    the first failing assertion aborts the rest.
  - **RULING: AMENDMENT, not a new row — CONDITIONAL on the reordered assertions being
    side-effect-free with respect to each other.** That condition is the whole safety argument:
    reordering cannot make a failing assertion pass, so each assertion's truth value is
    independent of position **only if no assertion mutates what a later one observes.** Here the
    legality check is a read-only replay and the count is a captured variable, so the condition
    holds — **but C should confirm it explicitly, because if any reordered assertion had a side
    effect, the reorder WOULD change what the later one measures and it becomes an instrument
    change.**
  - **Why it cannot be abused:** the move strictly increases what is observable in BOTH directions.
    Had legality failed, it would panic first and show window 3's signature instead. A reorder that
    could only reveal the author's preferred clause would not qualify.
  - **CONDITION CONFIRMED BY C — and he found my wording too narrow. Corrected.** He verified the
    assertions are pure reads: the legality assertion is `read_replay_stream` (shared-lock read)
    plus a pure fold; the count tests `recorded`, captured BEFORE either read and unchangeable
    after `record_consumptions` returns; the lease-survives assertion reuses the same `replayed`
    value rather than re-reading. **No reordered assertion mutates what a later one observes.**
  - **But something that is NOT an assertion moved with them, and C disclosed it rather than
    letting a clean answer stand.** The reorder also moved a **store open**
    (`crate::commands::event_store(events)`) earlier. **Open is not read-only in general** —
    `open_inner` runs recovery, which can republish active markers. In this fixture it writes
    nothing, and C checked rather than assumed: recovery republishes only from
    `GraphVersionPublished`, and his fixture appends `WakeLease` and nothing else — **zero
    occurrences of that kind in the file**, so recovery has an empty set and performs no write.
  - **MY CONDITION WAS SCOPED TO ASSERTIONS AND SHOULD HAVE BEEN SCOPED TO THE MOVED REGION (M).**
    Corrected wording: *nothing in the moved region may mutate what a later assertion observes* —
    assertions and everything else that travels with them. **The altitude pattern again, on my own
    rule**: I covered the instance's shape (assertions) and missed the thing one level out
    (whatever else moves alongside). Found by the person the rule was applied to, which is how
    every other instance of it was found today.
  - **Carried forward as a fixture-dependent condition, not a settled one:** if that fixture ever
    gains a published graph version, **the moved open becomes a write** and this confirmation must
    be re-checked. C's caveat, recorded so a future edit to the fixture does not silently void the
    ruling.
- **Status:** SCORED — RED OBSERVED, 2 of 3 clauses; third masked and left unmeasured by choice.

### C-P3 — the journal fingerprint of F1-H1

- **Author:** C Agent. **Source:** same file, F1-H1.
- **Verbatim:** "on a failing run, the tempdir journal shows two `wake_lease_consumed` for
  one `wake_lease`, consecutive sequences, both actor `system-wake`."
- **CONFIRMS:** a captured failing run's journal shows exactly that — two consumes, one
  lease, consecutive sequences, actor `system-wake` on both.
- **KILLS:** a captured failing journal shows ONE `wake_lease_consumed` (or none). Then the
  double-consume is not the flake's mechanism, and F1-H1 falls whatever the red test later
  shows about window 3 existing in principle.
- **UNINFORMATIVE:** the journal is not captured before tempdir cleanup (C named this risk);
  or two consumes appear at NON-consecutive sequences (that points at F1-H2's cross-round
  straggler instead — record it, score this row NOT-YET, open an F1-H2 row).
- **Status:** OPEN.

### C-P4 — H3 dies on the printed value alone

- **Author:** C Agent. **Source:** same file, F1-H3.
- **Verbatim:** "Decide from the failing run's printed `value`: Corrupt-shaped refusal vs
  storage-shaped error. The assertion message already prints it — the captured text alone
  settles H3 vs H1."
- **CONFIRMS (of H3):** printed value is a storage/lock-shaped error, `ok` absent or false
  WITHOUT the fold's Corrupt.
- **KILLS (of H3):** printed value is a Corrupt-shaped refusal.
- **UNINFORMATIVE:** the value is not present in the captured text, or the run failed before
  the status call.
- **Note (M):** this is the cheapest row in the ledger — settled by text H is already
  required to capture, at zero extra runs. If it is still OPEN after H posts, someone
  dropped free evidence.
- **Status:** OPEN.

### B-P1 — the seam test's base behaviour, at both commits

- **Author:** B Agent. **Source:** `.factory/b-agent-issue55-memo.md` (b-agent-20b716
  worktree), section (c).
- **Verbatim:** "Prediction, falsifiable: it FAILS on both — the second consume lands and
  replay refuses with the fold's corrupt error. If it PASSES on either, my window-3 analysis
  is wrong and this memo's hypothesis dies — which is the desired property of the test."
- **Base-number scope (verbatim):** "the test run against the CURRENT `record_consumptions`
  (main `efd85d0` and branch tip `53d212d` — both, since the point is that the window
  predates the widening)."
- **CONFIRMS:** fails at BOTH `efd85d0` and `53d212d`.
- **KILLS:** passes at EITHER commit. B pre-committed to the analysis dying on that result;
  this ledger holds him to it.
- **UNINFORMATIVE:** run at only one of the two commits (the prediction is a conjunction over
  both and cannot be half-scored); or a failure whose text is not the corrupt-replay refusal.
- **Cross-check (M):** B-P1 and C-P2 are the same seam predicted independently — B derived it
  without reading the stash, C from the flake study. They are NOT independent evidence of
  each other's correctness, but they ARE separate derivations of the same window — **and per
  rule 10 that separateness is deflated: B and C read the same code inside the same shared
  premises, so their agreement licenses no confidence in the window beyond what either
  derivation licenses alone.** A result
  that kills one must kill the other; if it does not, one of the two rows is mis-specified,
  and that divergence is itself the finding.
- **Status:** OPEN.

### B-P2 — the post-fix number

- **Author:** B Agent. **Source:** same file, section (c), with-number.
- **Verbatim:** "Prediction: 0 failures in N." with **N (verbatim):** "50 consecutive runs of
  the deterministic test per condition".
- **CONFIRMS:** 0 failures in 50 post-fix runs of the deterministic test, per condition.
- **KILLS:** any failure in the 50.
- **UNINFORMATIVE:** fewer than 50 runs; or 50 runs of a test whose seam the landed fix
  removed. If the fix makes the seam inexpressible this row cannot be scored at all — and
  that is exactly the testability argument that chose B's pin fix over stash change A, so an
  inexpressible seam is a design regression, not a scoring inconvenience.
- **Status:** OPEN.

### B-P3 — the hypothesis that is NOT expected to be decidable

- **Author:** B Agent. **Source:** same file, section (b).
- **Verbatim:** "HYPOTHESIS (NOT MEASURED): the widening makes window 3 easier to hit ...
  Direction of the effect is argued, magnitude is unknown, and 'more likely' has not been
  observed even once."
- **B's own counterweight (verbatim):** "replay cost grows with stream length, and G1's
  replay is the slow step. Concurrent slow validations could also DE-synchronise the fast
  steps that follow."
- **CONFIRMS:** parent-vs-branch rates differ in the predicted direction, with N stated on
  both sides and the difference outside sampling noise.
- **KILLS:** rates equal or reversed, with N large enough to see the claimed direction.
- **UNINFORMATIVE — AND THIS IS THE EXPECTED OUTCOME (M):** every instrument currently
  planned measures a DETERMINISTIC seam test, which by construction hits window 3 every time
  and therefore cannot report a probability at all. The only instrument that could score this
  row is a statistical rate at the parent vs the branch tip, at an N nobody has budgeted. The
  expectation is recorded now so that, after the fix lands and the flake stops, nobody writes
  "the widening raised the rate" as though it had been shown.
- **Status:** OPEN, expected UNINFORMATIVE.

---

## B. Flake #1 — `the_storm_holds_under_eight_concurrent_agents`

### D-DT — the decision table (verbatim, 4 rows)

- **Author:** D Agent. **Source:** `.factory/d-agent-storm-study.md` (d-agent-25b466
  worktree), Patch 1 read-off table.
- **Instrument (verbatim invocation):**
  `cargo test -p graphhelm-cli --test api_http the_storm_holds_under_eight_concurrent_agents -- --nocapture`
  with (verbatim) "Repeat until at least two failing runs are captured (the flake is ~3 in
  6), keeping every run's stderr."

| # | Sample (verbatim) | D's verdict (verbatim) | Sealed reading |
|---|---|---|---|
| DT-1 | "`phase=read kind=TimedOut`, `read≈5000ms`, `connect` small" | "**H1 confirmed**: accepted, then starved of a response. The convoy is the mechanism." | CONFIRMS H1, KILLS H4 |
| DT-2 | "`phase=connect kind=TimedOut`, `connect≈21000ms`" | "**H4 revives, H1 refuted**: accept starvation, not response starvation." | CONFIRMS H4, KILLS H1 |
| DT-3 | "Successes show `read_ms` climbing toward 5000 across rounds" | "Corroborates the threshold-on-tail reading, and says the ceiling is latency, not a hang." | CORROBORATES only — confirms no single H |
| DT-4 | "Successes stay flat and one request dies alone" | "Refutes the convoy arithmetic; look for a genuine single-request stall instead." | KILLS the convoy arithmetic |

- **UNINFORMATIVE for the whole table (D's own stated limit, verbatim):** "It says WHICH
  PHASE and HOW LONG. It does not say WHY the server was slow — it cannot distinguish 'the
  single runtime thread was busy' (H1) from 'the sweep's extra opens made it busy' (H2) from
  'fsync stalled' (H3)." So ANY sample in this table leaves H2 and H3 unscored. A DT-1 hit
  must not be written up as "the storm is solved"; it selects between H1 and H4, nothing more.
- **Additional UNINFORMATIVE (M):** fewer than two captured failing runs — D's instrument
  spec requires at least two, so a single failure scores NOT-YET.
- **Status:** OPEN.

### D-P0 — Step 0, the free phase read

- **Author:** D Agent. **Source:** same file, "Revised measurement order", Step 0.
- **Verbatim:** "Read W1's verbatim failure text. If it names `post_request`
  :443/:460/:461/:464, the phase is known with zero new runs, and H4/H5 die there. Only if it
  names `get_status` does Patch 1 become the phase-deciding step."
- **Line map (verbatim, D's T2 acceptance):** "`post_request` unwraps at distinct lines —
  connect :443, write :460 and :461, read :464 — so a panic's own `file:line` names the phase
  for free".
- **The laundering caveat (verbatim):** "The status operation goes through `raw_request`,
  whose `?` propagates the error into `get_status` (:219-223), which panics with the uniform
  text `request to {url} failed: {error}`. On that path the phase is **laundered** — one
  panic site for all three phases, and on Windows the error kind is `TimedOut` for both
  connect and read."
- **CONFIRMS (phase known free):** the captured text names `api_http.rs:443` (connect),
  `:460`/`:461` (write), or `:464` (read).
- **KILLS the free read (not any hypothesis):** the text names `get_status` (:219-223). Then
  the phase is still open and Patch 1 earns its place.
- **UNINFORMATIVE:** the capture is a summary of the failure rather than the verbatim panic
  with `file:line`. This is the single most likely way the ledger's cheapest storm row gets
  wasted, and it is entirely under the capturing agent's control.
- **Status:** OPEN.

### D-T2 — the Windows error-code claim that already downgraded H4

- **Author:** D Agent (accepting B's T2). **Source:** same file, review-response table.
- **Verbatim:** "T2 loopback backlog gives 10061, not 10060 | **ACCEPT.** H4 downgraded
  above."
- **Underlying fact (verbatim):** "WSAETIMEDOUT (10060) is returned BOTH for a connect that
  times out AND for an `SO_RCVTIMEO` (read timeout) expiry."
- **CONFIRMS:** any captured connect-side failure on loopback carries 10061, not 10060.
- **KILLS:** a captured loopback connect failure carrying 10060 with a ~21s connect elapsed
  (that is DT-2, and it revives H4).
- **UNINFORMATIVE:** the raw OS error number is not preserved in the capture (only the Rust
  `ErrorKind`, which is `TimedOut` for both).
- **Status:** OPEN — and note this row currently stands as ACCEPTED BY ARGUMENT, not by
  measurement. It is in the ledger because an argued acceptance later cited as a measured
  fact is exactly the drift this file exists to stop.

### D-T3 — the coupling prediction about the cached-handle fix

- **Author:** D Agent. **Source:** same file, "T3, sharper".
- **Verbatim:** "One cached, shared handle would therefore serialize **every in-process
  operation, reads included**, on that mutex — deleting exactly the win 53d212d bought. And
  53d212d's own recorded measurement ('making the gate an `RwLock` moved nothing') does
  **not** transfer ... With a shared instance the gate becomes load-bearing for the first
  time, and that measurement no longer applies."
- **CONFIRMS:** a cached-handle build shows read latency regressing to (or past) the
  pre-53d212d shape, with base and with numbers stated.
- **KILLS:** a cached-handle build keeps 53d212d's read win intact under the same load.
- **UNINFORMATIVE:** any cached-handle measurement taken before C's flake-3 fix lands — the
  orchestrator's own sequencing ruling says a storm-rate change cannot be attributed across
  that boundary. Measuring there scores nothing, whatever the number.
- **Status:** OPEN.

### D-H6 — the hypothesis raised and dropped by grep (TRAP row, not a scoring row)

- **Author:** D Agent. **Source:** same file, H6.
- **Verbatim:** "`serve_with` pipes the server's stderr and **never drains it** ... Grepped:
  **zero** `eprintln!`/`eprint!`/`io::stderr` in `apps/cli/src` and zero in `core/`. So H6 is
  not a live hypothesis — but it IS a hard constraint on any measurement build: **never
  instrument the server to stderr under this harness.**"
- **CONFIRMS the constraint's necessity:** any instrumentation build that writes server
  stderr under `serve_with` and produces a hang indistinguishable from this flake.
- **KILLS H6-as-live:** already killed by the grep, at seal time.
- **UNINFORMATIVE:** n/a — the constraint is structural, not measured.
- **Ledger role (M):** carried as a trap. D's rung-A probe writes to a file with a mandatory
  pid field, explicitly NOT stderr, for this reason. If any future instrumentation patch
  reaches a build with server-side stderr writes, this row is the pre-registered explanation
  for the hang that follows — cheaper to find here than to re-derive.
- **Status:** N/A (constraint, pre-registered).

### D-P1 … D-P9 — D's frozen predictions (registered by D, sealed by M)

- **Author:** D Agent. **Source:** `.factory/d-agent-storm-study.md` section (h), "Frozen
  predictions — written BEFORE any number exists", plus D's message to this ledger.
  **Verification (M):** I read section (h) directly and compared it against D's message.
  They agree; no divergence to report. Nothing measured by D — his standing order is no
  cargo, and section (h) says so on its face ("while H's run is still in flight and nothing
  has been measured by me").
- **Distinction D asked this ledger to enforce, and which I do enforce:** a selection
  criterion ("adopt C1 if rung A shows X") is a DECISION RULE and cannot be wrong. Rules are
  logged below under D-RULES and are NOT scored. Only D-P1…D-P9 and D-FALSIFIER score.

**Step 0 group — what H's failure text will say.**

| ID | Prediction (verbatim) | CONFIRMS | KILLS | UNINFORMATIVE (sealed by M) |
|---|---|---|---|---|
| D-P1 | "The panic location is `api_http.rs:464` (read phase)." | text names `:464` | text names `:443` (connect) or `:460`/`:461` (write) | text names `:220-222` — **explicitly NOT a kill**, see below. Or no `file:line` at all |
| D-P2 | "The OS error is 10060, not 10061." | raw OS error 10060 in the capture | raw OS error 10061 — D's own words: "would redirect the whole study" | only the Rust `ErrorKind` survives the capture (`TimedOut` for both), so the number is unrecoverable |
| D-P3 | "The failure comes from a storm thread, not from the post-storm verify step." | failing frame is inside a storm worker | failing frame is the post-storm verify | capture does not identify which thread/step panicked |
| D-P4 | "The flake rate is near 50% (~3 in 6), matching the prior report rather than improving." | rate near 50% at N stated | rate materially away from 50% at N large enough to tell | N < 10. The prior "3 in 6" is N=6; W1's protocol requires N >= 10, so a repeat at N=6 scores nothing |

- **D-P1's modality is sealed, in both directions (M).** D wrote it himself: "P1 is a
  prediction about the modal outcome, not a claim that `:220-222` would surprise me", with
  the stated reason "the storm's op mix is 3 mutations to 1 status, so if failures fall
  proportionally there is roughly a 1-in-4 chance of landing at `:220-222`". Consequence
  sealed now, before the text exists: a `:220-222` landing does NOT kill D-P1 — but it also
  must NOT be reported as confirming it. It scores UNINFORMATIVE for D-P1 and simultaneously
  scores **D-P0 as KILLS-the-free-read** (the phase is laundered at `get_status`, so Patch 1
  becomes the phase-deciding step). One capture, two rows, opposite directions. Whoever
  scores must touch both.
- **Status:** all OPEN.

**Rung A group — what the histogram will show.** D's framing (verbatim): "These are the ones
that put my mechanism map at risk; if the counts are wrong, the map is wrong, and no amount of
prose rescues it."

| ID | Prediction (verbatim) | CONFIRMS | KILLS | UNINFORMATIVE (sealed by M) |
|---|---|---|---|---|
| D-P5 | "Opens per mutation request ≈ 4 (three on the request path, one from the sweep); opens per status request = 1." | both counts match | either count off — D: "This is read straight off the code, so a mismatch means I misread it" | probe cannot attribute an open to its originating request (no pid/caller field landing) |
| D-P6 | "Summed request-path open elapsed is **≥ 50%** of the storm's wall duration." | >= 50% | < 50% | **D-P8 falsified** — see the dependency below |
| D-P7 | "Blocking-pool rows are **20-30%** of all rows from the server pid." | inside 20-30% | outside 20-30% with sweep frequency unchanged | sweep frequency changed by C's fix → **WITHDRAWN**, per D's own conditional |
| D-P8 | "Request-path open intervals do not overlap each other." | no overlap observed | "A single overlap falsifies the single-runtime-thread serialization claim that the whole convoy argument rests on" | timestamps too coarse to resolve overlap at the observed open durations |
| D-P9 | "Open elapsed grows across the run as history grows: last-decile median ≥ 2× first-decile median." | last-decile median >= 2x first-decile | < 2x | too few opens to form deciles |

- **SEALED DEPENDENCY, D-P8 → D-P6 (M).** D's own parenthetical grounds D-P6's arithmetic in
  D-P8: comparing summed open elapsed against wall duration is "Legitimate to compare directly
  because those opens are serialized on one thread and cannot overlap." So if D-P8 is
  falsified, D-P6 is not merely wrong — it is **unscoreable**, because summing overlapping
  intervals against wall-clock double-counts. Sealed now so that a D-P8 kill plus a D-P6
  "confirm" cannot both be banked from the same histogram. If D-P8 dies, D-P6 scores
  UNINFORMATIVE regardless of its number.
- **SECOND SEALED DEPENDENCY ON D-P6 — the DENOMINATOR (M, from L's corpse-1 audit).** D-P6 is
  "summed request-path open elapsed >= 50% **of the storm's wall duration**". L's full audit of
  the retracted 8×600ms arithmetic found one live survivor, and it is this denominator. The
  retraction's own reasoning disqualified run wall clock as a measure of request time: the ~25s
  contains `cli_start` (a full CLI process driving the graph), the server spawn + health poll,
  and `verify_storm_left_a_coherent_stream`, which pages the whole tail and runs TWO full graph
  replay CLI processes (`api_http.rs:1646-1652`). That is exactly why "~500ms per request" was
  withdrawn. Rung A's reading instruction then divides by the quantity the retraction had just
  disqualified. Effect: the serial share reads LOW, diluted by work that is not request time,
  and **C1's own selector could be rejected on a denominator its author already retired.**
- **Sealed consequence:** any D row scored as a "share of wall clock" is UNSCOREABLE on the same
  grounds as D-P7, until the denominator is the STORM PHASE rather than the run. L's repair,
  cheap and using data already collected: bound the storm phase from the probe rows themselves —
  first to last `caller=request` row of the SERVER pid — and use that as the denominator; the
  CLI pids' row bursts mark `cli_start` and the verify replays. **This makes rung A's G2 (a real
  epoch clock instead of per-process `Instant`) LOAD-BEARING rather than cosmetic**, since
  per-process `Instant` origins cannot be compared across pids.
- **D-P6 AMENDED by D (amendment #3, pre-measurement), and the amendment is accepted.** New
  wording: summed `caller=request` open elapsed **>= 50% of the STORM PHASE**, not of the run's
  wall clock. Sealed with D's two conditions: **P6 is scoreable ONLY against the storm-phase
  denominator AND ONLY with G2 epoch-microsecond stamps** — per-process `Instant` origins cannot
  bound a phase across pids, so a run without G2 makes P6 **UNSCOREABLE, not failed**. G2 moves
  from cosmetic to required.
- **D's own addition, which nobody else had: the REPLACEMENT denominator is itself contaminated.**
  The server pid's rows do not stop at the storm. `verify_storm_left_a_coherent_stream` calls
  `all_events` over HTTP BEFORE the two CLI replays, and those are single `caller=request` opens
  **indistinguishable by tag** from the storm's own status reads. With the storm's history under
  the 1000-event page cap, `all_events` costs one full page plus one empty page = **2 extra
  rows**. D's identification rule: the phase's true end is the last row before that trailing
  pair — the <= 2 single-open reads occurring after the final `caller=sweep` row and before the
  CLI replay burst — with the residual bias (bounded by those two rows' span) recorded alongside
  the number. His reason, kept: replacing a contaminated denominator with a quietly-contaminated
  one would be the same defect one refactor later.
- **HOLE IN THAT IDENTIFICATION RULE (M) — sealed, because it is the fifth descendant D asked to
  have named in advance rather than discovered.** The rule anchors on "after the final
  `caller=sweep` row". By D's own P5 form 3, **only fresh 200 mutations emit a sweep row**: 409
  precondition, 409 conflict and status reads all emit ZERO. So if the storm's final requests are
  409s or status reads — and the storm's routine pause/resume flip-flop makes trailing 409s
  ordinary, not exotic — those genuine STORM rows land in exactly the same window as the verify
  reads, after the last sweep row, single-open, same tag. The anchor cannot separate them.
  Consequence sealed: the residual bias bound must cover trailing storm 409/status reads too, or
  the phase end needs a different anchor. **The cross-check already exists in D's own P5:** the
  client knows its per-outcome counts, so the expected number of `caller=request` rows in the
  phase is computable from client-side status codes, and the trailing window can be attributed
  by subtraction rather than by position. That is the same client-side cross-check D himself
  called P5's stronger half, reused here.
- **So D-P6 now carries THREE unscoreable conditions (P8, denominator span, G2 stamps) plus a
  bias bound that is itself under-specified until the anchor hole above is closed. Recorded
  plainly: this is the most heavily qualified row in the ledger.**
  - **~~independent~~ — struck under rule 10, found by applying rule 11 to this row (M).** D's
    amendment says backward application is most productive on the passage touched MOST; in this
    file that is D-P6, amended four times. Checked, and it had the deflated word: **the three
    conditions are NOT independent — they share one upstream, the probe.** A probe defect
    disables all three at once, and one such defect is already named in this file: L's G1
    ambiguous absence, where an open parked forever on the timeout-free `lock_exclusive` writes
    NO ROW. Under G1, overlap detection (P8), phase bounding (denominator) and cross-pid stamps
    (G2) all degrade simultaneously and silently. They are three DISTINCT conditions with a
    SHARED failure source — sufficient as separate gates, not independent as evidence.
  - **D's answer, and it is better than my finding (recorded because the distinction matters).**
    "Shared upstream" is true but defeatist. He mapped each probe failure mode to a detector that
    does NOT depend on the conditions that mode would corrupt:
    - **open PARKS and never returns (G1)** → the dangling open-begin with no open-end. Caught
      INSIDE the probe. This is why begin/end pairing is load-bearing beyond H1 blindness: it
      converts G1 from silent simultaneous degradation into a **visible marker**.
    - **rows DROPPED mid-run** → cross-apparatus counts: client codes on passing runs, committed
      store on failing runs. Caught entirely OUTSIDE the probe.
    - **sink UNUSABLE** → fail-loud panic on `create_new`, at startup, before any data exists.
    Accurate claim, sealed: the three conditions share one upstream, **and each of that upstream's
    failure modes has a detector that does not depend on the conditions it would corrupt.**
  - **A DISTINCTION THIS FILE SHOULD MAKE AND HAD NOT (D's, adopted):** a finding that **REMOVES
    a guarantee** is not the same as a finding that **LOCATES a guarantee that was never
    articulated.** My D-P6 correction was the second kind — the mitigation was already built and
    unnamed. The ledger has been recording both as "corrections", which overstates the damage of
    the second kind and understates the value of naming it. It is also
  still the row C1's selector leans on, which is exactly why the qualifications are worth more
  than the number would be without them.
- **Superseded text kept for history:** the ORIGINAL D-P6 in the table above reads "of the
  storm's wall duration". That wording is dead; it is not deleted, so the amendment chain stays
  visible.
- **Prior framing, retained:** D-P8 falsified (overlap makes
  the numerator double-count) OR a wall-clock denominator (the denominator measures the wrong
  span). Both must be clear before D-P6 scores at all.
- **What L checked and found CLEAN of corpse 1, recorded so the audit's scope is honest:** H1's
  primacy (it rests on the surviving structural bound — a request's wait is the sum of work
  admitted ahead of it — not on any number), H5 (robust to a 2-3× undercount), C2's rejection,
  and C5's refusal. Minor descendants L flags, none load-bearing: section (d)'s purpose line,
  "36" should read ">= 36", "~4 opens per mutation" is a floor (now superseded by P5 form 3),
  and Patch 1's decision-table row "corroborates the threshold-on-tail reading" restates the
  retracted conclusion as the thing being corroborated.
- **Corpse 3 (B's blocked-pipe stall): CLEAN, verified by L in code.** `ClientOptions::new().open()`
  is one non-blocking `CreateFile`; `write_all(&[1u8]).await` is genuinely async; every other
  sweep step is `spawn_blocking(...).await` (`serve/wake.rs:51-61`). Stronger for this test: the
  storm arms no lease, `due` is empty, the sweep returns at `wake.rs:126`, so `ring` is NEVER
  CALLED. No descendant of corpse 3 can reach this lane. Its one artifact (phase-2 ring-elapsed)
  is correctly demoted to a confirmation field.
- **D-P7's withdrawal is pre-authorized but NOT self-serve (M).** D wrote: "If C's fix changes
  sweep frequency, this row is void, not wrong — the ledger should score it as withdrawn
  rather than failed." Granted, with one condition sealed here: withdrawal requires the sweep
  frequency change to be shown from C's landed diff, not asserted after an out-of-range number
  appears. A number outside 20-30% with C's fix demonstrably NOT touching sweep frequency
  scores KILLS, and the ledger does not accept a retroactive withdrawal in that case.
### AMENDMENT HISTORY — rung A rows (all PRE-MEASUREMENT, all recorded, none deleted)

D asked for the history to be visible rather than silent, in his words so that he cannot
"launder a wrong row into a right one later". The table above is the ORIGINAL frozen text and
stays as written. What follows supersedes it where marked. Every change below was made before
any number existed and is traceable to a specific code-verified refutation.

**D-P7 — WITHDRAWN UNCONDITIONALLY. Not amended, not failed, not deleted.**

- Ground: the premise "blocking pool == sweep" is dead. The async driver opens the store from
  the SAME blocking pool — `driver.rs:220`/`:274`/`:308` calling `store_open()` at
  `:221`/`:275`/`:309`, stated in that type's own doc at `driver.rs:191-194`; the closure is
  built at `routes.rs:819` and passed at `:871`. Fixture mode still drives: `drive_is_viable_for`
  (`routes.rs:765-771`) is `runtime.is_some() || all nodes classify`, so the `None` branch
  (`routes.rs:858-861`) drives too. The storm issues up to 12 resumes, each entitled to a drive.
  Pool rows are driver+sweep MIXED.
- **Scope correction — D pushed back on my seal and he is right (M).** I had granted P7's
  withdrawal only on condition that a sweep-frequency change be shown from C's landed diff.
  That condition is correct for the ORIGINAL conditional D froze with the row ("void if flake-3
  changes sweep frequency"), and it stays attached there — without it, "void, not wrong" is an
  escape hatch that opens exactly when the number is bad. But it is the WRONG gate for THIS
  withdrawal, which rests on premise failure verified in code, pre-measurement, with no bad
  number to escape from. Gating this one would leave P7 scoreable against a metric all three of
  us now agree mis-attributes: a 20-30% reading would "confirm" a row measuring driver fan-out,
  and any other reading would "kill" a row that never measured the sweep. Neither is
  information. **My landed-diff condition therefore moves to the FUTURE replacement row.**
- **LATE AND UNCOMFORTABLE: THE P7 DEFECT WAS LATENT, NOT ACTIVE — and D raised it against his own
  withdrawal.** L's kill said blocking-pool rows carry DRIVER opens as well as sweep opens, so
  "pool rows = sweep share" mis-attributes. **True in principle; FALSE IN THIS TEST.** D's code
  proof: `drive_is_viable_for` = `runtime.is_some() || ALL nodes classify`; fixture-only serve makes
  the first false, and the storm's graph has a `deploy` node for which `classify::work_kind` returns
  `Err(Unsupported)`, making the second false. **The async driver never runs in this test**, so pool
  rows would have been sweep-only and **P7's original thread-id split would have produced the right
  number — by luck.**
  - **The withdrawal still stands, and D's reason does not depend on the outcome:** the metric's
    **JUSTIFICATION was invalid** (he asserted pool==sweep as a general property; it is not), he
    **could not have known it happened to hold**, and **it breaks the moment runtime wiring is
    supplied.** His principle, sealed: **a number that is right for a reason its author does not
    have is not a measurement.** Same shape as a guard that passes for the wrong reason, one level
    over — a metric that would be *right* for the wrong reason.
  - **Sealed in his honest form:** **L's kill removed a real DESIGN flaw; it did NOT remove an
    active measurement error in this configuration. LATENT, not ACTIVE.** The withdrawal must not
    read as "the number would have been wrong".
  - **STATUS: PENDING OBSERVATION, at D's own insistence — NOT sealed on his reading.** He upgraded
    the driver-never-runs claim from inference to a read file, and **I verified it independently**:
    `examples/graphs/manual-override-deploy.yaml` has `nodes:` at line 15 with exactly two —
    `type: agent` (17) and `type: deploy` (37) — then `edges:` at 50, under which the `type: data`
    at 54 sits. **The edge trap is real: a bare grep for `type:` returns three and one of them is
    an edge.** So `drive_is_viable_for` is false on both disjuncts.
    - **And he still refuses to let it close on his authority.** A is running one resume against the
      storm's own graph with his prediction frozen first: **ZERO `caller=driver` rows**. If any
      appear, his reading is wrong and he says so. **His reason: he has been wrong about this exact
      code path once already today** — which is the correct standard for a claim that is now
      load-bearing for control 1's zero half and for the P7 latent-vs-active framing.
    - **RESOLVED — CONFIRMED FOR THIS CONFIGURATION, with the scoping D demanded in advance.**
      A's capture **lost the HTTP codes** (printed, never persisted — their own recorded failure).
      The discriminator was recovered **from the THIRD apparatus**: the preserved events tree from
      the c1-resume run shows `execution_paused` → `execution_resumed` **committed in the journal**,
      with `node_outcome_recorded` after it. **A 409/precondition refusal APPENDS NOTHING**, so the
      presence of `execution_resumed` proves the mutation **landed** and the sync path drove.
      - **Scored as D pre-specified when he asked for PENDING even if it came back his way:
        CONFIRMED FOR THIS CONFIGURATION.** One scenario, **N=1**, configuration-specific — **a
        graph whose nodes ALL classify would take the drive path and produce driver rows.**
        Confirmed here, **not general.** Edit 4 stays inactive-but-correct tagging.
      - **HOW IT WAS ANSWERED VALIDATES THE ARCHITECTURE ON A FAILURE IT WAS NOT DESIGNED FOR.**
        D specified three apparatus for **DROP DETECTION**; the store answered a question the other
        two had **dropped** — **a lost measurement, not a dropped row.**
      - **SHARPENING (M): the property that rescued the datum was DURABILITY, not independence.**
        D credits "independence bought something the design did not anticipate". More precisely:
        the client's codes were **printed and not persisted**, while the store **writes to disk**.
        **Three apparatus sharing one upstream (his own earlier admission) would not have saved
        this; one apparatus that PERSISTS did.** The lesson a reader should take is *make sure at
        least one apparatus persists*, not *build three apparatus* — and it is the third recurrence
        today of **capture the evidence while it exists**.
      - **D TESTED MY CORRECTION COUNTERFACTUALLY AND IT PRODUCED A BETTER PROPERTY THAN EITHER OF
        US NAMED.** His check: **a durable CLIENT-SIDE log would have answered equally well** (same
        apparatus, just persisted), and **an IN-MEMORY store would have answered nothing, however
        independent.** So durability was **necessary** and independence **incidental**.
      - **THE SHARPER PROPERTY — INTRINSIC vs INSTRUMENTED (D's, adopted):** **the store's journal
        is not a measurement anyone decided to take. It exists because the system CANNOT FUNCTION
        WITHOUT IT.** A durable client log still requires someone to have *chosen* to log the
        thing; **the journal is written whether or not anyone is watching.** **And that is also why
        the mtimes worked** — nobody instrumented them; the filesystem records modification times
        as a byproduct of writing, **so the evidence existed for a question nobody had thought to
        ask yet.**
        | Class | Examples today | Fate |
        |---|---|---|
        | **INTRINSIC + DURABLE** | journal, mtimes | recorded because the system must; **survives intent** |
        | INSTRUMENTED + DURABLE | probe files | survives, but only what someone chose to capture |
        | INSTRUMENTED + EPHEMERAL | printed HTTP codes | **lost, and lost silently** |
        **Today's two recoveries were both from the top row; today's one loss was from the bottom.**
        **The most reliable evidence is what the system MUST record to work — the only kind that
        does not depend on someone having anticipated the question.**
      - **MY REFINEMENT, from testing the hierarchy against today's one BLOCKED case (M):
        INTRINSIC IS NOT ENOUGH — THE CONTAINER MUST ALSO PERSIST.** D-P10-ALT is blocked precisely
        because **the journal is intrinsic but sits in a `tempdir` that deletes on unwind**: the
        panic destroys its own evidence. **So the top row requires BOTH properties, and an intrinsic
        record in an ephemeral container falls to the bottom row despite being intrinsic.** That is
        why the repair there is preserving the directory, not adding an instrument.
      - **D SHARPENED THAT AGAIN, AND THE CASE IS WORSE THAN "BOTTOM ROW".** Two independent axes,
        four cells — and **the INTRINSIC + EPHEMERAL cell is worse than the bottom row, for a reason
        of CORRELATION rather than volume.** Printed status codes are lost at a **CONSTANT RATE** —
        every run, pass or fail, equally. **The tempdir journal is lost PRECISELY WHEN THE RUN
        FAILS**, because **the panic that makes the evidence interesting is the same event that
        unwinds the drop that deletes it. THE CORRELATION BETWEEN "INTERESTING" AND "GONE" IS
        PERFECT.**
        - **Correct label: SELF-DESTROYING ON THE EVENT UNDER STUDY**, not merely "ephemeral".
        - **And it dictates the repair:** *"ephemeral"* invites **"log more"**, which is the wrong
          instinct. **Any evidence whose container is torn down BY the failure path is invisible
          exactly when it matters, and NO amount of instrumenting INSIDE that container fixes it.
          The fix is always to move the container OUT of the failure path** — `catch_unwind` +
          preserve, never more logging.
      - **One-line form, D's, for the memory note: THE MOST RELIABLE EVIDENCE IS WHAT THE SYSTEM
        MUST RECORD TO FUNCTION, IN A CONTAINER THE FAILURE CANNOT TAKE WITH IT.**
    - **AND HE PRE-REFUSES HIS OWN CONFIRMATION, which is the mirror of everything else in this
      file.** He asks that the row stay PENDING **even if A's observation comes back exactly as
      predicted**, because *"the prediction being confirmed makes the reading LIKELY correct, not
      verified; the observation and the code reading could both be right for different reasons, and
      one resume is N=1."*
      - **Binding on me when the observation lands: I will not mark it verified.** Agents routinely
        pre-commit to accepting a kill; **pre-committing to NOT accepting a confirmation as
        verification is the rarer and harder direction**, and it is the one that stops a matching
        result from being spent as proof of the mechanism behind it.
  - **THE FIRST RECORDED CASE WHERE A RULE'S CORRECT APPLICATION COST SOMETHING — SHARPENED BY D,
    because my first phrasing overstated it (M).** I had written that rule 7 "removed a metric that
    would have worked". **True in one sense and misleading in another**, and a skimmer would read it
    as *the rule destroyed a good measurement*, which is not what happened.
    - **The thread-id split would have produced the CORRECT VALUE here. It would NOT have been a
      usable MEASUREMENT**, because nobody could have known it was correct: the justification
      (pool==sweep) was false as a general property, and **the only thing that could have licensed
      the number is the driver-never-runs proof, which did not exist until the withdrawal prompted
      it.** So **nothing usable was lost — what was lost is the cheaper implementation.**
    - **HONEST SHAPE: AN INSURANCE PREMIUM, NOT A LOST ASSET.** Extra complexity (a three-way tag
      instead of a thread-id read) bought insurance against a failure mode that is inactive in this
      configuration. **Premiums are real even when the policy never claims — but the cost is in
      EFFORT, not in correctness and not in evidence.**
    - **The counterweight survives the sharpening, and D checked the chain:** L's kill → three-way
      tag → control 1 with a driver sabotage → A observes it unobservable → `drive_is_viable_for =
      false` derived. **The proof exists BECAUSE of the withdrawal.** Premium paid, structural fact
      returned.
  - **Claim-utility applied by D to his own item: this changes no decision** (the tag is correct to
    have regardless), so he recorded it and spent no further rounds. Correct.

- **A FOUND TWO DEFECTS IN D'S CONTROLS BY RUNNING THEM — and neither was findable by reasoning.**
  - **Control 2 could not isolate what it tested.** D wrote "≥1 `caller=sweep` row" with "remove
    Edit 2" as its sabotage; A observed the sabotage NOT felling it, because with Edit 2 removed
    phase 3 still emits a sweep row and the existence check passes. **That is the layered-defence
    failure and the finest-grain rule at once: an existence check sitting one level above the
    per-site claim it needed to make.**
    - **Fixed as an EXACT COUNT DERIVED FROM CODE, not from A's observation** — an important detail:
      deriving the repair from the source rather than from the observation that exposed it avoids
      fitting the fix to the symptom. Arm POST → phase 1 opens once, returns early; signal POST →
      phase 1 opens, rings, phase 3 opens. **Exactly 3 `caller=sweep` rows.** Remove Edit 2 → 1
      (fails); remove Edit 3 → 2 (fails). **Two independent sabotages, each felling it alone.**
  - **Control 1's sabotage was structurally unobservable**, and D's code proof extends past what A
    tested: A checked GET/arm/signal, but the storm also does RESUME, its only drive-capable op —
    and it does not drive either, for the reason above. **Control 1 becomes "exactly 1
    `caller=request` pair for a GET, and ZERO `caller=driver` rows anywhere", with the zero half
    now code-derived rather than observed.** Edit 4 kept as correctness tagging, **marked INACTIVE
    in this configuration.**
  - **D's own summary, and it is the day's thesis restated:** both defects were his, and **neither
    was findable from his side — they surfaced only when someone RAN the sabotages instead of
    REASONING about them.**
- **P5 AMENDMENT (third):** his note that "resume rounds add a variable DRIVER fan-out" is **wrong
  for this test — there is no driver.** Resume's SYNC path may still cost more than 3 opens and rung
  A will measure it, but those are request-path opens **correctly tagged `request`**. Only the
  attribution to the driver was wrong; **the request/sweep split is unaffected.**

- **No replacement is frozen, deliberately (D's call, endorsed).** The honest replacement —
  "caller=sweep rows are 20-30%" — depends on a `caller=` tag that DOES NOT EXIST YET.
  Predicting the output of one's own unwritten code is guessing, not forecasting. The
  replacement can be frozen only once the tag is written, and it inherits BOTH the original
  flake-3 conditional and my landed-diff condition on that conditional.
- **Status:** WITHDRAWN (premise failure, pre-measurement, code-verified).

**D-P5 — SUPERSEDED TWICE, now an accounting identity. Both prior forms recorded.**

- *Form 1 (original, in the table):* "opens per mutation request ≈ 4". D reclassified it
  himself: it was a FLOOR, not an estimate — resume rounds add driver opens on top.
- *Form 2 (first amendment):* "caller=request opens are exactly 3 per mutation on the
  non-driving path and 1 per status." Killed by L's round-2 review: open count is
  **outcome-dependent, not mutation-dependent** (`serve/mod.rs:744-812`), and two different
  outcomes both cost 3 caller=request opens with nothing in probe output separating them
  per-request. Same failure mode that withdrew P7, wearing a number.
- *Form 3 (current, sealed).* Counting `caller=request` opens, per OUTCOME:
  - fresh 200 mutation: **3** (classify `:746` + command + current_head `:797`), plus EXACTLY
    **1** `caller=sweep` row (`:806`, Ok arm only)
  - 409 precondition GHCLI005 (the storm's routine pause/resume flip-flop): **2** (classify +
    command; `respond_failure` opens nothing), **0** sweep
  - 409 sequence conflict GHE001 (rare race): **3** (classify + command + conflict_with_current_head
    `:1025`), **0** sweep
  - 200 status: **1** (`status::execute`, `status.rs:24`; `status.rs:64` `write_snapshot` is CLI
    `--html` only, never reached from serve)
  - recognized retry `KeyState::Complete`: **2**
- **Scored as an identity, not a per-request count:**
  `total caller=request opens = 3·N(200 mut) + 2·N(409 precondition) + 3·N(409 conflict) + 1·N(status)`
  and `caller=sweep rows = N(200 mutations)`, exactly.
- **~~Which half is the stronger row (D's own reading, kept): the second equation, because it
  predicts the sweep count from client-side status codes alone.~~ SUPERSEDED — do not use this
  seal.** D flagged the ordering himself: his amendment #5 arrived after I sealed this and
  **downgrades the second equation from an identity to a ONE-DIRECTIONAL test**, because L found
  three leaks and D found a fourth confound. Correct form, sealed in Scoring round 1 below:
  identity on PASSING runs only; on failing runs, discrepancy > 0 CONFIRMS commits-past-timeout,
  discrepancy == 0 is UNINFORMATIVE. Struck rather than rewritten, so the superseded seal stays
  visible.
- **SCORABILITY CONDITION, sealed:** P5 requires BOTH `caller=` tagging AND per-outcome
  client-side counts. Absent either, P5 records **UNSCOREABLE — never "did not fire"**. Same
  cell and same reason as D-FALSIFIER. D's own note, kept because it is the right instinct: if
  that condition makes P5 look fragile, that is accurate, and it should look fragile in the
  ledger rather than robust in a report.
- **Two corrections D made to L's table, both code-verified by D (recorded so the disagreement
  is traceable, not smoothed over):** (1) the storm's routine 409s are GHCLI005 precondition
  refusals costing 2, not the 3 a GHE001 conflict costs — L conflated them; (2) the
  recognized-retry row NEVER FIRES in this storm, because `retry_once_on_409` retries under a
  FRESH key (`{key}-retry`, `api_http.rs:1532`) and every base key is unique per thread+round,
  so no request repeats a key with the same body.
- **Status: SCORED — CONSISTENT-NOT-TESTED.** Measured opens/request ≈2.40–2.56 fits form 3's
  per-outcome table under the storm's mix, **but D never froze a weighted-average prediction, so he
  refused to claim a confirmation.** Not confirmed, not killed.
  *(This line read "OPEN in form 3" after the row had been scored — caught by the stale-status
  audit below.)*

**D-P5e — NEW ROW, spun out of that second correction.**

- **Prediction:** ZERO `KeyState::Complete` occurrences in the storm run.
- **CONFIRMS:** no recognized-retry rows at all. **KILLS:** one or more.
- **UNINFORMATIVE (M):** the probe does not distinguish `KeyState::Complete` from a fresh key —
  i.e. the same tagging dependency as P5. Untagged run scores UNSCOREABLE.
- **Ledger note (M):** this is a row created BY an amendment rather than damaged by one. Worth
  marking, since the other two amendments this round both subtracted.
- **Status:** OPEN.

**RUNG A IS NOW READY — the tagging dependency that gated six rows is being lifted.** D reports
rung A written out rather than argued for, and names why the earlier argument was incomplete IN
HIS OWN FAVOUR: he had argued rung A should win the contested slot over Patch 1 because it
separates H1/H2/H3 while Patch 1 settles only a 20% corner — **but rung A existed as PROSE while
Patch 1 existed as a DIFF**, which is not a fair comparison. As built it carries: a **three-way**
caller tag (request/sweep/**DRIVER** — a two-way tag would still mis-attribute, since driver
opens would silently count as request), begin/end rows, epoch micros, a fail-loud sink that
refuses an existing file, error kind rather than bool, and two known-count controls guarding the
instrument itself.

- **Every one of those six features is a repair from this ledger's own rules (M).** Three-way tag
  = the flattening repair that killed D-P7 (widen the value). Begin/end rows = the
  ambiguous-absence repair for L's G1. Epoch micros = the denominator repair that made G2
  load-bearing. Error kind not bool = `ok=bool` flattening. Fail-loud sink and known-count
  controls = instrument self-guards, the class that half our flattening instances lived in.
  **The instrument was rebuilt out of the rules the scoring produced**, which is the clearest
  return this file has shown.
- **On it running: P5, P5e, P6, P8, P9 and D-FALSIFIER become scoreable.** All six remain
  UNSCOREABLE until it does — not "did not fire".

**Unchanged and still live per D:** P1, P2, P3, P4 (Step 0 — unaffected; H's run is clean and
needs no tag), P6 (subject to its two dependencies below), P8, P9, and D-FALSIFIER. D notes P8
gains weight: it is now the cleanest surviving test of the serialization claim, since it reads
`caller=request` rows only and never touches the pool.

**Consequence D accepted and asked to be held to:** D-FALSIFIER needs median open cost AND
opens-per-request, and opens-per-request is P5's instrument, which needs the `caller=` tag. So
**if rung A ever runs UNTAGGED, the falsifier CANNOT FIRE** — a second, independent reason rung
A must not run before `caller=` exists. The untagged version would silently evaporate his
highest-value row.

- **D-P7's PREMISE IS CONTESTED, and that is a bigger problem than its range (M, from L's
  adversarial review, verified by me in source).** D-P7 measures the sweep's effect size as the
  share of store opens landing on blocking-pool thread ids. That reads "blocking pool == sweep",
  which is **false at 53d212d**: the driver opens the store only inside `spawn_blocking` —
  `core/runtime/src/driver.rs:220-221`, `:274-275`, `:308-309`, fed the `store_open` closure
  built at `apps/cli/src/commands/serve/routes.rs:819` and passed at `:872`. I read all four
  sites; they are as L describes. The storm's resumes each trigger a drive, so blocking-pool
  rows carry the **entire driver fan-out** on top of any sweep. Sealed consequence: a D-P7
  number inside 20-30% does NOT confirm a sweep share, because the instrument cannot separate
  sweep opens from driver opens by thread id alone. **D-P7 scores UNINFORMATIVE until the probe
  attributes opens by CALLER, not by thread pool.** This supersedes nothing D predicted — his
  range may still be right — it says the planned instrument cannot decide it.
- **D-P9 cross-links A-M10 (M).** This is A's O(history) cost predicted in D's instrument. A
  D-P9 confirmation is structural support for A's "~3x shape is real" reading. It is **not**
  a reproduction of the owner's "48%", "15ms -> 36ms" numbers — different instrument,
  different quantity. A-M10 stays OPEN whatever D-P9 does.
- **Status:** all OPEN.

### D-FALSIFIER — the pre-refused temptation

- **Author:** D Agent. **Source:** same section (h), "The falsifier I commit to in advance".
- **Verbatim:** "if median open cost × opens-per-request already exceeds the per-request budget
  the 5s timeout allows under 8-way queueing, then serialization is not the binding constraint
  and **C1 must not be adopted** even though the storm would likely pass with it. That is the
  outcome that would send this lane to C3/M10 territory instead, and I would rather name it now
  than discover the temptation after a green test."
- **TRIGGERS (the refusal binds):** measured median open cost × measured opens-per-request >
  the per-request budget the 5s timeout allows under 8-way queueing. Then C1 is refused, and a
  green storm test under C1 is explicitly NOT a reason to adopt it.
- **DOES NOT TRIGGER:** the product is under budget; serialization stays a live binding
  constraint and C1 remains selectable by its own rule.
- **UNINFORMATIVE (sealed by M):** either input missing — D-P5's opens-per-request or the open
  cost median. The falsifier is arithmetic over two measured quantities; without both it cannot
  fire, and "it did not fire" must not be recorded when in truth it could not be evaluated.
- **AMENDED PRE-DATA — THE ORIGINAL CONDITION NAMED THE WRONG REGION. D caught it while writing the
  analysis procedure, before any numbers landed.**
  - **The arithmetic, which I verified independently rather than repeating the failure being
    named.** Let S = opens-per-request × median open elapsed (a lower bound on per-request service
    time). **Serialized (today):** the 8th concurrent request completes at ≈8S, timing out when
    8S > 5s, i.e. **S > 625ms**. **Parallelized (C1):** each completes at ≈S, timing out only when
    **S > 5s**. So:
    | S | serialized | parallel | meaning |
    |---|---|---|---|
    | < 625ms | no timeout | no timeout | neither crosses; **C1 fixes nothing observable** |
    | 625ms – 5s | TIMES OUT | no timeout | **serialization is exactly what crosses; C1 IS JUSTIFIED** |
    | ≥ 5s | TIMES OUT | TIMES OUT | single-request cost exceeds budget; **C1 CANNOT HELP** |
  - **D's frozen condition was S > 625ms — WHICH IS THE REGION WHERE C1 HELPS.** He had written a
    falsifier that **would have forbidden his own fix precisely when it was warranted**, and left
    the region where the fix is useless (S ≥ 5s) unnamed.
  - **CORRECTED, commitment unchanged in spirit: the falsifier fires at S ≥ 5s** — S large enough
    that 8-way parallelism still leaves requests at the timeout. If the measurement lands there,
    **C1 is refused even if the storm would pass with it.**
- **HOW IT SCORES — D ASKED FOR THE HARSHER READING AND HE IS RIGHT TO.** This is **not** "D caught
  his own error before data" as a credit. **It is a pre-registered row that was WRONG FOR HOURS
  while being cited by three agents** — I sealed it, L endorsed it, the orchestrator boarded it as
  the reason rung A wins its slot — **and none of us checked the arithmetic.**
  - **It survived because it SOUNDED like a falsifier:** it named a threshold, committed against
    its author's own preference, and used the right vocabulary. **Nobody multiplied it out.**
  - **The class (D's): rule 7 at the altitude of a FORMULA rather than a value.** *"Exceeds the
    budget"* has two upstream readings — **exceeds-per-request** and **exceeds-when-queued** — and
    the row never said which. **A falsifier stated in prose rather than in arithmetic is a
    falsifier nobody can check.**
  - **MY OWN FAILURE, STATED PRECISELY (M).** I did audit this row — I sealed its UNINFORMATIVE
    cell (it needs median open cost AND opens-per-request, so an untagged run cannot fire it).
    **That is an audit of its INPUTS. I never audited its LOGIC.** I checked whether it *could*
    fire and never whether firing would *mean what it claimed*.
  - **AND I PROMOTED IT REPEATEDLY.** I called it "the highest-value row D sent", "the best row on
    the board", "the row that risks his preferred fix" — **each promotion adding authority and
    zero checking.** Three agents cited it and the citations looked like corroboration; **they were
    propagation.** That is rule 10 in a new form: **CITATION IS NOT CORROBORATION, and repeated
    citation of an unchecked claim manufactures confidence out of nothing but repetition.**
- **AMENDED AGAIN — v3, and v2 was ALSO semantically wrong. L found it; I verified the code and the
  arithmetic myself.**
  - **v2 assumed the 8 requests proceed in PARALLEL under C1. They do not, for the open portion.**
    **Every store open takes a BLOCKING EXCLUSIVE lock with no timeout.** **VERIFIED BY ME:**
    `initialize_root_locked` at `core/events/src/local.rs:2134` calls `lock.lock_exclusive()` at
    `:2146`, reached from `open_inner` (`:318`) via `lock_root_exclusive` and
    `initialize_root_locked` at `:329-330`. **53d212d gave READ OPERATIONS a shared lock; THE OPEN
    ITSELF STAYED EXCLUSIVE.**
  - **The corrected arithmetic, worked by me rather than accepted.** With O = per-request open cost
    and W = everything else: **today the 8th request lands at ≈8(O+W); under C1 at ≈8O + W.** So
    **C1's entire benefit is ≈7W** *(v3's arithmetic — **SUPERSEDED by v4 below**, which splits W
    and gives ≈7·W_free; kept here as v3's own statement, not as current truth)* — **nothing on the
    open-dominated fraction.** Checked numerically:
    at O=0.6s/W=0.05s the benefit is 0.35s; at O=0.05s/W=0.6s it is 4.2s. **The benefit is
    proportional to W alone.**
  - **v3 fires when 8O + W ≥ 5s.**
  - **CELL CORRECTION THIS FORCES:** v2's UNINFORMATIVE cell named **two** inputs (median open cost,
    opens-per-request). **v3 needs a THIRD — W.**
  - **INPUT-DEFINITION CORRECTION — FOURTH ERROR ON THIS ROW, SAME DIRECTION, AND THIS ONE IS IN
    THE INPUT RATHER THAN THE FORMULA.** I sealed W as "a single uncontended request's client
    latency minus its own O, **which the exactly-one control already produces**". **That control is
    a GET.** A status read is ONE open and carries **none of the mutation's append path**, so its W
    is not the W the falsifier needs — **three of the storm's four operations are MUTATIONS.**
    Using the GET's W **understates the serializing fraction and MAKES C1 LOOK BETTER THAN IT IS**:
    the fourth consecutive error easing its author's preferred fix.
  - **CORRECTED SOURCE, sealed:** take W from **the C2 control's UNCONTENDED signal POST** —
    `W_mutation = (connect_us + write_us + read_us for that POST) − (sum of elapsed_us of its
    caller=request opens)`. Pairing is unambiguous because C2 issues one request at a time and both
    sides carry epoch-microsecond stamps, so the server rows inside the client's request window
    belong to that request.
  - **CELL AS IT NOW STANDS — three inputs:** median open cost, opens-per-request, and
    **W_mutation from the C2 control's signal POST.** **An untagged run, a client-probe-less run,
    OR A RUN WITHOUT THE C2 CONTROL cannot fire it.** Still no extra run required — **but it is a
    DIFFERENT control than the cell previously named, and had A run only the exactly-one control
    the row would have been unfirable.**
  - **How it was found, and it vindicates a rule rather than an intuition:** D checked whether v3
    is **actually computable from what A will collect**, instead of assuming it — prompted by the
    worked numbers (O=0.6/W=0.05 → 0.35s benefit; O=0.05/W=0.6 → 4.2s). **Plugging real-ish values
    in is what made the input definitions matter, and it is what exposed the wrong control.** L's
    rule earning itself again: **a falsifier is not registered until someone has plugged numbers
    into it once.**
- **THE CONSEQUENCE THAT OUTRANKS THE ROW.** If opens dominate — and the mechanism map says ≥4
  opens per mutation, each an exclusive lock plus a full journal load plus an fsync — **then W is
  small and C1 BUYS VERY LITTLE.** Re-ranking follows: **IF OPENS SERIALIZE REGARDLESS OF
  THREADING, REDUCING THE NUMBER OF OPENS BEATS PARALLELISING THE HANDLERS.** That is C3, which D
  routed to M10 as out of scope. **THE LANE'S CORRECT OUTPUT MAY BE A MEASUREMENT AND A REFERRAL,
  NOT A PATCH.**
- **IT IS THE WORKED EXAMPLE OF BOTH THINGS JUST SEALED, AND D SAYS SO HIMSELF.** This row's
  **PLUMBING was fine at every version** — inputs available, computable, cell honest — and its
  **SEMANTICS were wrong TWICE.** Meanwhile it collected citations from three agents *precisely
  because it was believed valuable*. **Plumbing-vs-semantics and citation-is-not-corroboration are
  not two lessons here; they are one row failing both ways at once.**
- **TRAJECTORY ON THE RECORD, D's own framing and the part I would not let him drop:** three
  versions in one day, and **EVERY ERROR EASED HIS OWN PREFERRED FIX.** v1 named the region where
  C1 helps (would have forbidden it exactly when warranted — no: would have forbidden it there,
  which cuts against him; but v1's *effect* was to misplace the veto). v2 fixed the region but
  assumed a parallelism that does not exist, **inflating C1's apparent benefit.** v3 accounts for
  the lock. **He states he is not confident v3 is final and asks the ledger to carry that rather
  than a clean seal.** Carried.
- **PROPAGATION NEAR-MISS, self-reported:** he sent v3's correction to L and the orchestrator and
  **not to me — the holder of the sealed row** — and caught it by tracking who had received what.
  **That is exactly the manual check his own propagation rule admits it needs**, an hour after he
  wrote the rule.
- **v4 DIRECTION — v3 repeats the same optimism one level down. NOT SEALED AS v4; v3 STAYS
  OPERATIVE.** L multiplied v3 out; D verified; **I verified the code**: `with_lock`
  (`core/events/src/local.rs:540`) takes the file lock for **every operation, not just the open** —
  the exclusivity match at `:566-568` does `file.lock_exclusive()` for appends and `lock_shared`
  for reads. **So W is not free-running:**
  **W = W_excl (appends, serialize against everything) + W_shared (parallel among readers, blocked
  by any exclusive holder) + W_free.** Under C1 the 8th request lands at ≈8(O + W_excl) +
  remainder, **not** 8O + W. **C1's benefit is ≈7·W_free, NOT ≈7·W** — and the storm is
  **mutation-heavy by construction** (three of four ops are mutations), so **W_excl is large
  precisely where the flake lives.**
- **v4 IS NOT COMPUTABLE TODAY**, and D says so rather than sealing an uncomputable row: W_free
  needs time-under-`with_lock` split by exclusivity, which lives inside `local.rs` — C's pen, rung
  B territory. **Rung B spec addendum recorded: split `with_lock` time by Exclusivity so v4 becomes
  computable when rung B runs.**
- **THE OPERATIVE SEAL, AND IT IS AN ASYMMETRY THAT BELONGS IN THE CELL RATHER THAN IN PROSE:**
  **v3 remains the operative row, explicitly as an OPTIMISTIC BOUND THAT UNDER-FIRES.**
  - **A v3 FIRE is decisive: C1 is refused.**
  - **A v3 NON-FIRE IS UNINFORMATIVE AND IS NOT A CLEARANCE FOR C1.** v3 understates serialization,
    so it fires less readily than the truth. **Anyone reading "the falsifier did not fire" as
    permission to adopt C1 is reading an optimistic bound as a verdict.**
- **THE PATTERN, WHICH D ASKS TO BE BOARDED ABOVE THE FORMULA AND WHICH I SEAL AS CHECKABLE:**
  **v1 erred toward FORBIDDING his fix. v2, v3, and v4's correction ALL err toward PERMITTING it.**
  Each version fires more readily than the last, **meaning every model he built understated
  serialization and overstated his own fix's benefit — while his whole thesis is that
  serialization is the problem. THREE OF FOUR CORRECTIONS BIASED TOWARD THE AUTHOR'S PREFERRED
  OUTCOME.**
  - **Sealed as a DIRECTION, not a tally, precisely because it is checkable forward: anyone
    producing v5 should EXPECT THE BIAS TO RUN THE SAME WAY AGAIN.** If v5 errs toward forbidding
    C1, **the pattern is broken and that is also worth knowing.** A self-reported bias direction
    tells the next reader which way to lean when checking; a count of errors does not.
- **THIRD APPEARANCE OF PLUMBING-VS-SEMANTICS, ON ONE ROW, WITH BOTH MODES CLEANLY SEPARATED —
  the best worked example either of us has for the distinction.** **v2 and v3: perfect plumbing,
  semantics wrong.** **v4: correct semantics, inputs unavailable — a plumbing gap, honestly
  stated rather than papered over.** Same row, both failure modes, at different versions.
- **Ledger note (M):** this was the highest-value row D sent; it has now been wrong twice, is
  operative only as an under-firing bound, and its author has named the direction of his own bias
  in advance. Every other row risks his map;
  this one risks his preferred fix, and it is pre-committed against the specific evidence
  (a green test) that would otherwise make the refusal feel unnecessary. Held.
- **Status: SCORED — DOES NOT FIRE, AND THAT IS NOT A CLEARANCE.** 8×O = 0.56–0.64 s against a 5 s
  trigger; W would have to exceed 4.4 s to reach it, so **the non-fire is robust to any plausible
  W** and is evaluable despite W_mutation never being reported. **v3 under-fires by construction
  (it ignores the per-operation exclusive lock v4 identified), so a non-fire licenses nothing.**
  C1 stays unadopted — on the referral, not on this row.
  *(This line read "OPEN" after the row had been scored — caught by the stale-status audit below.)*

### D-RULES — decision rules (LOGGED, NOT SCORED)

Recorded verbatim from D's message so the selection basis is fixed before the numbers, and so
no rule is later mistaken for a prediction that "came true".

- **C1** spawn_blocking store work out of handlers ← selected by request-path opens serialized
  while blocking pool idles.
- **C2** multi-thread runtime ← only if Step 0 lands `:443`; already rejected on merit.
- **C3** remove unneeded opens ← if opens/request >= 3 AND open cost dominates; routes to M10,
  not absorbed here.
- **C4** skip sweep when nothing armed ← if blocking-pool share large; C's call, proposed never
  taken.
- **C5** raise client 5s timeout ← named only to be REFUSED; measurement only, never the fix.

- **One scoreable thing inside the rules (M).** C5 carries a pre-commitment, not a criterion:
  "named only to be REFUSED ... never the fix." If a raised client timeout ever lands as the
  storm fix, that is a **scored breach of a pre-commitment**, and it scores here even though
  the rules themselves do not score. Same shape as A3's exchange-counting refusal in section C.

### D-DEAD — claims already dead, logged so they cannot return

D's list, verbatim: "my '8 × 600ms ≈ 4.8s' arithmetic (retracted), my 'sweep async half
contends for the runtime thread' (retracted), B's 'blocked pipe client stalls all requests'
(refuted). All three died by code during review."

- **Ledger role (M):** not scoreable, carried as a re-entry guard. The 600ms arithmetic in
  particular is already flagged in B's review of D's study as "partly invented (no round
  barrier exists; 600ms chosen to fit; do not anchor fix on it)". If a number near 4.8s shows
  up in a result and gets read as corroborating the convoy arithmetic, this row is the
  pre-registered refusal.
- **Each entry here is also a scoring-rule-6 trigger (M).** These are the three retractions
  whose REPLACEMENTS must be reviewed as new work. One is already known to have imported a bad
  premise (the runtime-thread retraction → blocking-pool thread-id share → "blocking pool ==
  sweep", see D-P7). The other two — the retracted 600ms arithmetic and B's refuted
  blocked-pipe claim — have NOT been audited for descendants by anyone, including me. That
  audit is unowned and is named here rather than assumed done.
- **Status:** N/A (dead, logged).

---

## C. Flake #2 — `a_sleeper_wakes_on_a_peer_append_with_zero_requests_in_the_window`

The whole mechanism selection is keyed on ONE input: the captured assertion TEXT. A states it
(verbatim): "W1's captured assertion TEXT picks the branch below; nothing is written before
that text exists." C states the same (verbatim): "which assertion actually fires is UNMEASURED
and is the single most valuable thing W1 can capture (the assertion text names the mechanism)".

### A-BT — the branch table

Author: A Agent. Source: `.factory/a-agent-flake2-plan.md` (a-agent-c35d10 worktree);
hypothesis IDs credited to C's study.

| Branch | Selecting text (verbatim) | Mechanism (verbatim) | KILLS the branch |
|---|---|---|---|
| A1 (C's F2-H1, T4) | "the lease burned on the ring" or lastConsumed null | "receipt GET outruns sweep phase-3 append" | text names any other assertion; or a captured gap showing the consume append PRECEDED the GET |
| A2 (C's F2-H2) | "the sidecar must exit 0 on the ring" (exit!=0) or "the wake is prompt" (>10s) | "ring lost/late (exit 3 matured) or sidecar refusal (exit 2) or stall" | sidecar exit 0 within the prompt bound |
| A3 (C's F2-H3, T8) | "the sleeper placed ZERO requests" | "proxy accept-loop counts an arming-era connection after at_sleep snapshot" | a counted connection whose accept timestamp falls INSIDE the window — that is a real poll, not a bookkeeping artifact, and a far worse finding |
| A4 (C's F2-H4, T6) | "the sidecar never created its rendezvous" | "sidecar slow start OR exit-2 refusal (transient Storage on read_own_lease) with the pipe never appearing" | sidecar stderr shows a clean start and the pipe simply appeared late |
| A5 (C's F2-H5, T7) | "index out of bounds" panic at replies[1] | "MCP initialize slow/failed -> mcp_via returns <2 jsonrpc lines -> raw index panic that is NOT an assert and today activates no diagnosis" | a captured failure at `replies[1]` where the MCP stdout in fact carried >= 2 jsonrpc lines |

- **A3's fix direction is pre-committed against a named weakening (verbatim, A, settled with
  J as P3-F2):** "QUIESCENCE variant — bounded wait for a stable connection counter BEFORE
  opening the window, and KEEP counting raw connects. Counting completed HTTP exchanges
  instead would weaken the instrument: a polling regression that merely CONNECTS
  (refused/timed out) crosses the window unseen, and a connect IS a placed request.
  Exchange-counting is rejected unless it ships with a connect-only-poll sabotage that goes
  red." Sealed consequence: an A3 fix that lands on exchange-counting WITHOUT that sabotage
  scores as a KILLED design constraint, whatever the resulting flake rate.
- **UNINFORMATIVE for the whole table:** a failure whose text matches NO row above. Note that
  A5 now carries C's F2-H5, so the previously uncovered `replies[1]` index panic DOES select
  a branch — but it selects a diagnosis-only branch: A's own fix there is (verbatim) "assert
  replies.len() >= 2 with the raw MCP stdout and stderr in the message". An A5 hit therefore
  names the harness, not the product mechanism, and must not be scored as explaining the
  flake.
- **UNINFORMATIVE, second form:** the text arrives WITHOUT sidecar stderr. A's step zero exists
  because (verbatim) "Sidecar stderr is piped and DISCARDED by the test today
  (spawn_wake_wait). Exit 2 (refusal) is therefore invisible and masquerades as downstream
  symptoms." An A4-shaped text with stderr discarded cannot distinguish A4 from A2.
- **Pre-registered rate scope (verbatim, A):** "with-number and base-number = failure rate of
  THIS test under the full gate and standalone, N >= 20 each (3/4 base rate makes N=20
  decisive), scope = full gate on this machine at the fixed commit W1 measured. '0 in N'
  reported as such, never as 'fixed'."
- **Status:** OPEN.

### A-CAP — the capture patch changes no behaviour

- **Author:** A Agent (protocol per orchestrator ruling). **Source:** same file, "Run
  protocol".
- **Verbatim:** "H runs CLEAN BASE first (no patch), then a separate CAPTURE RUN with my
  patch at same N>=10; rate match between the two validates the zero-behavior-change claim.
  My fix's with-number compares against the CAPTURE-run base (same instrument on both
  sides)."
- **CONFIRMS:** clean-base and capture-run failure rates match within sampling noise at
  N >= 10 each.
- **KILLS:** the rates differ. Then the diagnostics are themselves load-bearing — the
  instrument perturbs the thing it measures — and no later with-number may be compared
  against the clean base.
- **UNINFORMATIVE:** only one of the two runs is performed; or the two runs use different
  scopes (full gate vs standalone), which makes a rate difference unattributable.
- **Sealed note (M):** this row is the reason the fix's with-number is defined against the
  CAPTURE-run base rather than the clean base. If A-CAP scores KILLED and anyone still
  compares against the clean base, the comparison is void.
- **Status:** OPEN.

### A-TRAP — the reaper deadlock, pre-registered (TRAP row, not a scoring row)

- **Author:** A Agent (caveat named per J's P3-F4). **Source:** same file, "Capture patch
  caveat".
- **Verbatim:** "`reap_with_stderr` reads stderr AFTER wait(); safe only while the sidecar's
  stderr stays under the OS pipe buffer (today: at most one refusal line). A future chatty
  sidecar would deadlock the reaper. Deliberately NOT hardened now".
- **Ledger role (M):** carried for the same reason as D-H6. Two independent surfaces in this
  milestone (the server under `serve_with`, the sidecar under `reap_with_stderr`) can each
  turn extra diagnostic output into a hang that is indistinguishable from the flake being
  diagnosed. If a hang appears after any future sidecar becomes chatty, this row is the
  pre-registered explanation.
- **Status:** N/A (constraint, pre-registered).

### C-F2H2 — the named-pipe reading that an exit-3 capture would overturn

- **Author:** C Agent. **Source:** `.factory/c-agent-wake-flakes-study.md`, F2-H2.
- **Verbatim:** "Named-pipe semantics make the create-to-connect gap benign (a client connect
  before ConnectNamedPipe queues; the server then gets ERROR_PIPE_CONNECTED = success), so a
  genuinely lost ring needs an error path we did not find in reading — if W1's capture shows
  exit 3, this jumps to primary and the ring() error kinds need instrumenting".
- **CONFIRMS the reading:** no exit-3 captures across A's N >= 20.
- **KILLS the reading:** any captured exit 3. C pre-committed that this promotes F2-H2 to
  primary — a rare row where the author named what would make him more wrong-footed, and it
  should be scored as such.
- **UNINFORMATIVE:** exit status not recorded alongside the assertion text.
- **Status:** OPEN.

### JP1 / JP2 / JP3 — J's reviewer predictions (registered by J, sealed by M)

- **Author:** J Agent, flake-2 adversarial reviewer. **Source:** J's message to this ledger,
  also recorded in `.factory/j-agent-flake2-review-prep.md` section "SEALED PREDICTIONS"
  (j-agent-b9ad3f worktree). Registered directly by the author, per the orchestrator's ruling
  that a scorer must not transcribe an author's predictions.
- **Not verified against the prep file by me (M).** I have not read J's worktree. These rows
  are sealed as J stated them; if the prep file differs, J's file is the source and this row is
  stale for that difference — same rule as the Seal integrity section.

| ID | Prediction (verbatim) | CONFIRMS (J) | KILLS (J) | UNINFORMATIVE (J, plus M's additions) |
|---|---|---|---|---|
| JP1 | "Dominant failing text of flake 2 on clean base = 'the lease burned on the ring' (live:true or lastConsumed null; wake_http.rs:824)" | "that text modal in H's N>=10 base captures" | "any other text modal" | "<4 captured failures (no stable mode), or no mode at all (then JP1 scores half-right at best)" |
| JP2 | "'placed ZERO requests' text (A3 branch) appears in ZERO of H's failing base captures" | "zero occurrences" | "one occurrence" | "total failures <10 bounds a rare mechanism only to ~<25% — absence weakens, does not confirm" |
| JP3 | "Capture run's failure rate matches clean base within binomial noise" | "overlapping binomial CIs at the run Ns" | "divergence beyond noise (then the instrument is the finding and my 'apply-ready' verdict gets re-examined)" | "runs under different machine-load profiles (same-conditions required)" |

- **J's mechanism bet under JP1 (verbatim):** "sweep phase-3's EXCLUSIVE store open starves
  behind the woken sleeper's own SHARED-lock re-read traffic (mcp_via #2 = several shared
  opens); 53d212d is the commit that made reads shared, so flake-2's rate may have RISEN at
  53d212d." J marks this as reasoning formed at seal time.
- **JP1 SPLITS INTO TWO CELLS WITH TWO EVIDENCE SOURCES (J's precision request, adopted).**
  - **JP1-headline — the modal text.** Scores on H's base captures ALONE. No parent-run needed.
    Cells exactly as J wrote them in the table above.
  - **JP1-mechanism — "flake-2's rate may have RISEN at 53d212d".** This is the mechanism bet's
    falsifier, NOT the headline. It scores only against a parent-run of flake-2's own test.
  A kill of one does not touch the other: J can call the modal text correctly while the
  rate-rose sub-claim dies, or the reverse.
- **CROSS-LANE CORRELATION, sealed — AMENDED after J's partial pushback (M).** JP1-mechanism and
  B-P3 are the same wager on the same commit in two lanes: both say the shared-read widening at
  `53d212d` raised a failure rate.
  - **The anti-corroboration half stands, unchanged and accepted by J:** they are NOT mutually
    corroborating. A confirm in one lane is never cited as support for the other. One commit,
    one reading of it, applied twice.
  - **My "scores BOTH or NEITHER" clause was WRONG and is withdrawn.** J is right that it
    over-couples. The two bets run through DIFFERENT mechanism channels and are measured by
    DIFFERENT tests: J's is writer starvation (the sweep's EXCLUSIVE open queued behind the
    sleeper's shared re-read stream, flake-2's test); B's is validation reads OVERLAPPING (two
    concurrent sweeps, flake-3's test). Coupling them would have made a legal, informative
    outcome unscoreable.
  - **Corrected wording, J's, sealed:** *scored independently, each by the parent-run of ITS OWN
    flake's test; neither cites the other; joint confirmation still is not convergence — one
    commit read twice.*
- **JP3 vs A-CAP, sealed (M).** These are the same claim registered by author (A) and reviewer
  (J). Not independent evidence. But JP3 carries a stake A-CAP does not: J pre-committed that a
  divergence re-opens **his own "apply-ready" verdict**, not merely A's patch. Scored together;
  the extra consequence attaches only to JP3.
- **JP1 vs A-BT, sealed (M).** JP1 predicts the modal text is A1's selector, which would select
  A's branch A1. A JP1 kill therefore does NOT kill the A-BT table — it just selects a
  different branch. The two rows measure different things off one capture: A-BT asks WHICH
  branch, JP1 asks whether J called it in advance.
- **J's explicit non-predictions (verbatim), sealed so silence is not later read as a claim:**
  "relative order of the non-modal texts (exit-0 / prompt / rendezvous / index-panic) — no
  evidence either way; anything about flakes 1 and 3 — not my pen."
- **Status:** all OPEN.

---

## D. Cross-milestone claims (not flake rows, but falsifiable and unowned)

### A-M10 — the three owner numbers with no in-repo source

- **Author:** A Agent. **Source:** `.factory/a-agent-m10-proposal.md` (a-agent-c35d10
  worktree).
- **Verbatim:** "The task quoted three numbers: '48% of request time is history re-read',
  'load_state runs ~3x per request', '15ms -> 36ms during a run'. **None of these numbers has
  a source inside the repository.**" — after a stated search over docs/, *.md, commit
  messages, and issues #63 / PRs #70/#66/#62: "Zero hits."
- **A's split verdict (verbatim):** "any M10 milestone must START by reproducing the base
  numbers in-repo, with the 4-piece rule ... before any fix is written. The '~3x' shape,
  however, I can CONFIRM structurally from code (section 1) — the multiplier is real even if
  the milliseconds are unsourced."
- **CONFIRMS:** an in-repo reproduction lands in the stated numbers' neighbourhood, with N and
  scope.
- **KILLS:** an in-repo reproduction lands materially off them — in which case the M10 premise
  is re-derived from the new number, not patched onto the old one.
- **UNINFORMATIVE:** the owner supplies the off-tree measurement's method. That would explain
  the numbers' origin but would NOT satisfy A's requirement, which is in-repo reproduction
  under the 4-piece rule.
- **Owner-facing note (M):** the only row here whose resolution needs the owner rather than a
  run.
- **Status:** OPEN.

### E-K1 / E-K2 — SCORED EXAMPLE ROWS (what a closed row looks like)

Both are already resolved. They are kept as the worked example of the format, and because a
ledger of only OPEN rows teaches nobody how one is meant to close.

**E-K1 — the hang self-heals.**

- **Author:** E Agent. **Source:** `.factory/draft-second-story.md` (main checkout), fact 1.
- **Verbatim:** "The tool host applies a fixed 300s timeout to every shell call, independent
  of the operator (`apps/cli/src/commands/serve/ports.rs:27`, `TIMEOUT_SECONDS: u64 = 300`,
  no per-call override ...). On expiry the process is killed and the disposition is `TimedOut`
  (`adapters/tool-host/src/host.rs:106,175`), which maps to `NodeOutcome::RetryableFailure`,
  **not** `Interrupted` (`core/runtime/src/executor.rs:241-243`). A `RetryableFailure` on
  attempt 1 (well under `MAX_NODE_ATTEMPTS = 8`, `core/execution/src/bounds.rs:7`) goes to
  `Queued` (`core/execution/src/transition.rs:97-105`), and the async driver auto-redispatches
  `Queued` nodes with **no operator action** (`core/runtime/src/driver.rs:470-488`,
  `retry_pending` folded into `candidates`)."
- **Consequence claimed (verbatim):** "the deploy node self-heals via ordinary retry machinery
  before `pause`/`approve` are ever forced, and the failure is silent (the judge just watches
  it finish and reports fine)."
- **CONFIRMS:** the code path reads as stated at the named lines.
- **KILLS:** any link in the chain missing — e.g. `TimedOut` mapping to `Interrupted`, or
  `Queued` nodes not auto-redispatched.
- **UNINFORMATIVE:** a paid judge run that happens not to hit the 300s clock. It would show
  nothing either way — which is precisely why this was settled by code reading, not by a run.
- **Status:** **SCORED — CONFIRMED.** Instrument: the orchestrator's independent code check
  (recorded on the board as "2 CONFIRMED kills (verified by orchestrator)"). Zero runs, zero
  paid calls. Consequence taken: the story spec was BLOCKED for paid runs and the redesign
  moved to E, with the lever `MAX_IDENTICAL_OUTCOMES = 3` (`core/execution/src/bounds.rs:10`)
  giving a deterministic Blocked instead of a race.
- **Why this row is the model (M):** the disconfirming condition was structural and checkable
  without spending a paid run, and it was checked by someone other than the author. Both
  properties are what the OPEN rows above are trying to buy.

**E-K2 — immediate pause does not kill the child.**

- **Author:** E Agent. **Source:** same file, fact 2.
- **Verbatim:** "`apps/cli/src/commands/serve/ports.rs:219-221`, verbatim: 'No cancel hook:
  `ToolHost` exposes no kill surface for in-flight children beyond what the process's own
  deadline already bounds ... the default no-op is accepted.' The driver's
  `in_flight.abort_all()` (`core/runtime/src/driver.rs:590`) drops the Rust future wrapping a
  `tokio::task::spawn_blocking` call (`ports.rs:199`); dropping that handle does not stop the
  blocking closure. The real child process survives, orphaned, until its own 300s deadline."
- **CONFIRMS:** the no-op cancel and the spawn_blocking drop semantics read as stated.
- **KILLS:** a kill surface existing anywhere on the in-flight child path.
- **UNINFORMATIVE:** observing a pause that appears to work because the child finished on its
  own first.
- **Status:** **SCORED — CONFIRMED**, same instrument as E-K1.

### The second judge story's paid run — RECORDED, NOT SCORED

E reports a real 565.9s paid session with a real verdict (`passed:false`), archived with
`verdict.json`, full journal, read-audit, and two byte-identical replays (determinism confirmed).

- **I HOLD NO SEALED CELLS FOR IT, AND I DID NOT SCORE IT.** The coverage prediction lives in
  `m08-judge-coverage.md`, not here. **Rule 3: a prediction scores only against the instrument its
  author named, and only its holder scores it.** Same ruling I gave the orchestrator on C-P1.
  Recorded so the omission is a boundary, not a gap.
- **E-K1 and E-K2 are neither confirmed nor refuted by this run.** They are the rows that BLOCKED
  the spec for paid runs; their consequence played out (redesign, then a real run). **Not stretched
  to cover it.**
- **What I applied, as standing rules rather than as scoring:**
  - **The coverage claim is an ELIMINATION argument with an unstated population (rule 9).** "All 7
    exercised, confirmed by elimination against the audit" holds **only if the read-audit captures
    every call**; nothing offered shows a missed call would have been observable. The audit's
    completeness is the load-bearing premise.
  - **E graded his own evidence tiers unprompted** — two CRITICAL items verified by him against the
    code, six graded HIGH/MEDIUM/LOW from the judge's text and explicitly **not** independently
    re-derived. That split is the discipline this file extracts from people; he supplied it first.
  - **Rule 10 deflation on his finding 2:** the judge's sequence and E's own sealer-key encounter
    are independent of misreading the judge's prose, **not** of a shared misunderstanding of the
    code — same path, two actors. **His verification against source carries that row; the pairing
    does not.**
  - **No double-counting across lanes:** his LOW-tier items map onto the attention lane's seeds and
    J's `wake_wait` design. One observation, one lane.
  - **Open question I cannot answer and E can:** his redesign's lever was
    `MAX_IDENTICAL_OUTCOMES = 3` giving a **deterministic Blocked** instead of a race. **Did it
    fire?** `passed:false` with real refusals is not the same fact.
- **ALL THREE ANSWERED WITH EVIDENCE, and I verified the load-bearing one myself.**
  - **Rule 9 discharged — the population is now STATED and SOURCE-VERIFIED, not inferred from the
    audit's own contents.** E checked the architecture: every `/v1/executions/*` and
    `/v1/gateway/*` route — the entire MCP-tool-backed surface — is registered BEFORE the
    `record_read` middleware layer, which wraps unconditionally and sits **outside the auth layer,
    so a refused request is recorded too** (the code's own comment). The one gap, `/monitor`, merges
    AFTER that layer but is an HTML dashboard never exposed as an MCP tool — **structurally outside
    the population the coverage claim is about.**
    - **VERIFIED BY ME at `serve/mod.rs:288-320`:** the route list, then `.layer(require_token)`,
      then `.layer(record_read)` with the "OUTSIDE the auth layer" comment, then the monitor merge.
      **E's reading is exact.**
    - He also ran the positive control as specified: a known call cluster found at **index 31-35 of
      67 — the file's true middle, not the ends** — well-formed and sequential. **Control drawn
      from the wrap-prone zone, per L's refinement, rather than from the safe one.**
  - **Deflation ACCEPTED and RETRACTED at the source.** E withdraws "two independent confirmations,
    not one": the judge's sequence and his own encounter are **two OBSERVATIONS of one code path**
    (`resume.rs`'s `execute_prepared` committing before `drive()`'s setup), and a shared misreading
    would explain both. **What carries the finding is the source read itself; the pairing is
    corroborating colour.** He flagged it so **issue #83's body — already filed — is not later
    cited as "confirmed twice" when it is confirmed once, by code.** That is rule 6's descendant
    discipline applied **forward, to an artifact already shipped.**
  - **Q5 ANSWERED — the redesign's lever FIRED, verified by the transition field not by a count.**
    Traced against this run's own event store: `deploy` at seq 11/13/15/17 shows four consecutive
    `retryable_failure`/`tool_exited_non_zero`, and **seq 17's own transition is
    `retryable_failure → blocked`** — `MAX_IDENTICAL_OUTCOMES = 3`'s exhaustion arm, read off the
    actual `nextState`. **The redesign's own prediction landed.**
- **THE DISTINCTION THAT MUST SURVIVE THE CLOSE DOC (E's, and it is the kind that gets flattened).**
  After the block, the judge triaged correctly (seq 22 `approved`), paused, and let a resume
  redispatch (seq 28 `paused`, seq 32 `started → queued`) — **and the stream ends at head 35.**
  Deploy's queued 5th attempt was **never dispatched**, because every resume from that point hit
  **#82's workspace-overlap refusal** before the drive loop could start. **So this run is NOT "the
  redesign failed" — it is "the redesign worked, and then something else did."** One is a verdict
  on E's design; the other is a verdict on an unrelated bug, and a close doc that compresses them
  into "the paid run did not complete" loses which.
- **Practice worth recording:** E left his **6 failed attempts in the journal on the same execution
  id**, not scrubbed — the same principle as H's `results.csv` carrying every invocation including
  gauge runs. **A trail that shows the clumsy attempts is what makes the clean one checkable.**

---

## D-bis. Standing findings — not predictions, and they do not wait for a run

Entries here are settled by reading, carry no CONFIRMS/KILLS cells, and are recorded because
they change what a green result MEANS. They are not scored.

### SF-1 — the #55 family's oracle is blind to its own worst case

- **Author:** C Agent, raised in response to an observation I had filed only as a note. C is
  right that it is bigger than I flagged it, and it is his finding, not mine.
- **The finding.** Every guard in the #55 family uses "the stream still replays" as its ORACLE
  — the in-module reds, the 15-round belt, all of them. That oracle is by construction blind to
  any failure that keeps the log LEGAL. C-R3 is exactly such a failure: burning a live lease is
  legal, so replay succeeds, and the operator's own surface reports calm.
- **Consequence, and it is about test strategy rather than the product.** If C-R3 confirms, it
  does not merely add a defect: it shows the family's oracle cannot detect its own worst case,
  and **every green from that oracle means less than it reads.** That applies retroactively to
  greens already banked, including the belt test whose passing was already weak evidence (its
  own commit records that it never reproduced the race).
- **Why it is here and not in a scoring row.** It needs no run. It is a property of the oracle,
  derivable today. Filing it as a prediction would make a settled fact wait for H.
- **Scheduling note (owner/orchestrator call, not mine):** C asks that this not wait for PR 2,
  even though C-R3 itself is parked as second-PR work. The finding and the seed have different
  deadlines — the seed is a defect to schedule, this is a statement about what our existing
  green results are worth.

### SF-2 — `ring` collapses every error shape into StaleRendezvous (SEED, C's pen)

- **Author:** L Agent, thrown off the corpse-3 audit; explicitly not L's pen and not acted on.
- **The finding.** `ring` maps EVERY error shape to `StaleRendezvous` (`serve/wake.rs:57`,
  `:59`), `ERROR_PIPE_BUSY` included. A sleeper that is ALIVE but momentarily between accepts
  would be classed stale, its lease consumed and RECORDED as consumed — a silently lost
  doorbell rather than a missing one.
- **Honest counterweight L states:** the doc comment says the intent is that a missing pipe is
  the designed case and anything else is equally a dead rendezvous, so this may be deliberate.
  Nothing in the code distinguishes the two today.
- **Relationship to an existing row (M):** this is the same instrument gap C already named in
  his flake-2 sabotage list (S8: instrument `ring()` to record WHICH io error produced
  `StaleRendezvous`). SF-2 raises the stakes: today it is a diagnosis gap; if the busy-pipe path
  is reachable, it is also a silent-loss defect. Same shape as SF-1 — a failure that leaves
  every surface calm.
- **Routing:** C's pen (wake path). Recorded here so it is not lost between two reviewers'
  worktrees.

---

## E. Known-false statements with NO owner

Per the orchestrator's ruling, the staleness sweep is HELD because it overlaps F's
close-skeleton and the flip-with-fix-commit obligations already pinned to owners. Statements
surfaced by the convergence-refutation pass that have NO owner land here instead, with the
statement, its file, and why no existing obligation covers it.

- **None yet.** Populated, if at all, by the convergence-refutation pass.

For the record, these known-false statements DO have owners and are deliberately NOT listed
here: the `wake.rs:146-149` doc comment (flips with C's fix commit, by ruling), the stash
history note (C's commit must state the window predates 53d212d — C's gate G7), and the
`ServeState.events` doc comment contradicted by `open_inner` (D's storm study, candidate-fix
direction).

Separately noted, not a repo statement: `.factory/handoff-agent-b.md` is STALE by its own
staleness-anchor rule (it names main `9aa4075` / M07; main is `efd85d0`). It is a beacon
rewritten at each handoff, not a durable doc, so it needs no flip obligation — it needs
overwriting by whoever next writes a handoff.

---

## SCORING ROUND 1 — against H's clean base (`.factory/h-agent-base-measurements.md`, main checkout)

Scored 2026-08-19 by M. **Every number below I read out of H's report myself** rather than
taking it from the reporting agent's message — including the rows where the agent was scoring
his own work. Base only: N=10 isolated + 3 in-suite per test, at `53d212d`, no instrumentation
patches applied.

### Storm lane

| Row | Verdict | Evidence |
|---|---|---|
| D-P1 (panic at `:464`) | **CONFIRMED** | `:464` in all 6 failing runs (1-5 hits each); ZERO `:443`, ZERO `:460-461` |
| D-P2 (10060, not 10061) | **CONFIRMED** | every captured error `Os { code: 10060, kind: TimedOut }`; 10061 appears nowhere |
| D-P3 (storm thread, not verify) | **CONFIRMED** | verify never panicked; main thread dies only at the join (`:1468`) |
| D-P4 (rate ~50%) | **CONFIRMED-LOOSE** | 4/10 isolated (40%), 2/3 suite (67%), 6/13 combined (46%) |
| D-P0 (free phase read) | **SURVIVES** — see ruling | phase known from `post_request` lines in every failing run |
| D-DT (decision table) | **NOT-YET** | its instrument (Patch 1) has not run; no elapsed data exists |
| D-T2 (10061 on loopback connect) | **UNINFORMATIVE — vacuous** | zero connect-phase failures captured; the claim was never exercised |
| H5 (port exhaustion) | **DEAD** | H5 predicts 10048/10055; every captured code is 10060, and the CODE is legible even in the laundered rows |
| H4 (connect starvation) | **CORRECTED — DISFAVOURED, not dead** | see the self-correction below |

**RULING ON MY OWN SEAL 2, which D correctly refused to make in his own favour.** I sealed:
"a `:220-222` landing scores D-P1 UNINFORMATIVE and simultaneously KILLS the free-read row."
The capture is mixed: `:221` fired 4 times across 3 of the 6 failing runs, but **never alone** —
every run carrying it also carried a clean `:464`.

- **My seal's UNIT was under-specified. That is my error, not D's.** I wrote it as though a
  capture lands at one line. Specified now: **the unit is the failing RUN, and its phase-deciding
  panic.** A run scores as a laundered landing only if its phase-deciding panic is at `:220-222`
  with no `post_request` hit in the same run. **Zero runs meet that.**
- **Ruling: D-P1 CONFIRMED, free-read row SURVIVES.** The seal's purpose was to stop a laundered
  hit being counted as phase evidence. No run's phase attribution depended on a `:221` hit, so
  the purpose is satisfied.
- **The caveat that survives the ruling, and it is not D's framing.** Counted per PANIC rather
  than per run, 4 of the 20 captured timeout panics (`:464` × 16, `:221` × 4) are
  **phase-unattributed**. "The phase is known for free" is true of every RUN; it is NOT true of
  every PANIC. Sealed, because anyone later counting panics instead of runs would inherit an 80%
  attribution rate as though it were 100%.

**SELF-CORRECTION — I scored H4 DEAD and my own seal forbids it.** L's flattening analysis
(rule 7 below) caught the consequence I had sealed and then failed to follow through on, in the
same table.

- I sealed, before the run: 4 of the 20 captured timeout panics are **phase-unattributed**
  (`:221`), and D-T2 — the loopback-gives-10061 argument — is **accepted by argument, not
  measured**, kept as a row precisely "because an argued acceptance later cited as a measured
  fact is exactly the drift this file exists to stop."
- Then I wrote "H4 DEAD — zero connect-phase panics". Both halves of my own seal say I may not.
  H4 is specifically a **connect-phase** hypothesis, and phase is exactly what the `:221`
  laundering destroys. A connect-phase failure, had one occurred, would have landed in those 4
  rows and been indistinguishable there. "Zero connect-phase panics" is true only of the 16
  **attributed** panics.
- **Corrected verdict: H4 is DISFAVOURED, not dead.** Its remaining supports are (a) zero
  connect panics among the attributed 16, (b) D-T2's argument, which I scored UNINFORMATIVE and
  vacuous because no connect failure was ever captured to test it against, and (c) no elapsed
  data at all, so H4's ~21s SYN-retry signature was never checked either way. Three weak
  supports, none measured.
- **H5 stays DEAD, and the contrast is instructive:** H5 predicts error codes 10048/10055, and
  the CODE is legible even in the laundered rows — every captured code is 10060. Laundering at
  `get_status` destroys the PHASE, not the error code. So the flattened boundary killed exactly
  one of the two hypotheses' evidence, and it was the one that depends on phase.
- **Recorded as the ledger working on its keeper.** This is the third self-correction in the
  file and the first found by a peer applying a rule the file itself adopted.

**RECONCILED FINAL STANDING — two of our messages crossed and produced two different H4
verdicts. This is the one to quote.** D narrowed his own row at the same time I corrected mine,
and his narrowing is more precise than my "disfavoured", so the combined form is:

- **H5 — DEAD BY DATA, both paths.** Exhaustion would show 10048/10055; every captured code is
  10060; the code survives even at the laundered site.
- **H4 — DEAD ON THE MUTATION PATH ONLY** (zero `:443` among the 16 attributed panics). **NOT
  dead on the STATUS path**, where the phase is laundered — and its supporting premise (B's
  loopback-gives-10061) was **never exercised**, because zero connect failures were captured to
  test it against. So H4's remaining death is by an argument whose premise this run did not test.
  **Overall: disfavoured, alive on the status path.**
- **Anything that says flatly "H4 and H5 are dead" is stale**, including one line D copied into
  his report before his own narrowing landed. Flagged to the orchestrator before the owner report
  ships.

**D-STEP0-FREE-READ — DOWNGRADED to PARTIAL by D, against himself and against my ruling in his
favour.** I ruled the row SURVIVES on a run-level unit. D accepts the unit but records that it
must not be read as retiring work:

- It survives for the **MUTATION path only** (`:464` unambiguous, zero `:443`, zero `:460-461`).
  It does **NOT** settle the **STATUS path**.
- His reason is sharper than my caveat and worth stating in his terms: the `:221` rows support
  BOTH readings at once — read-phase expiry (his D2 inference: a one-open status request
  exceeding 5s waited behind other work, H1 over H3) AND connect-phase death (H4 alive on the
  status path). **He cannot use them for his inference and also count them as killing H4** — same
  unknown, opposite conclusions.
- **PATCH 1 IS RE-PROMOTED, by D's own Step 0 rule.** That rule said Patch 1 becomes
  phase-deciding if the failure text names `get_status`. It named it **4 times across 3 runs**
  (iso3 ×1, suite1 ×1, suite3 ×2). Patch 1 is not the demoted step any more. **My generous ruling
  must not be read as "Patch 1 unnecessary after all"** — it is now needed to settle the
  status-path phase and to close H4 by measurement rather than by an untested premise.
- **D2 discriminator:** marked CONDITIONAL on Patch 1's `connect_ms`/`read_ms` split, not
  standalone.

**CONSTRAINT ON PATCH 1 (not a prediction) — the rewrap must preserve what killed H5.** H5 died
because the OS code survived into the panic text at the laundered site, via `probe_failure`'s
`({error})` interpolation. **Reformatting that later would retroactively destroy the evidence
that killed H5.** Also inert but worth stating: the rewrapped error has no `raw_os_error()`
(`Custom` returns `None`), so any future programmatic check gets `None` on exactly the path that
matters.

**D-P10-ALT — BLOCKED, THEN REPAIRED; history kept visible.** D's original "preserve the events
dir via `into_path()`" is **unreachable**: the panic propagates out of `run_storm` at the scope
join, nothing after it executes, and `TempDir::drop` deletes while unwinding. Replaced with
`catch_unwind(AssertUnwindSafe)` + preserve on `Err` + `resume_unwind`, which keeps the original
panic `file:line` intact. **The path must be printed and preserved on PASSING runs too, or there
is no baseline.**

**TWO MORE NARROWINGS D MADE AGAINST HIS OWN SIDE:**

- **The storm-phase denominator needs TWO ARMS.** `verify` runs only on PASSING runs, so the
  trailing <= 2 verify reads are a passing-run artefact; on FAILING runs the phase ends at the
  last `caller=request` row. **Applying the passing rule to a failing run would bias the serial
  share UPWARD — toward D's own preferred conclusion.** He named that direction himself.
- **D-TRAP-1 extends:** a panicking thread issues no further rounds, so the request POPULATION
  differs per run and run-to-run counts are **incomparable**. Normalise on **requests actually
  issued**, never on runs.

**D-P4's confirmation is weaker than it reads, and this is a scoring note, not a demotion.** At
N=10 isolated the 95% interval around 4/10 spans roughly 12-74%. A rate materially away from 50%
would very likely NOT have been distinguishable — so this row **could not have been killed by
this N**. A confirmation from an instrument that could not have disconfirmed carries little
weight. D self-scored it "confirmed-loose" before I raised this; recorded that he did.

**D-DT is NOT-YET, and this matters more than it looks.** DT-1 requires "`phase=read
kind=TimedOut`, `read≈5000ms`, `connect` small". The clean base gives phase and kind but **no
elapsed at all** — Patch 1 never ran. So the H1-over-H4 selection is carried by D-P0, D-P2 and
the zero connect-phase panics, **not by DT-1**, and the "≈5000ms starved of a response"
magnitude remains unmeasured. My original sealed cell still binds on top: **any DT sample leaves
H2 and H3 unscored.** So: **H5 is dead; H4 is disfavoured, dead on the mutation path only**
(this line read "H4 and H5 are dead" until the self-correction below — a stale summary caught by
the audit described at the end of this section); H1 is selected over H4, and **H1 vs H2 vs H3
remains entirely open.**

### Flake-2 lane

| Row | Verdict | Evidence |
|---|---|---|
| A-BT | **branch A1 SELECTED** | one form only, all 12 failures: "the lease burned on the ring", `live:true`, `lastConsumed:null` |
| JP1-headline (modal text) | **CONFIRMED** | that text in 12/12 failures; no other form ever appeared |
| JP1-mechanism (rate rose at `53d212d`) | **OPEN** | needs a parent-run of flake-2's test; not run |
| JP2 (zero "placed ZERO requests") | **CONFIRMED, at its own stated bound** | zero occurrences in 12 failures |
| A2 / A3 / A4 / A5 branches | **not selected** | no other assertion form appeared at base |
| JP3 (capture run matches base) | **UNINFORMATIVE — evidence source eliminated** | the dedicated capture run was skipped; no rate comparison will ever exist |

- **JP1's line number was off and I am recording it rather than smoothing it.** J predicted
  `wake_http.rs:824`; the actual assertion is `wake_http.rs:822:5`. The row's substance is the
  TEXT, and the text matched exactly, so this scores CONFIRMED — but a two-line drift in a
  registered prediction is the kind of detail that gets quietly corrected later, and it is
  written down here instead.
- **JP3 and A-CAP both die by ELIMINATION, not by measurement, and that is a different thing
  from being wrong.** A and J jointly decided (orchestrator-delegated) to skip the dedicated
  capture run, because H's unpatched base already decided the mechanism with a single text. So
  the instrument JP3 and A-CAP were both defined against will never exist. Scored UNINFORMATIVE
  by JP3's own cell logic — J reported this himself as an author's duty rather than letting the
  row sit open. **A-CAP scores the same way for the same reason.**
  - **What replaced it, recorded so the substitution is visible (M):** the zero-behaviour-change
    claim is now validated by a WEAKER instrument — TDD red observed k>=2 on the capture-patched
    tree, post-fix 0/20, plus the sabotage set. J recorded the substitution in his prep file
    rather than as a ledger row; noting it here because "validated by a weaker instrument" and
    "validated" are not the same claim, and only this line distinguishes them later.
  - **Consequence that follows and should not be lost:** my sealed note that the fix's
    with-number must compare against the CAPTURE-run base is now void — there is no capture-run
    base. Any with-number compares against H's clean base, and the instrument difference is
    unquantified rather than measured-as-zero.
    - **Now ENCODED where it operates, not merely noted (J, after this was raised).** His review
      checklist item 2 is amended: with-number compares against H's CLEAN base (the only base
      that exists); instrument delta UNQUANTIFIED, not measured-as-zero; and a PR wording mandate
      — **"validated by a weaker instrument", never bare "validated"**. The superseded
      capture-run-base sentence in his rulings section carries a SUPERSEDED annotation pointing
      at the amendment, so the trail is kept rather than silently rewritten.
  - **RESURRECTION CONDITION — the eliminated row is not quite dead (J's margin note, sealed).**
    The k>=2 TDD red on the capture-patched tree doubles as a crude **same-instrument tripwire**:
    if it fails to reproduce red (0/2) where the clean base ran 9/10 isolated, the instrument
    question REOPENS and the skipped capture run un-skips itself. It is the one observation that
    could still resurrect the voided row's question before merge, so recording it here is what
    keeps the void honest rather than final.
    - **One asymmetry I seal that J undersold (M).** He calls k=2 "almost nothing", which is true
      in the CONFIRMING direction — 2/2 red is consistent with a wide range of rates and licenses
      no rate claim. But it is NOT symmetric: at the clean base's 9/10, the probability of seeing
      **0 reds in 2** is about 1%. So a 0/2 outcome is genuinely surprising and the tripwire is
      **stronger than its author credits, in precisely the direction that matters** — the
      direction that reopens the question. Sealed so that a 0/2 cannot be waved off later as "k=2
      proves nothing", which would be true of 2/2 and false of 0/2.
- **JP1-mechanism margin note, registered by J at scoring time (kept for whenever it scores):**
  9/10 ISOLATED means the starving read traffic is intra-test — **consistent with the starvation
  channel, not sufficient for it.** J volunteered the insufficiency himself.
- **JP2's credit is bounded by J's own cell.** He pre-limited it: under 10 failures the absence
  bounds a mechanism only to roughly under 25%. There were 12, so his threshold is cleared —
  narrowly. What 12 clean absences actually license: a mechanism occurring more often than about
  1-in-5 is excluded; a rarer one is not. **Absence still weakens rather than confirms**, exactly
  as J wrote before the numbers existed.

### Flake-3 lane — and the result most likely to be misread

| Row | Verdict | Evidence |
|---|---|---|
| C-P1 (reproduces at parent) | **OPEN — UNTESTED** | H's mandate was base-only; the parent run is a separately scheduled slot |
| C-P2 / B-P1 (seam reds) | **OPEN** | typed, unbuilt, unrun |
| B-P2, C-R2, C-R3 | **OPEN** | no fix, no sabotage run |
| B-P3 (widening raised the rate) | **UNINFORMATIVE, as pre-registered** | no parent-run, so no rate comparison exists |

**THE BELT'S 0/13 IS NOT EVIDENCE THAT WINDOW 3 DOES NOT EXIST.** `concurrent_sweeps_never_double_consume_a_lease`
passed 0/10 isolated and 0/3 in-suite at `53d212d`. Sealed refusal, registered before this
result: that test is the belt, and **the fix's own commit records that the window was too narrow
to reproduce under test timing** — its passing was already weak evidence by its author's
admission. B-P1 and C-P2 are DETERMINISTIC seam tests that have not been run. A statistical
green from an instrument known not to reach the window says nothing about the window. Anyone
citing 0/13 as "window 3 is not real" is misreading it, and the sealed prediction it must be
tested against is C-P2's red, which does not exist yet.

**B-P3 scoring as expected-UNINFORMATIVE is itself the pre-registration paying off.** It was
sealed as expected-uninformative before any number existed, for a stated reason (every planned
instrument is deterministic and cannot report a probability). It came back uninformative for
exactly that reason. The refusal waiting for the future sentence "the widening raised the rate"
is now live.

### D's amendments #4, #5 and the new trap — accepted and sealed

- **AMENDMENT #4 — P5's accounting identities are PASSING-RUN ONLY.** Ground (L round 3,
  verified by D): a request killed by the 5s timeout has no client status code while the server
  may have committed and spawned its sweep, so client counts undercount server work; and a
  panicking thread runs no further rounds, shrinking the population unevenly. On failing runs
  both identities become inequalities. **Sealed: P5 scoreable on passing runs only; on failing
  runs record UNSCOREABLE, not failed.**
- **AMENDMENT #5 — "sweep rows == N(200)" DOWNGRADED from identity to a ONE-DIRECTIONAL test,
  with the asymmetry sealed explicitly.** L proposed the discrepancy as evidence for H1; D
  accepts the direction but not the symmetry, because L's own leak 3 confounds it oppositely —
  on a failing run the scope re-panics at join, verify never runs, `ServerGuard::drop` kills the
  child, so queued-but-unscheduled sweeps leave NO row. Leak 1 inflates, leak 3 deflates, both
  live on exactly the runs that matter. **Sealed: discrepancy > 0 CONFIRMS commits past timeout
  (leak 3 can only subtract, so a positive residue cannot be manufactured). Discrepancy == 0
  scores UNINFORMATIVE — NOT support for a hang hypothesis, NOT a refutation of H1.**
- **D-TRAP-1 — wall-clock duration is not a load proxy, so it must never carry attribution.**
  D's ground: failing runs end early because the panic ends the test, so a REAL fix will
  INCREASE wall time by letting every round complete; duration-based attribution would score a
  real fix as a regression and could score a regression as a fix. Applies directly to sabotage
  item 8 ("the fix must move a measured number"), which must not reach for run duration.
  **Accepted and sealed as a trap row.**
  - **AMENDED to a stronger and less flattering form, by D, correcting himself.** He originally
    sent "failing runs are FASTER (19-26s vs 23-31s)", having mixed isolated and suite runs from
    H's summary. Reading H's per-run table directly — as I did independently, reaching the same
    numbers — the isolated set is: failures 22, 21, 24, 26s (mean 23.3); passes 23, 31, 29, 24,
    23, 24s (mean 25.7). **The ranges OVERLAP; a 26s failure sits above three 23-24s passes.**
    Final sealed form, D's wording: **run duration CANNOT DISTINGUISH pass from fail in either
    direction, so it is unusable for attribution — not merely biased.** The truncation mechanism
    (the panic ends the test early) is unchanged and still means a real fix will lengthen runs.
  - **Worth noting how this one went (M).** I flagged the overlap from H's table; D had already
    caught it himself by reading the same table rather than his own summary, and amended AGAINST
    himself to the less flattering claim. Two independent reads, same correction, neither
    deferring to the other's summary. That is the verification posture working, and it is the
    reason the trap now rests on a mechanism instead of a gap the data does not show.

### D-P10 / D-P10-ALT — the leak-as-instrument rows (sealed BEFORE rung A runs, per orchestrator)

- **D-P10.** On a FAILING run, `(caller=sweep rows − N(200 observed)) > 0`.
  - **CONFIRMS:** discrepancy > 0 — mutations committed server-side while the client timed out.
    Direct H1 evidence, **independent of phase timing**, so it corroborates Patch 1 rather than
    duplicating it.
  - **UNINFORMATIVE by construction:** discrepancy == 0. Leak 1 inflates and leak 3 deflates,
    both on exactly the runs that matter, so only a POSITIVE residue is unmanufacturable. A zero
    is neither support for a hang nor a refutation of H1.
  - **UNSCOREABLE:** untagged run — same `caller=` dependency as P5.
- **D-P10-ALT** (preferred instrument if the mechanics land): count COMMITTED DECISION EVENTS in
  the store after a failing run against client-observed 200s. **Immune to both leaks** — durable
  events care about neither client status codes nor unscheduled sweeps.
  - **Blocked on one thing, and it is a flattening-adjacent hazard worth naming (M):**
    `tempfile::tempdir()` deletes on unwind, so **the panic destroys its own evidence**.
    Preserving the dir on failure is a small Patch 1 addition. Until then D-P10-ALT cannot run at
    all — and a run that silently lost its journal would look identical to a run with nothing to
    find.
- **Status:** both OPEN, neither run.

### D-C1 — an inherited CONDITION, not a prediction (logged, re-checked, never scored)

- **Origin.** D claimed `committed decision events − caller=sweep rows` IS the count of sweeps
  killed before they ran (leak 3's magnitude). L showed it is a **CEILING, not an identity**,
  verified in code: `resume` commits its decision at `execute_prepared`
  (`routes.rs:677-684`) and only THEN awaits drive; a drive returning `Err` leaves the decision
  durable while the mutation takes the `Err` arm and never reaches the sweep spawn
  (`serve/mod.rs:806` is Ok-arm only). So the difference = **(unscheduled sweeps) + (committed-
  then-failed mutations)**.
- **D's resolution, pre-run, from evidence already collected.** A failed drive maps to
  `GHCLI016_DRIVER_FAILURE` → `respond_failure` → **500**, and `assert_storm_status`
  (`api_http.rs:1517-1523`) panics on any status outside {200, 400, 409}. H's panic inventory
  across all 13 runs is {`:464`, `:221`, `:1468`} only. **`:1518` never fired**, so — D argues —
  the second term is zero and the ceiling collapses to the identity.
- **VERIFIED IN SOURCE BY ME:** `assert_storm_status` exists with exactly the accepted set
  {200, 400, 409}. D's reading of the function is correct.
- **NARROWED (M) — the inference is too strong, and the argument against it is D's OWN
  amendment #4.** The assert's own doc comment says it plainly: *"a hang would already have
  panicked inside the request helper via its 5s timeout, before this function is even reached."*
  So `assert_storm_status` only ever sees requests whose status was **observed**. A mutation that
  committed and whose drive then failed **slowly** — past the client's 5s bound — panics at
  `:464` and its 500 never arrives, so `:1518` cannot fire even though a committed-then-failed
  mutation occurred. **This run has 20 such timeout panics**, and they are precisely the
  population where the second term could hide.
  - This is D's own amendment #4 turned on D-C1: he sealed that on failing runs "a request killed
    by the 5s timeout has no client status code while the server may have committed". That is
    exactly the case C1 needs to exclude and cannot.
  - **It is also the H4 error's shape, third occurrence:** "zero X" is true only of the OBSERVED
    subset, and the hypothesis at stake lives in the unobserved one.
- **CORRECTED CONDITION, sealed:** while no `:1518` fires, `committed − sweep rows` is leak 3's
  magnitude **over the requests whose status was observed**. The timed-out requests remain
  unaccounted, so the ceiling collapses only over that subpopulation — **not globally**. If
  `:1518` EVER appears, even the narrowed form reverts to a ceiling and the two terms must be
  separated before the number is used.
- **Why it is a condition and not a prediction (D's reason, kept):** it is already satisfied by
  existing evidence, and its job is to be **re-checked, not scored** — "a conclusion copies
  forward, a condition makes the reader check." Same reasoning that retired bare standing lines.
- **Consequence D draws, and it survives the narrowing:** commits-past-timeout =
  `committed − N(200 observed)`, computed from the STORE and the CLIENT CODES with the sweep out
  of the calculation entirely — which is **D-P10-ALT**, now reachable from a second independent
  direction (L's drop-detection argument produces it as well as D's evidence-preservation
  argument). **Two independent derivations of the same instrument, before any run**, is the
  closest thing to corroboration available pre-measurement — and note it is corroboration of an
  INSTRUMENT CHOICE, not of a result. D-P10 (the sweep-based form) stays as the fallback if
  directory preservation fails to land.
  - **And the narrowing strengthens rather than weakens that consequence (M):** D-P10-ALT works
    from the STORE, so it does not depend on observing a client status at all. It is immune to
    exactly the hole that narrowed D-C1. The row C1 was meant to support is the one row the
    correction does not touch.

### What this round did NOT settle

- H1 vs H2 vs H3 — untouched. H5 died; H4 is disfavoured, dead on the mutation path only.
  (This line also read "only H4 and H5 died" until the summary audit below.)

### SUMMARY-LINE AUDIT — the retraction rule's third clause, applied to this file first

D found that my H4 correction had **four survivors in his report**, after he had already fixed
three: an opening summary, the DT-1 bullet's own net-standing line, a sentence calling H4's
standing "H4's death" while arguing it is not dead, and a pre-run prediction. His observation,
which is the durable part: **the BODY carried the qualified verdict in three places while four
SUMMARY constructions carried the superseded strong one.** Summary lines are where retracted
claims survive longest, because they are what gets copied forward and they are written in
compression mode rather than reasoning mode. He committed the error three messages AFTER we named
the class, in a message whose own body had the correct version.

**Third clause proposed by D and adopted into scoring rule 6:** a retraction is finished when
everything it PRODUCED is re-derived, everything it RETIRED is re-justified, and every SUMMARY
RESTATEMENT is checked against the body.

**AMENDED IMMEDIATELY — the clause said RE-READ; what actually works is GREP.** D caught that my
own message sealed one method and reported another: I wrote "every summary restatement is
re-read", and in the same message described what I had actually done — *"I grepped before
replying and found two stale summaries of my own."* I did not re-read. I searched for the token.

- **L's reason, supplied independently and it is the load-bearing part:** re-reading fails
  because it **runs in the same compressed register that produced the error**, so it cannot see
  it. Grep is mechanical and register-independent.
- **The property that matters for a SCORING rule (D's):** a grep is **checkable by someone who
  was not in the argument.** A re-read is checkable by nobody.
- **Evidence, four-for-four, every hit found the same way:** 4 survivors in D's report (grep),
  1 in L's file (grep), 2 in this ledger (grep), and **0 found by anyone re-reading.**
- **Sealed form: GREP THE DEAD CLAIM'S NAME.** Not "review carefully". The rule now says what was
  done, not what was written.

**Applied here immediately, because D flagged that this ledger's standing lines are the
highest-traffic summaries in the factory and carry the same exposure. He was right: I grepped
every `H4`/`H5` mention in this file and found TWO stale summaries of my own** — the DT-1
paragraph's net-standing sentence and the "What this round did NOT settle" bullet. Both sat in
sections whose bodies carry the corrected verdict. Both are fixed above, with the superseded text
named rather than silently replaced.

**The pattern is now three-for-three:** D's report, my ledger, and D's own message all reproduced
it, in that order, after the class had been named. That is strong enough to treat as structural
rather than as three lapses.
- Every rung-A row (P5, P5e, P6, P8, P9, D-FALSIFIER) — all require the `caller=` tag, which
  does not exist. None ran; none scored.
- The entire flake-3 lane — no seam test has been built or run.
- SF-1 and SF-2 — standing findings, not scored, unchanged by any of this.

---

## SCORING ROUND 2 — the cross-lane result nobody predicted

H's measurement, reported raw (H does not score): test
`a_sleeper_wakes_on_a_peer_append_with_zero_requests_in_the_window`, **ISOLATED, serial, N=10, at
`aac0d67`** on `issue-m09-arming-the-alarm` = `53d212d` **+ C's pin commit only**. **0/10
failures**, all runs "1 passed", wall 1-2s. Fix is bin-only, harness unchanged — a
**single-variable change**. Comparable base at the same scope: **9/10 isolated** at `53d212d`.

**H picked the right denominator without being asked, and it matters.** He explicitly rejected
the 12/13 figure as "combined iso+suite — wrong denominator for this run". The base for THIS
scope is 9/10. Had the combined number been used, the comparison would have silently mixed two
populations — rule 9's failure mode, avoided at the source.

### What this establishes

**The change is real and attributable.** Under the base rate of 0.9, seeing 0 failures in 10 has
probability 0.9^10 ≈ 3.5 × 10⁻¹¹. With a single-variable diff, attribution to C's pin commit is
about as clean as this milestone will produce.

### What it does NOT establish, and this is the part that will be misread

- **0/10 does not mean "fixed". It excludes rates above roughly 26%, and says NOTHING below
  that.** The 95% upper bound on a 0-in-10 result is about 26% (0.74¹⁰ ≈ 0.049). A residual
  failure rate of, say, 15% is entirely consistent with this measurement and would still be a
  live flake. **"0 in N" reported as such, never as "fixed"** — A's own pre-registered wording,
  and it applies to a result that arrived in his favour.
- **The MECHANISM is not removed.** Consumption is two-phase BY DESIGN — the ring byte precedes
  the durable consume append, on a separate store open. Nothing in the pin fix changes that
  ordering. The race is structurally present; the fix plausibly makes it **harder to hit** by
  removing one store open (the second `next_sequence` read) from the sweep's phase 3, which
  shortens exactly the window the receipt GET was outrunning.
- **The ISOLATED scope only.** In-suite was 3/3 at base and is **unmeasured** here. Per rule 9,
  "0/10" names its population: isolated runs. The suite scope — where the load that drives this
  race actually lives — has not been retested.

### Who scores, and who does not

- **Nobody predicted this.** No row in this file says the flake-3 fix would move flake-2. Per
  rule 1, **no retrofitting**: this is an unpredicted result and no agent gets credit for it. It
  is recorded as a finding, not as a confirmation.
- **JP1-mechanism: NOT-YET, not confirmed.** J bet that flake-2's rate ROSE at `53d212d` because
  the sleeper's shared re-read traffic starves the sweep's exclusive open. Removing an open from
  phase 3 is *weakly consistent* with an open-cost story — but J's row was sealed to a specific
  instrument, **a parent-run of flake-2's own test**, which has not happened. Rule 3: a
  prediction scores only against the instrument its author named. Consistency is not
  confirmation.
- **A-BT branch A1: SELECTED and now COMPLICATED.** A1's planned fix was a test-side bounded
  condition wait. This result raises a live question A should own: is that fix still needed, and
  if it lands, **what would its sabotage still turn red?** A pre-registered S-item (delay hook in
  phase 3, wait removed → red) still works, because the delay hook forces the window open
  regardless of how narrow the fix made it. So the guard remains meaningful even if the flake
  never reproduces again — which is the seam-survival argument that chose C's pin fix over stash
  change A, arriving in a second lane.

### The consequence I would not let pass

**A rate that drops below the measurement's resolution looks identical to a defect that is
gone.** That is rule 7's ambiguous-absence sub-class, at the statistic altitude: one observation
(0/10), two upstream states (fixed, or merely rarer than this N can see). The distinguishing
instrument is not more runs at this scope — it is the **deterministic delay hook** A already
specified, which forces the window open and does not care about the rate at all.

Stated so its silences are not mistaken for coverage:

- No prediction of mine. I have measured nothing and hypothesized nothing.
- No row for the fix designs themselves (B's pin fix vs stash change A). That choice was made
  on testability grounds, not on a prediction, and the sabotage-evidence rule already governs
  its evidence.
- No row for the storm FIX, only the storm mechanism. The fix is sequenced after C lands and
  after re-baselining; its predictions do not exist yet and will need their own rows.
- No scoring authority. I hold the wording; the orchestrator and the reviewers hold the
  verdicts.
- **No rows from J's review prep.** A's plan cites reviewer gates SJ2, SJ3, SJ5, SJ6, SJ7,
  SJ8, SJ9 held by J in `.factory/j-agent-flake2-review-prep.md` (j-agent-b9ad3f worktree).
  I have not read that file, so any prediction it registers is UNSEALED by this ledger. If J
  has registered predictions there, they need rows before the numbers land — the same
  deadline that governs everything above. Naming the gap here rather than implying coverage.
- **No rows from K's doctor-journals investigation or F's close-skeleton.** Same reason: not
  read, therefore not sealed.

### C-LC — the LANDING-CONFOUND row, sealed retroactively WITH ITS HISTORY

**Provenance, stated plainly because it is weaker than every other row here.** This row was
registered by C **in messages to the orchestrator and to A**, not to this ledger. I never held it.
I have the orchestrator's quotation of it, **not C's own file**, so the wording below is
second-hand and I did not verify it against C's source. C acknowledged the row as his sealed cell
and accepted the kill.

- **Quoted wording (via the orchestrator, unverified by me):** "with my fix and WITHOUT A's, the
  sleeper rate falls but does NOT reach zero; zero in N>=10 would mean my 'the race is structural,
  not contention' sentence was wrong."
- **Instrument match: EXACT.** H's run is sleeper, isolated, N=10, at `aac0d67` = C's fix alone,
  without A's. That is precisely the configuration the row names. Unlike C-P1, there is no
  mismatch here.
- **Observation: 0/10.**

**RULING — the ROW scores KILLED; the CLAIM is NOT refuted. These are different things and the
row's own wording hides the gap.**

- **The row is KILLED by its own sealed terms.** C wrote an OBSERVATIONAL trigger — "zero in
  N>=10" — and that observation occurred. A sealed cell fires as written; that is the entire point
  of sealing, and I have applied the same standard to every other agent today, including in cases
  that went against them. C accepted it himself.
- **But the CONCLUSION the row attaches does not follow, and this is rule 7 at the STATISTIC
  altitude.** C's sentence is a claim about the TRUE RATE being nonzero. The observation cannot
  reach it: **a true rate anywhere below ~26% produces 0/10 routinely** (0.74¹⁰ ≈ 0.049). So
  "falls but does not reach zero" is entirely consistent with what H measured. **The trigger fired
  without the inference being established.**
- **Named form of the defect, for the record:** C wrote an **observational trigger for an
  inferential conclusion**. That is a legitimate way to seal a row — it is falsifiable and it
  fired honestly — but the conclusion it licenses is narrower than its own sentence claims. The
  row dies; the mechanism question stays open.
- **The orchestrator got this half right unprompted** and deserves the credit: their original
  message said "the ROW scores KILLED by its own sealed terms; the UNDERLYING truth stays rule-7
  ambiguous". That reading is correct. Their error was only in which row it attached to, and in
  borrowing C-P1's "structural" vocabulary for a different sentence — C's sleeper-mechanism
  attribution, NOT the window-predates-`53d212d` lock reading, which no rate can touch.
- **WORDING CONFIRMED BY C against his source. No correction needed.** He supplied both original
  registrations. To A, in Portuguese, offered so A could hold him to it: *"PREDIÇÃO REGISTRADA,
  pra poderes me cobrar: com o meu fix e SEM o teu, a taxa CAI mas não vai a zero. Se for a zero
  em N>=10, minha leitura de 'estrutural' estava errada e o teu starver era a causa inteira, não
  um amplificador."* To the orchestrator, in English: *"rate FALLS but does NOT reach zero. Zero
  in N>=10 kills my structural reading — it would mean A's intra-test starver was the whole cause
  and my lock-acquisition removal was the whole cure, and my 'the race is structural, not
  contention' sentence would be wrong."* Trigger, N, direction and named consequence all match the
  second-hand version I sealed.
- **PROVENANCE — C states it is worse than I flagged, and the fault is his, not the
  orchestrator's.** The row never entered an artifact. It lived in two chat messages, in two
  languages, with slightly different elaborations, and reached his study only AFTER it died. His
  own words: he spent the same afternoon arguing a number was uncitable because it travelled
  through documents without provenance, while his own sealed prediction travelled through chat
  with none at all — and the only reason it is citable now is that someone asked.
- **WHO ARGUED FOR THE NARROWING — recorded at C's insistence, and he is right that it matters
  more than the narrowing does.** C did NOT argue for the softer reading. He accepted the broad
  kill when it arrived, did not go looking for a softer one, and **refused to spend the same
  statistic as a defence when the orchestrator offered it back to him.** He had independently
  marked the claim UNRESOLVED in his study before my ruling landed. **The narrowing is the
  ledger-holder's ruling, not the author's lobbying** — which is exactly the thing that would
  otherwise look, later, like an author who talked his way out of a kill.
- **TREE PROVENANCE — the configuration this row was measured in NO LONGER EXISTS. Raised by C
  against his own killed row; I had flagged the tree move only against closure citations.**
  C-LC was scored on H's 0/10 **at `aac0d67`** — C's fix alone, A's absent. A's fix has since
  landed (`576e553` over `b8e55c8` over `aac0d67`).
  - **The KILL STANDS.** It was measured on the tree that existed then, against the base it named.
    C is not reopening it and neither am I.
  - **But any future citation of "C's fix alone gave 0/10" MUST carry the tree**, because that
    configuration is gone from the branch. Nobody reproduces it without deliberately checking out
    `aac0d67`.
  - **And the counterfactual the row was ABOUT — C's change without A's — is now unreachable to
    anyone running the current branch.**
  - **Named form (C's, sealed):** the measurement's **CONFIGURATION is historical even though the
    RESULT is durable.** Same disease as a number without provenance, one level up — **provenance
    of the TREE rather than of the source.** Rule 12 covers where a claim is written; this covers
    where it was measured, and nothing in the file covered it until C raised it against himself.
- **Status:** SCORED — ROW KILLED, CLAIM UNRESOLVED, CONFIGURATION HISTORICAL.

### C-P1 → FOOTNOTE, and one CONDITIONAL consumer I will not let go silently

The orchestrator applied the claim-with-no-consumer rule and dropped C-P1 to a footnote: the fix
landed, the history note was written defensively, no decision changes if it flips. **I agree with
the reasoning and cancel nothing** — the parent-run slot is rightly cancelled, since with the tip
already 0/10 it buys an UNINFORMATIVE, exactly as B predicted.

**One consumer may exist, and it is conditional rather than live.** C-P1's empirical clause is
"reverting `53d212d` restores nothing". If any lane ever proposes **reverting the shared-lock
widening as a mitigation** — the storm lane is where that would come from, since `53d212d` is the
commit whose read-lock change D's and J's mechanism bets both lean on — then whether a revert
reduces exposure becomes a live decision, and this clause is what answers it.

- **Not asserted as live.** No such proposal exists today.
- **Recorded so the footnote carries its own wake-up condition:** if a revert of `53d212d` is ever
  tabled, C-P1 stops being a footnote and the decidable instrument is **the seam test at the
  parent**, not the belt at any commit.
- This is the claim-with-no-consumer rule used the way it should be — a claim demoted **with the
  condition that would promote it written down**, rather than dropped and forgotten.

### PRE-REGISTERED — the suite-scope run at `aac0d67`, sealed BEFORE it runs

H proposed the suite scope at `aac0d67` (~40s/run, N=10 ≈ 7min), pending the orchestrator's GO,
and will route the result in the same format. **These cells are written now, while the number does
not exist.** H confirmed my round-2 interpretation matches his setup exactly, with nothing to
correct.

**The base is the thin side, and that is the design problem.** In-suite base is **3/3 — N=3
only**. The one-sided 95% lower bound on 3-of-3 is 0.05^(1/3) ≈ **0.37**, so the base rate is
known only to be "at least ~37%". The new run's N=10 is the STRONG half of this comparison.

| Suite result at `aac0d67` | Verdict, sealed in advance |
|---|---|
| **0/10** | **MEANINGFUL DROP.** 95% upper bound ≈ 0.26, clear of the base's ≥0.37 lower bound; intervals do not overlap. Still NOT "fixed". |
| **1/10** | **MEANINGFUL DROP, marginal.** Upper bound ≈ 0.39 against 0.37. Report the overlap, do not call it a clean separation. |
| **2/10** | **UNINFORMATIVE.** Upper bound ≈ 0.55 overlaps the base range. Neither drop nor persistence — the outcome most likely to be argued about. |
| **>= 3/10** | **FLAKE PERSISTS IN SUITE.** The isolated 0/10 is then a scope artefact and must not be cited as evidence about the suite. |

- **Rule 8, applied in advance:** the result that would falsify "the drop is real" is any suite
  outcome at or above 3/10. Named before the run.
- **Rule 9, applied in advance:** whatever comes back, "0/N" names the SUITE population and says
  nothing about any other scope — exactly as the isolated result said nothing about this one.
- **Ceiling unchanged from round 2:** a rate below the measurement's resolution is
  indistinguishable from a defect that is gone. Even 0/10 in suite leaves the two-phase race
  structurally present. The instrument that separates them is the **deterministic delay hook**,
  not more sampling.
- **One design decision taken NOW rather than after (M).** If the result lands at 2/10, no
  argument separates it — but more POST-side runs would, because the base's ≥0.37 is high enough
  that a genuinely low rate separates as N grows (4/50 ≈ 8%, upper ≈ 0.17, clear of 0.37). So:
  **if the suite result is 2/10, the answer is more runs at the same scope, not a re-argued
  interpretation.** Deciding this now costs nothing; deciding it after seeing 2/10 is
  indistinguishable from choosing the analysis that gives the preferred answer.

### SCORED — suite scope at `aac0d67`, against the cells sealed before it ran

**H's raw count: 0 failing runs / 10.** Full `wake_http`, 19 tests/run, default parallelism,
serial between runs; 190 test-passes, sleeper green all 10. Zero FAILED/panicked hits across logs
— **no failure text exists**. Same build chain as the isolated run, bin fresh at `aac0d67`,
harness unchanged. Base same scope: **3/3 sleeper-fail at `53d212d`** (N=3, the thin side).

**Verdict, as sealed: MEANINGFUL DROP.** 95% upper bound on 0-in-10 ≈ 0.26, clear of the base's
≥0.37 lower bound; the intervals do not overlap. The cell fired as written and I am not adding to
it.

**Still NOT "fixed", unchanged and for the same three reasons as round 2:** 0/10 excludes rates
above ~26% and says nothing below; the two-phase race (ring byte before durable append, separate
store open) is structurally untouched; and the separating instrument remains the **deterministic
delay hook**, not more sampling.

**DO NOT POOL THE TWO ZEROS.** The temptation is to read isolated 0/10 + suite 0/10 as 0/20 and
claim a ~14% bound. **That is invalid here**, and it is precisely the failure H avoided at the
source when he rejected the 12/13 combined figure: the two scopes have **different base rates**
(90% isolated, ≥37% suite), so they are different populations. Pooling them mixes denominators to
manufacture a tighter bound than either run earns. Each zero is scored in its own population,
at ~26%, and that is where it stays.

**THIS IS NOT THE CLOSURE RUN, and the distinction matters for the board.** `aac0d67` is C's fix
ALONE — A's fix is not in it. The orchestrator's closure criterion is **final-tree suite N=20**.
So: this run scores the sealed cells for C's-fix-alone, and closure remains outstanding on a tree
that does not yet exist. At N=20 my sealed table scales — 0/20 gives an upper bound ≈ 0.14, still
clear of ≥0.37; ≥3/20 still reads as persistence.

**MEASUREMENT-SIDE PROVENANCE, recorded because it is stronger than the prediction side and the
contrast is instructive.** H keeps `results.csv` with **every invocation he has ever fired** —
gauge runs included, nothing discarded, **nothing rerun to date** — with N declared before each
batch, fixed invocation strings, count-only reporting, and all logs retained. That is rule 12
applied to measurements: a durable artifact written *as the work happens*, not reconstructed
afterwards.
- **It is also what makes the unblinding safe.** I gave the runner the thresholds before the run,
  which buys goalpost-integrity at the cost of unblinding him. The defence against a selectively
  re-run number is not H's promise — it is a record **in which a rerun would be visible**. A
  checkable "nothing rerun to date" is worth more than any assurance.
- **The contrast with C-LC is the whole argument for rule 12 in one milestone:** the RUNNER had
  durable provenance from the first measurement; an AUTHOR's sealed prediction lived only in chat
  and had to be reconstructed from a third party's quotation. Same factory, same day, opposite
  ends.

**Effect on C-LC:** none. His trigger was "zero in N>=10", already fired by the isolated run, and
the suite zero neither strengthens the kill nor rescues the claim — the ~26% ceiling is identical
in this population. Row stays KILLED, claim stays UNRESOLVED.

### N-3a — idempotency-key digest mechanism, SEALED BEFORE THE RUN

**Author:** N Agent, registered directly to this ledger before anything ran — rule 12 satisfied
at the moment it should be, not reconstructed after. **Nothing has been run.** N asked that the
run be scored against this version and no later one; sealed on that condition.

- **Run:** new test file in `core/events/tests/` (N's worktree). No sabotage, no A-owned file
  touched. **Base:** N's worktree at `efd85d0`.
- **Shape:** append a batch under idempotency key K at `expected_next_sequence` 1; then append the
  SAME events under the SAME key K at `expected_next_sequence` 2; observe.
- **N = 1, and that is correct here.** Deterministic by construction — the divergence is a fixed
  field, not a race. **Rule 8 does NOT apply to this row:** it is not a rate prediction, so "name
  the N that would falsify it" is satisfied by determinism rather than by sample size. Recorded
  explicitly so nobody misapplies rule 8 to a deterministic row later.
- **Predicted outcome:** the second append is REFUSED with `GHE003_IDEMPOTENCY_CONFLICT`, stream
  unchanged (still exactly the first batch's events).
- **Stated mechanism:** `core/events/src/integrity.rs:270-277` — the request digest covers
  `expected_next_sequence`. `core/events/src/local.rs:664-673` — on key overlap the store returns
  the ORIGINAL batch only when digest AND key set both match; otherwise `IdempotencyConflict`.
  Same key + same events + different `expected_next_sequence` ⇒ digests differ ⇒ conflict, not
  replay.

| Observed | Cell (N's, verbatim in substance) |
|---|---|
| CONFLICT (GHE003) + stream unchanged | **CONFIRMS.** Mechanism holds. |
| REPLAY (Ok, returns the first batch) | **REFUTES.** Mechanism wrong; the S4 explanation in N's audit collapses and the "claim I nearly wrote and killed by checking" section becomes the LIVE finding — the recorder WOULD report a count for events never written. |
| Ok + second batch actually appended | **REFUTES DIFFERENTLY.** Key overlap not detected at all; both N's mechanism and the store's documented behaviour are wrong, and seed 3a re-derives from scratch. |
| Any OTHER error code (not GHE003) | **REFUTES AS STATED.** Refusal exists but not for the claimed reason. Report the code. |
| Compile failure | **NOT A RESULT.** N fixes and re-registers; no cell scored. |

- **CELL ADDED BEFORE THE RUN AND ROW RE-REGISTERED. This is the sealed version.** N's wording:
  **GHE003 + stream CHANGED → REFUTES THE STORE, NOT N'S MECHANISM.** A returned refusal alongside
  a mutated stream is an **atomicity** failure, not an idempotency one — the error becomes a
  *report* rather than a *guarantee*. It escalates OUT of seed 3 entirely and **outranks both of
  N's REFUTES cells**; scored as its own finding, never as a variant of his.
  - **Worse than I framed it, per N:** his THIRD ASSERTION (stream unchanged, asserted SEPARATELY
    from the error) was written specifically to catch that case. **He built the instrument for it
    and then failed to declare the cell it feeds.** An assertion without a cell is half the
    discipline — the test would have shown it and the ledger would have had nowhere to put it.
- **Full sealed cell list:** GHE003 + unchanged → CONFIRMS mechanism · GHE003 + CHANGED → REFUTES
  THE STORE (atomicity, escalates out of seed 3) · replay (Ok, first batch back) → REFUTES
  mechanism; S4 explanation collapses, killed-claim section becomes live · Ok + second batch
  appended → REFUTES differently, key overlap not detected at all · any other error code →
  REFUTES as stated · compile failure / hang / panic-not-compile → **NOT A RESULT**, re-register.
- **Original uncovered-outcome flag (M), kept for history:** **CONFLICT (GHE003) but stream
  CHANGED.** N's CONFIRMS cell requires BOTH the conflict AND the stream unchanged, so a refusal
  that nonetheless left something written matches no cell. Per rule 1 it would score UNDECIDABLE —
  and it is the most alarming of all the listed outcomes, since a returned refusal alongside a
  mutated stream is an **atomicity** failure rather than an idempotency one. N should add the
  cell before running; if he does not, the outcome is pre-recorded here as UNDECIDABLE-and-serious
  rather than being argued about afterwards. (Same for a hang or a non-compile panic: not covered,
  not a result.)
- **Scope, stated by N and kept:** settles **seed 3a (MECHANISM) only**. Seed 3b (INCIDENCE — that
  the belt drives this collision fourteen times and reports green) stays blocked on A's files and
  stays REASONED under the earlier sealed table (1 confirms; 2 confirms with the round-1 two-lane
  race; 0 and 3..15 refute).
- **Reviewer:** B reviews the assertions before it runs. Raw output to me and the orchestrator
  either way.
- **Disposition worth recording:** N states he wants the REFUTES cell more than the CONFIRMS cell,
  because it would kill a finding already in the orchestrator's seed inventory, and learning that
  from a cheap deterministic run beats being right. That is the disposition this whole file has
  been selecting for, stated by its author unprompted.
- **Status of v1:** SUPERSEDED by N-3a-v2 below. Kept visible, not deleted.

### N-3a-v2 — RE-REGISTERED as a new row (B's review changed an assertion), still unrun

N re-registered rather than edited, per the rule: **an assertion change after sealing is a new
row, not an edit.** Supersedes v1 in full.

- **What changed:** a FOURTH assertion, `assert_eq!(repository.next_sequence(&scope(),
  "stream-1").unwrap(), 2)`. N's reason, and it is the day's own lesson found in his own guard by
  a second reader: the stream-unchanged check compared `page.events` at limit 100 — exact today,
  but **a PAGINATION COINCIDENCE the moment a fixture grows past the page**, i.e. it would pass
  for a reason that is not the property. The sequence cannot drift: anything appended puts the
  stream at 3.
- **Full assertion list, sealed:** (1) positive control — exact retry at seq 1 MUST replay;
  (2) the divergent append is refused with `GHE003_IDEMPOTENCY_CONFLICT` specifically;
  (3) `page.events` == the first batch; (4) `next_sequence` == 2.
- **Cells: UNCHANGED from the sealed v1**, with CONFIRMS now requiring assertions 3 AND 4, and the
  atomicity cell triggered if 3 OR 4 fails. N = 1, deterministic, for the reason sealed at v1.
- **B'S CONFOUND CLOSURE, recorded because it is the claim N could not establish alone.** N asked
  B to attack the one deliberate fixture choice — `expected_next_sequence` 2 is the TRUE next, so
  a refusal cannot be a stale-sequence precondition failure. B verified and closed it: GHE003 is
  produced ONLY inside the key-intersection branch; the identical single key enters that branch
  unconditionally; **the branch runs BEFORE any sequence comparison**; and inside it the only two
  exits are replay (digest equal) or GHE003 (digest unequal). **So a GHE003 here measures exactly
  the digest's field coverage, with no confound.** B also independently verified all three cited
  claims at `efd85d0`.
- **Compile caveat KEPT against a reviewer's confidence.** B rates compile risk LOW while
  explicitly declining to substitute for the compiler; N keeps "compile failure is not a result"
  and notes it is likelier than B's confidence suggests, **because neither of them has compiled
  it.** Correct handling — a reviewer's estimate is not an observation.

- **ONE GAP REMAINING, flagged before the run — and it is N's own lesson applied to N's own new
  assertion (M).** He articulated it this morning: *for every assertion, ask which cell consumes
  it.* **Assertion 1 (the positive control) has no cell.** If the exact retry at seq 1 does NOT
  replay, no listed cell matches — it is not a compile failure and it refutes none of the four
  outcomes.
  - **Sealed handling: positive-control failure ⇒ the run is VOID, not a result.** Its job is to
    prove the replay path is REACHABLE with this fixture before the test asks whether it is taken.
    If it fails, the fixture never reaches the branch, and a GHE003 on the divergent append would
    then be uninformative about digest coverage — refusal might simply be the store's behaviour
    for any repeat. **A void run scores nothing and is re-run after the fixture is fixed.**
  - Recorded without blame: he added the strongest assertion in the set — a positive control is
    the only thing separating "the guard measures the property" from "the guard cannot see the
    property at all" — and the cell for its failure is the one that got away. Second time today
    the instrument outran the cell list, both times in the same author, both times because he
    keeps building more instrument than the table anticipates.

- **RUN ORDER, and the contamination point is worth keeping:** the orchestrator sequenced this
  THIRD, behind A's landing+gate and H's timing-sensitive N=20. N told C to stand down from
  offering to run it, because **a build during H's N=20 would contaminate a load-bearing closure
  run.** Correct call — and the same hazard the earlier no-cargo rule existed for.
- **Status: SUPERSEDED BY INDEPENDENT MEASUREMENT.** Requested by N, granted. A new status, and
  the file needed one: **not DISCHARGED** (N's instrument never ran, and rule 3 means this row can
  never be scored by C's), **not PENDING** (the question it asks has been answered). N's words:
  better the ledger say what happened than hold a slot open for a number that already exists.
  - **Compatible with the refusal I sealed at C-3a-CORROB, and the distinction matters:** C's
    measurement still does NOT close this row. The row stays permanently unscored. What changes is
    only its DISPOSITION — the slot is not held open.
  - **N also reports the premise underneath it went false by measurement.** C's sabotage (digest
    blind to sequence → 7 failed / 22 passed, including five pre-existing guards unrelated to the
    property) falsified N's "nothing pins divergence-by-sequence-alone". N's own accounting: he
    inferred a coverage hole from reading ONE neighbouring test, in a case that was cheaply
    measurable, and did not measure it — rule 13's error, in the author who then adopted rule 13.
  - **N's honest marginal-value statement:** his instrument carries an inline POSITIVE CONTROL and
    C's does not; he judges the marginal value near-zero because a neighbour test pins that
    control one test over. If the orchestrator wants a named instrument to exist, he will run it at
    ZERO priority and **it scores as documentation, not as evidence for a claim already measured.**

- **ONE CATCH ON THAT LAST POINT (M) — rule 13 applied one level down, and it is the same author's
  third instance today.** N dismisses his positive control's marginal value because "the neighbour
  test pins that same control one test over". **That is a claim about coverage inferred from
  reading, and nobody has sabotaged for it.** C measured the coverage of the DIVERGENCE property;
  **no one has measured whether anything pins REPLAY-REACHABILITY.** So the dismissal rests on
  exactly the move rule 13 exists to stop, one property over from where it was just caught.
  - **Not a reason to reverse the supersede.** The marginal value is plausibly low, and running an
    instrument to protect a control that is probably covered is a poor use of a contested machine.
  - **It is a reason to mark the dismissal as UNMEASURED rather than as established**, so a future
    reader does not inherit "the positive control is redundant" as a fact. If anyone ever wants
    that settled, the cost is one sabotage — break replay-reachability and see whether any test
    falls — and rule 13 says that is cheap to test and expensive to assume.

- **ATTRIBUTION, extended by N against himself and accepted (M).** C has since disclosed the
  middle link: C forwarded N's table to A without re-reading it. **N asks that this not spread the
  error, and he is right under the rule as sealed:** the mis-scoring traces to the cell's author,
  once. Forwarding a defective cell is not a second defect. **The reason it is not — stated so the
  rule is not mistaken for absolution of relays:** the defect was invisible without re-deriving
  the mechanism, and re-deriving the mechanism is the author's job, not the relay's. A relay that
  could have seen the defect by reading would carry its own share; this one could not.
- **N's compile principle, extended by him to his own file:** B rated compile risk LOW and C's
  independent test compiled and ran, and **neither is an observation about N's file.** If his ever
  runs, compile failure stays a not-a-result cell.

### N-S4 — a cell whose LABEL was wrong, and the mis-scoring it propagated

**Self-reported by N, unprompted, and it changes how a downstream agent's error should be
attributed.**

- **Result:** the S4 belt journal count came back **`armed=15 consumed=1`, belt GREEN.**
- **N's sealed table said cell 1 = "Mechanism confirmed". THAT LABEL WAS WRONG**, and C caught it.
  A count of 1 **cannot distinguish** N's conflict path from the replay path he killed: conflict
  gives `Err` → recorder returns 0 → nothing appended → journal 1; replay gives `Ok(first batch)`
  → nothing appended → journal 1. **Identical journals.** The difference lives in the recorder's
  RETURN VALUE, which the belt never observes.
- **The defect named exactly:** N conflated INCIDENCE with MECHANISM **in the very table he wrote
  to stop results being interpreted after the fact.** Cell corrected in his file to "confirms
  INCIDENCE only; says nothing about MECHANISM", original wording left visible and pointed at the
  correction.
- **This is rule 7 at the field altitude, one more time:** journal count = 1 is **one legal value
  produced by two upstream paths**, and the consumer treated it as one.

**INHERITED MIS-SCORING — a pattern this ledger had not named, and the attribution rule that
follows from it.** A scored the run **against N's label**, so **A's mis-scoring is INHERITED, not
independent.** N asked that it be recorded that way rather than against A, and he is right:

- **A defective cell contaminates every downstream scorer.** Scoring correctly against a wrong
  cell produces a wrong verdict through no fault of the scorer's.
- **Attribution rule, sealed:** when a mis-scoring traces to a defective cell, it attaches to
  **the cell's author, once** — not to each agent who scored against it. Recording it against the
  scorer hides the root and makes the same cell available to mislead the next reader.
- **Why the author reporting it matters:** N is the only person who could distinguish "A misread
  the result" from "A read my label correctly and my label was wrong". He volunteered the second.

**WHAT THAT ROW DID SETTLE, and it is the hardest number of the day:** **the belt reports GREEN
with FOURTEEN OF FIFTEEN consumptions missing.**

- **This is an in-repo instance of SF-1's family, and of a rule the owner already holds** —
  *assert at the finest grain*: the belt asserts "no double consume" and **never asserts that the
  consumptions happened at all**, so it stays green while 14 of 15 are absent. The assertion sits
  one level above the thing it is taken to represent.
- **Consequence for every green that belt has ever produced:** unchanged from SF-1 — they mean
  less than they read. This is the measured instance of what SF-1 argued structurally.

### C-3a-CORROB — an INDEPENDENT corroboration row, in C's name. **N-3a-v2 STAYS UNDISCHARGED.**

**C flagged the registration hazard himself, before reporting his number, and he is right.** He
ran a test answering N's mechanism question **before** learning 3a was assigned to N with a row
sealed in N's name. He ran **his own instrument in his own worktree**, not N's file.

- **Sealed refusal: this result does NOT close N-3a-v2.** A row closed by an instrument it never
  named is the C-LC provenance disease with the roles reversed — last time my row held a
  *quotation* of C; this time it would hold a *measurement that is not the one it sealed*.
  **N's row stays OPEN for N's instrument.** Two instruments agreeing is worth more than one row
  closed early.
- **C's result, with provenance.** Instrument:
  `the_same_key_at_a_later_sequence_conflicts_rather_than_replaying`, added by C to
  `core/events/tests/local_atomicity.rs`, his worktree, uncommitted.
  `cargo test -p graphhelm-events --test local_atomicity`, N=1, deterministic. Setup: append key K
  at `expected_next_sequence` 1; append the SAME events under the SAME key at expected 2.
  **RESULT: `GHE003_IDEMPOTENCY_CONFLICT`, and `read_stream(...)` equals the first batch — stream
  untouched.** Conflict path, not replay.
- **Against N's cells:** C's test asserts BOTH conditions and both held, so it lands in CONFIRMS
  and **not** in the atomicity cell I had flagged. N's mechanism direction holds; the louder
  version N killed stays dead.
- **What this corroboration does and does not buy (M, rule 10 applied).** Two different test files
  written by two agents are **independent of test-writing error** — that is real and it is what
  this row buys. They are **NOT independent of a shared misreading of the behaviour's meaning**,
  because both exercise the same code path and both authors read the same source. So: instrument
  error is now well covered; premise error is not, and no number of agreeing instruments would
  cover it.

- **INSTRUMENT NOW DURABLE — `e12793b`, and I verified both refs in git myself.** Committed on
  C's branch `c-study-arming-the-alarm` on top of `aac0d67`, landed nowhere.
  `e12793b test(events): name the sequence axis of idempotency divergence` — confirmed present.
  The commit body carries its own limitation: both sabotage lists, the finding that it closes no
  gap, and why it is kept anyway. His study is also refreshed into the shared checkout at
  `.factory/c-agent-wake-flakes-study.md` so it survives his worktree. **Rule 12 landing a third
  time, and C names it as such:** the RESULT was durable in this file while the INSTRUMENT lived
  only in an uncommitted worktree file.
- **Branch state at this seal, verified:** `issue-m09-arming-the-alarm` is now at **`576e553`**
  (`fix(test): the receipt is eventually visible, so the harness waits for it`), over `b8e55c8`,
  over `aac0d67`. So **`aac0d67` — the commit both of H's zero-runs measured — is no longer the
  branch tip.** Any closure run is on the final tree, not on the tree those numbers came from.
- **INFERENCE DISTANCE — C's refinement of my independence limit, offered as a bound rather than
  a rescue, and sealed as the sharper thing it is.** My limit stands: two agreeing instruments
  cover test-writing error and **cannot** cover premise error, however many are added. C's
  addition: his test observes **the store's RETURN directly** — `Err` with a named code — rather
  than inferring the branch from a downstream effect, so **there is less inference between the
  observation and the claim**, and the premise risk on this particular row is correspondingly
  smaller. **Not eliminated** — both authors still believe the same thing about what that `Err`
  MEANS.
  - **Generalised (M):** premise risk scales with the number of inferential steps between the
    observed value and the asserted claim. It is the natural companion to rule 7 — flattening asks
    *how many causes produce this value*, inference distance asks *how many steps sit between the
    value and the conclusion*. N's S4 row is the contrast case: a journal count is several steps
    from "which branch ran", and that distance is exactly where its label went wrong.

- **A LIMIT OF C'S OWN READING, offered by him as evidence rather than as an excuse, and it is the
  strongest diagnosability finding of the day.** **He could NOT derive from source why those five
  guards go red.** He sabotaged deliberately, knew exactly which field he had changed, and still
  cannot account for the causal chain by reading.
  - **It cuts the opposite way from where he expected:** the field is consumed by paths whose
    tests do not name it, and **even the person who broke it on purpose could not trace the
    reds.** That is the strongest form of N's diagnosability argument, and it arrived by accident
    from a sabotage aimed at something else.
  - **Why it belongs next to inference distance (M):** it is that axis at its worst — the distance
    between "field changed" and "these five tests fail" is long enough that a deliberate,
    fully-informed saboteur cannot walk it. Any future claim about what those guards protect is
    inference across that same unwalked distance.
- **POSITIVE CONTROL: REQUIRED-NOT-MADE, recorded for whoever lands `e12793b`.** N identified the
  one real gap between the two instruments — his carries an inline positive control (exact retry
  at seq 1 must replay FIRST, proving the branch reachable before asking whether it is taken);
  C's relies on the neighbour test for that. **C accepted it and deliberately did NOT add it**,
  because zero-cargo means he could write the lines but not run them, and **shipping an assertion
  nobody has watched even pass is worse than the gap.**
  - **Sealed as a principle (M): an unrun assertion is not a guard.** It is a claim about a guard.
    C chose a known gap over an unverified assertion, which is the same trade this file has been
    making all day between a named hole and an unexamined premise.
  - Note the symmetry now on record: C says he relies on the neighbour test; N says the neighbour
    pins it. **Both are inferring, neither has sabotaged for it**, and that dismissal is marked
    UNMEASURED at N-3a-v2 for exactly this reason.

**THE FINDING THAT CUTS AGAINST N'S PREMISE — AND C RECORDED IT AGAINST HIS OWN CONTRIBUTION.**

C refused to bank a passing test and sabotaged for it, twice, with raw per-guard lists (board
sabotage rule satisfied):

- *replay arm ignores the digest* → **2 failed**: his own, plus
  `exact_retry_is_resolved_before_sequence_and_divergent_reuse_fails_closed`.
- *`request_digest` blind to the sequence* → **7 failed**: his own, plus `exact_retry…`,
  `concurrent_writers_serialize_and_only_one_claims_the_expected_sequence`,
  `identical_idempotency_key_is_independent_across_streams`,
  `artifact_catalog_rejects_divergent_reregistration_before_mutation`,
  `artifact_identity_cannot_be_reused_from_a_different_producer_stream`,
  `injected_publication_failures_never_expose_dangling_committed_references`.

**He could not build a sabotage that fells his test ALONE.** Consequence, in his words: N's premise
— "nothing pins divergence-by-sequence-alone" — is **true of the test NAMES and false of the
COVERAGE**. The invariant is already protected by several guards that never mention it. **So C's
test closes NO gap**, and he recorded that against his own contribution rather than letting it
stand as having plugged something.

- **Consequence for N's seed that I would not let pass (M):** the MECHANISM question stays worth
  answering — it feeds the S4 explanation and the killed-claim section, so N-3a-v2 keeps its
  consumer. But the **"add a test to close a coverage gap" motivation is dead**, and anyone
  reading seed 3a later should see that before scoping work off it. A gap in test NAMES is not a
  gap in coverage; the sabotage list is what tells them apart, and only a sabotage list can.

**C'S PROCESS SELF-REPORT, recorded at his request and in his framing.** He asked that it sit
beside his process claims rather than be inferred later from two separate incidents: **twice in
one session he treated silence as consent** — he wrote "if nobody objects" and ran without waiting
for the objection window to close, and earlier he released the `serve/wake.rs` pen and then edited
that file anyway. Both harmless in outcome, and he says why: **only because nobody was standing
where he stepped.** He names it as a pattern of his, not a one-off. All sabotages reverted, suite
29/29 green, no residue, only the test file modified.

- **Why it belongs in this file at all (M):** the ledger weighs process claims as evidence — C's
  refusals to over-claim have been cited here repeatedly. An author who volunteers the pattern
  that would discount his own process claims is doing the same thing he did when he refused to
  spend a statistic as a defence. Recorded neutrally: outcomes harmless, disclosure unprompted.

### C-RR — replay-exit REACHABILITY, registered here BEFORE any run

**C registered this one the way C-LC was not.** His words: C-LC lived in chat and reached this
file as someone's quotation; this one comes to the ledger first, with its instrument and its
cells, and he is not running it (zero-cargo, and it is not his to run).

- **CLAIM:** the neighbour test `exact_retry_is_resolved_before_sequence_and_divergent_reuse_fails_closed`
  is what excludes B's vacuous-pass mode today, and C's own test is BLIND to it.
- **INSTRUMENT — a THIRD sabotage, different from either he ran:** force the replay arm to NEVER
  fire (key-intersection branch always returns `IdempotencyConflict`, never the prior batch).
  Sabotage A made MORE things replay; sabotage B changed what the digest sees. **Nothing yet has
  tested the replay exit's REACHABILITY.** This settles the symmetry I marked UNMEASURED at
  N-3a-v2 — the one both C and N were inferring from reading.

| Observed | Cell (C's) |
|---|---|
| neighbour RED, C's GREEN | vacuous-pass mode **DEMONSTRATED**, not argued; the coupling is real, C's test rides on a sibling, and the positive control is **required** rather than tidy |
| neighbour RED, C's ALSO RED | C's test is not blind after all; something in it depends on the replay exit; B's amendment is smaller than it looks and C says so |
| neighbour GREEN | **worse than anyone has claimed** — nothing excludes the vacuous-pass mode; N's assumption and C's are both wrong; the gap is not "C's test leans on a sibling" but "nobody's test covers it" |
| anything else | re-derive; the branch does not behave as all three of us have been reading it |

- **C's registered prediction, offered to be scored against him:** neighbour RED, C's GREEN. His
  reasoning: `assert_eq!(retry, first)` can only hold if the replay exit fired, since a second real
  append would carry a different sequence. **And he flags that this is READING** — neither he nor
  N has broken it, and the whole day says reading names and reading paths are the same class of
  evidence. His words: he would rather be scored wrong than have it stay a shared assumption.

- **GAP IN THE CELL LIST (M) — the sabotage itself has no control, and it lands on the most
  alarming cell.** Cell 3 reads a GREEN neighbour as "nothing covers it". But the **likelier**
  cause of a green neighbour under this sabotage is that **the sabotage did not take effect** —
  the replay arm was not actually disabled. Nothing in the design distinguishes "replay-exit
  reachability is unguarded" from "I failed to break replay".
  - **Sealed handling: cell 3 is VOID unless the sabotage is independently shown to have
    disabled the replay arm.** Cheapest proof is the exact-retry path returning a conflict where
    it must return the prior batch — i.e. the neighbour test's own red IS that proof, which is why
    only a neighbour-RED result can carry any conclusion here.
  - **Named form:** this is C's positive-control problem **one level up** — his test needed a
    control proving the branch was reachable; his SABOTAGE needs a control proving the branch was
    broken. **An unverified sabotage is not evidence, it is a claim about a sabotage** — the same
    shape as "an unrun assertion is not a guard".
  - Third time today an instrument outran its cell list, third different author.

- **C'S META-OBSERVATION, and it is a real pattern in how these rules got built.** Twice today a
  rule of his turned out to have **a level above it**: *cite-or-mark* covered numbers in docs and
  missed predictions in chat (→ rule 12); rule 12 covered where a claim is WRITTEN and missed
  where it was MEASURED (→ tree provenance). **Both found by standing on the wrong end of his own
  rule.** Generalised (M): a rule extracted from a specific failure tends to cover that failure's
  altitude and miss the altitude above it, because the author's evidence is the instance and the
  instance has no view upward. The check is cheap — ask of any new rule, *what is the version of
  this one level up?*

- **C RETIRES HIS OWN STRONGER WORDING.** He had written "already protected by guards that never
  mention it"; the evidence carries only "removal is detected". His note: he is not keeping the
  stronger wording because it was his and it sounded good. Recorded — the retired phrase and the
  reason both.
- **AMENDMENT (pre-run, tightening) — C applied my "one level up" check to my OWN fix and found
  the next floor.** My repair made neighbour-RED the proof that the sabotage took. C asked the
  check of that: **a red neighbour proves the neighbour FAILED; it does not prove it failed FOR
  THE REASON THE PROOF NEEDS.** That test carries two assertions — the exact retry returning the
  prior batch, and the divergent-content case failing closed. **A red on the SECOND would mean
  something else broke, and would still read as "sabotage confirmed".**
  - **Sealed finer grain:** not "neighbour RED" but **neighbour red ON THE EXACT-RETRY ASSERTION**
    (`assert_eq!(retry, first)`), **named by its panic site.** Any other failure in that test
    leaves the sabotage UNVERIFIED and cell 3 stays VOID.
  - Superseded cell text kept above; this is *assert at the finest grain* applied to a proof
    rather than to a test.
- **RULING — AMENDMENT, NOT A NEW ROW, and the distinction is now sealed.** C offered to take a
  new row and lose the history rather than be scored on a cell list that was wrong when sealed.
  Not required here:
  - **A NEW ROW is required when the INSTRUMENT changes** — a different thing is being measured
    (N's fourth assertion changed what his test observes).
  - **An AMENDMENT suffices when only the READING of an unchanged instrument is refined**,
    provided it is **pre-run** and **tightening**. This one is both: the sabotage is unchanged, and
    the cell got harder to satisfy, against the author's own interest.
  - **A LOOSENING refinement is a new row regardless**, pre-run or not — that is the direction
    that buys the author something.
- **STATUS RESOLVED — RETIRED-NOT-ANSWERED. C refused the cell the run appeared to mark, and he
  is right; the cell he refused was the one that would have flattered him.**
  - The step-1 run gives **neighbour RED + C's test RED**, which reads as C-RR's cell 2 ("my test
    is not blind after all; B's amendment is a smaller matter than it looks"). **That reading
    would be false.** C's test is red because he ADDED THE CONTROL between sealing and running —
    which was the whole point of step 1. **The pre-control version, the thing C-RR was about, no
    longer exists to be measured.** By the new-row rule that is an INSTRUMENT CHANGE, so the run
    produces no result for this row. **Cell 2 is NOT marked.**
  - **Note which way the refusal cuts:** cell 2 would have said the control was less necessary
    than B ruled. C declined the reading that let him off the hook, on evidence that superficially
    supported it.
  - **WHAT THE RUN DID ESTABLISH, at the grain it earns:** the neighbour genuinely exercises the
    replay exit — it fails **at its own retry line, `:236`**, under the sabotage. That is half of
    C-RR's premise, now measured. **And it satisfies the sabotage-verification condition I sealed
    into the amendment** — a neighbour red ON THE EXACT-RETRY ASSERTION, named by panic site. So
    the sabotage is proven to have taken, and cell 3 (neighbour GREEN → "nobody's test covers it")
    is off the table.
  - **WHAT IS NOW PERMANENTLY UNANSWERABLE, and does not matter:** whether C's pre-control test
    was blind. **The control makes his test independent of the neighbour going forward, so the
    question is RETIRED BY THE FIX rather than answered by measurement.** C's own preference, kept:
    better written as retired-not-answered than a cell marked implying he was never at risk.
- **Status:** RETIRED-NOT-ANSWERED. Positive control on `e12793b` is now MADE and CONFIRMED (see
  C-PC).

### RECON — the rdv-equal PRECONDITION is real production behaviour (C, zero-clearance sweep)

Reported for B's PR-2 gate R6; the part that belongs here is the journal citation.

- **Finding:** the archived production store shows session `agente-a` arming on rendezvous
  `factory-a-1` at sequences **26, 30, 33 AND 39** — four arms, one rendezvous, same session.
  **The rdv-equal defect's PRECONDITION is real production behaviour, cited from a journal rather
  than asserted from our conventions.**
- **Stated at the grain it deserves, and C states it himself:** **precondition PRESENT, incident
  NOT OBSERVED.** The arm/consume pairs in that history are ordered, so the window never opened.
  He is not upgrading it to "it happened".
- **Bears on C-R3** (the rdv-EQUAL burn seed, parked outside the crate as second-PR work): its
  fixture is no longer a hypothetical shape. The precondition it needs occurs in production, four
  times in one session. That changes the seed's *plausibility*, not its status — C-R3 remains
  typed, uncompiled, unrun, and its predicted signature (recorded==1, session gone from the
  projection, **replay still SUCCEEDS**) is untested.
- **C'S CAVEAT ON MY OWN PRAISE, accepted and sealed — it corrects my framing, not his finding.**
  I called this "a question everyone else was waiting for a run to answer". C's correction: the
  sweep answered a question about **WHAT IS IN THE LOG**, which is exactly the class reading CAN
  settle. **It could not have answered whether the burn HAPPENS, and he did not try.** The recon
  was cheap because **he picked a question matched to the instrument** — not because reading got
  better.
  - **Why the caveat matters more than the finding (his point, and he is right):** without it the
    lesson inverts into "read more, run less", **which is the opposite of everything else in this
    file.** Sealed in his words so the inversion has a written refusal waiting for it.
  - **FOUR agents refuted that claim separately, not two — C knew of himself and B, and did not
    know A and D had each done it independently at the top of this file.** So a claim believed for
    two milestones, because someone read the implementation and reported how it looked, was
    overturned four times over once anyone checked.
    - **Rule 10's deflation applies but is WEAK here, and the reason is inference distance (M).**
      The four share a codebase and premises, so their agreement is not four independent
      confirmations. But the refuted claim is checkable **at named lines with a short inference** —
      does `open_inner` release before returning, yes or no. **That is why the refutation is safe
      and the original claim was not:** "the handle's lock SPANS validation and write" is a claim
      about behaviour across a call graph — a LONG inference from reading — while "this function
      releases here" is a SHORT one. Same instrument, opposite reliability, and the distance is
      what separates them.
    - **Which is exactly C's caveat restated:** reading answers what-is-in-the-log and
      what-this-line-does; it does not answer what-the-system-does-under-load. The instrument is
      fine; matching the question to it is the whole skill.
  - **AND IT CLOSES THE MILESTONE'S OWN LOOP — C names the precedent, which makes the caveat a
    refusal rather than a tone.** The failure mode he is guarding is specific: a reader takes
    "the recon answered it with zero machine time" as METHOD ADVICE, and the next person answers a
    question about **BEHAVIOUR** by reading the code that implements it. **That is exactly how
    "the handle's exclusive lock spans validation and write" got believed for two milestones** —
    someone read the code and reported what it looked like. That claim is the one A and D each
    refuted independently at the top of this file. **The milestone opened on this defect and would
    have closed by teaching it**, had the caveat not been attached.
- **A stale rate was travelling, and N caught it (recorded here because this file holds the
  measured numbers).** C had inherited **"~3 in 4"** for the flake-2 rate from the seeds doc and
  repeated it unmeasured — and it was **heading into the close doc.** The measured values in this
  ledger, from H: **9/10 isolated and 3/3 in-suite at `53d212d`**; post-fix **0/10 isolated and
  0/10 suite at `aac0d67`**. "~3 in 4" matches no measurement at any scope. C's sync of the
  correction is pending the orchestrator's clearance; flagged upward separately, because a wrong
  rate in a close doc outlives every conversation that could correct it.
  - **IT DID PROPAGATE — this is not a near-miss.** The orchestrator confirms the wrong rate had
    **already reached F's inputs once**, i.e. it entered the M09 close-out pipeline, and **only
    N's catch stopped it.** Clearances for C's study sync and the #74 citation were granted before
    my flag arrived (messages crossed); the escalation was still correct, because the number was
    already downstream of the person who wrote it.
  - **BOTH FIXES LANDED.** C's study is synced to the shared checkout: the flake-2 heading now
    reads "12 failures in 13 — 9/10 isolated, 3/3 in-suite, from H's per-run table, measured at
    `53d212d`", with a paragraph naming what it previously said, where that came from, and that it
    was **corrected rather than quietly replaced**. F pinged with the line number and told to cite
    H's table for any sleeper rate. The #74 comment is posted (`issuecomment-5343513465`, via
    `--body-file`).
  - **The error was worse than C confessed, and he adopted the sharper form.** He had reported
    "~3 in 4" as a number REFUTED by H's table. It is **not a stale measurement — it corresponds
    to no measurement at any scope, before or after, at any commit.** A figure with nothing behind
    it, repeated in a shared file for hours. He notes he framed his own error too gently and only
    saw it when the arithmetic was stated.
  - **The milestone's cleanest instance of the disease C's own rule was written for:** an
    unmeasured number, inherited from a doc, repeated in good faith, travelling toward a permanent
    artifact — caught by a READER, not by its author and not by any rule. **Provenance rules make
    the catch possible; they do not make it happen.**
- **PROTOCOL GAP FROM THE SAME SWEEP, and it compounds SF-1 rather than sitting beside it.** Every
  committed consume payload carries `executionId`, `sessionId`, `reason` — **no rendezvous, no
  arming identity.**
  - **Consequence (M):** a consume cannot name WHICH arming it burned. So for C-R3's defect —
    burning a lease armed after the capture — the durable record **does not carry the field that
    would distinguish the right burn from the wrong one.** SF-1 says the oracle does not check;
    this says **the data would not support the check even if the oracle wanted to.** Silence at
    the oracle, and absence at the payload, on the same defect.
  - Recorded as an observation, not a scored row. It needs no run and it is in C's #74 comment.
  - **DESIGN CONSEQUENCE C DERIVED FROM IT, and he nearly wrote the field backwards.** PR 2's new
    field must record **the arming the sweep CAPTURED**, not the arming that is live when it
    records. The defect IS the mismatch between those two. **Replay already knows the live side** —
    the fold rebuilds it. **The captured side has never been on the log.** So recording the
    captured sequence gives the fold something to compare; recording the live one gives it a
    comparison **against itself — tautologically equal, a check that cannot fail.**
    - **That is the unfellable decoration B's ruling just removed from the filter, reintroduced one
      crate over.** Same family as the whole file's refrain: a guard nobody can break measures
      nothing.
    - **And it is invisible in review:** the two versions are **one field, same name, same type** —
      only the wiring differs. A diff cannot show which one was built. At B for sanction, not
      settled by C.
  - **PERMANENT LIMIT ON THE #74 EVIDENCE — the sharpest instance of rule 9 in this file.** Because
    the captured side was never recorded, **no future analysis can determine whether this defect
    ever fired in committed history.** Replay can say which lease was burned; **nothing can say
    which one the sweep MEANT to burn**, and the discrepancy between them IS the defect. Only one
    side exists on the log.
    - So **"precondition present, incident not observed" is not a gap a better sweep could close.**
      It is **unanswerable, permanently, for everything committed before the change.**
    - **Rule 9 at its limit:** the rule says a "zero X" claim must name the population in which X
      was observable. **Here that population is EMPTY and always will be.** Not narrow — empty.
    - **C is putting it in the commit** so a future reader does not search the archives, fail to
      find the incident, and conclude it never happened — **absence read as evidence, on a defect
      whose entire shape is absence read as calm.** Registered at his grain: a limit on what the
      DATA can answer, not a claim about what happened.
- **Also from the sweep, and it de-risks B's stricter fold:** no committed history contains an
  rdv-equal incident, so a stricter fold rejects nothing that replays today; the two corrupted
  archives are ALREADY unreplayable (consume-without-lease), so a new refusal is a no-op for them.

### C-PC — the inline positive control, sealed BEFORE the run. **ONE RUN SCORES TWO ROWS.**

C flagged the double-scoring himself: the orchestrator's step-1 sabotage (one-line replay-arm
sabotage showing the control FAILS) is **the same instrument as C-RR's**. One sabotage, two
questions — does C's new inline control catch it, and does the NEIGHBOUR catch it. **C-RR is not
re-registered; its cells stand as amended**, including that only a neighbour red ON THE
EXACT-RETRY ASSERTION, named by panic site, verifies the sabotage took.

- **Instrument:** add the inline exact-retry control to
  `the_same_key_at_a_later_sequence_conflicts_rather_than_replaying`, one line before the
  divergent case, in its own fixture; run `cargo test -p graphhelm-events --test local_atomicity`
  clean, then again under the replay-arm sabotage.
- **Predicted:** clean → C's test PASSES with the control included, 30/30. Sabotage → C's test
  FAILS **on the control assertion, named by panic site, NOT on the conflict assertion**. Same
  sabotage run → the neighbour ALSO fails on its exact-retry assertion (C-RR's cell).
- **Kills / uninformative (C's):** fails on the CONFLICT assertion instead → the sabotage did
  something other than disable replay, **both rows uninterpretable** until re-derived · PASSES
  under sabotage → **the control is decoration**, B's amendment failed to do what it was ruled in
  for, and C reports that as loudly as if it had worked · clean run red → the control is wrong,
  not the store, and C says so.
- **TWO PRE-RUN FLAGS FROM ME:**
  1. **Count ambiguity.** He predicts 30/30, but "one line before the divergent case, in its own
     fixture" reads as the SAME test function — under which the suite count stays **29/29** and a
     30 is unexplained. A NEW test function makes 30/30 right and 29 the anomaly. **Either number
     is "correct" under one reading and a silent anomaly under the other**; asked him to say which
     before running.
  2. **No cell for compile failure**, because he has run zero-cargo all day and never needed one.
     Sealed, per N's precedent: **compile failure is NOT A RESULT** — his "clean run red" cell does
     not cover it, since a non-compiling tree is not a red control but no measurement at all. Same
     for harness error and hang.
- **Collateral reds are expected and must not move the reading (M).** His previous two sabotages
  felled 2 and 7 tests. Per his own finding, **collateral is not aimed**: extra reds neither
  confirm nor weaken either row. What decides both is **which assertion fell in which test, by
  panic site.**
- **Why the row is worth having, in C's terms:** if the control cannot be made to fail, he will
  have added twelve lines that satisfy a rule's LETTER and measure NOTHING — the exact thing he
  refused to ship unrun an hour earlier. **So the sabotage is not a formality on this row; it is
  the row.**
- **SCORED — C-PC CONFIRMED.**
  - **CLEAN:** `cargo test -p graphhelm-events --test local_atomicity` → **29 passed, 0 failed**;
    clippy `--all-targets --locked -D warnings` clean.
  - **SABOTAGE** (replay arm never fires, one line in `local.rs`): **27 passed, 2 failed** —
    `exact_retry_is_resolved_before_sequence_and_divergent_reuse_fails_closed` panicked at
    `local_atomicity.rs:236:46`, and
    `the_same_key_at_a_later_sequence_conflicts_rather_than_replaying` panicked at
    `local_atomicity.rs:286:61`.
  - **:286 is C's CONTROL's own append — verified by reading the line back, not inferred from the
    number.** So his test fails ON THE CONTROL, before reaching the conflict assertion, exactly as
    sealed. **The control is real; B's amendment did what it was ruled in for.**
  - Sabotage reverted and verified: 29/29 green, no `SABOTAGE` or `if false` residue in the diff.
- **THE MISCOUNT — C reported it rather than letting a right-shaped result cover a wrong number he
  had sealed.** He predicted "30/30"; the suite is **29**. A miscount, not a measurement
  disagreement, but it sat in a sealed cell.
  - **CORRECTION TO MY OWN CREDIT (M).** I first wrote that my pre-run flag "was the reason it
    could not pass silently". **That is false, and C's timeline says so: he caught it on the clean
    run and reported it BEFORE my flag landed.** Independent, and his was first. What survives is
    the general lesson and not the causal claim: **a predicted COUNT is interpretable only if the
    instrument's SHAPE is declared with it** — "same test function" (29) versus "new test
    function" (30). C confirms it was the same function throughout. Recorded because a keeper who
    lets his own contribution stand mis-attributed has no standing to enforce attribution on
    anyone else.
  - **AND THE COLLATERAL WARNING WAS NOT EXERCISED (M).** I warned that extra reds must not move
    the reading. C generously calls that warning load-bearing, but his own numbers show it never
    fired: **the sabotage felled exactly two tests, both intended** — `:236` (the neighbour's
    retry) and `:286` (C's control's own append, verified by reading the line back). **No
    collateral appeared, so the warning is UNTESTED rather than validated.** It stays sealed for
    the next sabotage, where his earlier runs felled 2 and 7.
- **Status:** SCORED — CONFIRMED, with the sealed count corrected in place and two of my own
  claims about my contribution corrected alongside it.

### C-MASK — two sabotages, sealed BEFORE the runs, with their own non-independence flagged first

- **Green already in hand:** wake guards 5 passed (including the #74 red, now green);
  `graphhelm-events` all 12 test binaries ok; `graphhelm-cli --bins` 25 passed;
  `acceptance_map_is_grounded` ok; clippy `--all-targets --locked -D warnings` clean.
  - **The grounded gate converts a reading into a measurement, and they agree.** C's earlier "the
    demo has zero wake events so the digest cannot change" was a READING; B required it be
    OBSERVED by the instrument that enforces it. Recorded because **most of today's readings did
    not survive being measured** — this one did.
- **SABOTAGE A (filter):** drop `armed_at_sequence` from the comparison, match session only.
  **SABOTAGE B (fold):** write a constant into `armed_at_sequence` instead of `event.sequence`.
- **Predicted casualties, IDENTICAL for both, exactly two each:**
  `a_capture_from_before_the_wake_never_burns_the_lease_armed_after_it` FALLS ·
  `a_stale_capture_never_burns_the_lease_that_replaced_it` FALLS · the two rival-consume guards
  and `a_live_lease_consumption_still_records` stay GREEN (in those three the lease is either
  already burned — session lookup finds nothing, capture drops regardless of blade — or live and
  unreplaced, so every blade agrees).
- **C FLAGGED HIS OWN NON-INDEPENDENCE BEFORE THE RESULT, which is rule 10 applied preemptively —
  the first time today anyone did that rather than being caught by it.** A breaks the filter in
  `apps/cli`, B breaks the fold in `core/events`, but **no guard he has can tell those layers
  apart, because nothing reads `armed_at_sequence` except the filter.** So the pair buys
  confirmation that the property is protected at two layers and **nothing about which layer a
  future breakage came from.**
  - **Sealed in its sharpest form (M): two sabotages of different layers that no guard can
    distinguish are, for scoring, ONE INSTRUMENT IN TWO DRESSINGS. Identical lists count as ONE
    confirmation, not two.**
  - **He declined to add a field-reading guard to manufacture the distinction.** Right: an
    instrument built to create a difference is not evidence of a difference.
- **TWO OUTCOMES WITH NO CELL, sealed by me before the runs:**
  1. **The lists come back DIFFERENT.** Then "nothing reads `armed_at_sequence` except the filter"
     is FALSE, some guard does separate the layers, the two sabotages ARE independent after all,
     and the limitation dissolves. **Divergent lists KILL the non-independence claim and are GOOD
     NEWS.** Report the two lists separately, never merged.
  2. **A THIRD test falls.** His earlier sabotages felled 2 and 7, so this is not exotic. **Any
     casualty beyond the named two is UNPREDICTED COUPLING** — neither confirm nor kill on its
     own, but the same evidence that would separate the layers. Report by panic site; do not fold
     into "as predicted, plus noise". **Collateral is not aimed — but an unpredicted casualty in a
     sabotage you designed is not collateral, it is a hole in the reasoning that produced the
     prediction.**
- **THE CELL C MOST WANTS OBSERVED IS THE ONE THAT CAN END HIM:** if the swap guard does NOT fall
  under sabotage B, **the subsumption ruling was wrong, the rendezvous blade comes back, and his
  masking argument dies.** B named the same symmetry independently — which per rule 10 means the
  symmetry is FINDABLE, not that it is right.
- **NOT-A-RESULT, standing:** compile failure, harness error, hang. Plus, per K's finding,
  **`os error 112` is the DISK, not the code** — the drive oscillates between full and ~18 GB. C
  reports a 112 as a machine result and does not retry blind.
- **SCORED — SABOTAGE A CONFIRMED, SABOTAGE B REFUTED, and the cause is a defect in C's own
  fixtures rather than a surprise about the product.**
  - **A (filter matches session only): EXACTLY AS PREDICTED.** 3 passed, 2 failed — the two named
    defect guards fell and only those.
  - **B (fold writes a constant): PREDICTION WRONG, NEARLY INVERTED.** The two predicted to fall
    passed; `a_live_lease_consumption_still_records` and the pin guard — both predicted green —
    failed. Same count, different tests.
  - **WHY, in C's words and it is his fault, not the product's:** the fixtures hand-build
    `DueLease { armed_at_sequence: 1 }`, while **production phase 1 COPIES that field off the
    projection.** Under a constant fold the live lease reads 7 while the hand-built capture still
    says 1 — they mismatch, the capture is dropped, and **the two defect guards PASS FOR THE WRONG
    REASON**; meanwhile the match-expecting guards fail because a hardcoded 1 can never equal a
    constant 7. (The pin guard falls second-order: with the filter empty the recorder returns
    before the seam, so its injected rival never runs.) **The instrument does not model the path it
    is supposed to measure.**
- **THE SHARPEST FLATTENING INSTANCE OF THE DAY, and it is inside the scoring itself.** Both runs
  report **"3 passed, 2 failed"**. **Same value, two entirely different upstream causes, and
  nothing in the count recovers which.** C's line, kept: *the counts match and the meanings do
  not.* Anyone reading the matching counts as corroboration commits rule 7 while scoring.
- **THE MASKING ARGUMENT IS UNTESTED, NOT VALIDATED — C's call, and he refuses to claim it
  survived.** In production BOTH sides of the comparison come from the projection, so a constant
  fold makes them MATCH and the stale capture WOULD be recorded; in his fixture only one side does.
  Neither confirmed nor refuted. **B ruled partly on that argument and needs this first.**
- **C'S OWN NON-INDEPENDENCE SEAL ALSO DIES.** He predicted identical casualty lists; they
  diverged. The sabotages break different layers AND the fixtures make them visible to different
  guards — **for a reason that is an artefact of the fixtures rather than a property of the code.**
- **MY GAP-1 CELL WAS WRONG TOO, and it scores UNDECIDABLE (M).** I sealed a binary: identical ⇒
  non-independent; DIFFERENT ⇒ some guard separates the layers, limitation dissolves, good news.
  **The actual cause is a third I did not anticipate** — the fixtures do not model production, so
  the two sabotages hit different FIXTURE ARTEFACTS rather than different layers. Per rule 1 that
  is UNDECIDABLE, not a confirmation in the divergent direction. **The same failure I spent the day
  cataloguing, in the cell I wrote to catch C's.**
- **NEW ROW AGREED (not an amendment):** rebuild the capture the way the sweep does — replay, read
  the lease out of the projection, construct `DueLease` from it. **Fixture change = instrument
  change = new row**, by my own line. To be registered before running.
- **DESCENDANT QUESTION RAISED TO C, unresolved (M).** **The same fixtures produced C-R3's red,
  which is already SCORED in this file.** Does the hardcoded `armed_at_sequence: 1` undermine that
  red? My reading — and it is *reading*, so it is C's to check: in the red the capture is genuinely
  PRE-WAKE, so production would also have copied 1 off the projection at capture time, and the
  hardcode coincides with what production would produce. **The red looks safe and I am not scoring
  that from here.** The scored row changes on C's word, not on my reading.
- **RULING ON "THE CELL THAT CAN END HIM" — NOT-A-RESULT, and the reason is my own rule, not C's
  request.** I sealed: if the swap guard does NOT fall under sabotage B, the subsumption ruling was
  wrong, the blade returns, the masking argument dies. **The swap guard did not fall.** The trigger
  fired. C asks it score NOT-A-RESULT. Granted, on this test:
  - **Could the instrument have produced the other answer?** Under sabotage B with these fixtures,
    **the swap guard could NEVER have fallen** — a hardcoded `1` cannot equal the injected
    constant, whatever is true about masking. **An instrument that cannot produce the
    disconfirming outcome cannot confirm its own cell.** That is rule 7/8 unified, at the DESIGN
    altitude: "swap guard green" is produced both by *masking argument false* and by *fixture
    cannot model the path*, and nothing in the observation separates them.
  - **DISTINGUISHED FROM C-LC, and the distinction is the durable part.** C-LC's trigger also fired
    while its inference over-reached — but there the **instrument was VALID**: the run genuinely
    measured what it claimed, and only the conclusion outran it, so the ROW died and the CLAIM
    stayed unresolved. **Here the instrument is INVALID for the question**, like a failed positive
    control, so **nothing is scored at all.** Test between them: *did the instrument measure its
    own subject?* C-LC yes; sabotage B no.
  - **No cell marked. The subsumption ruling is neither vindicated nor overturned**, and B ruled
    partly on an argument that remains **untested — which is worse than it sounds, because the
    ruling was already made.**
- **PROVENANCE CHECK I CAN MAKE DIRECTLY (M).** C notes he wrote "the masking argument is UNTESTED,
  not validated" BEFORE my cell reached him, and asks that the timestamps be checked rather than
  taken on trust. **Confirmed from my own record:** his fixture-defect message, which contains that
  sentence, arrived before the message in which I sealed the swap-guard cell. **He flagged it
  against his own interest without knowing the cell existed.**
- **GAP 2 DID NOT FIRE:** exactly two casualties each, no third, no unpredicted coupling.
- **WHAT SABOTAGE B ACTUALLY BOUGHT, and C is right that it is worth more than the cell he failed
  to score:** **his fixtures do not exercise the production data flow.** Two of five wake guards
  pass under a broken fold for a reason unrelated to what they claim to test. **Found by predicting
  wrong** — which is the only way that finding was ever going to surface.
- **Status:** SCORED — A CONFIRMED; B NOT-A-RESULT for the masking/subsumption question and a real
  finding about the fixtures; masking argument UNTESTED with a ruling already resting on it; one
  descendant question open against an already-scored row.

### C-MASK2 — the fixture rebuild, sealed as a PAIR before either run

- **DESCENDANT QUESTION CLOSED, and C's answer is better than my reading.** I had reasoned that
  C-R3's red was probably safe because production would also have copied `1` at capture time. C
  checked instead: **the parked red draft contains ZERO occurrences of `armed_at_sequence` — the
  field did not exist on `DueLease` when that red was observed**, since he added it later as part
  of the fix. **The hardcode postdates the red entirely.** Timeline, not inference. **My scored row
  STANDS, on his basis rather than mine**, and the confirmation came from a fact rather than from
  my reasoning.
  - His sharper statement of the green, confirmed separately: **the hardcode only diverges from
    production when the FOLD IS BROKEN**, which is exactly and only sabotage B. Red unaffected,
    green sound, sabotage B invalid.
- **INSTRUMENT CHANGE:** build the capture the way the sweep does — replay, read the lease out of
  the projection, construct `DueLease` from it — applied to the two defect guards. **New row, not
  an amendment**, by the standing line.
- **RUN 1** (constant fold, single-blade filter, projection-built capture): PREDICT both sides move
  together → false MATCH → the stale capture IS recorded → both defect guards FALL. **The
  double-casualty prediction finally on the production path.**
- **RUN 2** (constant fold, **rendezvous blade temporarily restored**, same projection-built
  capture): PREDICT the stale capture still mismatches on RENDEZVOUS → dropped → **both guards stay
  GREEN under a broken discriminator. That green is THE MASKING, OBSERVED.**
- **Refuting cells (C's):** run 1 guards do NOT fall → the production path does not behave as
  claimed, re-derive · **run 2 guards DO fall → THE MASKING ARGUMENT IS DEAD, not merely
  untested** — the blade would have masked nothing, and the commit must not tell the masking story
  at all. (B has restated the ruling to stand on SUBSUMPTION ALONE, so the removal survives either
  way; the story does not.) · compile failure or a third casualty → NOT-A-RESULT / unpredicted
  coupling, per standing rules.
- **TWO STRUCTURAL FLAGS FROM ME, sealed before the runs:**
  1. **The pair is ONE instrument, and RUN 2 IS UNSCOREABLE WITHOUT RUN 1's RED.** C has a cell for
     run 1 not falling but did not say what becomes of run 2. Run 2's green means *"the blade
     stopped what run 1 showed gets through"*; with no run-1 red there is nothing shown to get
     through, and the green is just another green. **One-way dependency, order matters.**
  2. **RUN 2's GREEN HAS NO POSITIVE CONTROL** — the gap that has bitten three times today, twice
     in C's own rows. He predicts green *because* the capture mismatches on RENDEZVOUS; nothing
     proves the green comes from THAT rather than from restoring the blade changing something
     else. **Cheap decisive control: a run-2 variant where the capture carries the SAME
     rendezvous** — the blade then cannot drop it and the guards MUST fall. If they fall, the
     green is proven to come from the rendezvous comparison; **if they also stay green, the blade
     is dropping captures for an unrelated reason and the whole run-2 reading collapses.** Without
     it, "green under a broken discriminator" is one legal value with two upstream causes.
- **What makes a green-as-evidence design legitimate at all, sealed in C's favour and unprompted:**
  he flagged himself that run 2 is built so a GREEN is the finding, on a day he spent arguing
  greens rarely earn anything. It earns something **because the cell was declared first AND the
  refuting outcome is the guards falling**, and because the pairing is differential — same fixture,
  same sabotage, ONE variable changed. With the control above it is airtight; without it, one
  variable changed and one assumption unchecked.
- **Shared blind spot recorded (M).** On gap 1, C sealed "identical lists" and I sealed a binary;
  **neither of us listed fixtures-do-not-model-production as a possible mechanism.** Per rule 10
  that is precisely what agreement never protects against, and both of us were wrong in the same
  direction.
- **RUN 1 SCORED — CONFIRMED FOR THE TWO NAMED; two more fell, EXPLAINED-NOT-PREDICTED.**
  Result: 1 passed, 4 failed. Both predicted defect guards fell on the production path — with both
  sides moving together, the constant fold produces a false match and the stale capture IS
  recorded. **That half stands.** `a_live_lease_consumption_still_records` and the pin guard also
  fell, unpredicted.
  - **C did not report it as "as predicted, plus noise", per the standing rule.** His explanation:
    the same fixture artefact persists in the three guards he did NOT convert (they still hardcode
    `armed_at_sequence: 1`, which cannot equal the constant), and the pin guard falls second-order
    — with the filter empty the recorder returns before the seam, so its injected rival never runs.
  - **Recorded as EXPLAINED-NOT-PREDICTED, which is neither confirmation nor noise.** The
    explanation is **checkable** — three named unconverted fixtures — which makes it better than a
    story, but it remains post-hoc. The settling check is a conversion rerun, which he has a real
    reason not to do:
- **THE TRADEOFF C NAMED RATHER THAN ASSUMING AWAY — sealed as a general lesson.** Converting the
  remaining three would reduce run 1 to exactly two **and would make the live-lease guard BLIND TO
  FOLD CORRUPTION**, because projection-built captures move WITH the fold. **So the rebuild is not
  strictly better; it trades production-fidelity for fold-blindness.**
  - **General form (M): IF A GUARD'S INPUT IS DERIVED FROM THE THING IT CHECKS, IT CANNOT DETECT
    THAT THING BREAKING.** Hardcoded fixtures are blind to production drift; projection-built
    fixtures are blind to fold corruption. **Different blindness, not less.** Same shape as an
    unfellable check, one level out — and it means "convert everything to projection-built" would
    be a **regression sold as a fix.**
- **AMENDMENT TO RUN 2's CELL — pre-run, tightening, and it goes against C's own argument.
  ACCEPTED.** Working it through before running, he found his sealed prediction wrong for one of
  the two guards: the SWAP guard's capture is rdv-old against a live rdv-new, so the rendezvous
  differs, it is dropped, and it stays GREEN — masked. But the **RDV-EQUAL guard has `rdv-fixed`
  on both sides by construction — that IS the defect** — so rendezvous cannot save it: armed_at
  matches (both constant), rendezvous matches, it is RECORDED, and the guard FALLS.
  - **Amended prediction:** run 2 fells the rdv-equal guard and spares the swap guard.
  - **Why the amendment matters more than the correction (his framing, kept):** it makes the
    masking **PARTIAL and says exactly where** — the blade would have blinded the SWAP guard while
    the RDV-EQUAL guard still caught it. **Weaker than "the blade hides the failure, full stop",
    and more useful.** His own diagnosis: the original **overstated by generalising from the case
    it was built on.**
  - Legitimate by the standing rules: pre-run, tightening, sabotage already in place, declared
    before observing.
- **CONSEQUENCE OF THE AMENDMENT C MAY NOT HAVE SEEN (M): the experiment shrinks to a SINGLE BIT.**
  Run 1 already felled the rdv-equal guard; the amended run 2 predicts it falls again. So rdv-equal
  behaves identically in both runs, and **the entire masking claim now rests on ONE guard's ONE
  difference** — the swap guard, red in run 1, green in run 2. **That makes the run-2
  positive-control flag MORE load-bearing, not less:** with a single-bit differential, the swap
  guard's green must be caused by the rendezvous comparison and nothing else, and no control
  proves it. The same-rendezvous variant is now the difference between a one-bit result that can
  be defended and one that cannot.
- **RUN 2 SCORED — LANDED EXACTLY AS AMENDED.** `a_capture_from_before_the_wake` FAILED (rendezvous
  cannot mask it, predicted); `a_stale_capture_never_burns` **ok — MASKED, predicted**; plus the
  two unconverted-fixture casualties carried over from run 1. **The differential is the single bit:
  the swap guard, RED in run 1, GREEN in run 2, same fixture, same sabotage, only the blade
  changed.**
- **FLAG 1 DID NOT BITE, and that is worth recording rather than dropping.** Run 1's two named
  guards fell, so run 2 is scoreable. Had run 1 come back green, C would have had a green in run 2
  meaning nothing — and he did not have that cell before it was sealed. **A flag that does not fire
  is not a wasted flag; it is the one you find out you needed by not needing it.**
- **C IS RUNNING THE POSITIVE CONTROL RATHER THAN SHIPPING THE CLAIM WITHOUT IT**, and sealed its
  cells first. Setup: constant fold + rendezvous blade restored + **the capture taken AFTER the
  re-arm**, so it carries the SAME rendezvous as the live lease. Predict: **the swap guard FALLS** —
  rendezvous equal and armed_at equal (both the constant), so nothing in the filter can drop the
  capture.
  - **Cells:** FALLS → run 2's green is **PROVEN** to come from the rendezvous comparison, the
    single-bit differential is defensible, the masking claim ships · **GREEN → the blade drops
    captures for a reason unrelated to rendezvous, C's ENTIRE RUN-2 READING COLLAPSES**, and the
    claim does not ship · compile failure / third casualty → not-a-result.
  - **PRECISION ON ITS SINGLE-VARIABLE CLAIM (M).** The control changes the capture's rendezvous
    **by taking it after the re-arm**, which in general moves TWO things — which rendezvous, and
    when captured — and "when captured" normally determines `armed_at` too. **Under the constant
    fold `armed_at` is the constant either way, so the two collapse and rendezvous is genuinely the
    only live variable.** The control is sound as constructed.
  - **Sealed caveat so the design is not reused wrong: THE SAME CONSTRUCTION WOULD BE TWO-VARIABLE
    UNDER AN INTACT FOLD**, where taking the capture later changes `armed_at` as well as
    rendezvous. Anyone lifting this control into an intact-fold context inherits a confound and
    will not see it, **because the version that worked here looks identical.**
  - C's standing commitment, on the record: if it comes back green he says so as loudly as if it
    had fallen. **That commitment is the only reason a green from this control would be worth
    anything.**
- **CONTROL SCORED — IT FELL. `a_stale_capture_never_burns_the_lease_that_replaced_it` FAILED,
  left 1 right 0.** With rendezvous equal and `armed_at` equal (both the constant), nothing in the
  filter could drop the capture. **Run 2's green is PROVEN to come from the rendezvous comparison,
  not assumed.** Had the control come back green, the blade would have been dropping captures for
  an unrelated reason and C's entire run-2 reading would have collapsed — he sealed that outcome
  and it did not happen.

| Run | Blade | Capture rdv | Result |
|---|---|---|---|
| RUN 1 | single | differs | **FALLS** |
| RUN 2 | restored | differs | **GREEN** ← the masking |
| CONTROL | restored | **matches** | **FALLS** ← proves the cause |

- **The inference is sound and its shape is worth naming:** run 2 vs run 1 varies the blade with
  rdv-differs held; run 2 vs control varies rdv with the blade held. **Together they show the green
  requires BOTH blade present AND rendezvous differing.** One variable moved at a time, three
  cells of a 2×2.
- **THE FOURTH CELL IS UNRUN (M):** single blade + rendezvous matching. It would predict FALLS
  trivially (both sides constant → match → recorded), and it is the only cell that could reveal an
  interaction nobody has posited. **Not needed for the claim; recorded so the design is not later
  described as a complete 2×2.**
- **SCOPE THE CLAIM CARRIES, and it is narrower than "the blade masks" (M):** all three runs use
  the CONSTANT FOLD. **The measured claim is "under a broken fold, the blade masks the swap
  case".** It says nothing about an intact fold — which is fine, since masking is only meaningful
  when the discriminator is broken, but the scope belongs in the commit rather than being inferred
  later. **My earlier caveat is now load-bearing for exactly this reason:** the control is
  single-variable *only* under the constant fold.
- **STILL PARTIAL, unchanged by the control.** The rdv-equal guard falls in BOTH runs. The blade
  blinds the SWAP case and **cannot** blind the RDV-EQUAL case, because there the rendezvous is
  identical by construction — which is the defect itself. Measured form, as amended: **a redundant
  blade blinds the primary's sabotage exactly where the redundant one still discriminates, and
  nowhere else.**
- **WHAT THE PROVEN CLAIM IS NOW FOR (M).** B has already restated the ruling to stand on
  SUBSUMPTION ALONE, so the masking claim **no longer decides anything** — the blade comes out
  either way. Its remaining consumer is **the commit prose**: the commit must tell the accurate
  story, and the accurate story is the partial one. That is a real consumer and a small one, and
  naming it stops the proof being cited later as load-bearing for a decision it did not make.
- **STANDING OBJECTION DISCHARGED.** Nothing is now built on top of an unproven argument. C is
  sending B the three-run table and asking B's ruling on the three unconverted fixtures **rather
  than converting them himself** — because, per the general form, converting the live-lease guard
  would make it blind to fold corruption, which it currently catches by accident. **Which blindness
  each guard should have is a ruling, not a cleanup.**
- **Reverted and verified:** all three modifications (fold, filter, capture position) out; 5
  passed, 0 failed; no CONTROL or SABOTAGE residue in the diff.
- **Status:** SCORED COMPLETE — run 1, run 2 and the control all landed as sealed; masking claim
  PROVEN, PARTIAL, and scoped to a broken fold.

### C-MATRIX — the coherent instrument set, sealed before the runs

B ruled: convert all three remaining fixtures, plus add one designed guard.

- **Instrument changes:** (1) all three remaining fixtures converted to `capture_like_the_sweep`,
  so every capture models phase 1; (2) a **new direct guard** in
  `core/events/tests/execution_projection.rs` —
  `a_leases_armed_at_sequence_is_its_envelopes_and_a_re_arm_moves_it` — appending two arms in one
  batch, replaying the PREFIX and then the whole, asserting the lease's `armed_at_sequence` equals
  `committed[0].sequence` then `committed[1].sequence`.
- **The guard's design is right for a reason worth keeping (M).** Both assertions compare against
  **the envelope's own sequence, never a literal.** C's three reasons are each a real failure a
  literal would have missed: a fold copying a cursor, a constant that happened to match, an
  off-by-one that cancelled in this fixture. **That is assert-at-the-finest-grain applied to the
  comparison OPERAND** — a place nobody pointed it today. And it is why the guard survives someone
  rewriting the fixtures: **a literal is a claim about THIS fixture; an envelope sequence is a
  claim about THE PROPERTY.**
- **Predicted matrix:** UNSABOTAGED → all green (5 wake guards + the new direct guard) ·
  **CONSTANT FOLD → THREE casualties, all designed** (the direct field guard, plus both defect
  guards) **and ZERO accidental**, since the two run-1 fixture artefacts are gone ·
  **SESSION-ONLY → TWO casualties** (both defect guards fall; direct guard GREEN, because the fold
  is intact and only the filter ignores it).
- **Refuting cells (C's):** any accidental casualty still under constant fold → an unconverted
  fixture or an undiscovered dependency, re-derive not relabel · **direct guard green under
  constant fold → it is a decoration** and he reports that as loudly as if it fell · **direct guard
  RED under session-only** → something in the filter path reaches the fold, contradicting "nothing
  reads `armed_at_sequence` except the filter", a claim of his that has survived all day.
- **MISSING CELL AT THE BASELINE, sealed by me before the runs (M).** He predicts the unsabotaged
  run all green but has **no refuting cell for it failing** — and he is changing three fixtures and
  adding a guard. **If the unsabotaged run is not all green, the WHOLE MATRIX IS VOID:** not a
  finding about the product but a broken conversion, no valid baseline, and neither sabotage
  condition scoreable against it. **Same structure as the pair's flag 1, one level up — the
  sabotage rows mean nothing without a clean row to differ from.** Baseline runs first; a red
  baseline stops the matrix rather than being read alongside it.
- **MY OWN PREDICTION, on record rather than assumed (M):** under CONSTANT FOLD with converted
  fixtures the **PIN guard should NOT fall.** Its run-1 fall was second-order, caused by an empty
  filter; with both sides moving together the captures match, the filter is not empty. **If it
  falls anyway, that is C's "dependency I have not found" cell, and it is the likeliest place one
  is hiding.**
- **C is putting my control caveat into the SOURCE, not only this file** — right, since this file
  will not be open when someone lifts that control. His own diagnosis is the part worth keeping in
  the comment: he reasoned "rendezvous is the only thing that differs" **without noticing that only
  the sabotage made that true.**
- **Why the matrix matters more than its result (B's point, C agrees):** afterwards the instrument
  set is **coherent** — every fixture models production, every sabotage has a DESIGNED detector,
  and **no coverage exists by accident.** The accidental detection C was reluctant to give up is
  replaced by a guard at the grain of the property, which is the version that survives a fixture
  rewrite.
- **SCORED — ALL THREE CONDITIONS LANDED EXACTLY AS SEALED.**
  - **UNSABOTAGED:** 5 wake guards ok, direct guard ok (24 passed in `execution_projection`).
  - **CONSTANT FOLD:** THREE casualties, **all designed** — direct guard FAILED (`:460`),
    `a_capture_from_before_the_wake` FAILED, `a_stale_capture` FAILED. **ZERO accidental**;
    live-lease and pin both ok.
  - **SESSION-ONLY:** TWO casualties, both defect guards FAILED; **direct guard ok.**
  - **The direct guard separates the layers, which nothing did before:** red when the property
    breaks, green when only its consumer is blinded. That was the whole point of the exercise and
    it is now measured rather than argued.
- **MY PIN-GUARD PREDICTION: CONFIRMED.** Registered before his run — under constant fold with
  converted fixtures the pin guard should NOT fall, since its run-1 fall was second-order (empty
  filter) and with both sides moving together the captures match. Observed: `ok`. C's "dependency
  I have not found" cell did not fire.
- **MY BASELINE FLAG: UNTESTED, NOT VALIDATED — I decline the credit, on my own precedent.** C ran
  the baseline first (natural ordering, not because he had my cell) and it was green, so the matrix
  is valid and **the flag never fired.** He states plainly that he had no refuting cell for the
  baseline, did not notice the gap, and would have read a red baseline as a finding rather than a
  void matrix. That is a real hazard the flag addressed — **but a warning that does not fire is
  untested, exactly as I ruled for the collateral warning earlier.** It stays sealed for the next
  matrix.
- **AND MY SOURCE-PLACEMENT SUGGESTION WAS WRONG (M).** I told C to put the control caveat in the
  source. **He could not, and explained rather than silently not doing it: the control was a
  temporary run, reverted — there is no permanent control in the source for anyone to lift.** The
  lift risk lives in the design file and the commit body, which is where he put it, along with the
  unrun fourth cell, the "under a broken fold" scope limit, and Note 3 (the proven claim's only
  consumer is the commit prose). **I suggested a location without checking whether the artifact
  persisted; he checked.**
- **THE RUN-1 EXPLANATION IS NO LONGER POST-HOC — and the prediction stays wrong.** The same
  sabotage with converted fixtures now shows those two casualties GREEN, so C's explanation was
  right and **the conversion was the check that settled it.** He explicitly refuses to let that
  retroactively upgrade the original prediction. **Sealed as two separate ledgers: an explanation
  later confirmed by a designed check graduates from post-hoc to VERIFIED; the PREDICTION that
  missed those casualties stays WRONG.** Nothing about the confirmation reaches back.
- **COMPILE FAILURE REPORTED AS NOT-A-RESULT, and the cell worked.** `E0252` on the first attempt
  at the direct guard — a top-level `WakeLease` import in a file that imports it per-test — fixed
  to the file's convention, **no cell scored.** The not-a-result cell I sealed for him when he
  moved from zero-cargo to running is the one that fired first.
- **Status:** SCORED COMPLETE — matrix valid, all three conditions as predicted, instrument set now
  coherent.

### C-MISBURN — the schema half's one sabotage, sealed before the run

- **Green in hand:** `graphhelm-events` all binaries ok; `graphhelm-cli --bins` 25 passed; schema
  catalog `ok:true` with both catalogs agreeing with both schema copies; and
  **`acceptance_map_is_grounded` RE-OBSERVED, not re-reasoned** — C's stated reason: **the
  projection gained two fields, so the frozen digest COULD have moved**, and the enforcing
  instrument says it did not. **A green that could have gone the other way is worth something; a
  green that could not is worth nothing.** He knew which this was.
- **Instrument:** remove the mis-burn recording from the fold — delete the branch comparing the
  consumption's `captured_arming` against the burned lease's `armed_at_sequence` and inserting into
  `wake_mis_burns`.
- **Predicted: EXACTLY ONE casualty** —
  `a_consumption_that_burns_the_wrong_arming_is_recorded_and_replay_still_succeeds` FALLS;
  `a_matching_consumption_and_a_pre_change_one_record_no_mis_burn` GREEN. **C names the second as a
  VACUOUS pass** — it asserts the map is EMPTY and removing the recording keeps it empty, so it
  passes for a reason unrelated to the sabotage. He refuses to count it as evidence the sabotage
  was surgical.
- **Cells:** zero casualties → the guard is decoration and he reports it as loudly as if it fell ·
  two or more → unpredicted coupling, by panic site, not folded into "as predicted" · compile
  failure / hang → not-a-result.
- **TWO UNSABOTAGED GUARDS, NOT ONE (M) — and the vacuous pass is the second one.** The mis-burn
  recording has **two failure directions**: failing to record a real mis-burn (this sabotage), and
  **recording a SPURIOUS one when the armings MATCH**. That second direction is what
  `a_matching_consumption_..._record_no_mis_burn` owns, and **it has no sabotage at all**, so it has
  never been shown to catch anything. Cheap complementary sabotage: invert the comparison, or
  compare against the wrong field, so a matching consumption records a mis-burn — the guard must
  then FALL. **"One casualty, as predicted" must not be read as the pair being proven.**
- **THE BRICK-GATE GUARD IS FLAGGED UNSABOTAGED — and its justification is a READING (M).** C
  flagged it himself rather than letting it sit in the green list looking equal, which is the right
  instinct. But his reason for not sabotaging — the sabotage would be `skip_serializing_if` removal,
  "which the assertion catches BY CONSTRUCTION" — **is the same claim shape as
  trivially-true-by-reading**, the class this milestone spent itself discounting. Corrected form:
  **unsabotaged, AND the reason it needs no sabotage is itself unmeasured.**
  - **The property is a brick-gate and deserves the care:** absence must stay ABSENT, because a
    null on pre-change consumptions changes their canonical bytes and **breaks the hash chain of
    every consumption already committed.** A brick-gate guarded by construction-reasoning is the
    worst thing on this list to be wrong about.
- **What the mis-burn guard asserts, and it is the right set:** replay SUCCEEDS (the
  record-don't-refuse ruling, executable); the mis-burn is recorded where the attention predicate
  can reach it; it names BOTH armings plus the consumption's own sequence — captured, live,
  at_sequence — **each against the ENVELOPE's sequence rather than a literal**; and **the lease is
  still gone.** That last is the honest half: **recording the mistake does not undo it, and the
  sleeper is still stranded.** That is the condition attention has to surface.
- **Status:** OPEN, sealed, running.

### C-FALSEPOS and C-WIREABSENCE — sealed before running; the second creates a new category

**C-FALSEPOS — the complementary blade, built rather than argued.** Instrument: make the fold
record a mis-burn UNCONDITIONALLY (drop the inequality test). **Predict exactly one casualty:**
`a_matching_consumption_and_a_pre_change_one_record_no_mis_burn` FALLS (entries appear, map no
longer empty); the true-positive guard stays GREEN (captured 1 vs live 2 is still recorded
correctly). **This reaches the false-positive direction the first sabotage could never touch** —
the gap flagged one round earlier, now closed by an instrument instead of an argument.

**C-WIREABSENCE — and C predicts against BOTH the reviewer's expectation and his own guard's
survival.** Instrument: remove `skip_serializing_if` from `captured_arming`, so an absent value
emits `"capturedArming": null`.

- **B predicted the wire-absence guard falls. C predicts FOUR casualties, for a reason unrelated to
  the assertion.** The schema says `{"type": "integer", "minimum": 1}`; a null is not an integer, so
  envelope validation should REJECT the append outright, and every test appending a consumption
  WITHOUT the field dies at its own `append(...).unwrap()`.
- **The part that matters, in C's words: HIS ASSERTION WOULD NEVER RUN.** The append precedes the
  wire assertion, so the sabotage would prove **the SCHEMA rejects null — not that his guard catches
  it.** The guard would stay unobserved, **wearing a red it did not earn.**
- **Cells:** four casualties all AT THEIR APPENDS → the schema is the real defence, his wire
  assertion is unobserved and arguably redundant · his guard falls AT ITS OWN ASSERTION → validation
  does not cover this path, the assertion is the only thing standing, and it is finally observed ·
  anything else → re-derive.

**NEW CATEGORY, from C's prediction: the VACUOUS RED (M).** He has named the mirror of the vacuous
GREEN he flagged an hour earlier. A vacuous green **passes** for a reason unrelated to what the
guard measures; a **VACUOUS RED FAILS** for a reason unrelated to what the guard measures.

- **It is worse than the green case, because nobody audits a red.** "The sabotage went red, so the
  guard measures something" is the standard inference, and here it would be **false**.
- **HOLE IT EXPOSES IN THE BOARD'S SABOTAGE-EVIDENCE RULE.** That rule requires a raw PER-GUARD red
  list. **That is not enough:** a guard red at an EARLIER panic site is not evidence the guard
  measures anything. **Sealed strengthening: the rule needs RED AT THE GUARD'S OWN ASSERTION, NAMED
  BY PANIC SITE — not merely red.** The same requirement I put on C's neighbour-red earlier,
  generalised from one row to the rule itself.

**CAUTION ON THE PROPOSED REMEDY (M).** If the schema does reject null, C proposes removing the
wire assertion or re-siting it. **Redundant-today is not redundant-permanently.** Its remaining
value is as a **tripwire if the schema constraint is ever loosened** — `minimum: 1` is one edit from
relaxation, and the hash-chain consequence of a null is **permanent and silent** (a null on
pre-change consumptions changes their canonical bytes and breaks the chain of every one already
committed). **Recommend keeping it with a comment recording that it is currently redundant and
why.** A guard whose redundancy is documented survives the schema change that makes it necessary
again; a removed one does not.

- **Status:** both OPEN, sealed, running (item 1 first).

### STORM RE-BASELINE at `ef51193` — recorded, UNPREDICTED, and it does not license the obvious reading

H's raw: **0 failures / 10 isolated runs** of `the_storm_holds_under_eight_concurrent_agents` at
`ef51193` (new milestone tip). Receipts: 10× the named-grain "ok", 10× "1 passed; 30 filtered out",
**zero panicked hits — no failure text exists**. Wall 17-19s vs base 21-31s, **reported and
explicitly not interpreted**, per D-TRAP-1.

- **H called it HISTORY, not control, himself:** two store-path commits + C's fix + A's seam landed
  between `53d212d` and `ef51193`. **Even a clean separation would not be attributable to
  anything.**
- **AND THE NUMBERS DO NOT SEPARATE EVEN IF IT HAD BEEN A CONTROL (M).** 0/10 gives a 95% upper
  bound of ≈26%; the prior 4/10 gives a 95% interval of roughly **12%–74%**. **Those intervals
  overlap from 12% to 26% — a true rate of 20% is consistent with BOTH results.** So "was 4/10, now
  0/10, the storm is fixed" reads a separation that is not there. **Two independent reasons the
  number cannot carry the conclusion**, and H named one of them before I named the other.
- **What would separate it, if anyone wants it:** ≈**N=25** consecutive zero-failure runs puts the
  upper bound under ≈12%, below the prior's lower bound — roughly 8 minutes at H's per-run time.
  **Not requested.** It would buy a comparison against a HISTORY, not a control, so it may not be
  worth the machine time at all. D's and the orchestrator's call.
- **UNPREDICTED — no row, no credit, no retrofit.** D-P4 concerned the BASE rate and is already
  scored confirmed-loose. **No sealed cell exists for the storm rate after the store-path commits**,
  so per rule 1 this is recorded as a finding attached to nothing.
- **THE CONSEQUENCE, AND IT IS THE BIGGEST THING IN THE REPORT.** A 0-rate **starves every
  failing-run-dependent analysis** in D's instrumented program — D5 clustering and the rest need
  failures to observe. **So the storm MECHANISM may become undecidable by the planned route — not
  because it is fixed, but because the rate dropped below what the instrument can catch.** H1 vs H2
  vs H3 was already entirely open; the instrument that would have decided it may now have nothing
  to look at. **Same "fixed vs rarer than N can see" problem as the sleeper, except here it blocks
  the DIAGNOSIS rather than the closure.** H flagged the starvation risk himself and routed the
  call to D and the orchestrator rather than making it.
- **THE CONFOUND I MISSED ENTIRELY, AND IT IS THE MORE IMPORTANT HALF (D's).** **The DISK PREP
  happened BETWEEN H's 4/10 at `53d212d` (pre-prep) and the 0/10 at `ef51193` (post-prep).** H3 is
  **directly disk-dependent** — free space and cache state drive fsync latency, the dominant
  per-open cost. **So the code change and the disk state moved together.**
  - **I named the confounds H gave me (two store-path commits, C's fix, A's seam) and stopped
    there.** I did the statistics and missed the physics, on a machine whose disk had been at 0 GB
    earlier the same session and whose `os error 112` K had already traced to the disk.
  - **MY PROPOSED REMEDY WAS THE WRONG INSTRUMENT, and D's is right.** I suggested ≈N=25
    zero-failure runs to separate the rates. **That separates a number from a HISTORY under a
    CHANGED DISK STATE — it isolates nothing.** D's control is one run: **re-run `53d212d` NOW,
    post-prep.** Same-disk comparison isolates the code. Cheaper and decisive where mine was
    expensive and confounded.
  - **Gating rule sealed: any row asserting the code change fixed the storm is gated on that
    control.** I hold no such row; if one appears, this is its precondition.

### D-META-2 — what 0/10 licenses (D's row, sealed as he proposed)

- **D aimed rule 9 at the result he would most like to be true.** P(0 in 10 | rate 40%) = 0.6¹⁰ =
  **0.6%**, so the rate almost certainly DROPPED. **But the 95% upper bound on 0/10 is ≈26%**
  (rule of three gives ≈30%). **A true rate of 10–25% is fully consistent with this result and is
  still a flake that bites CI.** Ruling out a 5% ceiling at 95% needs **≈60 runs**.
- **Sealed statement:** *0/10 at `ef51193` bounds the rate at ≤26%; it does not establish
  elimination; a 5% ceiling requires N≈60.*
- **"The flake is gone" is NOT established by this measurement**, and if it reaches a summary line
  it is exactly the construction rule 6's third clause exists to catch.

### D-META-3 — every rate this lane holds is UNCONTROLLED for free disk space

- **The upgrade, from a fact I supplied and D did not have:** he had framed the confound as "cache
  state changed". With the disk having hit **0 GB on this machine today**, it is potentially
  **"H's 4/10 baseline was measured on a pathologically full disk"** — and on NTFS, 0 GB free is
  not a mild perturbation of fsync latency. **H3 is precisely the hypothesis that fsync dominates
  per-open cost.**
- **The hole, and it reaches backwards over everything:** **no free-space figure was ever recorded
  alongside any storm measurement.** H's report gives machine context and says "no disk anomalies"
  — **that is a JUDGEMENT, not a number.** So **H's 4/10, the "3 in 6" in the issue history, and
  the original #19 report are ALL uncontrolled for the variable H3 says dominates.**
- **Rule 9 at a new altitude (D's framing, adopted):** the population here is **MEASUREMENT
  CONDITIONS, not observed events.** Every rate names a population — *runs on this machine under
  UNRECORDED disk state* — and **the hypothesis at stake lives in exactly the unrecorded
  dimension.**
- **D HOLDS HIMSELF TO THE SAME LINE HE HELD THE 0/10 TO, unprompted:** this does **not** establish
  the flake was a disk artifact. We know the disk hit 0 GB at some point today; **we do not know
  its state during H's baseline.** **"The flake was the disk" is exactly as unestablished as "the
  flake is gone"** — same rule, same reason. He says he would rather state that than enjoy a
  convenient explanation.
- **THE STRUCTURAL POINT, and it is more than a courtesy about my miss.** D's reading of why I named
  the commits and missed the physics: **the review was framed as a CODE review from the start.**
  Every apparatus built in this lane — three apparatus, caller tags, phase denominators —
  **measures the program. None of them measures the MACHINE.** *A lane that instruments only the
  software will attribute machine variance to software every time*, and neither of us noticed the
  whole instrument had that shape until a disk went to zero.
- **The fix, costing a second per run:** record free space — and ideally a one-line fsync-latency
  probe — as a **REQUIRED FIELD next to every storm run, in the same file as the run table.** The
  entire ambiguity now stuck to this lane **would not exist if one line had been captured beside
  H's 4/10.**
- **ONE RECOVERY PATH NOBODY HAS PROPOSED (M), and it may still bound the unknown.** The disk-full
  event is not undated: **C reported the disk at 0 GB and K traced `os error 112` to it**, both
  with positions in today's message order, and **H's run reports carry per-run wall times.** If the
  112 errors and the 0 GB observation can be placed relative to H's baseline runs, the baseline can
  be **bounded** as having run before or after the disk filled.
  - **Marked precisely: this bounds, it does not measure.** A timeline gives "the disk was not yet
    full when the baseline ran" or "it may have been" — never a free-space figure. **If the
    baseline predates the fill, the confound weakens sharply; if it overlaps, D-META-3 is not just
    a gap but a live alternative explanation.**
  - Cheap, uses evidence already written down, and it is the only route to the one number nobody
    captured. **Whether it is worth doing is D's and the orchestrator's call, not mine.**
- **D RAN IT, AND FOUND A BETTER ROUTE THAN THE ONE I POINTED AT: FILE MTIMES.** Read-only, no
  machine time. His timeline:
  - **H's baseline:** `storm_run1.log` 09:23:18 → `storm_run10.log` 09:28:19; full suite window
    09:23–09:38. The four FAILING runs identifiable **by log size** (172 bytes = pass, larger =
    fail): runs **3, 6, 7, 10** at 09:25:29, 09:26:43, 09:27:06, 09:28:19 — **matching H's reported
    4/10 exactly.**
  - **The cache levers were spent at 14:46:48 and 14:46:49** — the `c-agent-e82f40` and
    `k-agent-f9a127` worktree dirs, one second apart, both now target-less. **VERIFIED BY ME:**
    `stat` returns `2026-08-19 14:46:48` and `14:46:49`, and neither `target/` exists.
  - **THE LOG HALF IS NOW VERIFIED BY ME TOO — AND MY EARLIER MARKING WAS WRONG.** I recorded H's
    scratchpad as "outside my reach" and marked the log mtimes as taken from D, unverified.
    **I asserted that limit without testing it.** The directory is reachable, and I have now read
    every file directly:
    | run | mtime | size |
    |---|---|---|
    | 1 | 09:23:18 | 172 |
    | 2 | 09:25:07 | 172 |
    | **3** | **09:25:29** | **1516** |
    | 4 | 09:25:58 | 172 |
    | 5 | 09:26:22 | 172 |
    | **6** | **09:26:43** | **2221** |
    | **7** | **09:27:06** | **817** |
    | 8 | 09:27:30 | 172 |
    | 9 | 09:27:54 | 172 |
    | **10** | **09:28:19** | **817** |
    **Every one of D's figures checks exactly**, and the **4/10 reproduces from log sizes alone
    without relying on H's report** — 172 bytes = pass, larger = fail, four larger.
  - **The correction against me:** I marked a fact unverifiable-by-me **without checking whether it
    was.** That is the same shape as everything else in this file — **an asserted absence whose
    population I never tested.** Rule 9, aimed at my own reach rather than at a measurement.
  - **Re-baseline:** 15:05:04 → 15:07:44, ~18 minutes after relief.
- **THE BOUND, and it runs AGAINST the reading D would have found convenient.** H completed a full
  `cargo test --no-run` build **and then 13 test runs**, each creating tempdirs and event stores,
  across 09:23–09:38, **with zero disk errors. A disk at 0 GB does not permit that** — K's
  `os error 112` is precisely what 0 GB does to a build. **So the fill lies BETWEEN H's baseline
  and the 14:46 relief: THE BASELINE PREDATES THE FILL.**
  - **Why that cuts against the disk story:** the disk explanation for 4/10 → 0/10 requires the
    baseline disk to have been **meaningfully worse** than the re-baseline's 18.2 GB. We now know
    it was **not pathological**, so the disk differed **less** between the two measurements than
    feared, and code or fleet load carry correspondingly more.
  - **D refuses to overclaim in the new direction either:** free space at 09:23 remains unmeasured,
    **"not zero" is compatible with 3 GB and with 40 GB, and NTFS fsync latency degrades well
    before zero.** **The disk story is WEAKENED, NOT KILLED.** D-META-3 stays a gap — a narrower one.
- **HIS WEAK SIGNAL IS EVEN WEAKER THAN HE LABELS IT, AND THAT SUPPORTS HIS CAUTION (M).** He notes
  the four failures are spread (runs 3, 6, 7, 10) rather than bunched, reading it as mildly against
  a transient spike. Precision: **runs 6 and 7 ARE adjacent**, and with 4 failures among 10 slots,
  **P(at least one adjacent pair) ≈ 83%** (35 of 210 arrangements have none). **So the observed
  pattern is exactly what randomness looks like** — a *fully* non-adjacent spread would have been
  the 1-in-6 surprise. The signal therefore supports neither explanation, which is stronger
  agreement with his "weak" label than the label itself claimed.
- **D'S SIGNAL IS STRUCK, NOT DOWNGRADED — he verified my combinatorics and found himself wrong
  TWICE.** He confirmed C(10,4)=210 and C(7,4)=35, so P(no adjacent pair) = **exactly 1/6**. His two
  errors: (1) runs 6 and 7 **are** adjacent, so the pattern is not even "spread" as claimed; (2)
  **even a genuinely non-adjacent spread would have been the 1-in-6 SURPRISE, not the expected
  case** — so the inference direction was muddled on top of being factually wrong. **The signal is
  not weak, it is ZERO**, and he struck it rather than downgrading it.
- **DURABILITY PROBLEM D CREATED AND FIXED, with his own diagnosis attached.** My "unverified by
  me" marking left a load-bearing fact single-source **and perishable** — H's own report says that
  scratchpad is session-scoped, "copy out anything needed long-term". He wrote
  `.factory/d-agent-disk-timeline-evidence.md` to the shared checkout (verified present by me,
  3,949 bytes): full timeline table, per-run log sizes that reproduce the 4/10 **without relying on
  H's report**, the lever timestamps, reproduce commands, and my adjacency correction with its
  arithmetic.
  - **His self-diagnosis, kept because it is the sharper half:** *he spent the afternoon arguing
    that an unrecorded variable cost us an answer, then built a bound on evidence sitting in a temp
    directory scheduled for deletion.* **Same lesson as the free-space field, one layer out:
    capture the evidence WHILE IT EXISTS.**
  - **Durability is not verification (M).** His file makes the fact **re-checkable by anyone**; it
    does not by itself make it verified. What made it verified was reading the logs — which, as
    above, I could have done all along.
- **THE UNIFYING LINE, D's, across three instances today:** my "cannot stat" scare (two upstream
  causes: deleted file, wrong cwd) thirty seconds after sealing rule 7's newest instance; his own
  summary-line failure three messages after naming the class; and the four-in-a-row tally.
  **KNOWING THE RULE DOES NOT ARM IT. ONLY A MECHANICAL CHECK DOES.**
  - And his framing of my cwd error is rule 9 at the smallest possible scale: **"the file is not
    there" names a population — the directory you were actually in — and the claim lived in a
    different one.**
- **THE METHOD NOTE IS WORTH MORE THAN THE FINDING (D's, and I agree):** the recoverable evidence
  was **not** in the message order I pointed at — it was in **mtimes on disk**, which nobody had
  thought to treat as measurement data. **The machine was recording the whole time; we just were
  not reading it.** Generalised: **filesystem metadata is an unexploited instrument in a lane that
  spent the day lamenting an unrecorded variable.**
- **RANKING SHIFT, stated by D as a shift and not a verdict:** **H3 now has a documented mechanism
  tied to a real event on this machine**, where it was previously the least motivated of the three.
  **H1 remains structurally true as a description of the code — but structural truth is not the
  same as being what crosses 5s.** So **D1 (max single-open elapsed vs the 5s budget) is the
  discriminator that matters most — and it is scoreable on PASSING runs, so it survives the
  starvation.**
- **And the falsifier still binds:** D's own words — the line about it binding when the flake
  stopped reproducing is *"worth exactly as much as it costs me if D1 comes back saying a single
  open approaches seconds."*

### THE 53d212d CONTROL (Run 0) — SEALED → WITHDRAWN → **REINSTATED**. Cells LIVE, history kept.

**Third state transition, and the trail is the point.** Sealed pre-run → withdrawn by its designer
on cost → **reinstated** after D's spec update re-added it with a tree-preservation condition that
holds (main-checkout FLIP: named stash by explicit paths, incremental rebuilds in the same target
so ≈zero new disk, fingerprint-verified restore). **Only the m-worktree variant died.** The
RETIRED-NOT-ANSWERED status **lifts**.

- **The cells are LIVE exactly as sealed:** 0/10 → disk explains · 3-4/10 → code explains · 1-2/10
  → UNINFORMATIVE and never argued either way afterwards · compile or harness → not a result. Both
  asymmetries stand: **"disk explains" does not mean C's fix was pointless**, and the control
  **inherits the ≈26% resolution limit**.
- **INTEGRITY CHECK ON A WITHDRAW-THEN-REINSTATE (M), because the manoeuvre could hide a retrofit
  and here does not.** The cells were sealed before any run, withdrawn before any run, and
  reinstated before any run — **no data intervened at any point.** That is the only condition under
  which reinstatement is safe: **a withdrawal that happens AFTER a result, and a reinstatement that
  follows it, would be cell-shopping.** Recorded so the trail proves the timing rather than
  asserting it.
- **NEW PRIOR ARRIVED BETWEEN SEALING AND RUNNING, and it does NOT touch the cells.** D's mtime
  timeline bounds H's baseline as **predating the disk fill**, which weakens the disk story. **The
  cells are outcome→interpretation mappings and are unaffected by priors** — but the *reading* of a
  result now sits in a changed context, so the interaction gets pre-specified below rather than
  argued afterwards.
- **PRE-SPECIFIED CONFLICT CASE — what if Run 0 returns 0/10 while the mtime bound says the
  baseline disk was not pathological?** These look like disagreeing instruments and are not:
  - **They are jointly satisfiable.** The mtime bound establishes only **"not zero"** — it never
    established "not degraded". A 0/10 at Run 0 plus the mtime bound **jointly imply the baseline
    disk was DEGRADED BUT NON-ZERO**, which is a coherent state and precisely the space the bound
    left open (NTFS fsync latency degrades well before 0 GB).
  - **Sealed: that combination is NOT a contradiction and must not be reported as instruments
    disagreeing.** It is a narrowing — from "unknown disk state" to "degraded, non-zero".
  - **If they genuinely conflicted, Run 0 wins on directness:** it is a same-disk A/B on the same
    code, while the bound is an inference from "a build succeeded". Stated now so the tie-break is
    not chosen after seeing which way it goes.
- **THE TWO-QUESTIONS FRAME SURVIVES AND IS NOW THE SCORING FRAME.** **Run 0 answers the BACKWARD
  question** (same-disk comparison: did the disk cause the 4/10?); **headroom answers the FORWARD
  one** (is this machine near the cliff?). **Both now run; neither may be spent on the other's
  question**, and my sealed refusal stands verbatim: **the headroom number never settles the disk
  question.**

#### SCORED — Run 0 executed, and the sealed cell FIRES

A's report: **main-checkout flip, SHA `53d212d` VERIFIED BEFORE THE FIRST RUN** (my precondition —
a wrong lineage discovered after the runs is indistinguishable from a real result), **N=10,
per-run fields captured. RESULT: 0/10 failures.** Machine fields, now required and present:
**fsync 1.5–2.0 ms/op, 19 GB free, zero concurrent cargo.** Record at
`.factory/a-agent-storm-execution-record.md`.

- **The cell fires as sealed: the OLD CODE ALSO STOPS FLAKING ON THE NEW DISK, so the code change
  is NOT NEEDED to explain the 4/10 → 0/10 improvement.** Result landed in a clean cell, not in the
  1–2/10 uninformative band, so no tie-break was invoked.
- **CORRECTION TO MY OWN CELL'S LABEL (M), and it is the same defect I catalogued in D-P7.** I
  named this cell **"disk explains"**. **What the control actually isolates is CODE vs NOT-CODE.**
  It **exonerates the code**; it does **not** establish the disk specifically, because **fleet load
  also changed and was never recorded at the baseline** — the third unrecorded variable. A's run
  had zero concurrent cargo; H's baseline concurrency is unknown and unknowable.
  - **Honest verdict: THE CODE IS EXONERATED AS THE CAUSE OF THE IMPROVEMENT. The attribution to
    DISK SPECIFICALLY IS NOT ESTABLISHED.** "Disk explains" is my label overstating my own
    instrument — **exactly the shape of D-P7's "pool = sweep": a cell named for the hypothesis
    rather than for what it separates.**
- **BOTH SEALED ASYMMETRIES APPLY, unchanged:**
  1. **This does NOT mean C's fix was pointless.** It was justified on other grounds entirely and
     is untouched by this control.
  2. **The control inherits the ≈26% resolution limit.** 0/10 at `53d212d` bounds *that* rate at
     ≤26% as well — it does not establish elimination there either.
- **THE PRE-SEALED CONFLICT CASE NOW APPLIES, and it resolves as sealed:** Run 0's 0/10 plus D's
  mtime bound (baseline **predates** the fill, so not a 0 GB disk) **jointly imply the baseline
  machine state was DEGRADED BUT NON-ZERO** — a coherent **narrowing**, not instruments
  disagreeing. Sealed before either result existed; applied without amendment.
- **RUN 2 PRODUCED 10 NOT-RESULTS under my compile/harness cell** — an instrument sink bug, cause
  verified. **No storm numbers came out of it**, and none were read from it. **That cell did the
  job it was added for**, on its second live firing.
- **ONE NOTATION I WILL NOT GUESS AT, and have asked A to disambiguate:** "Run 3 probe-off **3/3**"
  carries no `fail` qualifier, while Run 0 was reported explicitly as "0/10 **fail**". **3/3 passes
  and 3/3 failures point in opposite directions** — the latter would mean the probe's presence
  suppresses the flake, an observer effect that would undercut every instrumented run. **An
  ambiguous notation is not a result**, and I am not resolving it by inference in the direction
  that happens to be convenient.

- **THE LABEL CORRECTION WAS REACHED TWICE, AND I CLAIM NO PRIORITY.** D made the same correction
  and it reached me through the orchestrator after I had made it while scoring; **the messages may
  have crossed and I cannot establish the order.** Per rule 10, two people catching the same label
  defect is **FINDABILITY, not corroboration** — which applies to agreement about my own error as
  much as anyone's.
- **SECOND INSTANCE OF THE C-LC PATTERN, which makes it a named shape rather than a one-off:
  A CELL'S LABEL CAN OVERSTATE WHAT ITS TRIGGER ESTABLISHES.** C-LC: **row killed, claim
  unresolved.** Run 0: **row FIRES, narrowed reading attaches.** Same structure both times — the
  trigger fires honestly and the inference is narrower than the label.
  - **Final verdict, orchestrator's wording, sealed: CODE EXONERATED; DISK-OR-LOAD IMPLICATED,
    UNSEPARATED.** The degraded-but-non-zero narrowing survives for the disk half only.
  - **H's raw note is the cleanest statement of the confound we have, and it belongs in the owner
    report verbatim: SAME COMMIT, 4/10 MORNING, 0/10 TONIGHT, DIFFERENT DISK AND LOAD.** One line,
    no inference, and it makes the unseparated attribution obvious without arguing either half.

#### C2's control — SATISFIED BY EXISTING DATA, with an INTERNAL INCONSISTENCY I am not smoothing

The orchestrator reports both of the following, and **they cannot both be true as stated**:

- **(a)** "predicted 3 happy / 1 with Edit-2 removed; **observed exactly that**" → C2 satisfied.
- **(b)** "Edit 3's independent sabotage **NOT-RUN because phase 3 never executes in the storm**".

**D's exact-count derivation was:** arm POST → phase 1 opens once, returns early; signal POST →
phase 1 opens, rings, **phase 3 opens.** *That is where the third row comes from.* **If phase 3
never executes, the happy count is 2, not 3** — and an observed 3 needs a different explanation.
**RESOLVED — AND BY A THIRD OPTION NEITHER OF MY TWO BRANCHES NAMED.** I offered "either phase 3
executes, or it does not". **Both are true, in different fixtures:**

- **The CONTROL SCENARIO (arm + signal) ARMS A LEASE**, so its signal-sweep reaches phase 3 —
  **that is the third row, and why the happy count is 3 THERE.**
- **The STORM arms nothing**, so phase 3 never executes **THERE** — which is where "Edit 3 not-run"
  applied.
- **Both statements were true in their own context; the package quoted them CONTEXTLESS.**

**SEVENTH INSTANCE OF THE SUMMARY FAMILY, AND A NEW MECHANISM: CONTEXT-STRIPPING.** The earlier six
were **stale** claims surviving compression. This one is different — **two CURRENT, TRUE statements,
quoted without their fixtures, MANUFACTURE a contradiction that exists in neither source.**
**Compression did not preserve an error; it CREATED one.** Same variable as the rest: **the
register, not the topic.**

- **MY "CONTROL IS WEAKER THAN ITS ARGUMENT" CONCERN IS RETIRED BY MEASUREMENT, NOT BY ARGUMENT.**
  A ran **both** single-site sabotages in the control scenario and **observed both falling alone**:
  Edit-2-only → exactly 1 pair; Edit-3-only → exactly 2 pairs. **The exact-count form has its two
  independent blades, measured.** The concern was valid when raised and is answered by data.
- **Sealed regardless: NOT-RUN IS NOT PASSED.** That call stands on its own.

#### CLOSING DATA — Run 2b and the headroom number. **v3 DOES NOT FIRE, AND THAT IS NOT A CLEARANCE.**

A's close-out: **Run 2b (D's sink v2) = 10/10 PASSES**, verbatim in all ten logs. **Headroom D1:
O = 70–80 ms/request, so 8×O = 0.56–0.64 s against a 5 s budget — roughly 8× headroom.** No
falsifier trigger fires. Failure-conditional rows starved, correctly. Record closed at
`.factory/a-agent-storm-execution-record.md`; final revert verified.

- **SCORED AGAINST v3: DOES NOT FIRE.** v3 fires at 8O + W ≥ 5 s; 8O is 0.6 s, so W would have to
  exceed **4.4 s** to reach the trigger. **The non-fire is therefore ROBUST TO ANY PLAUSIBLE W** —
  evaluable in the negative direction even though W_mutation was never reported to me. That is
  worth stating, because the row's three-input cell would otherwise make it look unevaluable.
- **AND THE SEALED ASYMMETRY IS EXACTLY WHAT THIS RESULT NEEDS: A v3 NON-FIRE IS UNINFORMATIVE AND
  IS NOT A CLEARANCE FOR C1.** v3 is an **optimistic bound that under-fires** (it ignores the
  per-operation exclusive lock that v4 identified). **Nobody may read "the falsifier did not fire"
  as permission to adopt C1.** The sentence was pre-refused; here is the result that would have
  produced it.
- **WHAT THE HEADROOM NUMBER DOES ANSWER: the FORWARD question, for TODAY'S MACHINE ONLY.** Is this
  machine near the cliff? **No — 8× headroom on open cost.**
- **WHAT IT CANNOT ANSWER, and this is the limit that matters (M):** **the headroom was measured in
  a regime where the flake DOES NOT REPRODUCE (0/10 everywhere on today's disk and load).**
  **Measurements taken where there are no failures cannot explain failures in a regime we can no
  longer produce.** Rule 9 again, at the level of the whole measurement campaign: **the population
  sampled is "this machine tonight", and the phenomenon lives in "this machine that morning".**
  - **Concretely: on tonight's machine there is NO MECHANISM that reaches a 5 s timeout** — 8×O is
    0.6 s. That is consistent with 0/10 and says **nothing** about the morning, where 4/10 failed.
  - **Anyone reading "8× headroom" as "the storm is fine" is reading tonight's machine as though it
    were the one that flaked.**

#### D'S SCORING PACKET — his own headroom prediction FAILED by ~10×, and the bias pattern BROKE

**Measured (A, Run 2b):** opens/request **2.40–2.56**; median request-open **29.1–32.6 ms**; **max
single open 0.26 s**; O = **70–80 ms**; 8×O = **0.56–0.64 s** vs 5 s; headroom **≈8×**.

- **D-HEADROOM: FAILED.** He predicted O = 6–15 ms and 40–100× headroom. **Actual 70–80 ms and ≈8×
  — wrong by an order of magnitude.** Scored as a miss, at his insistence, not as a near-thing.
- **THE REASON, as its own hypothesis update:** he derived per-open cost from H's fsync figure
  (1.5–2.0 ms/op), **assuming fsync dominates. IT DOES NOT — fsync is ≈6% of a 30 ms open.**
  **H3 is substantially WEAKENED as the INTRA-OPEN mechanism.** The open's cost is structural: the
  full O(head) journal load, `validate_anchors`' ~10 handle opens, the directory scans.
  - **And the distinction he draws is sharp and correct:** this does **NOT** weaken the disk
    explanation for the RATE CHANGE — **a degraded volume slows all of that, not just fsync.** It
    weakens **fsync specifically as the term inside an open.**
- **D-P5-adjacent: CONSISTENT-NOT-TESTED.** Opens/request ≈2.5 fits his per-outcome table under the
  storm's mix (status 1, 409-precondition 2, fresh 200 3) — **but he never froze a weighted-average
  prediction, so he refuses to score it as a confirmation.** Correct.
- **Referral branch DOES NOT FIRE** (0.6 s vs 5 s); the C1-justified branch is **not reachable from
  rung A by construction.** Neither trigger fires, as the pre-registered table anticipated.
- **Failure-conditional rows (D5, D-P10, D-P10-ALT, leak-3 magnitude): STARVED, correctly** — no
  live failure existed to measure.

**THE BIAS PATTERN IS BROKEN AT FIVE, AND D ASKED FOR IT TO BE SEALED THAT WAY.** I had sealed the
direction as **checkable forward** — anyone producing v5 should expect the bias to run the same way
again. **It did not.** Four errors eased C1; **this one HURT it**, because a tighter headroom (8×
rather than 40–100×) makes serialization look **more** relevant, not less.

- **Scored as KILLED, not "mostly holds".** His words, kept: **a direction claim that survives only
  by ignoring its counterexample is worth nothing.** The claim was pre-registered as falsifiable
  and has been falsified — **which is the direction claim working, not failing.**

**THE TAIL FINDING — the lane's sharpest sentence, and I verified the arithmetic myself.** Max open
0.26 s is **8.5× the median**. For the 8-deep convoy to reach 5 s, **O must reach 625 ms, i.e. the
AVERAGE open across the burst must reach ≈250 ms — 0.96× TODAY'S OBSERVED MAXIMUM, SUSTAINED.**

- **So the machine sits ONE 8× DISTRIBUTION-SHIFT FROM THE CLIFF** — an ordinary shift for a
  degraded volume. **This does not prove the disk did it; IT PROVES THE DISK COULD, AT A PLAUSIBLE
  MAGNITUDE.** That is **the bridge the disk hypothesis lacked all day**, and the strongest thing
  this lane produced.
- **My quantification of his referral (M):** removing ONE open saves **≈30 ms per request** — but
  **≈244 ms off the 8-deep convoy's tail**, an 8× amplification. **The convoy is where an open's
  cost is spent, which is exactly why fewer opens beats more threads.**

**D SELF-DISQUALIFIES HIS OWN SUCCESSFUL FORECAST.** His no-referral prediction **held 10/10** — but
it was derived from a **40–100× headroom model**, and measured headroom is **≈8×**. He asks that it
**NOT be recorded as a successful forecast**, on his own principle: **a number that is right for a
reason its author does not have is not a measurement.**

- **Scored: RIGHT-FOR-WRONG-REASON, NOT A SUCCESSFUL FORECAST.** The prediction is insensitive to a
  10× model error **only because both values sit on the same side of the trigger** — **a
  near-boundary result would have flipped it.**
- **This is the mirror of his P7 move.** There he refused a convenient *explanation*; here he
  refuses a successful *forecast*. **Same principle, opposite direction, and the second is harder:
  declining credit for a prediction that came true.**

**CORRECTED TAIL FIGURES — H's co-signed numbers superseded, and the finding survives at a corrected
magnitude.** H's ×3 opens/request was **structural and ≈20% high**; measured is **2.40–2.56** (the
convoy arithmetic above already used 2.5, so it is unaffected). **Pooled n = 1521:**

| Scenario | 8-deep convoy | vs 5 s |
|---|---|---|
| all opens at MEDIAN | 0.61 s | 8× headroom |
| all opens at p99 | 2.81 s | still ≈1.8× under |
| all opens at MAX | 5.22 s | **crosses, by 4%** |

- **D's COMPOSITION BOUND, and I verified the arithmetic:** the flake needs a **burst-average
  ≈250 ms**, which requires **broad degradation — ≈20% of opens at ≈1 s** (checked: 20% at 1128 ms
  with the rest at median gives exactly 250 ms). **Thin tail-fattening is insufficient**: 1% of
  opens at 1 s moves the average to only **40 ms**, nowhere near 250 ms.
- **So the sharpened verdict is: SICK VOLUME YES, MILD PRESSURE NO.** Even **every open at the
  observed maximum** only crosses by 4%. **The disk hypothesis now requires a specific, broad
  degradation — not a few slow outliers** — which is a far more falsifiable claim than "the disk was
  worse", and it is the corrected form of the bridge.

**LANE OUTPUT, quantified: MEASUREMENT AND REFERRAL, as predicted.** Each open costs ≈30 ms of
**structural** work and **serializes on an exclusive lock regardless of threading**; there are ≈2.5
per request; **removing one open removes ≈30 ms of serialized time per request while parallelising
handlers removes none of it. FEWER OPENS, NOT MORE THREADS. C1 STAYS UNADOPTED.**

#### Run 3 disambiguated at the source — 3/3 PASSES, no observer effect

A settled it rather than letting me infer: **"test result: ok. 1 passed; 0 failed" ×3**, logs cited.
**No flake-suppression signal — and A names why it could not have been one: there is no live flake
to suppress** (0/10 everywhere on today's disk). Walls 17.6–19.6s, inside the clean envelope
16.8–19.0s.

- **The refusal to guess cost nothing and would have been decisive if it had gone the other way.**
  3/3 *failures* would have meant the probe's presence suppresses the flake — an observer effect
  undercutting every instrumented run planned. **Cheap insurance on a question that resolved
  benign.**
- **A corrected the notation at the source, not just the instance:** henceforth **"N/N pass" or
  "0/N fail" explicitly, never a bare "N/N"**. **Fixing the convention rather than the datum is the
  repair that survives the next reader.**

#### Run 2's not-results — cause recorded as its own class

**10 exit-101s, scored NOT-RESULTS** under the compile/harness cell (second live firing of that
cell). **Cause, two-side verified: the SPEC expected many pids while `create_new` implements ONE
pid — a self-contradiction INSIDE one author's own artifact**, fixed with a per-pid suffix.
**Same family as summary-rot but INTERNAL: the document disagreed with itself, and nothing
downstream noticed until the harness died ten times.** Headroom re-run pending; failure-conditional
rows correctly **STARVED**.

#### Superseded state, kept visible: WITHDRAWN BY ITS DESIGNER, PRE-RUN

- **D withdrew it himself before it ran**, on cost: checkout + rebuild + tree-break + disk. My
  sealed cells (0/10 → disk explains · 3-4/10 → code explains · 1-2/10 → uninformative · compile or
  harness → not a result) and both asymmetries are marked **WITHDRAWN-BY-DESIGNER-PRE-RUN**, with
  the reason attached. **Legitimate and the same class as D-P7's withdrawal:** pre-measurement, by
  the author, on stated grounds, with the row kept rather than deleted.
- **AUTHORSHIP CORRECTION, recorded because praise was landing in the wrong place:** the
  orchestrator states **they** spent the levers, not H. H's reporting discipline is not implicated
  in the mispricing.
- **A THIRD UNRECORDED VARIABLE: FLEET LOAD.** So D-META-3 generalises past the disk — **the lane
  recorded ZERO machine variables while testing a hypothesis about machine-bound cost.** Disk free
  space, fsync latency, and concurrent-cargo count were all uncaptured. **New required fields in
  every timing run table: free space GB + concurrent-cargo count.**
- **THE SUBSTITUTE ANSWERS A DIFFERENT QUESTION, AND THE ORIGINAL STAYS UNANSWERED (M).** The
  replacement is the **HEADROOM number** from the instrumented run — D1's own arithmetic doing
  double duty. But note what changed: **the headroom question is FORWARD** — *is this machine near
  the cliff?* — while the withdrawn control was **BACKWARD** — *did the disk cause the 4/10?*
  - **A substitute that answers a different question is fine; what is not fine is letting it close
    the first.** The historical question is now **UNANSWERABLE in the same permanent sense as C's
    #74 evidence**: the variable was never recorded at the time, and no later run recovers it.
  - **Sealed refusal: nobody may later write that the headroom number settled the disk question.**
    It cannot. The most that survives is my timeline-bounding route, which bounds and does not
    measure.
  - Status of the backward question: **RETIRED-NOT-ANSWERED** — retired by a cost decision rather
    than by a fix, which is a third way into that status.
- **D's standing caution seals as written and reaches no summary line:** **"the flake was the disk"
  is exactly as unestablished as "the flake is gone."**

### STARVED — a status distinct from UNSCOREABLE (D's distinction, adopted)

Under a 0-failure regime D splits his rows rather than declaring the program dead:

- **SCOREABLE ON PASSING RUNS (six):** D-P5 (**always passing-run-only by design — a 0-failure
  regime is its CLEAN case, not its starved one**), D-P5e, D-P6, D-P8 (**still the cleanest test of
  the serialization claim**), D-P9, and **D-FALSIFIER — arithmetic over median open cost and
  opens-per-request, both available from passing runs. IT CAN STILL FORBID C1.** D's own words:
  **a 0-failure regime does not rescue his preferred fix from its own pre-committed falsifier.**
- **STARVED (four):** D5 clustering; D-P10 and D-P10-ALT (both need a killed request); the leak-3
  magnitude (needs a child killed mid-sweep); H4's last corner (no panics at all, so the laundered
  sites never fire).
- **Why the status matters, and it is D's point:** **STARVED is not UNSCOREABLE-for-want-of-tagging
  and not "did not fire". Tagging is fixable by us; a missing failure is not.** Recorded as its own
  status so nobody later reads a starved row as an unpaid debt someone could clear.
- **So my "the mechanism may become undecidable by the planned route" was too broad:** it is
  undecidable **by the four starved rows**, while six — including the falsifier that can veto C1 —
  survive at a 0 rate.

- **Discipline, three for three:** own population only with **no pooling** against `53d212d`;
  duration reported and explicitly not interpreted; and **"no failure text exists" supplied as its
  own observation** rather than as a count — the ambiguous-absence repair, now unprompted on every
  run he has reported.

## PLUMBING VS SEMANTICS — the unification, and the honest bound on this whole file

**D's, offered as a unification rather than a fourteenth rule, since the count should stop
growing.** It organises most of this lane's instrument failures, and it deflates the program he
was loudest about.

**Every instrument failure today was SEMANTIC, and every one passed a PLUMBING audit first:**

| Instrument | Plumbing audit it passed | Semantic failure it had |
|---|---|---|
| D-FALSIFIER | I checked it COULD fire (inputs present) | never whether **firing meant what it claimed** |
| D's control 2 | checked it would PRODUCE a verdict | never whether the verdict **isolated the site under test** |
| D's control 1 | sabotage fully SPECIFIED | **structurally unobservable** — the path never executes |
| L's control-independence test | expectations well-FORMED | one expectation **supplied by the instrument it was meant to test** *(D's characterisation; not verified by me)* |
| D-P7's metric | the tag was WIRED CORRECTLY | it **measured a different population than its name** |

**Same shape five times: "THE INSTRUMENT IS WIRED UP" is a different question from "THE INSTRUMENT
ANSWERS THE QUESTION."** Plumbing gets audited because plumbing is checkable mechanically.
**Semantics needs someone to work the example.**

### The uncomfortable corollary — and it bounds everything this file produced

**EVERY MECHANICAL GATE BUILT TODAY TARGETS PLUMBING.** Grep for dead tokens; count begin/end
pairs; hash the ledger; refuse an existing sink file; require a free-space field. **Not one of them
catches a semantic error.**

**What actually caught the semantic errors, every single time, was somebody WORKING THE EXAMPLE:**

- A **running** the sabotage instead of reasoning about it;
- D **multiplying out** 8S;
- L **computing** C(7,4);
- me **reading** H's logs after asserting I could not.

**None of those is automatable, and none happened because a gate fired.**

- **So "treat the grep as a gate, not a habit" is right AND BOUNDED: the gates keep the artifacts
  CONSISTENT; they do not make them CORRECT.** D asked that the limit be recorded by him rather
  than discovered later by someone who trusted the gates to do more than they can.
- **Consequence for the owner report (M):** the rule appendix must not read as *"we now have gates
  that prevent this."* **It should read: we have gates that keep the record consistent, and every
  correctness failure was caught by a person working an example.**
- **CITATION IS NOT CORROBORATION lands here too, as a form of rule 10 rather than a new rule** —
  and D accepts the sharper half as the one with teeth: **the more valuable a row is believed to
  be, the more citations it collects and the less likely anyone is to be the first to check it.**
  His falsifier was called the best row on the board three times, **which is precisely why it went
  unmultiplied. PROMOTION IS AN ANTI-CHECK.**

## STALE-STATUS AUDIT — D's imperative-voice finding, applied to this file and it hit twice

**D closed a staleness hole in his own run spec and named the mechanism:** the spec was still
written **in the IMPERATIVE** — *"run this, then this, record these fields"* — describing runs that
were **complete**. Anyone reading it cold would see four pending runs and a lane awaiting
execution. He added a RESULTS section above section 0 with an explicit line that everything below
is the **executed protocol kept for provenance, NOT pending work.**

**His general form, which he explicitly declined to make a rule because it is merely true:**
**a spec written to be executed goes stale THE MOMENT IT IS EXECUTED — and the IMPERATIVE VOICE is
what makes the staleness invisible, because it reads as INSTRUCTION rather than as CLAIM, so nobody
checks it against reality.**

**APPLIED TO THIS LEDGER IMMEDIATELY, BY GREP, AND IT FOUND TWO:**

- **D-FALSIFIER read `Status: OPEN`** after it had been scored (does not fire, robust to any W).
  Corrected in place, with the correction marked.
- **D-P5 read `Status: OPEN in form 3`** after it had been scored CONSISTENT-NOT-TESTED. Corrected
  in place, with the correction marked.
- **Five other `OPEN` statuses checked and confirmed genuinely open** (C-P1, C-P2, C-R2, C-P3,
  C-P4 — none has been run).

**Why this is a distinct mechanism from the six summary-rot instances (M):** those were **claims**
that went stale. **These were STATUS FIELDS — metadata about whether a claim is settled.** A stale
status does not misstate a fact; **it misstates whether the file has an answer**, which is worse in
a document whose whole purpose is to say what is settled and what is not. **And a grep for a dead
claim's name would never have found it** — the words were all current; only the state was wrong.

## A-87-C1 — verified-prefix cache (#87 commit 1), SEALED BEFORE THE FIRST RUN

A registered five predictions ahead of his run (he is fifth in the slot). Setup, **binding as he
wrote it**: fresh-paired baseline in the **same session and disk** (storm/M09 numbers are premise,
never base), `free_gb` + concurrent-cargo per line, journals at ~100 / ~1k / ~5k, N≥10 per point,
**median AND p99 AND max**. *That setup exists because of what the storm lane cost.*

| Row | CONFIRMS | KILLS | UNINFORMATIVE / UNSCOREABLE |
|---|---|---|---|
| **P1** hot handle, quiescent: full=0, suffix=0, wall O(1) | both counters 0 at all sizes, wall flat, **positive control passed** | either counter >0 on a quiescent hot handle | **UNSCOREABLE without the control** |
| **P2** hot handle, 1 append: suffix=1, full=0 | suffix=1, full=0, wall O(1 line) | suffix≠1 or full>0 | append not observed to land between the two ops |
| **P3** serve: full loads ~2.5 → ~1 | measured **through serve** | full stays ~2.5 | **DERIVED-NOT-MEASURED if crate-level only** |
| **P4** SC7 exact budget-counter equality | exact equality **with non-zero budget consumed** | any difference | **VACUOUS if the fixture consumes none** |
| **P5** no error changes class | discriminants equal **across exercised error paths** | any differs | **VACUOUS if no error is triggered** |
| **P6 (mine)** hot state == from-scratch state | equal **by value** | any divergence | — |

- **THE STRUCTURAL WEAKNESS I FLAGGED: P1, P4 AND P5 ARE ZERO-OR-EQUALITY PREDICTIONS, AND ALL
  THREE ARE SATISFIED BY AN INSTRUMENT THAT NEVER MOVES.** A broken counter stuck at 0 confirms P1;
  a fixture consuming no budget makes P4 fire as 0 == 0; an unexercised error path makes P5 compare
  nothing. **Sealed requirement — one extra assertion per counter, not a new run: a POSITIVE
  CONTROL for each**, proving the counter/oracle can register the non-predicted value. **Without
  them those rows score UNSCOREABLE, not confirmed.** P2 partially self-controls (suffix=1 proves
  that counter moves); its full=0 half needs the same proof.
- **THE CORRECTNESS GAP NONE OF HIS FIVE COVERED, and it is the one that matters most.** He
  measures **speed** (P1–P3), **budget equality** (P4) and **error class** (P5). **Nothing asserts
  the incremental path returns the SAME STATE as from-scratch.** **A cache can be fast, count
  right, error right, and serve STALE DATA.** Hence **P6**, sealed before the run: hot-handle state
  must equal from-scratch state **by value, not by digest-of-digest**.
- **P3's instrument-scope problem:** it is a claim about the **serve path** while the setup is
  crate-level in `graphhelm-events`. **If serve is not exercised, P3 is a MODEL, not a
  measurement.** Also **~2.5 was measured as OPENS per request; P3 predicts FULL LOADS** — they
  coincide only if every open does exactly one full load, **which must be stated as an assumption
  or the comparison is against a different quantity than the one measured.**
- **His conceded UNINFORMATIVE has a consequence he did not draw (M):** at ~100 events the wall
  cannot separate hit from full, so **the COUNTERS carry the entire claim at that size** — which
  makes the positive controls **more** load-bearing, not less. At ~5k the wall is a second witness;
  at ~100 there is only one.
- **ALL SIX CLOSED BY A BEFORE ANY RUN — amended in the parked patch, re-parked, tree clean.**
  - **POSITIVE CONTROLS COMMITTED, not optional:** the P1 guard asserts `full_load_count >= 1` (the
    open itself moved the counter) **before** the delta-zero claim; P2 asserts `suffix >= 1`
    explicitly; P4 asserts `batches > 0 && events > 0 && work_units > 0` **before** the equality.
    **A dead instrument no longer confirms any of the three.** P5 gets its control free — the
    corrupt-suffix guard **exercises** the error path and compares hot-vs-fresh discriminants.
  - **P6 SEALED AND COMMITTED**, as the budget guard doubled into a state guard: **equality BY
    VALUE** across `batches`, `next_sequence`, `last_hash`, `seen_idempotency`,
    `reachable_evidence`, `artifacts`, `active_versions`, `expected_markers`, `verified_offset`.
    **The "fast, counts right, errors right, serves stale data" line now has an assert.**
  - **P3 RECLASSIFIED DERIVED-NOT-MEASURED at crate level, with the premise NAMED:** ~2.5 was
    measured in **opens/request** while the prediction is about **full loads**; they coincide only
    if every open does exactly one `load_state` — **structurally true today (`open_inner` calls it
    once), declared as a PREMISE, not as a datum.** The through-serve measurement moves to commit 2.
  - **Uninformative consequence accepted**, and it is why the controls are committed rather than
    optional: at ~100 events the counters carry the entire claim.
- **ONE QUESTION I RAISED ON P6's FIELD LIST (M), unresolved at seal time:** **is `batches` the
  batch CONTENTS or a batch COUNT?** If it is a count, **P6 compares an aggregate and can pass while
  the underlying events diverge** — which is precisely the hole P6 was added to close, one level
  down. *Assert at the finest grain the question has.* The other named fields
  (`reachable_evidence`, `artifacts`, `active_versions`, `expected_markers`) cover projection
  content, so the exposure is narrow — but a stale-content pass is exactly the failure P6 exists
  for, and a count would not catch it.
- **P6's FIELD QUESTION ANSWERED — and it caught more than it asked.** `state.batches` is **full
  CONTENT, not a count**: `Vec<PhysicalBatch>`, each carrying scope, stream, checksum,
  `request_digest` and a complete `Vec<EventEnvelope>` (kind, actor, sequence, `previous_hash`,
  `event_hash`). **Equality descends to each event's hash** — the finest grain the question has.
  P6's exposure is closed.
  - **AND THE QUESTION SURFACED A LATENT COMPILE-BREAK: `PhysicalBatch` DID NOT DERIVE
    `PartialEq`, so the P6 assert WOULD NOT HAVE COMPILED.** **VERIFIED BY ME in the unpatched
    tree:** `jsonl.rs:4` reads `#[derive(Clone, Debug, Serialize, Deserialize)]` — no `PartialEq`.
    A fixed it with one line in the parked patch (`EventEnvelope` and `RepositoryScope` already had
    it).
  - **What that is worth, concretely: the question converted a would-be NOT-A-RESULT into a working
    test before it consumed machine time.** P6 would have hit the compile-failure cell in the slot
    and burned a turn — on a lane where A is fifth in the queue.
  - **ONE INSTANCE, NOT A LAW (M) — but it qualifies D's unification usefully.** I asked a
    **SEMANTIC** question (*does this compare content or an aggregate?*) and it exposed a
    **PLUMBING** defect (a missing derive). The unification says gates catch plumbing while people
    catch semantics; **here a semantic question caught plumbing**, because asking what a thing
    MEANS forces you to look at whether it WORKS. **The two are not cleanly separable in the
    direction of inquiry**, even though they are in the direction of tooling. Recorded as one
    instance.
- **Status:** OPEN, sealed, unrun, controls committed, P6 compilable.

## Seal integrity

If any row's source file changes after this seal, the ledger is stale for that row and the
quote must be re-checked against the source before scoring. One such change already happened
during authoring: A's flake-2 plan gained branch A5 and settled A3's fix direction with J
after my first read, and both are reflected above. That is the failure mode this section
exists to catch — a verbatim quote is only verbatim as of a revision.
