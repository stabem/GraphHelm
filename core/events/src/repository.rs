use chrono::{DateTime, Utc};
use graphhelm_protocols::{
    ArtifactId, ArtifactReference, EventEnvelope, EventHash, EvidenceId, NewEvent, OpaqueId,
    RepositoryScope,
};

use crate::{
    ArtifactRegistration, AuthenticationTag, EventRepositoryError, RepositoryFuture,
    SealedEvidence,
    limits::{
        MAX_ARTIFACTS, MAX_BATCH_EVENTS, MAX_EVIDENCE_BATCH_BYTES, MAX_EVIDENCE_ITEMS,
        MAX_READ_PAGE, MAX_SAFE_INTEGER,
    },
};

/// Owned, bounded starting point for an asynchronous stream read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadStart {
    Beginning,
    After {
        sequence: u64,
        event_hash: EventHash,
    },
    Cursor(String),
}

/// Owned request accepted by object-safe asynchronous repositories.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadStreamRequest {
    scope: RepositoryScope,
    stream_id: String,
    start: ReadStart,
    limit: u32,
}

impl ReadStreamRequest {
    pub fn new(
        scope: RepositoryScope,
        stream_id: String,
        start: ReadStart,
        limit: u32,
    ) -> Result<Self, EventRepositoryError> {
        OpaqueId::parse(&stream_id).map_err(|_| EventRepositoryError::Invalid)?;
        if limit == 0 || limit > u32::try_from(MAX_READ_PAGE).expect("bounded constant") {
            return Err(EventRepositoryError::LimitExceeded);
        }
        match &start {
            ReadStart::After { sequence, .. } if *sequence == 0 || *sequence > MAX_SAFE_INTEGER => {
                return Err(EventRepositoryError::LimitExceeded);
            }
            ReadStart::Cursor(cursor) if cursor.is_empty() || cursor.len() > 4 * 1024 => {
                return Err(EventRepositoryError::LimitExceeded);
            }
            _ => {}
        }
        Ok(Self {
            scope,
            stream_id,
            start,
            limit,
        })
    }
    pub const fn scope(&self) -> &RepositoryScope {
        &self.scope
    }
    pub fn stream_id(&self) -> &str {
        &self.stream_id
    }
    pub const fn start(&self) -> &ReadStart {
        &self.start
    }
    pub const fn limit(&self) -> u32 {
        self.limit
    }
    pub fn into_parts(self) -> (RepositoryScope, String, ReadStart, u32) {
        (self.scope, self.stream_id, self.start, self.limit)
    }
}

/// Authenticated last-known state for one exact scoped stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamHead {
    pub next_sequence: u64,
    pub last_event_hash: EventHash,
}

/// Bounded integrity verification request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifyRangeRequest {
    scope: RepositoryScope,
    stream_id: String,
    start_sequence: u64,
    max_events: u32,
}

impl VerifyRangeRequest {
    pub fn new(
        scope: RepositoryScope,
        stream_id: String,
        start_sequence: u64,
        max_events: u32,
    ) -> Result<Self, EventRepositoryError> {
        OpaqueId::parse(&stream_id).map_err(|_| EventRepositoryError::Invalid)?;
        if start_sequence == 0
            || start_sequence > MAX_SAFE_INTEGER
            || max_events == 0
            || max_events > 100_000
        {
            return Err(EventRepositoryError::LimitExceeded);
        }
        Ok(Self {
            scope,
            stream_id,
            start_sequence,
            max_events,
        })
    }
    pub const fn scope(&self) -> &RepositoryScope {
        &self.scope
    }
    pub fn stream_id(&self) -> &str {
        &self.stream_id
    }
    pub const fn start_sequence(&self) -> u64 {
        self.start_sequence
    }
    pub const fn max_events(&self) -> u32 {
        self.max_events
    }
    pub fn into_parts(self) -> (RepositoryScope, String, u64, u32) {
        (
            self.scope,
            self.stream_id,
            self.start_sequence,
            self.max_events,
        )
    }
}

/// Result of a bounded contiguous hash-chain verification.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntegrityReport {
    pub verified_events: u32,
    pub verified_through: Option<u64>,
    pub head: Option<StreamHead>,
}

/// Provider-authenticated integrity checkpoint persisted without key material.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthenticatedCheckpoint {
    pub scope: RepositoryScope,
    pub stream_id: String,
    pub sequence: u64,
    pub event_hash: EventHash,
    pub repository_format_version: u32,
    pub created_at: DateTime<Utc>,
    pub key_version: String,
    pub provider_epoch: u64,
    pub active_graph: Option<crate::ActiveGraphIdentity>,
    pub tag: AuthenticationTag,
}

/// Stable fail-closed Evidence availability state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvidenceUnavailableReason {
    ErasurePending,
    Erased,
    Expired,
    MissingKey,
    IntegrityFailed,
}

/// Exact-scope Evidence lookup result.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EvidenceRead {
    Available(SealedEvidence),
    Unavailable(EvidenceUnavailableReason),
}

/// Object-safe asynchronous event repository contract.
pub trait AsyncEventRepository: Send + Sync {
    fn append_atomic<'a>(
        &'a self,
        request: PreparedAppend,
    ) -> RepositoryFuture<'a, Result<Vec<EventEnvelope>, EventRepositoryError>>;
    fn read_stream<'a>(
        &'a self,
        request: ReadStreamRequest,
    ) -> RepositoryFuture<'a, Result<EventPage, EventRepositoryError>>;
    fn stream_head<'a>(
        &'a self,
        scope: RepositoryScope,
        stream_id: String,
    ) -> RepositoryFuture<'a, Result<Option<StreamHead>, EventRepositoryError>>;
    fn verify_range<'a>(
        &'a self,
        request: VerifyRangeRequest,
    ) -> RepositoryFuture<'a, Result<IntegrityReport, EventRepositoryError>>;
    fn append_checkpoint<'a>(
        &'a self,
        checkpoint: AuthenticatedCheckpoint,
    ) -> RepositoryFuture<'a, Result<AuthenticatedCheckpoint, EventRepositoryError>>;
    fn latest_checkpoint<'a>(
        &'a self,
        scope: RepositoryScope,
        stream_id: String,
    ) -> RepositoryFuture<'a, Result<Option<AuthenticatedCheckpoint>, EventRepositoryError>>;
}

/// Object-safe exact-scope Evidence lookup contract.
pub trait EvidenceRepository: Send + Sync {
    fn get_sealed<'a>(
        &'a self,
        scope: RepositoryScope,
        evidence_id: EvidenceId,
    ) -> RepositoryFuture<'a, Result<EvidenceRead, EventRepositoryError>>;
}

/// Object-safe immutable artifact metadata lookup contract.
pub trait ArtifactCatalog: Send + Sync {
    fn resolve<'a>(
        &'a self,
        scope: RepositoryScope,
        artifact_id: ArtifactId,
    ) -> RepositoryFuture<'a, Result<Option<ArtifactReference>, EventRepositoryError>>;
}

/// Complete synchronous append input for the local atomic repository.
#[derive(Clone, Debug, PartialEq)]
pub struct PreparedAppend {
    scope: RepositoryScope,
    stream_id: OpaqueId,
    expected_next_sequence: u64,
    events: Vec<NewEvent>,
    evidence: Vec<SealedEvidence>,
    artifacts: Vec<ArtifactRegistration>,
}

impl PreparedAppend {
    pub fn new(
        scope: RepositoryScope,
        stream_id: OpaqueId,
        expected_next_sequence: u64,
        events: Vec<NewEvent>,
        evidence: Vec<SealedEvidence>,
        artifacts: Vec<ArtifactRegistration>,
    ) -> Result<Self, EventRepositoryError> {
        if expected_next_sequence == 0
            || expected_next_sequence > MAX_SAFE_INTEGER
            || events.is_empty()
            || events.len() > MAX_BATCH_EVENTS
            || evidence.len() > MAX_EVIDENCE_ITEMS
            || artifacts.len() > MAX_ARTIFACTS
        {
            return Err(EventRepositoryError::LimitExceeded);
        }
        let evidence_bytes = evidence.iter().try_fold(0_usize, |total, item| {
            total.checked_add(item.plaintext_byte_length())
        });
        if evidence_bytes.is_none_or(|total| total > MAX_EVIDENCE_BATCH_BYTES) {
            return Err(EventRepositoryError::LimitExceeded);
        }
        Ok(Self {
            scope,
            stream_id,
            expected_next_sequence,
            events,
            evidence,
            artifacts,
        })
    }

    #[must_use]
    pub const fn scope(&self) -> &RepositoryScope {
        &self.scope
    }

    #[must_use]
    pub const fn stream_id(&self) -> &OpaqueId {
        &self.stream_id
    }

    #[must_use]
    pub const fn expected_next_sequence(&self) -> u64 {
        self.expected_next_sequence
    }

    #[must_use]
    pub fn events(&self) -> &[NewEvent] {
        &self.events
    }

    #[must_use]
    pub fn evidence(&self) -> &[SealedEvidence] {
        &self.evidence
    }

    #[must_use]
    pub fn artifacts(&self) -> &[ArtifactRegistration] {
        &self.artifacts
    }
}

/// Bounded stream page with a scope-bound continuation cursor.
#[derive(Clone, Debug, PartialEq)]
pub struct EventPage {
    pub events: Vec<EventEnvelope>,
    pub next_cursor: Option<String>,
    pub head: Option<StreamHead>,
}

/// Last active graph publication for one scoped stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveVersion {
    pub number: u64,
    pub semantic_hash: String,
    pub sequence: u64,
    pub event_hash: String,
}

/// Exact identity of one durable event stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepositoryStream {
    pub scope: RepositoryScope,
    pub stream_id: String,
}

/// Single synchronous local repository contract for Task 6.
pub trait EventRepository: Send + Sync {
    fn append_atomic(
        &self,
        request: &PreparedAppend,
    ) -> Result<Vec<EventEnvelope>, EventRepositoryError>;

    fn read_stream(
        &self,
        scope: &RepositoryScope,
        stream_id: &str,
        limit: usize,
        cursor: Option<&str>,
    ) -> Result<EventPage, EventRepositoryError>;

    fn read_replay_stream(
        &self,
        scope: &RepositoryScope,
        stream_id: &str,
    ) -> Result<Vec<EventEnvelope>, EventRepositoryError>;

    fn next_sequence(
        &self,
        scope: &RepositoryScope,
        stream_id: &str,
    ) -> Result<u64, EventRepositoryError>;

    fn evidence_exists(
        &self,
        scope: &RepositoryScope,
        evidence_id: &EvidenceId,
    ) -> Result<bool, EventRepositoryError>;

    fn artifact_exists(
        &self,
        scope: &RepositoryScope,
        artifact_id: &ArtifactId,
    ) -> Result<bool, EventRepositoryError>;

    fn active_version(
        &self,
        scope: &RepositoryScope,
        stream_id: &str,
    ) -> Result<Option<ActiveVersion>, EventRepositoryError>;

    /// Returns the complete originally committed physical batch containing an exact key.
    /// This is a read-only recovery boundary for callers whose prior append committed
    /// authoritatively but failed while publishing disposable derived state.
    fn committed_events_for_idempotency(
        &self,
        scope: &RepositoryScope,
        stream_id: &str,
        idempotency_key: &OpaqueId,
    ) -> Result<Option<Vec<EventEnvelope>>, EventRepositoryError>;
}

pub(crate) fn validate_page_limit(limit: usize) -> Result<(), EventRepositoryError> {
    if limit == 0 || limit > MAX_READ_PAGE {
        return Err(EventRepositoryError::LimitExceeded);
    }
    Ok(())
}
