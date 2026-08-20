# #87 commit 1 — SEALED CELLS, DELTA 4 (2026-08-19)

**Supersedes ONLY L5** (delta 2) and **adds one reading hazard**. Everything else in the chain
stands. **Enumerated explicitly, because delta 2 taught that "supersedes in full" silently unseals
what it forgets.**

Chain, all pins untouched:
- ORIGINAL `064de7be132271336d7db2be7e513eea2afca03df2869c48a43b89070f39840a` (7958) — C1–C9
- DELTA 1 `c2f589915a14440358b8012c5d7162023a0d5cf6b348caf1bae310218afb1841` (5261) — historical
- DELTA 2 `3292d60ba349191b5603fc2f83a0125272567d3c9259087bf3a7f853193d3a53` (5409) — L1–L5
- DELTA 3 `c966059ac3c15d5d…` (4242) — L6 restored, condition-1 election
- **DELTA 4 — this file** — L5′, L7

---

## 1. L5 SHRANK — instrument change, so a delta and not an amendment

A added a **new guard, `by_kind_probe`**: seven read operations exercised **once each on a warm
handle**, each asserting its label shows exactly **(0, 0, 1)**.

- **10 of 12 stamps are now VERIFIED BY OBSERVATION**, up from 3.
- **The 2 remaining unverified: `committed_events_for_idempotency` and the test site.** A names
  them in the evidence plan and states they do not travel as covered.

**L5′ (supersedes L5):** **two** stamps are asserted by hand and checked by nothing —
`committed_events_for_idempotency` and the test site. **Named, not counted.** The gap is now small
enough to enumerate, which is a different kind of claim than "nine unverified".

- **L4 gains context rather than changing:** sum reconciliation still cannot catch a **swap**, and
  **the derivation guards are still the only anti-swap control — but they now cover 10 sites
  instead of 3.** A swap between the two unverified stamps remains undetectable by anything in the
  suite.

## 2. L7 — NEW READING HAZARD, and it is the one most likely to be "resolved" by picking a side

**THE TRIPLE IS SCENARIO-DEPENDENT. IT IS NOT A PROPERTY OF THE OPERATION.**

- In the hot-scenario guard, `next_sequence` = **(0, 1, 1)** — the first call pays the suffix for
  the line the append wrote.
- In `by_kind_probe`, `next_sequence` = **(0, 0, 1)** — no append intervenes, so it is a pure hit.

**Both are sealed cells and both are correct.** A reader comparing them will see the same operation
with two different triples and reach for a contradiction. **There is none: the triple is a fact
about the SCENARIO, not about the operation.**

- **Sealed refusal:** neither number may be cited as *"`next_sequence` costs X"* without its
  scenario. **An operation has no context-free cost in this instrument.**
- **This is the same failure the aggregation note already guards** (the triple is a scenario sum,
  not a per-call value) — one level out: **not only "how many calls", but "which scenario".**

## 3. A's other change, recorded

**The aggregation note now lives IN THE CODE beside the `(0,1,1)` assert** — *"triples are scenario
sums, not per call"* — rather than only in a sealed file. **Correct placement: the seal is not open
when someone reads the assert.** Same principle as the control caveat going into the design file,
and as the in-place-rewrite trigger living at the reuse site.

## 4. SCORING BASE — A's stated base is now incomplete

A says the table comes **scored against original + delta 2.** **The current base is ORIGINAL +
DELTA 2 + DELTA 3 + DELTA 4.** Delta 3 restored **L6** (per-kind under concurrency distinguishes
nothing at commit 1 — because the map is `cfg(test)` and real serve is uninstrumented), which
delta 2 had silently dropped. **Scoring against delta 2 alone would leave L6 unapplied.**

## Unchanged

**C1–C9.** **L1–L4** (delta 2). **L6** (delta 3). **Condition 1 kept by A's election; condition 2
dissolved.** **Scope fence.** **Inherited condition.** **All binding conditions** — fresh-paired
baseline, `free_gb` + concurrent-cargo, three sizes, N≥10, median/p99/max, serve and CLI never
pooled, not-a-result classes.
