use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};

use crate::{
    ArtifactReference, EventHash, EvidenceId, EvidenceReference, ExecutionMode, NodeOutcome,
    NodeState, OpaqueId, PersistedActor, PersistedDiagnostic, PersistedGraphVersion,
    PersistedTimestamp, PolicyWaiver, RawSha256, RepositoryScope, SemanticVersion, Sensitivity,
    SignalSeverity, SignalSourceKind, SimulationStatus, WireHash,
    persistence::{PersistenceError, deserialize_optional_non_null},
};

/// The only accepted pre-release production event schema.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventSchemaVersion {
    #[serde(rename = "1.0.0")]
    V1,
}

/// Stable, bounded machine code used by replay-safe event payloads.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct SafeCode(String);

impl SafeCode {
    pub fn parse(value: impl Into<String>) -> Result<Self, PersistenceError> {
        let value = value.into();
        let bytes = value.as_bytes();
        if bytes.is_empty()
            || bytes.len() > 64
            || !bytes[0].is_ascii_lowercase()
            || !bytes[1..]
                .iter()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
        {
            return Err(PersistenceError::new("safe code"));
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SafeCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl<'de> Deserialize<'de> for SafeCode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::parse(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// A safe event before repository-assigned identity, timestamp, sequence and hashes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewEvent {
    pub idempotency_key: OpaqueId,
    pub actor: PersistedActor,
    pub sensitivity: Sensitivity,
    pub kind: EventKind,
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceReference>,
    #[serde(default)]
    pub artifact_refs: Vec<ArtifactReference>,
}

impl NewEvent {
    #[must_use]
    pub fn new(
        idempotency_key: OpaqueId,
        actor: PersistedActor,
        sensitivity: Sensitivity,
        kind: EventKind,
        evidence_refs: Vec<EvidenceReference>,
        artifact_refs: Vec<ArtifactReference>,
    ) -> Self {
        Self {
            idempotency_key,
            actor,
            sensitivity,
            kind,
            evidence_refs,
            artifact_refs,
        }
    }
}

/// A fully ordered, hash-chained production event envelope.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EventEnvelope {
    pub schema_version: EventSchemaVersion,
    pub event_id: OpaqueId,
    pub scope: RepositoryScope,
    pub stream_id: OpaqueId,
    pub sequence: u64,
    pub occurred_at: PersistedTimestamp,
    pub idempotency_key: OpaqueId,
    pub actor: PersistedActor,
    pub sensitivity: Sensitivity,
    pub kind: EventKind,
    pub evidence_refs: Vec<EvidenceReference>,
    pub artifact_refs: Vec<ArtifactReference>,
    pub previous_hash: EventHash,
    pub event_hash: EventHash,
}

impl EventEnvelope {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        event_id: OpaqueId,
        scope: RepositoryScope,
        stream_id: OpaqueId,
        sequence: u64,
        occurred_at: PersistedTimestamp,
        event: NewEvent,
        previous_hash: EventHash,
        event_hash: EventHash,
    ) -> Self {
        Self {
            schema_version: EventSchemaVersion::V1,
            event_id,
            scope,
            stream_id,
            sequence,
            occurred_at,
            idempotency_key: event.idempotency_key,
            actor: event.actor,
            sensitivity: event.sensitivity,
            kind: event.kind,
            evidence_refs: event.evidence_refs,
            artifact_refs: event.artifact_refs,
            previous_hash,
            event_hash,
        }
    }
}

/// The closed set of 23 replay-safe production events.
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
    ExecutionStarted(ExecutionStarted),
    ExecutionModeChanged(ExecutionModeChanged),
    NodeOutcomeRecorded(NodeOutcomeRecorded),
    ExecutionCompleted(ExecutionCompleted),
    SignalRecorded(SignalRecorded),
    GhostNodeProposed(GhostNodeProposed),
    MutationAccepted(MutationAccepted),
    IntegrityCheckpointCreated(IntegrityCheckpointCreated),
    EvidenceErasureRequested(EvidenceErasureRequested),
    EvidenceErasureCompleted(EvidenceErasureCompleted),
    EvidenceCiphertextDeleted(EvidenceCiphertextDeleted),
    EvidenceLegalHoldChanged(EvidenceLegalHoldChanged),
}

impl EventKind {
    #[must_use]
    pub const fn is_project_level(&self) -> bool {
        matches!(
            self,
            Self::IntegrityCheckpointCreated(_)
                | Self::EvidenceErasureRequested(_)
                | Self::EvidenceErasureCompleted(_)
                | Self::EvidenceCiphertextDeleted(_)
                | Self::EvidenceLegalHoldChanged(_)
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphSourceKind {
    GraphDocument,
    Generated,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphImported {
    pub source_sha256: RawSha256,
    pub source_kind: GraphSourceKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphValidationFailed {
    pub diagnostics: Vec<PersistedDiagnostic>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphVersionPublished {
    pub version: PersistedGraphVersion,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DraftProposed {
    pub draft_id: OpaqueId,
    pub expected_version: u64,
    pub expected_hash: WireHash,
    pub operation_count: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DraftRejected {
    pub draft_id: OpaqueId,
    pub reason_code: SafeCode,
    pub diagnostics: Vec<PersistedDiagnostic>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_non_null"
    )]
    pub detail_evidence_id: Option<EvidenceId>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DraftApplied {
    pub draft_id: OpaqueId,
    pub graph_version: u64,
    pub graph_hash: WireHash,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersistedObligationStatus {
    Satisfied,
    Unsatisfied,
    Waived,
    Impossible,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PolicyObligationEvaluated {
    pub draft_id: OpaqueId,
    pub requirement_id: OpaqueId,
    pub status: PersistedObligationStatus,
    pub evidence_ids: Vec<EvidenceId>,
    pub reason_code: SafeCode,
    pub overrideable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PolicyWaiverCreated {
    pub waiver: PolicyWaiver,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SimulationStarted {
    pub simulation_id: OpaqueId,
    pub graph_version: u64,
    pub graph_hash: WireHash,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeStateChanged {
    pub simulation_id: OpaqueId,
    pub node_id: OpaqueId,
    pub previous_state: Option<NodeState>,
    pub next_state: NodeState,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SimulationCompleted {
    pub simulation_id: OpaqueId,
    pub status: SimulationStatus,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionStarted {
    pub execution_id: OpaqueId,
    pub graph_version: u64,
    pub graph_hash: WireHash,
    pub mode: ExecutionMode,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionModeChanged {
    pub execution_id: OpaqueId,
    pub previous_mode: Option<ExecutionMode>,
    pub mode: ExecutionMode,
}

/// One reported outcome for one node.
///
/// `next_state` is the decision `graphhelm_execution::apply_transition` produced. It is recorded
/// rather than recomputed here because `core/events` must not depend on `core/execution`; the
/// dependency direction runs the other way.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeOutcomeRecorded {
    pub execution_id: OpaqueId,
    pub node_id: OpaqueId,
    pub outcome: NodeOutcome,
    pub next_state: NodeState,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionCompleted {
    pub execution_id: OpaqueId,
    pub status: SimulationStatus,
}

/// A Graph Signal was validated and recorded.
///
/// No free-form content, per D-036: the description and the raw envelope are externalized as
/// encrypted Evidence and referenced by this event's evidence list; `envelope_sha256` binds this
/// record to those exact bytes. `kind` is the raw type string, preserved even when unrecognized,
/// because the emitting agent is not authoritative and the record is the evidence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SignalRecorded {
    pub execution_id: OpaqueId,
    pub signal_id: OpaqueId,
    pub source_kind: SignalSourceKind,
    pub source_id: OpaqueId,
    pub kind: String,
    pub severity: SignalSeverity,
    pub envelope_sha256: RawSha256,
}

/// The Governor proposed an expansion. The node exists in state `Ghost` from this moment,
/// visible and never scheduled, per decision 5.2. Approval travels as the existing
/// `node_outcome_recorded` with `Approved -> Ready`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GhostNodeProposed {
    pub execution_id: OpaqueId,
    pub node_id: OpaqueId,
    pub draft_id: OpaqueId,
}

/// The Governor accepted a mutation, under the mode in force at acceptance (decision 5.5), and
/// published `graph_version` as the successor (decision 5.1). The version content travels in the
/// existing `graph_version_published` event; this records the governance act itself.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MutationAccepted {
    pub execution_id: OpaqueId,
    pub draft_id: OpaqueId,
    pub mode: ExecutionMode,
    pub graph_version: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersistedAuthenticationTag {
    pub key_id: OpaqueId,
    pub algorithm: PersistedAuthenticationAlgorithm,
    pub tag_sha256: RawSha256,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PersistedAuthenticationAlgorithm {
    #[serde(rename = "hmac-sha256")]
    HmacSha256,
    #[serde(rename = "blake3-keyed")]
    Blake3Keyed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IntegrityCheckpointCreated {
    pub stream_id: OpaqueId,
    pub sequence: u64,
    pub event_hash: EventHash,
    pub repository_format: SemanticVersion,
    pub authentication_tag: PersistedAuthenticationTag,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidencePriorState {
    Available,
    Expired,
    MissingKey,
    IntegrityFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErasurePendingState {
    #[serde(rename = "erasure_pending")]
    ErasurePending,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErasedState {
    #[serde(rename = "erased")]
    Erased,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PendingPriorState {
    #[serde(rename = "erasure_pending")]
    ErasurePending,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvidenceErasureRequested {
    pub evidence_scope: RepositoryScope,
    pub operation_id: OpaqueId,
    pub evidence_id: EvidenceId,
    pub key_handle_id: OpaqueId,
    pub retention_policy_id: OpaqueId,
    pub retention_policy_version: SemanticVersion,
    pub authority: OpaqueId,
    pub reason_code: SafeCode,
    pub prior_state: EvidencePriorState,
    pub state: ErasurePendingState,
    pub requested_at: PersistedTimestamp,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvidenceErasureCompleted {
    pub evidence_scope: RepositoryScope,
    pub operation_id: OpaqueId,
    pub evidence_id: EvidenceId,
    pub key_handle_id: OpaqueId,
    pub retention_policy_id: OpaqueId,
    pub retention_policy_version: SemanticVersion,
    pub authority: OpaqueId,
    pub reason_code: SafeCode,
    pub ciphertext_sha256: RawSha256,
    pub provider_receipt_id: OpaqueId,
    pub provider_epoch: u64,
    pub prior_state: PendingPriorState,
    pub state: ErasedState,
    pub requested_at: PersistedTimestamp,
    pub completed_at: PersistedTimestamp,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvidenceCiphertextDeleted {
    pub evidence_scope: RepositoryScope,
    pub operation_id: OpaqueId,
    pub evidence_id: EvidenceId,
    pub ciphertext_sha256: RawSha256,
    pub deleted_at: PersistedTimestamp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegalHoldState {
    Placed,
    Released,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvidenceLegalHoldChanged {
    pub evidence_scope: RepositoryScope,
    pub hold_id: OpaqueId,
    pub evidence_id: EvidenceId,
    pub authority: OpaqueId,
    pub reason_code: SafeCode,
    pub state: LegalHoldState,
    pub changed_at: PersistedTimestamp,
}
