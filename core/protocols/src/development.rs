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
macro_rules! wire_vocabulary {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $wire:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        pub enum $name {
            // serde is the THIRD producer of these spellings, and it takes the same literal as the
            // other two. Left to its default naming it emits the Rust identifier, so a variant can
            // serialise as one spelling while `wire_name` and the schema use another — two
            // serialisers of one closed vocabulary, disagreeing. The equality cells cannot see it:
            // they compare `wire_name` against the schema and never exercise serde.
            $(#[serde(rename = $wire)] $variant),+
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
