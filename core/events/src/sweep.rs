//! The customs sweep verb (#162): the ACTION-and-RECORD half of overdue handling.
//!
//! Detection is read-time and needs no caller — `overdue_at` derives lapsed episodes from the
//! journal whenever anyone looks. This module is the other half: the one that WRITES, journalling
//! a `sweep_performed` and minting one `overdue_exception` per lapsed episode IN THE SAME BATCH,
//! so no state exists in which a sweep is recorded and its exceptions are not. Adjacency is what
//! links an exception to its sweep; no field carries the link, so the batch is the link.

use graphhelm_protocols::{
    EventEnvelope, EventKind, NewEvent, OpaqueId, OverdueException, PersistedActor,
    PersistedTimestamp, RepositoryScope, Sensitivity, SweepCaller, SweepPerformed,
};

use crate::store::EventRepositoryError;
use crate::{LocalEventRepository, PreparedAppend, overdue_at, replay};

/// Evaluate the stream's customs stages at `as_of` and journal the result.
///
/// Returns the batch that was appended, so a caller — or a test — OBSERVES the adjacency between
/// the sweep record and its exceptions rather than being told about it. Returning only a count, or
/// nothing, would make the one property that has no field of its own unobservable from outside.
///
/// The verdict is computed by `overdue_at` over the replayed projection and is not recomputed
/// here. A second copy of that arithmetic would be a duplicated ORACLE: the two would diverge in
/// silence, and silent divergence in an oracle changes what "passed" means without anything going
/// red.
///
/// An empty verdict still writes the `sweep_performed`. A sweep that ran and found nothing is a
/// fact worth having: without it, "no exceptions" and "no sweep" are the same absence in the log.
#[allow(clippy::missing_errors_doc)]
pub fn sweep(
    repository: &LocalEventRepository,
    scope: &RepositoryScope,
    stream: &str,
    as_of: &PersistedTimestamp,
    actor: &PersistedActor,
    caller: SweepCaller,
) -> Result<Vec<EventEnvelope>, EventRepositoryError> {
    // ONLY THE PAST IS ASKABLE, and the refusal comes first — before anything is read, computed or
    // written. A sweep does not predict: a future-dated answer would be indistinguishable from a
    // real one in the journal while permanently SPENDING the episodes it touched, so the honest
    // sweep that arrived later would find nothing left to raise. The damage is the marking, not
    // the answer, which is why refusing has to happen before the marking can.
    //
    // The comparison is against the instant an append would carry, read through the store's own
    // clock rather than a second one of ours. `as_of` equal to that instant is allowed: "now" is
    // the last askable moment, not the first unaskable one.
    if as_of > &repository.now()? {
        return Err(EventRepositoryError::Invalid);
    }

    let history = repository.read_replay_stream(scope, stream)?;
    let projection = replay(scope, stream, &history).map_err(|_| EventRepositoryError::Invalid)?;

    let execution_id = projection
        .execution_id
        .as_deref()
        .and_then(|id| OpaqueId::parse(id).ok())
        .ok_or(EventRepositoryError::Invalid)?;

    let overdue = overdue_at(&projection, as_of);

    let at = repository.next_sequence(scope, stream)?;

    // Keys are unique PER CALL, and the sequence supplies the uniqueness without a clock or a
    // generator, so replay stays deterministic.
    //
    // An earlier version derived them from `as_of` alone, reasoning that the same question at the
    // same instant should collapse. It does not: two sweeps at one instant are two events with
    // different content, so the store answered `IdempotencyConflict` and the verb failed instead
    // of recording. The deeper error was locating idempotence in the KEY at all — the sweep that
    // matters runs on a tick, at a NEW instant every time, against the same lapsed episode, and no
    // key derived from the instant can see that. One exception per episode is a property of the
    // FOLD (`exception_marked`), and this line is only about not colliding.
    //
    // `:` is not a legal opaque-id byte and an instant is full of them: replaced rather than
    // dropped, because dropping maps two different instants onto one key.
    let mut events = Vec::with_capacity(overdue.len().saturating_add(1));
    events.push(new_event(
        &format!(
            "sweep-{}-{at}",
            as_of.as_datetime().to_rfc3339().replace(':', "-")
        ),
        actor,
        EventKind::SweepPerformed(SweepPerformed {
            execution_id: execution_id.clone(),
            as_of: as_of.clone(),
            caller,
        }),
    )?);

    for stage in overdue {
        let node_id = OpaqueId::parse(&stage.node).map_err(|_| EventRepositoryError::Invalid)?;
        events.push(new_event(
            &format!("overdue-{}-{at}", stage.episode_seq),
            actor,
            EventKind::OverdueException(OverdueException {
                execution_id: execution_id.clone(),
                node_id,
                episode_sequence: stage.episode_seq,
                stage: if stage.claim_seq.is_some() {
                    graphhelm_protocols::CustomsStage::Claimed
                } else {
                    graphhelm_protocols::CustomsStage::Parked
                },
                deadline: stage.deadline,
            }),
        )?);
    }

    let request = PreparedAppend::new(
        scope.clone(),
        OpaqueId::parse(stream).map_err(|_| EventRepositoryError::Invalid)?,
        at,
        events,
        Vec::new(),
        Vec::new(),
    )?;
    repository.append_atomic(&request)
}

fn new_event(
    key: &str,
    actor: &PersistedActor,
    kind: EventKind,
) -> Result<NewEvent, EventRepositoryError> {
    Ok(NewEvent::new(
        OpaqueId::parse(key).map_err(|_| EventRepositoryError::Invalid)?,
        actor.clone(),
        Sensitivity::Internal,
        kind,
        Vec::new(),
        Vec::new(),
    ))
}
