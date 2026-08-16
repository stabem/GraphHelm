//! The wake lease's command layer (05g Task 3): arming and reading — the SLEEPER-ONLY
//! surface. There is deliberately no command to ring another session: the ring belongs to
//! the serve sweep alone (05g Task 2), and omission is the enforcement, the 05e pattern.

use std::path::Path;

use graphhelm_protocols::{EventKind, NewEvent, OpaqueId, PersistedActor, Sensitivity, WakeLease};

use super::{Failure, append_event, execution_state, load_projection, resolve_stream};
use crate::commands::event_store;

/// Arms (or re-arms — the fold replaces, never stacks) the caller's OWN lease. `cursor`
/// defaults to the stream's current head: "wake me for anything after now".
pub(crate) fn arm(
    events: &Path,
    execution: Option<&str>,
    session_id: &str,
    rendezvous_id: &str,
    cursor: Option<u64>,
    actor: PersistedActor,
    key: OpaqueId,
) -> Result<serde_json::Value, Failure> {
    let store = event_store(events).map_err(|error| super::repository_failure(&error))?;
    let (scope, stream, history) = resolve_stream(&store, execution)?;
    let projection = graphhelm_events::replay(&scope, &stream, &history)
        .map_err(|error| super::replay_failure(&error))?;
    let execution_id = projection
        .execution_id
        .clone()
        .ok_or_else(|| execution_state("no execution has started on this stream", "/execution"))?;

    let head = history.last().map_or(0, |event| event.sequence);
    let armed_cursor = cursor.unwrap_or(head);

    let session = OpaqueId::parse(session_id)
        .map_err(|_| execution_state("the session identifier is not wire-safe", "/sessionId"))?;
    let rendezvous = OpaqueId::parse(rendezvous_id).map_err(|_| {
        execution_state(
            "the rendezvous identifier is not wire-safe (an OPAQUE id, never a path)",
            "/rendezvousId",
        )
    })?;
    let execution_opaque = OpaqueId::parse(&execution_id)
        .map_err(|_| execution_state("the execution identifier is not wire-safe", "/execution"))?;
    let stream_opaque = OpaqueId::parse(&stream)
        .map_err(|_| execution_state("the stream identifier is not wire-safe", "/execution"))?;

    append_event(
        &store,
        &scope,
        &stream_opaque,
        NewEvent::new(
            key,
            actor,
            Sensitivity::Internal,
            EventKind::WakeLease(WakeLease {
                execution_id: execution_opaque,
                session_id: session,
                cursor: armed_cursor,
                rendezvous_id: rendezvous,
            }),
            vec![],
            vec![],
        ),
    )?;

    Ok(serde_json::json!({
        "executionId": execution_id,
        "sessionId": session_id,
        "armedCursor": armed_cursor,
        "rendezvousId": rendezvous_id,
    }))
}

/// Reads the caller's OWN live lease from the projection — `live: false` when none is
/// armed (consumed, replaced elsewhere, or never armed; the projection cannot tell those
/// apart and honestly does not try).
pub(crate) fn status(
    events: &Path,
    execution: Option<&str>,
    session_id: &str,
) -> Result<serde_json::Value, Failure> {
    let store = event_store(events).map_err(|error| super::repository_failure(&error))?;
    let (_, _, projection) = load_projection(&store, execution)?;
    Ok(match projection.wake_leases.get(session_id) {
        Some(lease) => serde_json::json!({
            "sessionId": session_id,
            "live": true,
            "cursor": lease.cursor,
            "rendezvousId": lease.rendezvous_id,
        }),
        None => serde_json::json!({ "sessionId": session_id, "live": false }),
    })
}
