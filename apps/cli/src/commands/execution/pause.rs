use std::path::Path;

use graphhelm_protocols::{
    EventKind, ExecutionPaused, NewEvent, NodeOutcome, NodeState, OpaqueId, PersistedActor,
    Sensitivity, SimulationStatus,
};

use graphhelm_graph::GraphVersion;

use super::{
    Failure, RecordedOutcome, append_event, execution_state, finish, idempotency_key, owner_actor,
    record_outcome, render, replay_failure, replay_projection, repository_failure, resolve_stream,
    simulation_status_label, verify_graph_matches_execution,
};
use crate::commands::{event_store, owner, publish_loaded};
use crate::output::Outcome;

const COMMAND: &str = "execution.pause";

/// Holds every node the EDGES leave eligible for dispatch, refusing unless the aggregate status is
/// `None` or `Running` — said here, before the fold's own `ExecutionPaused` guard would call a
/// second pause corrupt.
///
/// `file` is optional and supplies the edges (#157). With it, the hold is the driver's own
/// EDGE-ELIGIBLE candidate set; without it, the hold falls back to bare state and the reply says
/// so.
///
/// WHAT "CANDIDATE" MEANS HERE, narrowed by review: `dispatch_candidates` answers the EDGE
/// question only, upstream of capacity planning — its own doc comment records that its bound
/// covers the ready half alone, and concurrency is decided later, by the driver, per pass. So a
/// node in this hold is one the edges do not gate, NOT one this command predicts would run
/// next.
///
/// Calls `execute` with the owner actor and a fresh per-invocation idempotency key.
pub fn run(events: &Path, execution: Option<&str>, file: Option<&Path>) -> Outcome {
    let mut warnings = Vec::new();
    let version = match file {
        None => None,
        Some(file) => {
            // The same three steps `resume::run` takes for its own `--file`, in the same order:
            // load, lint, publish. A graph that does not lint cannot be trusted to answer an
            // edge question, and nothing is appended before it does.
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
            warnings = report.warnings;
            match publish_loaded(&loaded, owner("owner-local")) {
                Ok(version) => Some(version),
                Err(error) => {
                    return Outcome::internal(COMMAND, error).with_warnings(warnings);
                }
            }
        }
    };
    finish(
        COMMAND,
        execute(
            events,
            execution,
            owner_actor(),
            idempotency_key("execution-paused"),
            version.as_ref(),
        ),
        |value| value,
    )
    .with_warnings(warnings)
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
    graph: Option<&GraphVersion>,
) -> Result<serde_json::Value, Failure> {
    let store = event_store(events).map_err(|error| repository_failure(&error))?;
    // `resolve_stream` rather than `load_projection` because the file-trust seam below needs the
    // HISTORY as well as the fold: the recorded graph hash lives in the `ExecutionStarted`
    // payload, which the fold does not keep.
    let (scope, stream, history) = resolve_stream(&store, execution)?;
    let projection = graphhelm_events::replay(&scope, &stream, &history)
        .map_err(|error| replay_failure(&error))?;

    // Checked BEFORE any append, exactly as `resume`/`claim`/`clear` check it, so a pause handed
    // the wrong graph leaves the store untouched instead of holding nodes against edges this
    // execution never had.
    let spec = match graph {
        None => None,
        Some(version) => {
            verify_graph_matches_execution(version, &projection, &history, "pause")?;
            Some(&version.graph().spec)
        }
    };

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

    // Held exactly here, from the projection as read before the pause itself.
    //
    // SCOPE OF THE GUARANTEE, corrected in #80 — it protects against a stale LIST, not against an
    // over-broad STATE. `resume` re-derives its own list independently rather than reading this
    // one back, so a node this pause missed cannot be re-dispatched by trusting a stale record of
    // what was held. That was true and it was not enough: the filter used to test BARE STATE
    // (`Ready | Queued`), so it also held a node that is `Ready` but edge-gated behind an
    // unfinished predecessor — one the driver would never have dispatched.
    //
    // #157 narrows that here — for THIS reply, the response an operator reads. It does not
    // rewrite what the fallback path persists: see the limit stated at the end of this comment.
    // The dispatch consequence was already closed at the driver
    // (`dispatch_candidates`), so what was left was a RECORD-ACCURACY defect: the list claimed to
    // have held a node that was never going anywhere, which makes a stream unreadable as an
    // incident. The narrowing asks the DRIVER'S OWN function rather than restating its rule —
    // `dispatch_candidates` is the union both drivers plan over, and a second copy of the edge
    // rule here would be the drift `ready.rs` names in `edges_satisfied`'s doc comment.
    //
    // THE BARE-STATE LIST SURVIVES AS THE FALLBACK, and it is reported as such. Two cases reach
    // it: no `--file`, so there are no edges to ask about at all (the HTTP door, and every CLI
    // caller that does not pass one); and `ReadySetTooLarge`, where the driver itself blocks for
    // an owner decision and this command is the wrong place to refuse a hold. Neither may be
    // silent — a narrow list and a wide list that look identical on the wire is the same
    // over-claim in a new place — so `heldNodesGated` states which one produced this list.
    //
    // THE LIMIT, stated rather than implied: `heldNodesGated` travels in this REPLY only. It is
    // not part of any appended event, so a stream read back later still cannot distinguish a
    // narrow hold from a wide one. An idempotent HTTP retry reconstructs current status from
    // events; it does not replay a stored response, so it cannot recover that edge evidence.
    // Persisting the evidence is a separate change with its own surface;
    // #157 stays open for it.
    let bare: Vec<String> = projection
        .node_states
        .iter()
        .filter(|(_, state)| matches!(state, NodeState::Ready | NodeState::Queued))
        .map(|(node, _)| node.clone())
        .collect();
    // `edge_gated`: the edge rule was APPLIED to this list, not that the listed nodes are gated.
    let (held, edge_gated) = match spec
        .map(|spec| graphhelm_execution::dispatch_candidates(spec, &projection.node_states))
    {
        Some(Ok(candidates)) => (
            bare.into_iter()
                .filter(|node| candidates.contains(node))
                .collect::<Vec<String>>(),
            true,
        ),
        Some(Err(_)) | None => (bare, false),
    };
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
        map.insert("heldNodesGated".to_owned(), serde_json::json!(edge_gated));
    }
    Ok(data)
}
