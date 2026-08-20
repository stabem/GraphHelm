# #87 commit 1 — SEALED CELLS, DELTA 2 (2026-08-19)

**Supersedes DELTA 1 in full** (`c2f589915a14440358b8012c5d7162023a0d5cf6b348caf1bae310218afb1841`,
5261 bytes — kept, untouched, as the record of what was sealed while kind-by-protocol stood).
**The ORIGINAL seal is untouched and still governs C1–C9:**
`064de7be132271336d7db2be7e513eea2afca03df2869c48a43b89070f39840a`, 7958 bytes.

Hash-pinned artifacts are immutable; this is a dated delta, not an edit.

---

## What changed

**The label landed in the instrument.** The orchestrator's order superseded A's kind-by-protocol
proposal, and A implemented it: `load_state` takes `kind: &'static str`; a `cfg(test)`
`loads_by_kind` map records **(full, suffix, hit) per operation**; **12 call sites stamped**
(`open`, `append`, `next_sequence`, `read_replay_stream`, …); and a **committed guard asserts the
exact hot-scenario derivation**:

| Operation | (full, suffix, hit) | Why |
|---|---|---|
| `open` | **(1, 0, 0)** | the load that warms the handle |
| `append` | **(0, 0, 1)** | **hits the cache the open just filled** |
| `next_sequence` | **(0, 1, 1)** | the **FIRST** call pays the suffix for the line the append wrote; the **second** is a pure hit |

**The map is per-kind AGGREGATED ACROSS THE SCENARIO, not per call.** `(0, 1, 1)` for
`next_sequence` is two calls summed. **A cell that reads it as one call is incoherent** — stated
here because that is the misreading available to anyone meeting the number cold.

**Recorded because it happened before the run, not after:** deriving the counts from the mechanism
**corrected a wrong assert A was about to commit** — he was going to assert a pure hit on the FIRST
`next_sequence`, and the derivation showed it pays the append's suffix. **An error found by
working the example, at the cheapest possible moment: typed, unbuilt, unrun.**

---

## DELTA 1'S CONDITIONS ARE NOW MOOT — stated explicitly so they do not outlive their premise

Delta 1 priced kind-by-protocol with two conditions. **The label carries attribution INSIDE the
datum, so both are discharged and neither remains in force:**

- **Isolation control (empty window ⇒ delta 0): NO LONGER REQUIRED for attribution.** It was the
  price of a protocol whose precondition was invisible. *(It remains a fine assertion; it is simply
  no longer load-bearing.)*
- **Protocol-travels-with-the-numbers: NO LONGER REQUIRED.** The kind is in the record itself; a
  lost protocol note no longer makes the table uninterpretable.

**An instruction whose premise has died must be voided out loud, not left standing.** That is the
same fault the board just adopted a fix for; this is me applying it to my own conditions.

---

## THE NEW FAILURE MODE THE LABEL INTRODUCES

**A label can be WRONG, and a mislabelled site is silent in the data.** Delta 1's method could not
mis-attribute if isolation held; **the label can mis-attribute while every count looks plausible.**

- **Sum reconciliation catches a DROPPED or DOUBLED site, NOT a SWAP.** If `loads_by_kind` totals
  equal the global `full_load_count` / `suffix_load_count`, a missing or extra stamp shows up.
  **Swap two labels and every total is unchanged.**
- **The committed derivation guard IS the anti-swap control — for the sites it exercises.**
  `open = (1,0,0)` would fail if the open's site were stamped `append`. That covers `open`,
  `append`, `next_sequence`.
- **SEALED GAP: of the 12 stamped sites, only those exercised by the derivation guard have
  VERIFIED labels. Every other site's stamp is UNVERIFIED — asserted by hand, checked by nothing.**
  Not a blocker for the cells below, all of which live inside the guarded scenario. It is a claim
  about the other sites that must not travel as though the guard covered them.

---

## Cells re-sealed (supersede K1–K4)

| # | Prediction | CONFIRMS | KILLS | UNSCOREABLE |
|---|---|---|---|---|
| **L1** | `open` = (1, 0, 0) | exactly that | any other triple | counters absent or not per-kind |
| **L2** | `append` = (0, 0, 1) — hits the open's cache | exactly that | full or suffix > 0 | — |
| **L3** | `next_sequence` = (0, 1, 1) **aggregated over the scenario's two calls** — first pays the append's suffix, second is a pure hit | exactly that | any other triple, **including (0,0,2)**, which was A's pre-derivation expectation | the scenario's call count differs from two, making the aggregate incomparable |
| **L4** | Sum of `loads_by_kind` == global `full_load_count` + `suffix_load_count` | equal | any difference ⇒ a **dropped or doubled** stamp | — |
| **L5 (UNINFORMATIVE, named)** | Label correctness of the **9 sites outside the derivation guard** | — | — | **DISTINGUISHES NOTHING: their stamps are unverified by construction. L4 would catch a dropped stamp; NOTHING here catches a swap between two unexercised sites.** |

## Unchanged

**C1–C8 stand** (per-handle and per-state; never needed the kind dimension). **C9 stands** (wall at
~100 distinguishes nothing). **Scope fence stands** (commit 1 is the cache alone; Metric-B-flat-for-
serve is commit 2's criterion). **Inherited condition stands** (in-place-rewrite exposure bounded by
handle lifetime — true at commit 1, false at commit 2). **All binding conditions stand**
(fresh-paired baseline, `free_gb` + concurrent-cargo, three sizes, N≥10, median/p99/max, serve and
CLI never pooled, not-a-result classes).
