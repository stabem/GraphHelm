//! #219 task-003: snapshot-bound retrieval plan compilation.
//!
//! Under construction by TDD. Only what a currently-failing test demanded exists here.

use std::collections::BTreeSet;

use graphhelm_protocols::{
    ArtifactBinding, CoverageState, DeclaredLimits, DevelopmentEnvelope, DevelopmentKind,
    DevelopmentRefusalCode, RETRIEVAL_COVERAGE_RECEIPT_API_VERSION,
    RETRIEVAL_COVERAGE_RECEIPT_KIND, RetrievalBrokerRecordBinding, RetrievalCoverageEntry,
    RetrievalCoverageReceiptBody, RetrievalCoverageReceiptV1, RetrievalCoverageTarget,
    RetrievalFallbackKind, RetrievalFallbackOutcome, RetrievalFallbackReceipt,
    RetrievalProviderBinding, RetrievalReceivedLimits, RetrievalStepBinding, SemanticVersion,
    SnapshotBinding, WireHash, canonical_json, development_api_version_major,
};
use graphhelm_tool_broker::record::{ToolCallRecord, ToolDisposition};
use sha2::Digest as _;

const MAX_JSON_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const MAX_RECEIPT_WIRE_BYTES: usize = 8 * 1024 * 1024;
const MAX_RECEIPT_TARGETS: usize = 512;
const MAX_RECEIPT_PAGES: usize = 1024;
const MAX_RECEIPT_TEXT_BYTES: usize = 4096;
const DEVELOPMENT_ENVELOPE_SCHEMA_ID: &str =
    "https://p50.dev/schemas/development-envelope.schema.json";
const SUPPORTED_RETRIEVAL_PLAN_MAJOR: &str = "1";
const SUPPORTED_DEVELOPMENT_API_MAJOR: u16 = 1;

/// What an index returned, with its coverage verdict **in the same value**.
///
/// Coverage is a RETURN VALUE rather than a query option on purpose: results cannot be obtained
/// without obtaining the coverage that says what a zero among them would mean, so "forgot to check
/// coverage" is unrepresentable rather than merely discouraged.
pub struct IndexResponse {
    pub hits: Vec<String>,
    pub coverage: CoverageState,
    /// Free prose the provider attached to its answer.
    ///
    /// Carried so it can be preserved as candidate evidence, and **never read by plan
    /// compilation**. A provider can write anything here, including text shaped like an
    /// instruction about how its own structured fields should be interpreted. The structured
    /// fields are the input; this is a claim about the input, which is a different kind of thing.
    pub summary: Option<String>,
    /// How many pages the runtime walked to assemble this response.
    ///
    /// Counted by the side that DROVE the pagination, never reported by the provider. A page count
    /// taken from the thing being bounded is a self-report about the budget it is spending.
    pub pages: u32,
}

impl IndexResponse {
    #[must_use]
    pub const fn new(hits: Vec<String>, coverage: CoverageState) -> Self {
        Self {
            hits,
            coverage,
            summary: None,
            pages: 1,
        }
    }

    /// Record how many pages the runtime walked for this response.
    #[must_use]
    pub const fn after_pages(mut self, pages: u32) -> Self {
        self.pages = pages;
        self
    }

    /// Attach the provider's prose. Deliberately does not participate in compilation.
    #[must_use]
    pub fn with_summary(mut self, summary: &str) -> Self {
        self.summary = Some(summary.to_owned());
        self
    }
}

/// Immutable request passed to a [`crate::ports::StructuralCodeIndex`]. Provider output echoes the
/// authority-bearing fields so Runtime can prove it answered this request rather than a nearby one.
#[derive(Clone, Debug, PartialEq)]
pub struct StructuralIndexRequest {
    pub plan: DevelopmentEnvelope,
    pub plan_binding: ArtifactBinding,
    pub step: RetrievalStepBinding,
    pub provider: RetrievalProviderBinding,
    pub requested_paths: Vec<String>,
    pub negative_scopes: Vec<String>,
    pub limits: DeclaredLimits,
}

/// Fail-closed reasons while turning untrusted provider evidence into a receipt. These are local
/// construction errors, not a second wire vocabulary and never cross into a development envelope.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RetrievalReceiptError {
    #[error("structural code index unavailable")]
    ProviderUnavailable,
    #[error("retrieval binding mismatch")]
    BindingMismatch,
    #[error("retrieval index stale")]
    IndexStale,
    #[error("tool broker record invalid")]
    BrokerRecordInvalid,
    #[error("coverage confidence cannot support the claimed state")]
    CoveragePromotion,
    #[error("retrieval pagination unfinished")]
    PaginationUnfinished,
    #[error("retrieval pagination position repeated")]
    PaginationLoop,
    #[error("retrieval pagination totals disagree")]
    PaginationInconsistent,
    #[error("retrieval declared limit exceeded")]
    LimitExceeded,
    #[error("retrieval evidence is invalid")]
    EvidenceInvalid,
    #[error("retrieval receipt wire shape invalid")]
    InvalidWire,
    #[error("retrieval receipt digest mismatch")]
    DigestMismatch,
}

/// A receipt that has passed binding, pagination, coverage, Tool Broker, limit and digest checks.
/// The wire value is private so callers cannot mutate it back into an unverified state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidatedRetrievalCoverageReceipt {
    wire: RetrievalCoverageReceiptV1,
    broker_record: ToolCallRecord,
}

impl ValidatedRetrievalCoverageReceipt {
    #[must_use]
    pub const fn plan_binding(&self) -> &ArtifactBinding {
        &self.wire.body.plan_binding
    }

    #[must_use]
    pub const fn step(&self) -> &RetrievalStepBinding {
        &self.wire.body.step
    }

    #[must_use]
    pub const fn provider(&self) -> &RetrievalProviderBinding {
        &self.wire.body.provider
    }

    #[must_use]
    pub const fn scope(&self) -> &graphhelm_protocols::DevelopmentScope {
        &self.wire.body.scope
    }

    #[must_use]
    pub const fn snapshots(&self) -> &SnapshotBinding {
        &self.wire.body.snapshots
    }

    #[must_use]
    pub const fn broker_record(&self) -> &ToolCallRecord {
        &self.broker_record
    }

    #[must_use]
    pub fn fallback(&self, kind: RetrievalFallbackKind) -> Option<RetrievalFallbackOutcome> {
        self.wire
            .body
            .fallbacks
            .iter()
            .find(|fallback| fallback.kind == kind)
            .map(|fallback| fallback.outcome)
    }

    pub fn stable_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(&self.wire)
    }
}

/// Call the structural-index port once and publish a receipt only after every returned field is
/// independently checked by Runtime.
pub fn retrieve_coverage<
    I: crate::ports::StructuralCodeIndex + ?Sized,
    R: crate::ports::SourceReader + ?Sized,
>(
    index: &I,
    reader: &R,
    request: &StructuralIndexRequest,
) -> Result<ValidatedRetrievalCoverageReceipt, RetrievalReceiptError> {
    let response = index
        .retrieve(request)
        .map_err(|_| RetrievalReceiptError::ProviderUnavailable)?;
    let broker_record = response.broker_record.clone();
    let body = validate_response(request, response, reader.current_snapshot())?;
    let schema_version =
        SemanticVersion::parse("1.0.0").map_err(|_| RetrievalReceiptError::InvalidWire)?;
    let digest = receipt_digest(&body, &schema_version)?;
    let wire = RetrievalCoverageReceiptV1 {
        api_version: RETRIEVAL_COVERAGE_RECEIPT_API_VERSION.to_owned(),
        kind: RETRIEVAL_COVERAGE_RECEIPT_KIND.to_owned(),
        schema_version,
        body,
        digest,
    };
    validate_wire_size(&wire)?;
    Ok(ValidatedRetrievalCoverageReceipt {
        wire,
        broker_record,
    })
}

/// Rehydrate a receipt only when its canonical digest, current source snapshot, expected request
/// and exact durable Tool Broker record all still agree.
pub fn validate_retrieval_receipt<R: crate::ports::SourceReader + ?Sized>(
    bytes: &[u8],
    reader: &R,
    request: &StructuralIndexRequest,
    broker_record: &ToolCallRecord,
) -> Result<ValidatedRetrievalCoverageReceipt, RetrievalReceiptError> {
    if bytes.len() > MAX_RECEIPT_WIRE_BYTES {
        return Err(RetrievalReceiptError::LimitExceeded);
    }
    let wire: RetrievalCoverageReceiptV1 =
        serde_json::from_slice(bytes).map_err(|_| RetrievalReceiptError::InvalidWire)?;
    if wire.api_version != RETRIEVAL_COVERAGE_RECEIPT_API_VERSION
        || wire.kind != RETRIEVAL_COVERAGE_RECEIPT_KIND
        || wire.schema_version.as_str() != "1.0.0"
    {
        return Err(RetrievalReceiptError::InvalidWire);
    }
    if receipt_digest(&wire.body, &wire.schema_version)? != wire.digest {
        return Err(RetrievalReceiptError::DigestMismatch);
    }

    let body = validate_response(
        request,
        crate::ports::StructuralIndexResponse {
            plan_binding: wire.body.plan_binding.clone(),
            step: wire.body.step.clone(),
            scope: wire.body.scope.clone(),
            snapshots: wire.body.snapshots.clone(),
            provider: wire.body.provider.clone(),
            broker_record: broker_record.clone(),
            confidence: wire.body.confidence,
            coverage: wire.body.coverage,
            entries: wire.body.entries.clone(),
            pages: wire.body.pages.clone(),
            total_results: wire.body.total_results,
            hits: wire.body.hits.clone(),
        },
        reader.current_snapshot(),
    )?;
    if body != wire.body {
        return Err(RetrievalReceiptError::BindingMismatch);
    }
    Ok(ValidatedRetrievalCoverageReceipt {
        wire,
        broker_record: broker_record.clone(),
    })
}

/// Compile only from a validated producer receipt. Positive findings retain their coverage; a zero
/// becomes absence only when every path and bounded negative scope has exact complete coverage.
#[must_use]
pub fn compile_receipt(receipt: &ValidatedRetrievalCoverageReceipt) -> RetrievalOutcome {
    let body = &receipt.wire.body;
    if body.hits.is_empty() {
        if negative_proof_complete(body) {
            return RetrievalOutcome::VerifiedAbsence;
        }
        return RetrievalOutcome::Refused {
            code: DevelopmentRefusalCode::NegativeClaimUnverified,
        };
    }
    RetrievalOutcome::Claim {
        hits: body.hits.clone(),
        coverage: body.coverage,
    }
}

fn validate_response(
    request: &StructuralIndexRequest,
    response: crate::ports::StructuralIndexResponse,
    current_snapshot: graphhelm_protocols::OpaqueId,
) -> Result<RetrievalCoverageReceiptBody, RetrievalReceiptError> {
    if !request.plan_binding.snapshots.is_fresh()
        || current_snapshot != request.plan_binding.snapshots.repo_snapshot
        || response.coverage == CoverageState::Stale
        || response.snapshots != request.plan_binding.snapshots
    {
        return Err(RetrievalReceiptError::IndexStale);
    }
    if response.plan_binding != request.plan_binding
        || response.step != request.step
        || response.scope != request.plan_binding.scope
        || response.provider != request.provider
    {
        return Err(RetrievalReceiptError::BindingMismatch);
    }
    validate_request_targets(request)?;
    validate_coverage_entries(request, response.coverage, &response.entries)?;
    if response.confidence == graphhelm_protocols::ProviderCoverageConfidence::BestEffort
        && response.coverage == CoverageState::Complete
    {
        return Err(RetrievalReceiptError::CoveragePromotion);
    }

    let received_limits = validate_pages_and_limits(request, &response)?;
    let broker_binding = broker_binding(
        &request.provider,
        &response.broker_record,
        received_limits.bytes,
    )?;

    let mut hits = response
        .hits
        .iter()
        .map(|hit| canonical_hit(hit))
        .collect::<Vec<_>>();
    if hits.len() > MAX_RECEIPT_TARGETS
        || hits
            .iter()
            .any(|hit| hit.len() > MAX_RECEIPT_TEXT_BYTES || !is_repository_relative(hit))
    {
        return Err(RetrievalReceiptError::EvidenceInvalid);
    }
    hits.sort();
    if hits.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(RetrievalReceiptError::PaginationInconsistent);
    }

    let mut entries = response.entries;
    for entry in &mut entries {
        entry.value = canonical_hit(&entry.value);
        entry
            .gap_ranges
            .sort_by_key(|range| (range.start, range.end));
    }
    entries.sort_by(|left, right| {
        (left.target, left.value.as_str()).cmp(&(right.target, right.value.as_str()))
    });

    let mut body = RetrievalCoverageReceiptBody {
        plan_binding: request.plan_binding.clone(),
        step: request.step.clone(),
        scope: request.plan_binding.scope.clone(),
        snapshots: request.plan_binding.snapshots.clone(),
        provider: request.provider.clone(),
        broker_record: broker_binding,
        confidence: response.confidence,
        coverage: response.coverage,
        requested_paths: request
            .requested_paths
            .iter()
            .map(|value| canonical_hit(value))
            .collect(),
        negative_scopes: request
            .negative_scopes
            .iter()
            .map(|value| canonical_hit(value))
            .collect(),
        entries,
        pages: response.pages,
        declared_limits: request.limits,
        received_limits,
        total_results: response.total_results,
        hits,
        fallbacks: Vec::new(),
    };
    let source_outcome = if negative_proof_complete(&body) {
        RetrievalFallbackOutcome::NotRequired
    } else {
        RetrievalFallbackOutcome::Unavailable
    };
    body.fallbacks = vec![
        RetrievalFallbackReceipt {
            kind: RetrievalFallbackKind::Source,
            outcome: source_outcome,
        },
        RetrievalFallbackReceipt {
            kind: RetrievalFallbackKind::Reindex,
            outcome: RetrievalFallbackOutcome::NotRequired,
        },
    ];
    Ok(body)
}

fn validate_request_targets(request: &StructuralIndexRequest) -> Result<(), RetrievalReceiptError> {
    if request.limits.max_bytes > MAX_JSON_SAFE_INTEGER {
        return Err(RetrievalReceiptError::LimitExceeded);
    }
    if request.plan_binding.schema_id != DEVELOPMENT_ENVELOPE_SCHEMA_ID
        || !plan_matches_binding(request)
        || !has_supported_retrieval_plan_major(&request.plan_binding.document_version)
        || !has_supported_retrieval_plan_major(&request.plan_binding.schema_version)
        || request.provider.tool.is_empty()
        || request.provider.tool.len() > 128
        || request.provider.action.is_empty()
        || request.provider.action.len() > 128
    {
        return Err(RetrievalReceiptError::EvidenceInvalid);
    }
    let total = request
        .requested_paths
        .len()
        .checked_add(request.negative_scopes.len())
        .ok_or(RetrievalReceiptError::LimitExceeded)?;
    if total > request.limits.max_results as usize || total > MAX_RECEIPT_TARGETS {
        return Err(RetrievalReceiptError::LimitExceeded);
    }
    let mut seen = BTreeSet::new();
    for value in request
        .requested_paths
        .iter()
        .chain(request.negative_scopes.iter())
    {
        let canonical = canonical_hit(value);
        if canonical.len() > MAX_RECEIPT_TEXT_BYTES
            || !is_repository_relative(&canonical)
            || !seen.insert(canonical)
        {
            return Err(RetrievalReceiptError::EvidenceInvalid);
        }
    }
    Ok(())
}

fn plan_matches_binding(request: &StructuralIndexRequest) -> bool {
    let plan = &request.plan;
    let binding = &request.plan_binding;
    plan.kind == DevelopmentKind::RetrievalPlan
        && development_api_version_major(&plan.api_version) == Some(SUPPORTED_DEVELOPMENT_API_MAJOR)
        && plan_digest_matches(plan)
        && plan.metadata.id == binding.artifact_id
        && plan.metadata.artifact_version == binding.document_version
        && plan.metadata.scope == binding.scope
        && plan.producer == binding.producer
        && plan.digest == binding.digest
}

fn plan_digest_matches(plan: &DevelopmentEnvelope) -> bool {
    let actual = sha2::Sha256::digest(plan.digest_input().as_bytes());
    plan.digest.as_str() == format!("sha256:{}", hex::encode(actual))
}

fn has_supported_retrieval_plan_major(version: &SemanticVersion) -> bool {
    version
        .as_str()
        .split_once('.')
        .is_some_and(|(major, _)| major == SUPPORTED_RETRIEVAL_PLAN_MAJOR)
}

fn validate_wire_size(wire: &RetrievalCoverageReceiptV1) -> Result<(), RetrievalReceiptError> {
    let bytes = serde_json::to_vec(wire).map_err(|_| RetrievalReceiptError::InvalidWire)?;
    if bytes.len() > MAX_RECEIPT_WIRE_BYTES {
        return Err(RetrievalReceiptError::LimitExceeded);
    }
    Ok(())
}

fn validate_coverage_entries(
    request: &StructuralIndexRequest,
    overall: CoverageState,
    entries: &[RetrievalCoverageEntry],
) -> Result<(), RetrievalReceiptError> {
    if entries.len() > MAX_RECEIPT_TARGETS {
        return Err(RetrievalReceiptError::LimitExceeded);
    }
    let requested_paths = request
        .requested_paths
        .iter()
        .map(|value| canonical_hit(value))
        .collect::<BTreeSet<_>>();
    let negative_scopes = request
        .negative_scopes
        .iter()
        .map(|value| canonical_hit(value))
        .collect::<BTreeSet<_>>();
    let mut seen = BTreeSet::new();
    for entry in entries {
        let value = canonical_hit(&entry.value);
        let requested = match entry.target {
            RetrievalCoverageTarget::Path => requested_paths.contains(&value),
            RetrievalCoverageTarget::NegativeScope => negative_scopes.contains(&value),
        };
        if !requested
            || !seen.insert((entry.target, value))
            || entry.value.len() > MAX_RECEIPT_TEXT_BYTES
            || entry.gap_ranges.len() > MAX_RECEIPT_TEXT_BYTES
            || entry.gap_ranges.iter().any(|range| {
                range.start == 0
                    || range.start > MAX_JSON_SAFE_INTEGER
                    || range.end > MAX_JSON_SAFE_INTEGER
                    || range.end < range.start
            })
            || (entry.coverage == CoverageState::Complete && !entry.gap_ranges.is_empty())
        {
            return Err(RetrievalReceiptError::EvidenceInvalid);
        }
        if overall == CoverageState::Complete && entry.coverage != CoverageState::Complete {
            return Err(RetrievalReceiptError::CoveragePromotion);
        }
    }
    Ok(())
}

fn validate_pages_and_limits(
    request: &StructuralIndexRequest,
    response: &crate::ports::StructuralIndexResponse,
) -> Result<RetrievalReceivedLimits, RetrievalReceiptError> {
    let Some(last) = response.pages.last() else {
        return Err(RetrievalReceiptError::PaginationUnfinished);
    };
    if response.pages.len() > MAX_RECEIPT_PAGES {
        return Err(RetrievalReceiptError::LimitExceeded);
    }
    if last.has_more
        || response.pages[..response.pages.len() - 1]
            .iter()
            .any(|page| !page.has_more)
    {
        return Err(RetrievalReceiptError::PaginationUnfinished);
    }
    let mut positions = BTreeSet::new();
    if response.pages.iter().any(|page| {
        page.position.is_empty()
            || page.position.len() > MAX_RECEIPT_TEXT_BYTES
            || !positions.insert(page.position.clone())
    }) {
        return Err(RetrievalReceiptError::PaginationLoop);
    }
    let results = response.pages.iter().try_fold(0u64, |total, page| {
        total.checked_add(u64::from(page.results))
    });
    let bytes = response
        .pages
        .iter()
        .try_fold(0u64, |total, page| total.checked_add(page.bytes));
    let (Some(results), Some(bytes)) = (results, bytes) else {
        return Err(RetrievalReceiptError::LimitExceeded);
    };
    if results != u64::from(response.total_results) || results != response.hits.len() as u64 {
        return Err(RetrievalReceiptError::PaginationInconsistent);
    }
    let pages =
        u32::try_from(response.pages.len()).map_err(|_| RetrievalReceiptError::LimitExceeded)?;
    let tokens_u64 = bytes.div_ceil(4);
    let tokens = u32::try_from(tokens_u64).map_err(|_| RetrievalReceiptError::LimitExceeded)?;
    if results > u64::from(request.limits.max_results)
        || pages > request.limits.max_pages
        || bytes > request.limits.max_bytes
        || tokens > request.limits.max_tokens
    {
        return Err(RetrievalReceiptError::LimitExceeded);
    }
    Ok(RetrievalReceivedLimits {
        results: u32::try_from(results).map_err(|_| RetrievalReceiptError::LimitExceeded)?,
        pages,
        bytes,
        tokens,
    })
}

fn broker_binding(
    provider: &RetrievalProviderBinding,
    record: &ToolCallRecord,
    received_bytes: u64,
) -> Result<RetrievalBrokerRecordBinding, RetrievalReceiptError> {
    if record.tool != provider.tool
        || record.action != provider.action
        || !matches!(
            record.disposition,
            ToolDisposition::Completed { exit_code: 0 }
        )
        || record.truncated
        || record.stdout_bytes != received_bytes
        || record.stdout_bytes > MAX_JSON_SAFE_INTEGER
        || record.stderr_bytes > MAX_JSON_SAFE_INTEGER
        || record.actor.is_empty()
        || record.actor.len() > 128
        || !is_raw_sha256(&record.stdout_sha256)
        || !is_raw_sha256(&record.stderr_sha256)
    {
        return Err(RetrievalReceiptError::BrokerRecordInvalid);
    }
    let record_value =
        serde_json::to_value(record).map_err(|_| RetrievalReceiptError::BrokerRecordInvalid)?;
    let record_digest = hash_canonical(&record_value)?;
    Ok(RetrievalBrokerRecordBinding {
        tool: record.tool.clone(),
        action: record.action.clone(),
        actor: record.actor.clone(),
        record_digest,
        stdout_digest: WireHash::parse(format!("sha256:{}", record.stdout_sha256))
            .map_err(|_| RetrievalReceiptError::BrokerRecordInvalid)?,
        stdout_bytes: record.stdout_bytes,
        stderr_digest: WireHash::parse(format!("sha256:{}", record.stderr_sha256))
            .map_err(|_| RetrievalReceiptError::BrokerRecordInvalid)?,
        stderr_bytes: record.stderr_bytes,
        reused: record.reused,
    })
}

fn negative_proof_complete(body: &RetrievalCoverageReceiptBody) -> bool {
    if body.confidence != graphhelm_protocols::ProviderCoverageConfidence::Verified
        || body.coverage != CoverageState::Complete
        || body.pages.last().is_none_or(|page| page.has_more)
        || body.negative_scopes.is_empty()
    {
        return false;
    }
    let complete = |target, value: &str| {
        body.entries.iter().any(|entry| {
            entry.target == target
                && canonical_hit(&entry.value) == canonical_hit(value)
                && entry.coverage == CoverageState::Complete
                && entry.gap_ranges.is_empty()
        })
    };
    body.requested_paths
        .iter()
        .all(|value| complete(RetrievalCoverageTarget::Path, value))
        && body
            .negative_scopes
            .iter()
            .all(|value| complete(RetrievalCoverageTarget::NegativeScope, value))
}

fn receipt_digest(
    body: &RetrievalCoverageReceiptBody,
    schema_version: &SemanticVersion,
) -> Result<WireHash, RetrievalReceiptError> {
    let input = serde_json::json!({
        "apiVersion": RETRIEVAL_COVERAGE_RECEIPT_API_VERSION,
        "kind": RETRIEVAL_COVERAGE_RECEIPT_KIND,
        "schemaVersion": schema_version,
        "body": body,
    });
    hash_canonical(&input)
}

fn hash_canonical(value: &serde_json::Value) -> Result<WireHash, RetrievalReceiptError> {
    let digest = sha2::Sha256::digest(canonical_json(value).as_bytes());
    WireHash::parse(format!("sha256:{}", hex::encode(digest)))
        .map_err(|_| RetrievalReceiptError::InvalidWire)
}

fn is_raw_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// A compiled plan, or the typed refusal that replaced it.
#[derive(Debug, PartialEq, Eq)]
pub enum RetrievalOutcome {
    Claim {
        hits: Vec<String>,
        /// The coverage the hits were found under.
        ///
        /// Carried rather than dropped: a partial search returning three hits reports a FLOOR, a
        /// complete one returning three reports a TOTAL, and a caller handed the bare list cannot
        /// tell which it was given. The input boundary refuses to hand over results without this
        /// verdict; the output boundary must not undo that.
        coverage: CoverageState,
    },
    /// A zero that coverage licenses as a fact about the subject.
    VerifiedAbsence,
    Refused {
        code: DevelopmentRefusalCode,
    },
}

/// Compile a retrieval plan from an index response bound to a snapshot pair.
pub fn compile_plan(binding: &SnapshotBinding, response: &IndexResponse) -> RetrievalOutcome {
    // The binding is consulted BEFORE coverage, and the order is the point. Coverage is the
    // provider's claim about its own search; the binding is a fact about which bytes the
    // coordinates were computed against. A provider reporting `Complete` over a stale binding is
    // the most confident wrong answer available, so the binding wins.
    //
    // There are two independent staleness signals and they are not redundant: this one is
    // staleness DETECTED here, the `CoverageState::Stale` arm below is the provider DECLARING it.
    // Either alone must refuse.
    if !binding.is_fresh() {
        return RetrievalOutcome::Refused {
            code: DevelopmentRefusalCode::IndexStale,
        };
    }
    if response.hits.is_empty() {
        // Matched exhaustively rather than with a wildcard, and that is the point: `CoverageState`
        // is a CLOSED set, so a state added upstream must break this build instead of silently
        // falling into whatever the catch-all happened to say. A `_ =>` arm here would make the
        // closed-world claim shrink in silence the day a variant is added.
        return match response.coverage {
            CoverageState::Complete => RetrievalOutcome::VerifiedAbsence,
            // Stale is NOT pooled with the rest: it is the one state whose repair is known.
            // Reindexing answers staleness and says nothing about an unfinished search, so a
            // caller that wants to fix the situation needs the two told apart.
            CoverageState::Stale => RetrievalOutcome::Refused {
                code: DevelopmentRefusalCode::IndexStale,
            },
            CoverageState::Partial
            | CoverageState::Excluded
            | CoverageState::Skipped
            | CoverageState::ExtractionGap
            | CoverageState::Unknown
            | CoverageState::Unresolved => RetrievalOutcome::Refused {
                code: DevelopmentRefusalCode::NegativeClaimUnverified,
            },
        };
    }
    // Provider output is untrusted typed evidence, and a hit is a path this plan would hand to a
    // reader. Validated BEFORE the claim is built, so nothing escaping ever reaches a reader.
    if response.hits.iter().any(|hit| !is_repository_relative(hit)) {
        return RetrievalOutcome::Refused {
            code: DevelopmentRefusalCode::ScopeMismatch,
        };
    }
    RetrievalOutcome::Claim {
        hits: response.hits.iter().map(|hit| canonical_hit(hit)).collect(),
        coverage: response.coverage,
    }
}

/// One file named two ways is one hit.
///
/// This repository is developed on Windows and runs on Linux, so the same file arrives with one
/// separator from one provider and the other from another. Carrying the provider's separators into
/// the plan splits every downstream identity — digests, caches, comparisons — by the platform the
/// provider happened to run on, and the split is silent because both plans look right.
///
/// Canonicalised where the claim is BUILT rather than at every point one is compared: a
/// normalisation each consumer has to remember is one some consumer will forget.
fn canonical_hit(hit: &str) -> String {
    hit.replace('\\', "/")
}

/// Whether a provider-supplied hit stays inside the repository.
///
/// Checked by SEGMENT rather than by substring: a substring test for `".."` also rejects the
/// perfectly ordinary `src/..foo.rs`, and a rule that fires on innocent input gets relaxed by the
/// next person who hits it.
fn is_repository_relative(hit: &str) -> bool {
    // A hit may carry a trailing `:<line>`, and on Windows `:` also separates a DRIVE. Splitting on
    // the first colon conflates the two: `C:/Windows/System32` yields `"C"`, which has no `..` and
    // no leading slash, so a drive-qualified escape reads as an ordinary relative path. Strip only
    // a trailing all-digit suffix, from the END.
    let path = match hit.rsplit_once(':') {
        Some((head, tail)) if !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()) => head,
        _ => hit,
    };

    if path.is_empty() {
        return false;
    }
    // Rooted at a separator: absolute POSIX, or a Windows UNC/rooted path.
    if path.starts_with('/') || path.starts_with('\\') {
        return false;
    }
    // Drive-qualified (`C:` or `C:/...`): an escape carrying no `..` at all.
    let mut chars = path.chars();
    if matches!((chars.next(), chars.next()), (Some(letter), Some(':')) if letter.is_ascii_alphabetic())
    {
        return false;
    }
    // Parent traversal, checked by SEGMENT rather than substring: a substring test for `".."` also
    // rejects the perfectly ordinary `src/..foo.rs`, and a rule that fires on innocent input gets
    // relaxed by the next person who hits it.
    !path.split(['/', '\\']).any(|segment| segment == "..")
}

/// Compile a plan, verifying the bytes a reader would serve against the binding first.
///
/// This is the half [`compile_plan`] structurally cannot do. A [`SnapshotBinding`] compares its two
/// ids to each other, so it catches staleness that was already visible and is blind to the case
/// where both ids agree and the bytes underneath moved. Asking the reader what it would actually
/// serve is the only way to reach that case, and it is why `repo_snapshot` must be derived from
/// CONTENT rather than from a ref — see [`crate::ports::SourceReader`].
pub fn compile_plan_against<R: crate::ports::SourceReader + ?Sized>(
    binding: &SnapshotBinding,
    response: &IndexResponse,
    reader: &R,
) -> RetrievalOutcome {
    if reader.current_snapshot() != binding.repo_snapshot {
        return RetrievalOutcome::Refused {
            code: DevelopmentRefusalCode::IndexStale,
        };
    }
    compile_plan(binding, response)
}

/// Compile a plan under declared bounds.
///
/// **Order: the plan is compiled FIRST, bounds are applied to what it produced.** A stale binding
/// or an escaping path invalidates the response outright; the payload being too large is a fact
/// about a response that was otherwise usable. Reporting the budget over the staleness sends the
/// caller to trim their query, which is the wrong repair and leaves the stale index in place.
/// (Found by F: the first version checked bounds first and returned `cardinality_violation` for a
/// response that was ALSO stale.)
///
/// Bounds are then enforced HERE, over whatever the provider actually returned, rather than trusted
/// to the provider: a flooding provider is the threat the bound exists for, so asking it to respect
/// a limit it is the one violating is not a bound at all.
///
/// Over-budget REFUSES rather than truncating. A silently truncated result set is a partial search
/// wearing a complete search's clothes: the caller sees a plausible number of hits under a
/// `Complete` coverage verdict and no way to tell the rest were dropped.
pub fn compile_plan_within(
    binding: &SnapshotBinding,
    response: &IndexResponse,
    limits: &DeclaredLimits,
) -> RetrievalOutcome {
    let outcome = compile_plan(binding, response);
    if !matches!(outcome, RetrievalOutcome::Claim { .. }) {
        return outcome;
    }

    if response.hits.len() as u64 > u64::from(limits.max_results) {
        return RetrievalOutcome::Refused {
            code: DevelopmentRefusalCode::CardinalityViolation,
        };
    }
    // Checked separately from the result bound because they fail independently: many tiny hits are
    // under the byte bound and over the result bound, and one enormous hit is the reverse. Measured
    // over the representation this plan would carry.
    let payload_bytes = response
        .hits
        .iter()
        .map(|hit| hit.len() as u64)
        .sum::<u64>()
        + response.summary.as_ref().map_or(0, |s| s.len() as u64);
    if payload_bytes > limits.max_bytes {
        return RetrievalOutcome::Refused {
            code: DevelopmentRefusalCode::ArtifactTooLarge,
        };
    }
    if response.pages > limits.max_pages {
        return RetrievalOutcome::Refused {
            code: DevelopmentRefusalCode::CardinalityViolation,
        };
    }
    // Estimated, deliberately crude, and deliberately OURS. A provider's own token accounting is a
    // self-report about the very thing it is being bounded on. Four bytes per token is wrong in
    // detail and right in the property that matters: it grows with the payload the caller pays for.
    if payload_bytes.div_ceil(4) > u64::from(limits.max_tokens) {
        return RetrievalOutcome::Refused {
            code: DevelopmentRefusalCode::ArtifactTooLarge,
        };
    }
    outcome
}

/// Whether bounded source fallback exists yet. It does not.
///
/// This is not a feature flag and must never become one. It exists so a guard can assert the REASON
/// a non-complete coverage state refuses, rather than only that it refuses.
///
/// Today `ExtractionGap` refuses because there is nothing to fall back TO. The acceptance criterion
/// gives that state two exits — bounded source fallback, or `negative_claim_unverified` — and only
/// the second is built. "Refused after trying the source" and "refused because no source path
/// exists" are identical in the outcome and are different facts.
///
/// **When fallback lands, flip this to `true` and the guard that reads it goes RED on purpose.**
/// That is the point: an assertion that survives the change which makes it meaningless is the one
/// nobody looks at again. Whoever builds the fallback is then forced to rewrite that arm
/// deliberately, asserting that the fallback was ATTEMPTED, instead of inheriting a green cell that
/// silently changed meaning underneath them.
#[must_use]
pub const fn source_fallback_available() -> bool {
    false
}
