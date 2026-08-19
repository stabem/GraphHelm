use std::path::Path;

use graphhelm_execution::{ResumeError, recovery_plan, resume_preconditions};
use graphhelm_graph::GraphVersion;
use graphhelm_protocols::{
    EventKind, ExecutionResumed, NewEvent, NodeOutcome, NodeState, OpaqueId, PersistedActor,
    Sensitivity,
};

use super::driver::drive_to_quiescence;
use super::{
    Failure, PreparedDrive, RecordedOutcome, append_event, execution_state, finish,
    idempotency_key, load_fixtures, owner_actor, record_outcome, render, replay_failure,
    replay_projection, repository_failure, resolve_stream, system_actor,
};
use crate::commands::{event_store, owner, publish_loaded};
use crate::output::Outcome;

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
    let version = match publish_loaded(&loaded, owner("owner-local")) {
        Ok(version) => version,
        Err(error) => return Outcome::internal(COMMAND, error),
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
        &prepared.fixtures,
        &system_actor(),
    )?;
    Ok(render(
        &projection,
        // Nothing measured here on purpose: this command reports the mutation it just made,
        // not a liveness reading. The seam turns "not measured" into `silenceUnevaluated`
        // rather than into calm, so the omission is stated instead of implied.
        &graphhelm_execution::AttentionInputs::default(),
        // Same posture for the instants: a mutation reply publishes null rather than a
        // stillness it never looked for.
        &super::Liveness::default(),
    ))
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
    // untouched. An execution with no published graph cannot resume against any file.
    let supplied_hash = graphhelm_protocols::WireHash::parse(version.content_hash().as_str())
        .map_err(|_| {
            execution_state(
                "the supplied graph's hash is not wire-safe",
                "/execution/graph",
            )
        })?;
    // Discrepancy vs the plan, reported: `current_graph` is populated by the M03-era
    // graph-publication event class, which the CLI's own `execution start` never appends —
    // "refuse None" would refuse every CLI resume. The CLI path's published identity is the
    // `graph_hash` the `execution_started`/`execution_resumed` payloads record, so the check
    // honors `current_graph` when a publication event exists and otherwise falls back to the
    // LAST recorded graph hash in the stream's own history. Only an execution with neither —
    // no publication and no recorded start — refuses outright.
    let recorded_hash = initial
        .current_graph
        .as_ref()
        .map(|published| published.semantic_hash().clone())
        .or_else(|| {
            history.iter().rev().find_map(|event| match &event.kind {
                EventKind::ExecutionStarted(payload) => Some(payload.graph_hash.clone()),
                _ => None,
            })
        });
    match recorded_hash {
        None => {
            return Err(execution_state(
                "resume refused: the execution has no recorded graph to check against",
                "/execution/graph",
            ));
        }
        Some(recorded) if recorded != supplied_hash => {
            return Err(execution_state(
                "resume refused: the supplied graph file does not match the graph this execution started from",
                "/execution/graph",
            ));
        }
        Some(_) => {}
    }

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
    let paused_nodes: Vec<String> = projection
        .node_states
        .iter()
        .filter(|(_, state)| **state == NodeState::Paused)
        .map(|(node, _)| node.clone())
        .collect();
    for node in &paused_nodes {
        record_outcome(
            &store,
            &scope,
            &stream_id,
            &execution_id,
            &actor,
            node,
            RecordedOutcome::uncaused(NodeOutcome::Started),
        )?;
    }

    // The resume decision and its Started redispatches are the owner's acts; the drive that
    // follows (sync, in `execute` above, or async over HTTP) is the driver's own bookkeeping and
    // stays under the system actor, so the log can tell sovereignty from machinery.
    Ok(PreparedDrive {
        scope,
        stream: stream_id,
        execution_id,
        spec: version.graph().spec.clone(),
        fixtures,
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
