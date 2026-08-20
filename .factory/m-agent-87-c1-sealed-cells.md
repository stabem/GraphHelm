# #87 commit 1 — SEALED PREDICTION CELLS

Sealed by M Agent **BEFORE any run**, so A's slot is never blocked on sealing.
Package read directly, not from summaries: `.factory/a-agent-m10-d1-design.md` (shared checkout),
`.factory/a-agent-issue87-core.patch` (a-agent-c35d10 worktree, +471/−21, **events crate only** —
`core/events/src/jsonl.rs` and `core/events/src/local.rs`), branch `issue-87-verified-prefix-cache`
@ `0f4e7fe`.

**Scoring authority:** none. I hold the wording; the orchestrator and reviewers hold verdicts.
**Nothing here is measured by me. I ran no cargo.**

---

## 0. BINDING CONDITIONS — inside the seal, not alongside it

A result that violates any of these **scores UNSCOREABLE, not confirmed or killed.**

1. **FRESH-PAIRED BASELINE.** Before/after measured in the **same session and the same disk
   state**. **M09 and storm figures are PREMISE, NEVER the comparison base** — that confound cost
   the storm lane its attribution and must not be re-imported.
2. **MANDATORY PER-LINE FIELDS: `free_gb` and concurrent-cargo count.** A run table missing either
   is unscoreable. (The lane recorded zero machine variables while testing a machine-bound
   hypothesis; that is not repeated here.)
3. **Journal sizes ~100 / ~1k / ~5k events; N ≥ 10 per point; median AND p99 AND max** — never a
   median alone.
4. **SERVE PATH AND CLI ONE-SHOT MEASURED SEPARATELY**, never pooled. They have different base
   rates and different handle lifetimes; pooling them manufactures a bound neither earns.
5. **POSITIVE CONTROLS COMMITTED** (already done by A): each zero-or-equality claim asserts its
   counter moved before asserting the delta. Absent one, that row is unscoreable.
6. **NOT-A-RESULT:** compile failure, harness error, hang. Re-register; no cell scored.

---

## 1. THE SCOPE FENCE — the most likely way this gets mis-scored

**Commit 1 is the events-crate cache ALONE. Per-request opens REMAIN in place.** The design's
§1.5 (ServeState holding one long-lived repository) is **commit 2**, not this one.

**Consequence, sealed:** at commit 1 each serve request still gets a **FRESH handle**, so the cache
is **cold at every request boundary**. Per-handle behaviour is **1 full load at open + N suffix
hits for subsequent operations on that handle** — against today's **full load per operation**.

- **THE DESIGN'S SUCCESS CRITERION BELONGS TO COMMIT 2, NOT HERE.** §2 says *"Success = Metric B
  flat in journal size for warm-handle serve ops."* **At commit 1, Metric B will NOT be flat for
  serve**, because the handle is not warm across requests. **That is the scope, not the cache
  failing, and it must not be scored as a miss.**
- **CLI one-shot is where commit 1's within-handle amortisation is visible**, since one command
  holds one handle across many operations. A's design already says CLI is "out of D1's scope,
  priced for D2" — **for the long-lived-handle benefit. The within-handle cache benefit is visible
  at commit 1 and is the cleanest place to observe it.**

---

## 2. CELLS

| # | Prediction | CONFIRMS | KILLS | UNINFORMATIVE / UNSCOREABLE |
|---|---|---|---|---|
| **C1** | Per handle, quiescent journal: after the open's load, subsequent ops do **0 full, 0 suffix** | both counters flat across all three sizes, **positive control passed** | either counter increments on a quiescent handle | control absent → **UNSCOREABLE** |
| **C2** | Per handle, 1 append between ops: **suffix +1, full +0** | exactly that | suffix ≠ +1, or full > 0 | the append is not observed to land between the two ops |
| **C3** | Per handle lifetime: **exactly 1 full load** (the open), **N suffix** for N subsequent ops | full == 1 for the handle at all sizes | full > 1 on a handle whose journal was never replaced or truncated | handle boundaries not recorded, so loads cannot be attributed to a handle |
| **C4** | **State equality by value**, hot vs from-scratch, across `batches` (full content, down to each `event_hash`), `next_sequence`, `last_hash`, `seen_idempotency`, `reachable_evidence`, `artifacts`, `active_versions`, `expected_markers`, `verified_offset` | all equal | any divergence | — |
| **C5** | **Budget equality**, incremental vs from-zero, with **non-zero budget consumed** | exact equality, controls passed | any difference | fixture consumes no budget → **VACUOUS** |
| **C6** | **No error changes class**: discriminant hot == fresh across **exercised** error paths | equal | any differs | no error path triggered → **VACUOUS** |
| **C7** | `len < verified_offset` → **full reload from zero**, matching today byte-for-byte | full +1 and state equals from-scratch | partial reuse, or state diverges | truncation not actually induced |
| **C8** | Mid-line truncation → **Integrity error**, same as today's whole-file rule | Integrity, discriminant unchanged | any other class | — |
| **C9 (UNINFORMATIVE, named in advance)** | Wall-clock of individual ops at **~100 events** | — | — | **DISTINGUISHES NOTHING: hit and full are both fast at that size. THE COUNTERS CARRY THE ENTIRE CLAIM AT ~100, which is why C1/C3's positive controls are load-bearing there and the wall is only a second witness at ~5k.** |

---

## 3. AMBIGUITY I CANNOT SEAL AGAINST — named to A, per the assignment

**THE PATCH'S COUNTERS ARE NOT PER OPERATION KIND, AND THE DESIGN REQUIRES THAT THEY BE.**

- **Patch as typed:** `full_load_count` and `suffix_load_count`, both `Arc<AtomicU64>`, incremented
  inside `load_state`. **Verified: ZERO occurrences of any operation-kind label in the patch** — no
  `OperationKind`, no `op_kind`, no kind field.
- **A's own design §2 requires the opposite:** *"emit per OPERATION KIND (open / read_replay /
  next_sequence / append), never flattened into one counter (M09 lesson: flattening in instruments
  — one bucket made F2-H2 unfalsifiable; the metric must name which operation paid)."*
- **So the instrument distinguishes full-vs-suffix but NOT WHICH OPERATION PAID** — which is the
  flattening lesson, in A's own instrument, against A's own stated requirement.
- **CONSEQUENCE, SEALED: any per-kind cell is UNSCOREABLE at commit 1.** Metric A ("load_state
  calls per logical operation") **cannot be computed from these counters.** Either the kind
  dimension is added, or per-kind claims wait for commit 2.
- **This is a design finding, not a blocker:** C1–C8 above are all per-handle or per-state and
  **do not require the kind dimension.** They stand.

---

## 4. INHERITED CONDITION — activates at commit 2, recorded now so it is not rediscovered

A's patch comment carries a **named trigger** worth preserving as a condition rather than prose:

> the reuse test catches replacement (identity) and truncation below the offset (length); it
> **CANNOT catch an in-place rewrite of already-verified bytes**, and the lock is no answer — the
> per-line verification exists for writers that never take the lock. That exposure is **bounded by
> HANDLE LIFETIME, which is milliseconds under per-operation opens.**

- **That bound is TRUE AT COMMIT 1 AND FALSE AT COMMIT 2.** When handles become long-lived, the
  exposure window grows from milliseconds to the process lifetime.
- **Sealed as an inherited condition:** *while per-request opens remain, the in-place-rewrite
  exposure is bounded by handle lifetime. The commit that makes handles long-lived MUST revisit
  this or bound the reuse (full re-verify every N loads or on a time bound).* A already named the
  trigger; this records it where a scorer will meet it.

---

## 5. WHAT THIS SEAL DOES NOT COVER

- **No per-kind cells** (see §3).
- **No serve-warm-handle cells** — that is commit 2 by construction.
- **No claim about the owner's motivating numbers** (48% / ~3× / 15→36 ms). A's design already
  MARKS them **UNSOURCED-IN-REPO**, verified 2026-08-19. **Step zero reproduces them or replaces
  them; neither this seal nor commit 1 establishes them.**
