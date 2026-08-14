use std::path::Path;

use graphhelm_execution::{ResumeError, recovery_plan, resume_preconditions};
use graphhelm_graph::GraphVersion;
use graphhelm_protocols::{
    EventKind, ExecutionResumed, NewEvent, NodeOutcome, NodeState, OpaqueId, PersistedActor,
    Sensitivity,
};

use super::driver::drive_to_quiescence;
use super::{
    Failure, append_event, execution_state, finish, idempotency_key, load_fixtures, owner_actor,
    record_outcome, render, replay_failure, replay_projection, repository_failure, resolve_stream,
    system_actor,
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
    let store = event_store(events).map_err(|error| repository_failure(&error))?;
    let fixtures = load_fixtures(fixtures)?;
    let (scope, stream, history) = resolve_stream(&store, execution)?;
    let stream_id = OpaqueId::parse(&stream)
        .map_err(|_| execution_state("the stream identifier is not wire-safe", "/execution"))?;
    let initial = graphhelm_events::replay(&scope, &stream, &history)
        .map_err(|error| replay_failure(&error))?;

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
                NodeOutcome::Interrupted,
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
            NodeOutcome::Started,
        )?;
    }

    // The resume decision and its Started redispatches are the owner's acts; the drive that
    // follows is the driver's own bookkeeping and stays under the system actor, so the log can
    // tell sovereignty from machinery.
    let projection = drive_to_quiescence(
        &store,
        &scope,
        &stream,
        &version.graph().spec,
        &fixtures,
        &system_actor(),
    )?;
    Ok(render(&projection))
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
