# Milestone 06 — Quality Gates: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: the two-agent pair loop (05e/05f/05g protocol
> verbatim, PLUS the factory doorbell: waiting between handoffs is `wake_arm` +
> `graphhelm wake-wait` against the standing pair serve, never timed polling). Task 0
> reconciles against post-05g main.

**Goal:** GraphHelm's correctness story (TDD, sabotage, the acceptance map) gains a
USEFULNESS story: gates that prove a delivered feature works as a journey, a screen is
right, a UX is efficient — and that CANNOT be gamed, because every gate must first prove it
knows how to reject. The `Gate` and `Evaluator` node types, refusing execution since 05d,
finally RUN — and the dev factory is their first production user.

**The five load-bearing decisions (fixed by the 2026-08-16 divergent pass — binding):**
1. **The thymus rule**: a gate earns the right to gate by REJECTING a bred suite of
   pathogen deliverables (useless-but-green); a gate that passes a pathogen is itself fake.
2. **The blind judge**: usefulness is scored by an Evaluator that sees ONLY the user story
   and the running system — never code, tests, or rubric. Verdicts are always
   **refusal-with-findings** (severity, cited evidence, remediation), never bare pass/fail.
3. **The journey is a replayable artifact**: acceptance-map clauses gain a third binding —
   clause → tests → **demonstration** — replayed deterministically, seeds sampled at review.
4. **Beauty is geometry, never prose**: layout grammar + content manifest + view→clause
   peptides over computed projections, with builder-authored text stripped before any judge.
   Screenshot diffing is refused as a foundation.
5. **Dogfood with separation of powers**: gate definitions are event-sourced and FREEZE
   before implementation; a gate-definition change in the same PR as gated code is a hard
   fail. **Refused metric:** any single quality scalar.

**Rules of engagement (the 05g set, plus):**
1. The code wins over the plan; discrepancies reported in the handoff.
2. TDD with observed red; sabotage per guard with the failing test cited; restore from `cp`.
3. fmt + workspace clippy `-D warnings` before every commit; the house trailer; per-file `git add`.
4. One writer at a time; SPEC-then-QUALITY cross-review; nothing starts without [APPROVED].
5. Failure-code registry: GHCLI001–017 taken; this plan reserves **GHCLI018_GATE_INVALID**.
6. New CLI test binary `gate_http` joins the gate list (twelfth suite).
7. Evidence-with-checksum artifacts hash the bytes git stores; `.gitattributes -text` first;
   the artifact verifier gains the tracked-vs-named check (the 05f gitignore lesson, paid here).

**Files (map, indicative — Task 0 finalizes):**
- Create: `core/quality/` (verdict vocabulary, layout grammar, content manifest — pure),
  `tools/pathogens/` (specimens + the certification harness),
  `apps/cli/tests/gate_http.rs`
- Modify: `core/protocols` + schemas (D-037: `GateVerdict`, `GateCertified`), `core/events`
  fold, `core/runtime/src/classify.rs` (Gate/Evaluator stop refusing), `serve` routes,
  `tools/acceptance-map` (demonstration binding + tracked-vs-named), `ci/gate.ps1`
- Hotspots: `classify.rs`, `mcp/tools.rs`, `acceptance-map/src/lib.rs`, `gate.ps1`

### Task 0: reconciliation
- [ ] Read post-05g main: GHCLI018 free; gate list (eleven suites); kind registry tail;
  `classify.rs`'s refusal arms for Gate/Evaluator; what the monitor renderer exposes that
  the layout grammar can consume; the acceptance map's current clause schema. Rewrite stale
  references; re-check #35 triggers. **Commit** `docs(plans): reconcile M06 plan (Task 0)`.

### Task 1: the verdict vocabulary — refusal-with-findings (D-037)
- [ ] **Failing tests:** `GateVerdict` round-trips with findings only (severity, claim,
  evidence refs, remediation) — a bare pass/fail without findings is schema-invalid by
  construction; `GateCertified` (the thymus receipt) requires the pathogen-suite digest it
  certified against; fold arms explicit; replay-stable.
- [ ] **Sabotage:** allow an empty findings list on a failing verdict; the
  no-bare-verdicts test fails. **Commit** `feat(events): gate verdict and certification kinds`.

### Task 2: the pathogen suite and the thymus harness
- [ ] **Failing tests:** ten specimens exist as fixtures, each a deliverable that is GREEN
  by correctness measures and USELESS by construction (dead feature, unreachable UI,
  tautological journey, blank screen, orphan view, gutted assertion, happy-path-only,
  spec-claim-without-artifact, minimal-diff-no-behavior, label-swapped UI); the
  certification harness runs a candidate gate against ALL specimens and refuses
  certification on ANY pass; a certified gate's `GateCertified` event carries the suite
  digest, so growing the suite VOIDS old certifications (recertify or stand down).
- [ ] **Sabotage:** weaken one specimen until a trivial gate passes it; the
  harness-self-test (a known-fake gate must fail certification) catches it.
  **Commit** `feat(quality): the pathogen suite — a gate must earn the right to gate`.

### Task 3: the deterministic evaluators — content manifest + layout grammar
- [ ] **Failing tests** (over the REAL monitor page as the first subject): the
  spec-derived content manifest (`these N elements MUST be visible and reachable`) fails on
  a blanked section; the layout grammar (alignment variance, density budget, contrast
  pairs, orphan-state detection) fails on a deliberately broken render; every
  builder-authored string is STRIPPED from what the evaluator scores (sentinel: a page
  stuffed with "excellent beautiful perfect" scores identically to one without).
- [ ] **Sabotage:** let the text through; the sentinel-equality test fails.
  **Commit** `feat(quality): geometry evaluators — manifest and layout grammar`.
- [ ] These two are the FIRST gates through the thymus: certified in this task's tests.

### Task 4: Gate/Evaluator nodes execute
- [ ] **Failing tests:** `classify.rs` routes Gate → deterministic-evaluator work and
  Evaluator → judge work (refusal arms die for these two, EXPLICITLY, everything else
  still refuses); a Gate node run appends `GateVerdict` with findings; an UNCERTIFIED gate
  refuses to run (the thymus receipt is a precondition, checked against the current suite
  digest); the driver treats a failing verdict as the node outcome the graph routes on.
- [ ] **Sabotage:** run an uncertified gate; the precondition test fails.
  **Commit** `feat(runtime): gate nodes run — certified or not at all`.

### Task 5: the blind judge
- [ ] **Failing tests** (fake model port first, one real run in Task 7): the judge
  executor assembles ONLY the user story + the live system's MCP surface — a source scan
  pins that no code path feeds it repository contents, tests, or rubric text; its verdict
  is refusal-with-findings carrying steps-over-par and stall points; its long waits use
  the 05g doorbell (wake_arm between probe steps), pinned by the zero-polling counter.
- [ ] **Sabotage:** leak the rubric into the judge's prompt; the source scan fails.
  **Commit** `feat(quality): the blind judge — information asymmetry as code`.

### Task 6: demonstrations — the acceptance map's third binding
- [ ] **Failing tests:** `m05-clauses.toml`-style clauses accept
  `[[clause.demonstration]]` (a recorded journey artifact: transcript + expected
  projection digest); the grounding test replays each demonstration against the current
  build and fails on divergence; traversal seeds sampled at run time from the gate's own
  entropy, so a build cannot precompute the exact path; the artifact verifier gains
  tracked-vs-named (a demonstration named but gitignored is a grounding failure — the 05f
  lesson paid).
- [ ] **Sabotage:** gitignore a named demonstration; the tracked-vs-named check fails.
  **Commit** `feat(acceptance): demonstrations — the third binding`.

### Task 7: dogfood — the factory gated by its own product
- [ ] The factory pipeline as a GraphHelm graph: a real (small) change PR'd through a graph
  whose Gate nodes run the geometry evaluators and whose Evaluator node runs the blind
  judge (ONE real model run, announced, artifacts + checksums committed, never re-run —
  the 05f acceptance-run pattern). The gate-freeze rule ships as a check: gate definitions
  event-sourced; a PR touching both a gate definition and code that gate gates = hard fail
  (test: such a synthetic PR diff is refused).
- [ ] **Commit** `feat(quality): the factory eats its own gates`.

### Task 8: the gate stage and regression
- [ ] `'gate_http'` twelfth suite, red-proven; full CLI regression `--locked`; workspace
  fmt/clippy. **Commit** `chore(ci): gate_http stage`.

### Task 9: documentation, honest limits, the full gate, the PR
- [ ] The M06 section from the code as built; honest limits (the judge's model cost and
  variance; geometry ≠ taste — the grammar catches broken, not sublime; pathogens are ten,
  not infinity, and the suite grows per escape; adoption/pain telemetry still deferred to
  the first-external-operator trigger); CHANGELOG; spec/status lines; [GATE] announced,
  full gate GREEN; PR (Closes the milestone issue).

**Refused scope (diff-visible):** any single quality scalar; LLM judges reading
builder-authored text; screenshot diffing as foundation; adoption metrics before real
users; a gate whose definition can change beside the code it gates.

**Suggested split:** B (plan author): 0, 2, 4, 6, 8 + final PR review · A: 1, 3, 5, 7, 9 +
opens the PR — or inverted at kickoff.
