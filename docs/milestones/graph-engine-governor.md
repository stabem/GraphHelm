# Graph Engine and Governor

Status: complete. All six plans, 04a through 04f, are implemented; the operator commands drive a published graph end to end. Toolchain: Rust 1.97.1, edition 2024. Schema baseline: `1.0.0`, envelope contract corrected in place per D-037 to carry the four execution event kinds.

Milestone 03 made persistence production-safe but nothing executed a published graph. This milestone makes a published graph run. All six plans are done: the pure execution contracts, the durable execution projection, the scheduler, in-flight governance, the lifecycle with crash recovery, and the operator CLI whose driver wires them to the durable store.

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

A wall-clock bound would make the same event history replay to a different decision on a slower machine, which breaks the replay guarantee the rest of this milestone rests on. Current code consults all five bounds. `apply_transition` uses `MAX_NODE_ATTEMPTS` and `MAX_IDENTICAL_OUTCOMES`; scheduling uses `MAX_READY_SET`; in-flight governance uses `MAX_ACCEPTED_MUTATIONS` and `MAX_SIGNALS_PER_EXECUTION`.

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

## What 04c shipped: the scheduler and the effect-free executor

04c decides *what runs next* and *when to stop trying*. It dispatches nothing and appends no events; both additions are total functions over values, so a replayed execution schedules identically to the original.

### Ready-set computation

`graphhelm_execution::ready_set(spec, states)` answers which nodes may be dispatched now. Three rules define it, each pinned by a test in `core/execution/src/ready.rs` — including the edge-type rule, which a non-`Control` edge now covers:

- **Dependencies are fail-closed.** *Every* incoming edge gates a node, whatever its `EdgeType`. A node never runs before something it is connected downstream of. Refining this per edge type needs the condition evaluation 04d brings; guessing now would let a node run early.
- **A predecessor releases its dependent only when `Succeeded`, `Waived` or `Skipped`.** Waived and skipped count because an owner exercising D-019 sovereignty must not stall the run. `Failed`, `Cancelled` and `Blocked` deliberately do not release anything.
- **Only `Ready` nodes are dispatchable.** An untouched node behaves as `Draft` and reaches `Ready` through approval; dispatching a draft would propose work `apply_transition` rejects, and a test now pins that every dispatchable state accepts a `Started`. Resuming `Paused`, `WaitingInput` or `WaitingCapacity` is the lifecycle's business, not the scheduler's — the resume arm itself predates 04e; what 04e added is a producer for `Paused`, so the arm finally has something to resume.

`NodeState::Ghost` is excluded by construction from both rules: a ghost is never dispatchable and never releases a dependent. That is what makes decision 5.2's "consumes no tokens" structural rather than a convention someone has to remember. `core/execution/tests/scheduling_properties.rs` samples it across randomised assignments of the other nodes' states (proptest, not exhaustion), and was verified able to fail by adding `Ghost` to the dispatchable set.

Exceeding `MAX_READY_SET` returns `ScheduleError::ReadySetTooLarge` rather than a truncated set, per decision 5.7. A truncated ready set is indistinguishable from a smaller graph and would lose work silently.

### No-progress classification

`classify_progress(projection, node, outcome)` returns `Continue`, `AttemptsExhausted` or `NoProgress`, reading the counters 04b derives against the bounds 04a fixed. Every threshold is a count, never a duration, so the same history reaches the same verdict on a slower machine.

It reads the run length through `ExecutionProjection::identical_outcomes_for` rather than the raw map. The two are different quantities — the map includes the last outcome recorded — and reading the map directly would block a node on its *first* failure whenever some other outcome had already run to the bound. That defect was observed failing before the accessor was used.

**Five of `OBSERVABILITY_AND_RECOVERY.md` §15's seven no-progress conditions are not detected.** Only *retries with no change* and *semantically identical outputs* are decidable from what the projection records. Recurring remediation loops, alternating graph mutations, agent delegation chains, repeated tool failure, and budget consumed without evidence gain need signal intake (04d) or real tool calls (Milestone 05). None of them is approximated.

### The effect-free executor

`graphhelm_simulation::FixtureExecutor` is the first and only milestone-04 implementation of the `NodeExecutor` seam. It consults a fixture table and nothing else — no model, no tool, no sandbox, no network. `Success` maps to `Succeeded`, `Failure` to `RetryableFailure`, and both an explicit `Unknown` and an absent fixture to `NeedsInput`: nobody said what that node does, and waiting is honest where inventing a success is not.

Its answer does not depend on the attempt number, which is tested. An executor whose answer changed with the attempt would make a replay diverge from the run it replays.

**`simulate()` does not call it, by design.** `simulate()` in `core/simulation/src/engine.rs` drives its own transitions and reads the fixture table itself. 04f reclassified the divergence as intentional rather than outstanding: `simulate()` is the authoring-time explorer, so an absent fixture defaults to `Success` — authoring asks what the graph would do if things work — while the executor is the execution-time worker, where an absent fixture is `NeedsInput` because execution must not invent results. Same table, two honest readings; consumption was never an acceptance criterion. The driver (`apps/cli`) is the executor's real caller.

The two disagree today, and the divergences are the work item:

| case | `engine.rs` | `FixtureExecutor` |
|---|---|---|
| fixture absent | `Success` → `Succeeded` | `NeedsInput` |
| `Failure` | `Failed`, run fails | `RetryableFailure`, retryable |
| `Unknown` | `Paused` + `GHSIM001` | `NeedsInput` |
| predecessor released by | `Succeeded` only | `Succeeded`, `Waived` or `Skipped` |
| edge conditions | evaluated | deferred to 04d |

The absent-fixture row is a straight inversion: `FixtureExecutor` argues that inventing a success for an unspecified node is dishonest, while the engine that actually runs invents exactly that. Reconciling them changes simulation's observable behaviour, so it is not a refactor to slip in silently — it needs its own scoped change with the fixture semantics decided deliberately.

### A resource guard is not a domain bound

The 04b review found `MAX_PROJECTION_NODES` returning `LimitExceeded`, which makes a projection permanently unrebuildable, while decision 5.7 says exceeding a bound blocks for an owner decision and never truncates. Both are defensible alone and contradictory together.

They are different kinds of limit, and 04c separates them. `MAX_READY_SET` is a **domain bound**: a real execution reaches it and must block. `MAX_PROJECTION_NODES` is a **resource guard** against a corrupt or hostile history exhausting memory, and a legitimate execution must never reach it. That is only true while it stays above every domain bound, which nothing checked. A module-scope `const` in `core/execution/src/ready.rs` now pins that one relationship — verified to fail `cargo build`, not merely `cargo test`. It compares against `MAX_READY_SET` alone; `MAX_SIGNALS_PER_EXECUTION` is also 10,000 but counts signals rather than nodes, so it is not comparable and is deliberately not asserted.

### The purity invariant, corrected

04a's `source_invariants.rs` forbade `graphhelm-events`, `graphhelm-graph` and `graphhelm-policy` in the manifest. The design's §6 says `core/execution` depends on exactly those crates — the invariant was over-broad, not the design. It now forbids adapters, clocks and randomness, which is what it was always for, and a second test pins the exact dependency set so adding one is a deliberate edit rather than a silent manifest change.

## What 04d shipped: in-flight governance decisions

04d lets the Governor decide about a running graph. Every function is pure: nothing here appends an event, externalizes Evidence, or reads a clock — the decisions return what should happen, and the 04f driver makes it happen.

### Three governance event kinds

`signal_recorded`, `ghost_node_proposed` and `mutation_accepted` grow the closed event set from 20 to 23, with the envelope schema corrected in place per D-037, both copies byte-identical and both catalog digests recomputed (`schemas/catalog.json` and the frozen `1.0.0` copy agree; `checked_in_1_0_0_release_is_complete_and_raw_byte_identical` passes unmodified).

`signal_recorded` carries no free-form content, per D-036: the typed fields plus `envelopeSha256`, a digest binding the record to the raw envelope bytes that `admit_signal` returns for externalization as encrypted Evidence. The event contract is stricter than the signal contract — a schema-valid signal whose `id` or `source.id` is not an `OpaqueId` cannot be recorded on the wire, surfacing as `GovernanceError::InvalidSignal` (`core/governor/src/inflight.rs`, documented on `build_signal_record`).

`SignalSeverity` and `SignalSourceKind` moved into `graphhelm-protocols` — they now travel on the wire, and `core/events` cannot import them from `core/execution`. The originals derived only `Deserialize`; the moved enums add `Serialize`, which the wire role requires. `core/execution` re-exports both.

### The fold counts and guards; it does not judge

`ExecutionProjection` gains `signals_recorded` and `accepted_mutations`, both derived by folding history with `checked_add` (field declarations at `core/events/src/projection.rs:176-183`, fold arms in `apply_projection_event`), both `#[serde(default)]` so a pre-04d generation loads with them zero. A ghost is born, not transitioned into: `ghost_node_proposed` inserts `NodeState::Ghost`, and a proposal for a node that already has any state is `Corrupt` — proven able to fail by removing the guard (`a_ghost_proposal_for_an_existing_node_is_corrupt`). An acceptance whose recorded mode disagrees with the projection is `Corrupt` (`an_acceptance_under_the_wrong_mode_is_corrupt`, likewise sabotage-proven): decision 5.5's mode-binding is enforced at replay, not just at decision time.

### The pure decisions

`core/governor/src/inflight.rs`:

- `admit_signal` validates through `TypedSignal::parse`, refuses the signal past `MAX_SIGNALS_PER_EXECUTION` with `SignalBudgetExhausted` — blocking, never dropping, per decision 5.7 — and returns the payload to record, the bytes to externalize, and `may_propose_mutation` from decision 5.4's closed subset.
- `decide_mutation` rejects an unactionable signal in every mode and at every counter value (`the_unrecognized_kind_can_never_mutate` sweeps mode × counters), blocks at `MAX_ACCEPTED_MUTATIONS` in every mode, and otherwise maps D-022's modes: Autopilot accepts, Supervised requires approval, Manual — and an execution with no mode — rejects. The Manual arm was proven able to fail by making it accept: an autonomy grant nobody approved is the security property here.
- `override_with_waiver` mirrors the M03 waiver construction at `core/governor/src/apply.rs:323` — the same `PolicyWaiver` struct, the same `graphhelm_schema::validate_waiver` check — bound to `WaiverScope::Node` and the obligation it clears, refusing an empty risk acknowledgement. Decision 5.8 holds: no second waiver shape exists. Two consequences the schema enforces and the tests fixed against reality: the actor must match the `actorId` pattern (no `@`), and a projection with no published graph cannot produce a valid waiver, because `graphVersion` has a schema minimum of 1.

Id and timestamp are injected parameters, exactly as `ApplyServices` injects `ids` and `clock` — this crate never reads a clock or generates an id.

### Ghost approval was already on the wire

No new event kind for it: `node_outcome_recorded` with `Approved -> Ready` exists since 04b and `apply_transition` maps `(Ghost, Approved) -> Ready` since 04a.

### Bounded concurrency

`dispatch_plan(ready, in_flight, max_parallel)` in `core/execution/src/dispatch.rs` selects at most `max_parallel - in_flight` nodes in `BTreeSet` order — a replay dispatches the identical prefix, sabotage-proven by reversing the iterator. `max_parallel == 0` is `DispatchError::ZeroParallelism`, an authoring error surfaced loudly rather than an empty plan returned forever. `GraphBudgets.max_parallel_model_calls` (`Option<u64>`, `core/protocols/src/graph.rs:64`) gets its first reader in the caller's hands; the function keeps minimal `usize` inputs. `dispatch.rs` is covered by the purity source scan.

### Seams the 04f driver must respect

The mode-mismatch guard makes a stale acceptance *stream-poisoning*: an acceptance decided under one mode and appended after a mode change folds as corrupt on every subsequent replay. The decision must be re-derived against the projection as of the append point; `decide_mutation`'s rustdoc says so.

Three accounting seams are known and deferred: ghost births have no domain budget of their own (only the `MAX_PROJECTION_NODES` resource guard, which a signal-saturated execution proposing ghosts could legitimately approach); `node_states` can exceed that guard through the `NodeOutcomeRecorded` and `NodeStateChanged` arms, which guard other maps or nothing; and `MutationAccepted.graph_version` is folded without a successor check against `current_graph` — lineage is enforced by the `graph_version_published` checks, not here.

### What 04d does not do

When 04d shipped, nothing appended these events or drove intake; the 04f driver now does both (`apps/cli/src/commands/execution/`), and acceptance-to-publication wiring over the M03 `apply_draft`/`prepare_draft_publication` path remains for when signal-to-draft translation is designed — the Governor still decides *whether*, never *what*. The five undetected no-progress conditions stay undetected until real signals flow. The `simulate()`/`FixtureExecutor` divergence was reclassified as intentional in 04f; see the 04c section.

## What 04e shipped: pause, resume, cancel and recovery

### The vocabulary

`NodeOutcome` gained `Paused` and `Interrupted`; `SimulationStatus` gained `Cancelled`. All three appended at the end of their enums, so every existing wire name is untouched by construction. Cancel needed no new event kind: §13 defines it as a final status with partial effects recorded, and `execution_completed` already carries a status. Two event kinds were added — `execution_paused` and `execution_resumed` — growing the closed set from 23 to 25, with the envelope schema corrected in place per D-037, both copies byte-identical, both catalog digests recomputed.

### The transitions

Three arms, each closing a gap named in earlier milestones (`core/execution/src/transition.rs`):

- `(Blocked, Approved) -> Ready` — the owner resume path out of `Blocked`, open since 04a. Approval makes the node dispatchable on the next scheduler pass; nothing auto-starts out of a manual intervention.
- `(Ready | Queued, Paused) -> Paused` — graceful pause holds work that has not started. A running node is not pausable in this milestone (it completes instantly under the effect-free executor), and a ghost is not pausable in any: the ghost arm precedes the pause arm, and `a_ghost_never_becomes_runnable` now excludes `Paused` as a ghost destination too.
- `(Running, Interrupted) -> Blocked` — a node running when the execution stopped has unknown effects, and `Blocked` is the only legal consequence. `Interrupted` exists as its own outcome precisely because a crash is not something the executor reported; mapping it onto `RetryableFailure` would silently authorize a retry nobody judged safe. Verified able to fail by making the arm return `Queued` — the silent-retry bug — which `an_interrupted_running_node_blocks` catches.

The fold guards pause and resume as coherent history: pausing an execution that is not running, or resuming one that is not paused, is `Corrupt` (`an_incoherent_pause_or_resume_is_corrupt`, sabotage-proven).

### Recovery and resume preconditions

`core/execution/src/recovery.rs`, pure and covered by the purity source scan. `recovery_plan` names exactly the `Running` nodes in deterministic order. `resume_preconditions` enforces §11.4's decidable subset — an execution exists, is `Paused`, no node is `Running`, and the graph version to resume against matches — and its rustdoc names what it does not validate: lease renewal, route health, sandbox recreation and session invalidation are Milestone 05's.

The checkpoint question was answered by refusing to build a second checkpoint: the decidable subset of §11.2's content — graph version, node states, attempts, mode, counters, watermark — is exactly the `ProjectionGeneration` that 04b made durable, atomically swapped and rebuild-proven.

### The composed lifecycle, proven

`core/execution/tests/execution_lifecycle.rs` is the first time every pure piece since 04a runs together: `ready_set` proposes, `dispatch_plan` bounds, `FixtureExecutor` executes, `apply_transition` decides, the fold records, `recovery_plan` and `resume_preconditions` gate the lifecycle. The driver here is a test-only loop; 04f later shipped the production one in `apps/cli`, which inherits this test's sequencing. A three-node chain runs, pauses, crashes on a fresh history, recovers, is owner-approved, resumes and completes — and the entire history replays byte-identically, both directly and split through `ProjectionGeneration::apply_page`. The replay assertion was proven non-vacuous by mutating one serialized byte.

### Findings for 04f, discovered by composing

Three things the composition surfaced that no single piece showed:

1. **Resolved in 04f.** `resume_preconditions` accepted an execution with `Blocked` nodes regardless of why they were blocked. It now refuses `UntriagedInterruption` — `Blocked` with `last_outcome Interrupted` — and only that (`core/execution/src/recovery.rs`, `resume_refuses_an_untriaged_interruption_but_not_other_blocks`). Original finding: The gate refuses `Running` nodes (unrecovered interruptions) but not `Blocked` ones — so a driver can legally resume an execution whose interrupted nodes were recorded but never triaged by an owner. The blocked nodes simply never dispatch. Whether resume should also demand triage is a 04f design call, recorded here rather than decided silently.
2. **Enforced by the driver in 04f** (`apps/cli/src/commands/execution/resume.rs` recovers on entry; `pause.rs`/`approve.rs` carry the rest). The order: pause first, recover second, approve third. `execution_paused` folds legally while a node is still `Running` (its guard checks only the aggregate status), which is what lets `resume_preconditions` be observed refusing with `UnrecoveredInterruption`. The working sequence a driver must follow: fold `execution_paused` while the node is still `Running`, record the interruption (`Running -> Blocked`), owner-approve the blocked node, then resume. This is the order `execution_lifecycle.rs` actually executes and asserts; the plan's original narration (recover before pausing) makes the `UnrecoveredInterruption` refusal unobservable, because by then no node is `Running`.
3. **Resolved in 04f.** `Started` no longer touches run-length accounting (`dispatch_hops_do_not_break_an_identical_outcome_run`), so the bound fires for retry loops. Original finding: `Running` is reachable only via `(Queued, Started)`, so any state-machine-conforming history interleaves a `Started` between consecutive failures of one node, and the run-length counter resets every time. This holds for *any* driver, not just the test's: a persistently failing node exhausts `MAX_NODE_ATTEMPTS`, never `MAX_IDENTICAL_OUTCOMES`. The bound remains live only for outcomes that repeat without redispatch (`NeedsInput`/`NeedsCapacity` self-loops). 04f must either remove the dead condition from the retry arm or redesign the counting so the bound is reachable where intended. `a_failing_node_blocks_and_owner_resumes` pins the behaviour as it actually is.
4. **Enforced by the driver in 04f**: `resume` redispatches only nodes whose state is `Paused` (`resume_never_redispatches_a_waiting_node`). Original finding: `execution_paused`'s guard checks only the aggregate status, so pausing with a node in `WaitingInput` or `WaitingCapacity` is legal — but the pause arm accepts only `Ready | Queued`, so the waiting node stays waiting, unmarked. The pre-existing `(WaitingInput | WaitingCapacity | Paused, Started) -> Queued` arm means a resume driver that naively walks non-terminal nodes and emits `Started` would redispatch a waiting node as if its wait condition had resolved, without anything having checked that it did. The 04f driver must resume only the nodes it paused.


One test-infrastructure correction rode along: the manifest purity scan now reads only the `[dependencies]` table, because purity is a claim about the compiled library and the lifecycle test legitimately needs `graphhelm-simulation`, `chrono` and `tempfile` as dev-dependencies. The exact-dependency pin still holds for production dependencies.

## What 04f shipped: the driver and the operator CLI

### Two semantic corrections first

The driver was not built on top of known defects. `Started` no longer touches the run-length accounting — it is dispatch bookkeeping, not a semantic outcome — so `MAX_IDENTICAL_OUTCOMES` finally fires for a retry loop: the reworked `a_failing_node_blocks_and_owner_resumes` blocks at `attempts = MAX_IDENTICAL_OUTCOMES + 1 = 4`, nowhere near `MAX_NODE_ATTEMPTS`. And `ResumeError::UntriagedInterruption` refuses resume exactly when a node is `Blocked` with `last_outcome Interrupted` — recorded but never looked at — while a node blocked for any other reason does not hold the rest of the graph hostage.

### The driver

`apps/cli/src/commands/execution/driver.rs` drives to quiescence: approve drafts to `Ready`, `ready_set`, `dispatch_plan` (`max_parallel_model_calls`, default 1 — the field's first real reader), `classify_progress` before every dispatch, two `Started` hops, the executor's outcome, every `next_state` from `apply_transition`, every append through the production store. One derivation the plan glossed and the implementer proved: `ready_set` returns only `Ready` nodes, so the dispatch candidates are its output **union** the currently `Queued` nodes, or a retry could never redispatch.

### The commands

JSON-only `execution start|status|signal|approve|pause|resume|cancel`, every reply the standard four-key envelope, every failure a redaction-safe code (`GHCLI003_SIGNAL_INVALID`, `GHCLI004_SIGNAL_UNRECORDABLE`, `GHCLI005_EXECUTION_STATE`). `signal` writes the envelope bytes to `--evidence-out` before anything else and fails closed if it cannot — an event is never appended without its evidence preserved, and an unrecordable identity still preserves the evidence while refusing, saying so. `approve` is the triage act and never auto-drives (D-020). `pause` reports exactly which nodes it held; `resume` recovers first, gates on the preconditions, and redispatches only the held nodes — the blind-redispatch sabotage fails the suite. `cancel` finalizes every non-terminal node and refuses a second call; the CLI-level terminal check is currently the only guard, since the fold accepts a second `execution_completed`.

`the_operator_story_runs_end_to_end_and_replays_byte_identical` drives the story through the compiled binary and replays the final stream twice, byte-identically — the operator-visible form of the milestone's replay guarantee. The `execution_cli` suite runs in the gate as its own stage, proven able to go red.

### Seams stated, not hidden

`resume` re-takes `--file`/`--fixtures` and trusts them: the projection does not carry the graph spec, and nothing cross-checks the supplied file against the version and hash `execution_started` recorded. An operator can resume against the wrong graph file and the driver will believe them. Closing it needs either spec recovery from the published version or a hash cross-check on resume — Milestone 05, recorded here so 04f does not claim it. `UntriagedInterruption` is enforced and library-tested but not exercised through the CLI suite: the synchronous driver resolves every dispatch before returning, so only a hard kill of the CLI process between the non-transactional appends of one dispatch can leave a node `Running` today — rare but possible, and `resume`'s `recovery_plan` step is what catches it. A real runtime that can crash mid-node makes the path routine. Ghost approval is exercised at the library level; the CLI cannot yet author a ghost because signal-to-draft translation is undesigned.

Three more, from the closing review. The driver **never invents an outcome**: it consults `classify_progress` before each dispatch but always calls the executor and records what it actually said — an earlier draft fabricated a `RetryableFailure` on a predicted bound, which would have made approval a permanent dead end for an attempts-exhausted node; `approve_is_not_a_dead_end_once_the_condition_is_fixed` now pins the honest behaviour. Owner-initiated commands are recorded under the owner actor while the driver's own hops stay under the system actor, so the log can tell sovereignty from machinery. And dispatch under the blended candidate pool is lexicographic-prefix fair only: with `max_parallel` at the default 1, a persistently retrying, alphabetically earlier node can starve a sibling's first attempt for up to the bound's worth of passes — latent for fan-out graphs, none of which exist in the examples yet; a fairness policy belongs to Milestone 05's scheduler work.

## Acceptance, mapped

The design's §8 criteria against their evidence. Nothing below claims more than the named test shows.

| §8 criterion | Evidence |
|---|---|
| A published graph executes to completion, every transition durable and replayable | `start_drives_a_two_node_graph_to_completion_and_status_reports_it_independently`; every append via the production store |
| Replay reconstructs identical state, no wall-clock or ordering dependence | `replay_is_identical_across_runs`, `a_discarded_generation_rebuilds_to_identical_state`, and the CLI-level `the_operator_story_runs_end_to_end_and_replays_byte_identical` |
| Only the Governor mutates a running graph; a signal alone never changes it | `decide_mutation` under Manual/no-mode rejects; `the_unrecognized_kind_can_never_mutate`; the fold's mode-mismatch guard |
| A ghost consumes no work until approved and is visible before approval | `a_ghost_never_becomes_runnable` (property), `a_proposed_ghost_appears_in_ghost_state` (fold), `(Ghost, Approved) -> Ready` (transition) |
| Mode switching mid-execution has defined, tested behaviour both directions | `mode_binds_at_the_decision_not_at_admission`, `an_acceptance_under_the_wrong_mode_is_corrupt` |
| An interrupted execution recovers to a defined state and never resumes a node with unknown effects | `an_interrupted_running_node_blocks`, `recovery_interrupts_exactly_the_running_nodes`, `resume_refuses_an_untriaged_interruption_but_not_other_blocks`, the lifecycle act 3 |
| An owner override records actor, reason, risks, version, waiver; status stays accurate | `an_override_is_the_existing_waiver_bound_to_node_and_obligation`, `an_override_without_acknowledged_risks_is_refused`, schema-validated against the M03 waiver contract |
| The full local gate is green, including both PostgreSQL locale passes | `./ci/gate.ps1` GREEN at every milestone close, now including `cli: execution_cli` |

**Named gaps, in the table's own spirit:** signal envelopes externalize to operator files, not encrypted Evidence (the sealed pipeline has no signal content position — Milestone 05); signal-to-draft translation is undesigned, so the Governor decides *whether*, never *what*; ghost births carry no domain budget (accepted risk: bounded by the resource guard and, on acceptance, the mutation budget); the resume file-trust seam above; `simulate()` and `FixtureExecutor` intentionally diverge — authoring explores optimistically, execution refuses to invent — and the divergence table earlier in this document is the reference, not outstanding work.

## Explicitly out of scope

Everything below is outside Milestone 04 entirely:

- **Wiring `simulate()` through `FixtureExecutor` and `apply_transition`,** resolving the divergences tabulated above.

One consequence of this still stands from `apply_transition`'s table: `Linting` is accepted as a transition *source* (`(S::Draft | S::Linting, O::Approved) => Ok(S::Ready)`) but nothing produces a node in that state — lint completion belongs to the authoring flow, not this milestone. `Paused` gained its producers in 04e (`(Ready | Queued, Paused) -> Paused`), and `Blocked` gained its owner resume path there too (`(Blocked, Approved) -> Ready`).

## Acceptance evidence

`core/execution/tests/source_invariants.rs`, `core/execution/tests/transition_properties.rs` and `core/execution/tests/scheduling_properties.rs` cover purity, the state machine and scheduling. `core/events/tests/execution_projection.rs` and `adapters/postgres-event-store/tests/projection.rs` cover the projection, its generation compatibility, and the watermark guard; the PostgreSQL suite is `#[ignore]`d in ordinary runs and requires `GRAPHHELM_TEST_ADMIN_URL`, matching Milestone 03's convention.
