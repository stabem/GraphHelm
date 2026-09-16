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
    /// WHICH ARMING this capture is for, copied from the projection phase 1 already holds.
    ///
    /// The rendezvous is kept for phase 2 — it is what the ring is addressed to — but it is no
    /// longer a blade in the filter, because a session that re-arms on the same fixed id
    /// produces two leases the rendezvous cannot tell apart. This can.
    pub(crate) armed_at_sequence: u64,
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

/// Phase 1 of the sweep: what is due? Replay the stream and keep every live lease whose
/// cursor lies before the latest NON-wake append. `None` on any failure — the sweep is
/// fire-and-forget and every failure path is a silent no-op.
///
/// Its own function so the unit tests below build their captures by CALLING it rather than by
/// copying it (#258). A copy cannot notice the original drifting, and that drift is exactly
/// what those tests exist to catch: with `armed_at_sequence` corrupted here, every cell stayed
/// green while the read was a copy, because none of them ran this code.
fn due_leases(events: &Path, execution: &str) -> Option<Vec<DueLease>> {
    let store = crate::commands::event_store(events).ok()?;
    let streams = store.list_streams().ok()?;
    let stream = streams
        .into_iter()
        .find(|stream| stream.stream_id == execution)?;
    let history = store
        .read_replay_stream(&stream.scope, &stream.stream_id)
        .ok()?;
    let projection = graphhelm_events::replay(&stream.scope, &stream.stream_id, &history).ok()?;
    // Only NON-wake appends ring: bookkeeping past the cursor is not content. A presence
    // declaration is bookkeeping too (#1054): it is appended right after the mutation that
    // armed the lease, by the same session, and counting it as content consumed the lease
    // before the sleeper ever waited (gate 9b277afb, `wake_http`:
    // a_sleeper_wakes_on_a_peer_append_with_zero_requests_in_the_window`).
    let content_head = history
        .iter()
        .filter(|event| {
            !matches!(
                event.kind,
                EventKind::WakeLease(_)
                    | EventKind::WakeLeaseConsumed(_)
                    | EventKind::AgentPresenceDeclared(_)
            )
        })
        .map(|event| event.sequence)
        .max()
        .unwrap_or(0);
    let due = projection
        .wake_leases
        .iter()
        .filter(|(_, lease)| lease.cursor < content_head)
        .map(|(session, lease)| DueLease {
            execution_id: execution.to_owned(),
            session_id: session.clone(),
            rendezvous_id: lease.rendezvous_id.clone(),
            armed_at_sequence: lease.armed_at_sequence,
        })
        .collect();
    Some(due)
}

/// The sweep: read the stream's projection, ring every lease whose cursor lies before the
/// latest NON-wake append, and record each consumption. Called fire-and-forget after a
/// mutation route's own append succeeded; every failure path is a silent no-op.
pub(super) async fn sweep(events: Arc<Path>, execution: String) {
    // Phase 1 (blocking): what is due?
    let events_read = events.clone();
    let execution_read = execution.clone();
    let due = tokio::task::spawn_blocking(move || due_leases(&events_read, &execution_read))
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
        test_only_phase3_delay();
        record_consumptions(&events, &execution, &consumptions);
    })
    .await;
}

/// Test-only seam (#72): stretches the gap between the ring (phase 2) and the consume
/// append becoming durable (phase 3) by the number of milliseconds named in
/// `GRAPHHELM_TEST_WAKE_PHASE3_DELAY_MS`. The receipt is eventually-visible BY DESIGN;
/// this seam makes "eventually" a chosen number so a guard can prove it waits for the
/// CONDITION instead of winning a schedule. Production cost when the variable is absent:
/// one getenv, no delay. Setting it in production would only slow receipts down — it can
/// never reorder the phases or drop a consumption.
fn test_only_phase3_delay() {
    if let Some(delay) = std::env::var("GRAPHHELM_TEST_WAKE_PHASE3_DELAY_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
    {
        std::thread::sleep(std::time::Duration::from_millis(delay));
    }
}

/// Records consumptions for leases that were STILL LIVE when this decision was taken,
/// silently dropping the rest (hotfix #55; the archived corrupted pair store is the incident
/// this guard exists for). Returns how many consumptions were recorded.
///
/// This used to claim that one store handle's exclusive lock made the re-validation and the
/// append atomic. It never did: the lock is taken and released per operation, and an open
/// handle holds none in between — at the parent of the read-concurrency change as much as
/// after it. Nothing was made worse by widening reads, and reverting that would restore
/// nothing here; what was missing was a guard, and the sentence describing one was standing
/// in for it.
pub(crate) fn record_consumptions(
    events: &Path,
    execution: &str,
    consumptions: &[(DueLease, WakeConsumeReason)],
) -> usize {
    record_consumptions_inner(events, execution, consumptions, &|| {})
}

/// The recorder proper, with a seam between DECIDING a lease is still live and PINNING the
/// sequence the consumption will append at.
///
/// `after_validation` runs once, exactly there, and production passes a no-op closure — so
/// the path a test exercises IS the production path, not a `cfg(test)` copy of it. A test
/// against a duplicated path proves something about the duplicate.
///
/// The seam is placed there because that is where the race lives: no lock is held at that
/// point (`with_lock` takes and releases per call, and an open handle holds none between
/// operations), so a rival writer can land there. Making the window executable is what turns
/// an argued interleaving into an observed one.
fn record_consumptions_inner(
    events: &Path,
    execution: &str,
    consumptions: &[(DueLease, WakeConsumeReason)],
    after_validation: &dyn Fn(),
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
    // The guard, in TWO blades that cover opposite sides of the same instant.
    //
    // Blade one, below: only a consumption whose lease is still live, AND IS STILL THE SAME
    // ARMING, may be recorded. That covers a rival who burned the lease BEFORE this read —
    // ours drops silently (its ring was at worst a spurious content-free byte the sleeper's
    // own re-read absorbs) — and it covers the case that has nothing to do with rivals: the
    // sleeper we just rang woke up and re-armed, and this capture is for the lease it replaced.
    //
    // The rendezvous is NOT compared and its absence is not a weakening — it is IMPLIED. A
    // lease is a pure function of its arming event, so the same session at the same arming
    // sequence is the same event, which has one rendezvous. Comparing it as well could never
    // decide anything, and keeping it would be worse than useless: with the rendezvous still
    // in the filter, a fold that wrote a constant into `armed_at_sequence` would leave the
    // rendezvous-swap guard GREEN, hiding the discriminator's own failure in the one test
    // built to see it. A redundant blade blinds the sabotage of the blade that matters.
    //
    // Blade two: the sequence this batch will append at is PINNED FROM THIS READ, not asked
    // for separately afterwards. That covers a rival who lands AFTER it: the pin goes stale
    // and the store's own sequence check refuses us.
    //
    // Both blades are needed and neither is redundant. Asking the store for the next
    // sequence in a second call was the defect: this read said the lease was live, that call
    // returned a number taken after a rival had already burned it, and the number was
    // genuinely current — so nothing downstream had anything to object to, and a second
    // consumption landed on a lease that no longer existed. The fold then refused every
    // later replay of the stream.
    let Ok(history) = store.read_replay_stream(&stream.scope, &stream.stream_id) else {
        return 0;
    };
    // Pinned here, from the history this decision is made against.
    //
    // `max`, not `last`: the ordering of a replay read is journal order, and this must not
    // quietly depend on that being sequence order. Equal to what `next_sequence` would
    // answer, because sequences are contiguous per stream — local.rs assigns the next as
    // expected + batch length and refuses on load any batch whose first event does not carry
    // exactly the expected sequence. If that ever changes, this line is where it breaks.
    let next = history
        .iter()
        .map(|event| event.sequence)
        .max()
        .map_or(1, |sequence| sequence + 1);
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
                .is_some_and(|live| live.armed_at_sequence == lease.armed_at_sequence)
        })
        .collect();
    if still_live.is_empty() {
        return 0;
    }
    // The seam: the decision above is made, the append below has not happened yet.
    after_validation();
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
                // The arming THIS CAPTURE named, not whatever is live now. The filter above has
                // already established they agree; recording the captured one is what lets a
                // later replay notice if they ever do not.
                captured_arming: Some(lease.armed_at_sequence),
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
    use graphhelm_protocols::{
        EventKind, NewEvent, NodeState, NodeStateChanged, OpaqueId, Sensitivity, WakeLease,
    };

    /// The sweep's OWN read — `due_leases`, the function phase 1 of `sweep` runs — narrowed to
    /// the one session a fixture cares about.
    ///
    /// Two generations of this helper came before it, each found wanting the expensive way.
    /// Hand-written captures proved things about the fixture rather than about the production
    /// data flow: with `armed_at_sequence` typed in as a literal, a sabotage that corrupts the
    /// FOLD moves the live lease's value while the capture's stays put, so the two disagree and
    /// a stale capture is dropped — the guard passes, for a reason production would never
    /// reproduce. The replacement replayed the stream and copied the lease's fields "the way
    /// the sweep does" — a copy of the read, which cannot notice the read drifting: with
    /// `armed_at_sequence` corrupted INSIDE the sweep, every cell stayed green, because none of
    /// them ran the sweep's code (#258). It also skipped the sweep's due-predicate outright,
    /// handing back leases the sweep itself would never have captured.
    ///
    /// Now the fixture is judged by the production read, filter included. That is why every
    /// fixture appends content after arming — see `append_content` — and why this panics rather
    /// than returning an `Option`: a fixture whose lease the sweep would not capture is not a
    /// fixture for the sweep's recorder.
    fn captured_by_the_sweep(events: &std::path::Path, stream: &str, session: &str) -> DueLease {
        due_leases(events, stream)
            .expect("the sweep's read phase must find the fixture's stream")
            .into_iter()
            .find(|lease| lease.session_id == session)
            .expect(
                "the fixture's lease must be DUE under the sweep's own predicate: armed, with \
                 content appended past its cursor",
            )
    }

    /// One CONTENT event — a node moving state, the kind of append a sleeper is waiting on.
    fn content_event() -> EventKind {
        EventKind::NodeStateChanged(NodeStateChanged {
            simulation_id: OpaqueId::parse("simulation-1").unwrap(),
            node_id: OpaqueId::parse("node-1").unwrap(),
            previous_state: None,
            next_state: NodeState::Running,
        })
    }

    /// Appends one content event, because a lease armed on an otherwise-empty stream is live
    /// but NOT due: the sweep hands back only leases whose cursor lies before the latest
    /// non-wake append. Arm, content, sweep is the interleaving #55 describes, not an extra
    /// step — and it is the reason the sequence literals in these fixtures run 1, 2, 3 rather
    /// than 1, 2.
    fn append_content(
        events: &std::path::Path,
        scope: &graphhelm_protocols::RepositoryScope,
        stream: &str,
        key: &str,
        next: u64,
    ) {
        let store = crate::commands::event_store(events).unwrap();
        let request = graphhelm_events::PreparedAppend::new(
            scope.clone(),
            OpaqueId::parse(stream).unwrap(),
            next,
            vec![NewEvent::new(
                OpaqueId::parse(key).unwrap(),
                PersistedActor::new(
                    PersistedActorType::System,
                    ActorId::parse("system-test").unwrap(),
                ),
                Sensitivity::Internal,
                content_event(),
                vec![],
                vec![],
            )],
            vec![],
            vec![],
        )
        .unwrap();
        store.append_atomic(&request).unwrap();
    }

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
                matures_in_seconds: None,
            }),
            "arm-r",
            1,
        );
        // Content past the cursor: what makes the lease due, and what the sweep is reacting to.
        append(content_event(), "content-r", 2);
        let captured = vec![(
            captured_by_the_sweep(events, "exec-race", "session-r"),
            WakeConsumeReason::StaleRendezvous,
        )];

        // The rival got there first.
        append(
            EventKind::WakeLeaseConsumed(WakeLeaseConsumed {
                execution_id: OpaqueId::parse("exec-race").unwrap(),
                session_id: OpaqueId::parse("session-r").unwrap(),
                reason: WakeConsumeReason::Rung,
                // A rival written the way history already contains them: no captured arming.
                captured_arming: None,
            }),
            "rival-consume",
            3,
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

    /// The sub-window #55's fix left conflict-UNDETECTED, made deterministic.
    ///
    /// The existing red above covers a rival that got there BEFORE our validation read: the
    /// still-live filter drops us. The CAS covers a rival that lands AFTER our sequence pin:
    /// our expected sequence is stale and the append is refused. Between those two sits a
    /// third window, and nothing guards it — our validation passed against a replay that
    /// predates the rival, and our pin is then taken AFTER it, so the sequence we ask for is
    /// genuinely current and the CAS has nothing to refuse. Both consumptions land, and the
    /// fold then refuses every later replay: #55's own corruption, reached through the door
    /// its fix left open.
    ///
    /// This also settles a claim both surviving commit messages make — that the handle's
    /// exclusive lock spans the validation and the write. It does not; the lock is taken and
    /// released per call, which is why a rival can be injected at the seam at all. If this
    /// test passes before the fix, that reading is wrong and the fix has no premise.
    #[test]
    fn a_rival_consume_between_validation_and_the_sequence_pin_appends_nothing() {
        let directory = tempfile::tempdir().unwrap();
        let events = directory.path();
        let scope = graphhelm_protocols::RepositoryScope::new(
            graphhelm_protocols::WorkspaceId::parse("workspace-w").unwrap(),
            graphhelm_protocols::ProjectId::parse("project-w").unwrap(),
            Some(graphhelm_protocols::ExecutionId::parse("exec-pin").unwrap()),
        );
        let stream = OpaqueId::parse("exec-pin").unwrap();
        let actor = PersistedActor::new(
            PersistedActorType::System,
            ActorId::parse("system-test").unwrap(),
        );

        // The armed lease — live, and still live when our sweep validates it.
        {
            let store = crate::commands::event_store(events).unwrap();
            let request = graphhelm_events::PreparedAppend::new(
                scope.clone(),
                stream.clone(),
                1,
                vec![NewEvent::new(
                    OpaqueId::parse("arm-p").unwrap(),
                    actor.clone(),
                    Sensitivity::Internal,
                    EventKind::WakeLease(WakeLease {
                        execution_id: OpaqueId::parse("exec-pin").unwrap(),
                        session_id: OpaqueId::parse("session-p").unwrap(),
                        cursor: 0,
                        rendezvous_id: OpaqueId::parse("rdv-p").unwrap(),
                        matures_in_seconds: None,
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
        append_content(events, &scope, "exec-pin", "content-p", 2);

        let captured = vec![(
            captured_by_the_sweep(events, "exec-pin", "session-p"),
            WakeConsumeReason::StaleRendezvous,
        )];

        // The rival: another sweep that won the race, burning the same lease in the window
        // between our validation and our pin. It is a LEGITIMATE consumption at the instant
        // it lands — the lease is live and its rendezvous matches — so the test corrupts
        // nothing by hand. Whatever damage appears is the product's own.
        //
        // It opens its OWN handle while the recorder's is still alive, which is safe because
        // a live handle holds no lock between operations: `core/events/tests/read_concurrency`
        // has eight threads doing exactly this at once. The sibling guard above drops its
        // store before calling the recorder, and that drop reads as a requirement — it is not
        // one. Checked rather than inherited, because an unwritten coupling nobody could see
        // is what this whole area cost us.
        let rival = || {
            let store = crate::commands::event_store(events).unwrap();
            let next = store.next_sequence(&scope, "exec-pin").unwrap();
            let request = graphhelm_events::PreparedAppend::new(
                scope.clone(),
                stream.clone(),
                next,
                vec![NewEvent::new(
                    OpaqueId::parse("rival-pin").unwrap(),
                    actor.clone(),
                    Sensitivity::Internal,
                    EventKind::WakeLeaseConsumed(WakeLeaseConsumed {
                        execution_id: OpaqueId::parse("exec-pin").unwrap(),
                        session_id: OpaqueId::parse("session-p").unwrap(),
                        reason: WakeConsumeReason::Rung,
                        captured_arming: None,
                    }),
                    vec![],
                    vec![],
                )],
                vec![],
                vec![],
            )
            .unwrap();
            store.append_atomic(&request).unwrap();
        };

        let recorded = record_consumptions_inner(events, "exec-pin", &captured, &rival);
        assert_eq!(
            recorded, 0,
            "a lease a rival burned after our validation and before our pin must not be \
             consumed a second time"
        );

        // The oracle the live incident failed: one arming, at most one consumption, and the
        // stream still replays.
        let store = crate::commands::event_store(events).unwrap();
        let history = store.read_replay_stream(&scope, "exec-pin").unwrap();
        let replayed = graphhelm_events::replay(&scope, "exec-pin", &history);
        assert!(
            replayed.is_ok(),
            "the stream must remain replayable: {replayed:?}"
        );

        // And the RIVAL's burn is the last word on this session.
        //
        // Without this, the guard rests on `recorded == 0` — which this recorder can return
        // from a dozen places, all but one of them for reasons that have nothing to do with
        // the property. That is fine for a red, which fails today for the stated reason, but
        // the moment the fix lands this becomes a REGRESSION guard, and there it would stay
        // green while the recorder was broken anywhere else at all. The rival's own append is
        // something a recorder that did nothing cannot produce, so asking for it separates
        // "our consumption was correctly refused" from "nothing got that far".
        let receipt = replayed
            .expect("replayable")
            .wake_last_consumed
            .get("session-p")
            .expect("the rival's consumption is on record")
            .sequence;
        assert_eq!(
            receipt, 3,
            "the rival's burn at #3 must be the last consumption on this session — if ours \
             had also landed it would be #4, and if the recorder had bailed early for an \
             unrelated reason there would be no receipt here at all"
        );
    }

    /// The rendezvous half of the filter, which nothing pinned until now.
    ///
    /// A session re-arms: the fold REPLACES its lease, so `session-p` is live again under a
    /// new rendezvous while a sweep still holds a capture naming the old one. Matching on the
    /// session alone would burn the replacement — a lease whose sleeper is waiting, consumed
    /// on the strength of a ring that went to a rendezvous nobody is listening to any more.
    ///
    /// The reason this guard is worth its lines: the fold cannot catch the mistake. A
    /// `wake_lease_consumed` event carries execution, session and reason — no rendezvous — so
    /// a replay sees a consumption of a live lease and accepts it. The recorder's own
    /// comparison is the ONLY thing standing here, and an only-defense with no test is one
    /// refactor away from silently not existing. Sabotage: compare sessions and drop the
    /// rendezvous check; this must fall, and nothing else does.
    ///
    /// The second assertion is the one that measures. `recorded == 0` alone would also hold
    /// if the recorder had refused for some unrelated reason, so it asks the finer question:
    /// is the REPLACEMENT still live afterwards.
    #[test]
    fn a_stale_capture_never_burns_the_lease_that_replaced_it() {
        let directory = tempfile::tempdir().unwrap();
        let events = directory.path();
        let scope = graphhelm_protocols::RepositoryScope::new(
            graphhelm_protocols::WorkspaceId::parse("workspace-w").unwrap(),
            graphhelm_protocols::ProjectId::parse("project-w").unwrap(),
            Some(graphhelm_protocols::ExecutionId::parse("exec-swap").unwrap()),
        );
        let stream = OpaqueId::parse("exec-swap").unwrap();
        let actor = PersistedActor::new(
            PersistedActorType::System,
            ActorId::parse("system-test").unwrap(),
        );

        {
            let store = crate::commands::event_store(events).unwrap();
            let arm = |key: &str, rendezvous: &str, next: u64| {
                let request = graphhelm_events::PreparedAppend::new(
                    scope.clone(),
                    stream.clone(),
                    next,
                    vec![NewEvent::new(
                        OpaqueId::parse(key).unwrap(),
                        actor.clone(),
                        Sensitivity::Internal,
                        EventKind::WakeLease(WakeLease {
                            execution_id: OpaqueId::parse("exec-swap").unwrap(),
                            session_id: OpaqueId::parse("session-p").unwrap(),
                            cursor: 0,
                            rendezvous_id: OpaqueId::parse(rendezvous).unwrap(),
                            matures_in_seconds: None,
                        }),
                        vec![],
                        vec![],
                    )],
                    vec![],
                    vec![],
                )
                .unwrap();
                store.append_atomic(&request).unwrap();
            };
            // The lease a sweep captured.
            arm("arm-old", "rdv-old", 1);
        }
        append_content(events, &scope, "exec-swap", "content-swap", 2);

        // The sweep's capture, taken by phase 1 itself — naming the rendezvous it rang and the
        // arming it saw, both from the same read.
        let stale = vec![(
            captured_by_the_sweep(events, "exec-swap", "session-p"),
            WakeConsumeReason::Rung,
        )];

        // THEN the re-arm that replaced it.
        {
            let store = crate::commands::event_store(events).unwrap();
            let request = graphhelm_events::PreparedAppend::new(
                scope.clone(),
                stream.clone(),
                3,
                vec![NewEvent::new(
                    OpaqueId::parse("arm-new").unwrap(),
                    actor.clone(),
                    Sensitivity::Internal,
                    EventKind::WakeLease(WakeLease {
                        execution_id: OpaqueId::parse("exec-swap").unwrap(),
                        session_id: OpaqueId::parse("session-p").unwrap(),
                        cursor: 0,
                        rendezvous_id: OpaqueId::parse("rdv-new").unwrap(),
                        matures_in_seconds: None,
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

        let recorded = record_consumptions(events, "exec-swap", &stale);
        assert_eq!(
            recorded, 0,
            "a capture naming a rendezvous the session has since replaced must record nothing"
        );

        let store = crate::commands::event_store(events).unwrap();
        let history = store.read_replay_stream(&scope, "exec-swap").unwrap();
        let projection = graphhelm_events::replay(&scope, "exec-swap", &history).unwrap();
        let live = projection
            .wake_leases
            .get("session-p")
            .expect("the re-armed lease is still live — the stale capture must not burn it");
        assert_eq!(
            live.rendezvous_id, "rdv-new",
            "and it is the REPLACEMENT that survived, under its own rendezvous"
        );
    }

    /// A capture from BEFORE the sleeper woke must not burn the lease it armed AFTER it.
    ///
    /// The sequence, and it is the factory's own: a lease is armed on rendezvous X; a sweep
    /// captures it and rings it; the sleeper wakes, works, and re-arms — on the SAME rendezvous,
    /// because our agents use fixed rendezvous ids; then the first sweep's delayed phase 3
    /// finally runs. Session matches. Rendezvous matches. Nothing else is compared, so the stale
    /// consumption burns a lease whose sleeper is asleep on it at that moment.
    ///
    /// Not hypothetical: the archived pair store has session `agente-a` arming `factory-a-1` at
    /// sequences 26, 30, 33 and 39 — four arms, one rendezvous. Precondition present there;
    /// incident not observed, because those arm/consume pairs happen to be ordered.
    ///
    /// Nothing downstream catches it. Consuming a LIVE lease is legal, so the fold accepts it
    /// and every replay succeeds — none of the noise the #55 family makes. The sleeper is simply
    /// never rung again: it blocks to its horizon and `wake-wait` exits 3, which reads as "my
    /// deadline passed and nothing happened", while the store's receipt for that session says
    /// `rung`. Two surfaces, one question, and the operator acts on the calm one.
    ///
    /// ASSERTION ORDER IS DELIBERATE. The legality check comes first because it holds in BOTH
    /// states and is what separates this defect from window 3: that one corrupts the stream and
    /// is loud, this one leaves the log perfectly legal and is silent. Asserted after the count,
    /// it would be masked by the count's own failure on every red run, and the clause that
    /// identifies WHICH defect we have would never be observed.
    ///
    /// That ordering is safe only because nothing in the moved region writes: the replay is a
    /// pure fold over a read, the count is captured before it, and the store open that travels
    /// with them republishes active markers only from `GraphVersionPublished` events — of which
    /// this fixture appends none. IF THIS FIXTURE EVER GAINS A PUBLISHED GRAPH VERSION, that
    /// open becomes a write and the ordering has to be re-checked before it can be trusted.
    #[test]
    fn a_capture_from_before_the_wake_never_burns_the_lease_armed_after_it() {
        let directory = tempfile::tempdir().unwrap();
        let events = directory.path();
        let scope = graphhelm_protocols::RepositoryScope::new(
            graphhelm_protocols::WorkspaceId::parse("workspace-w").unwrap(),
            graphhelm_protocols::ProjectId::parse("project-w").unwrap(),
            Some(graphhelm_protocols::ExecutionId::parse("exec-rearm").unwrap()),
        );
        let stream = OpaqueId::parse("exec-rearm").unwrap();
        let actor = PersistedActor::new(
            PersistedActorType::System,
            ActorId::parse("system-test").unwrap(),
        );

        let arm = |key: &str, cursor: u64, next: u64| {
            let store = crate::commands::event_store(events).unwrap();
            let request = graphhelm_events::PreparedAppend::new(
                scope.clone(),
                stream.clone(),
                next,
                vec![NewEvent::new(
                    OpaqueId::parse(key).unwrap(),
                    actor.clone(),
                    Sensitivity::Internal,
                    EventKind::WakeLease(WakeLease {
                        execution_id: OpaqueId::parse("exec-rearm").unwrap(),
                        session_id: OpaqueId::parse("session-p").unwrap(),
                        cursor,
                        // THE SAME rendezvous both times: the fixed-id convention.
                        rendezvous_id: OpaqueId::parse("rdv-fixed").unwrap(),
                        matures_in_seconds: None,
                    }),
                    vec![],
                    vec![],
                )],
                vec![],
                vec![],
            )
            .unwrap();
            store.append_atomic(&request).unwrap();
        };

        // The lease that exists when the sweep looks, and the content that makes it due.
        arm("arm-before", 0, 1);
        append_content(events, &scope, "exec-rearm", "content-rearm", 2);

        // THE SWEEP CAPTURES HERE — before the sleeper wakes. Taken by phase 1 itself, so a
        // corrupted fold moves this and the live lease TOGETHER just as it would in production.
        // Its ring already crossed, which is why the reason is Rung.
        let stale = vec![(
            captured_by_the_sweep(events, "exec-rearm", "session-p"),
            WakeConsumeReason::Rung,
        )];

        // Now the sleeper wakes and re-arms — same session, same rendezvous, cursor at the
        // content it just read. Both arms land BEFORE the recorder runs: this is the
        // OUT-of-window slice, the one the sequence pin cannot reach. An in-window variant
        // would pass because of that fix and prove nothing about this defect.
        arm("arm-after", 2, 3);

        let recorded = record_consumptions(events, "exec-rearm", &stale);

        let store = crate::commands::event_store(events).unwrap();
        let history = store.read_replay_stream(&scope, "exec-rearm").unwrap();
        let replayed = graphhelm_events::replay(&scope, "exec-rearm", &history);
        assert!(
            replayed.is_ok(),
            "this defect is the SILENT one: burning a live lease is legal, so the log stays \
             replayable. A corrupt stream here would mean the fixture is reproducing window 3 \
             instead, and the shape needs re-deriving rather than relabelling: {replayed:?}"
        );

        assert_eq!(
            recorded, 0,
            "a consumption for the lease that existed before the wake must not be recorded \
             against the lease armed after it"
        );

        // The assertion that measures. `recorded == 0` alone would also hold if the recorder
        // had refused for an unrelated reason; the property is that the sleeper's CURRENT lease
        // survives, because that lease is what a future append will ring.
        assert!(
            replayed
                .expect("replayable")
                .wake_leases
                .contains_key("session-p"),
            "the lease the sleeper is asleep on must still be live — burned here, no later \
             append can ever ring it and the sleeper waits out its full horizon believing \
             nothing happened"
        );
    }

    /// The recorder still records when the lease IS live — the guard filters rivals'
    /// leftovers, never legitimate consumptions.
    #[test]
    fn a_live_lease_consumption_still_records() {
        let directory = tempfile::tempdir().unwrap();
        let events = directory.path();
        let scope = graphhelm_protocols::RepositoryScope::new(
            graphhelm_protocols::WorkspaceId::parse("workspace-w").unwrap(),
            graphhelm_protocols::ProjectId::parse("project-w").unwrap(),
            Some(graphhelm_protocols::ExecutionId::parse("exec-live").unwrap()),
        );
        {
            let store = crate::commands::event_store(events).unwrap();
            let request = graphhelm_events::PreparedAppend::new(
                scope.clone(),
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
                        matures_in_seconds: None,
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
        append_content(events, &scope, "exec-live", "content-l", 2);
        let recorded = record_consumptions(
            events,
            "exec-live",
            &[(
                captured_by_the_sweep(events, "exec-live", "session-l"),
                WakeConsumeReason::Rung,
            )],
        );
        assert_eq!(recorded, 1, "a live lease's consumption records normally");

        // And what it recorded is the RIGHT consumption, not merely one consumption.
        //
        // A count is the only thing this guard used to check, and measurement showed that
        // matters more here than anywhere else in the family: under a sabotage that stopped
        // the recorder dead, this was the ONLY guard in the whole #55 set that noticed. So
        // the chain's entire ability to see the recorder rests on this assertion — and a
        // count can only see a recorder that is DEAD, never one that is WRONG. A recorder
        // that burned this lease with the wrong reason satisfied it completely.
        //
        // Absent-by-key rather than an empty map: a later fixture with a second session would
        // make "empty" false while this lease's burn was still perfectly correct, and the
        // guard would then fail for something that is not the property.
        let store = crate::commands::event_store(events).unwrap();
        let history = store.read_replay_stream(&scope, "exec-live").unwrap();
        let projection = graphhelm_events::replay(&scope, "exec-live", &history).unwrap();
        assert!(
            !projection.wake_leases.contains_key("session-l"),
            "the lease it consumed must be gone from the live set"
        );
        assert_eq!(
            projection
                .wake_last_consumed
                .get("session-l")
                .expect("the consumption is on record")
                .reason,
            WakeConsumeReason::Rung,
            "and the receipt must carry the reason the ring actually earned — a burn recorded \
             as a stale rendezvous would be the store asserting the sleeper died when it was \
             rung, which no count can tell apart"
        );
    }
}
