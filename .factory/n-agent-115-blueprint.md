# N Agent — #115 blueprint: ReuseDecision observability (RA steps 16–17)

Read-only design deliverable. Base for every citation: `origin/main`. I ran no cargo and wrote no
code. CITE-or-MARK throughout: every claim about the tree names where I read it; every claim I did
not verify is marked.

---

## 0. PREMISE CORRECTION — read this before the rest

The brief states: *"today no ReuseDecision in the event vocabulary, no token accounting surface."*

**The first half is false.** `ReuseDecision` exists on `origin/main`:

- the kind: `core/protocols/src/event.rs:180` — `ReuseDecision(ReuseDecision)`
- the payload: `core/protocols/src/event.rs:665`, carrying `execution_id`, optional `node_id`,
  `plane`, `decision`, `forced_reason`, `freshness_class`, `key_components`, `key_digest`,
  `evidence_ref`, `provenance_erased`
- the fold treats it as LEDGER, not state: an explicit no-op arm at
  `core/events/src/projection.rs:1020`
- exactly ONE production emitter: `core/runtime/src/driver.rs:116`

So this is not a design-from-nothing. It is an **extension of a shipped, audited event**, and that
changes the shape of the work: the key/audit half is done and must not be redesigned, while the
half steps 16–17 actually need is absent.

The second half of the premise holds: there is no token accounting surface on this event.

**Why the correction matters more than the fact.** A blueprint written to the brief would have
proposed a new event, and a new event competing with a shipped one is how a vocabulary acquires
two ways to say the same thing. The existing `key_digest` is described in its own doc comment as
"auditable and joinable" — joinability is exactly what step 17's *cited* events need, and it
already exists.

---

## 1. What steps 16–17 require, quoted

**Step 16** (RA §3.4): *"the execution's context-efficiency figure (§9.2) and per-node token counts
are visible in local observability alongside the recorded full-context estimate"*.

**Step 17**: *"the system demonstrates measurably lower compiled-context cost through reuse —
capsule compilation cache hits, provider prompt-cache hits, agent reuse — with the cited
`ReuseDecision` events visible in the timeline (reuse across distinct work, not replay of identical
work)"*.

**§11.2** additionally requires *"cited `ReuseDecision` event ids decomposing which reuse mechanisms
produced each reduction"*.

Three requirements fall out, and they are separable:
- **R1** — the event must be able to say WHICH MECHANISM reused (step 17 names three; §11.2 wants
  the decomposition).
- **R2** — the event must be able to say WHAT IT SAVED (§11.2's "produced each reduction").
- **R3** — the figure and per-node counts must be VISIBLE, with the full-context estimate beside
  them (step 16).

---

## 2. R1 — the mechanism vocabulary is 1-of-3, and the one it has is not among the three

`ReusePlane` has exactly one variant (`core/protocols/src/event.rs`, enum read at main):

```
pub enum ReusePlane {
    ToolBroker,
}
```

Step 17 names **capsule compilation cache**, **provider prompt-cache**, **agent reuse**. `ToolBroker`
is none of them. So the vocabulary today covers **zero of the three mechanisms step 17 requires**,
and §11.2's decomposition is not expressible at all.

**Design: extend the plane, do not add an event.** Three new variants, one per named mechanism, and
`ToolBroker` retained (it is shipped and emitted). The enum is the natural home because `plane` is
already the field whose job is "which reuse machinery decided this".

**THE G1 LESSON APPLIES HERE, and it is the single most likely way this work ships broken.** On #81
the enum gained a variant and the fix was only real because the CODE it maps to was also new —
`BackupError::code()` already collapsed two variants onto one string, so a variant-grain assertion
could have passed while the operator surface stayed ambiguous. The same trap is available here:

> A four-variant `ReusePlane` is worth nothing if the timeline surface renders reuse as a single
> undifferentiated "reused" row. The distinction must survive to **where the operator reads it**,
> not merely exist in the enum.

**MARK, not verified:** I did not audit the observability/timeline surfaces to determine whether
`plane` is rendered anywhere today. That audit is a prerequisite for R1 being callable done, and it
should be done at the RENDERER, not the enum — check what produces the row the operator sees.

---

## 3. R2 — what the event must record about savings

The event today records **identity** (what was reused, keyed and digested) and **outcome**
(hit/miss/forced-fresh/excluded). It records **nothing about cost**. So "what it saved" has no home,
and §11.2's decomposition cannot be computed from the ledger.

**Design constraint from §9.2, which already fixes the arithmetic** — this is pre-registered in the
RA and must NOT be re-derived here:

```
1 - tokens_sent_with_compiler / estimated_tokens_full_context
```

with, quoted from §9.2: the estimator "versioned and deterministic"; "the method id includes the
retrieval-recipe version; a recipe change starts a new series (no cross-series comparison)"; the
§11.2 paired inline arm as "the calibration reference, with a declared error tolerance that flags —
never blocks"; and the v1 estimator as "a free compiler byproduct" where "the capsule manifest
records `eligible_candidate_tokens` and `tokens_saved = eligible − shipped` per node, stated
explicitly as a conservative lower bound".

**So the formula is already pre-registered and the retrofit risk is not that someone invents a
number — it is that someone computes a DIFFERENT number and calls it §9.2.** The guard that follows:

- **G-ARITH-1:** the figure's producer must carry the method id including the retrieval-recipe
  version, and two figures with different method ids must be refused comparison BY THE SURFACE, not
  by a convention in a doc. A series break that only exists in prose will be crossed.
- **G-ARITH-2:** `tokens_saved = eligible − shipped` is a **lower bound by construction**. Any
  surface that presents it as "the saving" without that qualifier misreports it. The word appears in
  §9.2 and must survive to the operator.
- **G-ARITH-3:** the calibration tolerance **flags, never blocks** (§9.2). A surface that hides a
  flagged figure, or an implementation that refuses to emit one, contradicts the RA.

**Where the savings numbers attach — per NODE, not per event.** §9.2 puts `eligible_candidate_tokens`
and `tokens_saved` on the **capsule manifest, per node**. The `ReuseDecision` event should therefore
NOT duplicate them; it should carry the join that lets a reader reach them. It already has one:
`key_digest`, documented as "auditable and joinable, content-free". Duplicating token counts onto the
event would create two sources for one number, which is how they drift.

**Minimum addition to the event:** enough to decompose §11.2's reduction by mechanism — the plane
(R1) plus the join. If measurement shows the join is insufficient in practice, add a saving field
then, with the run that showed it. Adding it speculatively is the thing this factory spent M09
learning not to do.

---

## 4. R3 and the token surface — what exists, and one flattening already present

`core/gateway/src/call.rs` (read at main):

```
pub struct Usage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}
```

The gateway DOES see per-call token counts, so per-node counts are reachable by attribution from
calls to nodes. But:

**G-TOKEN-1, a flattening that exists today:** both fields are `Option<u64>`. **`None` is not zero.**
A provider that does not report usage and a call that genuinely consumed nothing are the same value
after any naive `unwrap_or(0)`, and they sum identically. Any per-node total built by summing these
must distinguish *unknown* from *zero*, or the context-efficiency numerator becomes a number nobody
can defend — and it will be a number that looks precise. This is the same disease as timing laundered
into a corruption verdict (#81): two causes, one value, the distinction destroyed at the boundary.

**MARK:** I did not trace how `Usage` reaches storage or whether any surface aggregates it. That
trace is the first implementation task and it should be done at the EMITTERS.

---

## 5. Dependency honesty — "#108" is too coarse, and this is the finding

The brief asks which half is buildable without #108 (*Context Compiler and runtime capsules do not
exist*, verified OPEN via `gh issue view 108`). The honest answer is that step 17's three mechanisms
block on **three different things**:

| step 17 mechanism | blocked on | buildable now? |
|---|---|---|
| capsule compilation cache hits | **#108** — there is no compiler and no capsule to cache | NO |
| provider prompt-cache hits | **the gateway surface** — `Usage` has no cache-hit field (read at main); independent of #108 | NO, but a different unblock |
| agent reuse | **the mechanism itself** — I found no agent-reuse machinery in `core/` outside tests (MARK: absence from one grep is weak evidence; treat as "not located", not "does not exist") | UNKNOWN |

**Buildable today, with no dependency:**
- the `ReusePlane` extension (R1) — a schema change, with the full ritual, but nothing blocks it;
- the renderer audit that makes R1 mean something at the operator surface (§2's MARK);
- the `Usage` `None`-vs-zero discipline (G-TOKEN-1) — fixable now and cheaper before there is a
  figure resting on it.

**Blocked, and the blueprint should say so plainly rather than schedule around it:** the
context-efficiency FIGURE itself. Its denominator is `estimated_tokens_full_context`, and §9.2 makes
the v1 estimator "a free compiler byproduct" of ranking. **No compiler, no ranking, no denominator.**
Step 16 cannot be delivered before #108 — not partially, not with a placeholder. A placeholder
denominator would produce a figure that reads exactly like the real one, which by this factory's own
M09 finding is worse than producing nothing.

---

## 6. What I recommend NOT doing

1. **Do not create a second reuse event.** One exists, is emitted, is folded as ledger, and carries a
   joinable digest. A parallel event would give the vocabulary two ways to say one thing.
2. **Do not re-derive the efficiency formula.** §9.2 fixes it, including the series-identity rules.
   A blueprint that restates it differently has already forked the metric.
3. **Do not put token counts on the event** while §9.2 puts them on the capsule manifest. Two homes
   for one number is a drift generator.
4. **Do not ship a figure with a placeholder denominator.** See §5.

## 7. What would make this work checkable when someone builds it

- the plane distinction asserted at the RENDERER, not the enum (the #81 G1 lesson);
- the method id + recipe version asserted on the figure's producer, with cross-series comparison
  refused by the surface (G-ARITH-1);
- "lower bound" surviving to the operator (G-ARITH-2);
- `None`-vs-zero distinguished in any token total (G-TOKEN-1);
- and for each new guard: **what ELSE would refuse if the thing under test were removed?** If a
  reuse-decomposition test passes because no reuse occurred at all, it measures nothing — the
  fixture must produce a real hit on the plane under test.
