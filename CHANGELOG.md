# Specification Changelog

## Graph Engine and Governor, 04d — 2026-08-13

- Three governance event kinds added — `signal_recorded`, `ghost_node_proposed`, `mutation_accepted` — growing the closed set from 20 to 23; the `1.0.0` envelope schema corrected in place under D-037 with both copies byte-identical and both catalog digests recomputed. `signal_recorded` carries no free-form content per D-036: typed fields plus a digest binding the record to the raw envelope bytes destined for encrypted Evidence.
- `SignalSeverity` and `SignalSourceKind` moved to `graphhelm-protocols` as wire vocabularies, gaining the `Serialize` their new role requires; `core/execution` re-exports both.
- The projection folds `signals_recorded` and `accepted_mutations` by counting history, and folds a ghost's birth: a proposal for a node that already has any state is corrupt, and an acceptance recorded under a mode the execution was not in is corrupt — decision 5.5 enforced at replay.
- Pure governance decisions added in `core/governor`: `admit_signal` blocks at the signal budget rather than dropping evidence; `decide_mutation` maps D-022's modes with Manual and no-mode rejecting, blocks at `MAX_ACCEPTED_MUTATIONS` in every mode, and rejects an unrecognized kind at every mode and counter value; `override_with_waiver` reuses the M03 waiver verbatim, node-scoped, refusing an empty risk acknowledgement. Id and clock are injected, never read.
- Bounded concurrency added as `dispatch_plan`: a deterministic prefix of the ready set, `ZeroParallelism` surfaced loudly. The function that will read `max_parallel_model_calls` now exists; wiring the field to it is the 04f driver's work.
- Nothing appends these events or drives intake yet; the decisions await the 04f driver. Ghost approval reuses the existing `Approved -> Ready` path unchanged.

## Graph Engine and Governor, 04c — 2026-08-13

- `ready_set` added: which nodes may be dispatched now. Only `Ready` nodes are dispatchable, and a test pins that every dispatchable state accepts a `Started` outcome, so the scheduler cannot propose work the state machine rejects. Dependencies are fail-closed — every incoming edge gates, whatever its type — and a predecessor releases its dependent only when `Succeeded`, `Waived` or `Skipped`. Exceeding `MAX_READY_SET` blocks rather than truncating, per decision 5.7.
- `NodeState::Ghost` is excluded from the ready set by construction and never releases a dependent, making "consumes no tokens" structural rather than conventional. Covered by a property test sampling randomised assignments of the other nodes' states.
- `classify_progress` added: retry exhaustion and repeated identical outcomes, read against the counters the projection derives. It reads the run length through `identical_outcomes_for`, so a run belonging to a different outcome cannot block a node on its first failure.
- Five of the seven `OBSERVABILITY_AND_RECOVERY.md` §15 no-progress conditions are deliberately not detected; they need signal intake or real tool calls, and none is approximated.
- `FixtureExecutor` added in `core/simulation`: the first and only milestone-04 `NodeExecutor`, consulting a fixture table and nothing else. Its answer does not depend on the attempt number. It has no callers yet — `simulate()` still drives its own transitions, so the seam is defined but simulation is not yet a consumer of it; the divergences between the two are tabulated in the milestone document and tracked as outstanding work.
- `MAX_PROJECTION_NODES` separated from decision 5.7's domain bounds as a resource guard, published, and pinned above `MAX_READY_SET` by a module-scope `const` verified to fail `cargo build`. It is not compared against `MAX_SIGNALS_PER_EXECUTION`, which counts a different dimension.
- 04a's purity invariant narrowed to what it protects — adapters, clocks and randomness, not sibling core crates — with the exact dependency set pinned by a second test.

## Graph Engine and Governor, 04a/04b — 2026-08-13

- `core/execution` added: a pure crate with no I/O, no clock, no randomness, and no adapter dependency, enforced by a source invariant test rather than documented alone.
- `NodeState::Ghost` added to the shared vocabulary; its only legal exit is approval to `Ready`, proven by property test across every outcome and counter value.
- Bounds fixed as counters, never durations: `MAX_NODE_ATTEMPTS` 8, `MAX_IDENTICAL_OUTCOMES` 3, `MAX_ACCEPTED_MUTATIONS` 64, `MAX_READY_SET` 1024, `MAX_SIGNALS_PER_EXECUTION` 10,000.
- The closed Graph Signal typed subset added; an unrecognized kind is recorded as evidence but can never propose a mutation.
- `apply_transition` added: total and property-tested for totality, determinism, and ghost-safety. The `NodeExecutor` trait added as the Milestone 05 seam, with no implementor yet.
- `NodeOutcome` and `ExecutionMode` added to `graphhelm-protocols` as closed vocabularies. The four execution event kinds — `execution_started`, `execution_mode_changed`, `node_outcome_recorded`, `execution_completed` — added to the closed event set, growing it from 16 to 20; the `1.0.0` envelope schema corrected in place under D-037, both schema copies kept byte-identical, and both catalog digests recomputed.
- `ExecutionProjection` extended with execution ID, mode, and per-node attempt and identical-outcome counters, all derived by folding history rather than read from a payload. A generation predating these fields loads with them empty; one missing a pre-existing field still fails.
- Replay proven identical across runs, and a discarded generation rebuilt through `apply_page` proven to land on the same state as a direct replay.
- `GHPROJ001_WATERMARK_MISMATCH`, shipped in Milestone 03 with no observed test, now has one: it drives the guard in `ProjectionRebuilder::rebuild` through a test-double repository, since the production PostgreSQL adapter cannot structurally reach it.
- Scheduling, in-flight governance, pause/resume/cancel, and the operator CLI remain out of scope; they are Milestones 04c through 04f.

## Production Event and Evidence Store — 2026-08-12

- Decisions D-035, D-036, and D-037 accepted with ADR-021 through ADR-023: authoring and persistence are distinct representations, the Governor externalizes free-form content as encrypted Evidence, and required content that is unavailable blocks execution.
- Safe persistence projection design and a focused Event/Evidence Store threat model added.
- Single pre-release schema baseline `1.0.0` rebuilt with 15 contracts, adding `PersistedGraphVersion`, event envelope, Evidence record, artifact reference, repository scope, and sensitivity; the intermediate release `1.1.0` removed.
- Eleven typed content positions registered, including the `context_path`, `permission_path`, and `isolation_path` authoring scopes.
- Local JSONL repository and PostgreSQL adapter implemented against one wire contract, with forced row-level security, transaction-local scope, authenticated stream heads and integrity checkpoints, and least-privilege runtime roles.
- Encrypted Evidence, sealed local key provider, authenticated revocation journal, legal holds, and auditable cryptographic erasure implemented.
- Disposable projection generations, fail-closed executable materialization, encrypted streaming backup, and verified restore implemented.
- Operator commands `events verify`, `events rebuild`, `events backup`, and `events restore` added, with bounded JSON configuration and out-of-band key material.
- No legacy compatibility layer: superseded event formats, importers, dual readers, and fallback branches removed before the first public release. Existing developer repositories and databases must be deleted and recreated.
- Milestone documentation and operational procedures published; the specification version remains 0.1.1.

## 0.1.1-spec — 2026-08-08

- Product name selected: GraphHelm.
- Naming decision and brand architecture documented.
- Initial Codex prompt for the Foundation Graph Kernel.
- Manual override example fixed with explicit deploy target.

## 0.1.0-spec — 2026-08-08

- Full PRD for Programação 5.0.
- Decisions on topology, autonomy, harness, agents, context, Dreams, models, isolation, UI, open source, and licensing.
- Studio functional specification.
- Harness Compiler and Graph Governor.
- Complete Graph Engineer guide.
- Graph DSL v1 and JSON Schemas.
- Event Store, Knowledge Graph, Living Documentation, and Context Capsules.
- Universal Model Gateway with BYOK, official subscriptions, and local models.
- Threat model and isolation tiers.
- Observability, checkpoints, replay, and recovery.
- AGPLv3 governance + commercial license + CLA.
- Graph and manifest examples.
