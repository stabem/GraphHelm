//! Development artifact contracts (#217).
//!
//! Every artifact here carries one closed envelope, is validated against its checked-in schema
//! BEFORE typed deserialization, and fails closed on an unknown major version.
//!
//! THE AUTHORITATIVE SIDE OF THE REFUSAL VOCABULARY IS THE SCHEMA, not this file. Validation runs
//! before typed deserialization, the plan makes serialization and grammar normative schema
//! concerns, and six other lanes consume these contracts, so the schema is what travels. The
//! `wire_name` match below exists so the Rust half is DERIVED rather than hand-listed: the
//! compiler refuses to build when a variant has no arm. A conformance test compares that derived
//! set against the set read from the schema file, by equality and never by count — a rename keeps
//! the count identical, which is the hole this arrangement exists to close.

use serde::{Deserialize, Serialize};

use crate::{ArtifactId, ExecutionId, OpaqueId, ProjectId, SemanticVersion, WireHash, WorkspaceId};

/// The API group these artifacts live in. The MAJOR segment is the compatibility boundary.
pub const DEVELOPMENT_API_GROUP: &str = "p50.dev/development";

/// The only major this build understands. Anything else fails closed rather than being coerced.
pub const DEVELOPMENT_API_MAJOR: u16 = 1;

/// Declare a closed wire vocabulary from ONE list.
///
/// This exists because proximity is not enforcement. The previous shape had the enum, `wire_name`
/// and `every()` written separately and kept together by discipline: the compiler forces an arm in
/// `wire_name`, but nothing forces an entry in `every()`. Measured, not feared - a variant added to
/// the enum and to `wire_name` while absent from `every()` and the schema passed all twenty-three
/// conformance cells, because the two sides were then compared as nine against nine.
///
/// The first repair was a guard reading each listed variant's ordinal, and it was CIRCULAR: it
/// could only inspect variants already in `every()`, so the smuggled one was never looked at and
/// the sabotage passed a second time. A check fed by the list it is checking is green by
/// construction - which the doc on that guard had already stated, two lines above the flaw.
///
/// One list generates all three, so there is nothing left to keep in sync.
///
/// `#[macro_export]` (#381): path-visible outside this crate as `graphhelm_protocols::
/// wire_vocabulary!`, so a closed vocabulary declared in another crate can use the same one-list
/// generator instead of writing a second, hand-listed `wire_name`. No new dependency edge is
/// needed for this: any crate reaching for it either already depends on `graphhelm-protocols` for
/// its wire types, or has no business declaring a wire vocabulary at all.
#[macro_export]
macro_rules! wire_vocabulary {
    ($(#[$meta:meta])* $name:ident { $($(#[$vmeta:meta])* $variant:ident => $wire:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        pub enum $name {
            // serde is the THIRD producer of these spellings, and it takes the same literal as the
            // other two. Left to its default naming it emits the Rust identifier, so a variant can
            // serialise as one spelling while `wire_name` and the schema use another — two
            // serialisers of one closed vocabulary, disagreeing. The equality cells cannot see it:
            // they compare `wire_name` against the schema and never exercise serde.
            $($(#[$vmeta])* #[serde(rename = $wire)] $variant),+
        }

        impl $name {
            /// This variant's wire spelling.
            #[must_use]
            pub const fn wire_name(self) -> &'static str {
                match self {
                    $(Self::$variant => $wire),+
                }
            }

            /// Every variant, generated from the same list as the enum and `wire_name`.
            #[must_use]
            pub const fn every() -> &'static [Self] {
                &[$(Self::$variant),+]
            }
        }
    };
}

wire_vocabulary! {
    /// The nine artifact kinds this task publishes. Closed: an unknown kind is refused, never ignored.
    DevelopmentKind {
        CodeRule => "CodeRule",
        ResolvedCodeContract => "ResolvedCodeContract",
        RetrievalPlan => "RetrievalPlan",
        MemoryCandidate => "MemoryCandidate",
        MemoryRecord => "MemoryRecord",
        AdvisoryDecisionResult => "AdvisoryDecisionResult",
        OwnerTaskResult => "OwnerTaskResult",
        OwnerPresentationPlan => "OwnerPresentationPlan",
        OwnerPresentation => "OwnerPresentation",
    }
}

wire_vocabulary! {
    /// The closed refusal vocabulary. The SCHEMA owns this set; this enum is checked against it.
    /// Codes needed by consuming lanes are allocated HERE, never minted downstream.
    ///
    /// JURISDICTION (ruling on #216, after two lanes answered the line above differently in one
    /// week): a refusal vocabulary belongs to the CONTRACT THAT CARRIES IT. This set owns the
    /// codes that travel in the development envelope, and the rule above is scoped to that wire
    /// — it is not a claim on every refusal in the repository.
    ///
    /// A different bounded domain MAY declare its own closed set, but only with all three of:
    /// (1) its codes never cross into another contract’s envelope — the day one needs to, that
    /// code is allocated in the TARGET vocabulary, not re-spelled locally; (2) it carries the
    /// same enforcement pair as this one, schema as authority plus a set-equality guard between
    /// the Rust type and the schema — a closed set without that pair is closed in prose only;
    /// (3) its declaration site says which contract it belongs to and why it is not this one.
    ///
    /// Condition (1) is the load-bearing one: a code that crosses envelopes is how two
    /// vocabularies drift while both look correct locally.
    DevelopmentRefusalCode {
        UnknownMajorVersion => "unknown_major_version",
        SchemaInvalid => "schema_invalid",
        ScopeMismatch => "scope_mismatch",
        BindingSchemaMismatch => "binding_schema_mismatch",
        BindingProducerMismatch => "binding_producer_mismatch",
        BindingDigestMismatch => "binding_digest_mismatch",
        BindingSnapshotMissing => "binding_snapshot_missing",
        ArtifactTooLarge => "artifact_too_large",
        CardinalityViolation => "cardinality_violation",
        DigestMismatch => "digest_mismatch",
        NegativeClaimUnverified => "negative_claim_unverified",
        IndexStale => "index_stale",
        // Required context could not be assembled within the budget (#222). Deliberately NOT
        // folded into cardinality_violation: nothing is malformed here, the evidence simply does
        // not fit, and the operator response is an expansion request rather than a correction.
        // Folding two causes with opposite responses into one code is the flattening this
        // milestone hunts.
        ContextBudgetInsufficient => "context_budget_insufficient",
        // Allocated by task-002 (#218) under the scope amendment in that issue's body. Appended,
        // never reordered: `wire_name` and the schema are generated from this one list, so a
        // reorder is invisible here and a renumbering downstream.
        //
        // The first two are the pair most at risk of becoming one condition with two names, so
        // the line between them is written here rather than left to each consumer. They differ in
        // what the OPERATOR has to do next, which is the only difference a refusal code is for:
        //
        //   `code_rule_conflict`             the rules cannot both hold. Two requirements on one
        //                                    key where neither strengthens the other under a
        //                                    registered operator -- including the case where no
        //                                    registered operator relates them at all. Remedy: fix
        //                                    the RULES, by reconciling the values or registering
        //                                    an operator for that key.
        //
        //   `code_rule_precedence_unresolved` the rules could both hold, but nothing says which
        //                                    wins. Their selectors are incomparable -- neither
        //                                    contains the other -- so no ordering applies.
        //                                    Remedy: declare PRECEDENCE, by an owner task decision
        //                                    or a declared priority. The rules themselves are fine.
        //
        // They were deliberately not folded. One code for both would send every operator down the
        // wrong path half the time, and a single name would make the two indistinguishable in the
        // record afterwards -- the flattening #247 records, which costs iterations rather than
        // information.
        CodeRuleConflict => "code_rule_conflict",
        CodeRulePrecedenceUnresolved => "code_rule_precedence_unresolved",
        // A declared source could not be read AS A CODE RULE. This is the resolver saying it
        // cannot interpret an input it was handed, and it is distinct from `schema_invalid`: that
        // one belongs to the envelope layer, where an artifact fails its kind's schema BEFORE any
        // typed deserialization. By the time the resolver runs, that validation has happened, so a
        // failure here means the source reached the resolver and still could not be used. A
        // malformed WAIVER is neither of these -- see the next code.
        CodeRuleSourceUnavailable => "code_rule_source_unavailable",
        // The artifact is schema-valid and readable; the OVERRIDE it carries does not meet the
        // rules for an override. Incomplete (a required field absent, or present and null), or
        // asserted against a `structural` rule, where completeness is irrelevant. The boundary
        // with `schema_invalid` is decided here rather than left to each consumer: schema validity
        // is a question about the DOCUMENT, waiver validity is a judgement this resolver makes
        // about a document that is already valid.
        CodeRuleWaiverInvalid => "code_rule_waiver_invalid",
        // #221 (task-005, owner-output validation): allocated here per this macro's own rule
        // ("codes needed by consuming lanes are allocated HERE, never minted downstream"),
        // append-only, scope amended by the orchestrator for this one addition. Fires when a
        // requested compression (a Terse style plan) would hide required consequence, rollback,
        // or evidence-limitation content for a risk-flagged result - design doc §8.4/§9.
        // Publication order, not merge-tool convenience (D's finding: a digest conflict on this
        // file has no "right side", the resolved file needs every lane's entries and the order is
        // whoever landed on main first, then whoever is merging). At this merge, task-002's four
        // codes above (#267) were already on main, so this entry is appended AFTER them.
        UnsafeCompression => "unsafe_compression",
        // A capsule item the result was required to rely on was never cited (#222). Distinct
        // from citation_unresolved below, and the distinction is the operator response: here the
        // evidence EXISTS and the answer does not connect to it, so the remedy is to cite it.
        RequiredCitationMissing => "required_citation_missing",
        // A citation names an item id the capsule does not contain (#222) -- citation spoofing.
        // Item ids are content-derived, so an id nothing hashes to is a typo or a fabrication;
        // the remedy is the OPPOSITE of the one above: stop citing what does not exist. Folding
        // the two into one code would fold two opposite remedies into one instruction.
        CitationUnresolved => "citation_unresolved",
    }
}

/// D-027 scope. `subprojectId` and `executionId` appear only where the owning schema permits them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DevelopmentScope {
    pub workspace_id: WorkspaceId,
    pub project_id: ProjectId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subproject_id: Option<OpaqueId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_id: Option<ExecutionId>,
}

/// A reference to an existing CLOSED artifact, never a copy of it.
///
/// #215's `JourneyContract` and `JourneyVerificationResult`, and the checked-in `ContextCapsule`,
/// are bound through this and are never wrapped or re-declared in a competing format.
///
/// SCOPE OF THE DIGEST, stated because it has a boundary: the digest is canonical, so it is blind
/// to object key order BY DESIGN. Inside this task every binding that is verified points at a file
/// this repository also pins by blob, so byte-identity is covered by that guard and the two
/// mechanisms overlap. The moment a binding is verified against a document from outside the tree,
/// the blob guard does not run and this digest CANNOT distinguish a byte-different document from
/// the pinned one. Whoever adds the first out-of-tree binding inherits that gap.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtifactBinding {
    pub artifact_id: ArtifactId,
    pub schema_id: String,
    pub document_version: SemanticVersion,
    pub schema_version: SemanticVersion,
    pub digest: WireHash,
    pub scope: DevelopmentScope,
    pub producer: OpaqueId,
    pub snapshots: SnapshotBinding,
}

/// The two snapshot identities a binding carries, and the reason there are TWO rather than one.
///
/// `repo_snapshot` is the identity of the BYTES a reader reads. `index_generation` is the identity
/// of what an index was BUILT FROM. They are independent, and **their relation is the freshness
/// verdict**: equal means coordinates resolve safely, different means stale.
///
/// A single opaque snapshot field cannot express that, and a flat list of ids is worse than
/// useless here — two entries with no roles cannot say which is which, so the distinction the
/// verdict rests on is destroyed by the container. That was the first shape of this struct and a
/// downstream consumer refused it before it published.
///
/// Stated by MECHANISM so it survives a different provider: **a byte range produced under
/// generation G may only be resolved against snapshot G.** Whoever resolves a G-coordinate against
/// another snapshot is the defect, whatever produced it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SnapshotBinding {
    /// Identity of the bytes.
    ///
    /// A CONTENT or TREE digest, **never a ref and never a commit sha**. A commit sha identifies
    /// a COMMIT, not a working tree: an uncommitted edit changes the bytes without changing the
    /// identity, so [`SnapshotBinding::is_fresh`] answers fresh while a stored coordinate slices
    /// bytes nobody pinned. The failure runs in the direction that hides — equal identities
    /// over DIFFERENT bytes read as safe, never as stale, so nothing downstream has a reason to
    /// look.
    ///
    /// This paragraph ARMS the requirement and does not enforce it: no comment fails when someone
    /// passes a commit sha. The half that fires is the guard in #219 (G2b/S9), and the pair is
    /// closed only because that guard exists — retire it and this text becomes decoration.
    pub repo_snapshot: OpaqueId,
    /// The repository snapshot this index was built from.
    pub index_generation: OpaqueId,
}

impl SnapshotBinding {
    /// Coordinates from this generation resolve against these bytes.
    ///
    /// Derived rather than stored: a stored `is_fresh` flag is a third fact that can disagree with
    /// the two it summarises.
    #[must_use]
    pub fn is_fresh(&self) -> bool {
        self.repo_snapshot == self.index_generation
    }
}

wire_vocabulary! {
    /// Why a bounded source search produced no evidence, as it travels on the wire.
    ///
    /// Deliberately two variants and not one: "there is no channel here" and "the channel refused
    /// to exceed its declared ceiling" send an operator in opposite directions -- wire one up,
    /// versus raise the bound or narrow the query. Folding them is the flattening #247 records.
    ///
    /// **Declared HERE and not in `core/runtime/src/ports.rs`, where the port lives (#724, Codex
    /// P1 on #745).** The jurisdiction rule above is the reason: this value travels in the
    /// development envelope, so it belongs to the contract that carries it, and that contract's
    /// enforcement pair -- schema as authority plus a set-equality guard between the Rust type
    /// and the schema -- can only be met where the schema is. `core/runtime` re-exports it, so
    /// every caller keeps the name it already used.
    ///
    /// **This is NOT `fallbackOutcome`, and the difference has two owners.**
    /// `retrieval-coverage-receipt.schema.json` `$defs/fallbackOutcome` is
    /// `["not_required", "unavailable"]`: the RECEIPT's record of a fallback, owned by the
    /// receipt boundary (#655), and D-045 states it has no success state. This set is the
    /// CHANNEL's typed failure at the COMPILE layer, returned beside an outcome that still
    /// stands -- the channel is evidence, not authority -- which is why `bound_exceeded` exists
    /// here and has no counterpart there.
    ///
    /// The two overlap on `unavailable`, and that overlap was reached by luck rather than by
    /// derivation: the spelling was chosen from neighbouring snake_case by an author who did not
    /// know `fallbackOutcome` existed. `the_two_fallback_vocabularies_overlap_on_exactly_one`
    /// makes the agreement a checked property. It asserts the INTERSECTION and not equality on
    /// purpose: #655 owns `fallbackOutcome` and may add a member to it legitimately, and a guard
    /// that fired on that would be firing on correct divergence.
    SourceSearchError {
        /// No source channel is wired for this workspace.
        Unavailable => "unavailable",
        /// The search would have exceeded a declared bound. NOT a partial answer.
        BoundExceeded => "bound_exceeded",
    }
}

wire_vocabulary! {
    /// What a search actually covered, as a CLOSED set: each state has a DIFFERENT correct response
    /// to a zero result, so a bool or an error channel would destroy the distinction the fallback
    /// decision rests on.
    CoverageState {
        Complete => "complete",
        Partial => "partial",
        Excluded => "excluded",
        Skipped => "skipped",
        ExtractionGap => "extraction_gap",
        Stale => "stale",
        Unknown => "unknown",
        Unresolved => "unresolved",
    }
}

/// Explicit bounds a producer declares, so "bounded" is a number a reader can check rather than a
/// promise in prose.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeclaredLimits {
    pub max_results: u32,
    pub max_pages: u32,
    pub max_bytes: u64,
    pub max_tokens: u32,
}

/// The first closed retrieval-evidence sidecar. It is deliberately not a `DevelopmentKind`:
/// `RetrievalPlan` is immutable pre-execution intent, while this receipt records what one provider
/// attempt actually covered. See D-045.
pub const RETRIEVAL_COVERAGE_RECEIPT_API_VERSION: &str = "p50.dev/retrieval-coverage-receipt/v1";
pub const RETRIEVAL_COVERAGE_RECEIPT_KIND: &str = "RetrievalCoverageReceipt";

wire_vocabulary! {
    /// Strength the provider can support for its coverage report. `best_effort` is evidence, never
    /// permission to promote a zero to verified absence.
    ProviderCoverageConfidence {
        Verified => "verified",
        BestEffort => "best_effort",
    }
}

wire_vocabulary! {
    /// What a coverage entry addresses. Paths and bounded negative scopes are intentionally
    /// separate because proving every cited file is not proof that a directory has no other hit.
    RetrievalCoverageTarget {
        Path => "path",
        NegativeScope => "negative_scope",
    }
}

wire_vocabulary! {
    /// The only fallback classes this first receipt can name.
    RetrievalFallbackKind {
        Source => "source",
        Reindex => "reindex",
    }
}

wire_vocabulary! {
    /// A Runtime-owned fallback result. There is no `succeeded` member in this slice: source
    /// fallback and reindexing are not implemented, so accepting that spelling would mint proof
    /// for work no producer can perform.
    RetrievalFallbackOutcome {
        NotRequired => "not_required",
        Unavailable => "unavailable",
    }
}

/// The exact step and query inside one immutable `RetrievalPlan`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RetrievalStepBinding {
    pub step_id: OpaqueId,
    pub step_digest: WireHash,
    pub query_digest: WireHash,
}

/// Brand-neutral provider and capability identity. The concrete adapter name is evidence, never
/// authority to bypass the Tool Broker.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RetrievalProviderBinding {
    pub provider_id: OpaqueId,
    pub capability_id: OpaqueId,
    pub capability_version: SemanticVersion,
    pub tool: String,
    pub action: String,
}

/// One inclusive source gap. Lines are one-based and validated by Runtime before publication.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RetrievalGapRange {
    pub start: u64,
    pub end: u64,
}

/// Coverage for one exact path or one bounded negative scope.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RetrievalCoverageEntry {
    pub target: RetrievalCoverageTarget,
    pub value: String,
    pub coverage: CoverageState,
    pub gap_ranges: Vec<RetrievalGapRange>,
}

/// Runtime-observed evidence for one page. `position` is opaque and exists to prove progress and
/// detect loops; it never becomes a provider command.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RetrievalPageEvidence {
    pub position: String,
    pub results: u32,
    pub bytes: u64,
    pub has_more: bool,
}

/// What Runtime actually received, computed from pages and hits rather than copied from a provider
/// self-report.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RetrievalReceivedLimits {
    pub results: u32,
    pub pages: u32,
    pub bytes: u64,
    pub tokens: u32,
}

/// Digest binding to the exact durable Tool Broker record. Core protocols cannot depend on the
/// broker crate, so the Runtime computes this from the complete record and carries the fields an
/// operator needs to join it without duplicating the broker's disposition vocabulary here.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RetrievalBrokerRecordBinding {
    pub tool: String,
    pub action: String,
    pub actor: String,
    pub record_digest: WireHash,
    pub stdout_digest: WireHash,
    pub stdout_bytes: u64,
    pub stderr_digest: WireHash,
    pub stderr_bytes: u64,
    pub reused: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RetrievalFallbackReceipt {
    pub kind: RetrievalFallbackKind,
    pub outcome: RetrievalFallbackOutcome,
}

/// Everything covered by the receipt digest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RetrievalCoverageReceiptBody {
    pub plan_binding: ArtifactBinding,
    pub step: RetrievalStepBinding,
    pub scope: DevelopmentScope,
    pub snapshots: SnapshotBinding,
    pub provider: RetrievalProviderBinding,
    pub broker_record: RetrievalBrokerRecordBinding,
    pub confidence: ProviderCoverageConfidence,
    pub coverage: CoverageState,
    pub requested_paths: Vec<String>,
    pub negative_scopes: Vec<String>,
    pub entries: Vec<RetrievalCoverageEntry>,
    pub pages: Vec<RetrievalPageEvidence>,
    pub declared_limits: DeclaredLimits,
    pub received_limits: RetrievalReceivedLimits,
    pub total_results: u32,
    pub hits: Vec<String>,
    pub fallbacks: Vec<RetrievalFallbackReceipt>,
}

/// Immutable, canonical retrieval evidence. Runtime validates every binding before constructing
/// this value and verifies them again when rehydrating bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RetrievalCoverageReceiptV1 {
    pub api_version: String,
    pub kind: String,
    pub schema_version: SemanticVersion,
    pub body: RetrievalCoverageReceiptBody,
    pub digest: WireHash,
}

/// The common envelope every new artifact carries.
///
/// **NOT `deny_unknown_fields`, and that is the criterion rather than an oversight.** #217 requires
/// that *"compatible minor-version unknown fields are preserved but never interpreted as
/// authority"*, and those are TWO properties that a single setting cannot deliver:
///
/// - **Preserved** needs capture. Merely permitting unknown fields makes serde DROP them, which is
///   worse than refusing: a consumer reads and rewrites an artifact and the producer's field is
///   silently gone. `additional` captures them so a round-trip through an older reader is lossless.
/// - **Never authority** needs enumeration. Unknown fields are excluded from the digest and from
///   every verification path, so they cannot change identity or any decision.
///
/// An unknown **major** still fails closed — that is `apiVersion`, a different question from an
/// unknown field, and the issue states the two in consecutive lines.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DevelopmentEnvelope {
    pub api_version: String,
    pub kind: DevelopmentKind,
    pub metadata: DevelopmentMetadata,
    pub producer: OpaqueId,
    pub producer_version: SemanticVersion,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bindings: Vec<ArtifactBinding>,
    /// Per-kind payload, validated against the kind's schema before typed deserialization.
    ///
    /// Held as an untyped value on purpose: unknown fields INSIDE a compatible minor's spec then
    /// round-trip for free, without a capture map at every nesting level.
    pub spec: serde_json::Value,
    pub digest: WireHash,
    /// Unknown top-level fields from a compatible minor, preserved and never authoritative.
    #[serde(flatten)]
    pub additional: serde_json::Map<String, serde_json::Value>,
}

impl DevelopmentEnvelope {
    /// The semantic fields, in canonical form — the digest input.
    ///
    /// `additional` is deliberately absent: that is the mechanism by which a preserved unknown
    /// field is denied authority. If unknown fields entered the digest, a minor-compatible producer
    /// could change an artifact's identity by adding a field an older reader cannot even name.
    #[must_use]
    pub fn digest_input(&self) -> String {
        let semantic = serde_json::json!({
            "apiVersion": self.api_version,
            "kind": self.kind.wire_name(),
            "metadata": serde_json::to_value(&self.metadata).unwrap_or(serde_json::Value::Null),
            "producer": self.producer.as_str(),
            "producerVersion": self.producer_version.as_str(),
            "bindings": serde_json::to_value(&self.bindings).unwrap_or(serde_json::Value::Null),
            "spec": self.spec,
        });
        canonical_json(&semantic)
    }
}

/// Envelope metadata: identity, artifact version, and D-027 scope.
///
/// Open for the same reason as the envelope: a compatible minor may add a metadata field, and
/// refusing it would turn a version question into "invalid artifact".
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DevelopmentMetadata {
    pub id: ArtifactId,
    pub artifact_version: SemanticVersion,
    pub scope: DevelopmentScope,
}

/// The MAJOR segment of an `apiVersion`, or `None` when the string is not this group's.
///
/// Returning `None` rather than a default is the fail-closed half: a caller cannot accidentally
/// treat an unparseable version as major 1.
#[must_use]
pub fn development_api_version_major(api_version: &str) -> Option<u16> {
    let suffix = api_version
        .strip_prefix(DEVELOPMENT_API_GROUP)?
        .strip_prefix('/')?
        .strip_prefix('v')?;
    let digits: String = suffix.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// Verify a candidate binding against the reference it claims to bind.
///
/// FIVE checks, FIVE distinct refusal codes. One code for all of them would let a guard assert
/// "the binding was rejected" and stay green with four of the five checks deleted; the codes are
/// what make five separate cells possible.
///
/// Order is fixed and documented because it is observable: a candidate wrong in two ways reports
/// the first check that fails, so a caller comparing codes is comparing against this order.
pub fn verify_binding(
    candidate: &ArtifactBinding,
    reference: &ArtifactBinding,
) -> Result<(), DevelopmentRefusalCode> {
    if candidate.scope != reference.scope {
        return Err(DevelopmentRefusalCode::ScopeMismatch);
    }
    if candidate.schema_id != reference.schema_id
        || candidate.schema_version != reference.schema_version
    {
        return Err(DevelopmentRefusalCode::BindingSchemaMismatch);
    }
    if candidate.producer != reference.producer {
        return Err(DevelopmentRefusalCode::BindingProducerMismatch);
    }
    if candidate.digest != reference.digest {
        return Err(DevelopmentRefusalCode::BindingDigestMismatch);
    }
    if candidate.snapshots != reference.snapshots {
        return Err(DevelopmentRefusalCode::BindingSnapshotMissing);
    }
    Ok(())
}

/// Canonical JSON: object keys sorted, no insignificant whitespace.
///
/// THE SORT IS EXPLICIT AND THAT IS DELIBERATE, even though it is redundant today. This workspace
/// builds `serde_json` without `preserve_order`, so its object map is a `BTreeMap` and parsing
/// already sorts — measured, not inferred: two documents differing only in textual key order
/// serialise identically before this function is reached.
///
/// So key-order determinism is currently supplied by the dependency, NOT by this code, and no
/// mutation of this function can be observed to break it. Sorting here anyway is what makes the
/// property survive someone enabling `preserve_order` later, which would otherwise change
/// canonical output silently and invalidate every digest already published.
#[must_use]
pub fn canonical_json(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Object(map) => {
            let mut entries: Vec<_> = map.iter().collect();
            entries.sort_by(|left, right| left.0.cmp(right.0));
            let body: Vec<String> = entries
                .into_iter()
                .map(|(key, nested)| {
                    format!(
                        "{}:{}",
                        serde_json::Value::String(key.clone()),
                        canonical_json(nested)
                    )
                })
                .collect();
            format!("{{{}}}", body.join(","))
        }
        serde_json::Value::Array(items) => {
            let body: Vec<String> = items.iter().map(canonical_json).collect();
            format!("[{}]", body.join(","))
        }
        other => other.to_string(),
    }
}

/// Normalise a path-like value to forward slashes before it enters a digest.
///
/// The envelope declares no path field today, so nothing in this module calls it yet. It exists
/// because the kinds that carry source coordinates will, and because a digest taken over a
/// Windows-shaped path and one taken over its POSIX twin must not differ - a producer's platform
/// is not part of the artifact's identity.
#[must_use]
pub fn normalise_path_separators(value: &str) -> String {
    const WINDOWS_SEPARATOR: char = '\\';
    value.replace(WINDOWS_SEPARATOR, "/")
}

#[cfg(test)]
mod receipt_schema_tests {
    use super::*;

    #[test]
    fn receipt_schema_accepts_the_valid_fixture_and_rejects_best_effort_complete() {
        let package = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../extensions/builtin/graphhelm-development-contracts");
        let read_json = |relative: &str| {
            let path = package.join(relative);
            let bytes = std::fs::read(&path)
                .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
            serde_json::from_slice::<serde_json::Value>(&bytes)
                .unwrap_or_else(|error| panic!("invalid JSON in {}: {error}", path.display()))
        };
        let schema = read_json("schemas/retrieval-coverage-receipt.schema.json");
        let validator = jsonschema::validator_for(&schema).expect("receipt schema compiles");
        let valid = read_json("fixtures/retrieval/coverage-receipt-valid.json");
        let promoted =
            read_json("fixtures/retrieval/coverage-receipt-invalid-best-effort-complete.json");

        assert!(
            validator.is_valid(&valid),
            "the valid receipt fixture must conform"
        );
        assert!(
            !validator.is_valid(&promoted),
            "best_effort plus complete must be structurally unrepresentable on the wire"
        );

        let mut newer_bound_contract = valid.clone();
        newer_bound_contract["body"]["planBinding"]["documentVersion"] =
            serde_json::Value::String("2.1.0".to_owned());
        newer_bound_contract["body"]["planBinding"]["schemaVersion"] =
            serde_json::Value::String("2.0.0".to_owned());
        newer_bound_contract["body"]["provider"]["capabilityVersion"] =
            serde_json::Value::String("3.4.5".to_owned());
        assert!(
            validator.is_valid(&newer_bound_contract),
            "receipt v1 binds exact external versions; it does not require them to also be v1"
        );

        let schema_confidence = schema["$defs"]["confidence"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<std::collections::BTreeSet<_>>();
        let rust_confidence = ProviderCoverageConfidence::every()
            .iter()
            .map(|value| value.wire_name())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(schema_confidence, rust_confidence);

        let schema_coverage = schema["$defs"]["coverage"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<std::collections::BTreeSet<_>>();
        let rust_coverage = CoverageState::every()
            .iter()
            .map(|value| value.wire_name())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(schema_coverage, rust_coverage);

        let schema_targets = schema["$defs"]["coverageEntry"]["properties"]["target"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<std::collections::BTreeSet<_>>();
        let rust_targets = RetrievalCoverageTarget::every()
            .iter()
            .map(|value| value.wire_name())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(schema_targets, rust_targets);

        let schema_fallbacks = schema["$defs"]["fallbackOutcome"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<std::collections::BTreeSet<_>>();
        let rust_fallbacks = RetrievalFallbackOutcome::every()
            .iter()
            .map(|value| value.wire_name())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(schema_fallbacks, rust_fallbacks);
    }
}

#[cfg(test)]
mod source_search_error_tests {
    use super::SourceSearchError;

    /// The two variants, their two spellings, and nothing else in the set.
    ///
    /// `every()` is asserted alongside the spellings on purpose. A cell that only checked
    /// `wire_name()` variant by variant would stay green if a THIRD variant were added without a
    /// spelling decision -- it would simply never be asked. Pinning the set makes adding a
    /// variant a change that must be made here too, which is what "closed vocabulary" means.
    ///
    /// This pins the macro's output against itself and is NOT the contract guard: `wire_name()`
    /// and `every()` are generated from one literal list, so comparing them compares that list
    /// with itself. The schema is the authority, and
    /// `apps/cli/tests/development_contract_schemas.rs` holds the guard that anchors this set to
    /// it. Both are needed: this one names the intended spellings, that one proves the schema
    /// agrees.
    #[test]
    fn the_two_channel_failures_have_their_two_spellings() {
        assert_eq!(SourceSearchError::Unavailable.wire_name(), "unavailable");
        assert_eq!(
            SourceSearchError::BoundExceeded.wire_name(),
            "bound_exceeded"
        );
        assert_eq!(
            SourceSearchError::every(),
            &[
                SourceSearchError::Unavailable,
                SourceSearchError::BoundExceeded
            ],
        );
    }

    /// serde is the THIRD producer of these spellings, and the one the other cells cannot see.
    ///
    /// serde is what actually travels. Left to its default naming it would emit the Rust
    /// identifier (`"Unavailable"`, `"BoundExceeded"`) while `wire_name()` said something else
    /// -- two serialisers of one closed vocabulary, disagreeing, with every other cell green.
    #[test]
    fn serde_emits_the_same_spelling_wire_name_does() {
        for failure in SourceSearchError::every() {
            let json = serde_json::to_string(failure).expect("a fieldless enum serialises");
            assert_eq!(
                json,
                format!("\"{}\"", failure.wire_name()),
                "serde and wire_name disagree for {failure:?}",
            );
        }
    }
}
