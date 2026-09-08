> **OBSOLETE - DO NOT ACT ON THIS FILE (tombstone, 2026-09-08).** Everything below was written by
> sessions that no longer exist, during M09/M10. Its worker table, its addresses and its standing
> orders are all dead: a session name here cannot be messaged, and a task here has no owner. Authority
> now lives in the **execution records** (append-only, read by the owner in the Studio); the lane
> sequence lives in `.factory/lane-loop.md` and the press rules in `.factory/MERGE-CHECKLIST.md`.
> **Two rules that lived only here were moved out BEFORE this line was written** - `gh`/`git` bodies
> via `--body-file`/`-F` and `git add` per file - and are now in `.factory/MERGE-CHECKLIST.md`'s tooling
> traps. Nothing else here was found to exist nowhere else.
> Kept only as history - it misled a lane on 2026-09-08, which is why this line exists. A dead file
> nobody marked dead is a loaded trap.

# Orchestrator board — M09 stabilization

## FULL AUTONOMY (owner, 2026-08-19, sleeping): orchestrator authorizes everything incl.
## merges and paid runs; single rule = best harness system on the internet; loop until the
## whole MVP ships. ALL discipline rules stand — autonomy raises the evidence bar.
## #74 landing decision: (b) land-on-evidence, red stage named, NOT re-rolled; close-out
## gate re-covers before the main merge. Paid judge run: re-authorized, fires on C's push.

Updated: 2026-08-19. Branch `issue-m09-arming-the-alarm` @ 53d212d, tree clean.

## ONBOARDING — read this FIRST if you just arrived (you were not born knowing)
1. CHECK IN with the orchestrator session before touching anything. State: worktree,
   branch, HEAD, tree state. You get a role, files, and a base commit FROM the
   orchestrator — never self-assign.
2. NO CARGO (build/test/clippy) and NO codebase-memory indexing until the orchestrator
   lifts it. One cargo runner at a time on this machine; timing flakes are being measured.
3. TOKEN ECONOMY (owner order): caveman-full on ALL inter-agent messages and reports
   (if your session lacks the SessionStart hook, invoke skill caveman:caveman level full);
   after cargo lifts: restart session, prefer codebase-memory graph queries over
   file-by-file reads; agent-reach exe (~/.agent-reach-venv/Scripts/agent-reach.exe) for
   web/GitHub content; humanizer for owner-facing prose only. NEVER compress technical
   substance — verbatim failure text, per-run tables, exact commands are data.
4. ONE WRITER PER FILE. Check the table below before writing anywhere. New files go in
   .factory/<your-letter>-agent-*.md in YOUR OWN worktree unless told otherwise.
5. EVIDENCE RULES: causal claim = with-number + base-number + N + scope, or returned
   unread. Sabotage report = file:line of change + exact invocation + N + raw per-guard
   pass/fail list. Predictions about upcoming measurements get SEALED with M's ledger
   BEFORE numbers land — send M your rows yourself.
6. Merge, scope, priority, paid judge runs = OWNER only. Agent report never equals owner
   approval. GitHub Actions stays disabled — never enable/run workflows.
7. gh/git bodies via --body-file/-F, never inline (backticks execute). git add per file,
   never -A. Pair serve stays DOWN unless orchestrator says otherwise.

## Stash ledger
- `stash@{0}` — candidate-fix-flake3-atomic-conditional-append-UNPROVEN. STATUS RESOLVED:
  change A (append_atomic_if) DEAD by both readers' agreement (testability — see fix-phase
  plan); change B (shared-lock open fast path) PARKED with named hazard. Stash is
  HISTORICAL REFERENCE only now. Never pop it. Delete only after flake-3 PR lands.

## File ownership (one writer per file)
| Worker | Files | Branch | State |
|---|---|---|---|
| W1-measure | — | — | DEAD: stopped by owner mid-run (discovered via SendMessage error). Base numbers NEVER landed. Task inherited by H. |
| H Agent | .factory/h-agent-base-measurements.md (main checkout) | main @ 53d212d | BASE DONE (~13min, serial, binaries direct):
  STORM 4/10 isolated, 2/3 in-suite — ALL failures READ phase api_http.rs:464:34 (10060
  TimedOut), some +get_status :221:33, ZERO connect :443, ZERO write :460 -> H5 dead;
  H4 mutation-path-only dead (see RECONCILED FINAL FORM below — the authoritative line).
  SLEEPER 9/10 isolated, 3/3 in-suite — near-DETERMINISTIC red, ONE form: wake_http.rs:822:5
  "the lease burned on the ring" (lease live:true, lastConsumed:null, contentHead:15,
  cursor:13) -> A's mechanism FORK DECIDED: branch A1 (consume-append race, no wait between
  ring and receipt GET).
  SWEEPS 0/10 + 0/3 — NO repro at 53d212d in N=13. Flake-3 fix-gate has NO stochastic red
  at HEAD; C's deterministic seam red carries the whole burden. C-P1 (parent) untested.
  Suites: only flakes-under-study ever failed. H standing by for ordered runs only. |
| W2-doctor | — | — | DEAD: stopped by owner. Task inherited by K. |
| J Agent | NONE (new file .factory/j-agent-flake2-review-prep.md own worktree) | claude/j-agent-b9ad3f | Fable extra. Designated adversarial reviewer for A's flake #2 work; prepping independent sabotage candidates per mechanism branch. |
| K Agent | .factory/k-agent-doctor-journals.md own worktree | claude/k-agent-f9a127 | DONE queue item 4. Root cause: git cannot track EMPTY DIRS (.tmp/active) -> committed acceptance stores fail classify_layout (local.rs:2126-2128) BEFORE any byte is parsed. Content byte-intact (11/11 journals pass full independent Python verification incl. hash chain + requestDigest recompute, 165 batches 0 mismatch). Fresh clone fails THREE not two (demo dir too). .gitkeep measured DEAD (GHE007 — placeholder violates store contract). Decisive: mkdir .tmp+active -> opens, correct heads; control (.tmp only) still fails. RECOMMENDATION B: keep bytes, document tree as byte-archive, restore-line mkdir -p, ONE test copy->temp->mkdir->open->assert heads (turns citation into gate). Option A (re-record) destroys the record it defends. OWNER DECISIONS: (1) A vs B; (2) should classify_layout treat absent .tmp/active as recoverable (product promise change, local.rs:2153-2155 already creates them when partial). SIDE: m07/m08(+rejudge x6) journals are bare transcripts (no format.json/blobs), never openable — label where cited. NOW: typing option-B test + doc paragraphs PARKED (unbuilt) so a cargo slot can land them fast if owner picks B. |
| N Agent | .factory/n-agent-oracle-audit.md own worktree | claude/n-agent-0608e8 | Opus 5 med. ACTIVATED: ORACLE AUDIT of wake-family guards. Finding (C, ratified): "stream still replays" oracle sees only ILLEGAL-log failures; burning a live lease is LEGAL -> invisible. Classify every guard replay-legality / receipt-grain / mixed; per legality-only guard name one legal-but-wrong outcome it passes + minimal receipt assertion closing it. Banked greens are NOT evidence of absence for silent failures — OWNER decision on re-pricing earlier milestones. |
| W3-story | docs/ draft only (new file) | — | designing second judge story spec |
| A Agent | NONE (read only) | claude/a-agent-c35d10 worktree | DONE: M10 proposal at .factory/a-agent-m10-proposal.md (worktree). Owner's 48%/15->36ms/3x numbers have NO in-repo source (owner-measured off-tree) — M10 must reproduce base first. 3x confirmed STRUCTURALLY (driver.rs:66 = 3 load_state; each reloads+reverifies whole journal, local.rs:1074). Serve opens fresh repo per request (serve/mod.rs:106) citing an M05a claim now OBSOLETE (open_inner releases locks, local.rs:390-397). Recommends D1: incremental verified-prefix in-memory per handle + long-lived serve handle; D2 (disk snapshot) deferred. NOTE CONVERGENCE: per-request open is also D's storm-convoy driver — one mechanism, two symptoms. |
| B Agent | NONE (read only) | claude/b-agent-20b716 worktree | DONE: #55 memo at .factory/b-agent-issue55-memo.md (worktree). Verdict: widening does NOT reopen #55 strictly (CAS untouched, append still exclusive) BUT #55 never fully closed — middle window (between S2 validation replay and next_sequence pin, wake.rs:171→:190) passes both guards → double consume → GHE005 brick, exists in main TODAY. Proposed deciding test: rival-consume injection seam, N=50/cond, prediction: fails on efd85d0 AND 53d212d. Candidate fix converges with stash@{0} direction (decide+append single acquisition). NOW: cross-reviewing D's storm study. |
| E Agent | .factory/draft-second-story.md (sole writer) | claude/e-agent-138788 worktree | Review DONE, 2 CONFIRMED kills (verified by orchestrator): (1) hang self-heals — fixed 300s tool timeout (ports.rs:27) -> TimedOut -> RetryableFailure (executor.rs:241-243) -> auto requeue (MAX=8) -> attempt 2 sees marker, succeeds; judge realistically loses the race, miss is SILENT, 6/7 degrades to 4/7. (2) immediate pause does not kill child — cancel_all documented no-op (ports.rs:219-221), orphan lives to own 300s deadline. REDESIGN [APPROVED] by F (all 5 load-bearing claims verified file:line, arithmetic hand-re-derived, no findings): block device = fast identical failure x4 (deterministic Blocked), graceful pause via NotPaused refusal, approve-as-necessity, sync start, zero clock exposure. NEXT: free scripted rehearsal queued in machine queue AFTER flake-critical work; PAID judge run = owner authorization only. |
| C Agent | NONE (read only) | claude/c-agent-e82f40 worktree | DONE: wake-flakes study at .factory/c-agent-wake-flakes-study.md (worktree). Flake #3: rival consume between R2 validate-read and R2 next_sequence read -> fresh sequence, CAS misses, idempotency key differs -> double consume -> fold Corrupt (projection.rs:997). CONVERGES with B's independent #55 middle-window finding. Decisive evidence: failing run's journal shows two wake_lease_consumed for one lease. Flake #2: 4 candidate mechanisms, only failing assertion TEXT decides (W1 capturing). Stash@{0}: change A (append_atomic_if) SOUND by reading but bundles UNRELATED shared-lock-open perf change with its own hazard (recovery writes under shared lock) — SPLIT, land A alone. Registered prediction: flake #3 reproduces at parent of 53d212d. |
| D Agent | NONE (read only) | claude/d-agent-25b466 worktree | TWO-RUNG LADDER ready: rung A = per-open probe in event_store funnel (mod.rs:281-285, env-gated file probe, pid field mandatory — env inherits to child processes; NOT cfg(test), NOT stderr). Rung B (lock-wait/fsync/event-count split inside local.rs) SPECIFIED not diffed — local.rs pen is C's until flake-3 lands; rung B ownership decided AFTER rung A results. Clearance order approved: Step 0 (H's verbatim text, free) -> rung A -> decide rung B.
D's section (h): 9 numbered falsifiable predictions frozen with M pre-H (incl. P8
single-overlap falsifies serialization; P9 predicts A's O(history) in D's own data)
+ PRE-COMMITTED FALSIFIER against D's own preferred C1 — THRESHOLD CORRECTED BY D
(the original "S > 625ms" named the region where C1 HELPS; wrong for hours while three
of us cited it, nobody multiplied it out): let S = opens/request x median open elapsed.
S < 625ms -> C1 fixes nothing observable; 625ms < S < 5s -> serialization IS what crosses
the timeout, C1 justified; S >= 5s -> a single request's own cost exceeds the budget,
C1 CANNOT help. FALSIFIER v3 (L found v2's parallelism assumption false — the OPEN takes
a blocking EXCLUSIVE lock with no timeout, local.rs:2146 via open_inner:329; 53d212d
widened READS only, the open stayed exclusive): today the 8th request lands at ~8(O+W);
under C1 at ~8O+W — C1's ENTIRE benefit is ~7W and buys NOTHING on the open-dominated
fraction, which the mechanism map says dominates. v3 fires when 8O+W >= 5s (computable
from data already collected). LANE-OUTPUT CAVEAT (boarded next to C1 as D asked): if
opens serialize regardless of threading, REDUCING OPENS (C3/M10) beats parallelizing
handlers — the lane's correct output may be A MEASUREMENT AND A REFERRAL, not a patch;
the runs decide. TRAJECTORY ON THE RECORD: three wrong versions in one day, every error
easier on the author's preferred fix, each endorsed by someone before the next surfaced.
RULE ADOPTED (L): A FALSIFIER IS NOT REGISTERED UNTIL SOMEONE HAS PLUGGED NUMBERS INTO
IT ONCE, EVEN INVENTED ONES. L's DEEPER REFINEMENT: with_lock takes the FILE lock per
operation (exclusive for appends), so part of W serializes too — C1's benefit is
~7*W_free, and the storm is mutation-heavy exactly where the serializing share is large.
PRE-REGISTERED SCOPE RULE v2 (L caught v1 pinning its discriminator to an instrument that
cannot collect it — rung A has NO lock-acquire timings; append-held time is rung B):
(a) NARROWED to what rung A measures: IF opens-per-request x median open elapsed x 8
alone crosses the 5s budget -> lane closes as MEASUREMENT + REFERRAL to M10's
open-reduction, no serve patch (sufficient: if opens alone exceed it, no handler-threading
change helps). (c) THE RULE FIRES ONE-DIRECTIONALLY, stated: it can reach "referral" but
CANNOT reach "C1 stays" — W_free computed as (total - opens) silently absorbs the
unmeasured append-held term and reads HIGH, biased toward keeping C1 (the same direction
the falsifier erred three times); a residual is not a measurement. "C1 stays" requires
rung B's measured lock-mode split (deferred, C's coordination) — W_free must be MEASURED,
never subtracted. (b) the lock-mode timer folds into rung B's spec if it ever runs. (L's gate 5 — mutation tail latency, never /health — was
pointing at this mechanism before the arithmetic existed.) Full analysis procedure
pre-registered in shared spec section 4b before any data. TIMING AMENDMENT (D caught, adopted): rung A does NOT get its own pre-fix run — sweep-share
rows EXPIRE when C's flake-3 fix changes sweep behavior. Rung A probe applies AT the
"re-measure storm baseline" step post-flake-3: baseline + histogram in one run against
the code the storm fix will actually sit on. H's current run stays clean baseline. Older: Instrumentation READY unapplied: Patch 1 (api_http.rs per-phase timing, phase from WHICH call errored, no connect_timeout added — deliberate), decision table in report. Patch 2 (sweep-disable H2 discriminator) needs C coordination + my go (breaks doorbell by design). Patch 3 (fsync) deferred to open() distribution. DONE: storm study at .factory/d-agent-storm-study.md (worktree). H1 = 5s client read timeout vs single-thread convoy (serve = current_thread runtime, sync IO inline; ~4 full store opens per mutation, each open = excl lock + O(head) load + fsync; 10060 covers BOTH connect and read timeout on Windows). Deciding measurement: phase-logged client (connect vs read) + 30s-timeout corroboration run. Bonus: ServeState.events doc comment justifying per-request open contradicted by code (open_inner unlocks before return) — handle caching is candidate fix direction if two-process probe confirms. AWAITING W1 clearance. |
| F Agent | NONE (read only; new file .factory/f-agent-m09-close-skeleton.md in own worktree) | claude/f-agent-8dc3ee worktree | NOW: M09 close-out skeleton (claims vs in-repo evidence vs owed holes). STANDBY: adversarial reviewer of E's story-spec redesign. Tier: Sonnet 5. |
| L Agent | NONE (.factory/l-agent-storm-review-prep.md own worktree) | claude/l-agent-100f72 | PREP DONE, 1 KILL on rung A design (pre-run, routed to D): thread-id caller split conflates DRIVER spawn_blocking opens (driver.rs:220-221/:274-275/:308-309; ~12 storm resumes x N opens) with sweep opens -> sweep share reads high by driver fan-out, could wrongly select C4. Repair: thread_local caller label wake.rs:84/:140. + 8 probe gaps (G1: blocked open writes NO row — blind exactly where H1 lives; begin/end pair, begin-without-end IS evidence; G5 fresh path per run; G4 fail loud). DENOMINATOR RULING (L found, adopted): the ~25s wall clock is DISQUALIFIED as denominator
(contains cli_start + server spawn/health poll + verify's two replay processes,
api_http.rs:1646-1652) — a corpse's surviving denominator would dilute serial share and
could wrongly REJECT C1. Binding: "share of wall clock" is scoreable ONLY against the
storm phase, bounded by probe rows (first-to-last caller=request row of the SERVER pid).
This promotes gap G2 (epoch clock, not per-process Instant) to REQUIRED.
M's rule extended: a retraction's descendant can be a DENOMINATOR inside a reading
instruction, not only a claim or experiment.
DENOMINATOR REFINEMENT (D, adopted): replacement denominator itself mildly contaminated —
verify's all_events HTTP reads add <=2 trailing caller=request rows to the server pid;
phase end = last row before that trailing pair; REPORT residual bias with the number,
never absorb it. (4th surviving descendant of the T1 retraction.)
LEAK-AS-INSTRUMENT (L proposed, D's call): on a failing run, (sweep rows - observed 200s)
counts mutations committed server-side while the client timed out — free H1 evidence,
independent of phase timing.
STEP 0 CLOSED (D): read starvation confirmed — all panics :464, zero :443/:460, 10060 only,
10061 nowhere. RECONCILED FINAL FORM (quote this one): H5 DEAD BY DATA both paths (code survives
laundering; all 10060, no 10048/10055). H4 DEAD ON MUTATION PATH ONLY (zero :443 among 16
attributed); NOT dead on status path (phase laundered; its 10061-premise never exercised —
zero connect failures captured to test it). Any flat "H4 and H5 are dead" line is STALE.
PATCH 1 BOUNDED TIGHTER (L corrected D's unit, D verified — the flake's unit is the RUN,
not the panic): the four :221 hits sit inside three runs that EACH also carry a :464, and
the other three failing runs have no :221 — so H4 EXPLAINS ZERO RUNS whatever Patch 1
finds; its corner is empty. Patch 1's entire remaining justification is D2's survival.
PRIORITY REVISED (D corrected himself UPWARD — first upward correction of the lane):
PATCH 1 AND RUNG A RUN TOGETHER in position 5. Reason: the storm-phase denominator is
computed from the probe's OWN rows = SELF-ASSERTED — if the probe dropped opens, nothing
in its own data could reveal it, and P6 + C1's selector are both scored against it.
Patch 1's client-side timestamps are a DIFFERENT APPARATUS: first client request must
precede first server open row, last response must follow the last — disagreement means
the probe drops rows. Patch 1 = the independent clock that validates rung A's denominator
(not just D2's settler). If machine time forces a choice, rung A alone — but any
share-of-phase number then ships WITH "self-asserted denominator" named, never quietly.
POSITION-5 SPEC FINAL (D+L): rung A + Patch 1 together; wall-clock epoch stamps = ONE
requirement across BOTH patches (two separate notes would let one apply without the other
and the cross-check silently degrades to unevaluable); cross-check = TWO tests, neither
substituting: EDGES bound the span (catches truncation/shift), COUNTS detect drops (a
mid-run drop leaves edges intact — only client-vs-server counts by outcome catch it; P5
identity's second job). CAVEAT RE-GRAINED (D, own honesty note failed the dimension test): completeness is
limited by CONDITIONS, not sample — every plausible drop mechanism is load-dependent, so
a passing run cannot certify a failing one EVEN IN PRINCIPLE; spec reads "completeness
established under a load regime that EXCLUDES the suspected drop mechanisms".
THIRD APPARATUS (free — the preserved events dir already required for D-P10-ALT):
committed events are durable, independent of client codes AND probe rows; on failing runs
caller=request rows must be >= 3x committed storm decision events (shortfall = direct
drop evidence). FALLOUT: LEAK 3 NOW MEASURABLE — committed decisions minus caller=sweep
rows = sweeps killed before running; and D-P10 simplifies to committed - N(200), store
+ client only, sweep out of the calculation (D-P10-ALT corroborated from a second
direction pre-run). VERIFICATION TRIANGLE: probe rows (under test) / client codes
(independent; passing runs) / committed events (independent of both; works where the
flake lives).
STALE-DIFF SWEEP (D, one slot before application): prose had accepted every correction,
the DIFFS still carried the superseded designs — incl. a draft rung A that would have
reinstated everything the review killed (fenced with DO-NOT-APPLY banner, kept visible),
eprintln sink, and-then borrow risk, no tempdir preservation, AND a nobody-spotted killer:
Patch 1 stamped per-process Instant vs rung A's epoch micros — two clock origins, the
client-clock cross-validation (the reason they share position 5) WOULD NOT HAVE WORKED.
All fixed; clocks now both epoch with the reason written inline. New clause: when a NEW
requirement is adopted, re-derive the artifacts that must satisfy it.
Control-independence audit (L's test, D applied): exactly-one INDEP (status.rs:24),
forced-sweep INDEP (wake.rs Ok-arm), sweep-rows-vs-200s INDEP (client apparatus),
denominator NOT INDEP -> closed by Patch 1.
LANE METHOD HEADLINE (L's closing line, for the close-out doc): CHECK THAT THE DIMENSION
A FINDING WAS EARNED IN IS THE DIMENSION IT IS BEING SPENT IN. Four of this lane's errors
were that one shape. It is the mirror of flattening and HARDER — the finding is TRUE,
only its jurisdiction is wrong; a false claim gets argued with, a true one gets waved
through. RUNG-A CAVEAT (D, against his own instrument): built-from-our-own-repairs
protects against failures already seen — it is NOT a correctness claim; only the
known-count controls test it against reality. Never copy "built from our rules" forward
as "right".
COPY-FORWARD GUARD (D, keep these sentences adjacent): "16/20 unambiguously read-phase"
is a statement about PHASE, NOT MECHANISM — H1, H2 and H3 ALL predict read-phase death,
none is favoured by it. H1 vs H2 vs H3 remains ENTIRELY OPEN; rung A alone separates them.
PATCH 1
CONSTRAINT (M, binding): the rewrap MUST keep the original error's Display embedded —
H5 died because the OS code survived into panic text; reformatting later would
retroactively destroy that evidence. (Also: rewrapped error has no raw_os_error() — Custom
returns None on exactly the path that matters.)
SCORING RULE 8 (D, from his own P4 loss): a rate prediction must name the N that would
falsify it, or it is nearly free.
SCORING RULE 7 — FLATTENING (L generalized, M adopted after it caught M's own verdict):
if a value a verdict rests on can be produced by more than one upstream cause and the
consumer treats it as one, the row is UNSCOREABLE regardless of the number. The loss is
the DISTINCTION, not the signal. Four instances, three lanes; HALF ARE INSTRUMENTS
(get_status laundering already cost a wrong verdict; rung A's ok=bool) — a flattening
instrument produces a CONFIDENT WRONG READING, strictly worse than no reading.
CAPTURE-RUN SKIPPED (A+J joint, authorized): zero-behaviour-change claim now carried by a
WEAKER instrument (red k>=2 + post-fix 0/20 + sabotage set), with-numbers compare against
H's clean base with instrument difference UNQUANTIFIED — labeled as such, never as
"validated". D-P10-ALT blocked until Patch 1 preserves tempdir (panic destroys its own
evidence). :221 finding DOWNGRADED (D self-corrected via L round 4): :221 is phase-LAUNDERED, rows
compatible with both read-expiry (H1) and connect-death (H4-on-status-path) — cannot draw
the H1 inference AND count the rows against H4 from the same unknown. H5 dead by DATA both
paths (code visible even where phase is not: all 10060, no 10048/10055). H4 dead by DATA on
mutation path (zero :443), by ARGUMENT only on status path. PATCH 1 RE-PROMOTED (D's own
pre-registered rule: text named get_status 4x -> Patch 1 becomes phase-deciding); D2
conditional on its result.
INSTRUMENT REPAIRS (L found, D verified, adopted): tempdir preservation via into_path()
UNREACHABLE (panic unwinds past it) -> catch_unwind(AssertUnwindSafe) + preserve on Err +
resume_unwind (keeps panic file:line); path printed+preserved on PASSING runs too (no
baseline otherwise). Phase-end rule TWO ARMS (verify reads exist only on passing runs —
stripping them from failing runs truncates real storm rows). D-TRAP-1 extended: request
POPULATION differs per run (panicking thread stops issuing) — normalize on requests
actually issued (from probe rows), never on runs.
D-TRAP-1 (measured; D self-corrected the first form): run duration CANNOT DISTINGUISH
pass from fail IN EITHER DIRECTION (isolated fails mean 23.3s, passes mean 25.7s, ranges
overlap) — unusable for attribution, not merely biased. A real fix still lengthens runs.
D's discrimination plan (report §k): D1 DECISIVE = max single-open elapsed vs 5s budget
(far below 5s while requests exceed it -> queueing proven, H1; near seconds -> H3 binds
and the pre-committed falsifier FORBIDS C1). D2 free: :221 one-open status timeouts tilt
H1 now. D3: sweep share bounds H2 as amplifier. D5: failure time-clustering under H1.
D6 DROPPED (read: 15 sequential rounds, 2 racing clients, 600ms drains, subprocess per
round — longer wall, lighter contention; says nothing about 8-way H2). BYPRODUCT better
than the lead: the STORM ARMS NO LEASES, so its sweeps take the CHEAP path (one open,
due empty, early return wake.rs:126; phases 2-3 never reached) -> H2's maximum effect is
CAPPED at one extra open per successful mutation, read off code not modeled. D-P10 folded one-directional (>0 confirms commits-past-
timeout; ==0 uninformative); D-P10-ALT preferred (count store events post-fail, leak-immune,
needs tempdir preserved on unwind).
PRODUCT HOLE (C claimed, N verified at branch): wake_wait NEVER consults the receipt —
zero wake_last_consumed mentions in wake_wait.rs; lease read once at :116 before blocking;
timeout answers from lease alone. On timeout the waiter cannot separate "nothing happened"
from "burned as rung". No test can close it; the SURFACE never computes the distinction.
OWNER/PRD decision.
CHAIN VERDICT (N, from stacking inventories): #56's own commit says the belt never
reproduced the race — the deterministic red is the proof; that red cannot separate "guard
dropped it" from "recorder died first" (11/12 return-0 sites unrelated). The chain proving
#55 has NO link that fails when the recorder dies. Neither guard worthless; the CHAIN is.
(C's fix adds the rival-burn-as-last-word assertion, closing this.)
SF RULINGS (M asked): SF-1 (legality-blind oracles) lands NOW in three places — fix-phase
requirements (receipt-grain assertions, belt upgrade, RC1 helper, RC2 positive controls),
owner report (banked greens are not evidence of absence for silent failures), N's per-guard
closing assertions = reference doc for all wake-family PRs. SF-1+SF-2 PAIR ratified as
pattern (milestone's subject inverted: silence indistinguishable from loss, twice) — goes
in owner report + M09 close doc.
BARE-TRANSCRIPT LABELS (K, drafted+parked in §5 of k-agent-option-b-drafts.md): 3 prose
sites + 8 per-directory READMEs (the site that reaches the reader who opens the dir).
Label sentence fixed verbatim. Distinction explicit: m05/m06 = stores repairable by mkdir;
these 8 = transcripts, missing blobs were never recorded — label IS the remedy. OWNER
CALLS: (1) site 1 edits shipped CHANGELOG.md:5 — drop if changelog frozen (sites 2+4 carry
it); (2) finding: m08-run-2026-08-18 is cited by NOTHING (8 committed dirs, one referenced
by no document).
K's #59 PRECEDENT: option B is not new — replay_demonstration_store (acceptance-map
lib.rs:194-203, commit 20c341d, issue #58/#59) already restores empty dirs before opening.
B = applying #59's own decision to the binding that never got it. m05-run bound only as
artifact (never opens); m06-run bound by NOTHING (prose only) — owner items sharpened.
MILESTONE VOCABULARY (N+E converged, independent subsystems; orchestrator ratified):
IN A SELF-REPAIRING SUBSYSTEM, AN END-STATE ORACLE MEASURES THE REPAIRER, NOT THE FAULT.
Self-repair converts a fault into a silence; every end-state oracle reads that silence as
success. Instances: wake sweep (next round repairs a consume-nothing round — belt end
state clean), executor retry (hung tool self-heals, judge's miss silent). Predicts where
to look: ANY retry/requeue/sweep-again/next-append-picks-it-up component cannot be tested
at end state — assert per-event/per-window. C's REFINEMENTS adopted: operative condition
is repair latency < observation window (actionable: observe FINER than repair latency);
blindness is ASYMMETRIC (healer faults DO go red — oracle measures one component while
appearing to measure the system, which is why it survives review); green end state =
evidence for the disjunction only. EVIDENCE STATUS: wake instance source-verified;
executor instance convergent-PENDING-VERIFICATION (nobody checked E's leg — not settled).
CITED COST INSTANCE (C, against own commit): the window-3 fix ADDS self-repair surface
(benign drop) — its drop path is now end-state-untestable by design; per-event only. Generalizes: legality-shaped guards are one
instance; self-repair is the mechanism that makes end-state oracles lie.
THREE FINDINGS, ONE SHAPE (C): rdv-equal burn + oracle blindness + ring() error flattening
all share it — this subsystem's failures are LEGAL-AND-SILENT by default, and the guards
are LEGALITY-SHAPED by default. Strategic input to N's oracle audit + owner report.
Storm lane review contract = L's 5 landing gates (probe separates sweep/driver; probe records blocked opens; no handle caching without measurement; raw sabotage evidence; C1 guarded on MUTATION TAIL LATENCY not only /health — C1 relocates the convoy onto the exclusive open lock local.rs:2146, /health can go green while the 10060-producing number never moves). SURVIVOR HUNT rulings adopted: Patch 2 STAYS as fallback discriminator until caller= label exists ("redundant" was a descendant of the retracted pool==sweep claim); Patch 1 has unverified borrow risk (compile-check before apply, never on faith) + its own sink contradiction (eprintln vs file); ORDERING: H's base failure text is captured and INTERPRETED before Patch 1 ever applies (Patch 1 shifts the :443/:460-464 line mapping Step 0 depends on). |
| M Agent | .factory/m-agent-*.md own worktree | claude/m-agent-76c232 @ 53d212d | LEDGER SEALED pre-H (.factory/m-agent-prediction-ledger.md): 17 rows + 2 scored + 2 traps. Key seals: B-P3 expected-UNINFORMATIVE (no planned instrument can measure hit-probability — pre-registered refusal for the future "widening raised the rate" sentence); B-P1/C-P2 = same seam, NOT mutual corroboration (one result must kill both, else a row is mis-specified); D-T3 scores ZERO if measured pre-flake-3; A5 = diagnosis-only (names harness, not mechanism); A3 exchange-counting KILLED unless shipped with connect-only-poll sabotage red. J pinged to seal own rows before H posts. NOW: convergence refutation from code paths no report touched. |
| G | not yet checked in | — | Planned: clerical only (Haiku). |
| (unassigned) | fix work for flakes 1–3 | — | BLOCKED on W1 base numbers |

## Tooling (owner-ordered, 2026-08-19)
- caveman plugin ACTIVE — all inter-agent messages/reports in caveman-full.
- codebase-memory-mcp v0.10.8 installed user-level; loads on session restart.
  Do NOT index while no-cargo rule stands (CPU load vs timing flakes).
- humanizer plugin installed — owner-facing prose only.
- agent-reach v1.5.0 at ~/.agent-reach-venv/Scripts/agent-reach.exe (direct call;
  --system activation permission-blocked, not worked around).

## Rules in force
- DISK GATE (added after F: hit 335 KB free mid-milestone): whoever takes a cargo slot
  checks free space FIRST (Get-PSDrive F); under 10 GB free, report instead of building.
  target/ caches are regenerable; fossil-worktree caches get deleted by the orchestrator.
  (2026-08-19: milestone-4 target 15.6 GB deleted; C's own 5.3 GB cleaned — SPENT, cannot
  be spent twice; main checkout target kept warm for the gate. F: at 19.22 GB free.
  REMAINING LEVERS if under 10 again: main checkout 22.7 GB (costs next gate a cold
  rebuild — scheduling decision), a-agent 1.8, k-agent 0.9. Decide BEFORE tight, not
  during.)
  TRANSIENT EXHAUSTION OBSERVED (K, during #75 run): os error 112 with 19 GB showing free
  immediately after — builds spike temp usage; a mid-run 112 can MASQUERADE AS A FLAKE.
  BINDING for position 5 (storm re-baseline): free a-agent + k-agent caches first (lanes
  closed by then), verify >=15 GB free, and H checks disk before EACH measured run.
  OSCILLATION RULE (K): the drive oscillates between exhausted and ~19 GB under load —
  cleanup is judged TWICE, SPACED, UNDER LOAD; one green reading proves nothing; a
  passing retry does not prove the earlier failure spurious. TIMELINE CLEAR: all counted
  Ns (H base, H 0/10s, A N=20s) completed BEFORE the first observed 112 — no trusted
  number spans the window. K's 0.9 GB target: deletion accepted.
- PERMISSION-BOUNDARY RECORD (H flagged, orchestrator ruling on the record): routing a
  denied action to another session is ADJACENT to permission laundering and was examined,
  not assumed. Ruling basis: A's cargo access is standing owner-granted session config
  (built all day, owner-visible); H's denial was one session's auto-mode classifier
  heuristic (same command class allowed 3x earlier in H's own session — trigger plausibly
  the modified-production-tree context), not an owner policy. A decides under its OWN
  rules with no pressure to proceed; if A's classifier also denies, FULL STOP, the storm
  waits for the owner. The tie-break belongs to the owner and this record surfaces it for
  their waking review.
- DISK LEVER RULE (C, adopted): the cheapest lever is any WORKTREE (not just its target/)
  whose commits are ancestors of origin AND whose artifacts exist in shared .factory/ —
  mechanically checkable (git merge-base --is-ancestor <tip> origin/<branch> + ls), no
  3am judgement calls about who seems idle. Spend fully-replicated worktrees before
  touching any lever whose work is not on origin. (c-agent-e82f40 currently qualifies
  WHOLE: ~2x reclaim at the same one-cold-build price.)
- SHA-PINNED APPROVALS (B's framing, standing rule): a reviewer's approval is pinned to
  the approved hashes. A merge preserves them and the approval SURVIVES; a rebase produces
  different hashes and VOIDS it by the approval's own terms — re-review required. Refusing
  to rebase approved work is not caution; it is the approval's contract.
- PRE-EDIT ANNOUNCEMENT (adopted after C's self-reported near-miss — pen rule held by
  LUCK not enforcement): any edit to a file in the MAIN checkout or shared branch requires
  a one-line pre-edit announcement to the orchestrator naming the pen row that licenses
  it, BEFORE the first keystroke. Imperfect (still compliance-based) but forces the
  ownership check at the moment it matters. Honest status: a mechanism-shaped rule, not
  yet a mechanism.
- BOARD ROWS ARE NOT COMMIT STATES (M, after the orchestrator's stale @53d212d row handed
  A a wrong premise as fact): nothing on this board is read as a commit state without a
  rev-parse at the point of use. The board is a map, not the territory's git log.
- A COPY IS A CLAIM UNTIL RE-VERIFIED (M): a file under active edit silently invalidates
  its own backup; re-copy+hash on further edits, and a session ending without it means
  the shared copy is authoritative-as-of-its-hash — correct failure mode, no surprise.
- CHANGED PATHS, NOT CHANGE CATEGORY (D, after "test-only" nearly cost a storm slot):
  when a downstream lane anchors on files, the board records the CHANGED PATH LIST
  (diff --stat), never a category like "test-only" — a category is a summary, and the
  #72 case put +17 production lines exactly inside a downstream anchor block while
  wearing the test-only label. (Orchestrator propagated that label to H; D's
  verify-not-inherit caught it.)
- NAME YOUR WAIT (N, after losing an hour to a brief that never arrived): if you are
  holding on ANYTHING from another agent, say so to the orchestrator the moment the hold
  starts — "blocked on X's Y" costs one line and gets unblocked in one line. Silence is
  not a queue.
- GREP-AS-GATE (D, 4 sweeps/4 catches/0 by re-reading): after ANY retraction or
  correction, grep the dead claim's name in every artifact you own — executed like a
  test on every document change, never trusted as internalised. Applies to this board.
- SABOTAGE EVIDENCE RULE (N proposed, orchestrator adopted board-wide): each named
  breakage runs INDIVIDUALLY; its report must carry the sabotage applied (file:line +
  what changed), the exact cargo invocation, N, and the raw per-guard pass/fail list.
  "Guards went red as predicted" with no per-guard list is returned unread.
- TOKEN ECONOMY (owner order): caveman-full on all inter-agent traffic; codebase-memory
  graph queries instead of file-by-file reads once cargo clears (restart session first);
  agent-reach exe for web/GitHub reads; humanizer owner-prose only. Forbidden trade:
  dropping technical substance to save tokens — compress fluff only.
  Compliance confirmed: A, B, C, D, E, F (explicit 4-point); J/K/L/M/N (rules acked at
  check-in); G (onboarded with rules); H confirmed (caveman hook-loaded, level full).
  ALL 13 CONFIRMED.
- No cargo builds/tests by anyone except H (sole cargo runner) while base measurement runs.
- Base = clean HEAD 53d212d. Any causal claim needs: with-number, base-number,
  N, scope. Missing any → returned unread.
- PR #70 MERGED by owner -> main = efd85d0 (verified via gh + git fetch 2026-08-19).
  Branch issue-m09-arming-the-alarm does NOT contain it; rebase decision pending.
- Merge/scope/priority = owner only.

## Model tiers (owner, 2026-08-19; expanded to 13)
A = Fable med · B = Fable med · C = Opus high · D = Opus high · E = Sonnet extra ·
F = Sonnet 5 · G = Haiku · H = Fable extra · J = Fable extra · K/L/M/N = Opus 5 med.
Routing: hardest reasoning/adversarial review -> H/J then A/B; implementation -> C/D
then K/L/M/N; mechanical verification/docs -> E/F; G only for clerical/status tasks.
BENCH IS FINE: more agents than parallel lanes right now — idle beats conflicting.

## Sequencing ruling (B proposed, D concurs, orchestrator ACCEPTED)
Storm fix and flake-3 fix are COUPLED via the store handle (cached/shared handle makes
operation_gate load-bearing, expires 53d212d's RwLock measurement). ORDER: (1) C lands
flake-3 fix (wake.rs only, no lock-structure change) -> (2) re-measure storm baseline ->
(3) storm fix. Otherwise a storm-rate change cannot be attributed.

## Ledger practice upgrade (C registered against himself, adopted)
Kill bars phrased as RATE-EXCLUSION BOUNDS, not absolutes: "zero in N>=10" sounds like it
establishes zero but only excludes >~26% (rule of three). Honest form names the excludable
rate ("under 5% needs ~60 clean runs") or is framed as a bound. M co-holds kill-bar WORDING
with the author at sealing time — authors apply the standard to others and miss their own.
The kill still stands as sealed (no moving bars after results). NAMED FORM (M, from
scoring C-LC): an OBSERVATIONAL TRIGGER FOR AN INFERENTIAL CONCLUSION — legitimate seal,
fires honestly, licenses less than its own sentence claims. Verdict split: ROW KILLED
(trigger observed), CLAIM NOT REFUTED (true rate below ~26% produces 0/10 routinely) —
mechanism question OPEN, delay hook decides. Deliver both halves, never as one thing.
C-P1 footnote carries a WAKE-UP CONDITION (demote with the wake-up written down, never
drop-and-forget): if any lane ever proposes REVERTING the shared-lock widening (53d212d)
as mitigation — storm lane is where it would come from — the "reverting restores nothing"
clause becomes live and C-P1 is what answers it. Also recorded: registered
predictions living ONLY in ledger+messages (not durable docs) is the system working —
C's killed claim had propagated NOWHERE (4 artifacts + issue #71 swept clean; one study
hypothesis paragraph marked SUPERSEDED). Seed 9's lesson from the other side.

## Fix-phase pen plan (pre-assigned, ACTIVATES only after W1 base numbers)
- Flake #3 (concurrent_sweeps): C WRITES (owns wake.rs + local.rs). DESIGN MOVED OFF
  the stash: B's smaller pin fix (pin sequence from G1's own history read; verified
  next_sequence == max+1, local.rs:1372-1377/:1106-1111) beats stash change A on
  TESTABILITY — under append_atomic_if the seam dies inside the exclusive acquisition
  and the deterministic red test cannot model the race; under the pin fix the seam
  survives and sabotage (revert pin to second next_sequence read) turns it red.
  Stash may go entirely UNUSED (fine). Stash history note wrong — must not teach that
  reverting 53d212d restores safety. B APPROVED invariant (amended: names BOTH blades).
  RED TEST TYPED in C's worktree (wake.rs +144, seam = &dyn Fn() after still-live filter
  before sequence pin; unbuilt, unrun). Red prediction on record incl. kill condition.
  Doc comment wake.rs:146-149 left deliberately false — flips WITH the fix commit only.
  HANG-IS-A-FINDING (B+C agreed): if the red test HANGS instead of failing, that refutes
  B's lock reading and is REPORTED as a result — never patched with timeout/sleep.
  C is FIRST in cargo queue at H's report: red observation before all else.
  Stray zero-byte file `1` in C worktree root = shell-typo junk, harmless, left in place.
  Open item (B's call as reviewer): rendezvous-equality filter is the only defense
  against burning a re-armed lease and NO test pins it — this PR or a seed. B is adversarial cross-reviewer —
  B independently derived the same window without reading the stash, so B's review
  checks C's fix against an independently derived invariant. B's seam-test prediction
  (fails at BOTH efd85d0 and 53d212d) becomes a review gate. Extra base: C's registered
  prediction (reproduces at parent of 53d212d) gets measured then too.
- Flake #1 (storm): D writes, B reviewed the study (4 findings): T1 arithmetic partly
  invented (no round barrier exists; 600ms chosen to fit; do not anchor fix on it);
  T2 phase attribution already FREE via panic file:line (connect api_http.rs:443,
  write :460-461, read :464) — W1's captured failure text may kill H4/H5 with zero
  new runs; loopback connect cannot realistically 10060 (backlog -> 10061), H4 lower;
  T3 stale doc CONFIRMED, but cached-handle fix interacts with operation_gate mutex —
  becomes new bottleneck if handlers also go multi-thread; T4 D's phase-log CANNOT
  discriminate H1/H2/H3 — replace three ablations with ONE per-open timing histogram
  split by caller (request path vs sweep) giving fsync/sweep/serial shares at once.
- Flake #2 (a_sleeper_wakes): A writes, J reviews (prep done: .factory/j-agent-flake2-review-prep.md,
  independent-then-diff). PEN RULE: serve/wake.rs is C's until flake-3 PR lands, then passes
  to A; A parks wake.rs drafts in .factory/*.rs meanwhile. wake_http.rs = A's throughout.
  FLAKE #2 EXECUTED (A, branch issue-19-flake2-sleeper-receipt-wait @53d212d, 2 commits
  c9c1188 capture + dca0461 fix, unpushed): TDD red 2/2 on capture tree (same form as H's,
  :822 pre-fix — JP3 tripwire satisfied, instrument question stays closed); post-fix 20/20
  standalone vs base 9/10 = decisive; S10 connect-only sabotage OBSERVED felling the
  zero-requests assertion (left:2 right:1 — counter sees CONNECTS, J's P3-F2 met);
  deadline-zero sabotage FAIL 3/3 (wait is load-bearing); full wake_http 19/19; oracle
  untouched (single-shot discrimination after loop). Debt filed as issue #72 (delay-hook
  S7 + SJ2/3/5 discrimination class; blocked on C's wake.rs pen).
  J FINAL VERDICT: UNCONDITIONAL [APPROVED] — all 4 amendments satisfied; AM3 verified BY
  J against A's raw logs (20 files, 19 names each exactly 20x ok, mechanical by-name
  check). AM3 executed: suite N=20 = 20/20 pass vs base 3/3 fail; standalone 0-in-20 vs
  9/10. Harness attribution at MEASURED CONFIDENCE, not proven; product question stays
  with the double-duty run. Open reviewer task: PR-text grep at PR creation (weaker-
  instrument wording present; dead figure absent). Earlier conditional verdict: AM1 (blocking):
  durable run-evidence artifact w/ raw per-guard lists. AM2 (blocking): PR wording
  weaker-instrument + vs-CLEAN-base; plan's Run protocol marked superseded. AM3:
  ORCHESTRATOR RULED RUN, NO WAIVER — suite-condition N=20 post-fix on A's tree (base was
  3/3 suite-fail; "stabilized" is the milestone's core claim, no shortcut). AM4: SJ6/7/8
  documentation runs appended to #72.
  PR #73 FINAL SIGNOFF (J): rebase byte-identical to approved diff (mechanical patch
  compare), five-gate grep on live PR body ALL PASS (weaker-instrument present, dead
  figure absent, no live-kill claim, both bases both scopes, attribution exact). J's
  flake-2 review lane CLOSED; J to bench.
  FLAKE #2 CLOSED: PR #73 landed ff aac0d67->576e553; full gate GREEN on landed tree;
  final N=20 = 20/20 (by-name mechanical check, zero deadline-fired forms — product
  attribution still no-evidence on final tree). Three named closure cells: A's fix alone
  0/20+0-in-20 (pre-C) · C's fix alone 0/10 · final tree 0-in-20. (A ran the final N=20
  himself with the machine he held — deviation from "H runs it" noted; M scores the raw
  logs, .factory/a-final-n20-logs/.)
  PEN RULING: serve/wake.rs passes A -> C for the #74 window (owner-approved defect fix
  outranks #72 test-hardening debt); A's #72 defers until #74 lands.
  #74 RED OBSERVED: recorded==1 (wake.rs:719, left 1 right 0), replay-succeeds OBSERVED
  (legality assertion passed before the count panicked — C's flagged reorder paid off
  first run: silent-defect clause measured, not inferred). Kill condition did NOT fire —
  defect REAL. Session-gone NOT observed (masked; C refuses to log inference). Digest
  amendments done: 29 pass clean; replay-arm sabotage fells BOTH the neighbour AND C's
  inline control (:286 = control's own append — control is real, not decoration).
  #74 caveat discharged FOR THE DEFECT; permanently unanswerable for committed history
  (captured side was never on the log).
  STEP 4 (fix) waits on B: red receipt + fold-mismatch ruling (C proposes AGAINST B's
  lean: attention-route not refuse — an impossible log and a faithfully-recorded mistake
  differ; wake's own rule = a wake failure never fails the route).
  CARGO: K's #75 slot moved UP (C blocked on B, not machine; K's crate disjoint) ->
  then C's fix runs -> storm re-baseline + rung A + Patch 1 (post-#74 tree) -> storm fix
  -> rehearsal -> rebase + close doc.
  UNHUNTED OBSERVATION (A, parked): GET /health THROUGH the counting proxy HANGS >60s (2x
  reproduced); connect-only + MCP via proxy fine. Unknown: nonexistent route + serve close
  behavior vs proxy. Raw fact only, nobody assigned.
  CONSTRAINT HARDENED BY H's DATA (N cross-checked; the dead "~3 in 4" figure matches
  nothing in H's table — stop quoting it, real base 9/10 iso + 3/3 suite): all 12 sleeper
  failures show the same payload (live:true, lastConsumed:null AFTER the ring — in the
  guard's own vocabulary, the consumption did not land) while the belt passed 13/13 in
  the SAME runs. The one receipt-grain guard is the only one producing signal; the blind
  guards are green in runs where a burn is missing. Belt's 0/13 now has TWO independent
  explanations (S0 + this): its clean sheet is evidence it is NOT LOOKING, not that the
  sweep is healthy. Product-vs-harness attribution of the missing burn = A's lane, open.
  STANDING CONSTRAINT (N found, adopted): the sleeper guard is the family's ONLY sighted
  oracle (end-to-end lastConsumed.reason=="rung" + burn sequence). A's fix REPAIRS THE
  HARNESS, NEVER WEAKENS THE ORACLE (reason + burn sequence + proxy count stay asserted) —
  no quarantine, no retry-until-green, no #[ignore]. J enforces at review.
  MISREADING BLOCK (M, sealed): sweeps 0/13 at HEAD is NOT evidence window 3 is gone — the
  belt is legality-blind and its own author recorded it never reached the window; the
  deterministic seam tests have not run. Never report 0/13 as "flake 3 may be gone".
  N's audit copied to shared .factory/n-agent-oracle-audit.md (authorized, reference doc).
  INSTRUMENT RULE: H's run = clean base, unpatched. Capture run (A's patch, same N) runs
  right after; rates matching base within noise validates the patch's zero-change claim.
  Cargo queue order: H base -> C red observation -> capture run.
- Stash change B (shared-lock open fast path): SEPARATED, parked; needs own guard+review.
- NEW DEFECT (B found, C verified indep.): RDV-EQUAL BURN — stale sweep phase-3 burns a
  FRESH re-armed lease when session+rendezvous match (filter wake.rs:180-185 has no arming
  identity; DueLease wake.rs:42-46 carries none). SILENT: consuming a live lease is legal,
  no Corrupt; wake_wait timeout path (wake_wait.rs:85-101) never consults wake_last_consumed
  -> sleeper reads calm while store says `rung`. Pre-existing at efd85d0; SURVIVES window-3
  fix; triggered by our own fixed-rendezvous convention. NO schema change needed (C corrected
  B: DueLease is pub(crate), arming sequence one scan away). Plan: SEPARATE PR right after
  window-3, C's pen, own observed red. Severity: C ranks it ABOVE window-3 (silent vs loud).
  OWNER DECIDES priority; orchestrator recommends in-M09. Dead end named: cursor is NOT the
  discriminator (re-arm at contentHead fixed point -> identical cursor); arming sequence is.
  Its red test TYPED and PARKED at .factory/c-agent-rdv-equal-red-draft.rs (not in-crate:
  knowingly-red test would fail PR 1's own gate; #[ignore] rejected as rot-in-coverage's-
  clothing). PR 2 moves it verbatim. Second assertion (fresh lease survives) is the one
  that measures. Recorder-side fix = this seed; fold-side detection = separate
  defence-in-depth seed (needs consume event to carry arming identity; nothing today
  bypasses the recorder).

## Post-base phase (numbers landed ~12:5x)
- CARGO QUEUE: (1) DONE H base. (2)+(3) DONE C red+fix+sabotage: commit 0236ffa on
  c-study-arming-the-alarm — RED observed exact signature (recorded 1 vs 0; NO hang, lock
  reading survives); GREEN 4/4 wake guards + 24/24 unit + clippy clean + belt 33.8s alone;
  SABOTAGE 1 (two-read shape) -> ONLY pin guard falls; SABOTAGE 2 (session-only match) ->
  ONLY rendezvous guard falls (previously INVISIBLE — B's blind-review call vindicated).
  Raw lists with B. B VERDICT: [APPROVED], one condition — run S4 sabotage (fixed
  idempotency key): B predicts NOTHING falls (idempotent replay swallows second consume);
  if so SEED it, do not widen PR. N's last-word assertion ACCEPTED non-blocking (fold if
  amending). Arm-rival second red: NO confirmed (different-session = benign drop;
  same-session = rdv-equal seed PR 2, red must target OUT-of-window slice post-fix).
  B-P1 efd85d0 half scored TRANSITIVELY (seam lock-agnostic, code fact both commits).
  ISSUE #71 CREATED. Commit tip aac0d67 (history 0236ffa->185e457->072dda6->078aeed
  (message-only failed amend, caught by insertion count)->aac0d67; one unpushed commit,
  one file). ALL B CONDITIONS MEASURED: S4 = NOTHING FALLS (B's prediction exact; now a
  measured seed in the commit body); wrong-reason sabotage fells ONLY the strengthened
  entry #2 (was invisible before B's assertion); green 4/4+24/24+clippy. C self-reported
  two verification errors (amend --only left staged change out; -SimpleMatch matched
  nothing) — both erred toward worse-than-reality, both caught by number-vs-claim.
  LANE CLOSED: GATE GREEN FULL (22 stages, 260 test lines, 0 fail/panic; Postgres RAN —
  throwaway cluster :57292, 43 tests, both matrices; log scanned for failure markers, not
  just exit code). Branch tip = aac0d67. wake.rs pen RELEASED to A; ring() instrument +
  S7 delay-hook unblocked. C refused two N=1 greens as evidence (wake_http pass with his
  fix ~1-in-13 by luck alone; constructor_bounds pass likewise) — double-duty run scores
  them. Owed by C, small slot later: journal count under S4 (predicts 1; 3..15 refutes).
  STALE COMMITTED DOC (C traced his inherited dead figure to source):
  docs/milestones/m09-seeds.md seed 9 states "~3 in 4" (refuted by H: 12/13) AND asserts
  "fails at the parent commit too" AS FACT — exactly C-P1, which the ledger holds
  UNTESTED (N>=10 binding). Two wrong claims in the doc the next planner reads first.
  Fix assigned to F (close-doc lane), parked draft, lands with milestone close. B RULED: N's entry-#2
  strengthening IS IN this PR (the chain's only dead-recorder detector detects by COUNT;
  strengthening the only working link = finishing the guard the fix leans on). Conditions:
  wrong-reason sabotage OBSERVED felling it (raw list) + grain = assert session absent by
  key, not map empty. BATCHED with S4 in A's gap — ONE run, THREE raw lists. B re-approves
  per delta (approval follows content, not branch tip; re-issued for 185e457, will re-read
  072dda6 delta at sign-off). SOLE REMAINING BLOCKER: A's cargo gap.
  LANDING/CONFOUND ORDER (C raised — his fix removes one lock acquisition in exactly A's
  starver window): (1) A finishes measure/fix/sabotage on CURRENT tree (9/10 base valid);
  (2) C lands; (3) DOUBLE-DUTY RUN DONE: 0/10 isolated at aac0d67 (C's fix alone, A's
  absent; base 9/10 at 53d212d; ~1e-10 if rate unchanged). C's SEALED KILL CONDITION
  FIRED per its sealed terms. M's FORMAL SCORING adds the careful reading (all three into
  owner report): (a) 0/10 IS NOT "FIXED" — excludes rates >~26% only; 15% residual fully
  consistent; (b) MECHANISM NOT REMOVED — two-phase consumption unchanged; pin fix removed
  one next_sequence call from sweep phase 3 (a lock acquisition carrying a FULL load_state
  journal read + validate_anchors both sides — bigger than a lock round-trip, NOT an open;
  C corrected the wording against his own favour) = NARROWER WINDOW, SAME RACE; (c) ISOLATED SCOPE ONLY —
  suite scope (3/3 at base, where the load lives) UNMEASURED at aac0d67. Cross-lane effect
  (flake-3 fix moved flake-2) predicted by NOBODY — no retrofitting, no credit. Rule-7
  ambiguous absence at statistic altitude: 0/10 cannot separate "gone" from "rarer than
  N=10 sees"; more N at this scope does NOT separate them; the distinguishing instrument
  is A's deterministic delay hook (#72). JP1-mechanism scores NOT-YET (consistency is not
  confirmation; sealed to a parent-run instrument that has not happened).
  ORCHESTRATOR RULINGS: (1) A's A1 fix SHIPS — harness condition-wait replaces timing
  assumption (house no-sleep rule), latent-race removal, test-file only, review already
  unconditional; rate-attribution language per the reframe. (2) Flake-2 CLOSURE evidence =
  final-tree suite-condition N=20 after A lands (per-fix suite attribution deliberately
  unmeasured, marked as such — attribution slices only if the final suite shows red).
  (3) Delay hook (#72) remains the fixed-vs-rarer closure instrument.
  SUITE RESULT SCORED (M, cells pre-sealed): 0/10 suite at aac0d67 vs base 3/3 = MEANINGFUL
  DROP (bounds don't overlap: <=26% vs >=37%). TWO BINDING WARNINGS: (1) NEVER POOL the two
  zeros (isolated + suite = different base rates = different populations; pooling
  manufactures an unearned ~14% bound — the exact mistake H refused at source); (2) this is
  NOT the closure run (aac0d67 = C's fix alone; closure = final-tree suite N=20 by H after
  A lands). Owner-report line (M's): the flake stopped reproducing in both scopes on C's
  fix alone, at bounds excluding rates above ~26% in each; the race it comes from is still
  there by design; nobody has yet run the instrument that tells "gone" from "rare".
  C's durable copies in shared .factory/ (study 63KB, PR-2 red, issue draft) — authorized,
  matches N/H practice; tracking = owner call at close. CONSEQUENCE FOR A's LANDING: the flake A's fix
  targets is already at 0/10 on the milestone branch — A's PR must NOT claim to fix the
  live flake; it claims harness hardening (condition-wait replaces timing assumption, per
  house no-sleep rule) + latent-race removal, with BOTH named bases (9/10 pre-C, 0/10
  post-C). (4) A lands + confirm on final tree.
  (4) NOW: A's flake-2 work — cargo is A's. (5) storm re-baseline + rung A. (6) rehearsal.
  H parent-run for C-P1: CANCELLED (M's ruling + B's advance prediction: 0/N-vs-0/N
  proves nothing; tip already 0/10). C-P1 = OPEN-UNTESTED, dropped to FOOTNOTE per
  claim-with-no-consumer unless a consumer is named; decidable instrument if ever needed =
  seam test at parent, never the belt. TWO-ROW DISAMBIGUATION on record: the row that
  FIRED at 0/10 is C's landing-confound prediction (sleeper rate at aac0d67, sealed in
  messages, C accepted); C-P1 (belt at parent) untouched; the window-predates-53d212d
  LOCK reading is untouchable by rates (M's correction of orchestrator's conflated
  vocabulary — on record).
- MEASURED CHAIN-HOLLOWNESS (C ran, N corrected the sentence — THIS form ships): the chain
  proving #55 has exactly ONE link that fails when the recorder is DEAD (a_live_lease_
  consumption_still_records, a COUNT assertion) — and NO link that fails when the recorder
  is WRONG: a recorder consuming the wrong lease, or the right lease with the wrong reason,
  satisfies every guard in the set. DEAD is the axis the chain can see; WRONG is the axis
  #55 is about, and the chain is blind on it. Measured: old deterministic red GREEN under
  dead recorder; belt GREEN alone in 37.9s with record_consumptions deleted (S0). #55's
  fix is real; nothing measures that it stays RIGHT. Cheapest step onto the WRONG axis:
  assert the stored REASON, no schema change. Retroactive re-pricing = owner.
  C owes two citation fields (exact invocation + N-per-condition) — N logged the results
  as OBSERVED-PENDING-CITATION under his own evidence rule; endorsed.
- ARMING-SEQUENCE FIELD: C+B joint ruling = PR 2 (matches pre-ruling; PR 1's value is that
  every claim in it is measured — schema ritual is unmeasured surface). Belt per-round
  upgrade deliberately NOT half-fixed (needs the field; wake_http.rs is A's file).
- B-P1 efd85d0 half: B's TRANSITIVE acceptance stands (explicit, later ruling) — no
  cherry-pick run; would measure the same code fact twice.
- STILL OUTSTANDING before landing: B's S4 condition (fixed idempotency key sabotage) NOT
  yet reported — C runs it in a gap from A. Issue number: C drafts body, orchestrator
  creates.
- C's NUMBER ON THE ORACLE FINDING (owner report): defect real (1/1 deterministic) AND
  belt cannot reach it (0/13) — without the seam test, honest reading of 0/13 was "no
  defect" and the fix would have been dropped. Stochastic greens and legality-shaped
  greens fail in the same direction; both banked in earlier milestones.
- M's CONV findings: CONV-2 SURVIVES (G8 re-verified: one production writer). SHARPENING:
  rival need not be a consumer — out-of-process `wake arm` (execution/wake.rs:100) moves
  head in the same window; C-R3's shape via different actor. CONV-1 DENTED: per-request
  open is NOT pure cost — it is the CACHE COHERENCE mechanism protecting the multi-process
  premise (serve/mod.rs:106-117 names it; :1113-1119 records the incident that proved it).
  A's D1 must supply coherence another way or narrow the premise = OWNER decision, gates
  M10. Rule-6 pattern confirmed (2nd independent instance). Whoever flips the ServeState
  doc must NOT delete the live multi-process paragraph with the dead lock claim.
- Naming drift (M): no `doctor` command exists; queue item 4 = `events verify` /
  integrity path; K measured via `execution status --events` — correct instrument.

## DELEGATION (owner, 2026-08-19): orchestrator OWNS product-quality decisions — decide by
## best-for-product independent of tokens/time; report with reasoning; escalate only
## new-money / owner-vision promises / final merge to main.

## ORCHESTRATOR DECISIONS UNDER DELEGATION (2026-08-19)
0. RING-AFTER-APPEND REORDER: REJECTED (A argued, orchestrator ratified). The reorder buys
   an instant receipt and costs an UNRECOVERABLE failure class: append lands, ring then
   fails (dead pipe / serve dies between phases) -> lease BURNED, sleeper NEVER rung,
   sleeps to horizon with the alarm already consumed. Today the same accident leaves the
   lease LIVE and the next sweep re-rings — the current design fails in the safe
   direction; the residual transient (receipt eventually-visible) is benign and already
   absorbed by the harness wait. A trade of benign-transient for unrecoverable-loss is
   rejected on product value. If ever revisited: failpoint killing ring after append must
   show the burned-unrung state's handling first (today unreachable by construction).
1. CITE-or-MARK = OFFICIAL docs rule, effective now (docs that cannot lie > prose comfort).
2. CHANGELOG label site INCLUDED (all 3 sites + READMEs): an honest shipped doc beats a
   frozen wrong one; git preserves history, the edit adds truth, K executes in #75.
3. classify_layout: .tmp/ + active/ (transient workspace dirs, contents never part of
   history, dropped by git/zip/rsync routinely) become RECOVERABLE on open; blobs/ stays
   STRICT (missing blobs = missing evidence = real signal). Rationale: product thesis is
   "history that reproduces" — a byte-perfect archive must open anywhere; empty transient
   dirs carry zero integrity information. Separate small issue + guard + sabotage;
   assigned to bench after #75.
4. REBASE at milestone close (linear history, full gate after) — executes when storm lands.
5. JUDGE RUN: rehearsal authorized now; paid run AUTO-AUTHORIZED conditional on rehearsal
   green (standing owner authorization for one-at-a-time runs + delegation covers it).
   Announce results, don't re-ask.
6. M10: approved as next milestone. Premise ruling: KEEP multi-process (narrowing a product
   promise for implementation convenience is the classic bad trade); D1 must supply
   coherence (stat/head check under the per-op lock — cheap, already in D1's design).
   M10 step zero: reproduce the owner's 48%/15->36ms numbers with provenance.
- RDV-EQUAL: approved IN M09 -> PR 2 live, issue #74, C writes (per-step clearance), B reviews.
- JOURNALS: OPTION B APPROVED. K executes: (1) NEW test tools/acceptance-map/tests/
  committed_stores.rs (red = comment out create_dir_all -> GHE005 = main's state today;
  green = restore dirs -> open -> assert heads exec-m05-acceptance/12, exec-m06-dogfood/70,
  demo-journey/10 -> verify_artifacts in same run); (2) per-archive READMEs + SHA256SUMS
  lines same commit; (3) byte-archive paragraph in the GENERATOR (acceptance-map
  lib.rs:410-416 — K gets that pen) + regenerated M05_ACCEPTANCE_MAP.md same commit;
  (4) m07/m08 bare-transcript labels as SECOND commit, same PR (CHANGELOG site dropped
  unless owner asks — sites 2+4 carry it). Issue first: K drafts, orchestrator creates.
  REVIEWER: F. Cargo slot: after C's window, before storm re-baseline.
  STILL OPEN (owner, NOT this PR): classify_layout treating absent .tmp/active as
  recoverable (product promise, local.rs:2153-2155).

## REHEARSAL RESULT (E, real HTTP + subprocess, zero cargo)
NOT CLEAN — two real bugs found by running, which is the rehearsal's job:
- BUG 1 FIXED+RE-VERIFIED: workspace-relative marker wiped per drive call (staging is
  disposable PER HTTP CALL, ports.rs:178 keep_workspace:false) AND approve resets the
  identical-outcomes streak (last_outcome moves off RetryableFailure) — two effects
  stacking; fix = absolute path outside staging; post-fix history exact:
  [fail x4, approved, started, started, succeeded].
- BUG 2 = PRODUCT DEFECT, issue incoming (E drafts, M10 scheduling): mode:"manual" is a
  NO-OP on the async HTTP driver — drive_to_quiescence_async has zero mode handling
  (grep + empirical: execution_started{mode:manual} then auto-approve/dispatch to
  completion). The log records a mode the driver ignores. Tool-only graphs ALWAYS take
  this path.
- STORY REDESIGN RULED: duplicate parks VIA PAUSE (start -> immediate graceful pause ->
  Paused; cancel then acts on genuinely non-terminal). E edits spec, F re-reviews delta,
  affected rehearsal slice re-runs. Paid run conditional on the clean re-run.
- Mechanics verdicts: 4 clean PASS + 1 hollow (cancel_terminal — becomes clean under the
  redesign) + bonus approve-is-necessity held.

## SF-2 OVERCLAIM GUARD (L, re-pin check — #74's title invites the wrong reading)
#74 did NOT fix SF-2: ring() still maps EVERY error shape to StaleRendezvous
(wake.rs:64,:66 at d10916b) — a live-but-busy sleeper's lease is still consumed and
recorded consumed. What #74 closed is the OTHER half of the compounding gap: the
consumption now NAMES the arming it burns (captured arming sequence on WakeLeaseConsumed).
Rendezvous identity + error-shape flattening remain OPEN (seed 6, C's lane). No close-doc
sentence may read #74 as closing SF-2.

## STORM VERDICT (Run 0 fired M's sealed cell — the backward question is ANSWERED)
Run 0: 53d212d (old code) on TODAY'S disk = 0/10. VERDICT CORRECTED BY D (my boarded
"disk explains" overclaimed — a mechanism claim the control cannot support): Run 0 holds
CODE constant while disk AND fleet load moved TOGETHER since the 09:23 baseline. What
fired solidly: THE CODE IS EXONERATED as the improvement's cause. What remains:
DISK-OR-LOAD IMPLICATED, UNSEPARATED — we know what it was NOT; we do not know which of
the two it was, and the baseline's figures for both are gone (required fields collect
from today forward only). The flake is NOT "understood"; it is bounded. M's cell fired
as sealed; this narrowed reading attaches to it (row-fires / label-narrows, the C-LC
pattern). M's degraded-but-non-zero narrowing still holds for the disk half.
A's concurrent_cargo field from Run 0 (zero) is the one rough load lever for the close doc. Run 1 (fresh ef51193 pair): 0/10 — the pair
held, same-disk claim valid (fsync 1.5-2.0ms/op, 19G free, zero concurrent cargo, fields
in every row). Probe-off 3/3, probes inert. Sealed asymmetries stand: this does NOT mean
C's #74 fix was pointless (justified on its own deterministic grounds), and 0/10 bounds
the rate at ~26%, not zero. "The flake was the disk" remains AS SEALED: the cell says
disk SUFFICES to explain the delta on this instrument — the lane's output is the
MEASUREMENT + REFERRAL shape confirmed: #19/#81 carry the timeout-policy fix; M10 carries
open-reduction; headroom (Run 2, after D's sink fix) is the referral's number.
RUN 2: 10 NOT-RESULTS (instrument bug, properly classified — env inherits to cli_start
which CREATES the probe file; serve's create_new refuses existing files by its own
design; spec's one-file-many-pids expectation contradicts implementation). D fixes:
append sink / per-pid filename / serve-only env — his call.

## CONTROL FORM — RECORD CORRECTED (one form, not two): C2's control of record is the
## EXACT-COUNT form (D ruled; A followed correctly when my crossed ratification said
## extended). The extended form (remove both edits, see 0) fells without ISOLATING —
## re-adopting the original >=1-row defect in costume. A proved BOTH single-site blades:
## happy = 3 pairs matching site-by-site (ids 3/7/8); Edit-2-only = 1 pair FAILS;
## Edit-3-only = 2 pairs FAILS — A ran the Edit-3 blade D had shortcut as not-required,
## completing the isolation instead of assuming it.
## DRIVER-READING STATUS: PENDING, NOT CONFIRMED (D refuses a result going his way until
## it tests his claim): zero driver rows on pause+resume matches the frozen prediction,
## but if that resume returned 409 the command never reached the drive branch and the
## zero has a different cause — status code requested from A; M holds the row PENDING.

## STORM LANE — FINAL VERDICT (measurement + referral; C1 UNADOPTED; lane CLOSED)
Run 2b clean (10/10, multi-pid capture). Measured: 2.40-2.56 opens/req; median open
29-33ms; max 0.26s; O=70-80ms; 8O=0.56-0.64s vs 5s -> HEADROOM ~8x. Neither
pre-registered trigger fires -> lane closes as MEASUREMENT + REFERRAL per the table.
D's OWN PREDICTION WRONG 10x IN THE SAFETY-OVERSTATING DIRECTION (predicted 40-100x):
fsync is only ~6% of an open — the open is expensive STRUCTURALLY (O(head) journal load,
~10 handle opens in validate_anchors, dir scans), so H3 weakened as the intra-open
mechanism. FIRST error in this row's history running AGAINST the author's preferred fix
— the bias pattern did not hold here, recorded as diligently as the pattern was.
THE TAIL BRIDGE: max open (0.26s) is 8.7x median; a sustained shift of the MEDIAN to
today's max — an ordinary shift for a degraded disk — puts the convoy exactly at this
morning's failures. The disk COULD at plausible magnitude; not proven that it DID.
REFERRAL TO M10, QUANTIFIED: each store open = ~30ms structural work serializing on an
exclusive lock REGARDLESS of handler threading, ~2.5 per request. Removing ONE open
removes ~30ms of serialized time; parallelizing handlers removes NONE. Fewer opens, not
more threads. (A's D1 blueprint is the vehicle.)

## HEADROOM ARITHMETIC (D, first real numbers — H's fsync 1.5-2.0ms/op): today's machine
## sits ~40-100x BELOW the cliff (8*O = 48-120ms vs 5000ms budget). PREDICTION FROZEN
## pre-Run-2: the referral branch will NOT fire at this disk state. The quantitative
## bridge the disk story was missing: reaching 5s needs ~200ms/open — catastrophic, not
## mild, degradation; achievable at the TAIL of a degraded fsync distribution, extraordinary
## at the median. D's OWN FORMULA ERROR fixed pre-run: headroom was defined on the MEDIAN;
## the flake is a TAIL EVENT (4/10 runs, 1-5 of ~48 requests within them) — Run 2 reports
## p99 + MAX alongside median, headroom computed at the tail too. SEALED READING: a
## non-firing median with a tail near the budget is a LIVE FINDING, not a clearance — that
## IS the flake's mechanism, measured. Referral content sharpened for M10: "opens alone do
## not exceed the budget at a healthy disk; the mechanism is tail latency under contention,
## and the per-request open count is the multiplier."

## C1 STANDING — DEMOTED FROM PRESUMPTIVE (D's demand, ratified): C1's expected benefit
## revised DOWN three times in one day (relocates the convoy; opens don't parallelize
## under it; appends don't either — with_lock is exclusive per append, :567). Benefit =
## ~7*W_free, pure-compute fraction only, in a mutation-heavy storm. The lane's more
## likely output: MEASUREMENT + REFERRAL to M10 open-reduction. SEALED ASYMMETRY: a v3
## falsifier that FIRES is decisive against C1; a v3 that does NOT fire is NOT a
## clearance (optimistic bound, under-fires; v4 needs rung B). BIAS PATTERN ON RECORD:
## every model the author built understated serialization while his thesis IS
## serialization — 3 of 4 corrections erred toward his preferred fix; v5 reviewers
## should expect the same direction.

## STORM — OPERATIVE PLAN (FINAL; resolves all queue-lag versions; the shared spec governs)
Executor: A (H classifier-blocked; permission-boundary record above). Validator: H. Scorer: M.
THE SPEC (shared .factory/d-agent-storm-run-spec.md, base ef51193) DECIDES EVERYTHING,
row order included. Run 0 = 53d212d same-disk control, REINSTATED (withdrawn -> D's spec
re-add with tree-preservation condition -> condition satisfied by the FLIP: stash by
explicit paths excluding the beacon, SHA verified BEFORE first run, incremental rebuilds
same target ~zero new disk, restore by stash NAME, @{0} ends as candidate-fix, fingerprint
= H's per-file stats + byte-empty diff). M's control cells LIVE as sealed. Two questions,
never spent on each other (M): Run 0 = BACKWARD (did disk cause 4/10); headroom from the
instrumented run = FORWARD (near the cliff). Required fields per run: free_gb +
concurrent-cargo count. Every sabotage variant build carries its own receipt. If the
classifier denies A's first build: FULL STOP, owner wakes.

## STORM CONFOUND UPGRADE (D via M's fact — bears on whether there is a flake to fix)
The disk hit 0 GB earlier THIS SESSION. NO storm measurement ever recorded free space
alongside its runs — every held rate (H's 4/10, "3 in 6", #19's history) is UNCONTROLLED
for the variable H3 says dominates (fsync latency; near-0 NTFS is a pathological regime,
not a perturbation). NOT claiming disk caused it — the disk's state during H's baseline
is unknown, and "the flake was the disk" is exactly as unestablished as "the flake is
gone". The 53d212d same-disk control (running FIRST, main-flip protocol, sealed cells)
is the separator. RULE ADOPTED: free-space is a REQUIRED FIELD in every storm-run table
from now on (spec updated by D). D1 promoted to the discriminator that matters
(scoreable on passing runs, survives zero-failure regime).

## STORM PHASE STATE (post-#74)
- D's diffs RE-DERIVED vs d10916b (milestone-branch tip; main is df5e431 — do not confuse
  refs). wake.rs anchors moved (:84->:90, :140->:147), all others grepped-unchanged.
  Cheap-path claim RE-VERIFIED not inherited (one open :91, early return :131 — C's
  phase-3 changes cannot affect storm sweep cost; DueLease.armed_at_sequence reads from
  the already-loaded projection, no extra open).
- NEW laundering site :227 (get_json — head_sequence/last_event_of_kind/all_events) —
  same phase-unknown class as :221; Step 0 decision table extended.
- BINDING: H's 53d212d baseline is HISTORY, NOT CONTROL for any post-#74 number — #74
  changed projection.rs (+97) and integrity.rs (+10) in the replay/open path the
  instrument measures. THE RE-BASELINE IS THE COMPARISON POINT. Reusing H's figures to
  skip it silently invalidates the result (sabotage-8's failure through the back door).

## PAID JUDGE RUN RESULT (E, 565.9s real session, archive in E's scratchpad m09-paid-run/)
VERDICT passed:false — AND THE COVERAGE HYPOTHESIS SUCCEEDED: all 7 M08-untouched tools
genuinely exercised via real MCP calls (verified by elimination in read-audit.jsonl):
signal 9x, approve 1x, pause 2x, resume 3x, cancel 1x (dup cancelled, confirmed), probe,
wake-lease activity 8x. m08-judge-coverage's prediction held empirically for the first
time. Double-replay byte-identical.
TWO CRITICAL FINDINGS (E-verified in code, issues being drafted, M10 front-of-queue —
they gate the API's headline flow):
1. MCP surface exposes NO `project` param on resume/start (tools.rs schemas) -> server
   defaults to own CWD -> staging-overlap refusal STRUCTURALLY unavoidable for a pure-MCP
   operator; judge refused 3x deterministically; nothing shipped because of this.
2. resume NOT ATOMIC vs driver-setup failure: ExecutionResumed + hold release commit
   BEFORE drive() setup validation -> a REFUSED resume silently flips paused->running and
   drops the operator's hold (judge: same call twice, two different refusals, hold gone).
FIVE GRADED LESSER FINDINGS (fold to seeds after M scores): signal envelope schema
unexposed (high); no evidence-read tool -> failed node's stdout/stderr permanently
unexplainable, both hash empty (high); mutation replies null timing fields a status call
moments later has (medium); acceptedMutations stuck 0 (medium); silence-budget breach
writes no event so a lease cannot ring on it (low); wake_wait narration says timeout while
the log's recorded reason says stale_rendezvous for the same wait (low — maps to J's
design + flattening).
E's 6-attempt recovery trail stays in the journal — every failure was a product guard
holding correctly against a real operator (idempotency replay, NotPaused, exhaustion,
precise key errors). Close-doc material.

## Seed inventory (named, measured where stated — feeds M09 close doc + owner priority)
1. RDV-EQUAL BURN — OWNER APPROVED IN-M09 (2026-08-19): PR 2 authorized. WRITER: C, pen
   restored SCOPED — every cargo run and every shared write individually pre-cleared by
   orchestrator (the asking-before-acting lesson enforced structurally, not waived).
   REVIEWER: B. Scope per C+B's joint ruling: recorder-side fix (arming-sequence
   discriminator from the history phase 3 already replays) + arming-sequence FIELD in
   WakeLeaseConsumed (full schema ritual: both copies byte-identical, both catalog
   digests, CHANGELOG, fold arm, reproduce OLD digest before writing new) + belt
   per-round-identity upgrade + red from parked draft (out-of-window slice). Issue first:
   C drafts, orchestrator creates. QUEUE: after H's final N=20, BEFORE storm re-baseline
   (PR 2 changes sweep phase-3 work — re-baseline must sit on the tree the storm fix
   will sit on, per the timing amendment).
2. ARMING-SEQUENCE FIELD in WakeLeaseConsumed (N's one-field-three-holes; C+B ruled PR 2
   scope; full schema ritual).
3. S4 SILENT-SWALLOW — UPGRADED: TWO holes, different diseases, different cures (N, both
   source-verified; C's framing separates them):
   3a. REPEAT consumption, one session (DETECTION gap): IdempotencyConflict swallowed
   (append_atomic err -> return 0, no event, no surface) -> lease never burned -> RING
   LOOP WITH NO RECEIPT, receipt frozen at first burn. The belt already DRIVES this path
   14x and reports green — a new driving test cures nothing; the cure is the ORACLE
   (receipt sequence advance). Sequence grain beats reason grain (measured).
   3b. TWO+ DUE SESSIONS in ONE sweep (COVERAGE gap, WORSE): duplicate keys in one batch
   -> validate_prepared_append refuses the WHOLE batch (integrity.rs:107-113) -> recorder
   swallows -> EVERY consumption in the sweep dies together, incl. first-ever burns.
   NOTHING drives it (all fixtures = one armed session). Cure = FIXTURE arming two
   sessions on one execution. PRODUCTION-SHAPED: several agent sessions armed on one
   execution IS this factory's own daily pair-loop state.
   Design note for the taking PR: {next} and {index} in the key are NOT redundant —
   {next} defends ACROSS batches, {index} WITHIN one; S4 removed both and only the first
   is reachable by existing tests, so the measured "nothing falls" UNDER-REPORTS damage.
4. Stash change B (shared-lock open fast path) — parked, own hazard named.
5. Fold-side rendezvous detection (defence-in-depth; needs consume event to carry arming
   identity — folds into seed 2).
6. ring() error flattening (PIPE_BUSY = alive sleeper classed stale; reachability
   UNMEASURED) + wake_wait never consults receipt (PRODUCT/PRD decision).
7. /health through counting proxy hangs >60s (raw fact, unassigned).
8b. CITE VALIDATOR (K, parked: .factory/k-agent-cite-validator-proposal.md — moved out of
   session-scoped scratchpad deliberately): resolves every file:line citation against a
   NAMED BASE and prints the line it names — kills exactly the pointer-to-unrelated-code
   class (a wrong coordinate passes CITE-or-MARK completely; F opened 4 of 40, the rest
   surfaced only by radius-mapping). Honest state: hardcodes one file/base; generalizing
   is the work. Open: scope, working-tree vs git-show resolution (the latter would have
   PREVENTED the defect), base named in text vs inherited. If it becomes a gate, it needs
   a red first. Close-doc item.
8. CROSS-CRATE DEPENDENCY — CORRECTED BY MEASUREMENT (C sabotaged rather than banking a
   green; N's tripwire claim FALSE): removing the sequence field from request_digest fells
   5-7 PRE-EXISTING guards (concurrent-writers, cross-stream keys, artifact identity,
   dangling refs) — the property is INCIDENTALLY COVERED crate-wide, not unguarded. What
   survives: a NAMING/DIAGNOSABILITY problem — no test names the property, dependency
   stated in neither crate, so a maintainer gets 5-7 reds about other things and diagnoses
   backwards. "Guarded" vs "incidentally covered" distinction carried per audit vocabulary.
   N-3a RUN WITHDRAWN (N's own recommendation — mechanism now measured by C's test:
   CONFLICT + stream unchanged, deterministic; M marks row SUPERSEDED BY INDEPENDENT
   MEASUREMENT). C's test = KEEP candidate as the named record, with positive control
   INLINE if it ever lands (control in a different test is one refactor from measuring
   nothing).

## READY FOR OWNER MERGE
- PR #77 (closes #75, journals option B): F [APPROVED] after re-verify (6 citations
  spot-checked both sides of the insertion, SHA256SUMS recomputed by hand, zero stale
  numbers in PR/issue bodies). MERGEABLE, base main. Waits on owner.

## Queue (from owner)
1. Stabilize 3 flaky guards (storm, sleeper-wakes, concurrent-sweeps) ← ACTIVE
2. O(history) cost — SEPARATE milestone, not M09
3. Open-store queue — decide only after #1
4. doctor: 2 journals fail integrity ← W2 investigating
5. Second story for blind judge ← W3 drafting spec
6. Close M09 doc, then propose merge to owner
