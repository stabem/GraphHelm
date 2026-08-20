# Procedure: publishing the gate's compile/test split

D Agent, 2026-08-20. Operationalises **#153's third binding addendum** — item 2's re-derivation
valve may only be opened *after* the compile/test split is published from real #152 manifests.

Written slot-free; **nothing here has been run.** It is a procedure, not a result. Whoever holds
the slot after F (#152) can execute it with the gate they were going to run anyway.

---

## 0. The problem this exists to solve

**Per-stage wall time is not the compile/test split.** A `cargo test` stage compiles *and* runs;
its single duration mixes both. Summing stage times answers "how long is the gate", which is
already known, and not "how much of it is compilation", which is the number the bar turns on.

So the split has to be *decomposed*, and the cheapest honest decomposition uses two runs of the
same gate rather than any new instrumentation.

## 1. The measurement

On one held slot, at one commit, with nothing else on the machine:

1. **Run A — COLD.** Slot-start clean of all workspace-own packages (the standing law), then the
   full gate. This is the gate as it actually runs: every slot starts clean, so **run A is the
   honest serial baseline.**
2. **Run B — WARM.** Immediately after A completes, run the identical gate again, touching
   nothing in between. No clean, no edits, no rebase.

Per stage *k*:

```
compile_k  ≈  A_k − B_k          (lower bound — see §3)
test_k     ≈  B_k                (upper bound — see §3)
```

Summed over the 27 stages, that is the split.

**Both runs must record the sha AT LAUNCH, and exit codes read from the producing command itself**
— never through a pipe or a `;`-chain, which report the last element rather than the gate.

## 2. What to publish

A table (stage, A, B, A−B) plus three numbers:

- **`s` = compile fraction** = `Σ(A_k − B_k) / Σ A_k`
- total A (the serial baseline item 2 is measured against)
- total B (an instrument reading — **see the trap in §4**)

## 3. Why these are bounds, not point estimates — and the direction matters

**`compile_k` is a LOWER bound.** Cargo fingerprints per feature/flag/profile set. The gate's
stages do not all share one: `clippy --all-targets --all-features` and a plain `cargo test` are
different fingerprints, so run B may still recompile when consecutive stages disagree. Any such
rebuild lands in `B_k`, which *understates* compile and *overstates* test.

Consequence, and it is the useful direction: **the real compile fraction is at least `s`.**

**If run B's stages show large residual times where no test work exists, that is itself a finding**
— it means the gate's stage ordering thrashes the cargo cache, which is phase-3 (compile-once)
evidence and should be reported rather than smoothed away.

## 4. The trap: run B must never become item 2's denominator

Run B is an **instrument for decomposition only.** It is fast because the work was already done by
run A. Measuring the gate-graph against B would compare a cold graph to a warm serial gate and make
item 2 look far harder than it is — or, if someone warms the graph too, compare two warm runs and
make it trivially easy. **The baseline for item 2 is run A**, because a real slot always starts
clean. Any published comparison must name which run it used.

## 5. Reading the number against item 2 — the arithmetic that makes this decidable

Item 2 asks for ≤60% of serial wall-clock. Compile is serialised by design (#153 makes compile-lock
an exclusive resource edge; concurrent `cargo` invocations contend on one target dir anyway). With
compile fraction `s` and `P` effective parallel workers on the rest, the best achievable ratio is

```
s + (1 − s)/P
```

Setting that ≤ 0.6 gives the **maximum compile fraction item 2 can tolerate**:

```
s ≤ (0.6 − 1/P) / (1 − 1/P)
```

| P (parallel workers) | max tolerable `s` |
|---|---|
| 4 | **0.47** |
| 8 | **0.54** |
| 16 | **0.57** |
| ∞ | **0.60** |

**So if measured `s` exceeds ~0.60, item 2 is unreachable at any core count**, and the honest
options are to land phase 3 (compile-once) first — which lowers `s` directly — or to re-derive
item 2 *with this published number as the stated reason*, which is exactly what the addendum
permits and what it requires evidence for.

## 6. The asymmetry — this procedure can refute item 2, never certify it

The model above is generous on purpose: it assumes compile is perfectly serial and everything else
perfectly parallel, with no scheduling loss, no lease contention, no per-node event-store overhead.
Real execution is worse. Therefore:

- **`s` above the threshold is decisive** — it refutes reachability, because even the idealised
  bound fails.
- **`s` below the threshold proves nothing.** It removes one known obstacle. Item 2 still has to be
  met by measurement, on the async driver, per L's addendum.

Stating this before the number exists is the point: a result that can only ever kill or fail to
kill is harder to bend afterwards than one that can be read as support.

## 7. Dependencies and fallbacks

- **Needs #152's manifest** for per-stage wall time. Until it lands, the same procedure runs on any
  per-stage timing the gate can be made to print — the method does not depend on the manifest's
  format, only on having `A_k` and `B_k`.
- **If #152's manifest ships without per-stage wall time**, this procedure is blocked and that
  should be raised against #152 rather than worked around with hand timing, which would not be
  reproducible by the next person.
- **Two runs cost roughly one cold gate plus one warm gate** on an already-held slot. No extra slot
  is needed if it is folded into a run someone was making anyway.

## 8. What this procedure does not establish

- **Nothing is measured here.** No cargo was run to write it; every number above is algebra, not
  observation.
- **It says nothing about which driver executes the gate-graph.** L's rule stands independently:
  a measurement must declare the driver it touched, and a CLI-path measurement cannot certify a
  wall-clock item at all.
- **`P` is unknown.** The table is parameterised by it deliberately; nobody has established how many
  gate stages are genuinely independent, and that enumeration is separate work.
- **The bound assumes phase 2's target-dir scheme does not change compile cost.** If a
  content-addressed pool changes cache hit rates, `s` must be re-measured after it lands rather
  than carried forward.
