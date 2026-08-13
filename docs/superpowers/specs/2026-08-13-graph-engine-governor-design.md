# Milestone 04 - Graph Engine and Governor: design

Status: proposed. Derived by aggregating the existing normative documentation; no new product design is
introduced here. Every requirement below cites the document or decision it comes from.

## 1. What this milestone is

Milestone 03 made persistence production-safe: a graph can be published, versioned, externalized into
encrypted Evidence, and replayed. Nothing executes it.

Milestone 04 makes a published graph **run**, durably and adaptively, with the Governor as the only
authority that may change it while it runs. Model calls, tool calls, and sandboxes stay out; the
executor drives the same effect-free node transitions `graphhelm-simulation` already performs, so the
entire milestone is testable offline. Milestone 05 (Runtime) replaces the effect-free executor with
real agent work behind the same contracts.

The split matters: this milestone proves *governance and durability of execution*. If real model calls
were in scope, every interesting property would be untestable without providers.

## 2. Sources

| Requirement area | Source |
|---|---|
| Graph is adaptive, versioned, altered only by the Governor | `docs/DECISION_REGISTER.md` D-018 |
| Replacement agent never auto-starts after manual intervention | D-020 |
| Ghost nodes: proposed expansions visible, consuming no tokens pre-approval | D-021 |
| Autopilot / Supervised / Manual Graph, switchable mid-execution | D-022 |
| Visual edits immediate; operational edits become a transactional Graph Draft | D-025 |
| Owner sovereignty: pause, remove gates, skip phases, force deploy with waiver | D-019 |
| Execution and feedback loop | `docs/harness/HARNESS_SPEC.md` §18 |
| Graph Signals | `HARNESS_SPEC.md` §19 |
| Graph Governor responsibilities | `HARNESS_SPEC.md` §20 |
| Pause, cancel, retry, no-progress detection, failure categories | `docs/operations/OBSERVABILITY_AND_RECOVERY.md` §12-16 |
| Checkpoints and resume preconditions | `OBSERVABILITY_AND_RECOVERY.md` §11 |
| Node/edge contracts, states, waivers, replay | `docs/product/ROADMAP_AND_ACCEPTANCE.md` §8.2 |

## 3. What already exists

Do not rebuild these. Milestone 04 composes them.

- `graphhelm-graph` - canonical semantic hashing, immutable `GraphVersion`, lint, safe persistence
  projection.
- `graphhelm-governor` - `analyze_draft`, `apply_draft`, externalization, safe publication,
  fail-closed materialization. This is **draft application at rest**, not execution.
- `graphhelm-simulation` - bounded, fixture-driven, effect-free node state transitions.
- `graphhelm-events` / `graphhelm-postgres-event-store` - durable append-only history, replay,
  projections, integrity checkpoints.
- `graphhelm-policy` - deterministic obligations and waiver eligibility.

The gap is that nothing owns a *running* execution: there is no scheduler, no signal intake, no
in-flight mutation path, no mode switching, and no pause/resume over durable state.

## 4. Boundary

**In scope:** execution state machine and scheduler; Graph Signal intake; in-flight Governor
mutation via transactional drafts; ghost nodes; the three modes; pause, resume, cancel; retry
classification; no-progress detection; owner override with waiver during execution; durable
checkpoint and recovery of an interrupted execution; replay of a complete execution.

**Out of scope:** model calls and the Universal Model Gateway; tool calls and the Tool Broker;
sandboxes and isolation tiers; Credential Broker; Context Compiler; the public Runtime API and its
authentication; Studio; multi-node scheduling; Knowledge Graph and Dreams.

Node work is performed by an injected `NodeExecutor` trait whose only milestone-04 implementation is
effect-free and fixture-driven. Milestone 05 supplies the real one.

## 5. Decisions that must be accepted before any task starts

Milestone 03 required an accepted ADR before implementation and that gate held; the same applies. Each
of these is a genuine tension in the existing documentation, not a detail.

1. **Mutation identity during execution.** D-018 says the graph is versioned and only the Governor
   changes it. Does every accepted in-flight mutation publish a new `GraphVersion`, or does an
   execution carry a version plus an ordered mutation log? Publishing per mutation is simpler to
   reason about and reuses the M03 publication path; it also multiplies versions and Evidence during
   an adaptive run.
2. **Ghost node representation.** D-021 requires proposed expansions to be visible while consuming no
   tokens. Are ghosts part of the persisted graph in a `proposed` state, or execution-local until
   approved? Persisting them makes them replayable and visible after a restart; it also puts
   unapproved content into the authoritative graph.
3. **Node state vocabulary.** `graphhelm-simulation` already has states and
   `ROADMAP_AND_ACCEPTANCE.md` §8.2 names more (pause/resume, ghost, bypassed, waived). One closed
   enum must serve both, or simulation and execution diverge permanently.
4. **Signal authority.** §19 says agents emit typed signals and §20 says only the Governor mutates.
   Signals must therefore be proposals with no direct effect. The exact closed signal grammar, and
   what happens to an unrecognized signal, must be fixed - fail closed is the milestone-03 precedent.
5. **Mode semantics on an in-flight execution.** D-022 allows switching Autopilot/Supervised/Manual
   during execution. What happens to a mutation already accepted under Autopilot when the owner
   switches to Manual mid-run must be defined, not discovered.
6. **Where execution state lives.** The M03 event store is the durable source of truth and
   projections are disposable. Execution state must be a projection rebuildable from events, or the
   append-only guarantee is broken by a mutable execution table.
7. **No-progress detection thresholds.** `OBSERVABILITY_AND_RECOVERY.md` §15 lists seven detectable
   conditions but no bounds. Deterministic, replay-stable thresholds are required; anything
   wall-clock dependent breaks replay.
8. **Owner override during execution.** D-019 permits removing gates mid-run. The waiver must bind
   the exact graph version, node, and obligation, reusing the M03 waiver contract rather than adding
   a second one.

## 6. Proposed crate boundaries

- `core/execution` (new) - execution state machine, scheduler, signal grammar, no-progress
  detection, mode semantics, `NodeExecutor` trait. Depends on `protocols`, `graph`, `policy`,
  `events`. Must not depend on any adapter, on `simulation`, or on a runtime.
- `core/governor` (extend) - in-flight mutation intake: signal-to-draft translation, ghost node
  lifecycle, approval, and the existing transactional application path.
- `core/simulation` (extend) - provide the effect-free `NodeExecutor` implementation, so simulation
  becomes a consumer of the execution contract instead of a parallel one.
- `apps/cli` (extend) - JSON-only `execution start|status|signal|approve|pause|resume|cancel`.

Dependency direction stays inward toward protocols; no core crate may depend on an adapter.

## 7. Plan decomposition

Each is one plan producing working, testable software on its own. They are ordered by dependency.
Writing the task-level implementation plan for 04a is the next action.

- **04a - Execution contracts and state machine.** The closed node/execution state enum reconciling
  simulation and roadmap vocabularies, the closed signal grammar, transition rules, and the
  `NodeExecutor` trait. Pure, no I/O, exhaustively property-tested. Resolves decisions 3 and 4.
- **04b - Durable execution projection.** Execution state as a disposable projection over the M03
  event store, with rebuild and watermarks. Resolves decision 6.
- **04c - Scheduler and effect-free executor.** Ready-set computation, bounded concurrency, retry
  classification, no-progress detection. `simulation` supplies the executor. Resolves decision 7.
- **04d - In-flight governance.** Signal intake, ghost node lifecycle, approval, mutation publication,
  owner override with waiver. Resolves decisions 1, 2, 5 and 8.
- **04e - Pause, resume, cancel, recovery.** Checkpoint content per §11.2, resume preconditions per
  §11.4, crash recovery of an interrupted execution, and full replay of a completed one.
- **04f - Operator CLI, gate, documentation.** JSON-only commands, `ci/gate.ps1` integration,
  milestone document, and the final review.

## 8. Acceptance

- A published graph executes to completion with every transition durable and replayable.
- Replaying an execution's events reconstructs identical execution state, with no wall-clock or
  ordering dependence.
- Only the Governor mutates a running graph; a signal alone never changes it.
- A ghost node consumes no execution work until approved and is visible before approval.
- Switching mode mid-execution has defined, tested behaviour in both directions.
- An interrupted execution recovers to a defined state and never resumes a node whose effects are
  unknown.
- An owner override records actor, reason, acknowledged risks, graph version, and waiver, and the
  result status stays accurate.
- The full local gate is green, including both PostgreSQL locale passes.

## 9. Risks

- **Scope creep into Runtime.** The moment a real model or tool call appears, the milestone stops
  being offline-testable. The `NodeExecutor` seam is the defence and must not leak provider concepts.
- **Version explosion.** Decision 1 taken naively can publish a graph version per signal on an
  adaptive run. Whichever way it is decided, bound it.
- **Replay divergence.** Any wall-clock, map-iteration, or locale dependence in scheduling breaks
  replay. Milestone 03 shipped exactly this defect class twice; the source invariant added there
  should be extended to cover scheduling order.
