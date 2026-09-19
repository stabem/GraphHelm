use std::path::Path;

use graphhelm_protocols::{
    EventKind, ExecutionPaused, NewEvent, NodeOutcome, NodeState, OpaqueId, PersistedActor,
    Sensitivity, SimulationStatus,
};

use super::{
    Failure, RecordedOutcome, append_event, execution_state, finish, idempotency_key,
    load_projection, owner_actor, record_outcome, render, replay_projection, repository_failure,
    simulation_status_label,
};
use crate::commands::event_store;
use crate::output::Outcome;

const COMMAND: &str = "execution.pause";

/// Holds every dispatchable node (`Ready`/`Queued`), refusing unless the aggregate status is
/// `None` or `Running` — said here, before the fold's own `ExecutionPaused` guard would call a
/// second pause corrupt.
///
/// Calls `execute` with the owner actor and a fresh per-invocation idempotency key, exactly as
/// before Milestone 05a Task 4 — byte-identical CLI behaviour (the same pattern Task 3 established
/// for `signal`/`approve`).
pub fn run(events: &Path, execution: Option<&str>) -> Outcome {
    finish(
        COMMAND,
        execute(
            events,
            execution,
            owner_actor(),
            idempotency_key("execution-paused"),
        ),
        |value| value,
    )
}

/// Widened from private to `pub(crate)` (Milestone 05a Task 4), gaining `actor` and `key` as
/// explicit parameters — the mechanical widening the plan's file table names, exactly as Task 3
/// did for `signal`/`approve`.
///
/// `key` is used for exactly the one event that identifies "this pause command happened":
/// `ExecutionPaused`. The per-node `Paused` holds below still mint their own fresh keys through
/// `record_outcome` (unchanged from before this task) because their count varies with however many
/// nodes are `Ready`/`Queued` at the moment of the call — a variable-count fan-out cannot derive
/// deterministic keys from a fixed per-command suffix the way a single, always-present event can.
/// This is not a gap in the idempotent-retry guarantee: `run_idempotent_mutation`'s pre-flight
/// check classifies purely on `ExecutionPaused`'s derived key, and a retry it classifies as
/// `Complete` never calls this function a second time (see `serve::mod::run_idempotent_mutation`),
/// so the fan-out's fresh keys are never at risk of a double-apply from a caller's retry — they
/// only need to be valid, non-colliding keys for the one genuinely fresh attempt that reaches them.
/// They are still attributed to the caller's `actor`, matching this file's pre-existing behaviour
/// (holding a node is a direct, deterministic, non-branching consequence of the pause decision
/// itself, not the driver's own bookkeeping — unlike `drive_to_quiescence`'s hops, nothing here
/// calls an executor or branches on its outcome).
pub(crate) fn execute(
    events: &Path,
    execution: Option<&str>,
    actor: PersistedActor,
    key: OpaqueId,
) -> Result<serde_json::Value, Failure> {
    let store = event_store(events).map_err(|error| repository_failure(&error))?;
    let (scope, stream, projection) = load_projection(&store, execution)?;

    let execution_id = projection
        .execution_id
        .clone()
        .ok_or_else(|| execution_state("no execution has started on this stream", "/execution"))?;
    if !matches!(
        projection.simulation_status,
        None | Some(SimulationStatus::Running)
    ) {
        return Err(execution_state(
            &format!(
                "the execution is {}, not none or running",
                simulation_status_label(projection.simulation_status.as_ref())
            ),
            "/execution",
        ));
    }

    let stream_id = OpaqueId::parse(&stream)
        .map_err(|_| execution_state("the stream identifier is not wire-safe", "/execution"))?;
    let execution_id = OpaqueId::parse(&execution_id)
        .map_err(|_| execution_state("the execution identifier is not wire-safe", "/execution"))?;

    append_event(
        &store,
        &scope,
        &stream_id,
        NewEvent::new(
            key,
            actor.clone(),
            Sensitivity::Internal,
            EventKind::ExecutionPaused(ExecutionPaused {
                execution_id: execution_id.clone(),
            }),
            vec![],
            vec![],
        ),
    )?;

    // Held exactly here, from the projection as read before the pause itself: every node in bare
    // state `Ready` or `Queued` right now.
    //
    // SCOPE OF THE GUARANTEE BELOW, corrected in #80 — it protects against a stale LIST, not
    // against an over-broad STATE. `resume` re-derives its own list independently rather than
    // reading this one back, so a node this pause missed cannot be re-dispatched by trusting a
    // stale record of what was held. That was true and it was not enough: this filter tests BARE
    // STATE, so it also holds a node that is `Ready` but still edge-gated behind an unfinished
    // predecessor — one the driver would never have dispatched. `resume` then force-records
    // `Started` for it, `(Paused, Started) => Queued` puts it in the retry chain, and before #80's
    // gate the driver dispatched it.
    //
    // The dispatch consequence is closed at the driver
    // (`graphhelm_execution::dispatch_candidates`), so what remains here is a RECORD-ACCURACY
    // defect rather than a behavioural one: this list can claim to have held a node that was never
    // going anywhere, which makes a stream harder to read back as an incident. Narrowing it is
    // deliberately deferred and tracked separately — the fix belongs where dispatch is decided,
    // and widening this diff to also change what gets recorded would mix the two.
    let held: Vec<String> = projection
        .node_states
        .iter()
        .filter(|(_, state)| matches!(state, NodeState::Ready | NodeState::Queued))
        .map(|(node, _)| node.clone())
        .collect();
    for node in &held {
        record_outcome(
            &store,
            &scope,
            &stream_id,
            &execution_id,
            &actor,
            node,
            RecordedOutcome::uncaused(NodeOutcome::Paused),
        )?;
    }

    let projection = replay_projection(&store, &scope, &stream)?;
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
        // Same posture for the instants: a mutation reply publishes null rather than a
        // stillness it never looked for.
        &super::Liveness::default(),
        None,
    );
    if let serde_json::Value::Object(ref mut map) = data {
        map.insert("heldNodes".to_owned(), serde_json::json!(held));
    }
    Ok(data)
}
