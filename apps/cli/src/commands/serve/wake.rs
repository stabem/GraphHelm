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
#[derive(Clone)]
pub(crate) struct DueLease {
    pub(crate) execution_id: String,
    pub(crate) session_id: String,
    pub(crate) rendezvous_id: String,
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

    // Phase 3 (blocking): record every consumption in one follow-up append — through the
    // guarded recorder (hotfix #55): the consumption is validated against a FRESH replay
    // under the SAME open store handle that appends. The handle's exclusive lock spans
    // read and write, so the read-then-append window two racing sweeps used to slip
    // through (the double-consume that poisoned the factory pair store) no longer exists.
    let _ = tokio::task::spawn_blocking(move || {
        record_consumptions(&events, &execution, &consumptions);
    })
    .await;
}

/// Records consumptions for leases that are STILL LIVE at append time, silently dropping
/// the rest — under one store handle, whose exclusive lock makes the re-validation and the
/// append atomic against every other writer (hotfix #55; the archived corrupted pair store
/// is the incident this guard exists for). Returns how many consumptions were recorded.
pub(crate) fn record_consumptions(
    events: &Path,
    execution: &str,
    consumptions: &[(DueLease, WakeConsumeReason)],
) -> usize {
    let Ok(store) = crate::commands::event_store(events) else {
        return 0;
    };
    let Ok(streams) = store.list_streams() else {
        return 0;
    };
    let Some(stream) = streams
        .into_iter()
        .find(|stream| stream.stream_id == execution)
    else {
        return 0;
    };
    // The guard: re-replay under THIS handle's lock; only a consumption whose lease is
    // still live, with the SAME rendezvous, may be recorded. A rival sweep that got here
    // first burned the lease — ours drops silently (its ring was at worst a spurious
    // content-free byte the sleeper's own re-read absorbs).
    let Ok(history) = store.read_replay_stream(&stream.scope, &stream.stream_id) else {
        return 0;
    };
    let Ok(projection) = graphhelm_events::replay(&stream.scope, &stream.stream_id, &history)
    else {
        return 0;
    };
    let still_live: Vec<&(DueLease, WakeConsumeReason)> = consumptions
        .iter()
        .filter(|(lease, _)| {
            projection
                .wake_leases
                .get(&lease.session_id)
                .is_some_and(|live| live.rendezvous_id == lease.rendezvous_id)
        })
        .collect();
    if still_live.is_empty() {
        return 0;
    }
    let Ok(next) = store.next_sequence(&stream.scope, &stream.stream_id) else {
        return 0;
    };
    let Ok(actor_id) = ActorId::parse("system-wake") else {
        return 0;
    };
    let actor = PersistedActor::new(PersistedActorType::System, actor_id);
    let mut batch = Vec::with_capacity(still_live.len());
    for (index, (lease, reason)) in still_live.iter().enumerate() {
        let Ok(idempotency) =
            OpaqueId::parse(format!("wake-consume-{next}-{index}-{}", lease.session_id))
        else {
            return 0;
        };
        let (Ok(execution_id), Ok(session_id)) = (
            OpaqueId::parse(lease.execution_id.clone()),
            OpaqueId::parse(lease.session_id.clone()),
        ) else {
            return 0;
        };
        batch.push(NewEvent::new(
            idempotency,
            actor.clone(),
            Sensitivity::Internal,
            EventKind::WakeLeaseConsumed(WakeLeaseConsumed {
                execution_id,
                session_id,
                reason: *reason,
            }),
            vec![],
            vec![],
        ));
    }
    let recorded = batch.len();
    let Ok(request) = graphhelm_events::PreparedAppend::new(
        stream.scope.clone(),
        OpaqueId::parse(stream.stream_id.to_owned()).expect("stream ids are wire-safe"),
        next,
        batch,
        vec![],
        vec![],
    ) else {
        return 0;
    };
    if store.append_atomic(&request).is_err() {
        return 0;
    }
    recorded
}

#[cfg(test)]
mod tests {
    use super::*;
    use graphhelm_protocols::{EventKind, NewEvent, OpaqueId, Sensitivity, WakeLease};

    /// The #55 interleaving, DETERMINISTIC: a sweep captured its due list, then a rival
    /// consumed the lease first (simulated by a direct consume append), then our sweep
    /// reaches the recorder. Unguarded, the second consumption lands and the fold refuses
    /// every later replay — the exact live failure that poisoned the factory pair store.
    /// Guarded, the recorder re-validates under its own append lock and records NOTHING.
    #[test]
    fn a_rival_consume_between_read_and_record_appends_nothing() {
        let directory = tempfile::tempdir().unwrap();
        let events = directory.path();
        let store = crate::commands::event_store(events).unwrap();
        let scope = graphhelm_protocols::RepositoryScope::new(
            graphhelm_protocols::WorkspaceId::parse("workspace-w").unwrap(),
            graphhelm_protocols::ProjectId::parse("project-w").unwrap(),
            Some(graphhelm_protocols::ExecutionId::parse("exec-race").unwrap()),
        );
        let stream = OpaqueId::parse("exec-race").unwrap();
        let actor = PersistedActor::new(
            PersistedActorType::System,
            ActorId::parse("system-test").unwrap(),
        );
        let append = |kind: EventKind, key: &str, next: u64| {
            let request = graphhelm_events::PreparedAppend::new(
                scope.clone(),
                stream.clone(),
                next,
                vec![NewEvent::new(
                    OpaqueId::parse(key).unwrap(),
                    actor.clone(),
                    Sensitivity::Internal,
                    kind,
                    vec![],
                    vec![],
                )],
                vec![],
                vec![],
            )
            .unwrap();
            store.append_atomic(&request).unwrap();
        };

        // The armed lease this sweep read.
        append(
            EventKind::WakeLease(WakeLease {
                execution_id: OpaqueId::parse("exec-race").unwrap(),
                session_id: OpaqueId::parse("session-r").unwrap(),
                cursor: 0,
                rendezvous_id: OpaqueId::parse("rdv-r").unwrap(),
            }),
            "arm-r",
            1,
        );
        let captured = vec![(
            DueLease {
                execution_id: "exec-race".to_owned(),
                session_id: "session-r".to_owned(),
                rendezvous_id: "rdv-r".to_owned(),
            },
            WakeConsumeReason::StaleRendezvous,
        )];

        // The rival got there first.
        append(
            EventKind::WakeLeaseConsumed(WakeLeaseConsumed {
                execution_id: OpaqueId::parse("exec-race").unwrap(),
                session_id: OpaqueId::parse("session-r").unwrap(),
                reason: WakeConsumeReason::Rung,
            }),
            "rival-consume",
            2,
        );
        drop(store); // release the handle so the recorder can take its own lock

        // Our sweep now reaches the recorder with its STALE capture.
        let recorded = record_consumptions(events, "exec-race", &captured);
        assert_eq!(
            recorded, 0,
            "a consumption whose lease a rival already burned must be dropped"
        );

        // The oracle the live incident failed: the stream still replays.
        let store = crate::commands::event_store(events).unwrap();
        let history = store.read_replay_stream(&scope, "exec-race").unwrap();
        let replayed = graphhelm_events::replay(&scope, "exec-race", &history);
        assert!(
            replayed.is_ok(),
            "the stream must remain replayable: {replayed:?}"
        );
    }

    /// The recorder still records when the lease IS live — the guard filters rivals'
    /// leftovers, never legitimate consumptions.
    #[test]
    fn a_live_lease_consumption_still_records() {
        let directory = tempfile::tempdir().unwrap();
        let events = directory.path();
        {
            let store = crate::commands::event_store(events).unwrap();
            let scope = graphhelm_protocols::RepositoryScope::new(
                graphhelm_protocols::WorkspaceId::parse("workspace-w").unwrap(),
                graphhelm_protocols::ProjectId::parse("project-w").unwrap(),
                Some(graphhelm_protocols::ExecutionId::parse("exec-live").unwrap()),
            );
            let request = graphhelm_events::PreparedAppend::new(
                scope,
                OpaqueId::parse("exec-live").unwrap(),
                1,
                vec![NewEvent::new(
                    OpaqueId::parse("arm-l").unwrap(),
                    PersistedActor::new(
                        PersistedActorType::System,
                        ActorId::parse("system-test").unwrap(),
                    ),
                    Sensitivity::Internal,
                    EventKind::WakeLease(WakeLease {
                        execution_id: OpaqueId::parse("exec-live").unwrap(),
                        session_id: OpaqueId::parse("session-l").unwrap(),
                        cursor: 0,
                        rendezvous_id: OpaqueId::parse("rdv-l").unwrap(),
                    }),
                    vec![],
                    vec![],
                )],
                vec![],
                vec![],
            )
            .unwrap();
            store.append_atomic(&request).unwrap();
        }
        let recorded = record_consumptions(
            events,
            "exec-live",
            &[(
                DueLease {
                    execution_id: "exec-live".to_owned(),
                    session_id: "session-l".to_owned(),
                    rendezvous_id: "rdv-l".to_owned(),
                },
                WakeConsumeReason::Rung,
            )],
        );
        assert_eq!(recorded, 1, "a live lease's consumption records normally");
    }
}
