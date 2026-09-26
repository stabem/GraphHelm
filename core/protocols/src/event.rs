use std::{collections::BTreeMap, fmt};

use serde::{Deserialize, Deserializer, Serialize};

use crate::{
    ActorId, ArtifactReference, CustomsBudgets, EventHash, EvidenceId, EvidenceReference,
    ExecutionMode, FreshnessClass, NodeOutcome, NodeState, NodeType, OpaqueId, PersistedActor,
    PersistedActorType, PersistedDiagnostic, PersistedGraphVersion, PersistedTimestamp,
    PolicyWaiver, RawSha256, RepositoryScope, SemanticVersion, Sensitivity, SignalSeverity,
    SignalSourceKind, SimulationStatus, WireHash,
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

/// The closed vocabulary of refusal reason codes for the customs pipeline.
///
/// TRANSCRIBED, NOT INVENTED. Source: design note #159, §2d (first read at
/// `origin/k-162-dlq-sweep`, before it reached `main`). Anyone re-deriving this list should read
/// that section rather than trust this comment.
///
/// **The source enumerates NINE, and #160 asks for eight.** §2d lists nine names and marks the
/// last as "the ninth added by amendment: J's custody finding, 2e"; §8 of the same document still
/// says "beyond the eight named". The count in §8 was not updated when the amendment landed, so
/// the document disagrees with itself and #160 inherited the older half. The NAMES are the
/// authority here, never the count — which is the same defect this milestone already fixed twice
/// (`assert_eq!(variants.len(), 25)` against a literal), and which the blueprint itself complains
/// about in its own opening section.
///
/// NOTHING PRODUCES THESE YET, and that is deliberate rather than an omission. The command layer
/// that would emit a `completion_refused` — decide-then-append with the sequence pinned from the
/// read, the #74 pattern — is not built. So this is a legal vocabulary with no producer, and a
/// reader must not take its existence as evidence that any refusal path exists. It is frozen here
/// because the blueprint asks the implementation lane to freeze it, and because a vocabulary
/// agreed across four lanes is cheaper to fix before anyone emits than after.
pub const REFUSAL_REASON_CODES: &[&str] = &[
    // A claim naming a wait that has been superseded or already answered.
    "stale_rendezvous",
    // A second claim against a wait that already carries one.
    "duplicate_completion",
    // Fewer evidence kinds presented than the node's `proof_kinds` declares.
    "evidence_budget_unmet",
    // Presented evidence does not hash to what it claims.
    "hash_mismatch",
    // The named wait sequence is not a wait.
    "unknown_wait",
    // The countersigning identity is not in the registry at this sequence.
    "unknown_identity",
    // Clearance arrived after the stage's declared patience ran out.
    "clearance_expired",
    // The node is not parked, so there is nothing to complete.
    "not_waiting",
    // A signature could not be verified — the ninth, added by amendment (blueprint §2e).
    "signature_unverifiable",
];

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

/// The closed set of 26 replay-safe production events.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum EventKind {
    GraphImported(GraphImported),
    GraphValidationFailed(GraphValidationFailed),
    MemoryAdmissionRefused(MemoryAdmissionRefused),
    MemoryPublicationTransitioned(MemoryPublicationTransitioned),
    MemoryRecordSuperseded(MemoryRecordSuperseded),
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
    ExecutionFormDeclared(ExecutionFormDeclared),
    ExecutionFormAmended(ExecutionFormAmended),
    ExecutionModeChanged(ExecutionModeChanged),
    NodeOutcomeRecorded(NodeOutcomeRecorded),
    ExecutionCompleted(ExecutionCompleted),
    SignalRecorded(SignalRecorded),
    GhostNodeProposed(GhostNodeProposed),
    MutationAccepted(MutationAccepted),
    ExecutionPaused(ExecutionPaused),
    ExecutionResumed(ExecutionResumed),
    IntegrityCheckpointCreated(IntegrityCheckpointCreated),
    EvidenceErasureRequested(EvidenceErasureRequested),
    EvidenceErasureCompleted(EvidenceErasureCompleted),
    EvidenceCiphertextDeleted(EvidenceCiphertextDeleted),
    EvidenceLegalHoldChanged(EvidenceLegalHoldChanged),
    ReuseDecision(ReuseDecision),
    WakeLease(WakeLease),
    WakeLeaseConsumed(WakeLeaseConsumed),
    GateVerdict(GateVerdict),
    GateCertified(GateCertified),
    CompletionClaimed(CompletionClaimed),
    CompletionCleared(CompletionCleared),
    CompletionRejected(CompletionRejected),
    CompletionRefused(CompletionRefused),
    ClearanceIdentityRegistered(ClearanceIdentityRegistered),
    ClearanceIdentityRevoked(ClearanceIdentityRevoked),
    DlqRouted(DlqRouted),
    DlqRedrive(DlqRedrive),
    DlqReturned(DlqReturned),
    SweepPerformed(SweepPerformed),
    OverdueException(OverdueException),
    AgentPresenceDeclared(AgentPresenceDeclared),
}

// ---------------------------------------------------------------------------------------------
// M11 #160: the customs family — a completion is TESTIMONY, clearance is the countersignature.
//
// The one rule that governs every deadline in this family: a stage's deadline is the
// `occurred_at` of the event that ENTERED the stage plus a duration DECLARED ON THE NODE SPEC.
// Durations ride the spec, instants live in the projection, and the fold computes one from the
// other using the envelope's own recorded instant — the M09 `matures_in_seconds` discipline,
// generalized to every stage so no customs state is exempt from the sweep. Nothing in this
// family reads a clock: `sweep`'s `as_of` is an ARGUMENT and every other instant is journal data.
// ---------------------------------------------------------------------------------------------

/// One piece of testimony offered with a completion claim.
///
/// The hash is the evidence's identity; the fold never opens the bytes. `kind` is matched against
/// the node's DECLARED `proof_kinds` list — fewer kinds than declared refuses
/// (`EvidenceBudgetUnmet`), extra kinds are accepted and marked `unverified_extra` in the fold:
/// logged, never counted as stronger proof.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClaimEvidence {
    pub kind: String,
    pub content_hash: WireHash,
    pub size: u64,
}

/// Who asserts a claim, and on what authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimAttestationMode {
    /// A human asserted this completion. Weaker than machine verification and recorded as such.
    OperatorAttested,
    /// A machine re-derived the evidence against the node's manifest.
    MachineVerified,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClaimAttestation {
    pub asserter: OpaqueId,
    pub mode: ClaimAttestationMode,
}

/// M11 #160: a claim that a waiting node's input has arrived — EVIDENCE-BEARING TESTIMONY, and
/// nothing more. A claim alone NEVER releases a dependent: it moves the node into a claimed
/// (quarantined) stage whose only exits are clearance, rejection, or the sweep. That separation
/// is the milestone's thesis, and `the_downstream_of_a_claimed_wait_is_not_ready` is its guard.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompletionClaimed {
    pub execution_id: OpaqueId,
    pub node: OpaqueId,
    /// Envelope sequence of the EXACT open wait this answers — the rendezvous.
    ///
    /// Sequence, not name: a node re-parks (`WaitingInput` on `NeedsInput`, the state machine's
    /// own arm), so names and node ids repeat across waits while sequences cannot. This is
    /// M09's `armed_at_sequence` decision applied to the same problem one layer over: a claim
    /// naming a SUPERSEDED wait is refused with `StaleRendezvous` rather than silently answering
    /// whichever wait happens to be open.
    pub completes_wait_seq: u64,
    pub evidence: Vec<ClaimEvidence>,
    pub attestation: ClaimAttestation,
}

/// Who countersigned a claim.
// `rename_all` renames the VARIANTS; the fields INSIDE a struct variant keep their Rust
// spelling unless `rename_all_fields` says otherwise. Without the second attribute this enum
// put `manifest_hash` and `key_fingerprint` on the wire in snake_case while every other
// payload in this file is camelCase — a silent escape from the convention that no amount of
// reading the type would show, because the attribute that looks like it covers fields does
// not. Caught by the store refusing the event against `event-envelope.schema.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "type"
)]
pub enum ClearanceVerifier {
    /// Clearance re-derived the evidence bundle against the node's declared manifest —
    /// deterministic and replayable.
    MachineReplay { manifest_hash: WireHash },
    /// A declared identity countersigned. Membership is checked by the fold against the
    /// registry; the cryptographic verification itself happens at append time, where evidence
    /// sealing already lives.
    Countersign {
        identity: OpaqueId,
        key_fingerprint: WireHash,
    },
}

/// M11 #160: the countersignature — the ONLY event in this family that releases a dependent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompletionCleared {
    pub execution_id: OpaqueId,
    /// Envelope sequence of the CLAIM this countersigns — same identity discipline as the claim's
    /// own `completes_wait_seq`.
    pub claim_seq: u64,
    pub verifier: ClearanceVerifier,
}

/// M11 #160: clearance withheld, with a registry reason. The node stays parked; the claim is
/// spent. Recording the rejection rather than refusing the append is the M09 rule: a faithfully
/// recorded mistake is journal data, and only an uninterpretable journal is fold-Corrupt.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompletionRejected {
    pub execution_id: OpaqueId,
    pub claim_seq: u64,
    pub verifier: ClearanceVerifier,
    /// `SafeCode`, not `String`, and the difference is not cosmetic: this value is supplied by
    /// whoever withholds clearance and is rendered to operators. `SafeCode` bounds it to 64 bytes
    /// of `^[a-z][a-z0-9_]{0,63}$` — the grammar every other reason code in this file already
    /// uses. A bare `String` accepted arbitrary length and arbitrary bytes on a field that
    /// reaches a screen, which is the shape `scan_safe_value` exists to refuse.
    pub reason_code: SafeCode,
}

/// M11 #160: a claim the command layer would not accept, recorded so a graph that cannot finish
/// leaves a legible trail. The refusal is the OUTCOME of a claim attempt, not an error swallowed
/// at the boundary — an idempotent retry of a refused claim replays this event rather than
/// fabricating a success (#83's stored-outcome lesson).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompletionRefused {
    pub execution_id: OpaqueId,
    pub node: OpaqueId,
    /// The wait the refused claim NAMED — frequently stale, which is frequently the reason.
    ///
    /// SENTINEL CONVENTION: when the claim named no wait and the node had none open, this equals
    /// the refusal's own envelope sequence. No wait can carry that number, because a wait's
    /// identity is the sequence of the envelope that MINTED it — a park (`node_outcome_recorded`
    /// into `waiting_input`) or a `dlq_returned`, which opens a new episode keyed by its own
    /// sequence — and this envelope is a `completion_refused`, which mints none. So a reader
    /// cannot mistake the record for a rendezvous. The field stays a positive sequence either way
    /// (#159, D7: no schema change).
    pub claimed_wait_seq: u64,
    /// The registry code naming WHY the command layer would not accept the claim. `SafeCode` for
    /// the same reason as on `CompletionRejected`: house grammar, bounded, and the spelling every
    /// sibling event already uses.
    pub reason_code: SafeCode,
}

// ONE SOURCE, TWO PRODUCTS (#160, added after the design — see the PR body).
//
// `wire_name` and `EVERY_WIRE_NAME` used to be two hand-written artifacts that had to agree by
// discipline. They drifted, and the drift was invisible in the worst way: a name missing from BOTH
// is absent from both sides of the coverage check's `difference()`, so it can never appear in
// `missing`. That is why a real failure on main named thirteen variants and not fifteen — the two
// newest were invisible to the check meant to catch exactly them.
//
// This macro emits both from one list, so they cannot disagree. The match stays exhaustive, so a
// variant nobody names here is still a COMPILE ERROR — which is the half that already proved
// itself by refusing to build main.
macro_rules! wire_names {
    ($($variant:ident => $name:literal),+ $(,)?) => {
        impl EventKind {
            /// Every wire name this enum can produce, in declaration order.
            ///
            /// Derived from the same list as [`EventKind::wire_name`], so the two cannot drift.
            pub const EVERY_WIRE_NAME: &'static [&'static str] = &[$($name),+];

            /// This variant's serde tag — the `type` string it carries on the wire.
            ///
            /// Exhaustive by construction: a variant absent from the list above fails to compile.
            #[must_use]
            pub const fn wire_name(&self) -> &'static str {
                match self { $(Self::$variant(..) => $name),+ }
            }
        }
    };
}

wire_names! {
    GraphImported => "graph_imported",
    GraphValidationFailed => "graph_validation_failed",
    MemoryAdmissionRefused => "memory_admission_refused",
    MemoryPublicationTransitioned => "memory_publication_transitioned",
    MemoryRecordSuperseded => "memory_record_superseded",
    GraphVersionPublished => "graph_version_published",
    DraftProposed => "draft_proposed",
    DraftRejected => "draft_rejected",
    DraftApplied => "draft_applied",
    PolicyObligationEvaluated => "policy_obligation_evaluated",
    PolicyWaiverCreated => "policy_waiver_created",
    SimulationStarted => "simulation_started",
    NodeStateChanged => "node_state_changed",
    SimulationCompleted => "simulation_completed",
    ExecutionStarted => "execution_started",
    ExecutionFormDeclared => "execution_form_declared",
    ExecutionFormAmended => "execution_form_amended",
    ExecutionModeChanged => "execution_mode_changed",
    NodeOutcomeRecorded => "node_outcome_recorded",
    ExecutionCompleted => "execution_completed",
    SignalRecorded => "signal_recorded",
    GhostNodeProposed => "ghost_node_proposed",
    MutationAccepted => "mutation_accepted",
    ExecutionPaused => "execution_paused",
    ExecutionResumed => "execution_resumed",
    IntegrityCheckpointCreated => "integrity_checkpoint_created",
    EvidenceErasureRequested => "evidence_erasure_requested",
    EvidenceErasureCompleted => "evidence_erasure_completed",
    EvidenceCiphertextDeleted => "evidence_ciphertext_deleted",
    EvidenceLegalHoldChanged => "evidence_legal_hold_changed",
    ReuseDecision => "reuse_decision",
    WakeLease => "wake_lease",
    WakeLeaseConsumed => "wake_lease_consumed",
    GateVerdict => "gate_verdict",
    GateCertified => "gate_certified",
    CompletionClaimed => "completion_claimed",
    CompletionCleared => "completion_cleared",
    CompletionRejected => "completion_rejected",
    CompletionRefused => "completion_refused",
    ClearanceIdentityRegistered => "clearance_identity_registered",
    ClearanceIdentityRevoked => "clearance_identity_revoked",
    DlqRouted => "dlq_routed",
    DlqRedrive => "dlq_redrive",
    DlqReturned => "dlq_returned",
    SweepPerformed => "sweep_performed",
    OverdueException => "overdue_exception",
    AgentPresenceDeclared => "agent_presence_declared",
}

impl EventKind {
    #[must_use]
    pub const fn is_project_level(&self) -> bool {
        matches!(
            self,
            Self::IntegrityCheckpointCreated(_)
                | Self::MemoryAdmissionRefused(_)
                | Self::MemoryPublicationTransitioned(_)
                | Self::MemoryRecordSuperseded(_)
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

/// A refused durable-memory admission.
///
/// Deliberately carries only bounded metadata. Rejected content and any digest of that content
/// are forbidden because this event is immutable once appended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryAdmissionRefusalCode {
    ScopeMismatch,
    RecaptureLoop,
    SecretDetected,
    SelfValidated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryAdmissionLocal {
    Content,
    Scope,
    Validators,
    Origin,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryAdmissionRefused {
    pub code: MemoryAdmissionRefusalCode,
    pub local: MemoryAdmissionLocal,
    pub bytes: u64,
}

/// A memory record's PUBLICATION axis moved. Never the semantic axis (ADR-032 decision 3) --
/// there is no field here for it, on purpose, the same way `MemoryAdmissionRefused` carries no
/// field for content it must never persist.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersistedMemoryPublicationTransition {
    Propose,
    Publish,
    Withdraw,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersistedMemoryPublicationState {
    Unpublished,
    Proposed,
    Published,
    Withdrawn,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryPublicationTransitioned {
    pub record_id: OpaqueId,
    pub transition: PersistedMemoryPublicationTransition,
    pub resulting_state: PersistedMemoryPublicationState,
}

/// Whether a memory record's content is still believed. Never touched by
/// [`MemoryPublicationTransitioned`] -- this is the axis ADR-032 decision 3 keeps independent of
/// publication, and the only durable event that moves it is [`MemoryRecordSuperseded`], for the
/// PREDECESSOR side of a supersession, never as a standalone transition of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersistedMemorySemanticState {
    Candidate,
    Validated,
    Contradicted,
    Deprecated,
    Expired,
}

/// Why a predecessor is superseded. Closed to exactly the two values ADR-032 names -- there is no
/// generic "superseded" reason, because the reason is exactly what a caller must supply and this
/// event must never infer from the relationship existing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersistedSupersessionReason {
    Contradicted,
    Deprecated,
}

/// A memory record supersedes another. Moves the PREDECESSOR's semantic axis to the reason's
/// target and records the relationship on the SUCCESSOR -- a fact about two records, never a
/// transition on one. Neither record's publication axis is touched: superseding is a
/// semantic-axis fact, orthogonal to [`MemoryPublicationTransitioned`].
///
/// `predecessor_new_semantic_state` is carried explicitly rather than re-derived from `reason` at
/// read time, the same reason `MemoryPublicationTransitioned` carries `resulting_state` alongside
/// `transition`: the persisted fact must not depend on the CURRENT code's mapping from reason to
/// state remaining unchanged forever. If that mapping ever changes, this event still says what
/// actually happened when it was appended.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryRecordSuperseded {
    pub predecessor_id: OpaqueId,
    pub successor_id: OpaqueId,
    pub reason: PersistedSupersessionReason,
    pub predecessor_new_semantic_state: PersistedMemorySemanticState,
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

/// The shape the operator DECLARED for this execution, recorded without a seal.
///
/// Separate from `GraphVersionPublished` on purpose: a seal exists to protect the evidence
/// behind content slots, and a declared shape — the node set and the deadlines — carries no
/// evidence to seal. Fusing the two concepts into one type is what forced every rule that
/// needs the shape to demand a credential it has no use for.
///
/// `node_ids` is the completeness set: it is what lets a rule ask "has everything this graph
/// declares been accounted for" without a published topology. `node_timeout_seconds` holds an
/// entry ONLY for a node that declared one, so an absent key means the operator declared
/// nothing — never a budget of zero.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionFormDeclared {
    pub execution_id: OpaqueId,
    pub node_ids: Vec<OpaqueId>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub node_descriptors: BTreeMap<OpaqueId, NodeDescriptor>,
    /// The graph's connections as they stood when this run started. Optional for old journals;
    /// its hash must match the atomic `ExecutionStarted` before a reader draws an edge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topology: Option<DeclaredTopology>,
    pub node_timeout_seconds: BTreeMap<OpaqueId, u64>,
    /// The graph document's `metadata.name` (#1063): what an authored graph calls itself, and
    /// where a synthesized graph puts the goal it was compiled from.
    ///
    /// Optional and `skip_serializing_if`, for the reason `NodeOutcomeRecorded::reason` gives:
    /// replay recomputes every stored envelope's hash from its re-serialized bytes, so a field
    /// that emitted `null` for histories written before it existed would break their chains.
    /// Absent means "written before the briefing existed", never "unnamed".
    ///
    /// Bounded by [`MAX_DECLARED_OBJECTIVE_CHARS`] and truncated at that bound on a char boundary
    /// rather than refused: a briefing with a truncated name is better than a start refused
    /// over a long one. See [`bound_declared_text`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The operator's request in their own words (#1063): the `objective` of the FIRST node in
    /// `spec.entrypoints` order, which is where the Studio's draft puts the words the operator
    /// typed (its `metadata.name` is a placeholder). Same optionality, same bound, same
    /// truncation rule as [`ExecutionFormDeclared::name`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub objective: Option<String>,
    /// Who was going to run the nodes when the operator started this execution (#1063), so a
    /// harness picking the run up later knows whether the outcomes it reads came from fixtures
    /// or from a gateway. Optional for the same hash-chain reason as the two fields above.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executor: Option<DeclaredExecutor>,
    /// Each node's declared customs budgets, snapshotted at start (#1184 review BLOCK).
    ///
    /// THE DECLARATION CARRIES ITS OWN DEADLINES, and that is the whole reason this field exists
    /// rather than a read-through to the graph. `stage_deadline` computes a customs horizon from
    /// `current_graph`, which only a sealed `GraphVersionPublished` fills; the ordinary `start`
    /// path publishes no graph version, so a node that parked on that path had a wait with NO
    /// deadline — `waitWithinSeconds`, required by its own schema, produced nothing and the sweep
    /// could never call the wait overdue. Populating `current_graph` from a declaration was the
    /// other way to close it and was rejected: a declaration is not a publication, and the fold
    /// has a cell that says so.
    ///
    /// AN ENTRY EXISTS ONLY FOR A NODE THAT DECLARED CUSTOMS, exactly as `node_timeout_seconds`
    /// above holds only nodes that declared a deadline. An absent key means the operator declared
    /// nothing and must never be read as budgets of zero — the trap `stage_deadline`'s own doc
    /// names, where a horizon at the instant of entry makes every stage instantly overdue.
    ///
    /// Optional and `skip_serializing_if`, for the hash-chain reason the three fields above give:
    /// a history written before this field existed must re-serialize to the same bytes. An old
    /// history decodes with an empty map, which reads as "nobody bounded these stages" — the same
    /// answer it gave before, and the honest one.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub node_customs_budgets: BTreeMap<OpaqueId, CustomsBudgets>,
}

/// A bounded, presentation-only snapshot of the run's graph connections. It is a declaration,
/// not a published Graph Version or a source of operational mutations.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeclaredTopology {
    pub graph_hash: WireHash,
    pub entrypoints: Vec<String>,
    pub edges: Vec<DeclaredTopologyEdge>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeclaredTopologyEdge {
    pub id: String,
    pub from: String,
    pub to: String,
    #[serde(rename = "type")]
    pub edge_type: String,
}

/// The upper bound, in characters, on [`ExecutionFormDeclared::name`] and
/// [`ExecutionFormDeclared::objective`]. Mirrored by `maxLength` in
/// `schemas/event-envelope.schema.json`, which counts code points as this does.
pub const MAX_DECLARED_OBJECTIVE_CHARS: usize = 2000;

/// Bound a declared name or objective to [`MAX_DECLARED_OBJECTIVE_CHARS`], cutting on a char
/// boundary so the result is always valid UTF-8 and never a refused start. An empty or
/// whitespace-only text is `None`: a briefing must not carry a blank where "not recorded" is
/// the honest answer.
#[must_use]
pub fn bound_declared_text(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.chars().take(MAX_DECLARED_OBJECTIVE_CHARS).collect())
}

/// Which executor an `execution start` declared it would drive the nodes with (#1063).
///
/// Recorded on the declared form rather than inferred later from the outcomes: a
/// `FixtureScripted` reason on one node says what produced THAT outcome, not what the operator
/// set the run up with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeclaredExecutor {
    /// Outcomes come from a fixtures file; no provider is called.
    Fixture,
    /// Outcomes come from the gateway's routed providers and tools.
    Gateway,
}

/// A bound the operator declared AFTER the run began, valid from its own sequence FORWARD.
///
/// Appended beside `ExecutionFormDeclared`, never replacing it: the fold takes the latest
/// amendment up to the sequence being replayed, so a replay positioned earlier still answers
/// with what was known THEN. The timeline reads "not judged for twelve minutes, judged from
/// here" -- an amendment is a declaration, never an eraser, and nothing an operator types can
/// make the history claim it knew something it did not.
///
/// `observed_silence_seconds` records what the operator was looking at when they decided, so
/// a later reader can see the decision in its own light rather than in hindsight's.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionFormAmended {
    pub execution_id: OpaqueId,
    /// The frontier this amendment was computed against. An amendment that cannot be placed
    /// in the history is refusable, and refusing needs the number it claimed.
    pub computed_at_sequence: u64,
    pub node_timeout_seconds: BTreeMap<OpaqueId, u64>,
    pub observed_silence_seconds: BTreeMap<OpaqueId, u64>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executor: Option<AttemptExecutor>,
    /// WHY the outcome was what it was (M07 F3), from the closed vocabulary below.
    ///
    /// `skip_serializing_if` is REQUIRED here and is not a style choice: replay
    /// re-serializes each deserialized envelope and recomputes its hash against the stored
    /// one (`core/events/src/projection.rs`). An always-emitted `"reason": null` would
    /// change the bytes of every event written before this milestone and break the hash
    /// chain of every existing stream — the committed acceptance evidence included. Absent
    /// therefore keeps meaning "written before causes existed", and the schema leaves the
    /// key out of `required` for exactly the same reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<NodeOutcomeReason>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeDescriptor {
    pub name: String,
    pub role: NodeType,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptExecutorKind {
    Fixture,
    Model,
    Tool,
    Gate,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttemptExecutor {
    pub kind: AttemptExecutorKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_id: Option<String>,
}

/// The closed cause vocabulary for a node outcome (M07 F3).
///
/// Deliberately an enum and never free text. The durable stream must not carry provider
/// prose, paths or anything a secret could ride in on; `GatewayError` is itself a
/// field-free enum of static classes, so naming the class is lossless for triage while the
/// human-readable material goes to Evidence under D-036. Successes carry no reason at all:
/// a cause on a success is noise, and noise is what the judge's finding is ultimately about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeOutcomeReason {
    // The §17 route classes, named one-for-one with `graphhelm_gateway::taxonomy`.
    AuthRequired,
    AuthRevoked,
    QuotaExhausted,
    RateLimited,
    ProviderUnavailable,
    ModelRemoved,
    ContextTooLarge,
    MalformedOutput,
    RuntimeCrashed,
    UnsupportedCapability,
    PolicyDenied,
    Cancelled,
    Timeout,
    /// The provider answered with nothing — distinct from answering unusably.
    EmptyReply,
    /// A judge replied outside the verdict contract: the deliverable was never judged.
    MalformedJudgment,
    /// A judge returned a well-formed refusal: the deliverable failed judgment.
    JudgeRefused,
    /// A deterministic gate refused the delivery.
    GateRefused,
    /// The tool ran to completion with a non-zero exit code.
    ToolExitedNonZero,
    /// The deadline killed the tool's child process.
    ToolTimedOut,
    /// The lease refused the call (`GatewayError::ToolDenied` and the broker's own refusal
    /// share this name because they are the same fact to an operator).
    ToolDenied,
    /// The host failed around the tool rather than the tool itself.
    ToolHostError,
    /// The outcome came from a simulation fixture, not from real work. Recorded rather
    /// than left blank so a scripted failure can never be misread as a provider defect
    /// during triage — the fixture IS the cause, and saying so costs one word.
    FixtureScripted,
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

/// The owner paused the execution, per D-019 and §12's graceful pause: nodes not yet started are
/// held; nothing in flight is interrupted in this milestone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionPaused {
    pub execution_id: OpaqueId,
}

/// The owner resumed a paused execution. The resume preconditions in `graphhelm_execution` gate
/// whether this may be appended; the fold only checks it is coherent history.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionResumed {
    pub execution_id: OpaqueId,
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

/// Which reuse layer decided. Closed; `ToolBroker` is the only variant this milestone — the
/// discriminator exists because identity is the one thing a wire kind cannot cheaply retrofit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReusePlane {
    ToolBroker,
}

/// What the reuse layer decided for one call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReuseOutcome {
    Hit,
    Miss,
    ForcedFresh,
    Excluded,
}

/// Why a cache-eligible call was forced fresh. Present iff the decision is `ForcedFresh`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForcedFreshReason {
    DirtyTree,
    OperatorForced,
}

/// One declared component of a persisted cache key — the `SYSTEM_ARCHITECTURE.md` section-7.2
/// dependency-hash subset, typed, never free strings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReuseKeyComponent {
    ToolVersion,
    CanonicalInput,
    LeaseScope,
    SourceSnapshot,
}

/// The reuse ledger: one record per reuse-layer decision (05c amendment, D-037 ritual). No
/// producer appends it in Milestone 05c — the broker CLI appends nothing to any store; the 05d
/// executor is the producer, exactly as the `NodeExecutor` seam shipped in 04a before its
/// implementor. Cost fields are deliberately absent: a unit-less cost on the wire is the
/// retrofit trap inverted, so they arrive additively WITH a unit discriminator once the
/// gateway's graded unit exists (spec-debt queue, issue #35). Ledger, not state: the fold's arm
/// for this kind changes no node state and no counter.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReuseDecision {
    pub execution_id: OpaqueId,
    /// `None` from a standalone broker call; `Some` under the 05d executor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<OpaqueId>,
    pub plane: ReusePlane,
    pub decision: ReuseOutcome,
    /// Required iff `decision == ForcedFresh` (validated by the producer; the fold checks
    /// coherence only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forced_reason: Option<ForcedFreshReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freshness_class: Option<FreshnessClass>,
    pub key_components: Vec<ReuseKeyComponent>,
    /// Digest of the composed key: auditable and joinable, content-free.
    pub key_digest: WireHash,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_ref: Option<EvidenceId>,
    pub provenance_erased: bool,
}

/// 05g Task 1 (the wake doorbell, D-037): a sleeper ARMS ITS OWN WAIT by depositing this
/// lease — its session, its last-seen cursor, and an OPAQUE rendezvous identity. Never a
/// filesystem path: both sides derive the platform rendezvous under a fixed local prefix
/// from the id, so a hostile lease can never aim the serve process at an arbitrary path.
/// The fold holds AT MOST ONE live lease per session (arming again replaces — the waker can
/// never schedule the sleeper into a loop); the signal chooses WHEN, never WHAT.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WakeLease {
    pub execution_id: OpaqueId,
    /// The sleeper's identity — the key of the one-live-lease invariant.
    pub session_id: OpaqueId,
    /// The sleeper's last-seen sequence: only an append BEYOND it may ring.
    pub cursor: u64,
    /// Opaque rendezvous identity (never a path).
    pub rendezvous_id: OpaqueId,
    /// M09 decision B: how long this sleeper's quiet may last, in seconds, declared at arming
    /// by the only party who is provably awake and consenting at that moment.
    ///
    /// A DURATION on the wire and an INSTANT in the projection, and the boundary between them
    /// is the point. The horizon is `occurred_at + this`, computed by the fold from the
    /// event's OWN recorded instant — so it is a pure function of the log, replay stays
    /// byte-identical, and no reader ever adds a duration to a clock of its own. Carrying the
    /// instant on the wire instead would have meant computing it from a second clock reading
    /// microseconds away from the one that stamped the event: two clocks answering "when did
    /// this happen", which is the defect this decision exists to remove, in miniature.
    ///
    /// Absent means absent: a lease armed without one gets no expiry promise, and no implicit
    /// horizon is ever invented for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matures_in_seconds: Option<u64>,
}

/// The lease's end — appended by the ringer as its own follow-up append AFTER the ring
/// attempt (the true reason, `rung` vs `stale_rendezvous`, is only knowable once the byte
/// was tried — the 05g Task 2 two-phase reality; a crash between trigger and consumption
/// leaves at worst a spurious content-free wake the sleeper's own re-read absorbs). A consumption with no matching live lease is a replay integrity
/// refusal: history cannot burn a lease that was never armed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WakeLeaseConsumed {
    pub execution_id: OpaqueId,
    pub session_id: OpaqueId,
    pub reason: WakeConsumeReason,
    /// The arming the sweep CAPTURED — the sequence of the `wake_lease` event it read — not the
    /// arming that happens to be live when this is written.
    ///
    /// WHICH SIDE THIS RECORDS IS THE WHOLE POINT, and the two are indistinguishable in a diff.
    /// The defect this exists for is a MISMATCH between what the sweep captured and what was
    /// live when it recorded, and replay already knows the live side — it rebuilds it from the
    /// arming events. Recording the live one would compare a value against itself: always
    /// equal, a check that cannot fail. Only the captured side is unknown to the log.
    ///
    /// OMITTED when absent, never null: replay re-hashes every event, so emitting a null for
    /// consumptions committed before this field existed would break the chain of every one of
    /// them. Absent stays absent on the wire, as it does in the store.
    ///
    /// Absent is also the permanent state of all committed history, which is why no analysis
    /// can ever decide whether this defect fired in the past — the captured side was never
    /// written down. Going forward it is what makes the question answerable at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub captured_arming: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WakeConsumeReason {
    Rung,
    StaleRendezvous,
}

/// The customs-holding stage an episode was in when it was measured against its deadline.
///
/// Carried on `overdue_exception` so a later reader can tell WHICH promise lapsed without
/// re-deriving it from a graph version that may since have been superseded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CustomsStage {
    Parked,
    Claimed,
    DeadLettered,
}

/// Who asked for a sweep. #162 decision 1b promised a background actor a reader can SEE: the
/// serve tick and an operator's explicit verb are the same operation, and only this field
/// distinguishes them in the log.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SweepCaller {
    Tick,
    Operator,
}

/// A node routed to the declared dead-letter node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DlqRouted {
    pub execution_id: OpaqueId,
    pub node_id: OpaqueId,
    /// The stage entry this routing CLOSES — the envelope sequence at which the node entered
    /// the state it is leaving. Captured, never re-derived: the `WakeLeaseConsumed` rule, which
    /// records the side the log does not already know.
    pub episode_sequence: u64,
    pub reason: SafeCode,
}

/// A dead-lettered node put back into play.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DlqRedrive {
    pub execution_id: OpaqueId,
    pub node_id: OpaqueId,
    /// The dead-letter episode being redriven. The NEW episode's identity is this event's own
    /// sequence and is therefore ABSENT here on purpose — carrying it would let a caller assert
    /// an identity the log can contradict.
    pub dlq_episode_sequence: u64,
}

/// A dead-lettered node returned to its wait, which REOPENS the wait as a new episode.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DlqReturned {
    pub execution_id: OpaqueId,
    pub node_id: OpaqueId,
    pub dlq_episode_sequence: u64,
    /// The reopened wait's budget, in seconds, anchored to THIS event: the fold computes the
    /// deadline from this envelope's own `occurred_at`, the `matures_in_seconds` shape.
    ///
    /// Absent means ABSENT — never zero, never a default. A node returned without a declared
    /// budget gets no deadline, forever. `skip_serializing_if` rather than a nullable required
    /// key, deliberately: an absent key says nobody declared a budget, while a present `null`
    /// would say a budget WAS declared and is nothing, which is a claim no caller made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait_within_seconds: Option<u64>,
}

/// A sweep that ran, recording the instant it was asked to evaluate AT.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SweepPerformed {
    pub execution_id: OpaqueId,
    /// An INSTANT on the wire, and this is the one place the `matures_in_seconds` duration rule
    /// does NOT apply. That rule exists so no reader invents an instant from a clock of its own,
    /// and it takes the shape of a duration where the value is a BUDGET, anchored to its own
    /// event by definition. `as_of` is a QUESTION — the instant the sweep was asked to evaluate
    /// at, which need not be the instant it ran. Deriving it from `occurred_at` would silently
    /// rewrite the question to "now" and destroy replay determinism.
    ///
    /// Only the past is askable: `as_of` after the appending instant is refused at the command
    /// layer, because a sweep does not predict, and a future-dated answer would be
    /// indistinguishable from a real one while permanently consuming the episodes it touched.
    pub as_of: PersistedTimestamp,
    pub caller: SweepCaller,
}

/// One episode found overdue by a sweep.
///
/// Appended in the SAME BATCH as the `sweep_performed` that minted it: adjacency is what links
/// an exception to its sweep, so no field carries the link and no state exists in which the
/// sweep is recorded and its exceptions are not.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OverdueException {
    pub execution_id: OpaqueId,
    pub node_id: OpaqueId,
    /// The stage entry that lapsed. One exception per episode, forever — a redrive starts a NEW
    /// episode and is eligible again, and the same episode never raises twice however many
    /// sweeps run.
    pub episode_sequence: u64,
    pub stage: CustomsStage,
    /// The instant the episode was measured against. Recorded so a later reader can check the
    /// verdict without re-deriving it from a graph version that may since have moved.
    pub deadline: PersistedTimestamp,
}

/// #1054: how hard the declaring agent says it is thinking.
///
/// A CLOSED vocabulary, and that is the whole reason it is a type rather than a `String`: a closed
/// set is refusable at the wire edge (`X-GraphHelm-Actor-Effort` outside it is a 400), while a free
/// string can only be recorded and hoped about. The schema's `$defs/agentPresenceDeclared/effort`
/// carries exactly these three members; the two are meant to be read together.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeclaredEffort {
    Low,
    Medium,
    High,
}

/// #1054: an agent saying WHICH MODEL is behind this session, and how hard it is set to think.
///
/// Declared, never inferred. Nothing in this codebase may derive a model from a route, a user
/// agent, or a default constant: a session that declares nothing produces no event of this kind at
/// all, so "absent" stays absent instead of being written down as a guess that later reads as
/// measurement. That was once the whole story, and #1057 narrowed it: writing NOTHING says nothing,
/// and silence cannot supersede an earlier session's record. So an event with an ABSENT `model` is
/// now the one way a session says "I declare nothing", and it is written only by a session that
/// named itself in `session`. Nothing is still ever inferred; the absence is just said out loud.
///
/// `model` is an opaque `String` and deliberately not an enum: the set of model names changes far
/// faster than this repository ships, so an enum here would refuse correct values in the field.
///
/// WHAT BOUNDS IT, cited, because the previous version of this comment claimed a safeguard that
/// did not exist and a comment asserting a safeguard is worse than no comment -- the next reader
/// stops looking.
///
/// - The append-time scan is real and DOES run on this payload:
///   `core/events/src/integrity.rs:124` (`validate_prepared_append`) serializes each event and
///   calls `scan_safe_value`, which is `graphhelm_graph::validate_durable_content`
///   (`core/graph/src/persistence.rs:446`). **But what it bounds is not this field.** It caps the
///   TOTAL scanned bytes of the whole envelope (`MAX_CONTENT_SCAN_BYTES`), caps the serialized
///   event (`MAX_EVENT_BYTES`), and refuses SECRET-SHAPED strings anywhere in it. There is no
///   per-field length cap in that path and no charset rule. Reading it as "so `model` is bounded"
///   is the mistake this paragraph exists to stop.
/// - The per-field bound is therefore written twice, once at each door. At the HTTP door,
///   `ACTOR_MODEL_HEADER_MAX_LEN` in `apps/cli/src/commands/serve/mod.rs` refuses a header over
///   128 bytes, a blank one, and one that is not visible ASCII. In the SCHEMA, which is the only
///   thing that binds a producer that never touches that door, `$defs/agentPresenceDeclared/model`
///   carries `maxLength: 128` and `pattern` `\S` -- the same 128, and "at least one non-whitespace
///   character", which is the part `minLength: 1` cannot say.
///
/// `effort` is `Option` with `skip_serializing_if`, and both halves are load-bearing. The schema
/// lists `effort` under `properties` but NOT under `required`, with `additionalProperties: false`
/// and a closed `enum` — so an absent effort must be an ABSENT KEY. Serializing `"effort": null`
/// would satisfy neither the enum nor the absence, and the store would refuse the envelope with a
/// bare `Invalid`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentPresenceDeclared {
    pub actor_id: ActorId,
    pub actor_type: PersistedActorType,
    /// `None` is a DECLARATION, not a gap: "this session declared nothing". It is written only by
    /// a session that named itself in `session` -- see that field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<DeclaredEffort>,
    /// #1057: WHICH SESSION is speaking, opaque and minted by the client (`graphhelm mcp` uses its
    /// own per-process nonce, the same one its idempotency keys and wake leases already carry).
    ///
    /// An actor id is STABLE across sessions; a model is a property of one session. Without this
    /// field a later session reusing the id and declaring nothing inherited the previous session's
    /// model forever, because "declared nothing" was expressed by writing no event -- and silence
    /// cannot supersede a record. With it, a session's first mutation records its own boundary
    /// even when it declares no model, and the newest record for the actor then honestly says
    /// nothing is declared.
    ///
    /// Optional, because a plain HTTP caller that names no session keeps the older behaviour
    /// exactly: it declares a model or it records nothing at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
}

/// One finding inside a gate verdict (M06 Task 1): severity, the claim, the evidence that
/// grounds it, and the remediation the operator can act on. The verdict vocabulary is
/// refusal-with-findings by CONSTRUCTION — the envelope schema refuses a failing verdict
/// whose findings list is empty, so a bare fail cannot exist on the wire (binding
/// decision 2: never pass/fail alone).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GateFinding {
    pub severity: SignalSeverity,
    /// What is wrong, cited — bounded free text under the same append-time wire-safety
    /// scan every payload string passes.
    pub claim: String,
    /// Evidence references grounding the claim (may be empty for a purely structural
    /// finding; the CLAIM is mandatory, the grounding is graded).
    pub evidence: Vec<OpaqueId>,
    /// What would resolve it — the operator-facing half of a refusal.
    pub remediation: String,
}

/// A gate node's verdict (M06 Task 1). Ledger, not state: the fold's arm changes no node
/// state and no counter — the driver maps a failing verdict onto the node outcome the
/// graph routes on (Task 4), and this event is the auditable WHY beside it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GateVerdict {
    pub execution_id: OpaqueId,
    pub node_id: OpaqueId,
    /// The event-sourced gate definition this verdict was produced by.
    pub gate_id: OpaqueId,
    pub passed: bool,
    pub findings: Vec<GateFinding>,
}

/// The thymus receipt (M06 Task 1, binding decision 1): a gate earned the right to gate by
/// rejecting the ENTIRE pathogen suite whose digest rides here. The fold records the
/// certification per gate; growing the suite changes the digest and VOIDS old
/// certifications — Task 4's precondition compares against the CURRENT suite, so a stale
/// receipt refuses execution rather than gating on old immunity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GateCertified {
    pub execution_id: OpaqueId,
    pub gate_id: OpaqueId,
    /// The pathogen suite digest this certification is valid against.
    pub suite_digest: WireHash,
    /// How many specimens the gate rejected to earn this — auditable breadth.
    pub specimens: u32,
}

/// M11 #161: an identity gains the power to countersign, FROM THIS SEQUENCE ON.
///
/// Runtime membership is journaled rather than spec-edited (blueprint 2e, D-039's one entry
/// road): rotation and revocation are events, so "who could countersign at sequence N" is a
/// question the journal answers by itself — kill-bar item 3 applied to identity.
///
/// THE ANSWER COMES FROM THE ORDER OF THE WALK, NEVER FROM THE END STATE. A left fold in
/// sequence order already holds the pre-N registry when it reaches N, so validating a clearance
/// as it is folded is both free and correct. Validating in a second pass against the FINAL
/// registry breaks it in two directions, and the dangerous one is silent: an identity registered
/// AFTER a clearance would retro-validate it, accepting a countersignature from someone who
/// could not sign at the time. This sentence is the canary for that change.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClearanceIdentityRegistered {
    pub execution_id: OpaqueId,
    pub identity: OpaqueId,
    /// Compared for EQUALITY by the fold, never verified cryptographically there: the fold must
    /// stay a pure function of the journal. Signature verification against key material is the
    /// command layer's job at append time, where evidence sealing already lives.
    pub key_fingerprint: WireHash,
}

/// M11 #161: an identity loses the power to countersign, FROM THIS SEQUENCE ON.
///
/// NOT RETROACTIVE, and that is the whole point: a clearance that was valid when it happened
/// stays valid forever. Revocation binds what comes after it and nothing before it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClearanceIdentityRevoked {
    pub execution_id: OpaqueId,
    pub identity: OpaqueId,
}
