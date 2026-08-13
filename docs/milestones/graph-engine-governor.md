# Graph Engine and Governor

Status: 04a and 04b implemented on this branch. Toolchain: Rust 1.97.1, edition 2024. Schema baseline: `1.0.0`, envelope contract corrected in place per D-037 to carry the four execution event kinds.

Milestone 03 made persistence production-safe but nothing executed a published graph. This milestone starts making a published graph run. Only two of its six plans, 04a and 04b, are done: the pure execution contracts and the durable execution projection. Nothing here schedules a node, dispatches a signal, or runs an operator command yet.

## What 04a shipped: pure execution contracts

`core/execution` is a new crate with no I/O, no clock, no randomness, and no adapter dependency. It depends only on `graphhelm-protocols`, `serde`, and `serde_json`; `proptest` is a dev-dependency only. `core/execution/tests/source_invariants.rs` enforces this at the source level: it fails if the manifest names `tokio`, `sqlx`, `chrono`, `getrandom`, `rand`, or any GraphHelm adapter crate, and it fails if any source file's code (comments stripped first, so prose cannot influence the check) references `SystemTime`, `Instant`, `now()`, a random source, or filesystem/network/environment access.

`NodeState::Ghost` was added to the shared vocabulary in `graphhelm_protocols::simulation`, immediately after `Draft`, rather than defining a second execution-only enum. A ghost's only legal exit is approval to `Ready`; every other outcome from `Ghost` is `ExecutionError::IllegalTransition`, including `Started`. This is enforced both by unit test and by a property test that tries every outcome, attempt count, and identical-outcome count against a ghost and asserts it never reaches `Queued` or `Running`.

### Bounds are counters, never durations

`core/execution/src/bounds.rs` fixes five limits, all counts of events or attempts:

| Constant | Value |
|---|---:|
| `MAX_NODE_ATTEMPTS` | 8 |
| `MAX_IDENTICAL_OUTCOMES` | 3 |
| `MAX_ACCEPTED_MUTATIONS` | 64 |
| `MAX_READY_SET` | 1024 |
| `MAX_SIGNALS_PER_EXECUTION` | 10,000 |

A wall-clock bound would make the same event history replay to a different decision on a slower machine, which breaks the replay guarantee the rest of this milestone rests on. Only two of the five are consulted by anything shipped so far: `apply_transition` reads `MAX_NODE_ATTEMPTS` and `MAX_IDENTICAL_OUTCOMES` to decide whether a retryable failure blocks the node for an owner decision instead of retrying it. `MAX_ACCEPTED_MUTATIONS`, `MAX_READY_SET`, and `MAX_SIGNALS_PER_EXECUTION` exist for 04c and 04d, which do not exist yet.

### The closed Graph Signal typed subset

`core/execution/src/signal.rs` models the stable typed subset of `schemas/graph-signal.schema.json`. `schemas/graph-signal.schema.json` closes `source.type` but leaves the signal's own `type` an open string for forward compatibility, so `TypedSignal::parse` validates against the schema's exact constraints (non-empty `type`, `description`, and `evidence`; a closed `source.type`) and then classifies `type` into one of six recognized `SignalKind` variants or `Unrecognized`. An unrecognized kind is still constructed and still carries its raw string, but `can_propose_mutation()` returns `false` for it and `true` for every recognized kind. Nothing in this crate turns a signal into anything — the Governor is 04d's job, not this one's — so `can_propose_mutation()` is the only place a signal's authority is decided at all, and today it is decided only in the sense of that boolean.

### `apply_transition`: total, deterministic, ghost-safe

`apply_transition` is a pure function from a `TransitionRequest` (current state, outcome, attempts so far, consecutive identical outcomes so far) to `Result<NodeState, ExecutionError>`. Three properties are proven by `core/execution/tests/transition_properties.rs` across the full state and outcome vocabularies, at a range of attempt and identical-outcome counts:

- **Totality**: every combination of state, outcome, attempts, and identical-outcome count returns a value or a typed error, never panics.
- **Determinism**: the same request always yields the same answer.
- **Ghost-safety**: from `Ghost`, no outcome at any counter value reaches `Queued` or `Running`.

Retry exhaustion (`MAX_NODE_ATTEMPTS` or `MAX_IDENTICAL_OUTCOMES` reached on a `RetryableFailure`) transitions to `Blocked` rather than `Failed` or looping forever: no work is discarded without a record, and an owner decision is required to proceed. A terminal state (`Succeeded`, `Failed`, `Waived`, `Skipped`, `Cancelled`) accepts no further transition except `Succeeded → Invalidated` on an `Invalidated` outcome, which models an upstream change rather than a new outcome of the node itself. Cancellation is accepted from any non-terminal state.

### The `NodeExecutor` seam

```rust
pub trait NodeExecutor {
    fn execute(&self, node_id: &str, attempt: u32) -> Result<NodeOutcome, ExecutionError>;
}
```

This trait exists and has no implementor in this milestone. Milestone 04c is expected to give `graphhelm-simulation` an effect-free implementation; Milestone 05 supplies one that calls real models and tools behind the same contract. Nothing here calls it.

## What 04b shipped: the durable execution projection

### Wire vocabulary and event kinds

`NodeOutcome` and `ExecutionMode` live in `graphhelm_protocols::simulation` as closed vocabularies (`#[serde(rename_all = "snake_case")]`, no `#[serde(other)]`, no `Default`). `core/execution` re-exports `NodeOutcome` rather than defining a second copy. `ExecutionMode` is `Autopilot`, `Supervised`, or `Manual`, per D-022; every field that carries it is required, so an absent mode fails deserialization instead of silently granting `Autopilot`.

The closed set of replay-safe production events grew from 16 to 20 with four execution kinds in `EventKind`:

- `ExecutionStarted { execution_id, graph_version, graph_hash, mode }`
- `ExecutionModeChanged { execution_id, previous_mode, mode }`
- `NodeOutcomeRecorded { execution_id, node_id, outcome, next_state }`
- `ExecutionCompleted { execution_id, status }`

`next_state` on `NodeOutcomeRecorded` is the decision `apply_transition` produced, recorded on the wire rather than recomputed inside `core/events` — `core/events` must not depend on `core/execution` — the design points the dependency the other way, and 04c wires `core/execution` to `core/events`, at which point importing back would cycle. Today `core/execution` depends only on `graphhelm-protocols`, so the cycle is prospective rather than present; the constraint is a design boundary being held in advance, not a compiler error being worked around.

`event-envelope.schema.json` was corrected in place under D-037 to add `executionMode` and `nodeOutcome` vocabularies plus the four execution payload definitions, mirrored byte-for-byte into the frozen `schemas/releases/1.0.0/` copy, with both `schemas/catalog.json` and `schemas/releases/1.0.0/catalog.json` digests recomputed. There is still one baseline, `1.0.0`, corrected in place; there is no `1.1.0` and no legacy branch.

### `ExecutionProjection` extended

`core/events/src/projection.rs` added five fields to `ExecutionProjection`:

- `execution_id: Option<String>` — set once, from `ExecutionStarted`; a second `ExecutionStarted` in the same stream is rejected as corrupt.
- `mode: Option<ExecutionMode>` — `None` until an execution starts; `ExecutionModeChanged` is rejected unless its `previous_mode` matches the projection's current mode.
- `node_attempts: BTreeMap<String, u32>` — attempts per node, `#[serde(default)]`.
- `last_outcome: BTreeMap<String, NodeOutcome>` — most recent outcome per node, `#[serde(default)]`.
- `identical_outcomes: BTreeMap<String, u32>` — consecutive identical outcomes per node, `#[serde(default)]`.

All three counters are **derived by folding history, never carried in a payload**: nothing in `NodeOutcomeRecorded` says "attempt 3", and the projection's fold is the only source of that number. An attempt is a dispatch: `node_attempts` increments only when `payload.outcome == NodeOutcome::Started`, not on every outcome report for that node. A node retried twice and then succeeded shows three attempts, not five. `identical_outcomes` counts a run of consecutive identical outcomes and resets to `1` the moment a different outcome intervenes, even if that different outcome is followed by a return to the original one.

`core/events/tests/execution_projection.rs` proves this by replaying real events through the real repository and the real `apply_transition` (so the fixture's `next_state` cannot drift from the production state machine). `attempts_are_derived_by_folding_outcomes` sends `Started, RetryableFailure, Started, RetryableFailure, Started` and asserts three attempts, not five. `identical_outcomes_count_consecutively_and_reset` sends three consecutive `RetryableFailure` outcomes and asserts a run of three, then sends two `RetryableFailure`, one `NeedsInput`, and one more `RetryableFailure`, and asserts the run resets to one. `replay_is_identical_across_runs` proves the same event slice replayed twice produces byte-identical serialized state.

### Generation compatibility, one direction only

The three new maps carry `#[serde(default)]`, and `execution_id` and `mode` are `Option`, which serde already treats as absent-tolerant. So a projection generation written before 04b — which never had `execution_id`, `mode`, or the three counter maps — loads with them empty rather than failing. `a_generation_written_before_these_fields_still_loads` proves this against a realistic pre-04b fixture that has every field that already existed and only omits the new ones.

The other direction is deliberately not defaulted: the collection fields the projection already had before 04b (`proposedDrafts`, `waivers`, `nodeStates`, `evidenceAvailability`, `legalHolds` and the rest) carry no `#[serde(default)]`, so a generation missing one of those is treated as corrupt, not old, and fails to deserialize. Defaulting a field every stored generation already has would let a truncated state load silently as empty. `streamId` is not among them — it is an `Option` and absent means `None`, as it always has.

`a_generation_missing_a_pre_existing_field_fails` takes the realistic fixture and removes each required key in turn, asserting each removal fails. A bare `{}` would have proven only that the *first* such field is required, leaving the rest free to acquire a default without any test noticing.

### Rebuild proven equivalent to replay

`a_discarded_generation_rebuilds_to_identical_state` splits one event history into two pages, applies them to a `ProjectionGeneration` across two calls to `apply_page` (exercising the resumable-rebuild path `ProjectionRebuilder::rebuild` also uses), and asserts the resulting serialized state is byte-identical to a single direct `replay` over the whole history. This is what decision 5.6 (execution state as a disposable, rebuildable projection with no mutable execution table) rests on.

### `GHPROJ001_WATERMARK_MISMATCH` now has a test

Milestone 03 shipped `GHPROJ001_WATERMARK_MISMATCH` with no test ever observed to produce it. `adapters/postgres-event-store/tests/projection.rs` now has one: `a_generation_with_mismatched_identity_is_rejected_as_watermark_mismatch_even_with_execution_state`.

The guard itself lives in `ProjectionRebuilder::rebuild` (`core/events/src/projection.rs`): when a stored generation's identity (scope, stream, projection name, version, or generation number) does not match the one requested, `rebuild` returns `EventRepositoryError::WatermarkMismatch` rather than resuming from a generation that does not describe the state being asked for.

**The production PostgreSQL adapter cannot structurally reach this guard.** `load_generation`'s SQL already filters by the exact requested identity, and `decode_generation` rejects a stored row whose embedded watermark disagrees with its own columns, with `Integrity` first. So through the real adapter alone, a mismatched generation can never reach `rebuild` in the first place. The new test exercises the guard through `MismatchedProjectionRepository`, a test-double `ProjectionRepository` that always answers `load_generation` with a fixed, pre-built generation regardless of what was requested, while delegating `save_generation`, `load_active`, and `swap_active` to the real adapter so the rest of the path is genuine. This proves the guard exists and works as defence in depth against a buggy or compromised storage-layer implementation — it is not a path the production adapter exercises today. The test also confirms that a mismatched generation carrying real execution state (mode, attempt counts, outcome runs) is rejected exactly the same as an old-format one, so adding fields to `ExecutionProjection` did not loosen the identity check guarding the swap.

## Explicitly out of scope

Nothing below exists yet. It is scoped to milestones 04c through 04f:

- **04c — Scheduler and effect-free executor.** Ready-set computation, bounded concurrency, retry classification, no-progress detection, and the first `NodeExecutor` implementation. Nothing computes a ready set today; `MAX_READY_SET` is defined but unread.
- **04d — In-flight governance.** Signal intake, ghost node lifecycle and approval, Governor mutation publication, owner override with waiver. `TypedSignal` exists and classifies signals, but nothing consumes one; `MAX_ACCEPTED_MUTATIONS` and `MAX_SIGNALS_PER_EXECUTION` are defined but unread.
- **04e — Pause, resume, cancel, recovery.** Checkpoint content, resume preconditions, crash recovery of an interrupted execution.
- **04f — Operator CLI, gate, documentation.** JSON-only `execution start|status|signal|approve|pause|resume|cancel` commands and the milestone-closing gate integration and final review.

Two consequences of this follow directly from `apply_transition`'s table, worth stating plainly: `Paused` and `Linting` are accepted as transition *sources* (`(S::Draft | S::Linting, O::Approved) => Ok(S::Ready)`, and `(S::WaitingInput | S::WaitingCapacity | S::Paused, O::Started) => Ok(S::Queued)`), but nothing in 04a or 04b ever produces a node in either state — pause and lint completion arrive with 04c and 04e. `Blocked` similarly has no resume path in the current transition table: nothing maps `(S::Blocked, _)` to any state except `(_, O::Waived) => Ok(S::Waived)` and `(_, O::Skipped) => Ok(S::Skipped)`, and `(_, O::Cancelled)`; there is no `(S::Blocked, O::Started)` or equivalent that returns a node to `Queued`. A resume path for `Blocked` is 04e's job.

## Acceptance evidence

`core/execution/tests/source_invariants.rs` and `core/execution/tests/transition_properties.rs` cover purity and the state machine. `core/events/tests/execution_projection.rs` and `adapters/postgres-event-store/tests/projection.rs` cover the projection, its generation compatibility, and the watermark guard; the PostgreSQL suite is `#[ignore]`d in ordinary runs and requires `GRAPHHELM_TEST_ADMIN_URL`, matching Milestone 03's convention.
