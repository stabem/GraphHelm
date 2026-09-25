use std::path::Path;

use graphhelm_execution::{ResumeError, recovery_plan, resume_preconditions};
use graphhelm_graph::GraphVersion;
use graphhelm_protocols::{
    EventKind, ExecutionResumed, NewEvent, NodeOutcome, NodeState, OpaqueId, PersistedActor,
    Sensitivity,
};

use super::driver::{Release, drive_to_quiescence};
use super::{
    Failure, PreparedDrive, RecordedOutcome, append_event, execution_state, finish,
    idempotency_key, load_fixtures, owner_actor, record_outcome, render, replay_failure,
    replay_projection, repository_failure, resolve_stream, system_actor,
    verify_graph_matches_execution,
};
use crate::commands::{event_store, owner, publish_loaded};
use crate::output::Outcome;
use graphhelm_simulation::FixtureExecutor;

const COMMAND: &str = "execution.resume";

/// Recovers any crashed node, gates on the resume preconditions, re-dispatches exactly the nodes
/// the pause held, and drives to quiescence again.
///
/// Calls `execute` with the owner actor and a fresh per-invocation idempotency key, exactly as
/// before Milestone 05a Task 4 — byte-identical CLI behaviour (the same pattern Task 3 established
/// for `signal`/`approve`).
pub fn run(
    file: &Path,
    events: &Path,
    fixtures: Option<&Path>,
    execution: Option<&str>,
) -> Outcome {
    let loaded = match graphhelm_schema::load_graph(file) {
        Ok(loaded) => loaded,
        Err(diagnostics) => return Outcome::domain(COMMAND, diagnostics),
    };
    let report = graphhelm_graph::lint(&loaded.graph, &loaded.source);
    if !report.errors.is_empty() {
        let mut diagnostics = report.errors;
        diagnostics.extend(report.warnings);
        return Outcome::domain(COMMAND, diagnostics);
    }
    // #192: a warning-only lint pass reaches every exit past this point, not only the success
    // one — a caller whose publish then failed already knew about the lint warnings, and the
    // reply must not look like it withheld something it already computed.
    let warnings = report.warnings;
    let version = match publish_loaded(&loaded, owner("owner-local")) {
        Ok(version) => version,
        Err(error) => return Outcome::internal(COMMAND, error).with_warnings(warnings),
    };
    finish(
        COMMAND,
        execute(
            &version,
            events,
            fixtures,
            execution,
            owner_actor(),
            idempotency_key("execution-resumed"),
        ),
        |value| value,
    )
    .with_warnings(warnings)
}

/// Widened from private to `pub(crate)` (Milestone 05a Task 4), gaining `actor` and `key` as
/// explicit parameters, exactly as Task 3 did for `signal`/`approve`.
///
/// `key` is used for exactly the one event that identifies "this resume command happened":
/// `ExecutionResumed`. The crash-recovery `Interrupted` records and the paused-node `Started`
/// redispatches below still mint their own fresh keys through `record_outcome` (unchanged) for the
/// same variable-count reasoning `pause::execute`'s doc comment gives — both are attributed to
/// `actor` (the owner's/caller's decision to resume implies triaging and redispatching), matching
/// this file's pre-existing behaviour.
///
/// The drive that follows (`drive_to_quiescence`) is a separate matter and was *already* split
/// from the owner's decision before this task (the 04f actor split this file's own comment below
/// names): it is called with a fresh `system_actor()`, never with `actor`, so the driver's own hops
/// stay attributed to the system regardless of who invoked resume over the API. This task preserves
/// that split unchanged — only the decision-side `actor`/`key` moved from being minted internally
/// to being accepted as parameters.
pub(crate) fn execute(
    version: &GraphVersion,
    events: &Path,
    fixtures: Option<&Path>,
    execution: Option<&str>,
    actor: PersistedActor,
    key: OpaqueId,
) -> Result<serde_json::Value, Failure> {
    let prepared = execute_prepared(version, events, fixtures, execution, actor, key)?;
    let store = event_store(events).map_err(|error| repository_failure(&error))?;
    let projection = drive_to_quiescence(
        &store,
        &prepared.scope,
        prepared.stream.as_str(),
        &prepared.spec,
        &FixtureExecutor::new(prepared.fixtures.clone()),
        &system_actor(),
        &Release {
            nodes: &prepared.release,
            actor: &owner_actor(),
        },
    )?;
    let mut data = render(
        &projection,
        // Nothing measured here on purpose: this command reports the mutation it just made,
        // not a liveness reading. The seam turns "not measured" into `silenceUnevaluated`
        // rather than into calm, so the omission is stated instead of implied.
        // But the BUDGET is not a measurement. It is declared in the graph this projection
        // already holds, and `default()` asserted there was none -- so `attention` took the
        // `(None, measured)` arm and answered `NoDeclaredBudget` with the remedy "declare a
        // budget for this node", to an operator who had declared one (G's measurement on
        // #1013). `for_surface` derives it from the projection; the empty map is the AGE,
        // which really is unmeasured here, and that lands on the honest `(Some, None)` arm:
        // `NotMeasured`, remedy `Unavailable { SurfaceMeasuredNoAge }`.
        &graphhelm_execution::AttentionInputs::for_surface(
            &projection,
            std::collections::BTreeMap::new(),
            None,
        ),
        // Preserve the main-branch liveness contract: the mutation did not measure node silence,
        // but the event store can still report the instant at which this resume moved it.
        &super::Liveness::from_store(&store, &prepared.scope, prepared.stream.as_str()),
        Some(&prepared.spec),
    );

    // #123's REQUIRED MITIGATION, and it is what pays for overloading `Paused` with a second
    // cause. A node this resume named but the drive could not release stays `Paused` — and the
    // NEXT pause cannot re-hold it, because pause filters on `Ready | Queued`, so it will never
    // appear in `heldNodes` again. Without this field the operator would have NO surface anywhere
    // that mentions the node at all: the resume they just ran silently declined to start it and
    // said nothing. The state alone cannot carry the distinction between "the owner paused this"
    // and "its dependencies are still unmet", so the response carries it instead — additive on an
    // envelope that already exists, costing no schema vocabulary.
    let withheld: Vec<String> = prepared
        .release
        .iter()
        .filter(|node| {
            projection.node_states.get(node.as_str()).copied() == Some(NodeState::Paused)
        })
        .cloned()
        .collect();
    if let serde_json::Value::Object(ref mut map) = data {
        map.insert("withheldNodes".to_owned(), serde_json::json!(withheld));
        map.insert(
            "withheldReason".to_owned(),
            serde_json::json!("edges_unsatisfied"),
        );
    }
    Ok(data)
}

/// The decision half of `execute` (Milestone 05d Task 9's `execute_prepared` split): the hash
/// cross-check, crash-recovery appends, `resume_preconditions` gate, the `ExecutionResumed`
/// append, and the paused-node `Started` redispatches — everything through the point 04e's
/// actor split already separated from the drive. `execute` above is exactly `execute_prepared`
/// plus the same sync drive and render as before this split: CLI behavior is byte-identical.
pub(crate) fn execute_prepared(
    version: &GraphVersion,
    events: &Path,
    fixtures: Option<&Path>,
    execution: Option<&str>,
    actor: PersistedActor,
    key: OpaqueId,
) -> Result<PreparedDrive, Failure> {
    let store = event_store(events).map_err(|error| repository_failure(&error))?;
    let fixtures = load_fixtures(fixtures)?;
    let (scope, stream, history) = resolve_stream(&store, execution)?;
    let stream_id = OpaqueId::parse(&stream)
        .map_err(|_| execution_state("the stream identifier is not wire-safe", "/execution"))?;
    let initial = graphhelm_events::replay(&scope, &stream, &history)
        .map_err(|error| replay_failure(&error))?;

    // The file-trust seam (the M04 ledger's own words: "an operator can resume against the
    // wrong graph file and the driver will believe them" — closed here, 05d Task 7): the
    // supplied file's derived content hash must match the published graph this execution
    // started from, checked BEFORE any recovery append so a refused resume leaves the store
    // untouched. The check itself moved to `verify_graph_matches_execution` (#159) when `claim`
    // and `clear` started needing the same seam; its messages are unchanged.
    verify_graph_matches_execution(version, &initial, &history, "resume")?;

    // Crash triage on entry (the pause-recover-approve order 04e settled): every node still
    // `Running` when the execution stopped has unknown effects. `recovery_plan` names them, and
    // each gets `Interrupted -> Blocked` before anything else, so `resume_preconditions` below can
    // see — and refuse — any that are still untriaged.
    if let Some(execution_id) = initial.execution_id.as_deref() {
        let execution_id = OpaqueId::parse(execution_id).map_err(|_| {
            execution_state("the execution identifier is not wire-safe", "/execution")
        })?;
        for node in recovery_plan(&initial) {
            record_outcome(
                &store,
                &scope,
                &stream_id,
                &execution_id,
                &actor,
                &node,
                // `Interrupted` is its own cause — the triage rule reads that outcome
                // directly, so restating it here would be noise (M07 F3).
                RecordedOutcome::uncaused(NodeOutcome::Interrupted),
            )?;
        }
    }

    let projection = replay_projection(&store, &scope, &stream)?;
    resume_preconditions(&projection, Some(version.number())).map_err(|error| {
        execution_state(
            &format!("resume refused: {}", resume_error_code(error)),
            "/execution",
        )
    })?;
    let execution_id = OpaqueId::parse(
        projection
            .execution_id
            .as_deref()
            .expect("resume_preconditions guarantees an execution id on success"),
    )
    .map_err(|_| execution_state("the execution identifier is not wire-safe", "/execution"))?;

    append_event(
        &store,
        &scope,
        &stream_id,
        NewEvent::new(
            key,
            actor.clone(),
            Sensitivity::Internal,
            EventKind::ExecutionResumed(ExecutionResumed {
                execution_id: execution_id.clone(),
            }),
            vec![],
            vec![],
        ),
    )?;

    // Exactly the nodes the pause held (`Paused`) — never a `WaitingInput`/`WaitingCapacity` node,
    // which is legitimately still waiting on something that has not changed. Forcing a `Started`
    // hop for one of those would be the blind-redispatch bug 04e named.
    //
    // 04e's door list was not complete, and #80 is the door it missed. This `Started` does NOT run
    // the node: `(Paused, Started) => Queued` (`core/execution/src/transition.rs:109`) puts it in
    // the driver's retry chain, and that chain used to be a bare `state == Queued` filter with no
    // edge check — so a node whose predecessor had never finished could reach dispatch one hop
    // after resume. The gate now lives where dispatch is DECIDED
    // (`graphhelm_execution::dispatch_candidates`), not here, deliberately: a node can reach
    // `Queued` by routes this function knows nothing about — an ordinary retry, or a predecessor
    // invalidated after the fact — and a filter here would cover only the one route it can see.
    // Re-checking readiness HERE would also be wrong in the opposite direction: `is_dispatchable`
    // is `Ready`-only, so a `Paused` node is never in `ready_set` by construction, and filtering
    // this list by it would strand every paused node permanently.
    // #123: NAMED HERE, RELEASED BY THE DRIVE. This used to force-record `Started` for every held
    // node right now, which is what made #80's gate expensive: a node whose edges are still unmet
    // went `Paused -> Queued`, the next `pause` re-held it on BARE STATE (`Ready | Queued`,
    // `pause.rs:124`), the next resume re-started it, and the pair appended two events per round
    // forever with no terminal state to stop it. Measured on this graph: 2 events/round before
    // #80's gate, 4 after, and each round costs more than the last because every append lengthens
    // the journal every open must load.
    //
    // Filtering by edges HERE does not work and the reason is timing, not predicate: after an
    // `approve` the predecessor is `Ready`, not `Succeeded` (`transition.rs:79`), so its edges are
    // unsatisfied at THIS instant and satisfied only after the drive runs it. A filter here would
    // leave the node `Paused` with nothing to ever dispatch it — the release would never happen.
    // So the list travels to the drive, which re-evaluates every pass.
    let paused_nodes: Vec<String> = projection
        .node_states
        .iter()
        .filter(|(_, state)| **state == NodeState::Paused)
        .map(|(node, _)| node.clone())
        .collect();

    // The resume decision and its Started redispatches are the owner's acts; the drive that
    // follows (sync, in `execute` above, or async over HTTP) is the driver's own bookkeeping and
    // stays under the system actor, so the log can tell sovereignty from machinery.
    Ok(PreparedDrive {
        scope,
        stream: stream_id,
        execution_id,
        spec: version.graph().spec.clone(),
        fixtures,
        release: paused_nodes.into_iter().collect(),
    })
}

/// `resume_preconditions`'s refusal, named in snake_case matching this module's own wire
/// vocabulary — the plan's "GHCLI005 with the variant name".
fn resume_error_code(error: ResumeError) -> &'static str {
    match error {
        ResumeError::NotStarted => "not_started",
        ResumeError::NotPaused => "not_paused",
        ResumeError::UnrecoveredInterruption => "unrecovered_interruption",
        ResumeError::UntriagedInterruption => "untriaged_interruption",
        ResumeError::VersionMismatch => "version_mismatch",
    }
}
