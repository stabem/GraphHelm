//! #1560: what a status or list read takes from a stream's raw history, kept as a fold.
//!
//! A status reply needs, besides the projection, a handful of facts only the raw history holds:
//! the first and last content instants, each node's last event instant, the wake leases that
//! armed, the head, and (for a list row) which succeeded results a passing verdict confirmed.
//! Each of those was a walk over the whole history on every read, and the node walk serialized
//! every event's kind: the dominant cost of `GET /v1/executions/{id}` and `GET /v1/executions`
//! on a long stream. They are all folds over the events in order, so serve keeps them as a
//! [`HistoryDigest`] next to the projection generation and advances both over the events
//! appended since the last read. Like the projection it is disposable: dropped, it is rebuilt
//! from the events by the same functions, and a test holds the two readings equal.

use std::collections::BTreeMap;

use graphhelm_events::ExecutionProjection;
use graphhelm_protocols::{EventEnvelope, EventHash, EventKind, NodeState};

/// The facts of one stream's history a status or list read renders, folded in log order.
#[derive(Clone, Debug, Default)]
pub(crate) struct HistoryDigest {
    /// Sequence and hash of the last event folded; `None` while nothing was.
    pub(crate) head: Option<(u64, EventHash)>,
    pub(crate) len: usize,
    first_content_at: Option<chrono::DateTime<chrono::Utc>>,
    last_content_at: Option<chrono::DateTime<chrono::Utc>>,
    node_last_event_at: BTreeMap<String, chrono::DateTime<chrono::Utc>>,
    /// Every `WakeLease` event: its sequence, session id and arming actor id.
    wake_leases: Vec<(u64, String, String)>,
    reviews: ResultReviews,
}

impl HistoryDigest {
    pub(crate) fn of(history: &[EventEnvelope]) -> Self {
        let mut digest = Self::default();
        digest.advance(history);
        digest
    }

    /// Folds `events`, which must continue the history already folded.
    pub(crate) fn advance(&mut self, events: &[EventEnvelope]) {
        for event in events {
            let at = *event.occurred_at.as_datetime();
            if super::wake::is_content(event) {
                self.first_content_at.get_or_insert(at);
                self.last_content_at = Some(at);
            }
            super::note_node_event(&mut self.node_last_event_at, event);
            if let EventKind::WakeLease(lease) = &event.kind {
                self.wake_leases.push((
                    event.sequence,
                    lease.session_id.as_str().to_owned(),
                    event.actor.id().as_str().to_owned(),
                ));
            }
            self.reviews.note(event);
            self.head = Some((event.sequence, event.event_hash.clone()));
            self.len += 1;
        }
    }

    pub(crate) fn at_sequence(&self) -> Option<u64> {
        self.head.as_ref().map(|(sequence, _)| *sequence)
    }

    pub(crate) fn liveness(&self) -> super::Liveness {
        super::Liveness {
            started_at: self.first_content_at,
            last_event_at: self.last_content_at,
            node_last_event_at: self.node_last_event_at.clone(),
        }
    }

    pub(crate) fn node_silence_seconds(
        &self,
        now: chrono::DateTime<chrono::Utc>,
    ) -> BTreeMap<String, u64> {
        super::silence_since(self.node_last_event_at.clone(), now)
    }

    /// The `(sequence, session id, actor id)` of every wake lease the history armed.
    pub(crate) fn wake_leases(&self) -> impl Iterator<Item = (u64, &str, &str)> {
        self.wake_leases
            .iter()
            .map(|(sequence, session, actor)| (*sequence, session.as_str(), actor.as_str()))
    }

    pub(crate) fn unverified_result_count(
        &self,
        projection: &ExecutionProjection,
    ) -> Option<usize> {
        (self.len > 0).then(|| self.reviews.unverified(projection))
    }
}

/// Which node results a passing gate verdict confirmed, folded in log order.
///
/// A successful lifecycle outcome is only an executor result. The Studio's review marker is
/// cleared by a later node outcome and by a later gate verdict, then restored only when the gate
/// passes for a result whose evidence identifies a judge or gate.
#[derive(Clone, Debug, Default)]
pub(crate) struct ResultReviews(BTreeMap<String, ResultReview>);

#[derive(Clone, Debug, Default)]
struct ResultReview {
    succeeded: bool,
    eligible_for_verification: bool,
    verified: bool,
}

impl ResultReviews {
    fn note(&mut self, event: &EventEnvelope) {
        match &event.kind {
            EventKind::NodeOutcomeRecorded(payload) => {
                let node = payload.node_id.to_string();
                let eligible = payload.next_state == NodeState::Succeeded
                    && event.evidence_refs.iter().any(|reference| {
                        let id = reference.evidence_id().as_str();
                        id.ends_with("-judgment") || id.ends_with("-verdict")
                    });
                let review = self.0.entry(node).or_default();
                review.succeeded = payload.next_state == NodeState::Succeeded;
                review.eligible_for_verification = eligible;
                review.verified = false;
            }
            EventKind::GateVerdict(payload) => {
                let review = self.0.entry(payload.node_id.to_string()).or_default();
                review.verified = false;
                if payload.passed && review.succeeded && review.eligible_for_verification {
                    review.verified = true;
                }
            }
            _ => {}
        }
    }

    fn unverified(&self, projection: &ExecutionProjection) -> usize {
        projection
            .node_states
            .iter()
            .filter(|(_, state)| **state == NodeState::Succeeded)
            .filter(|(node, _)| !self.0.get(*node).is_some_and(|review| review.verified))
            .count()
    }
}
