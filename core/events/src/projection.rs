use std::collections::{BTreeMap, BTreeSet};

use graphhelm_protocols::{
    EventEnvelope, EventKind, GraphVersionRecord, NodeState, PolicyWaiver, SimulationStatus,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Pure replay result used by CLI and recovery checks.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionProjection {
    pub stream_id: Option<String>,
    pub current_graph: Option<GraphVersionRecord>,
    pub proposed_drafts: Vec<String>,
    pub rejected_drafts: Vec<String>,
    pub applied_drafts: Vec<String>,
    pub waivers: Vec<PolicyWaiver>,
    pub node_states: BTreeMap<String, NodeState>,
    pub simulation_status: Option<SimulationStatus>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ReplayError {
    #[error("event stream is corrupt: {0}")]
    Corrupt(String),
}

impl ReplayError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        "GHE002_CORRUPT_BATCH"
    }
}

/// Rebuilds current execution state solely from ordered event envelopes.
pub fn replay(events: &[EventEnvelope]) -> Result<ExecutionProjection, ReplayError> {
    let mut projection = ExecutionProjection::default();
    let mut seen = BTreeMap::new();
    let mut applied_keys = BTreeSet::new();
    for (index, event) in events.iter().enumerate() {
        if event.sequence != index as u64 + 1 {
            return Err(ReplayError::Corrupt("non-contiguous sequence".into()));
        }
        if let Some(stream) = &projection.stream_id {
            if stream != &event.stream_id {
                return Err(ReplayError::Corrupt("mixed execution streams".into()));
            }
        } else {
            projection.stream_id = Some(event.stream_id.clone());
        }
        if let Some(previous) = seen.insert(event.idempotency_key.clone(), &event.kind) {
            if previous != &event.kind {
                return Err(ReplayError::Corrupt(
                    "conflicting duplicate idempotency key".into(),
                ));
            }
            continue;
        }
        if !applied_keys.insert(event.idempotency_key.clone()) {
            continue;
        }
        match &event.kind {
            EventKind::GraphVersionPublished(payload) => {
                projection.current_graph = Some(payload.version.clone());
            }
            EventKind::DraftProposed(payload) => {
                projection.proposed_drafts.push(payload.draft_id.clone());
            }
            EventKind::DraftRejected(payload) => {
                projection.rejected_drafts.push(payload.draft_id.clone());
            }
            EventKind::DraftApplied(payload) => {
                projection.applied_drafts.push(payload.draft_id.clone());
            }
            EventKind::PolicyWaiverCreated(payload) => {
                projection.waivers.push(payload.waiver.clone());
            }
            EventKind::SimulationStarted(_) => {
                projection.simulation_status = Some(SimulationStatus::Running);
            }
            EventKind::NodeStateChanged(payload) => {
                projection
                    .node_states
                    .insert(payload.node_id.clone(), payload.to.clone());
            }
            EventKind::SimulationCompleted(payload) => {
                projection.simulation_status = Some(payload.status.clone());
            }
            EventKind::GraphImported(_)
            | EventKind::GraphValidationFailed(_)
            | EventKind::PolicyObligationEvaluated(_) => {}
        }
    }
    Ok(projection)
}
