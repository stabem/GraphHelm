use std::path::Path;

use graphhelm_protocols::{OpaqueId, PersistedActor, PersistedTimestamp, SweepCaller};

use super::{Failure, argument, finish, owner_actor, repository_failure, resolve_stream};
use crate::commands::event_store;
use crate::output::Outcome;

const COMMAND: &str = "execution.sweep";

/// Evaluate the stream's customs stages and journal the result.
///
/// The verb itself lives in `core/events`; this file is the operator's door to it and deliberately
/// carries no arithmetic of its own. A second copy of the overdue reckoning here would be a
/// duplicated ORACLE — the two would diverge in silence, and a silent divergence in an oracle
/// changes what "passed" means without anything going red.
pub fn run(events: &Path, execution: Option<&str>, as_of: Option<&str>) -> Outcome {
    finish(
        COMMAND,
        execute(events, execution, as_of, owner_actor(), None),
        |value| value,
    )
}

/// `SweepCaller::Operator`, hard-coded and not a flag.
///
/// The caller field exists so the journal can tell an operator's deliberate sweep from the serve
/// tick's automatic one. A flag would let either surface claim to be the other, which is precisely
/// the distinction the field was added to preserve — the surface is the evidence, so the surface
/// decides.
pub(crate) fn execute(
    events: &Path,
    execution: Option<&str>,
    as_of: Option<&str>,
    actor: PersistedActor,
    key: Option<OpaqueId>,
) -> Result<serde_json::Value, Failure> {
    let store = event_store(events).map_err(|error| repository_failure(&error))?;
    let (scope, stream, _history) = resolve_stream(&store, execution)?;

    // ABSENT MEANS NOW, AND "NOW" IS READ FROM THE STORE'S OWN CLOCK. Defaulting to a time this
    // process reads would compare a caller's instant against a clock no event is ever stamped
    // with, and the verb's future-refusal would then be enforced against the wrong reference.
    let as_of = match as_of {
        Some(value) => PersistedTimestamp::parse(value).map_err(|_| {
            argument(
                "--as-of is not a canonical RFC 3339 UTC timestamp",
                "/as-of",
            )
        })?,
        None => store.now().map_err(|error| repository_failure(&error))?,
    };

    // The future-refusal is NOT re-implemented here. `sweep` refuses before it reads, computes or
    // writes anything, and a pre-check at this surface would be a second place for that rule to
    // live — one that can drift, and one that would make the verb's own guard unreachable from
    // this door and therefore untestable through it.
    let batch = graphhelm_events::sweep(
        &store,
        &scope,
        &stream,
        &as_of,
        &actor,
        SweepCaller::Operator,
        // Absent on the CLI, present over HTTP. The CLI has no caller-supplied identity to be
        // idempotent about -- every invocation is a fresh decision by whoever typed it -- so the
        // verb mints its own per-call key. An HTTP request carries an Idempotency-Key and the
        // whole surface promises a retry appends nothing, so that key reaches the record.
        key.as_ref(),
    )
    .map_err(|error| repository_failure(&error))?;

    let exceptions: Vec<serde_json::Value> = batch
        .iter()
        .filter_map(|envelope| match &envelope.kind {
            graphhelm_protocols::EventKind::OverdueException(payload) => Some(serde_json::json!({
                "nodeId": payload.node_id.to_string(),
                "episodeSequence": payload.episode_sequence,
            })),
            _ => None,
        })
        .collect();

    Ok(serde_json::json!({
        "stream": stream,
        "asOf": as_of,
        "caller": "operator",
        // The count and the list are both reported. A bare count would make "which episodes were
        // spent" a second command away, and the episodes a sweep marks are spent permanently.
        "exceptionCount": exceptions.len(),
        "exceptions": exceptions,
    }))
}
