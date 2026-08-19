# Storm lane — run protocol (D Agent), position 5

**NAMED BASE: `ef51193`** (tip of `origin/issue-m09-arming-the-alarm`, **not** `main`).
*(Was `d10916b`; #72 moved the tip and DID touch production `wake.rs` — see the rung A file.)* Companion files: `d-agent-rung-a-edits.md`, `d-agent-patch1-edits.md`.
Full reasoning: `d-agent-storm-study.md` in the D worktree.

---

## RESULTS — THE RUNS ARE COMPLETE. Everything below section 0 is the executed protocol, kept for provenance, NOT pending work.

Executed 2026-08-19 by A Agent, base `ef51193`, with H co-signing validation. Read this section
first; the imperative voice further down describes runs that have already happened.

| Run | Result |
|---|---|
| 0 — `53d212d`, today's disk | **0/10** |
| 1 — `ef51193`, paired back-to-back with Run 0 (~3 min gap, `free_gb` 19G identical across all 20 rows) | **0/10** |
| 2b — instrumented (rung A + Patch 1) | **10/10 pass**, multi-pid capture clean (server ~180 opens, `cli_start` 2, replays 1+1) |
| 3 — probe-off control | **3/3** |

**The confound control fired.** Old code on today's disk also passes, so **the code change is
exonerated as the cause of the improvement.** It does NOT separate disk from fleet load — both
moved together and neither was recorded on the morning baseline. Honest verdict: *code exonerated;
disk-or-load implicated, unseparated.*

**Headroom, measured:**

    opens/request        2.40 – 2.56
    median request-open  29.1 – 32.6 ms
    max single open      0.26 s          (8.7× the median)
    O = 70 – 80 ms       8·O = 0.56 – 0.64 s  vs  5 s budget  →  ~8× headroom

**Neither pre-registered trigger fires.** The referral branch needs `8·O ≥ 5s` (measured 0.6s); the
C1-justified branch is unreachable from rung A by construction. That is the table working, not a
gap in it.

**Two findings that outlive the lane:**

1. **fsync is ~6% of a 30 ms open.** The author's headroom prediction (O = 6–15 ms, 40–100×) was
   **wrong by ~10×**, because it derived per-open cost from fsync assuming fsync dominates. It does
   not — the cost is structural: the O(head) journal load, `validate_anchors`' ~10 handle opens,
   the directory scans. This weakens fsync as the mechanism *inside* an open. It does **not**
   weaken the disk explanation for the *rate change*, since a degraded volume slows all of that.
2. **The machine sits one 8× distribution-shift from the cliff.** To reach 5 s the average open
   across a burst must reach ~250 ms — **0.96× today's observed maximum**. This does not prove the
   disk caused the morning's failures; it proves it **could, at a plausible magnitude** — the
   bridge the disk hypothesis lacked all day.

   **Pooled distribution (H, computed from the landed per-pid rows): n = 1521 `caller=request`
   open-ends across the 10 passing runs — median 30.6 ms, p99 140.6 ms, max 261.1 ms.** The tail
   is heavy: p99 is **4.6×** the median, max **8.5×**. (A's per-run ranges — median 29–33, p99
   110–173, max 128–261 — bracket these; the pooled figures are the authoritative ones.)

   **Convoy multiplier: use 2.5 opens/request, not 3.** H's arithmetic used ×3 and so overstates
   by ~20%; A *measured* 2.40–2.56, because the storm's mix includes status reads at one open. At
   ×2.5 (≈20 opens per convoy): median **0.61 s**, p99 **2.81 s**, max **5.22 s**.

   **Separate the degenerate cases from the real requirement.** A convoy is ~20 opens (8 requests
   × ~2.5). Taking *every* open at one quantile gives: all-at-median **0.62 s**, all-at-p99
   **~2.8 s**, all-at-max **~5.2 s** — the last just touching the budget. But those are shapes, not
   predictions: the realistic sum is dominated by the median with occasional tail draws. **The
   actual requirement is on the burst AVERAGE, which must reach ~250 ms.**

   **What that costs, computed rather than asserted:** ~250 ms average from a 30 ms median needs
   either an ~8× shift of the whole distribution, **or** roughly **20% of opens at ~1 s** (0.8×30 +
   0.2×1100 ≈ 244 ms). A thin fattening of the extreme tail does *not* get there — 1% of opens at
   1 s moves the average to only ~50 ms. **So the flake requires a substantial, broad degradation,
   not a subtle one.** That is consistent with a genuinely sick volume and inconsistent with mild
   disk pressure — which sharpens the disk hypothesis rather than merely permitting it.

### ⚠ IF YOU ARE M10 USING THESE AS A "BEFORE" BASELINE — READ THIS FIRST

These numbers are **disk-conditional**, and that is not a footnote: this lane's central finding is
that the *same code* produced 4/10 failures and 0/10 passes on the same machine at two disk states.
The measurements above were taken at **`free_gb` ≈ 19 G, fsync 1.5–2.0 ms/op, base `ef51193`,
2026-08-19**.

**Comparing an "after" number taken at a different disk state against these reproduces exactly the
confound this lane spent a day failing to untangle** — and it will look like a clean result while
doing it. The whole reason this lane could not attribute the rate change is that nobody recorded
disk conditions beside the morning baseline.

So: **re-measure the "before" at the same session and disk state as the "after", or record both
conditions and report the delta with the numbers.** The required field exists for this reason. If
only one can be had, prefer a fresh paired baseline over reusing these figures — this document
would rather be superseded than misused.

**LANE OUTPUT: MEASUREMENT AND REFERRAL. C1 stays unadopted.** Each open costs ~30 ms of
structural work and serializes on an exclusive lock *regardless of handler threading*, at ~2.5 per
request. Removing **one** open saves ~30 ms per request and **~244 ms off the 8-deep convoy's tail**
— an 8× amplification, because the convoy is where an open's cost is actually spent. **Fewer opens,
not more threads.** Referred to M10's per-request-open work with these numbers attached.

Failure-conditional rows (clustering, commits-past-timeout, leak-3 magnitude) are **STARVED** —
there was no live failure to measure. That is a state, not a debt.

---

## 0. The one shortcut that silently invalidates everything

**H's existing baseline (`h-agent-base-measurements.md`) was measured at `53d212d` and is NOT a
valid control for post-#74 numbers.** #74 changed `core/events/src/projection.rs` (+97) and
`core/events/src/integrity.rs` (+10) — both in the replay/open path this instrument measures.
Comparing instrumented numbers against H's would compare **across a store change** and attribute
the difference to the wrong cause.

**The re-baseline at `d10916b` IS the comparison point.** H's numbers are history, not control.
If time or disk pressure ever makes reusing them tempting, that is the shortcut that ruins the
result while leaving it looking fine.

---

## 1. Order of runs

| # | Run | Patches applied | Purpose |
|---|---|---|---|
| 0 | **Same-disk control at `53d212d`**, N≥10 | **none** | separates CODE from DISK — see below |
| 1 | **Re-baseline** at the current tip, N≥10 isolated | **none** | the control for the instrumented run |
| 2 | **Instrumented**, N≥10 isolated | rung A **and** Patch 1 together | the measurement |
| 3 | **Probe-off control**, N≥3 | both applied, env vars **unset** | bounds the observer effect |

**Run 0 exists because three variables moved at once.** Between the 4/10 at `53d212d` and the
0/10 at the current tip, the **code** changed (#74, #72), the **disk** changed (cache levers spent
upstream), and **fleet load** changed. None was recorded. Re-running `53d212d` on *today's* disk
is the only same-disk comparison available: ~0/10 there means the **disk** explains the change and
the code is exonerated; ~4/10 there means the **code** explains it. Run it with the required
fields below, or it reproduces the very ambiguity it exists to resolve.

*(I originally proposed this as "one cheap run" and withdrew it on learning the cost — it needs a
checkout and build, and the no-flip shape breaches the disk floor. It is back only because the
board sequenced it with a flip protocol. It is worth a slot; it is not worth breaking another
agent's tree, and if the protocol ever conflicts with that, drop the run rather than the tree.)*

Both patches go in **together**. They are not alternatives: Patch 1 is the only independent
clock that makes rung A's phase denominator checkable rather than self-asserted.

Command shape (matching H's method — build once, invoke the binary directly, no cargo overhead
in wall times):

```
cargo test -p graphhelm-cli --test api_http --no-run
target\debug\deps\api_http-<hash>.exe --exact the_storm_holds_under_eight_concurrent_agents --nocapture
```

Environment, **fresh BASE paths per run** (both probes refuse an existing file on purpose):

```
GRAPHHELM_OPEN_PROBE=<dir>\open-run<N>.log
GRAPHHELM_CLIENT_PROBE=<dir>\client-run<N>.log
```

**⚠ `GRAPHHELM_OPEN_PROBE` is a BASE path — the server writes `open-run<N>.log.<pid>`.** Collect
with a glob over `<base>.*`. This was a real bug that killed a whole run: several `graphhelm`
processes inherit the variable (the server, `cli_start`, the replay subprocesses), and with a
single shared path `create_new` refused every process after the first — `cli_start` created the
file, the server panicked, and all ten runs died in under a second. **My spec's own expectation
("rows separate by pid") contradicted my implementation (one file, first writer wins).** Per-pid
files keep fresh-file enforcement *and* multi-process capture, and make the pid separation
structural rather than a parsing step. `GRAPHHELM_CLIENT_PROBE` needs no suffix — only the test
process writes it, since the spawned CLI binaries carry no client-probe code.

Run with an **exact test filter and `--test-threads=1`**: `Command` inherits the environment, so
these variables also reach `cli_start` and the two `graph replay` subprocesses, and any other
test's spawned server. Rows separate by **pid**, not by test.

### ⚠ ENV HYGIENE — two instruments now share this process tree, each invisible to the other

`#72` added an **env-gated sleep in the production sweep path**: `test_only_phase3_delay()` is
called at wake.rs:148, inside phase 3's `spawn_blocking`, and sleeps for
`GRAPHHELM_TEST_WAKE_PHASE3_DELAY_MS` milliseconds (wake.rs:161-166). Its cost when unset is one
`getenv`, and the storm never reaches it (`due` is empty, the sweep returns at wake.rs:131) — so
it is harmless *as long as it is unset*.

But environment inherits to every spawned child, which is this spec's own G6. **A value left over
in the shell from a flake-2 timing run would inject sleeps into phase 3 of any test that DOES arm
a lease — including the C2 control — and the probe rows would carry that latency with nothing in
the data naming its cause.** An inherited sleep looks exactly like latency. Therefore, asserted
before each run rather than assumed:

```powershell
if ($env:GRAPHHELM_TEST_WAKE_PHASE3_DELAY_MS) { throw "phase-3 delay var set; storm/rung-A runs require it UNSET" }
```

And the reciprocal, for whoever runs flake-2 timing work: **assert `GRAPHHELM_OPEN_PROBE` and
`GRAPHHELM_CLIENT_PROBE` are UNSET**, so a stray probe file cannot skew their timings either. Two
instruments in one process tree, each blind to the other, is the ambiguous-absence shape this lane
has been naming all day.

### A note on where claims count

The pair channel is entirely compressed register — it is the highest-rate producer of stale
restatements and the lowest-rate place anyone greps. **Messages are not artifacts; a claim only
counts once it lands in one.** If something in this lane exists only in a message, it is not yet
part of the spec, however many agents have agreed to it.

**Capture per run:** exit code, wall time, full panic text **including `file:line`**, both probe
files, and the `PRESERVED events tree at ...` path printed by Patch 1.

### REQUIRED FIELD — free space, in the run table itself

**Every storm run's row must carry the free space on the checkout's volume, measured immediately
before the run.** Not in prose, not in a machine-context paragraph — **in the same table as exit
code and wall time**, so it is impossible to read a rate without reading the conditions it was
measured under.

```powershell
"free_gb=" + [math]::Round((Get-PSDrive F).Free / 1GB, 2)
```

**Why this is required and not advisory.** The whole lane is currently unable to answer whether a
rate change came from the code or from the machine, because **no free-space figure was ever
recorded beside any storm measurement** — not the 4/10 baseline, not the "3 in 6" in the issue
history, not the original #19 report. One line beside each of those numbers would have settled
today's ambiguity outright. The dominant per-open cost is an fsync, and fsync latency is
disk-state dependent, so a rate recorded without free space is an anecdote rather than a
measurement.

### RECOMMENDED — fsync latency probe, same volume as the checkout

Free space is a proxy; this measures the quantity that actually matters. Run immediately before
the storm run, on **the same volume as the checkout** (a probe against `%TEMP%` on another drive
measures the wrong disk):

```powershell
$p="F:\github\GraphHelm\.factory\fsprobe.tmp"; $sw=[Diagnostics.Stopwatch]::StartNew()
1..100 | ForEach-Object { $f=[IO.File]::Create($p); $f.Write([byte[]]::new(64),0,64); $f.Flush($true); $f.Close() }
$sw.Stop(); Remove-Item $p -Force; "fsync_ms_per_op=" + [math]::Round($sw.Elapsed.TotalMilliseconds/100, 3)
```

`Flush($true)` forces the write through to the device, which is what the store's `sync_all` does
per append. Record the number in the same row.

### RECOMMENDED — concurrent build count

*(Added beyond the mandate; strike it if the board does not want it.)* Fleet load was the **third**
uncontrolled variable between the 4/10 and the 0/10, alongside code and disk, and it is equally
unrecoverable after the fact:

```powershell
"concurrent_cargo=" + @(Get-Process cargo,rustc -ErrorAction SilentlyContinue).Count
```

---

## 2. The instrument's own controls — run BEFORE trusting any number

Neither expectation comes from the probe's own output. **Both must be observed failing under
their sabotage**, or the caller tag is an assertion about itself.

1. **Exactly-one:** one `GET /v1/executions/{id}` produces exactly one `caller=request`
   begin/end pair and zero other-tagged rows. *Sabotage:* drop the `driver` wrapper
   (rung A edit 4) → must fail with a driver open tagged `request`.
2. **Forced-sweep:** one armed lease + one successful mutation produces ≥1 `caller=sweep` row.
   *Sabotage:* drop the `sweep` wrapper (rung A edit 2) → must fail.

---

## 3. Three-apparatus verification, and exactly what each can prove

| Apparatus | Independent of | Valid where |
|---|---|---|
| probe rows | — (this is the instrument under test) | everywhere, but self-asserted alone |
| client status codes | the probe's *existence* | **passing runs only** |
| committed store events (preserved tree) | both | **including failing runs** |

They have **different failure modes, one shared upstream** — all three come from the same harness
and server process, so a failure before the storm starts blinds all three. Not "disjoint".

**Denominator = the STORM PHASE, never the run's wall clock.** Wall clock contains `cli_start`,
the spawn and health poll, and the verify step. Bound the phase from the probe rows: first-to-last
`caller=request` row of the **server** pid, with CLI-pid bursts marking `cli_start` and the
replays. **Two arms:** on a **passing** run, strip the trailing ≤2 verify reads (`all_events`
pages once plus one empty page, over HTTP, before the CLI replays); on a **failing** run there is
no trailing pair — the verify step never runs — so the phase ends at the last `caller=request`
row. Applying the passing rule to a failing run truncates real storm rows and biases the serial
share upward. Report the residual bias with the number.

**Cross-checks — two tests that fail differently, neither substituting for the other:**
- **Edges bound the span:** first client request precedes the first server open row; last client
  response follows the last. Catches a truncated or shifted phase.
- **Counts detect drops:** a systematic mid-run drop leaves both edges intact and passes the span
  test. Client request counts by outcome vs server row counts by outcome catch it — **on passing
  runs**; on failing runs use committed store events instead (`caller=request` rows must be
  ≥ 3 × committed storm-attributed decision events).

**Completeness is regime-limited, and must be reported that way:** verified on a *passing* run
certifies the probe under **lighter load than the run we care about**. Every plausible drop
mechanism (sink `Mutex`, append syscall under contention, process killed mid-write) is
load-dependent. Say "completeness established under a load regime that excludes the suspected
drop mechanisms" — not "the probe is verified".

---

## 4. What the numbers decide

- **D1 (decisive):** max single-open elapsed vs the 5s budget. If the largest open is far below
  5s while requests exceed 5s, the wait cannot live in any single operation → queueing, with
  depth ≈ request wait ÷ median open. If one open approaches seconds, per-operation cost binds
  and **the pre-committed falsifier fires: C1 must NOT be adopted** even though the storm would
  likely pass with it.
- **D3:** `caller=sweep` share bounds sweep amplification. The storm arms no lease, so each sweep
  is **one open** (early return at `wake.rs:131`, re-verified at `d10916b`) — that caps the
  effect.
- **D4:** last-decile vs first-decile open elapsed ≥2× would confirm the O(history) cost.
- **D5:** in a multi-hit failing run, do the failures **cluster in time**? Queueing predicts yes;
  independent per-request cost predicts scatter. Patch 1's timestamps alone answer this.
- **Phase on the laundered path:** `connect_us` vs `read_us` where the panic is at `:221` or
  `:227`. Note the bound: 16 of 20 captured panics were already unambiguously read-phase, so this
  settles a corner and one supporting inference — **it cannot overturn the phase verdict.**

**⚠ Read-phase death is consistent with H1, H2 and H3 alike.** "Most failures are read-phase" is
a statement about **phase**, never about **mechanism**. It must never be copied forward as
support for queueing over the alternatives.

---

## 4b. ANALYSIS PROCEDURE — pre-registered before any data exists

Written before the runs so the analysis cannot be chosen to flatter the result. If the numbers
arrive and this procedure is inconvenient, that is the procedure working.

**Parse.** Rows are `open-begin id= t= pid= thread= caller=` and
`open-end id= t= elapsed_us= caller= result=`. Join on `id`. A **begin with no end** is not a
parse error — it is the G1 signal (an open parked on the blocking lock) and must be counted and
reported separately, never dropped.

**Bound the storm phase** (server pid only). Passing run: first `caller=request` row to the last,
minus the trailing ≤2 verify reads. Failing run: first to last, nothing stripped. Report the
residual bias with the number.

**Then, in order:**

| Row | Computation | Verdict |
|---|---|---|
| P8 | sort request-path intervals by begin; check for any overlap | any overlap ⇒ P8 falsified, **and P6 becomes UNINFORMATIVE** |
| P6 | Σ request-open elapsed ÷ storm-phase duration | ≥50% confirms |
| P5 | 3·N(200 mut) + 2·N(409 precond) + 3·N(409 conflict) + 1·N(status) vs actual request rows; `caller=sweep` rows vs N(200 mut) | passing runs only |
| P9 | median of last decile ÷ median of first decile, by begin time | ≥2× confirms |
| D3 | `caller=sweep` rows ÷ all rows; sweep elapsed ÷ total | bounds H2 |
| D1 | max single open elapsed vs 5s | see falsifier below |

### The falsifier, restated — my original wording was ambiguous and I am fixing it BEFORE data

I froze it as "if median open cost × opens-per-request exceeds the per-request budget 8-way
queueing allows under the 5s timeout, C1 must not be adopted." Writing out the arithmetic shows
that describes the wrong region. Let **S = opens-per-request × median open elapsed** (a lower
bound on per-request service time — it ignores non-open work).

- **Serialized** (today): the 8th concurrent request completes at ~8S. Timeout when **8S > 5s**,
  i.e. **S > 625ms**.
- **Parallelized** (C1 applied): each completes at ~S. Timeout only when **S > 5s**.

So the regions are:

> **⛔ THE THREE-REGION TABLE THAT STOOD HERE IS DELETED — it encoded the dead v2 model.** It
> labelled `625ms < S < 5s` as "C1 is justified", which is *exactly* the region the corrected model
> and the board's scope rule call **referral**. It survived my own v3 and v4 corrections because I
> appended them *below* it instead of rewriting it — the corrections sat next to the claim they
> killed, in the one table someone would read to fill a cell. L predicted this specific failure
> ("so the table does not encode the old shape") before I found it. Sixth correction to this row.

**Corrected decision table — two regions, not three.** Under the real model opens serialize even
with C1 applied, so `8·O` is a *floor* on the eighth request's latency regardless of threading:

| Measured (rung A alone) | Meaning | Verdict |
|---|---|---|
| **8·O ≥ 5s** (O ≥ 625ms) | opens alone exceed the budget; no handler-threading change can help | **LANE CLOSES: MEASUREMENT AND REFERRAL.** Storm hands to M10's open-reduction work with numbers attached. No serve-layer patch ships. The close doc carries the arithmetic. |
| **8·O < 5s** | C1 *might* help — but only to the extent W_free dominates, and W_free is not collectable by rung A | **BLOCKED ON RUNG B.** Not a fillable cell. Do NOT derive W_free as `total − opens`; that residual contains W_excl and reads high. |

where **O = opens-per-request × median open elapsed**, both from rung A.

### ⚠ MEDIAN IS THE WRONG STATISTIC FOR THE FLAKE — compute the headroom at the TAIL as well

The flake is a **tail event**: at its worst only 4 runs in 10 failed, and within a failing run only
1–5 requests out of ~48 died. So the question "did the convoy cross 5s" is never asked of the
median open — it is asked of the **slowest opens under contention**. A median-based `O` answers a
different question than the one the flake poses.

**Report both, always:**

| Quantity | Statistic | Answers |
|---|---|---|
| `O_median` | median open elapsed × opens-per-request | typical service cost; what the referral branch is defined on |
| `O_tail` | **p99 and max** open elapsed × opens-per-request | whether the convoy can reach 5s on a bad request — the flake's actual question |

The referral branch's threshold stays defined on `O_median` (it is the conservative form: if even
the *median* crosses, nothing can help). But **a non-firing median with a tail near the budget is
a live finding, not a clearance** — it means the machine sits close enough to the cliff that
contention or a slower disk crosses it, which is precisely the flake's mechanism.

### First real input, and it is dramatic

H measured **fsync at 1.5–2.0 ms/op on today's disk**. Each store open does at least one journal
fsync (`sync_loaded_journal`) plus a full O(head) journal load plus ~10 handle opens in
`validate_anchors` — so **≥2 ms is a floor per open**, call it 2–5 ms realistically.

    O ≈ 3 opens × 2–5 ms  ≈  6–15 ms per mutation
    8 · O                 ≈  48–120 ms
    budget                =  5 000 ms

**Today's machine sits roughly 40–100× below the cliff.** Predicted consequence, stated before
Run 2 confirms it: **the referral branch will NOT fire at this disk state.** For the eight-deep
convoy to reach 5 s, per-open cost must reach ~200 ms — some **40–100× today's** — which is why
the same code passes 10/10 now and failed 4/10 this morning. The disk story requires a factor of
that order, and at the *tail* rather than the median it is entirely achievable on a degraded
volume; at the median it would be extraordinary. That is the sharpest quantitative statement this
lane can make about the disk hypothesis, and it is an estimate built on a floor assumption, not a
measurement.

**The referral branch is computable the moment rung A produces rows.** The other branch is not
computable today by anyone, and is written that way on purpose so nobody fills it from a residual.

**Corrected once (`S ≥ 5s`), then corrected AGAIN by L — the table above still assumes something
false.** My original phrasing named `S > 625ms`, the region where C1 *helps*; that was version 2's
fix. But version 2 assumed that under C1 the eight requests proceed in parallel. **They do not, for
the open portion: every open takes a blocking exclusive lock** — `initialize_root_locked`
(core/events/src/local.rs:2134) calls `lock.lock_exclusive()` at :2146, reached from `open_inner`
(:318) at :329. `53d212d` gave *read operations* a shared lock; **the open itself is still
exclusive, and has no timeout.**

Split the per-request cost into **O** (its opens) and **W** (everything else):

| Configuration | 8th request completes at | Times out when |
|---|---|---|
| today (single runtime thread — everything serialized) | ~8·(O+W) | 8·(O+W) > 5s |
| **C1 applied** (handlers parallel, **opens still serialized on the file lock**) | **~8·O + W** | **8·O + W > 5s** |

**C1's entire benefit is therefore ~7·W.** It buys nothing on the open-dominated fraction.

**FALSIFIER, VERSION 3 — fires when `8·O + W ≥ 5s`.** That is the condition under which C1's
parallel service *still* leaves the eighth request over budget. It collapses toward the earlier
form when O is negligible, and it is computable from rows already being collected:

- **O** = opens-per-request × median open elapsed (rung A, server side).
- **W** = single **uncontended** request latency (client side, Patch 1) **minus** that request's O.

**⚠ W MUST COME FROM A MUTATION, NOT FROM THE GET.** My first draft said "from the exactly-one
control" — that control is a *status read*, one open, and its W is not the W the falsifier needs.
The falsifier is about the storm's dominant traffic, which is mutations (three of its four
operations). **Take W from the C2 control's uncontended `signal` POST:**

    W_mutation = (connect_us + write_us + read_us for that POST)  −  (Σ elapsed_us of its
                  caller=request opens)

Pairing is unambiguous there because the control issues one request at a time; both sides carry
epoch-microsecond stamps, so the server rows falling inside the client's request window are that
request's. Do **not** substitute the GET's W — a status read has one open and none of the
mutation's append path, so it would understate the serializing fraction and make C1 look better
than it is. (Same direction as every other error in this row's history, which is why it is called
out here rather than left to whoever runs the arithmetic.)

**⚠ THIS W IS A RESIDUAL, AND IT IS NOT W_free — NEVER SUBSTITUTE IT INTO v4** (L). What the
subtraction yields is *total non-open time*, which still contains **W_excl** (append time held
under the exclusive lock) and **W_shared**. That is the correct input for **v3**, whose model
lumps them together. It is the **wrong** input for v4, whose whole point is that W_excl
serializes. Feeding this residual into v4 as W_free would silently absorb the serialized term,
read high, and bias toward keeping C1 — the same direction as every prior error in this row.
**W_free must be MEASURED (rung B, `with_lock` time split by `Exclusivity`), never derived as
`total − opens`.**

**Consequence — rung A alone gives a ONE-DIRECTIONAL verdict.** With only the open funnel
instrumented, the arithmetic can reach **"refer to M10"** but can never reach **"C1 stays the
candidate"**:

- **Sufficient for referral:** if `8 × opens-per-request × median open elapsed` alone crosses the
  budget, no threading change can help, because that term serializes under C1 too. Decisive on
  rung A data.
- **NOT reachable on rung A:** "W_free dominates, so C1 is justified" — because the only W
  available is a residual containing an unmeasured serialized term. That conclusion needs rung B.

**Consequence to state plainly, because it cuts against the fix I proposed:** if opens dominate —
which the mechanism map suggests, at ≥4 opens per mutation, each a blocking exclusive lock plus a
full journal load plus an fsync — then **W is small and C1 buys very little.** Read the measurement
as "how large is W", not as "is serialization the problem". And note the re-ranking that follows:
**if opens serialize regardless of threading, then reducing the NUMBER of opens (C3) beats
parallelizing the handlers (C1)** — which points the real fix at M10's per-request-open work rather
than at a minimal serve-layer change.

### v3 IS AN OPTIMISTIC BOUND — it UNDER-fires, and the direction is not random

L multiplied v3 out too, and found it repeats the earlier optimism one level down. `with_lock`
(core/events/src/local.rs:540-568) takes the file lock for **every operation**, not just for the
open — `Exclusive => file.lock_exclusive()` at :567 for appends, `Shared => lock_shared` at :568
for reads. So **W is not free-running work.** Partition it:

- **W_excl** — appends. Exclusive lock: serializes against everything.
- **W_shared** — reads. Parallel among readers, but blocked by any exclusive holder.
- **W_free** — pure compute (replay, serialization). Genuinely parallel.

Under C1 the eighth request lands at roughly **8·(O + W_excl) + remainder**, not `8·O + W`. So
**C1's benefit is ~7·W_free, not ~7·W** — and the storm is mutation-heavy by construction (three
of its four operations are mutations), so W_excl is a large share of W *exactly where the flake
lives*.

**Therefore v3 stands as the operative falsifier but is explicitly an OPTIMISTIC BOUND: it fires
LESS often than reality warrants.** A v3 that does not fire is NOT evidence that C1 is justified.

**⚠ The error direction is a pattern, not noise.** v1 erred toward *forbidding* C1. **v2, v3, and
now v4's correction all err toward PERMITTING it** — each successive version fires more readily
than the last, meaning every one of my models understated serialization and overstated my own
fix's benefit. Three of four corrections biased the same way, toward the author's preferred
outcome. Anyone citing this row should weight it accordingly, and anyone producing v5 should
expect the bias to run the same direction again.

**v4 is not computable today.** O comes from rung A; W_free needs time-under-`with_lock` split by
exclusivity, which lives inside `local.rs` — C's pen, rung B territory. **Rung B spec addendum:**
record time held under `with_lock`, split by `Exclusivity`, so W_excl / W_shared / W_free become
separable and v4 becomes computable when rung B runs.

The binding commitment is unchanged in spirit: **if the measurement fires version 3, C1 is refused
even if the storm would pass with it** — and since v3 under-fires, a fire is decisive while a
non-fire is not a clearance.

## 5. Traps already paid for

- **Run duration is not a load proxy.** Failing runs are *not* reliably shorter (isolated fails
  22/21/24/26s vs passes 23/31/29/24/23/24s — overlapping), and the panic truncates the run, so a
  real fix will *increase* wall time. Never use duration for attribution in either direction.
- **Request population differs per run.** A panicking thread issues no further rounds. Normalise
  on requests actually issued (countable from probe rows), never on runs.
- **Never instrument the server to stderr under this harness.** `serve_with` pipes the server's
  stderr and never drains it; >64KB there blocks the server and manufactures the very hang under
  study. Both probes write to files for this reason.
- **`:221` and `:227` both launder the phase** (`get_status` and `get_json`), collapsing
  connect/write/read into one panic site.
