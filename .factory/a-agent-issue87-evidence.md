# Issue #87 (M10 D1) — commit 1: the verified-prefix cache in graphhelm-events

Branch issue-87-verified-prefix-cache @ 0f4e7fe. TYPED-UNBUILT (slot fifth); patch
parked as .factory/a-agent-issue87-core.patch (+471/-21, core/events/src/local.rs only).

## Scope decision (one sentence, recorded)
D1 splits in two: commit 1 = the cache inside the events crate (turns each handle's
~3 full loads into 1 full + N suffix/hits — wins even under serve's per-request opens);
commit 2 = serve's long-lived handle (38 event_store call sites ripple through the
command layer; separately typed and reviewed after commit 1's numbers).

## What was typed
- VerifiedPrefix + VerifyCtx (state, budget, uniqueness sets, evidence digests, offset,
  identity); repository field `verified: Arc<Mutex<Option<VerifiedPrefix>>>`.
- load_state -> Arc<LoadedState>; hit = O(1) Arc clone; growth = read+verify ONLY
  [offset..len) chaining from cached hashes/sequences/budget; shrink or no cache = full
  reload from zero (today's behavior byte-for-byte, line-boundary truncation included).
  Prefix TAKEN while verifying: a failed suffix drops the half-mutated ctx, next load
  runs the full path and surfaces the same error it always surfaced.
- verify_lines = the old load_state loop VERBATIM (two *deref touch-ups), resumable
  from a ctx; Arc::make_mut for normally-refcount-1 in-place growth.
- read_bounded_range helper (short read = Storage, impossible under the lock for
  API writers).
- 4 call sites that MOVED out of state converted to iter+clone (read paths return
  owned copies; previously they rebuilt the entire state per call).
- cfg(test) counters full_load_count / suffix_load_count.
- 8 committed guards: pure-hit; append-as-suffix; rival-handle append seen (the serve
  shape); corrupt-suffix error == FRESH-HANDLE error (today as oracle, discriminant
  compare); SC6 chain-from-cache (sabotage the cached hash -> legit suffix must fail);
  SC2/SC3 truncation pair (mid-line fails, line-boundary EXPECTED GREEN reloads
  shorter); SC1 both halves (prefix corruption EXPECTED GREEN warm / fatal fresh); SC7
  budget path-independence (every counter equal, incremental vs from-zero).

## At slot (planned, in order; disk + cargo-count fields per run)
1. Build + run the 8 guards -> green.
2. TDD-red per guard by IMPL sabotage (tests+impl are one unit, so red = sabotage the
   implementation): (a) never store the cache -> pure-hit and suffix guards fall;
   (b) chain suffix from GENESIS instead of cache -> SC6 falls; (c) skip budget carry
   in from_prefix -> SC7 falls. Each observed failing ON ITS OWN assert, reverted.
3. Full events-crate suite. TRIAGE EXPECTED: any existing test that tampers the journal
   and re-reads ON THE SAME HANDLE now sees the R1 widening — each such test becomes a
   named expected-green (or is adjusted), reviewer-visible, never silently.
4. Paired FRESH baseline + AFTER measurement per the issue's binding condition
   (same-session/same-disk, load counts per op kind, median AND p99 AND max).
5. M's cells to request BEFORE step 4.

## Ratified split + PR contract clause (orchestrator)
Two commits ratified. PR TEXT MUST SAY: CONV-1 (the long-lived handle SUBSTITUTES
open-per-request coherence) belongs to COMMIT 2 — commit 1 keeps opens per request, so
today's coherence story is INTACT; a blueprint reader must not look for the
substitution here. Binding on commit 1 alone: fresh-paired baseline (same session/disk,
free_gb + cargo-count per row, storm/M09 numbers = premise never base); M seals cells
BEFORE the first run; D reviews arithmetic. At slot each sabotage reports WHICH edit ran
+ raw output. Slot queue: E, C, D, K, then me.

## D's arithmetic review (pre-run) — verdicts and corrections applied
- Item 1 PASS: no uncarried field found (D traced every mutation); `expected`/
  `previous_hash` carried transitively through state — auditor comment ADDED at the
  splice point per D's note.
- Item 2 PASS: budget path-independence is STRUCTURAL (every accounting call gated on a
  carried uniqueness set; no order-dependent limit found).
- Item 3 CORRECTED, D's own number misapplied by me: the storm's 2.40-2.56 is OPENS per
  request, EACH a fresh handle with an EMPTY cache — commit 1 saves NOTHING across those
  opens. The claim is PER HANDLE: within one handle, loads drop from N to 1 full +
  (N-1) hits/suffix. The number commit 1's measurement must produce: LOAD_STATE CALLS
  PER HANDLE per operation kind (open=1 always; append path adds 1; CLI record path
  adds ~3). Per-request gains arrive only with commit 2's long-lived handle. All my
  artifacts restated; the old per-request phrasing is DEAD — grep before PR.
- Item 4 PASS: move->clone costs and Arc::make_mut caveat read correctly.
- Item 5 NAMED TRIGGER added as code comment at the reuse test: exposure to in-place
  rewrite of verified bytes is bounded by handle lifetime, which commit 2 deliberately
  lengthens — revisit or bound the reuse (re-verify every N loads / time bound) when
  handles become long-lived. The lock is not the answer for writers that never take it.

## PR-text label (M's condition on the citation)
The direction-of-inquiry observation (semantic question caught a plumbing defect) may
be cited in #87's body ONLY with its seal: "one instance, recorded, not promoted" — a PR
body is durable and gets quoted forward; unqualified, it returns later as a rule nobody
tested (the exact shape this milestone hit repeatedly today).

## M's sealed cells file: .factory/m-agent-87-c1-sealed-cells.md
sha256 064de7be132271336d7db2be7e513eea2afca03df2869c48a43b89070f39840a (7958 bytes).
C1-C8 stand; C9 = named uninformative (wall at ~100 events distinguishes nothing).

## FLATTENING FINDING (M's, against my own design section 2) — resolution PENDING M
My counters (full/suffix) carry no operation-kind dimension; design says never flatten.
Proposed to M: ISOLATED-DELTA protocol for commit 1 (serial single-call runs — the kind
is established by run construction, no concurrent payer exists); label-based per-kind
attribution (rung-A with_caller pattern) lands with commit 2 where concurrency makes it
necessary. M rules whether that satisfies the seal; if not, the label goes in NOW.
SCOPE FENCE (M's, adopted for PR): Metric B flatness-for-serve is COMMIT 2's criterion;
at commit 1 the handle is cold at every request boundary BY SCOPE — not a cache failure.
Commit 1's clean observable: within-handle amortisation on the CLI one-shot path.

## Pre-slot corrections round (orchestrator + D + M), all in the re-parked patch
- KIND DIMENSION IN THE INSTRUMENT (orchestrator's ruling, supersedes my protocol
  proposal): load_state(kind), cfg(test) loads_by_kind map (full,suffix,hit) per
  operation, 12 call sites stamped, committed guard asserts the exact derivation
  open=(1,0,0) append=(0,0,1) next_sequence=(0,1,1). Metric A computable. M re-seals
  affected cells as dated delta.
- SC7 FUTURE-PROOF (D): LoadBudget/LoadedState/LoadLimits derive PartialEq(+Debug);
  whole-struct assert_eq! replaces BOTH hand enumerations (10-field tuple removed, not
  left as a redundant blade). Positive controls kept, before the equalities.
- SC6 setup-red note (D): expect-panic in setup means "seam gone", not "guard caught".
- 94% CORRECTED AT THE SOURCE I OWN: issue #87 comment 5348926882 — residual not
  measurement, 4-cost bundle named, cache attacks the UNMEASURED journal-load share
  only, 3x-vs-2.5 conversion ban. Board/roster labels are orchestrator/G pens, flagged.
- CONV-1 lock-across-lookup: OPEN, D reports separately. Slot now 3 (after B).

## Success criterion REFINED (orchestrator, post-94%-correction; binding for the table)
Commit 1's claimable success = the JOURNAL-LOAD SHARE falling — and the table must
MEASURE that share (full vs suffix per operation kind IS the decomposition that was
missing from the 94% residual). Not "the open got faster" (validate_anchors + lock +
scans are untouched); the row is "loads that re-verified history" vs "loads that did
not", per kind. M's delta-2 will re-seal kind cells against the typed instrument
(condition 1 isolation-control kept by choice in measurement runs; condition 2
dissolved — attribution is in-datum). Board corrected re: residual; g-agent-roster
pending G's wake.

## CONV-1 report (D) + M's delta-2, both folded into the parked patch
- CONV-1 CLEARED for commit 1 (D's own predicted finding did not fire, said first):
  the verified-mutex IS held across lookup+IO, but opens/request untouched and the
  mutex is per-handle/uncontended at commit 1; at commit 2 it REPLACES a ~30ms
  serialization with a shorter one. RESIDUAL filed: critical section is O(suffix
  bytes) — a CONTENTION exposure on long-lived shared handles (a far-behind handle
  pays a long read with siblings waiting). SECOND consequence added to the SAME named
  trigger comment (staleness + contention, one trigger line, two consequences).
- M delta-2 (3292d60b, 5409 bytes; delta-1 conditions VOIDED out loud): label carries
  attribution in-datum; NEW failure mode = a swapped label is silent (L4 sum
  reconciliation catches drop/double, not swap; the derivation guard is the anti-swap
  control for exercised sites). L5 gap SHRUNK in the patch: by_kind_probe exercises 7
  read ops once each on the warm handle and asserts each label shows exactly (0,0,1) —
  10 of 12 stamps now guard-verified (remaining unverified: the two committed_events/
  test-only paths; named, not smuggled). Aggregation note added at the (0,1,1) assert
  (triples are per-scenario sums, not per call).

## M delta-3 (c966059a, 4242 bytes): K4's substance restored as L6
My holding of K4 against delta-2 exposed M's keeper's error ("supersedes in full"
dropped the concurrency limit without a home — a superseding document must ENUMERATE
what it carries forward or it silently unseals what it forgot). L6 = same conclusion,
CORRECTED mechanism: under the label the limit is not attribution-fails-under-
concurrency, it is THE INSTRUMENT DOES NOT EXIST on the concurrent path (map is
cfg(test); real serve uninstrumented). Commit 2 owns it, with_caller precedent.
Condition-1 kept-by-election re-sealed as a strengthening with the consequence: if that
control ever fails it is a REAL finding (a counter moving with no named operation
undermines L1-L4 at once). Cell chain: 064de7be / c2f58991 / 3292d60b / c966059a.

## SCORING BASE CORRECTED (M delta-4, aa84a3fe, 3944 bytes)
The table scores against ORIGINAL 064de7be + DELTA 2 3292d60b + DELTA 3 c966059a +
DELTA 4 aa84a3fe (my earlier "original+delta2" phrasing is DEAD - it left L6 unapplied).
- L5' sealed: 10 of 12 stamps verified by observation; the two remaining are NAMED
  (committed_events_for_idempotency, the test site), not counted - a swap between those
  two stays undetectable by anything in the suite; small, named, never travels as covered.
- L7 (new, the likeliest future "resolution" error): THE TRIPLE IS SCENARIO-DEPENDENT,
  not a property of the operation - next_sequence is (0,1,1) in the hot scenario and
  (0,0,1) in by_kind_probe, BOTH CORRECT. Sealed refusal: no triple may be cited as
  "operation X costs Y" without its scenario; an operation has no context-free cost in
  this instrument.

## Supersession procedure (M, from the K4 incident; cite when any sealed doc is superseded)
A superseding document must account for EVERY cell ID in its predecessor — each marked
CARRIED, SUPERSEDED, or RETIRED. Grep-checkable: predecessor's ID list vs successor;
an unaccounted ID is the omission. Delta 2 would have failed this on K4 in one command.
External-holder survival (someone holding a cell outside the successor) is luck, not
design.

## EXECUTED (slot, 2026-08-19 ~23:0x-23:4x): commit ea23101, all runs same session/disk
Launch: sha 0f4e7fe verified pre-run; base drift vs origin/main (21fd7dc, #123): zero
in core/events. free_gb=20-21G constant, cargo_procs=0 every run row. D's blades
applied: clean -p at start; workspace cargo check after suite; sha at launch; bare
invocations.

1. WRONG-FIXTURE RED FIRST (unplanned, honest): all 8 guards failed on first run —
   my fixture scope said execution-1, valid_graph_request uses execution-fixture.
   Diagnosed by temporary trace (suffix path verified 4407 bytes into key b42675...;
   lookup used a different key). One-root fix; traces removed.
2. GREEN: 52/52 (44 pre-existing + 8 guards); ZERO R1-widening breaks in existing
   tests; cargo check --workspace clean (blade-2: -p is not a subset).
3. IMPL SABOTAGES, each reported by edit + panic site:
   (a) never-store-cache -> 6 guards red (3 on own asserts :5119/:5184/:5205, 3
       setup-reds = seam gone, read per D's note);
   (b) re-derive-ignoring-cache -> EXACTLY SC6 red at :5263;
   (c) budget-not-carried -> EXACTLY SC7 red at :5368. All reverted; 52/52 re-green.
4. PAIRED MEASUREMENT (identical harness both trees, load_count-only by construction;
   measure_87_paired_rows, committed as #[ignore]):
   raw rows .factory/a-87-{baseline,after}-rows.txt; table (med/p90/max, n=10):
   - warm next_sequence: 120.8ms -> 0.36ms (size 100); 1061ms -> 0.42ms (1000);
     after-only 0.64ms (5000). NEAR-FLAT IN SIZE on the cache: 0.36/0.42/0.64ms.
   - warm read_replay: 120.3 -> 0.36ms; 1043 -> 0.93ms; after-only 9.78ms (5000).
   - warm append: 135 -> 9.0ms; 1021 -> 10.3ms; after-only 16.3ms (5000).
   - open (cold): 131.6 -> 77.5ms (100); 1065.7 -> 738.1ms (1000); after-only 4284ms
     (5000) — STILL O(n), as scoped (cold opens are commit 2 / D2 territory).
     RAW UNEXPLAINED: after-open is consistently ~30% FASTER than baseline-open;
     mechanism not established, NOT part of the claim, offered to D to attack.
   - loads=1 on every row both trees (Metric A per op = 1 load_state call; the
     full/suffix/hit decomposition is carried by the green per-kind guards).
   - BASELINE 5000: NOT-RUN — the fixture BUILD (5000 appends, each a full O(n)
     verify) exceeded the 590s budget; the miss is itself the O(n^2) phenomenon,
     bounded below by the timeout, per M's not-a-result discipline.
5. Scenario labels per L7: all warm rows are "quiescent warm handle, repeated op";
   append rows mutate (sequence advances per iteration; suffix-verify of own line
   included in the measured op).

## CONTAMINATION HANDLING (D's self-report: cargo ran ~23:20-23:27 during my slot)
Timeline check from artifacts: kept BASELINE run 23:30:00-23:39:51 -> CLEAN (starts
after D's window ends 23:27:16). Original AFTER run 23:19:00-23:21:24 -> overlaps D's
PRECAUTIONARY window start (23:20; D's hard artifact evidence starts 23:23:43) ->
treated as suspect, NOT argued as "probably fine".
RESOLUTION: full AFTER re-run 23:44:04-23:46:38 (cargo_procs=0, free 20G, outside all
windows) = .factory/a-87-after-rows-rerun.txt, ADOPTED AS THE SAMPLE OF RECORD.
Warm-op medians replicate within noise (next_sequence 0.42/0.56/0.63ms across sizes;
appends 10.1/10.6/14.6ms). PAIRED CONCLUSION UNCHANGED on clean data:
1061ms -> 0.56ms @1k warm next_sequence; near-flat in size.
DISSOLVED: the "after-open ~30% faster" unexplained raw — rerun open medians (80.2 /
1027.9 / 5055.9ms) sit within ~4% of baseline @1k; the earlier gap was cache-warmth
noise, not a mechanism. Withdrawn as an observation with cause recorded.

## M's scoring corrections (recomputed independently from my raw rows) — adopted
1. RETRACTION RESTATED: the open anomaly did NOT "dissolve to 4%" — that was the
   1000-size ratio only. Per size: 131.6->80.2ms (-39%) @100, 1065.7->1027.9 (-3.6%)
   @1000; ABSOLUTE deltas 51.4ms and 37.8ms = a ~40-50ms CONSTANT OFFSET independent of
   journal size — which is BETTER evidence for the page-cache-warmth cause than the 4%
   was (a mechanism proportional to work would scale; a constant does not). Cause
   right, number was wrong.
2. DERIVED-FIGURE RULE (new pre-PR grep line): when the sample of record changes, every
   derived figure is RECOMPUTED, never carried, and each derived number in the PR names
   its sample BY FILENAME. My "1.8x across 50x" was the SUPERSEDED sample's ratio; the
   rerun's is 1.5x (0.42/0.56/0.63) — and it is not pre-written: computed from whatever
   sample is of record at PR-writing time.
3. SPLIT THE SUBLINEAR CLAIM (M's unclaimed find): next_sequence and append are
   near-flat-sublinear (1.5x across 50x); read_replay_warm GROWS 19x across 50x
   (0.43/1.52/8.04ms — it returns the whole history, O(n) copy); open stays O(n) by
   scope. Lumping them would overstate. The PR states three regimes separately.
4. Blocking finding CONFIRMED BY THE INCIDENT (M): the original after-arm met a
   different environment than the baseline arm — that is what blocking exposes.
   Interleaved ABAB design goes on commit 2's measurement plan as a requirement.
5. Headers: landed in all three row files + shared at ~23:52 (M's grep predates them);
   binding condition 2 closes on the files as committed.

## M's condition-2 closure + residual (verified by M against the files)
CLOSED IN INTENT with the residual NAMED: headers are LAUNCH-ATTESTED, not
WINDOW-SAMPLED — a header proves the value at launch, not that it held throughout. At
2.5-minute durations it is unlikely to bite; COMMIT 2's (longer) runs take per-row
fields as a requirement, alongside ABAB interleaving. PR body leads with the
THREE-REGIME split (next_sequence/append near-flat 1.45x/50x per run-3; read_replay
grows 19x/50x, honest O(n) copy; open O(n) by scope) over any single headline ratio —
separating regimes is what makes the sublinear claim survive a hostile reading.

## THIRD WINDOW CLOSED, SAMPLE SEALED (run-3 = sample of record)
J confirmed zero invocations from 23:50:00 onward (task-output mtime evidence), covering
my whole run-3 window (23:52:37-23:54:55). J's correction to MY attribution was right
and resolved mechanically, not by recall: my "2 procs at ~23:52" sighting actually
occurred at 23:49:28 (the header-writing Bash call's file mtime, independently verified
by M) — inside J's attempt-2, which ended 23:50:00. The loose "~23:52" was
eyeball-recall; the mtime is the record. Derived figures recomputed from run-3
(next_sequence 1.45x across 50x). All three contamination windows adjudicated;
push+PR conditional signal now unblocked.
