# Storm execution record — A (mechanical executor), 2026-08-19 evening

Tree: main checkout, base ef51193 + D's 10 edits (fingerprint sha256 65c7ead2..., byte-
verified after every flip). Toolchain +1.97.1. Test exe api_http-9d8a75daaace784c.exe,
rebuilt at each checkout. All runs: --exact the_storm_holds_under_eight_concurrent_agents
--nocapture --test-threads=1, binary invoked directly. Disk floor respected throughout
(19G constant). Logs: .factory/a-storm-run{0,1,2,3}/.

## Run 0 — same-disk control at 53d212d (flip protocol, H's three blades applied)
SHA verified pre-run: 53d212d343d90eb9d593ef11235089ff326bc164. Stash by explicit paths
(4 files only, beacon untouched); ledger displaced and restored (@{0} = candidate-fix
verified at end); fingerprint identical on restore (same sha256).
| run | exit | wall_s | free_gb | fsync_ms_per_op | cargo_procs |
1..10: ALL exit=0. walls 16.5-18.9. free 19G. fsync 1.538-2.049. cargo 0.
All 10 logs verbatim "test result: ok. 1 passed" (storm ran each time).
**RESULT: 0/10 fail. M's sealed cell: DISK EXPLAINS the 4/10 -> 0/10 change; #74/#72
code EXONERATED as the cause of the improvement.** fsync ~1.5-2.0ms/op today.

## Run 1 — clean re-baseline at ef51193 (patches stashed, N=10)
1..10: ALL exit=0, walls 16.8-19.0, free 19G, cargo 0. All logs "1 passed".
**RESULT: 0/10 fail clean at tip.**

## Run 2 — instrumented (rung A + Patch 1, both probes) — BLOCKED, INSTRUMENT BUG
All 10 runs exit=101 in 0.8-2.4s: every storm thread panics "malformed HTTP response:
no header/body split" (api_http.rs:576:52 / :312:33). Cause, verified from probe file:
open-run1.log contains ONLY pid=45888 (the cli_start subprocess, 2 opens) — the env vars
inherit into cli_start, whose process CREATES the probe file; the serve process then hits
`create_new` on the existing path and PANICS by the sink's own design ("an existing file
is refused on purpose"). The spec's expectation ("rows separate by pid" — one file,
several pids) contradicts the implementation's create_new. Per M's cells: these 10 are
NON-RESULTS (harness failure), not storm data. Preserved non-result trees in
C:\Users\gabri\AppData\Local\Temp\.tmp* (10 dirs, small). D's call: append-mode sink,
or per-pid filename suffix, or set the env only for the serve process.

## Run 3 — probe-off control (both patches applied, env unset, N=3)
3/3 exit=0, walls 17.6-19.6 (same envelope as Run 1 clean: 16.8-19.0). Inert probes:
no observable cost at N=3.

## Instrument controls (section 2) — earlier this session, still pending D's ruling
C1 happy PASS (exactly 1 request pair). C2 happy PASS (3 sweep rows).
C2 sabotage as written (Edit 2 only): NOT failing (Edit 3's phase-3 row survives).
C2 sabotage extended (Edits 2+3): observed FAILING (0 rows).
C1 sabotage (Edit 4 removed): NOT failing, structurally unobservable — zero caller=driver
rows exist even happy-path in fixture-serve scenarios (the async driver never runs).

## Standing state
Tree: 10 edits applied, rebuilt, ready. Stash ledger: @{0} candidate-fix (intact).
BLOCKED on D: (a) control-1/control-2 sabotage forms; (b) Run 2 sink bug fix.
The 0/10-everywhere world also raises D5's precondition question: with zero observed
failures on today's disk, the instrumented run (once unblocked) measures headroom (D1
arithmetic), not failure clustering.

## ADDENDUM — D's revised controls + pair ruling (executed same session)

- PAIR (D's addendum): Run 0 (15:30-15:33) and Run 1 (15:35-15:38) ran back-to-back,
  ~3min apart, free_gb 19G identical on all 20 rows, delta ZERO. The 15:05 numbers were
  never the comparison. Verdict on the PAIR: 0/10 @ 53d212d vs 0/10 @ ef51193, same
  disk -> DISK explains, code exonerated (M's cells read over the pair).
- RESUME OBSERVATION (D's frozen prediction: zero driver rows): pause + resume against
  the storm's own graph + blocked fixtures -> ZERO caller=driver rows (log storm-c1-
  resume-*: only request and sweep). D's code reading confirmed empirically; option (b)
  dead; Edit 4 stays as correctness tagging, INACTIVE IN THIS CONFIGURATION.
- CONTROL 1 REVISED (a): happy = exactly 1 request pair + zero driver rows. SATISFIED.
- CONTROL 2 REVISED (EXACTLY 3 sweep begin/end pairs for arm + one mutation):
  happy = 3 pairs (ids 3,7,8 = arm-sweep phase 1, signal-sweep phase 1, phase 3) ✓.
  Sabotage Edit-2-alone = 1 pair (phase 3 only) != 3 -> OBSERVED FAILING ✓.
  Sabotage Edit-3-alone = 2 pairs (both phase 1) != 3 -> OBSERVED FAILING ✓.
  Extended form (2+3 together) DISCARDED per D: fells without isolating.
  Unit fixed: "row" = one begin/end pair. Conflict note: orchestrator's crossed message
  ratified the extended form; D (spec owner) overruled; golden rule (spec wins) applied.
- Fingerprint re-verified after sabotage restores: 65c7ead2... Tree instrumented, rebuilt.
- INSTRUMENT NOW BLESSED by its own revised controls. Sole remaining blocker for Run 2:
  the sink create_new/multi-pid bug (D's ruling pending).

## RUN 2b — instrumented with D's sink fix (option b, per-pid suffix), N=10
Sink change (v2 fingerprint sha256 7a0ab55b..., file a-storm-edits-fingerprint-v2.diff):
GRAPHHELM_OPEN_PROBE names a BASE; each process writes {base}.{pid}. Client probe
unsuffixed per D (only the test process writes it).
- 10/10 PASS ("test result: ok. 1 passed; 0 failed" verbatim per log), walls 17.4-19.3s,
  free 19G all rows, cargo_procs 0 all rows.
- Probe structure per run: 4 pids (server ~180 opens incl sweeps; cli_start 2 opens;
  2 replay pids 1 open each). Multi-pid capture works.

## D1 HEADROOM (the number Run 2 now exists for, per D's ruling)
Per run (server pid): request opens 143-156, sweep opens 30-35, median request-open
29.1-32.6ms, MAX single open 127-261ms (0.26s worst).
Client /v1 requests per run: 56-65. opens_per_request 2.40-2.56.
O = opens_per_request x median open = 69.9-79.9 ms/request.
8 x O = 0.56-0.64s vs the 5s budget -> HEADROOM ~8x on today's machine.
- D's closing branch (8*O >= 5s) does NOT fire.
- Per-op falsifier (single open approaching seconds) does NOT fire (max 0.26s).
- Failure-conditional rows (D5 clustering etc.) correctly STARVED: no live failure.
- Sweep share: ~32/182 opens ~18% of server opens, but the storm arms no lease — these
  sweeps are phase-1 early-return opens (one per mutation), consistent with D3's cap.

## RUN 3 notation corrected per M: 3/3 PASSES (zero failures), not ambiguous "3/3".
## M's label correction adopted: the Run-0 pair EXONERATES THE CODE (code vs not-code);
## attribution to the DISK SPECIFICALLY is NOT established (baseline fleet load unknown
## and unknowable). Ceiling: 0/10 bounds the 53d212d rate at <=26% (rule of three).

## FINAL REVERT — verified
All 4 files: git diff vs ef51193 EMPTY (exit 0). Stash ledger @{0} = candidate-fix
(intact). Clean binary rebuilt. Disk 19G. Only .factory/handoff-agent-b.md (B's beacon,
untouched by me) remains modified in the checkout, as it was before this lane started.

## M's SCORING GUARD on the headroom (verbatim intent, binding for any reader of this record)
- v3 (8O+W >= 5s) DOES NOT FIRE, and per the SEALED ASYMMETRY that is NOT a clearance
  for C1: v3 is an optimistic bound that UNDER-fires (ignores the per-op exclusive lock
  v4 identified). "The falsifier did not fire, so C1 can be adopted" was pre-refused
  before this number existed and MUST NOT be written anywhere downstream of this record.
- What the ~8x headroom answers: the FORWARD question, for TONIGHT'S MACHINE ONLY (near
  the cliff? no). What it CANNOT answer: the morning's 4/10 — measurements taken where
  there are no failures cannot explain failures in a regime we can no longer produce.
  On tonight's machine no mechanism reaches a 5s timeout (8O=0.6s); that is consistent
  with 0/10 and SILENT about the morning. Population sampled = this machine tonight;
  the phenomenon lived in this machine that morning.
- M's ledger: MATCH 67078b133fc643fe (closing data), MATCH 3c711265fba65656 (Run 0).

## D's CLOSING BOUNDS (adopted; corrections to tail reading)
- Resume verdict, scoped: D's code reading CONFIRMED FOR THIS CONFIGURATION (refusal
  confound EXCLUDED by store evidence: execution_resumed committed). NOT general — a
  graph whose nodes all classify would take the drive path and produce driver rows.
  Edit 4 stays inactive-but-correct. Transferable lesson: PERSIST status codes, never
  only print them.
- Convoy multiplier CORRECTED: use measured 2.40-2.56 opens/request, not x3. On the
  pooled distribution (n=1521, med 30.6, p99 140.6, max 261.1 ms) at x2.5: median 0.61s,
  p99 2.81s, all-at-max 5.22s — crosses the 5s budget by 4%, not 25%.
- D's bound on the flake's required regime: burst AVERAGE must reach ~250ms = either an
  ~8x shift of the WHOLE distribution or ~20% of opens at ~1s. A thin tail cannot get
  there (1% at 1s moves the average only ~50ms). THE FLAKE NEEDS SUBSTANTIAL, BROAD
  DEGRADATION — consistent with a genuinely sick volume, inconsistent with mild disk
  pressure. Sharpens (not merely permits) the disk hypothesis.
