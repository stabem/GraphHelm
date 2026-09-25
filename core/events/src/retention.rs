use std::sync::Arc;

use graphhelm_protocols::{
    EvidenceId, OpaqueId, PersistedTimestamp, RawSha256, RepositoryScope, SafeCode,
    SemanticVersion, Sensitivity,
};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    AuthenticateRequest, AuthenticationTag, KeyError, KeyProvider, RepositoryFuture,
    RevocationReceipt, RevokeKeyRequest, VerifyAuthenticationRequest,
};

const MAX_RETENTION_TARGETS: usize = 10_000;
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum RetentionError {
    #[error("retention request is invalid")]
    Invalid,
    #[error("retention scope is invalid")]
    Scope,
    #[error("retention target limit exceeded")]
    LimitExceeded,
    #[error("retention operation conflicts with durable state")]
    Conflict,
    #[error("retention policy does not permit this operation")]
    Ineligible,
    #[error("a legal hold blocks this operation")]
    LegalHold,
    #[error("key provider is unavailable")]
    KeyUnavailable,
    #[error("retention receipt failed integrity verification")]
    Integrity,
    #[error("retention storage failed")]
    Storage,
}

impl RetentionError {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::LegalHold => "GHEV002_LEGAL_HOLD",
            Self::Ineligible => "GHEV003_RETENTION_INELIGIBLE",
            Self::KeyUnavailable => "GHK001_KEY_UNAVAILABLE",
            Self::LimitExceeded => "GHE006_LIMIT_EXCEEDED",
            Self::Integrity => "GHE005_INTEGRITY_FAILURE",
            Self::Scope => "GHE011_SCOPE_VIOLATION",
            Self::Conflict => "GHE003_IDEMPOTENCY_CONFLICT",
            Self::Storage => "GHE008_STORAGE_FAILURE",
            Self::Invalid => "GHEV004_EVIDENCE_INVALID",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetentionPolicy {
    id: OpaqueId,
    version: SemanticVersion,
    retention_class: String,
    minimum_age_seconds: u64,
    cleanup_delay_seconds: u64,
}

impl RetentionPolicy {
    pub fn new(
        id: OpaqueId,
        version: SemanticVersion,
        retention_class: impl Into<String>,
        minimum_age_seconds: u64,
        cleanup_delay_seconds: u64,
    ) -> Result<Self, RetentionError> {
        let retention_class = retention_class.into();
        if !matches!(
            retention_class.as_str(),
            "ephemeral" | "standard" | "legal_hold"
        ) || minimum_age_seconds > MAX_SAFE_INTEGER
            || cleanup_delay_seconds > MAX_SAFE_INTEGER
        {
            return Err(RetentionError::Invalid);
        }
        Ok(Self {
            id,
            version,
            retention_class,
            minimum_age_seconds,
            cleanup_delay_seconds,
        })
    }

    pub const fn id(&self) -> &OpaqueId {
        &self.id
    }
    pub const fn version(&self) -> &SemanticVersion {
        &self.version
    }
    pub fn retention_class(&self) -> &str {
        &self.retention_class
    }
    pub const fn minimum_age_seconds(&self) -> u64 {
        self.minimum_age_seconds
    }
    pub const fn cleanup_delay_seconds(&self) -> u64 {
        self.cleanup_delay_seconds
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetentionAuthority {
    id: OpaqueId,
    scope: RepositoryScope,
    policy_id: OpaqueId,
    policy_version: SemanticVersion,
    authentication_tag: AuthenticationTag,
}

impl RetentionAuthority {
    #[must_use]
    pub fn new(
        id: OpaqueId,
        scope: RepositoryScope,
        policy_id: OpaqueId,
        policy_version: SemanticVersion,
        authentication_tag: AuthenticationTag,
    ) -> Self {
        Self {
            id,
            scope,
            policy_id,
            policy_version,
            authentication_tag,
        }
    }
    pub const fn id(&self) -> &OpaqueId {
        &self.id
    }
    pub const fn scope(&self) -> &RepositoryScope {
        &self.scope
    }
    pub const fn policy_id(&self) -> &OpaqueId {
        &self.policy_id
    }
    pub const fn policy_version(&self) -> &SemanticVersion {
        &self.policy_version
    }
    pub const fn authentication_tag(&self) -> &AuthenticationTag {
        &self.authentication_tag
    }
}

#[must_use]
pub fn retention_authority_authentication_bytes(
    id: &OpaqueId,
    scope: &RepositoryScope,
    policy: &RetentionPolicy,
) -> Vec<u8> {
    let mut bytes = b"graphhelm-retention-authority-v1".to_vec();
    for value in [
        id.as_str(),
        scope.workspace_id().as_str(),
        scope.project_id().as_str(),
        scope.execution_id().map_or("", |value| value.as_str()),
        policy.id().as_str(),
        policy.version().as_str(),
        policy.retention_class(),
    ] {
        push_field(&mut bytes, value.as_bytes());
    }
    bytes.extend_from_slice(&policy.minimum_age_seconds().to_be_bytes());
    bytes.extend_from_slice(&policy.cleanup_delay_seconds().to_be_bytes());
    bytes
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct RetentionTarget {
    evidence_id: EvidenceId,
}

impl RetentionTarget {
    pub fn new(evidence_id: EvidenceId) -> Result<Self, RetentionError> {
        Ok(Self { evidence_id })
    }
    pub const fn evidence_id(&self) -> &EvidenceId {
        &self.evidence_id
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetentionRequest {
    scope: RepositoryScope,
    operation_id: OpaqueId,
    idempotency_key: OpaqueId,
    policy: RetentionPolicy,
    authority: RetentionAuthority,
    reason_code: SafeCode,
    targets: Vec<RetentionTarget>,
}

impl RetentionRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        scope: RepositoryScope,
        operation_id: OpaqueId,
        idempotency_key: OpaqueId,
        policy: RetentionPolicy,
        authority: RetentionAuthority,
        reason_code: SafeCode,
        mut targets: Vec<RetentionTarget>,
    ) -> Result<Self, RetentionError> {
        if targets.is_empty() || targets.len() > MAX_RETENTION_TARGETS {
            return Err(RetentionError::LimitExceeded);
        }
        if authority.scope != scope
            || authority.policy_id != policy.id
            || authority.policy_version != policy.version
        {
            return Err(RetentionError::Scope);
        }
        targets.sort();
        if targets.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(RetentionError::Invalid);
        }
        Ok(Self {
            scope,
            operation_id,
            idempotency_key,
            policy,
            authority,
            reason_code,
            targets,
        })
    }
    pub const fn scope(&self) -> &RepositoryScope {
        &self.scope
    }
    pub const fn operation_id(&self) -> &OpaqueId {
        &self.operation_id
    }
    pub const fn idempotency_key(&self) -> &OpaqueId {
        &self.idempotency_key
    }
    pub const fn policy(&self) -> &RetentionPolicy {
        &self.policy
    }
    pub const fn authority(&self) -> &RetentionAuthority {
        &self.authority
    }
    pub const fn reason_code(&self) -> &SafeCode {
        &self.reason_code
    }
    pub fn targets(&self) -> &[RetentionTarget] {
        &self.targets
    }
}

pub fn retention_request_digest(request: &RetentionRequest) -> RawSha256 {
    let mut bytes = b"graphhelm-retention-request-v1".to_vec();
    for value in [
        request.scope.workspace_id().as_str(),
        request.scope.project_id().as_str(),
        request.scope.execution_id().map_or("", |id| id.as_str()),
        request.operation_id.as_str(),
        request.idempotency_key.as_str(),
        request.policy.id.as_str(),
        request.policy.version.as_str(),
        request.policy.retention_class.as_str(),
        request.authority.id.as_str(),
        request.reason_code.as_str(),
    ] {
        push_field(&mut bytes, value.as_bytes());
    }
    push_field(
        &mut bytes,
        request.authority.authentication_tag.key_id().as_bytes(),
    );
    push_field(
        &mut bytes,
        request.authority.authentication_tag.algorithm().as_bytes(),
    );
    push_field(&mut bytes, request.authority.authentication_tag.bytes());
    bytes.extend_from_slice(&request.policy.minimum_age_seconds.to_be_bytes());
    bytes.extend_from_slice(&request.policy.cleanup_delay_seconds.to_be_bytes());
    for target in &request.targets {
        push_field(&mut bytes, target.evidence_id.as_str().as_bytes());
    }
    RawSha256::parse(hex::encode(Sha256::digest(bytes))).expect("SHA-256 is valid")
}

fn push_field(output: &mut Vec<u8>, value: &[u8]) {
    output.extend_from_slice(
        &u32::try_from(value.len())
            .expect("bounded field")
            .to_be_bytes(),
    );
    output.extend_from_slice(value);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvidencePriorAvailability {
    Available,
    Expired,
    MissingKey,
    IntegrityFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetentionBlockReason {
    LegalHold,
    MinimumAge,
    PolicyMismatch,
    Unavailable,
}

#[derive(Clone, PartialEq, Eq)]
pub struct RetentionPlanTarget {
    evidence_id: EvidenceId,
    key_handle_id: OpaqueId,
    ciphertext_sha256: RawSha256,
    classification: Sensitivity,
    prior_availability: EvidencePriorAvailability,
    blocked_by: Option<RetentionBlockReason>,
}

impl std::fmt::Debug for RetentionPlanTarget {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RetentionPlanTarget")
            .field("evidence_id", &self.evidence_id)
            .field("classification", &self.classification)
            .field("prior_availability", &self.prior_availability)
            .field("blocked_by", &self.blocked_by)
            .finish_non_exhaustive()
    }
}

impl RetentionPlanTarget {
    pub fn new(
        evidence_id: EvidenceId,
        key_handle_id: OpaqueId,
        ciphertext_sha256: RawSha256,
        classification: Sensitivity,
        prior_availability: EvidencePriorAvailability,
        blocked_by: Option<RetentionBlockReason>,
    ) -> Result<Self, RetentionError> {
        Ok(Self {
            evidence_id,
            key_handle_id,
            ciphertext_sha256,
            classification,
            prior_availability,
            blocked_by,
        })
    }
    pub const fn evidence_id(&self) -> &EvidenceId {
        &self.evidence_id
    }
    pub(crate) const fn key_handle_id(&self) -> &OpaqueId {
        &self.key_handle_id
    }
    pub(crate) const fn ciphertext_sha256(&self) -> &RawSha256 {
        &self.ciphertext_sha256
    }
    pub const fn classification(&self) -> Sensitivity {
        self.classification
    }
    pub const fn prior_availability(&self) -> EvidencePriorAvailability {
        self.prior_availability
    }
    pub const fn blocked_by(&self) -> Option<RetentionBlockReason> {
        self.blocked_by
    }
}

pub struct PreparedRetentionTarget<'a>(&'a RetentionPlanTarget);

impl PreparedRetentionTarget<'_> {
    pub const fn evidence_id(&self) -> &EvidenceId {
        self.0.evidence_id()
    }
    pub const fn key_handle_id(&self) -> &OpaqueId {
        self.0.key_handle_id()
    }
    pub const fn ciphertext_sha256(&self) -> &RawSha256 {
        self.0.ciphertext_sha256()
    }
    pub const fn classification(&self) -> Sensitivity {
        self.0.classification()
    }
    pub const fn prior_availability(&self) -> EvidencePriorAvailability {
        self.0.prior_availability()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetentionPlan {
    operation_id: OpaqueId,
    request_digest: RawSha256,
    evaluated_at: PersistedTimestamp,
    targets: Vec<RetentionPlanTarget>,
}

impl RetentionPlan {
    pub fn new(
        operation_id: OpaqueId,
        request_digest: RawSha256,
        evaluated_at: PersistedTimestamp,
        mut targets: Vec<RetentionPlanTarget>,
    ) -> Result<Self, RetentionError> {
        if targets.is_empty() || targets.len() > MAX_RETENTION_TARGETS {
            return Err(RetentionError::LimitExceeded);
        }
        targets.sort_by(|left, right| left.evidence_id.cmp(&right.evidence_id));
        if targets
            .windows(2)
            .any(|pair| pair[0].evidence_id == pair[1].evidence_id)
        {
            return Err(RetentionError::Invalid);
        }
        Ok(Self {
            operation_id,
            request_digest,
            evaluated_at,
            targets,
        })
    }
    pub const fn operation_id(&self) -> &OpaqueId {
        &self.operation_id
    }
    pub const fn request_digest(&self) -> &RawSha256 {
        &self.request_digest
    }
    pub const fn evaluated_at(&self) -> &PersistedTimestamp {
        &self.evaluated_at
    }
    pub fn targets(&self) -> &[RetentionPlanTarget] {
        &self.targets
    }
    pub fn is_eligible(&self) -> bool {
        self.targets
            .iter()
            .all(|target| target.blocked_by.is_none())
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct PreparedRetention {
    request: RetentionRequest,
    plan: RetentionPlan,
    prepared_at: PersistedTimestamp,
    provider_epoch: u64,
    authentication_tag: AuthenticationTag,
}

impl std::fmt::Debug for PreparedRetention {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedRetention")
            .field("operation_id", self.request.operation_id())
            .field("target_count", &self.plan.targets().len())
            .field("prepared_at", &self.prepared_at)
            .field("provider_epoch", &self.provider_epoch)
            .field("authentication_tag", &"[redacted]")
            .finish()
    }
}

impl PreparedRetention {
    pub fn new(
        request: RetentionRequest,
        plan: RetentionPlan,
        prepared_at: PersistedTimestamp,
        provider_epoch: u64,
        authentication_tag: AuthenticationTag,
    ) -> Result<Self, RetentionError> {
        if plan.operation_id != request.operation_id
            || plan.request_digest != retention_request_digest(&request)
            || !plan.is_eligible()
            || provider_epoch > MAX_SAFE_INTEGER
        {
            return Err(RetentionError::Invalid);
        }
        let requested = request.targets.iter().map(|target| &target.evidence_id);
        let planned = plan.targets.iter().map(|target| &target.evidence_id);
        if !requested.eq(planned) {
            return Err(RetentionError::Invalid);
        }
        Ok(Self {
            request,
            plan,
            prepared_at,
            provider_epoch,
            authentication_tag,
        })
    }
    pub const fn request(&self) -> &RetentionRequest {
        &self.request
    }
    pub const fn plan(&self) -> &RetentionPlan {
        &self.plan
    }
    pub const fn prepared_at(&self) -> &PersistedTimestamp {
        &self.prepared_at
    }
    pub const fn provider_epoch(&self) -> u64 {
        self.provider_epoch
    }
    pub const fn authentication_tag(&self) -> &AuthenticationTag {
        &self.authentication_tag
    }
    pub fn resolved_targets(&self) -> impl ExactSizeIterator<Item = PreparedRetentionTarget<'_>> {
        self.plan.targets.iter().map(PreparedRetentionTarget)
    }
}

#[must_use]
pub fn prepared_authentication_bytes(prepared: &PreparedRetention) -> Vec<u8> {
    prepared_bytes(
        prepared.request(),
        prepared.plan(),
        prepared.prepared_at(),
        prepared.provider_epoch(),
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FinalizedRetention {
    prepared: PreparedRetention,
    receipts: Vec<RevocationReceipt>,
    completed_at: PersistedTimestamp,
    authentication_tag: AuthenticationTag,
}

impl FinalizedRetention {
    pub fn new(
        prepared: PreparedRetention,
        receipts: Vec<RevocationReceipt>,
        completed_at: PersistedTimestamp,
        authentication_tag: AuthenticationTag,
    ) -> Result<Self, RetentionError> {
        if receipts.len() != prepared.plan.targets.len()
            || completed_at.as_datetime() < prepared.prepared_at().as_datetime()
            || receipts
                .windows(2)
                .any(|pair| pair[0].epoch() > pair[1].epoch())
            || receipts
                .iter()
                .zip(prepared.plan.targets())
                .any(|(receipt, target)| {
                    receipt.handle() != target.key_handle_id().as_str()
                        || receipt.idempotency_key()
                            != provider_revocation_idempotency_key(
                                prepared.request().scope(),
                                prepared.request().operation_id(),
                                target.evidence_id(),
                            )
                            .as_str()
                        || receipt.epoch() < prepared.provider_epoch()
                })
        {
            return Err(RetentionError::Invalid);
        }
        Ok(Self {
            prepared,
            receipts,
            completed_at,
            authentication_tag,
        })
    }
    pub const fn prepared(&self) -> &PreparedRetention {
        &self.prepared
    }
    pub fn receipts(&self) -> &[RevocationReceipt] {
        &self.receipts
    }
    pub const fn completed_at(&self) -> &PersistedTimestamp {
        &self.completed_at
    }
    pub const fn authentication_tag(&self) -> &AuthenticationTag {
        &self.authentication_tag
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegalHoldChange {
    scope: RepositoryScope,
    hold_id: OpaqueId,
    evidence_id: EvidenceId,
    authority: OpaqueId,
    reason_code: SafeCode,
    placed: bool,
    changed_at: PersistedTimestamp,
    authentication_tag: AuthenticationTag,
}

impl LegalHoldChange {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        scope: RepositoryScope,
        hold_id: OpaqueId,
        evidence_id: EvidenceId,
        authority: OpaqueId,
        reason_code: SafeCode,
        placed: bool,
        changed_at: PersistedTimestamp,
        authentication_tag: AuthenticationTag,
    ) -> Self {
        Self {
            scope,
            hold_id,
            evidence_id,
            authority,
            reason_code,
            placed,
            changed_at,
            authentication_tag,
        }
    }
    pub const fn scope(&self) -> &RepositoryScope {
        &self.scope
    }
    pub const fn hold_id(&self) -> &OpaqueId {
        &self.hold_id
    }
    pub const fn evidence_id(&self) -> &EvidenceId {
        &self.evidence_id
    }
    pub const fn authority(&self) -> &OpaqueId {
        &self.authority
    }
    pub const fn reason_code(&self) -> &SafeCode {
        &self.reason_code
    }
    pub const fn placed(&self) -> bool {
        self.placed
    }
    pub const fn changed_at(&self) -> &PersistedTimestamp {
        &self.changed_at
    }
    pub const fn authentication_tag(&self) -> &AuthenticationTag {
        &self.authentication_tag
    }
}

#[must_use]
pub fn legal_hold_authentication_bytes(change: &LegalHoldChange) -> Vec<u8> {
    let mut bytes = b"graphhelm-legal-hold-change-v1".to_vec();
    for value in [
        change.scope().workspace_id().as_str(),
        change.scope().project_id().as_str(),
        change.scope().execution_id().map_or("", |id| id.as_str()),
        change.hold_id().as_str(),
        change.evidence_id().as_str(),
        change.authority().as_str(),
        change.reason_code().as_str(),
        &change.changed_at().as_datetime().to_rfc3339(),
    ] {
        push_field(&mut bytes, value.as_bytes());
    }
    bytes.push(u8::from(change.placed()));
    bytes
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegalHoldReceipt {
    change: LegalHoldChange,
}
impl LegalHoldReceipt {
    pub const fn new(change: LegalHoldChange) -> Self {
        Self { change }
    }
    pub const fn change(&self) -> &LegalHoldChange {
        &self.change
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CleanupRequest {
    scope: RepositoryScope,
    operation_id: OpaqueId,
    idempotency_key: OpaqueId,
    evidence_ids: Vec<EvidenceId>,
    requested_at: PersistedTimestamp,
}
impl CleanupRequest {
    pub fn new(
        scope: RepositoryScope,
        operation_id: OpaqueId,
        idempotency_key: OpaqueId,
        mut evidence_ids: Vec<EvidenceId>,
        requested_at: PersistedTimestamp,
    ) -> Result<Self, RetentionError> {
        if evidence_ids.is_empty() || evidence_ids.len() > MAX_RETENTION_TARGETS {
            return Err(RetentionError::LimitExceeded);
        }
        evidence_ids.sort();
        if evidence_ids.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(RetentionError::Invalid);
        }
        Ok(Self {
            scope,
            operation_id,
            idempotency_key,
            evidence_ids,
            requested_at,
        })
    }
    pub const fn scope(&self) -> &RepositoryScope {
        &self.scope
    }
    pub const fn operation_id(&self) -> &OpaqueId {
        &self.operation_id
    }
    pub const fn idempotency_key(&self) -> &OpaqueId {
        &self.idempotency_key
    }
    pub fn evidence_ids(&self) -> &[EvidenceId] {
        &self.evidence_ids
    }
    pub const fn requested_at(&self) -> &PersistedTimestamp {
        &self.requested_at
    }
}

pub fn cleanup_request_digest(request: &CleanupRequest) -> RawSha256 {
    let mut bytes = b"graphhelm-cleanup-request-v1".to_vec();
    for value in [
        request.scope.workspace_id().as_str(),
        request.scope.project_id().as_str(),
        request.scope.execution_id().map_or("", |id| id.as_str()),
        request.operation_id.as_str(),
        request.idempotency_key.as_str(),
        request.requested_at.as_datetime().to_rfc3339().as_str(),
    ] {
        push_field(&mut bytes, value.as_bytes());
    }
    for evidence_id in &request.evidence_ids {
        push_field(&mut bytes, evidence_id.as_str().as_bytes());
    }
    RawSha256::parse(hex::encode(Sha256::digest(bytes))).expect("SHA-256 is valid")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CleanupReceipt {
    operation_id: OpaqueId,
    deleted_evidence_ids: Vec<EvidenceId>,
    deleted_at: PersistedTimestamp,
}
impl CleanupReceipt {
    pub fn new(
        operation_id: OpaqueId,
        mut deleted_evidence_ids: Vec<EvidenceId>,
        deleted_at: PersistedTimestamp,
    ) -> Result<Self, RetentionError> {
        deleted_evidence_ids.sort();
        if deleted_evidence_ids.is_empty()
            || deleted_evidence_ids.len() > MAX_RETENTION_TARGETS
            || deleted_evidence_ids
                .windows(2)
                .any(|pair| pair[0] == pair[1])
        {
            return Err(RetentionError::Invalid);
        }
        Ok(Self {
            operation_id,
            deleted_evidence_ids,
            deleted_at,
        })
    }
    pub const fn operation_id(&self) -> &OpaqueId {
        &self.operation_id
    }
    pub fn deleted_evidence_ids(&self) -> &[EvidenceId] {
        &self.deleted_evidence_ids
    }
    pub const fn deleted_at(&self) -> &PersistedTimestamp {
        &self.deleted_at
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RetentionPrepareOutcome {
    Prepared(PreparedRetention),
    Finalized(FinalizedRetention),
}

pub trait RetentionRepository: Send + Sync {
    fn dry_run<'a>(
        &'a self,
        request: RetentionRequest,
        evaluated_at: PersistedTimestamp,
    ) -> RepositoryFuture<'a, Result<RetentionPlan, RetentionError>>;
    fn prepare<'a>(
        &'a self,
        prepared: PreparedRetention,
    ) -> RepositoryFuture<'a, Result<RetentionPrepareOutcome, RetentionError>>;
    fn finalize<'a>(
        &'a self,
        prepared: PreparedRetention,
        receipts: Vec<RevocationReceipt>,
        completed_at: PersistedTimestamp,
        authentication_tag: AuthenticationTag,
    ) -> RepositoryFuture<'a, Result<FinalizedRetention, RetentionError>>;
    fn pending<'a>(
        &'a self,
        scope: RepositoryScope,
        limit: u32,
    ) -> RepositoryFuture<'a, Result<Vec<PreparedRetention>, RetentionError>>;
    fn change_legal_hold<'a>(
        &'a self,
        change: LegalHoldChange,
    ) -> RepositoryFuture<'a, Result<LegalHoldReceipt, RetentionError>>;
    fn cleanup<'a>(
        &'a self,
        request: CleanupRequest,
    ) -> RepositoryFuture<'a, Result<CleanupReceipt, RetentionError>>;
}

pub trait RetentionClock: Send + Sync {
    fn now(&self) -> PersistedTimestamp;
}

pub struct RetentionService {
    repository: Arc<dyn RetentionRepository>,
    key_provider: Arc<dyn KeyProvider>,
    clock: Arc<dyn RetentionClock>,
}

impl RetentionService {
    #[must_use]
    pub fn new(
        repository: Arc<dyn RetentionRepository>,
        key_provider: Arc<dyn KeyProvider>,
        clock: Arc<dyn RetentionClock>,
    ) -> Self {
        Self {
            repository,
            key_provider,
            clock,
        }
    }

    pub fn dry_run<'a>(
        &'a self,
        request: RetentionRequest,
    ) -> RepositoryFuture<'a, Result<RetentionPlan, RetentionError>> {
        Box::pin(async move {
            let authority = request.authority();
            self.key_provider
                .verify(
                    VerifyAuthenticationRequest::new(
                        "retention-authority",
                        retention_authority_authentication_bytes(
                            authority.id(),
                            authority.scope(),
                            request.policy(),
                        ),
                        authority.authentication_tag().clone(),
                    )
                    .map_err(map_key_error)?,
                )
                .await
                .map_err(map_key_error)?;
            let digest = retention_request_digest(&request);
            let plan = self
                .repository
                .dry_run(request.clone(), self.clock.now())
                .await?;
            if plan.operation_id() != request.operation_id() || plan.request_digest() != &digest {
                return Err(RetentionError::Integrity);
            }
            Ok(plan)
        })
    }

    pub fn execute<'a>(
        &'a self,
        request: RetentionRequest,
    ) -> RepositoryFuture<'a, Result<FinalizedRetention, RetentionError>> {
        Box::pin(async move {
            let plan = self.dry_run(request.clone()).await?;
            if !plan.is_eligible() {
                return Err(
                    if plan
                        .targets()
                        .iter()
                        .any(|target| target.blocked_by() == Some(RetentionBlockReason::LegalHold))
                    {
                        RetentionError::LegalHold
                    } else {
                        RetentionError::Ineligible
                    },
                );
            }
            let metadata = self.key_provider.metadata().await.map_err(map_key_error)?;
            let prepared_at = self.clock.now();
            let bytes = prepared_bytes(
                &request,
                &plan,
                &prepared_at,
                metadata.current_revocation_epoch(),
            );
            let tag = self
                .key_provider
                .authenticate(
                    AuthenticateRequest::new("retention-prepared", bytes).map_err(map_key_error)?,
                )
                .await
                .map_err(map_key_error)?;
            let prepared = PreparedRetention::new(
                request,
                plan,
                prepared_at,
                metadata.current_revocation_epoch(),
                tag,
            )?;
            match self.repository.prepare(prepared).await? {
                RetentionPrepareOutcome::Finalized(finalized) => Ok(finalized),
                RetentionPrepareOutcome::Prepared(prepared) => self.finish(prepared).await,
            }
        })
    }

    pub fn reconcile<'a>(
        &'a self,
        scope: RepositoryScope,
        limit: u32,
    ) -> RepositoryFuture<'a, Result<Vec<FinalizedRetention>, RetentionError>> {
        Box::pin(async move {
            if limit == 0 || limit > 10_000 {
                return Err(RetentionError::LimitExceeded);
            }
            let pending = self.repository.pending(scope, limit).await?;
            let mut finalized = Vec::with_capacity(pending.len());
            for item in pending {
                finalized.push(self.finish(item).await?);
            }
            Ok(finalized)
        })
    }

    pub fn change_legal_hold<'a>(
        &'a self,
        change: LegalHoldChange,
    ) -> RepositoryFuture<'a, Result<LegalHoldReceipt, RetentionError>> {
        Box::pin(async move {
            self.key_provider
                .verify(
                    VerifyAuthenticationRequest::new(
                        "retention-legal-hold",
                        legal_hold_authentication_bytes(&change),
                        change.authentication_tag().clone(),
                    )
                    .map_err(map_key_error)?,
                )
                .await
                .map_err(map_key_error)?;
            self.repository.change_legal_hold(change).await
        })
    }

    pub fn cleanup<'a>(
        &'a self,
        request: CleanupRequest,
    ) -> RepositoryFuture<'a, Result<CleanupReceipt, RetentionError>> {
        self.repository.cleanup(request)
    }

    async fn finish(
        &self,
        prepared: PreparedRetention,
    ) -> Result<FinalizedRetention, RetentionError> {
        let bytes = prepared_bytes(
            prepared.request(),
            prepared.plan(),
            prepared.prepared_at(),
            prepared.provider_epoch(),
        );
        self.key_provider
            .verify(
                VerifyAuthenticationRequest::new(
                    "retention-prepared",
                    bytes,
                    prepared.authentication_tag().clone(),
                )
                .map_err(map_key_error)?,
            )
            .await
            .map_err(map_key_error)?;
        let mut receipts = Vec::with_capacity(prepared.plan().targets().len());
        for target in prepared.plan().targets() {
            let provider_idempotency_key = provider_revocation_idempotency_key(
                prepared.request().scope(),
                prepared.request().operation_id(),
                target.evidence_id(),
            );
            let receipt = self
                .key_provider
                .revoke(
                    RevokeKeyRequest::new(
                        target.key_handle_id().as_str(),
                        provider_idempotency_key.as_str(),
                    )
                    .map_err(map_key_error)?,
                )
                .await
                .map_err(map_key_error)?;
            if receipt.handle() != target.key_handle_id().as_str()
                || receipt.idempotency_key() != provider_idempotency_key.as_str()
                || receipt.epoch() < prepared.provider_epoch()
            {
                return Err(RetentionError::Integrity);
            }
            self.key_provider
                .verify(
                    VerifyAuthenticationRequest::new(
                        "revocation-receipt",
                        revocation_receipt_authentication_bytes(&receipt),
                        receipt.authentication_tag().clone(),
                    )
                    .map_err(map_key_error)?,
                )
                .await
                .map_err(map_key_error)?;
            receipts.push(receipt);
        }
        let metadata = self.key_provider.metadata().await.map_err(map_key_error)?;
        if receipts
            .iter()
            .any(|receipt| receipt.epoch() > metadata.current_revocation_epoch())
        {
            return Err(RetentionError::Integrity);
        }
        let completed_at = self.clock.now();
        let finalized_tag = self
            .key_provider
            .authenticate(
                AuthenticateRequest::new(
                    "retention-finalized",
                    finalized_authentication_bytes(&prepared, &receipts, &completed_at),
                )
                .map_err(map_key_error)?,
            )
            .await
            .map_err(map_key_error)?;
        self.repository
            .finalize(prepared, receipts, completed_at, finalized_tag)
            .await
    }
}

pub fn provider_revocation_idempotency_key(
    scope: &RepositoryScope,
    operation_id: &OpaqueId,
    evidence_id: &EvidenceId,
) -> RawSha256 {
    let mut bytes = b"graphhelm-retention-provider-revoke-v1".to_vec();
    for value in [
        scope.workspace_id().as_str(),
        scope.project_id().as_str(),
        scope.execution_id().map_or("", |id| id.as_str()),
    ] {
        push_field(&mut bytes, value.as_bytes());
    }
    push_field(&mut bytes, operation_id.as_str().as_bytes());
    push_field(&mut bytes, evidence_id.as_str().as_bytes());
    RawSha256::parse(hex::encode(Sha256::digest(bytes))).expect("SHA-256 is valid")
}

fn prepared_bytes(
    request: &RetentionRequest,
    plan: &RetentionPlan,
    prepared_at: &PersistedTimestamp,
    provider_epoch: u64,
) -> Vec<u8> {
    let mut bytes = b"graphhelm-retention-prepared-v1".to_vec();
    push_field(
        &mut bytes,
        retention_request_digest(request).as_str().as_bytes(),
    );
    push_field(&mut bytes, plan.request_digest.as_str().as_bytes());
    push_field(
        &mut bytes,
        plan.evaluated_at.as_datetime().to_rfc3339().as_bytes(),
    );
    push_field(
        &mut bytes,
        prepared_at.as_datetime().to_rfc3339().as_bytes(),
    );
    bytes.extend_from_slice(&provider_epoch.to_be_bytes());
    for target in &plan.targets {
        push_field(&mut bytes, target.evidence_id.as_str().as_bytes());
        push_field(&mut bytes, target.key_handle_id.as_str().as_bytes());
        push_field(&mut bytes, target.ciphertext_sha256.as_str().as_bytes());
        bytes.push(match target.classification {
            Sensitivity::Public => 0,
            Sensitivity::Internal => 1,
            Sensitivity::Confidential => 2,
            Sensitivity::Restricted => 3,
        });
        bytes.push(match target.prior_availability {
            EvidencePriorAvailability::Available => 0,
            EvidencePriorAvailability::Expired => 1,
            EvidencePriorAvailability::MissingKey => 2,
            EvidencePriorAvailability::IntegrityFailed => 3,
        });
        bytes.push(match target.blocked_by {
            None => 0,
            Some(RetentionBlockReason::LegalHold) => 1,
            Some(RetentionBlockReason::MinimumAge) => 2,
            Some(RetentionBlockReason::PolicyMismatch) => 3,
            Some(RetentionBlockReason::Unavailable) => 4,
        });
    }
    bytes
}

pub fn revocation_receipt_authentication_bytes(receipt: &RevocationReceipt) -> Vec<u8> {
    let mut bytes = b"graphhelm-revocation-receipt-v1".to_vec();
    push_field(&mut bytes, receipt.handle().as_bytes());
    push_field(&mut bytes, receipt.idempotency_key().as_bytes());
    bytes.extend_from_slice(&receipt.epoch().to_be_bytes());
    bytes
}

pub fn finalized_authentication_bytes(
    prepared: &PreparedRetention,
    receipts: &[RevocationReceipt],
    completed_at: &PersistedTimestamp,
) -> Vec<u8> {
    let mut bytes = b"graphhelm-retention-finalized-v1".to_vec();
    push_field(
        &mut bytes,
        prepared.request().operation_id().as_str().as_bytes(),
    );
    push_field(
        &mut bytes,
        prepared.plan().request_digest().as_str().as_bytes(),
    );
    push_field(
        &mut bytes,
        completed_at.as_datetime().to_rfc3339().as_bytes(),
    );
    for receipt in receipts {
        push_field(&mut bytes, receipt.handle().as_bytes());
        push_field(&mut bytes, receipt.idempotency_key().as_bytes());
        bytes.extend_from_slice(&receipt.epoch().to_be_bytes());
        push_field(&mut bytes, receipt.authentication_tag().bytes());
    }
    bytes
}

fn map_key_error(error: KeyError) -> RetentionError {
    match error {
        KeyError::Integrity => RetentionError::Integrity,
        KeyError::Conflict => RetentionError::Conflict,
        KeyError::Invalid | KeyError::Unavailable | KeyError::Storage => {
            RetentionError::KeyUnavailable
        }
    }
}
