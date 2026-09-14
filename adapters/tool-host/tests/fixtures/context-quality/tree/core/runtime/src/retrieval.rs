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

/// The `StructuralCodeIndex` that ships until a real producer exists.
pub struct UnavailableStructuralCodeIndex;

impl crate::ports::StructuralCodeIndex for UnavailableStructuralCodeIndex {
    fn retrieve(
        &self,
        _request: &StructuralIndexRequest,
    ) -> Result<crate::ports::StructuralIndexResponse, crate::ports::StructuralCodeIndexError> {
        Err(crate::ports::StructuralCodeIndexError::Unavailable)
    }
}

/// One retrieval attempt, compiled to an outcome a caller may act on.
///
/// This exists so that "a failed attempt is not an absence" is decided here, once, for every
/// caller that comes through this door -- rather than by each caller at the moment it first needs
/// an answer. Before it, the only implementation of the port lived in a test file and
/// `retrieve_coverage` had no production caller at all: the decision was unmade, and the cheapest
/// shape available to whoever made it first is an empty result, which `compile_receipt` is
/// entitled to read as proven absence.
///
/// **The limit, stated because the stronger sentence is the tempting one.** `retrieve_coverage`
/// remains `pub`, so this is the safe door and not the only one: an integrator can still call it
/// directly and interpret the `Err` alone. What ships is a correct answer available to everyone,
/// not an incorrect answer made unreachable. Whether that door narrows -- `pub(crate)` costs one
/// import today -- is #219's decision and not this slice's.
///
/// **No error arm may produce `Claim` or `VerifiedAbsence`.** Those two are claims about the
/// SUBJECT; an error is a fact about the ATTEMPT, and an instrument that did not speak has
/// established nothing about what it was pointed at.
#[must_use]
pub fn compile_attempt<
    I: crate::ports::StructuralCodeIndex + ?Sized,
    R: crate::ports::SourceReader + ?Sized,
>(
    index: &I,
    reader: &R,
    request: &StructuralIndexRequest,
) -> RetrievalOutcome {
    let error = match retrieve_coverage(index, reader, request) {
        Ok(receipt) => return compile_receipt(&receipt),
        Err(error) => error,
    };
    // Matched exhaustively rather than with a wildcard, for the reason `compile_plan` gives about
    // `CoverageState`: a variant added upstream must break this build instead of falling into
    // whatever the catch-all happened to say. A refusal vocabulary that shrinks in silence is the
    // failure this whole path exists to prevent.
    let code = match error {
        // Staleness is NOT pooled, and the split is the same one `compile_plan` already makes:
        // it is the single failure whose repair is known and nameable, so a caller who wants to
        // fix the situation needs it told apart from "the search was never finished".
        RetrievalReceiptError::IndexStale => DevelopmentRefusalCode::IndexStale,
        // Everything else pools deliberately. These differ in WHERE Runtime caught the provider
        // out, and not one of them changes what the caller may now say about the subject: the
        // evidence did not survive validation, so no negative claim is verified. Pooling here is
        // a statement about the caller's licence, not an admission that the causes are alike --
        // the cause is already named by `RetrievalReceiptError`, which is what a debugger reads.
        RetrievalReceiptError::ProviderUnavailable
        | RetrievalReceiptError::BindingMismatch
        | RetrievalReceiptError::BrokerRecordInvalid
        | RetrievalReceiptError::CoveragePromotion
        | RetrievalReceiptError::PaginationUnfinished
        | RetrievalReceiptError::PaginationLoop
        | RetrievalReceiptError::PaginationInconsistent
        | RetrievalReceiptError::LimitExceeded
        | RetrievalReceiptError::EvidenceInvalid
        | RetrievalReceiptError::InvalidWire
        | RetrievalReceiptError::DigestMismatch => DevelopmentRefusalCode::NegativeClaimUnverified,
    };
    RetrievalOutcome::Refused { code }
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
    // #226 S2: the plan may not claim MORE than the producer covered. `"complete"` is a legal
    // token, so nothing about the plan alone is wrong -- the lie only exists next to a producer
    // record that says it searched nothing (`extraction_gap`: "parser failed on 3 of 3 candidate
    // files"). No schema can see that: the two artifacts are false only TOGETHER, which is why
    // this check lives at the one site that holds both.
    //
    // Same error as the confidence check above and for the same reason, one artifact over: there a
    // RECEIPT's confidence cannot support its state, here a PLAN's claim cannot be supported by the
    // coverage that was actually achieved.
    if request
        .plan
        .spec
        .get("coverage")
        .and_then(serde_json::Value::as_str)
        == Some(CoverageState::Complete.wire_name())
        && response.coverage != CoverageState::Complete
    {
        return Err(RetrievalReceiptError::CoveragePromotion);
    }

    let received_limits = validate_pages_and_limits(request, &response)?;
    let broker_binding = broker_binding(
        &request.provider,
        &response.broker_record,
        received_limits.bytes,
    )?;

    // The RAW length is bounded BEFORE `canonical_hit` (Codex #608): the canonicalizer trims
    // trailing slashes, so a short name followed by thousands of `/` shrinks under
    // `MAX_RECEIPT_TEXT_BYTES` after being scanned and copied in full — the oversized raw value
    // would be admitted despite the hard per-text ceiling. Checked on the raw hit first.
    if response
        .hits
        .iter()
        .any(|hit| hit.len() > MAX_RECEIPT_TEXT_BYTES)
    {
        return Err(RetrievalReceiptError::EvidenceInvalid);
    }
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
        || !plan_coverage_is_a_closed_token(request)
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
        // RAW length first (Codex #608): `canonical_hit` trims trailing slashes, so the ceiling
        // must see the pre-canonical value or a slash-flood is admitted after being scanned.
        if value.len() > MAX_RECEIPT_TEXT_BYTES {
            return Err(RetrievalReceiptError::EvidenceInvalid);
        }
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

/// The plan's claimed coverage must be a token from the CLOSED set -- #226 S2.
///
/// `CoverageState` is declared closed where it is defined ("each state has a DIFFERENT correct
/// response to a zero result"), and the extension package's envelope schema repeats the eight
/// tokens under `$defs/coverageState`. That definition was referenced by nothing: a closed
/// vocabulary with no consumer, so a plan could claim any string.
///
/// The consumer is here rather than in the schema, and the honest reason is narrower than the one
/// this comment first gave. It cited the envelope's `spec` description as deciding that per-kind
/// payload is not guarded at the schema -- a MISREADING, spliced across two clauses: that sentence
/// says THE ORDER (validate-before-deserialize) is the consumer's obligation and is not guarded
/// there. It decides nothing about coverage.
///
/// What the same description DOES say is the opposite of an exemption: "The nine per-kind schemas
/// need this decision taken consciously; they do not inherit it." There is no `RetrievalPlan`
/// per-kind schema at `origin/main` -- eight schemas exist and that kind is not among them -- so
/// there is today no schema to carry this check, and the runtime is where the two artifacts meet.
/// That is a reason to guard here NOW, not a ruling that the schema must never guard it.
///
/// WHAT THIS COSTS, AND THE CONDITION THAT REOPENS IT. `spec` is untyped so a compatible minor's
/// unknown fields round-trip, and AGENTS.md requires those fields be PRESERVED. They are: `spec` is
/// held whole, the digest covers it, and an older reader still round-trips it byte-identically --
/// this predicate refuses a REQUEST, it never drops or rewrites a field. What it does cost is
/// interpretation: a future minor that puts something other than one of the eight tokens at
/// `spec.coverage` is refused rather than carried.
///
/// Accepted deliberately, because the population is narrow and the meaning is not invented here.
/// The gate this joins has already established `kind == RetrievalPlan`, so `coverage` is not an
/// unknown field in an unknown document -- it is the field the committed sabotage corpus produces
/// with exactly this meaning (`fixtures/sabotage/s2-false-structural-absence/`), and refusing an
/// unrecognised member of a closed set is what this codebase does elsewhere: "an unknown kind is
/// refused, never ignored".
///
/// REOPEN THIS if a normative `RetrievalPlan` per-kind schema gives `spec.coverage` a shape other
/// than one of the eight tokens. This predicate is the site to change, and the corpus above is the
/// population to re-measure first.
fn plan_coverage_is_a_closed_token(request: &StructuralIndexRequest) -> bool {
    let Some(claimed) = request.plan.spec.get("coverage") else {
        return true;
    };
    claimed.as_str().is_some_and(|token| {
        CoverageState::every()
            .iter()
            .any(|state| state.wire_name() == token)
    })
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
    // Validated on the CANONICAL form, because the canonical form is what is ADMITTED below
    // (Codex #608 P1): `is_repository_relative` on the raw spelling accepted `.//etc/passwd`
    // (leading `.`, no `..` segment), and `canonical_hit` then stripped the `./` to `/etc/passwd`
    // -- an absolute escape minted AFTER the check. Validate what will be stored, the same order
    // the receipt path at `canonical_hit(value)` + `is_repository_relative(&canonical)` uses.
    if response
        .hits
        .iter()
        .any(|hit| !is_repository_relative(&canonical_hit(hit)))
    {
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
    // Same three spellings the benchmark's `canonical_path` folds, for the same reason: `./x`,
    // `x` and `x/` are one file wearing three names, and two of them surviving into a claim is
    // the split this function exists to close (caught by the sorted-union cell on #608 -- the
    // separator swap alone kept the promise "one file named two ways stays one hit" only for
    // the separator). Case is NOT folded: identity here is byte identity, and case-folding
    // equates files that genuinely differ on Unix.
    let slashed = hit.replace('\\', "/");
    let stripped = slashed.strip_prefix("./").unwrap_or(&slashed);
    stripped.trim_end_matches('/').to_owned()
}

/// Whether a provider-supplied hit stays inside the repository.
///
/// Checked by SEGMENT rather than by substring: a substring test for `".."` also rejects the
/// perfectly ordinary `src/..foo.rs`, and a rule that fires on innocent input gets relaxed by the
/// next person who hits it.
fn is_repository_relative(hit: &str) -> bool {
    // ALLOW-LIST BY FORM, not a deny-list of known-bad shapes (Codex #608, the whole Win32
    // path-semantics class in one rule). Enumerating the class edge by edge -- drive-relative,
    // rooted, UNC, `\\?\`, alternate data streams, reserved device names, 8.3 short names,
    // trailing dots/spaces, full-width and mixed separators, control and non-ASCII bytes -- is a
    // race the enumerator wins, one reopened finding per edge. Instead a repository-relative path
    // is DEFINED positively: an optional trailing `:<digits>` line suffix (index hits carry one),
    // then a non-empty sequence of components joined by single '/', each a `[A-Za-z0-9._-]+` that
    // is not `.`/`..`, does not end in `.`, and is not a reserved DOS device. Every escape in the
    // class fails one clause BY CONSTRUCTION: `\`, `:`, space, control, full-width slash or any
    // non-ASCII byte is outside the component charset; a leading/trailing/doubled '/' makes an
    // empty component; a drive `C:` is caught on the raw hit before the line-suffix strip could
    // hide it as a bare `C`.
    //
    // Measured safe for this validator's population: every path in the served tree and the frozen
    // corpus is already within `[A-Za-z0-9._/-]`, so the allow-list rejects no legitimate evidence.
    let mut raw = hit.chars();
    if matches!((raw.next(), raw.next()), (Some(letter), Some(':')) if letter.is_ascii_alphabetic())
    {
        return false;
    }
    let path = match hit.rsplit_once(':') {
        Some((head, tail)) if !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()) => head,
        _ => hit,
    };
    if path.is_empty() {
        return false;
    }
    path.split('/').all(is_safe_component)
}

/// One component a repository-relative path may contain: non-empty, drawn from `[A-Za-z0-9._-]`,
/// not `.` or `..`, not ending in `.` (Win32 strips a trailing dot, so `foo.` and `foo` would be
/// one file under two spellings), and not a reserved DOS device stem. `..foo` is a real filename
/// and passes; `..`, `.. ` (space, rejected by the charset), `...` (trailing dot) do not.
fn is_safe_component(component: &str) -> bool {
    !component.is_empty()
        && component != "."
        && component != ".."
        && !component.ends_with('.')
        && component
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-')
        && !is_reserved_device_component(component)
}

/// Whether one path component is a Windows reserved DOS device name (case-insensitive, judged on
/// the stem before the first `.`, since `CON.txt` still resolves to the `CON` device).
fn is_reserved_device_component(component: &str) -> bool {
    // Win32 strips trailing spaces and dots from a component before resolving it, so `NUL ` and
    // `COM1.` resolve to the device (Codex #608). Trim them before extracting the stem, the same
    // normalization the parent-traversal check applies.
    let trimmed = component.trim_end_matches([' ', '.']);
    let stem = trimmed.split('.').next().unwrap_or(trimmed);
    let upper = stem.to_ascii_uppercase();
    matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (upper.len() == 4
            && (upper.starts_with("COM") || upper.starts_with("LPT"))
            && matches!(upper.as_bytes()[3], b'1'..=b'9'))
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

/// Whether bounded source fallback exists. It does, through [`compile_plan_composed_against`].
///
/// This is not a feature flag and must never become one. It exists so a guard can assert the REASON
/// a non-complete coverage state refuses, rather than only that it refuses.
///
/// **What it claims, precisely (K on #608): the fallback PATH exists in the runtime — not that a
/// producer is present.** The only entry point is `compile_plan_composed_against<R: SourceReader>`,
/// which takes the `BoundedSourceSearch` channel as an INJECTED parameter; no variant constructs a
/// producer internally. So "true" is correct on this branch: the path is real and the caller
/// supplies the channel. #622 adds a workspace-backed IMPLEMENTATION of the port, not the path; the
/// production CONSUMER is tracked in #724 (#223/#224 closed without it, see #722). A
/// presence-of-producer signal, if ever wanted, is a DIFFERENT symbol derived from the injected
/// channel — not this `const fn`.
///
/// **The scope of what landed, stated precisely, because this constant is read as a claim.**
/// `compile_plan_composed` consults the channel when a claim's coverage is non-complete. A
/// ZERO-hit response never reaches that point: `compile_plan` maps empty+non-complete to
/// `negative_claim_unverified` before composition begins. So `ExtractionGap` with no hits still
/// refuses without trying the source — the fallback exists for hits-without-the-required-path,
/// and the zero-hit exit is still unbuilt. Whoever builds THAT rewrites the arm again.
#[must_use]
pub const fn source_fallback_available() -> bool {
    true
}

/// Compile a plan with a bounded source channel available to non-complete coverage (#219).
///
/// **This is a CONTRACT AMENDMENT, declared as one.** `compile_plan` reads the coverage state
/// only when the index returned nothing, so today a partial search that returned the WRONG hits
/// is indistinguishable from one that returned the right ones: zero-hits and
/// hits-without-the-required-path are different failure modes and only the first had an exit.
/// Here `Partial` — which a best-effort provider reports on EVERY call — licenses one bounded
/// consultation of the source channel, whose paths JOIN the claim.
///
/// `Complete` never consults: a complete search has nothing to fall back from, and since the
/// live provider is best-effort by construction, an unconditional consult would put a
/// workspace walk in every compile.
///
/// **The reader and the limits are REQUIRED, not offered** (Codex on #608, second round):
///
/// - A readerless overload existed and let any caller compose the channel's CURRENT bytes onto
///   an older binding by simply not asking whether the workspace moved. Deleting it is the fix;
///   documenting the hole at its signature — which the overload did — was the hole with a label.
/// - The composed path ran the index half through the unbounded `compile_plan`, so a flooding
///   index response that `compile_plan_within` refuses could still buy composition. The index
///   half now inherits the caller's `DeclaredLimits`, and the COMPOSED claim re-enters the
///   result ceiling after the union — a bound checked before an append is not a bound on the
///   appended result.
///
/// The channel's typed failure is RETURNED beside the outcome: `Unavailable` (wire a channel)
/// and `BoundExceeded` (narrow the query or raise a bound) send an operator in opposite
/// directions, and the channel is EVIDENCE, not authority — its failure leaves the index's own
/// claim standing.
pub fn compile_plan_composed_against<R: crate::ports::SourceReader + ?Sized>(
    binding: &SnapshotBinding,
    response: &IndexResponse,
    reader: &R,
    channel: &dyn crate::ports::BoundedSourceSearch,
    terms: &[String],
    bounds: &crate::ports::SourceSearchBounds,
    limits: &DeclaredLimits,
) -> (RetrievalOutcome, Option<crate::ports::SourceSearchError>) {
    // BEFORE anything else: a workspace that moved makes the channel's current bytes and the
    // index's older claim two different subjects.
    if reader.current_snapshot() != binding.repo_snapshot {
        return (
            RetrievalOutcome::Refused {
                code: DevelopmentRefusalCode::IndexStale,
            },
            None,
        );
    }
    // DECLARED staleness outranks a budget refusal (Codex #608). A response that is BOTH stale
    // AND over-budget must send the caller to REINDEX (`IndexStale`), never to trim the query
    // (`ArtifactTooLarge`): stale coordinates are wrong regardless of payload size, and
    // `compile_plan_within` applies the limits first, so a stale-and-large response would be
    // masked as merely large. Checked here, before the limits, so the staleness signal wins.
    if response.coverage == CoverageState::Stale {
        return (
            RetrievalOutcome::Refused {
                code: DevelopmentRefusalCode::IndexStale,
            },
            None,
        );
    }
    // The index half is compiled UNDER the caller's declared limits (Codex on #608): a stale
    // binding or an escaping path invalidates the response outright, and a flooding response is
    // refused here exactly as it would be on the uncomposed path.
    let outcome = compile_plan_within(binding, response, limits);
    // A refusal is not a licence: staleness, an escaping path or a crossed limit invalidates the
    // response, and no evidence is gathered against an invalid response.
    let RetrievalOutcome::Claim { hits, coverage } = outcome else {
        return (outcome, None);
    };
    // The economic seal: only a non-complete search has something to fall back from. This is the
    // one branch that keeps a workspace walk out of every compile.
    if coverage == CoverageState::Complete {
        return (RetrievalOutcome::Claim { hits, coverage }, None);
    }
    // A DECLARED stale coverage has already refused above, before the limits, so the composition
    // below only ever runs on a non-complete, non-stale claim (Codex #608 moved the check up: a
    // stale-AND-over-budget response must read as stale, which `compile_plan_within` would have
    // masked as merely large).

    // The QUERY is bounded at the trust boundary, before any channel is invoked (Codex #608):
    // `terms` come from an untrusted plan, a channel searches each scanned file once per term, so
    // an oversized term slice — or overlong terms — scales the work as `files x terms` under every
    // corpus ceiling. Refused as a source-side `BoundExceeded` (the index claim stands, the query
    // crossed a ceiling), enforced HERE so it holds for every `BoundedSourceSearch` and not only
    // the implementor that caps its own input.
    // COUNT first, O(1), before summing bytes (Codex #608): an oversized term slice would
    // otherwise be walked in full by the `.sum()` before the count ceiling it should have stopped
    // is even consulted — the traversal the bound exists to prevent, run to compute the bound.
    if terms.len() > bounds.max_terms {
        return (
            RetrievalOutcome::Claim { hits, coverage },
            Some(crate::ports::SourceSearchError::BoundExceeded),
        );
    }
    // Bytes summed only over the already-count-bounded slice.
    let term_bytes: u64 = terms.iter().map(|term| term.len() as u64).sum();
    if term_bytes > bounds.max_term_bytes {
        return (
            RetrievalOutcome::Claim { hits, coverage },
            Some(crate::ports::SourceSearchError::BoundExceeded),
        );
    }
    let search_result = channel.search(terms, bounds);
    // Freshness re-checked the INSTANT the search returns, gating EVERY post-search path out of
    // here rather than only the success path (Codex #608). The pre-check ages while a bounded
    // walk runs, and a workspace edited mid-search makes even a FAILING (`Unavailable` /
    // `BoundExceeded`) or an over-result search's index claim describe a snapshot that no longer
    // exists — the earlier fix rechecked only after a successful union, so a slow failing search
    // still exposed stale coordinates. Rechecking before the match means a moved workspace
    // refuses `IndexStale` whatever the search did. The residual window (between the channel's
    // last read and this call) is unchanged and still declared; closing it needs the search to
    // carry the snapshot it ran over (receipt-boundary, #655).
    if reader.current_snapshot() != binding.repo_snapshot {
        return (
            RetrievalOutcome::Refused {
                code: DevelopmentRefusalCode::IndexStale,
            },
            None,
        );
    }
    let found = match search_result {
        Ok(found) => found,
        Err(reason) => return (RetrievalOutcome::Claim { hits, coverage }, Some(reason)),
    };
    // The channel's own result ceiling is enforced HERE, over what it actually returned, for
    // the same reason `compile_plan_within` re-checks the index's bounds: a flooding producer
    // is the threat the bound exists for, so asking IT to respect the limit is not a bound at
    // all (Codex on #608). The index's claim stands — the channel is evidence, not authority —
    // and the over-bound response carries the same typed reason a refusing channel would have
    // carried, so the operator hears "the source side crossed a ceiling" either way.
    if found.len() as u64 > u64::from(bounds.max_results) {
        return (
            RetrievalOutcome::Claim { hits, coverage },
            Some(crate::ports::SourceSearchError::BoundExceeded),
        );
    }
    // The channel's RAW payload is bounded BEFORE canonicalization, on BOTH bytes and tokens
    // (Codex #608). `canonical_hit` trims trailing slashes, so `"a"` followed by hundreds of `/`
    // shrinks toward one byte and the post-union byte/token checks never see the flood — but the
    // enormous string was already allocated and scanned to get there, and its token estimate can
    // exceed `max_tokens` even when `max_bytes` is permissive. The aggregate raw size is refused
    // the instant the search returns, before any per-path canonicalisation or set construction,
    // the same posture as the result-count ceiling: the index claim stands, the operator hears
    // `BoundExceeded`.
    // The AGGREGATE of both received payloads, not the source alone (Codex #608): the index half
    // already spent part of `max_bytes`/`max_tokens`, so giving the source a fresh full ceiling
    // lets index+source together exceed the declared budget (40 bytes of index + a 50-byte source
    // path under a 64-byte limit, both passing independently). The raw index hits and summary are
    // added in, so the ceiling bounds everything the caller received before any canonicalization.
    let index_raw: u64 = response
        .hits
        .iter()
        .map(|hit| hit.len() as u64)
        .sum::<u64>()
        + response
            .summary
            .as_ref()
            .map_or(0, |summary| summary.len() as u64);
    let raw_bytes: u64 = index_raw + found.iter().map(|hit| hit.len() as u64).sum::<u64>();
    if raw_bytes > limits.max_bytes || raw_bytes.div_ceil(4) > u64::from(limits.max_tokens) {
        return (
            RetrievalOutcome::Claim { hits, coverage },
            Some(crate::ports::SourceSearchError::BoundExceeded),
        );
    }
    // A channel hit carries NO line suffix — `BoundedSourceSearch` promises paths only — so ANY
    // colon in a source hit is illegal, and the index's `:<digits>` line-suffix exception must NOT
    // apply to it (Codex #608). `is_repository_relative` strips a trailing all-digit suffix, so it
    // would accept `src/lib.rs:123` (a numeric NTFS alternate-data-stream selector on Windows) by
    // reducing it to `src/lib.rs`. The channel is held to the stricter rule here, before that
    // shared check runs.
    if found.iter().any(|hit| hit.contains(':')) {
        return (
            RetrievalOutcome::Refused {
                code: DevelopmentRefusalCode::ScopeMismatch,
            },
            None,
        );
    }
    // SAME SHAPE, SECOND HALF (K's #608 P1). `compile_plan` validates every hit before building
    // a claim and states the invariant plainly: "nothing escaping ever reaches a reader". That
    // sentence was true of the INDEX's hits and false of the channel's, which joined the claim
    // unvalidated. The channel is another untrusted producer of paths, so it meets the same
    // check, and an escaping path refuses the WHOLE claim rather than being quietly dropped --
    // dropping it would leave a claim built from a producer that just tried to escape.
    // Validated on the CANONICAL form for the same reason as the index half (Codex #608 P1):
    // `canonical_hit` widened to strip a leading `./`, so a raw hit that passed the escape check
    // could canonicalize into an absolute path that then joined the claim. Validate what is
    // admitted.
    if found
        .iter()
        .any(|hit| !is_repository_relative(&canonical_hit(hit)))
    {
        return (
            RetrievalOutcome::Refused {
                code: DevelopmentRefusalCode::ScopeMismatch,
            },
            None,
        );
    }
    // Union, graph FIRST and order preserved; the channel's additions arrive SORTED and deduped
    // (Codex on #608): the port assigns no ranking semantics to source paths, so a filesystem's
    // enumeration order must not leak into `Claim.hits` — identical repositories must compose
    // identical claims on every filesystem. Canonicalised through the same function the index's
    // own hits go through, so one file named two ways stays one hit.
    let mut composed = hits;
    let additions: std::collections::BTreeSet<String> = {
        let existing: std::collections::BTreeSet<&str> =
            composed.iter().map(String::as_str).collect();
        found
            .iter()
            .map(|path| canonical_hit(path))
            .filter(|canonical| !existing.contains(canonical.as_str()))
            .collect()
    };
    composed.extend(additions);
    // The COMPOSED claim re-enters the caller's result ceiling: `compile_plan_within` bounded
    // the index's hits, and a union that then grows past the same ceiling would hand the caller
    // exactly the flood the limit refused (Codex on #608). Refused, never truncated — a silently
    // truncated union is a partial answer in a complete answer's clothes.
    if composed.len() as u64 > u64::from(limits.max_results) {
        return (
            RetrievalOutcome::Refused {
                code: DevelopmentRefusalCode::CardinalityViolation,
            },
            None,
        );
    }
    // No separate post-union BYTE/TOKEN check: it would be dead (Codex #608 aggregate finding).
    // The aggregate-raw ceiling above bounds `index_raw + source_raw` BEFORE canonicalization,
    // and the composed set is (canonical index hits ∪ canonical source additions, deduped) with
    // no summary — every transform from raw to composed only SHRINKS or removes bytes, so
    // `composed_bytes <= raw_bytes` always. A post-union byte check could therefore never fire
    // after the raw check passed, and a check that cannot gate is worse than none. The result
    // COUNT is different — the union can grow past `max_results` even when each half is under it —
    // so that ceiling stays, just above.
    // Coverage is NOT promoted. The composed set may be larger and the search is still the
    // best-effort search the provider reported -- a union of two incomplete answers is incomplete.
    (
        RetrievalOutcome::Claim {
            hits: composed,
            coverage,
        },
        None,
    )
}
