# #87 commit 1 — SEALED CELLS, DELTA 3 (2026-08-19)

**Repairs an OMISSION IN DELTA 2, which is mine.** Delta 2
(`3292d60ba349191b5603fc2f83a0125272567d3c9259087bf3a7f853193d3a53`, 5409 bytes) said it
"supersedes DELTA 1 in full" — **and in doing so it dropped K4's substance without carrying it
forward.** Delta 2 stands otherwise; **this adds back what it lost and corrects the reason.**

Prior pins, all untouched:
- ORIGINAL `064de7be132271336d7db2be7e513eea2afca03df2869c48a43b89070f39840a` (7958 bytes) — C1–C9
- DELTA 1 `c2f589915a14440358b8012c5d7162023a0d5cf6b348caf1bae310218afb1841` (5261 bytes)
- DELTA 2 `3292d60ba349191b5603fc2f83a0125272567d3c9259087bf3a7f853193d3a53` (5409 bytes) — L1–L5

---

## 1. THE OMISSION — per-kind attribution under CONCURRENCY became unsealed

Delta 1's **K4** named it: per-kind attribution under concurrency **distinguishes nothing at
commit 1**. Delta 2 replaced K1–K4 with L1–L5, and **L5 is a different uninformative** (unverified
stamps on the nine unexercised sites). **So the concurrency limit stopped being sealed anywhere.**
A caught it by holding K4 while my delta had already dropped it.

**L6 (UNINFORMATIVE, restored):** per-kind attribution under **concurrency** distinguishes nothing
at commit 1.

**AND THE REASON HAS CHANGED WITH THE MECHANISM — this is not K4 re-pasted.**

| Method | Why concurrency defeats per-kind attribution |
|---|---|
| kind-by-protocol (delta 1) | the delta window **cannot isolate concurrent payers** |
| kind-by-label (delta 2/3) | **the map is `cfg(test)` and guarded by a mutex — serial in the guards, and REAL SERVE IS NOT INSTRUMENTED AT ALL** |

**Same conclusion, different mechanism.** Under the label the limit is not that attribution fails
under concurrency — **it is that the instrument does not exist on the concurrent path.** Commit 2
owns it, where rung A's `with_caller` is the ready precedent.

**Recorded as a keeper's error:** "supersedes X in full" is a claim about coverage, and I made it
without checking that everything X sealed had a home in the successor. **A superseding document
must enumerate what it carries forward, or it silently unseals whatever it forgot.**

## 2. CONDITION 1 — A ELECTS TO KEEP IT; recorded, and it is a strengthening

Delta 2 discharged the isolation control as **no longer load-bearing for attribution** (the label
put attribution inside the datum). **A keeps it anyway in the measurement runs** — a window with no
store operation must show **delta == 0 across all kinds**, one assertion per run.

- **His reason is correct and is not the one the condition was written for:** it no longer proves
  *which* operation paid; it still proves **nothing unintended incremented**.
- **Consequence, sealed: if that control ever fails, it is a real finding and not a formality** —
  it would mean a counter moves without a named operation, which would undermine L1–L4 at once.
- **Condition 2 (protocol travels with the numbers) remains DISSOLVED**, as delta 2 sealed. The
  attribution is intrinsic to the datum; a lost note costs nothing.

## 3. A's ARGUMENT FOR KEEPING THE LABEL — accepted on my own criterion

A points out that the typed instrument satisfies **the test I wrote for delta 1**: *a label carries
attribution INSIDE the datum, findable later by inspection.* **That is now literally the case**, and
the production cost is one `&'static str` argument — not the new mechanism I had priced as too
heavy for a minimal commit.

**So my delta-1 verdict ("do not type the label") is superseded by execution, and by my own
criterion rather than by the order alone.** Recorded that way: the order arrived first and had
already been executed when my ruling landed; **the ruling would have been wrong on the merits by
delta 1's own standard**, because it assumed a heavier mechanism than the one A built.

## Unchanged

**C1–C9** (original seal). **L1–L5** (delta 2), including the sealed gap that only the
guard-exercised sites have verified labels. **Scope fence.** **Inherited condition.** **All binding
conditions** — fresh-paired baseline, `free_gb` + concurrent-cargo, three sizes, N≥10,
median/p99/max, serve and CLI never pooled, not-a-result classes.
