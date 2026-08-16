//! The serve-side ring (05g Task 2): after a mutation route's append is DURABLE, sweep the
//! stream's live leases and ring each rendezvous with ONE content-free byte, then record the
//! consumption with the reason the ring actually earned.
//!
//! Two-phase by physics (declared discrepancy against the Task 1 doc comment's "same durable
//! batch"): the ring may only happen after the trigger is durable, and the TRUE consume
//! reason (`rung` vs `stale_rendezvous`) is only known after the ring attempt — so the
//! consumption is its own small follow-up append. A crash between trigger and consumption
//! leaves a live lease and a possibly-delivered byte: a spurious wake, safe by design (the
//! wake is content-free and the sleeper re-reads its own log).
//!
//! Only NON-wake appends ring: the lease's own arming event (and any consumption) is
//! bookkeeping past the cursor, not content — without this rule, arming would self-ring.
//!
//! A wake failure never fails the route that triggered it: the append already happened, and
//! the sleeper's dead-man timer covers every lost byte (accelerator, never correction).

use std::path::Path;
use std::sync::Arc;

use graphhelm_protocols::{
    ActorId, EventKind, NewEvent, OpaqueId, PersistedActor, PersistedActorType, Sensitivity,
    WakeConsumeReason, WakeLeaseConsumed,
};

/// Derives the platform rendezvous from the OPAQUE id (the Task 1 security deviation): a
/// fixed local prefix plus the id — a hostile lease can never point the serve at an
/// arbitrary path.
#[cfg(windows)]
fn rendezvous_path(rendezvous_id: &str) -> String {
    format!(r"\\.\pipe\graphhelm-wake-{rendezvous_id}")
}

#[cfg(not(windows))]
fn rendezvous_path(rendezvous_id: &str) -> String {
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_owned());
    format!("{runtime}/graphhelm/wake-{rendezvous_id}.sock")
}

/// One live lease due to ring, as the sweep's read phase reports it.
struct DueLease {
    execution_id: String,
    session_id: String,
    rendezvous_id: String,
}

/// Writes the one content-free byte. `Rung` when it crossed; `StaleRendezvous` for every
/// failure shape (missing pipe is the designed one — the sleeper died; anything else is
/// equally a dead rendezvous from the ring's point of view).
#[cfg(windows)]
async fn ring(rendezvous_id: &str) -> WakeConsumeReason {
    use tokio::io::AsyncWriteExt;
    let path = rendezvous_path(rendezvous_id);
    match tokio::net::windows::named_pipe::ClientOptions::new().open(&path) {
        Ok(mut client) => match client.write_all(&[1_u8]).await {
            Ok(()) => WakeConsumeReason::Rung,
            Err(_) => WakeConsumeReason::StaleRendezvous,
        },
        Err(_) => WakeConsumeReason::StaleRendezvous,
    }
}

#[cfg(not(windows))]
async fn ring(rendezvous_id: &str) -> WakeConsumeReason {
    use tokio::io::AsyncWriteExt;
    let path = rendezvous_path(rendezvous_id);
    match tokio::net::UnixStream::connect(&path).await {
        Ok(mut stream) => match stream.write_all(&[1_u8]).await {
            Ok(()) => WakeConsumeReason::Rung,
            Err(_) => WakeConsumeReason::StaleRendezvous,
        },
        Err(_) => WakeConsumeReason::StaleRendezvous,
    }
}

/// The sweep: read the stream's projection, ring every lease whose cursor lies before the
/// latest NON-wake append, and record each consumption. Called fire-and-forget after a
/// mutation route's own append succeeded; every failure path is a silent no-op.
pub(super) async fn sweep(events: Arc<Path>, execution: String) {
    // Phase 1 (blocking): what is due?
    let events_read = events.clone();
    let execution_read = execution.clone();
    let due = tokio::task::spawn_blocking(move || -> Option<Vec<DueLease>> {
        let store = crate::commands::event_store(&events_read).ok()?;
        let streams = store.list_streams().ok()?;
        let stream = streams
            .into_iter()
            .find(|stream| stream.stream_id == execution_read)?;
        let history = store
            .read_replay_stream(&stream.scope, &stream.stream_id)
            .ok()?;
        let projection =
            graphhelm_events::replay(&stream.scope, &stream.stream_id, &history).ok()?;
        // Only NON-wake appends ring: bookkeeping past the cursor is not content.
        let content_head = history
            .iter()
            .filter(|event| {
                !matches!(
                    event.kind,
                    EventKind::WakeLease(_) | EventKind::WakeLeaseConsumed(_)
                )
            })
            .map(|event| event.sequence)
            .max()
            .unwrap_or(0);
        let due: Vec<DueLease> = projection
            .wake_leases
            .iter()
            .filter(|(_, lease)| lease.cursor < content_head)
            .map(|(session, lease)| DueLease {
                execution_id: execution_read.clone(),
                session_id: session.clone(),
                rendezvous_id: lease.rendezvous_id.clone(),
            })
            .collect();
        Some(due)
    })
    .await
    .ok()
    .flatten()
    .unwrap_or_default();

    if due.is_empty() {
        return;
    }

    // Phase 2: ring each rendezvous; the reason is what actually happened.
    let mut consumptions = Vec::with_capacity(due.len());
    for lease in due {
        let reason = ring(&lease.rendezvous_id).await;
        consumptions.push((lease, reason));
    }

    // Phase 3 (blocking): record every consumption in one follow-up append.
    let _ = tokio::task::spawn_blocking(move || -> Option<()> {
        let store = crate::commands::event_store(&events).ok()?;
        let streams = store.list_streams().ok()?;
        let stream = streams
            .into_iter()
            .find(|stream| stream.stream_id == execution)?;
        let next = store.next_sequence(&stream.scope, &stream.stream_id).ok()?;
        let actor = PersistedActor::new(
            PersistedActorType::System,
            ActorId::parse("system-wake").ok()?,
        );
        let mut batch = Vec::with_capacity(consumptions.len());
        for (index, (lease, reason)) in consumptions.iter().enumerate() {
            batch.push(NewEvent::new(
                OpaqueId::parse(format!("wake-consume-{next}-{index}-{}", lease.session_id))
                    .ok()?,
                actor.clone(),
                Sensitivity::Internal,
                EventKind::WakeLeaseConsumed(WakeLeaseConsumed {
                    execution_id: OpaqueId::parse(lease.execution_id.clone()).ok()?,
                    session_id: OpaqueId::parse(lease.session_id.clone()).ok()?,
                    reason: *reason,
                }),
                vec![],
                vec![],
            ));
        }
        let request = graphhelm_events::PreparedAppend::new(
            stream.scope.clone(),
            OpaqueId::parse(stream.stream_id.clone()).ok()?,
            next,
            batch,
            vec![],
            vec![],
        )
        .ok()?;
        store.append_atomic(&request).ok()?;
        Some(())
    })
    .await;
}
