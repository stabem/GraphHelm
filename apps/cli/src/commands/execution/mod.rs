pub(super) mod approve;
pub(super) mod cancel;
mod driver;
pub(super) mod pause;
pub(super) mod resume;
pub(super) mod signal;
pub(super) mod start;
pub(super) mod status;

use std::collections::BTreeMap;
use std::path::Path;

use graphhelm_events::{
    EventRepositoryError, ExecutionProjection, LocalEventRepository, PreparedAppend, ReplayError,
};
use graphhelm_execution::{TransitionRequest, apply_transition};
use graphhelm_protocols::{
    ActorId, Diagnostic, EventEnvelope, EventKind, ExecutionId, IdGenerator, NewEvent, NodeOutcome,
    NodeOutcomeRecorded, NodeState, OpaqueId, PersistedActor, PersistedActorType, ProjectId,
    RepositoryScope, Sensitivity, SimulationStatus, WorkspaceId,
};
use graphhelm_simulation::SimulationFixtures;

use crate::commands::UuidIds;
use crate::output::Outcome;

/// A command refused by a precondition or guard the fold or the pure execution crate enforces —
/// the CLI states the refusal before the fold would call the history corrupt.
pub(super) const EXECUTION_STATE_CODE: &str = "GHCLI005_EXECUTION_STATE";
/// Reused from `events`'s vocabulary for malformed CLI arguments, per the plan.
pub(super) const ARGUMENT_CODE: &str = "GHCLI001_ARGUMENT_INVALID";
/// A signal envelope that fails validation — either it is not valid JSON at all, or
/// `graphhelm_governor::admit_signal` rejects it as schema-invalid. Declared here, alongside
/// `GHCLI004_SIGNAL_UNRECORDABLE`, now that `signal.rs` is their first consumer (Task 3 deferred
/// both to avoid a genuine `dead_code` finding under this workspace's `-D warnings` clippy gate).
pub(super) const SIGNAL_INVALID_CODE: &str = "GHCLI003_SIGNAL_INVALID";
/// A schema-valid signal whose `id` or `source.id` cannot be represented as an `OpaqueId` — the
/// event contract that carries it on the wire is stricter than the signal schema. The envelope is
/// still externalized as evidence; only the record is refused.
pub(super) const SIGNAL_UNRECORDABLE_CODE: &str = "GHCLI004_SIGNAL_UNRECORDABLE";
const SOURCE: &str = "execution-cli";

/// The scope constants every `execution` command shares with `graph simulate`: a single
/// operator-local workspace and project, with the execution identity carrying the rest.
pub(super) const WORKSPACE: &str = "workspace-local";
pub(super) const PROJECT: &str = "project-local";

/// A redaction-safe operator failure — see `commands::events`'s identical pattern. Only a stable
/// code, a fixed message and a JSON Pointer ever reach the user.
pub(super) struct Failure {
    pub(super) code: &'static str,
    pub(super) message: String,
    pub(super) pointer: String,
}

impl Failure {
    fn into_outcome(self, command: &'static str) -> Outcome {
        Outcome::domain(
            command,
            vec![Diagnostic::error(
                self.code,
                self.message,
                &self.pointer,
                SOURCE,
            )],
        )
    }
}

pub(super) fn argument(message: &str, pointer: &str) -> Failure {
    Failure {
        code: ARGUMENT_CODE,
        message: message.to_owned(),
        pointer: pointer.to_owned(),
    }
}

/// A command refused because a precondition or a pure guard (`ready_set`, `dispatch_plan`,
/// `apply_transition`, ...) said no.
pub(super) fn execution_state(message: &str, pointer: &str) -> Failure {
    Failure {
        code: EXECUTION_STATE_CODE,
        message: message.to_owned(),
        pointer: pointer.to_owned(),
    }
}

/// Maps a repository error onto its stable code without exposing the underlying path.
pub(super) fn repository_failure(error: &EventRepositoryError) -> Failure {
    Failure {
        code: error.code(),
        message: error.to_string(),
        pointer: "/repository".to_owned(),
    }
}

/// Maps a replay error onto its stable code. Reached only if the driver's own bookkeeping
/// produced a stream it cannot make sense of.
pub(super) fn replay_failure(error: &ReplayError) -> Failure {
    Failure {
        code: error.code(),
        message: error.to_string(),
        pointer: "/events".to_owned(),
    }
}

/// A signal envelope invalid before it ever reaches `admit_signal` (unparseable JSON) or refused
/// by it (`GovernanceError::InvalidSignal`, a schema violation). Either way nothing is written or
/// appended: a garbage envelope is not evidence.
pub(super) fn signal_invalid(message: &str, pointer: &str) -> Failure {
    Failure {
        code: SIGNAL_INVALID_CODE,
        message: message.to_owned(),
        pointer: pointer.to_owned(),
    }
}

/// A schema-valid signal the event contract cannot carry on the wire
/// (`GovernanceError::UnrecordableIdentity`). The caller has already written the envelope to
/// `--evidence-out` before constructing this — the message says so.
pub(super) fn signal_unrecordable(message: &str, pointer: &str) -> Failure {
    Failure {
        code: SIGNAL_UNRECORDABLE_CODE,
        message: message.to_owned(),
        pointer: pointer.to_owned(),
    }
}

pub(super) fn finish<T>(
    command: &'static str,
    result: Result<T, Failure>,
    render: impl FnOnce(T) -> serde_json::Value,
) -> Outcome {
    match result {
        Ok(value) => Outcome::success(command, render(value)),
        Err(failure) => failure.into_outcome(command),
    }
}

/// Loads a JSON simulation fixture file, identically to `graph simulate`'s own loader. Kept local
/// rather than shared: `simulate.rs` does not expose it, and this module's file set is disjoint
/// from `simulate.rs`'s per the plan's scope discipline.
pub(super) fn load_fixtures(path: Option<&Path>) -> Result<SimulationFixtures, Failure> {
    let Some(path) = path else {
        return Ok(SimulationFixtures::default());
    };
    let bytes = std::fs::read(path)
        .map_err(|_| argument("--fixtures does not name a readable file", "/fixtures"))?;
    serde_json::from_slice(&bytes).map_err(|_| {
        argument(
            "--fixtures is not valid simulation fixture JSON",
            "/fixtures",
        )
    })
}

/// Resolves which stream a command addresses and reads its raw history: `--execution` when
/// given, otherwise `status.rs`'s own fallback — a repository holding exactly one stream selects
/// it. Every `execution` command past `start` shares this addressing rule.
pub(super) fn resolve_stream(
    store: &LocalEventRepository,
    execution: Option<&str>,
) -> Result<(RepositoryScope, String, Vec<EventEnvelope>), Failure> {
    match execution {
        Some(execution) => {
            let stream_id = OpaqueId::parse(execution)
                .map_err(|_| argument("--execution is not a valid identifier", "/execution"))?;
            let scope = RepositoryScope::new(
                WorkspaceId::parse(WORKSPACE).expect("constant workspace id is valid"),
                ProjectId::parse(PROJECT).expect("constant project id is valid"),
                Some(ExecutionId::parse(execution).map_err(|_| {
                    argument("--execution is not a valid identifier", "/execution")
                })?),
            );
            let history = store
                .read_replay_stream(&scope, stream_id.as_str())
                .map_err(|error| repository_failure(&error))?;
            Ok((scope, stream_id.to_string(), history))
        }
        None => {
            let (selection, history) = store
                .read_unique_replay_stream()
                .map_err(|error| repository_failure(&error))?;
            Ok((selection.scope, selection.stream_id, history))
        }
    }
}

/// `resolve_stream` plus the replay every caller immediately needs from it.
pub(super) fn load_projection(
    store: &LocalEventRepository,
    execution: Option<&str>,
) -> Result<(RepositoryScope, String, ExecutionProjection), Failure> {
    let (scope, stream, history) = resolve_stream(store, execution)?;
    let projection = graphhelm_events::replay(&scope, &stream, &history)
        .map_err(|error| replay_failure(&error))?;
    Ok((scope, stream, projection))
}

/// Re-reads and replays a stream already resolved to a `scope`/`stream` pair — every command that
/// appends an event needs this to see its own effect before reporting, or to re-derive state
/// before a further append.
pub(super) fn replay_projection(
    store: &LocalEventRepository,
    scope: &RepositoryScope,
    stream: &str,
) -> Result<ExecutionProjection, Failure> {
    let history = store
        .read_replay_stream(scope, stream)
        .map_err(|error| repository_failure(&error))?;
    graphhelm_events::replay(scope, stream, &history).map_err(|error| replay_failure(&error))
}

/// The system actor every `execution` command writes as, identically to `graph simulate`'s.
/// The actor for owner-initiated commands: approve, pause, resume, cancel, signal.
///
/// D-019 makes owner sovereignty the point of these commands, and an event log that cannot tell
/// "the owner cancelled this" from "the driver's own bookkeeping" would undercut it. The driver's
/// automatic hops stay under the system actor; every explicit decision is recorded as the owner's.
pub(super) fn owner_actor() -> PersistedActor {
    PersistedActor::new(
        PersistedActorType::Owner,
        ActorId::parse("owner-cli").expect("constant actor id is valid"),
    )
}

pub(super) fn system_actor() -> PersistedActor {
    PersistedActor::new(
        PersistedActorType::System,
        ActorId::parse("system-cli").expect("constant actor id is valid"),
    )
}

/// A fresh idempotency key for one appended event, derived the same way `driver.rs` derives its
/// own.
pub(super) fn idempotency_key(prefix: &'static str) -> OpaqueId {
    OpaqueId::parse(UuidIds.next_id(prefix)).expect("uuid-derived id is wire-safe")
}

/// Appends one prepared event to `stream` at its next sequence.
pub(super) fn append_event(
    store: &LocalEventRepository,
    scope: &RepositoryScope,
    stream: &OpaqueId,
    event: NewEvent,
) -> Result<(), Failure> {
    let next_sequence = store
        .next_sequence(scope, stream.as_str())
        .map_err(|error| repository_failure(&error))?;
    let request = PreparedAppend::new(
        scope.clone(),
        stream.clone(),
        next_sequence,
        vec![event],
        vec![],
        vec![],
    )
    .map_err(|error| repository_failure(&error))?;
    store
        .append_atomic(&request)
        .map_err(|error| repository_failure(&error))?;
    Ok(())
}

/// Appends one `node_outcome_recorded`, with `next_state` always computed by the real
/// `apply_transition` against the freshly replayed projection — never invented. The same pattern
/// `driver.rs`'s private copy uses; kept as a second copy here (rather than widened visibility on
/// that one) so this task's diff stays inside the files it owns.
pub(super) fn record_outcome(
    store: &LocalEventRepository,
    scope: &RepositoryScope,
    stream: &OpaqueId,
    execution_id: &OpaqueId,
    actor: &PersistedActor,
    node: &str,
    outcome: NodeOutcome,
) -> Result<NodeState, Failure> {
    let projection = replay_projection(store, scope, stream.as_str())?;
    let current = projection
        .node_states
        .get(node)
        .copied()
        .unwrap_or(NodeState::Draft);
    let attempts = projection.node_attempts.get(node).copied().unwrap_or(0);
    let identical_outcomes = projection.identical_outcomes_for(node, outcome);
    let next_state = apply_transition(&TransitionRequest {
        current,
        outcome,
        attempts,
        identical_outcomes,
    })
    .map_err(|_| {
        execution_state(
            "the command attempted an outcome the node's current state does not allow",
            "/execution/transition",
        )
    })?;
    let node_id = OpaqueId::parse(node)
        .map_err(|_| execution_state("node identifier is not wire-safe", "/execution"))?;
    append_event(
        store,
        scope,
        stream,
        NewEvent::new(
            idempotency_key("node-outcome"),
            actor.clone(),
            Sensitivity::Internal,
            EventKind::NodeOutcomeRecorded(NodeOutcomeRecorded {
                execution_id: execution_id.clone(),
                node_id,
                outcome,
                next_state,
            }),
            vec![],
            vec![],
        ),
    )?;
    Ok(next_state)
}

/// A node in one of these states will never be revisited by the driver; `cancel` is the only
/// other command that needs to tell terminal from non-terminal, so this mirrors — rather than
/// widens the visibility of — `driver.rs`'s private copy, keeping this task's diff inside the
/// files it owns.
pub(super) const fn is_terminal(state: NodeState) -> bool {
    matches!(
        state,
        NodeState::Succeeded
            | NodeState::Failed
            | NodeState::Waived
            | NodeState::Skipped
            | NodeState::Cancelled
    )
}

/// The wire label for an aggregate `SimulationStatus`, matching
/// `graphhelm_protocols::SimulationStatus`'s own snake_case wire vocabulary. `None` reads as
/// `"none"` for operator messages describing a status nothing has set yet — no `simulation`,
/// `execution_paused`, `execution_resumed` or `execution_completed` event has folded.
pub(super) const fn simulation_status_label(status: Option<&SimulationStatus>) -> &'static str {
    match status {
        None => "none",
        Some(SimulationStatus::Running) => "running",
        Some(SimulationStatus::Completed) => "completed",
        Some(SimulationStatus::Failed) => "failed",
        Some(SimulationStatus::Paused) => "paused",
        Some(SimulationStatus::Blocked) => "blocked",
        Some(SimulationStatus::Cancelled) => "cancelled",
    }
}

/// The wire label for a node state, matching the `snake_case` vocabulary
/// `graphhelm_protocols::NodeState` serializes to.
pub(super) const fn node_state_label(state: NodeState) -> &'static str {
    match state {
        NodeState::Draft => "draft",
        NodeState::Ghost => "ghost",
        NodeState::Linting => "linting",
        NodeState::Ready => "ready",
        NodeState::Queued => "queued",
        NodeState::Running => "running",
        NodeState::WaitingInput => "waiting_input",
        NodeState::WaitingCapacity => "waiting_capacity",
        NodeState::Paused => "paused",
        NodeState::Blocked => "blocked",
        NodeState::Succeeded => "succeeded",
        NodeState::Failed => "failed",
        NodeState::Waived => "waived",
        NodeState::Skipped => "skipped",
        NodeState::Cancelled => "cancelled",
        NodeState::Invalidated => "invalidated",
    }
}

/// Per-state node counts, keyed by the same wire vocabulary the projection itself uses.
fn state_counts(node_states: &BTreeMap<String, NodeState>) -> BTreeMap<&'static str, u64> {
    let mut counts = BTreeMap::new();
    for state in node_states.values() {
        *counts.entry(node_state_label(*state)).or_insert(0_u64) += 1;
    }
    counts
}

/// The operator's triage view (Task 2, 04f): a node is an untriaged interruption exactly when it
/// sits `Blocked` with `last_outcome == Interrupted` — recorded by a crash recovery but never
/// looked at since. Mirrors `resume_preconditions`'s own condition in `core/execution`; no
/// reusable list-producing function exists there to call instead.
fn untriaged_interruptions(projection: &ExecutionProjection) -> Vec<String> {
    projection
        .node_states
        .iter()
        .filter(|(node, state)| {
            **state == NodeState::Blocked
                && projection.last_outcome.get(*node) == Some(&NodeOutcome::Interrupted)
        })
        .map(|(node, _)| node.clone())
        .collect()
}

/// The shared reporting shape every `execution` command that returns a projection view uses
/// (`start`, `status`, `approve`, `pause`, `resume`, `cancel`): execution id, mode, aggregate
/// status, per-state node counts, signal/mutation counters, and the untriaged-interruption triage
/// list. `signal` reports its own governance-verdict shape instead, and `pause` extends this one
/// with `heldNodes`.
pub(super) fn render(projection: &ExecutionProjection) -> serde_json::Value {
    serde_json::json!({
        "executionId": projection.execution_id,
        "mode": projection.mode,
        "status": projection.simulation_status,
        "nodeStateCounts": state_counts(&projection.node_states),
        "signalsRecorded": projection.signals_recorded,
        "acceptedMutations": projection.accepted_mutations,
        "untriagedInterruptions": untriaged_interruptions(projection),
    })
}
