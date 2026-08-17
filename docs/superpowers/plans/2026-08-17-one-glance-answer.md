# Milestone 07 — The One-Glance Answer: Implementation Plan

> **For agentic workers:** the two-agent pair loop, doorbell-native (protocol v4 with the
> heartbeat hooks). Task 0 reconciles against post-M06 main (`20c341d`+).

**Goal:** Obey the judge. M07's scope IS the blind judge's four findings from the M06
dogfood run (`docs/acceptance/m06-run-2026-08-17/`, gate_verdict `judge-usefulness`,
refused with severity critical/high×3) — the first milestone whose backlog was produced,
on-stream and with remediations, by the product judging the product. Nothing enters this
plan that the verdict did not ask for; the judge re-judges at the end.

**The four findings, verbatim scope:**
- **F1 (critical):** the one-glance surface reports green on a wedged execution. →
  `status` answers the sleep question DIRECTLY: top-level `attentionRequired: bool` +
  `attentionReasons` computed from the projection (untriaged interruptions, blocked or
  failed nodes, a wedged-quiescence shape). **Corrected by B's FIX-1 (the plan's original
  premise was false against the code): the monitor does NOT inherit `render` — it
  reimplements the triage rule (`serve/monitor.rs:68`, admitted in its own comment), and
  `execution/mod.rs:437` holds a third copy of the same predicate that
  `core/execution/src/recovery.rs:68` decides with.** So F1 is an EXTRACTION, not a field:
  one pure function, called by `render` AND by the monitor, so the two surfaces cannot
  disagree by construction. A one-truth test over two copies that happen to agree on day
  one proves nothing.
- **F2 (high):** `nodeStateCounts` omits states with zero nodes. → **all sixteen** states
  of `node_state_label` are emitted, zero-filled (not a subset — a partial list re-creates
  exactly the ambiguity F2 is about). This REVERSES a documented guarantee: the assert at
  `apps/cli/tests/api_http.rs:951-958` pins the omission ON PURPOSE, so it is inverted with
  its reason rewritten in place, never deleted — a deliberate change, cited as one.
- **F3 (high):** `retryable_failure`/`blocked` outcomes carry no cause. →
  `NodeOutcomeRecorded` gains an optional, additive `reason` (bounded wire-safe string —
  D-037 ritual, BOTH oneOf lists) and the executor threads the real cause (gateway error
  class, parse refusal, tool disposition) with failure-side sealables (the malformed reply
  already seals; the gateway-error path gains its own) so failures carry evidence with the
  same fidelity as successes.
- **F4 (high):** `wake_status` cannot say whether the alarm fired. → the fold keeps the
  last consumption per session (`{reason, sequence}` — additive projection field) and
  `wake_status` returns `live`, the armed cursor, the current head AND
  `lastConsumed: {reason, atSequence}` — "your alarm rang at #N (rung)" vs "still armed"
  vs "burned as stale at #N".

**Rules:** the M06 set verbatim (code wins; TDD with observed red; sabotage per guard;
fmt+clippy; per-file adds; one writer; SPEC+QUALITY cross-review; review runs the WHOLE
touched crate's suites and reads the FAILED lines — the M06 lesson). Registry:
GHCLI001–018 taken; M07 expects to need NO new code (surfaces change shape, refusals
exist). Kinds 29/30 taken; F3 modifies kind 4's payload ADDITIVELY (serde default;
existing streams replay unchanged — pinned).

**Closing rule (decision, binding):** Task 6 re-runs the blind judge — same story, same
charter, one real subscription call, announced — against the fixed surface. The milestone
does not close on our opinion that we obeyed; it closes on the judge withdrawing F1–F4
(a verdict whose findings no longer include them; NEW findings it raises become M08 seed,
not M07 scope — the loop is the product).

**Files (fixed by Task 0):** `core/execution/src/attention.rs` NEW (the seam) +
`core/execution/src/lib.rs` (export); `apps/cli/src/commands/execution/mod.rs` (render
consumes the seam, zero-fill, local copy deleted); `apps/cli/src/commands/serve/monitor.rs`
(consumes the same seam, private copy deleted, header in words);
`core/protocols/src/event.rs` + BOTH schema copies + BOTH catalogs + `schemas/CHANGELOG.md`
(F3 additive reason); `core/runtime/src/executor.rs` (F3 causes + the gateway-error arm's
missing sealable); `core/events/src/projection.rs` (F4 last consumption);
`apps/cli/src/commands/execution/wake.rs` + `serve/routes.rs` (F4 surface);
`docs/acceptance/demos/m06-fixture-journey/demo.json` (re-recorded in Task 3); the parity,
choreography and monitor tests that pin these shapes today.

### Task 0: reconciliation — DONE (this document, both of B's FIXes applied)
Findings recorded on #60. The two that change the plan are folded in above (F1 is an
extraction; the re-record trigger belongs to Task 3). Standing facts for the writers:
`PARITY_EXCEPTIONS` (`api_http.rs:1721`) and `MCP_PARITY_EXCEPTIONS` (`mcp_stdio.rs:753`)
are empty BY DESIGN and asserted empty — `attentionRequired` must be surface-independent
and must NEVER earn an entry; the prose field list at `api_http.rs:1675-1688` goes stale on
the same commit that adds the field; `Blocked` is not an executor outcome but a transition
consequence (`core/execution/src/transition.rs:91-105`), so F3's "blocked carries a cause"
means the `RetryableFailure` that exhausted its attempts carried one.

**THE SEAM CONTRACT (declared before any code, per B's condition on split (i)).** New file
`core/execution/src/attention.rs`, exported from `core/execution/src/lib.rs`. Home chosen
because `core/execution` already depends on `core/events` (so it may take the projection),
already decides this predicate inside `resume_preconditions`, and keeps `core/events` a
pure ledger. Exact contract — B implements it in Task 1, A consumes it unchanged in Task 4:

```rust
/// Whether the operator may go back to sleep, decided ONCE over the projection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attention {
    pub required: bool,
    pub reasons: Vec<AttentionReason>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AttentionReason {
    /// `Blocked` + `last_outcome == Interrupted`: the 04f triage rule, now with one home.
    UntriagedInterruption { node: String },
    /// `Blocked` for any other cause (retries exhausted, a gate refusal).
    BlockedNode { node: String },
    /// The terminal `Failed` state.
    FailedNode { node: String },
    /// Status says `running` while NOTHING can advance — no node `Running`, `Queued` or
    /// `Ready`. This is the exact shape the judge saw reported green.
    WedgedQuiescence,
}

/// Deterministic: reasons in variant order, then node id (the projection's maps are
/// already ordered). `required` is `!reasons.is_empty()` — never an independent field.
pub fn attention(projection: &ExecutionProjection) -> Attention;
```

Consumption rules, binding on both agents: `render` emits `attentionRequired` and
`attentionReasons` from ONE call to `attention`, and rebuilds the existing
`untriagedInterruptions` list by filtering that SAME value (the local copy at
`execution/mod.rs:437` dies — `untriagedInterruptions` keeps its current shape, so its
tests stay green untouched). The monitor's private `untriaged` (`serve/monitor.rs:68`)
dies the same way; the header renders words from the same value. No surface recomputes.
### Task 1 (B): the extraction — F1 attention seam + F2 zero-filled buckets
- [ ] `core/execution/src/attention.rs` per the contract above, unit-tested per variant
  (including a wedged-quiescence fixture: status running, no advanceable node); `render`
  consumes it; all sixteen buckets zero-filled; the `api_http.rs:951-958` assert inverted
  with its reason rewritten. Sabotage: make `attention` return `required: false` while
  reasons are non-empty — the derived-not-declared test fails. Second sabotage: give the
  monitor back a private copy that disagrees — the one-truth test fails BECAUSE the monitor
  is required by construction to call the shared function.
### Task 2 (A): F3 causes — the additive reason + failure-side evidence
- [ ] D-037 additive on `NodeOutcomeRecorded` (BOTH oneOf lists; old streams replay
  byte-identical, pinned); executor threads gateway-error class / judge parse refusal /
  tool disposition into `reason`; the gateway-error path seals its error text like the
  malformed-reply path already does. Sabotage: empty the reason on a failure; the
  cause-required-on-failure test fails.
### Task 3 (B): F4 the alarm's own answer — AND the re-record (moved here by B's FIX-2)
- [ ] Fold: `wake_last_consumed[session] = {reason, sequence}` (additive, serde default);
  `wake_status` returns live/cursor/head/lastConsumed; the choreography test extends: the
  woken sleeper READS that its alarm rang at #N. Sabotage: report lastConsumed from the
  lease map instead of the consumption record; the burned-alarm test fails.
- [ ] **The frozen digest lands HERE, not in Task 0/1.** `expectedProjectionDigest` in
  `docs/acceptance/demos/m06-fixture-journey/demo.json` is sha256 over `ExecutionProjection`
  (`tools/acceptance-map/src/lib.rs:165-169`), NOT over `render` — so F1/F2 cannot break it
  and this task's new projection field certainly does (no field uses
  `skip_serializing_if`). **Decision, both agents agreed: re-record** via
  `tools/acceptance-map/src/bin/record_demonstration.rs`, citing the old and the new digest
  in the commit message. Rejected alternative: `skip_serializing_if` on the new field —
  it would preserve old digests by making one field lie about its own presence.
### Task 4 (A): the monitor says it in words — consuming the seam, never recomputing
- [ ] The header renders the F1 verdict ("needs you: …" / "can sleep") from `attention`;
  the private `untriaged` copy is gone; staleness reasons join the rendered words where the
  graph is present. Snapshot byte-equality and zero-JS guards must stay green untouched.
### Task 5 (B): acceptance-map bindings updated for any renamed prover (the clause
  fingerprints at `docs/acceptance/m05-clauses.toml:42-51` must still match the test
  bodies); the generated `M05_ACCEPTANCE_MAP.md` diff-checked; full CLI regression.
### Task 6 (A): the judge re-judges (closing rule) + docs + gate + PR
- [ ] One announced real run: same story, fixed surface; F1–F4 withdrawn or the milestone
  does not close. New findings → recorded as M08 seed. M07 record + CHANGELOG + honest
  limits; [GATE] 22 stages GREEN; PR.

**Split — SETTLED (B's option (i), accepted):** A: 0, 2, 4, 6 + PR · B: plan review,
1, 3, 5 + final PR review. B's condition is met above: the seam contract is declared in
Task 0 BEFORE any code, so B creates it in Task 1 and A consumes it in Task 4 without two
agents editing the same seam in the shared worktree.
