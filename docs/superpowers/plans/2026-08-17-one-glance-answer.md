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
  `status` answers the sleep question DIRECTLY: a top-level `attentionRequired: bool` +
  `attentionReasons: [str]` computed from the projection (untriaged interruptions, blocked
  or failed nodes, a wedged-quiescence shape), rendered by the ONE shared `render` so CLI,
  API, MCP and monitor all inherit it — one truth, no surface drift. The monitor's header
  says it in words ("can sleep" / "needs you: <reasons>").
- **F2 (high):** `nodeStateCounts` omits states with zero nodes. → every lifecycle state
  becomes a bucket, zero-filled, so absence is visible and dashboards cannot misread a
  missing key as "no such problem".
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

**Files (indicative — Task 0 finalizes):** `apps/cli/src/commands/execution/mod.rs`
(render: attentionRequired + zero-fill), `core/protocols/src/event.rs` + schemas (F3
additive reason), `core/events` fold (F4 lastConsumed), `core/runtime/src/executor.rs`
(F3 causes + failure sealables), `apps/cli/src/commands/execution/wake.rs` +
`serve/wake.rs` (F4 surface), `serve/monitor.rs` (F1 header), the parity/choreography
tests that pin shapes today.

### Task 0: reconciliation
- [ ] Post-M06 main: registry tails (GHCLI 018, kinds 30), gate list (12 suites/22
  stages), the exact shapes F1/F2 change and every test pinning them (parity lists,
  api_http asserts, demo projection digests — a zero-fill changes `render` output, so the
  M06 demonstration's frozen projection digest MAY need re-recording: decide and declare
  the re-record protocol BEFORE touching render). Commit doc-only.
### Task 1: F2 zero-filled buckets + F1 attentionRequired in `render`
- [ ] Failing tests first on the CLI/API/MCP parity surfaces; the wedged fixture story
  answers `attentionRequired: true` with named reasons; a clean completed story answers
  `false`. Sabotage: compute attention from a second copy of the rule; the one-truth test
  (monitor header vs render field) fails.
### Task 2: F3 causes — the additive reason + failure-side evidence
- [ ] D-037 additive on `NodeOutcomeRecorded` (BOTH oneOf lists; old streams replay
  byte-identical, pinned); executor threads gateway-error class / judge parse refusal /
  tool disposition into `reason`; the gateway-error path seals its error text like the
  malformed-reply path already does. Sabotage: empty the reason on a failure; the
  cause-required-on-failure test fails.
### Task 3: F4 the alarm's own answer
- [ ] Fold: `wake_last_consumed[session] = {reason, sequence}` (additive, serde default);
  `wake_status` returns live/cursor/head/lastConsumed; the choreography test extends: the
  woken sleeper READS that its alarm rang at #N. Sabotage: report lastConsumed from the
  lease map instead of the consumption record; the burned-alarm test fails.
### Task 4: the monitor says it in words
- [ ] The header renders the F1 verdict ("needs you: …" / "can sleep"); staleness reasons
  join attentionReasons where the graph is present. Snapshot byte-equality and zero-JS
  guards unchanged (their tests already exist — they must stay green untouched).
### Task 5: demonstrations re-recorded IF Task 0 declared it; acceptance-map bindings
  updated for any renamed prover; full CLI regression.
### Task 6: the judge re-judges (closing rule) + docs + gate + PR
- [ ] One announced real run: same story, fixed surface; F1–F4 withdrawn or the milestone
  does not close. New findings → recorded as M08 seed. M07 record + CHANGELOG + honest
  limits; [GATE] 22 stages GREEN; PR.

**Suggested split:** A: 0, 2, 4, 6 + PR · B: 1, 3, 5 + final PR review — or inverted at
kickoff (B reviews this plan first; the plan author does not review their own plan).
