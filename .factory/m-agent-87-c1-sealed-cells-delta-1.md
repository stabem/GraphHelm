# #87 commit 1 — SEALED CELLS, DELTA 1 (2026-08-19)

**Supersedes NOTHING in the original seal except §3 (the per-kind ambiguity).**
Original: `.factory/m-agent-87-c1-sealed-cells.md`,
`SHA256 064de7be132271336d7db2be7e513eea2afca03df2869c48a43b89070f39840a`, 7958 bytes —
**untouched and still valid for everything it covers. C1–C9 stand as sealed.**

A hash-pinned artifact is immutable; amendments are appended as dated deltas, never edited in.
Otherwise the pin certifies nothing.

---

## What changed

A proposed resolving the per-kind gap **by PROTOCOL rather than by LABEL**: in commit 1's
measurements — serial by construction (crate guards and CLI one-shot, one operation at a time) —
each per-kind cell measures the **delta of `full_load_count` / `suffix_load_count` around ONE
isolated call of ONE named operation** (`next_sequence` alone; `append` alone; `open` alone).
**Kind is established by construction of the run, not by a field in the datum.**

He named the honest limit himself: **under concurrency the delta does not isolate and the label
becomes necessary**, so concurrent per-kind attribution waits for commit 2. And he offered to type
the label now if I required it, while noting the cost is new mechanism in a commit he wants
minimal. **He did not self-serve the verdict.**

## RULING — ACCEPTED FOR COMMIT 1, with two conditions

**The method is sound where he proposes it, and his reasoning for why is correct:** the flattening
lesson (F2-H2) bit because **CONCURRENT** rows fell into one bucket. In a serial, single-call
window **there is no second payer**, so nothing can merge. **Accepted.**

**But it fails DIFFERENTLY from a label, and that difference sets the conditions:**

- **A LABEL carries attribution INSIDE the datum.** A mislabelled call site is an error **in the
  data**, findable by inspection later.
- **A PROTOCOL carries attribution OUTSIDE the artifact.** If isolation is violated, or if the
  protocol note is lost, **the numbers look exactly the same** — and no later inspection of the
  numbers can recover which operation paid.

### Condition 1 — ISOLATION CONTROL (the precondition must be shown, not assumed)

**A window containing NO store operation must show delta == 0**, asserted in the same run.
Without it, isolation is **assumed**, and an assumed precondition is where the error hides. This
is the same positive-control discipline already committed for C1/C4/C5 — applied to the protocol
instead of to a counter.

- **Rationale, stated so the condition is not read as ritual:** if the intended operation
  increments 0 while something unintended increments 1, the delta is 1 and is **silently
  mis-attributed**. The isolation control is what makes that case impossible rather than unlikely.
- *Note in A's favour:* the commoner failure — a second operation also incrementing — produces a
  delta LARGER than predicted, which **kills the cell loudly** rather than passing quietly. The
  exposure is narrow; the control closes it.

### Condition 2 — THE PROTOCOL TRAVELS WITH THE NUMBERS

**Each per-kind measurement's run record must state, per row, which single operation the window
enclosed.** With a label, a lost protocol note costs nothing. **With this method the protocol IS
the attribution** — lose it and the numbers become uninterpretable, permanently, because nothing
in the artifact recovers it.

- **This is the intrinsic-vs-instrumented distinction, applied to attribution:** a label would be
  **intrinsic to the datum**; the protocol is **separate from it**. Separate evidence must be
  captured while it exists.

## Cells re-sealed

| # | Prediction | CONFIRMS | KILLS | UNSCOREABLE |
|---|---|---|---|---|
| **K1** | `open` alone: **full +1, suffix +0** | exactly that, isolation control passed | any other delta | isolation control absent or failing |
| **K2** | `next_sequence` alone on a warm handle, quiescent journal: **full +0, suffix +0** | both zero, isolation control passed | either increments | isolation control absent; or the handle's warmth not established in the same run |
| **K3** | `append` alone: **suffix +1, full +0** on the following read; the append's own path unchanged | exactly that | full increments, or suffix ≠ +1 | isolation control absent |
| **K4 (UNINFORMATIVE, named)** | Any per-kind attribution **under concurrency** | — | — | **DISTINGUISHES NOTHING at commit 1: the delta method cannot isolate concurrent payers. Concurrent per-kind attribution is COMMIT 2's, where the label becomes necessary and rung A's `with_caller` is the ready precedent.** |

## What this delta does NOT change

- **C1–C9 of the original seal stand exactly as sealed.** They are per-handle or per-state and
  never required the kind dimension.
- **The scope fence stands** (commit 1 is the cache alone; Metric-B-flat-for-serve is commit 2's
  criterion; a commit must not be blamed for what it deliberately does not do).
- **The inherited condition stands** (in-place-rewrite exposure bounded by handle lifetime — true
  at commit 1, false at commit 2).
- **All binding conditions stand** (fresh-paired baseline, `free_gb` + concurrent-cargo, three
  sizes, N≥10, median/p99/max, serve and CLI never pooled, not-a-result classes).
