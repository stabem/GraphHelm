use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    Diagnostic, GraphVersionRecord, NodeState, PolicyObligation, PolicyWaiver, SemanticHash,
    SimulationStatus,
};

/// An event before store-assigned identity, time, stream, and sequence.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewEvent {
    pub idempotency_key: String,
    pub kind: EventKind,
}

/// A fully ordered event envelope returned by an event store.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventEnvelope {
    pub id: String,
    pub stream_id: String,
    pub sequence: u64,
    pub occurred_at: DateTime<Utc>,
    pub idempotency_key: String,
    pub kind: EventKind,
}

/// All event variants required by the Foundation Graph Kernel.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum EventKind {
    GraphImported(GraphImported),
    GraphValidationFailed(GraphValidationFailed),
    GraphVersionPublished(Box<GraphVersionPublished>),
    DraftProposed(DraftProposed),
    DraftRejected(DraftRejected),
    DraftApplied(DraftApplied),
    PolicyObligationEvaluated(PolicyObligationEvaluated),
    PolicyWaiverCreated(PolicyWaiverCreated),
    SimulationStarted(SimulationStarted),
    NodeStateChanged(NodeStateChanged),
    SimulationCompleted(SimulationCompleted),
}

impl EventKind {
    #[must_use]
    pub fn graph_imported(source: impl Into<String>) -> Self {
        Self::GraphImported(GraphImported {
            source: source.into(),
        })
    }

    #[must_use]
    pub fn draft_rejected(draft_id: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::DraftRejected(DraftRejected {
            draft_id: draft_id.into(),
            reason: reason.into(),
            diagnostics: Vec::new(),
        })
    }

    #[must_use]
    pub const fn simulation_started() -> Self {
        Self::SimulationStarted(SimulationStarted {})
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphImported {
    pub source: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphValidationFailed {
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphVersionPublished {
    pub version: GraphVersionRecord,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftProposed {
    pub draft_id: String,
    pub expected_version: u64,
    pub expected_hash: SemanticHash,
    pub operation_count: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftRejected {
    pub draft_id: String,
    pub reason: String,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftApplied {
    pub draft_id: String,
    pub graph_version: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyObligationEvaluated {
    pub draft_id: String,
    pub obligation: PolicyObligation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyWaiverCreated {
    pub waiver: PolicyWaiver,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimulationStarted {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeStateChanged {
    pub node_id: String,
    pub from: Option<NodeState>,
    pub to: NodeState,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimulationCompleted {
    pub status: SimulationStatus,
}
