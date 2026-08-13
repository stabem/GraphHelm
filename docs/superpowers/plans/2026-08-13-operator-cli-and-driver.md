# Milestone 04f - Operator CLI and Driver Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the driver that wires five milestones of pure decisions to the durable store, expose it as JSON-only `execution start|status|signal|approve|pause|resume|cancel` operator commands, resolve the design calls the earlier plans deferred, and close Milestone 04 with an acceptance map and a full-milestone review.

**Architecture:** The driver lives in `apps/cli` as a command-layer module, following `graph simulate`'s exact shape: load, lint and publish a graph with `publish_loaded`, then drive the pure pieces — `ready_set`, `dispatch_plan`, `classify_progress`, `FixtureExecutor`, `apply_transition`, `admit_signal`, `decide_mutation`, `recovery_plan`, `resume_preconditions` — against the local event repository, appending every step as events. The executor is effect-free, so driving runs to *quiescence* instantly: completion, blocked, waiting, or paused. No new core crate; two deliberate semantic changes land in `core/events` and `core/execution` first, because the driver must not ship on top of a bound that cannot fire and a resume gate that cannot see untriaged interruptions.

**Tech Stack:** Rust 1.97.1, edition 2024. `apps/cli` (clap, tokio runtime helper, `Outcome` JSON envelope), local `graphhelm-events` repository. No new dependency. PostgreSQL 16+ for the adapter matrix.

**Design source:** `docs/superpowers/specs/2026-08-13-graph-engine-governor-design.md` §6 (the `apps/cli` line), §7 (04f), §8 (acceptance); the four 04e findings and the 04d seams recorded in `docs/milestones/graph-engine-governor.md`.

---

## Design calls this plan resolves, stated up front

**1. `MAX_IDENTICAL_OUTCOMES` becomes reachable: the fold stops letting `Started` touch the run length.** The 04e review proved the bound structurally dead for retry loops — `Running` is reachable only via `(Queued, Started)`, so a `Started` always interleaves and resets the counter. The fix is semantic, not cosmetic: `Started` is dispatch bookkeeping, not a semantic outcome of work, so the fold now *skips* it in run-length accounting — it neither increments nor resets `identical_outcomes` (it still counts toward `node_attempts`, unchanged). Consecutive `RetryableFailure`s then accumulate across redispatches and the bound fires as designed. This changes replay semantics for histories containing interleaved `Started` outcomes; the product is unpublished and no stored history exists outside this repository's tests (D-037's premise), so the change is made cleanly rather than versioned.

**2. Resume refuses untriaged interruptions.** The 04e finding: `resume_preconditions` accepts an execution whose interrupted nodes sit `Blocked`, recorded but never looked at. The projection can decide this — a node is an *untriaged interruption* exactly when its state is `Blocked` and its `last_outcome` is `Interrupted`. `ResumeError` gains `UntriagedInterruption`, and the gate refuses until the owner approves, waives, skips or cancels each such node. A node blocked for any *other* reason (attempts exhausted, no progress) does not block resume: the owner may legitimately resume the rest of the graph and deal with it later.

**3. `UnrecordableIdentity` gets its own operator surface.** The CLI `signal` command distinguishes the two refusals the 04d split created: a garbage envelope is `GHCLI003_SIGNAL_INVALID` and nothing is stored; a schema-valid signal whose identity cannot go on the wire is `GHCLI004_SIGNAL_UNRECORDABLE`, and the command still writes the envelope bytes to the operator-supplied evidence path before refusing — the evidence is preserved, the event is not appended, and the diagnostic says exactly that.

**4. Ghost births stay unbudgeted, deliberately.** A ghost proposal is bounded today by `MAX_PROJECTION_NODES` (the resource guard) and, on acceptance, by `MAX_ACCEPTED_MUTATIONS`. A dedicated birth budget would need signal volume data that only a real runtime produces; Milestone 05 owns it. Recorded as an accepted risk in the milestone document, with the reasoning, not silently dropped.

**5. `simulate()` and `FixtureExecutor` stay divergent, reclassified as intentional.** The 04c review found the doc falsely claiming simulation consumed the executor. The truthful resolution is not to force consumption: `simulate()` is the *authoring-time* explorer — an absent fixture defaults to `Success` because authoring asks "what would this graph do if things work". The executor is the *execution-time* worker — an absent fixture is `NeedsInput` because execution must not invent results. Same fixture table, two honest readings. The milestone document's divergence table gets this rationale and drops the "outstanding work" framing; the design's acceptance criteria never required consumption.

**6. Driving to quiescence is the command model.** The design's command list has no `step` or `run` — because with an effect-free executor there is nothing to wait for. `execution start` publishes, starts and drives until quiescence: `Completed`/`Failed` (terminal, appends `execution_completed`), or blocked/waiting/paused (leaves the stream open). `resume` re-drives to quiescence after its preconditions pass. The drive loop honours the sequencing contract five reviews wrote down: two `Started` hops per dispatch, `classify_progress` consulted before each redispatch, decisions re-derived against the projection at the append point, resume touches only the nodes the pause held.

## Hard-won process rules, binding on every task

- **The code wins over this plan**; report every discrepancy.
- **Revert sabotage from a `cp` backup, never `git checkout --`.**
- **Run the workspace clippy**; a per-crate pass proves nothing.
- **Write documentation from the code as built, then re-read the whole file** — including the header, which went stale for three milestones.
- **Every new guard observed failing once, deliberately.**
- No schema changes are expected in this plan. If a task believes it needs one, stop and report NEEDS_CONTEXT — the wire vocabulary was deliberately completed in 04e.

---

## File structure

| File | Responsibility |
|---|---|
| `core/events/src/projection.rs` | `Started` skipped in run-length accounting |
| `core/execution/src/recovery.rs` | `ResumeError::UntriagedInterruption` |
| `apps/cli/src/commands/execution/mod.rs` | Command registry, shared failure codes, stream addressing |
| `apps/cli/src/commands/execution/driver.rs` | Drive-to-quiescence over the pure pieces |
| `apps/cli/src/commands/execution/{start,status,signal,approve,pause,resume,cancel}.rs` | One command each |
| `apps/cli/tests/execution_cli.rs` | The operator story end to end |
| `ci/gate.ps1` | New `cli: execution_cli` stage |

---

### Task 1: Make the no-progress bound reachable

**Files:**
- Modify: `core/events/src/projection.rs`
- Test: `core/events/tests/execution_projection.rs`, `core/execution/tests/execution_lifecycle.rs`

- [ ] **Step 1: Write the failing test**

Add to `core/events/tests/execution_projection.rs`:

```rust
/// `Started` is dispatch bookkeeping, not a semantic outcome of work. It must not break a run of
/// identical failures, or the no-progress bound can never fire for a retry loop — the state
/// machine forces a `Started` between any two failures of one node (04e finding 3).
#[test]
fn dispatch_hops_do_not_break_an_identical_outcome_run() {
    let events = execution_events(&[
        Outcome::RetryableFailure,
        Outcome::Started,
        Outcome::Started,
        Outcome::RetryableFailure,
    ]);
    let projection = replay(&scope(), STREAM, &events).unwrap();
    assert_eq!(projection.identical_outcomes.get("start"), Some(&2));
    assert_eq!(
        projection.identical_outcomes_for("start", NodeOutcome::RetryableFailure),
        2
    );
}
```

Note the existing helpers: `execution_events` computes `next_state` through the real `apply_transition`, so the sequence above is the legal `fail -> requeue -> redispatch -> fail` walk.

- [ ] **Step 2: Run to verify it fails** — the current fold resets on `Started`, so expected `left: Some(1)`. Quote it.

- [ ] **Step 3: Change the fold**

In `apply_projection_event`'s `NodeOutcomeRecorded` arm, wrap the run-length block:

```rust
            // `Started` is dispatch bookkeeping. It counts as an attempt when it enters `Running`,
            // but it neither extends nor breaks a run of identical work outcomes — otherwise the
            // two-hop dispatch pattern makes MAX_IDENTICAL_OUTCOMES structurally unreachable,
            // which the 04e review proved holds for every state-machine-conforming driver.
            if payload.outcome != NodeOutcome::Started {
                let run = match projection.last_outcome.insert(node.clone(), payload.outcome) {
                    Some(previous) if previous == payload.outcome => projection
                        .identical_outcomes
                        .get(&node)
                        .copied()
                        .unwrap_or(0)
                        .checked_add(1)
                        .ok_or(ReplayError::LimitExceeded)?,
                    _ => 1,
                };
                projection.identical_outcomes.insert(node.clone(), run);
            }
```

`last_outcome` therefore no longer records `Started` either — check every reader: `identical_outcomes_for` (pairs run with outcome — consistent), and any test asserting `last_outcome` after a `Started`. Fix the *tests'* expectations where they encoded the old semantics; report each one.

- [ ] **Step 4: Run the crate and lifecycle suites.** `core/execution/tests/execution_lifecycle.rs`'s `a_failing_node_blocks_and_owner_resumes` pinned the old behaviour (exhausts via attempts, never identical-outcomes). It must now block **earlier**, via `NoProgress` at `MAX_IDENTICAL_OUTCOMES` — rework that test to assert the new, intended route, and update the milestone-doc finding in Task 7 rather than here. Quote the before/after.

- [ ] **Step 5: Prove it can fail** — restore the unconditional reset transiently, confirm the new test fails, revert from a backup, re-confirm. Quote both.

- [ ] **Step 6: Commit**

```bash
git add core/events core/execution
git commit -m "fix(events): stop dispatch hops from breaking identical-outcome runs"
```

---

### Task 2: Resume refuses untriaged interruptions

**Files:**
- Modify: `core/execution/src/recovery.rs`
- Test: `core/execution/src/recovery.rs` (inline `mod tests`)

- [ ] **Step 1: Write the failing test**

```rust
    /// An interruption that was recorded but never looked at must hold resume. A node blocked for
    /// any other reason does not: the owner may resume the rest of the graph and deal with it
    /// later (04e finding 1, resolved).
    #[test]
    fn resume_refuses_an_untriaged_interruption_but_not_other_blocks() {
        let mut interrupted = projection(&[("a", NodeState::Blocked)], Some(SimulationStatus::Paused));
        interrupted
            .last_outcome
            .insert("a".to_owned(), NodeOutcome::Interrupted);
        assert_eq!(
            resume_preconditions(&interrupted, None),
            Err(ResumeError::UntriagedInterruption)
        );

        let mut exhausted = projection(&[("a", NodeState::Blocked)], Some(SimulationStatus::Paused));
        exhausted
            .last_outcome
            .insert("a".to_owned(), NodeOutcome::RetryableFailure);
        assert_eq!(resume_preconditions(&exhausted, None), Ok(()));
    }
```

- [ ] **Step 2: Run to verify it fails** (no such variant), then implement: add the variant with the doc comment carrying the triage rationale, and the check after `UnrecoveredInterruption`:

```rust
    if projection.node_states.iter().any(|(node, state)| {
        *state == NodeState::Blocked
            && projection.last_outcome.get(node) == Some(&NodeOutcome::Interrupted)
    }) {
        return Err(ResumeError::UntriagedInterruption);
    }
```

- [ ] **Step 3: Update the lifecycle test.** Its act-3 sequence (pause → recover → approve → resume) already triages before resuming, so it should still pass — run it and confirm rather than assume. If it asserted `Ok(())` at a point that is now refused, the test encoded the gap; fix the test to the new contract and say so.

- [ ] **Step 4: Sabotage** — make the new check compare against `NodeOutcome::Cancelled`, confirm the test fails, revert from a backup, re-confirm. Quote both.

- [ ] **Step 5: Commit**

```bash
git add core/execution
git commit -m "fix(execution): refuse resume while an interruption is untriaged"
```

---

### Task 3: The driver and `execution start` / `status`

**Files:**
- Create: `apps/cli/src/commands/execution/mod.rs`, `driver.rs`, `start.rs`, `status.rs`
- Modify: `apps/cli/src/commands/mod.rs`, `apps/cli/src/main.rs` (registry + clap wiring, mirroring how `events` subcommands register)
- Test: `apps/cli/tests/execution_cli.rs`

Follow `apps/cli/src/commands/simulate.rs` as the template for everything: `publish_loaded(&loaded, owner(...))` for publication, `event_store(events)` for the local repository, `SystemClock`/`UuidIds` for services, `Outcome::domain`/`Outcome::internal` for output, and the events module's redaction-safe `Failure` pattern for error codes. New codes live in `execution/mod.rs`: `GHCLI003_SIGNAL_INVALID`, `GHCLI004_SIGNAL_UNRECORDABLE`, `GHCLI005_EXECUTION_STATE` (a command refused by preconditions or guards), reusing `GHCLI001_ARGUMENT_INVALID` for argument problems.

- [ ] **Step 1: Write the failing CLI test first**

In `apps/cli/tests/execution_cli.rs`, modelled on `event_store_cli.rs`'s process-spawning pattern: `execution start --file <graph> --events <dir> --fixtures <fixtures> --mode supervised --execution <id>` on a two-node fixture graph must exit 0 and print a JSON object with `ok: true`, `command: "execution.start"`, and a `data` payload reporting the final aggregate status and per-state node counts; a following `execution status --events <dir> --execution <id>` must report the same state independently. Use the repository's existing example graph fixtures (`examples/` or the conformance graph the simulate tests use — read what `cli_smoke.rs` drives and reuse it).

- [ ] **Step 2: The driver**

`driver.rs` exposes one function used by `start` and `resume`:

```rust
pub(super) fn drive_to_quiescence(
    store: &LocalEventRepository,
    scope: &RepositoryScope,
    stream: &str,
    spec: &GraphSpec,
    fixtures: &SimulationFixtures,
    actor: &PersistedActor,
) -> Result<ExecutionProjection, Failure>
```

Loop, re-reading the projection each iteration (replay through the store — the driver has no other source of truth, exactly like the lifecycle test):

1. Approve untouched (`Draft`) nodes to `Ready` (the legal route; append the `Approved` outcome).
2. `ready_set` → on `ReadySetTooLarge`, `GHCLI005` with the blocking reason.
3. `dispatch_plan` with `max_parallel` from `spec.budgets.max_parallel_model_calls` (default 1 when absent; `ZeroParallelism` → `GHCLI005`).
4. Per dispatched node: `classify_progress` first — `AttemptsExhausted`/`NoProgress` append the blocking outcome (`RetryableFailure` with `next_state` from `apply_transition`, which blocks at the bounds) rather than dispatching; otherwise the two `Started` hops, then the executor's outcome, every `next_state` from `apply_transition`.
5. Quiesce when no node was dispatched in a full pass. If every node is terminal, append `execution_completed` (`Completed` when all `Succeeded | Waived | Skipped`, else `Failed`) — unless the aggregate status is already terminal.

Every append goes through the same idempotency-key discipline `simulate.rs` uses (`UuidIds`), and every envelope through the store's normal append path so hash chaining and schema validation are the production ones.

- [ ] **Step 3: `start` and `status`**

`start`: load + lint + `publish_loaded` (identical to `simulate.rs`), append `execution_started` with the graph version, hash and `--mode`, then `drive_to_quiescence`, then report. Refuse a stream that already has an `execution_started` (`GHCLI005` — the fold would call it corrupt; the CLI says it first).
`status`: replay only; report execution id, mode, aggregate status, per-state node counts, `signals_recorded`, `accepted_mutations`, and the untriaged-interruption list from Task 2's rule — this is the operator's triage view.

- [ ] **Step 4: Run the CLI test to green, then the workspace suite.** Quote both.

- [ ] **Step 5: Commit**

```bash
git add apps/cli
git commit -m "feat(cli): add the execution driver with start and status"
```

---

### Task 4: `signal` and `approve`

**Files:**
- Create: `apps/cli/src/commands/execution/signal.rs`, `approve.rs`
- Test: `apps/cli/tests/execution_cli.rs`

- [ ] **Step 1: Failing tests first**, three scenarios:
  1. A valid signal envelope file: `execution signal --events <dir> --execution <id> --signal <file> --evidence-out <path>` → `ok: true`, the raw envelope bytes written at `--evidence-out`, `signals_recorded` incremented (verify via `status`), and the `data` payload carrying `decide_mutation`'s verdict for the mode in force (`requires_approval` under Supervised).
  2. A garbage envelope → exit non-zero, `GHCLI003_SIGNAL_INVALID`, nothing written, nothing appended.
  3. A schema-valid envelope whose `id` is empty → `GHCLI004_SIGNAL_UNRECORDABLE`, the envelope **is** written to `--evidence-out`, nothing appended, and the diagnostic message says the evidence was preserved.

- [ ] **Step 2: Implement `signal`.** `admit_signal` from the replayed projection; on success write `admitted.externalize` to `--evidence-out` (fail closed if the write fails — do not append an event whose evidence was not preserved), then append `signal_recorded` from `admitted.record`, then report `decide_mutation`'s verdict as data. On `SignalBudgetExhausted`: `GHCLI005`, and the message states the execution must be blocked for an owner decision.

The evidence path is a file, not the encrypted Evidence store: the sealed-provider externalization pipeline expects the Governor's content slots, and forcing a signal envelope through it would invent a content position 04d never defined. The event's `envelope_sha256` still binds the record to those exact bytes. State this limit in the command's `--help` and the milestone doc: operator-grade encrypted externalization of signal envelopes is Milestone 05 work.

- [ ] **Step 3: Implement `approve`.** `execution approve --events <dir> --execution <id> --node <id>`: replay, and the node must be `Ghost` or `Blocked` (anything else: `GHCLI005` naming the state). For `Blocked + last_outcome Interrupted` this *is* the triage act. Append `node_outcome_recorded { Approved, next_state: apply_transition(...) }`. Do **not** auto-drive afterwards: D-020's rule — nothing auto-starts out of a manual intervention; the owner runs `resume` (or `start`ed executions quiesce again on the next `resume`). Test: approve a ghost (from a governance fixture stream) and a blocked node, assert both reach `Ready` and nothing else changed.

- [ ] **Step 4: Sabotage** — make `signal` append before writing the evidence file and point `--evidence-out` at an unwritable path; the scenario-3-adjacent test must fail (an event without preserved evidence). Revert from a backup, re-confirm. Quote both.

- [ ] **Step 5: Commit**

```bash
git add apps/cli
git commit -m "feat(cli): add signal intake and owner approval"
```

---

### Task 5: `pause`, `resume`, `cancel`

**Files:**
- Create: `apps/cli/src/commands/execution/pause.rs`, `resume.rs`, `cancel.rs`
- Test: `apps/cli/tests/execution_cli.rs`

- [ ] **Step 1: Failing tests first**, driving the 04e story through the CLI: start (Supervised, fixtures leaving `b` ready), `pause` → status `Paused` with the held nodes `Paused`; `resume` → runs to completion; `cancel` on a fresh execution → every non-terminal node `Cancelled`, then `execution_completed { cancelled }`, and a second `cancel` refused (`GHCLI005`, already terminal).

- [ ] **Step 2: Implement, honouring the written contracts:**
  - `pause`: refuse unless aggregate status is `None | Running` (the fold's own guard, said first by the CLI); append `execution_paused`, then `Paused` outcomes for every `Ready`/`Queued` node. Record in `data` exactly which nodes were held — the resume driver's own list.
  - `resume`: `recovery_plan` first — any `Running` node gets `Interrupted -> Blocked` appended (crash triage on entry, the pause-recover-approve order from 04e); then `resume_preconditions` (now with `UntriagedInterruption`) — refusals map to `GHCLI005` with the variant name; then `execution_resumed`, `Started` hops for exactly the nodes whose state is `Paused` (not waiting nodes — 04e finding 4), then `drive_to_quiescence`.
  - `cancel`: refuse when already terminal; append `Cancelled` for every non-terminal node (legal from any non-terminal state), then `execution_completed { Cancelled }`.

- [ ] **Step 3: Sabotage** — make `resume` also emit `Started` for `WaitingInput` nodes (the blind-redispatch bug 04e named); a test with a waiting node must fail. Revert from a backup, re-confirm. Quote both.

- [ ] **Step 4: Commit**

```bash
git add apps/cli
git commit -m "feat(cli): add pause, resume and cancel"
```

---

### Task 6: The operator story, replayed, and the gate stage

**Files:**
- Modify: `apps/cli/tests/execution_cli.rs`, `ci/gate.ps1`

- [ ] **Step 1: The end-to-end test.** One test drives the full story through the *binary* (not library calls): start → signal (Supervised: `requires_approval`) → pause → resume → completion, then runs `replay` (the existing `graph replay` command or direct library call, whichever `event_store_cli.rs` precedent uses) twice over the stream and asserts identical output — the operator-visible version of the milestone's replay guarantee. Also assert every command's stdout parses as the standard envelope (`ok`, `command`, `data`, `diagnostics`).

- [ ] **Step 2: The gate stage.** `ci/gate.ps1` lists CLI test binaries explicitly (`cli: cli_smoke`, `cli: schema_cli`, `cli: event_store_cli`) — a new binary is silently excluded until added. Add `cli: execution_cli` alongside them, same invocation shape. Prove the stage can fail: transiently break one assertion, run the gate's CLI stage (not the whole gate), watch it go red, revert from a backup. Quote both.

- [ ] **Step 3: Commit**

```bash
git add apps/cli ci/gate.ps1
git commit -m "test(cli): drive the operator story end to end and gate it"
```

---

### Task 7: Milestone closure — acceptance map, docs, full gate

**Files:**
- Modify: `docs/milestones/graph-engine-governor.md`, `CHANGELOG.md`, `docs/product/ROADMAP_AND_ACCEPTANCE.md` (only if it tracks milestone status — check first)

- [ ] **Step 1: The acceptance map.** A closing section in the milestone document tabulating all eight §8 criteria against their evidence — each row naming the test or command that demonstrates it, none claiming more than the tests show. The known honest gaps go in the same table, not a footnote: signal envelopes externalize to operator files rather than encrypted Evidence; signal-to-draft translation is undesigned; ghost births unbudgeted (accepted risk, rationale recorded); `simulate()`/executor divergence reclassified as intentional with decision 5's reasoning.

- [ ] **Step 2: Update the 04e findings section** — findings 1 and 3 are now resolved (Tasks 1 and 2); rewrite them as resolved-with-pointers rather than deleting them, so the history of the finding survives. Finding 2 (the sequencing order) is now *enforced by the driver*, cite `resume.rs`. Finding 4 likewise. Update the header status line. **Re-read the entire file.**

- [ ] **Step 3: CHANGELOG** entry in the established voice, and the milestone status wherever else it is tracked.

- [ ] **Step 4: Run the full gate.** `./ci/gate.ps1` with `GRAPHHELM_PG_BIN` set. Expected: `[gate] GREEN - every stage passed.` — now including `cli: execution_cli`. A red on the tracked flakes (#19) is re-run once with the flake noted; any other red is a defect.

- [ ] **Step 5: Commit**

```bash
git add docs CHANGELOG.md
git commit -m "docs(m04): close Milestone 04 with the acceptance map"
```

---

## Definition of done

- The driver drives a published graph to completion through the real store, and the operator story — start, signal, approve, pause, resume, cancel, status — runs end to end through the binary with JSON-only output and redaction-safe failures.
- `MAX_IDENTICAL_OUTCOMES` fires for a retry loop, proven by the reworked lifecycle detour test.
- Resume refuses untriaged interruptions and only them; approval is the triage act; nothing auto-starts out of a manual intervention.
- An unrecordable signal preserves its evidence and says so; a garbage one stores nothing.
- Resume re-dispatches exactly the nodes the pause held; the blind-redispatch sabotage fails the suite.
- `execution_cli` runs in the gate, proven able to fail.
- The acceptance map covers all eight §8 criteria with named evidence and named gaps.
- Every new guard observed failing once, deliberately; `./ci/gate.ps1` green, PostgreSQL matrix included.

## What this plan deliberately excludes

Real model/tool calls, sandboxes, encrypted externalization of signal envelopes, per-edge-type readiness, immediate-stop and branch pause, compensation, leases — Milestone 05, per the design's boundary. Signal-to-draft translation (undesigned; the governor decides *whether*, not *what*). The PostgreSQL execution-command path: the local repository is the milestone's driver substrate, and the operator commands say so — the adapter serves the events/backup surface it was built for.
