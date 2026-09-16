//! Conformance for the development artifact contracts (#217).
//!
//! The schema is the authoritative side of every closed vocabulary here. These guards compare it
//! against the Rust wire types by SET EQUALITY over two DERIVED sets — never by count, because a
//! rename preserves the count, and never against a list typed out inside the test, because a
//! hand-maintained list rots exactly like the hand-maintained count it replaced. That is measured
//! in this repository, not assumed: a coverage check kept its expected set by hand and could not
//! name two variants that were missing from both sides of its own comparison.

use std::collections::BTreeSet;
use std::path::PathBuf;

use graphhelm_protocols::{
    ArtifactBinding, ArtifactId, CoverageState, DEVELOPMENT_API_MAJOR, DevelopmentEnvelope,
    DevelopmentKind, DevelopmentRefusalCode, DevelopmentScope, OpaqueId, ProjectId,
    SemanticVersion, SnapshotBinding, SourceSearchError, WireHash, WorkspaceId, canonical_json,
    development_api_version_major, normalise_path_separators, verify_binding,
};

fn extension_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../extensions/builtin/graphhelm-development-contracts")
}

fn envelope_schema() -> serde_json::Value {
    let path = extension_dir().join("schemas/development-envelope.schema.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("envelope schema unreadable at {}: {error}", path.display())
    });
    serde_json::from_str(&text).expect("the envelope schema is JSON")
}

/// Read one closed vocabulary out of the schema. Reading it rather than transcribing it is the
/// point: a transcribed set is a second hand-maintained list and inherits the defect.
fn schema_enum(schema: &serde_json::Value, def: &str) -> BTreeSet<String> {
    schema["$defs"][def]["enum"]
        .as_array()
        .unwrap_or_else(|| panic!("$defs/{def}/enum is missing or not an array"))
        .iter()
        .map(|value| {
            value
                .as_str()
                .unwrap_or_else(|| panic!("$defs/{def}/enum holds a non-string"))
                .to_owned()
        })
        .collect()
}

/// D1 — the refusal vocabulary agrees between the schema (authoritative) and the Rust type.
///
/// The mutation this exists to catch is a RENAME on one side only. A count comparison cannot see
/// it, which is why this asserts set equality and prints the two differences rather than a length.
#[test]
fn the_refusal_vocabulary_is_one_set_on_both_sides() {
    let from_schema = schema_enum(&envelope_schema(), "refusalCode");
    let from_type: BTreeSet<String> = DevelopmentRefusalCode::every()
        .iter()
        .map(|code| code.wire_name().to_owned())
        .collect();

    let only_in_schema: Vec<_> = from_schema.difference(&from_type).collect();
    let only_in_type: Vec<_> = from_type.difference(&from_schema).collect();
    assert!(
        only_in_schema.is_empty() && only_in_type.is_empty(),
        "the refusal vocabulary diverged. only in schema: {only_in_schema:?}; only in the Rust \
         type: {only_in_type:?}. The schema is authoritative; the type follows it."
    );
}

/// The vocabulary is APPEND-ONLY, which is a claim about order that set equality cannot make.
///
/// The rule was stated in a comment at the declaration site and in a PR description — the site
/// that ARMS a property, not one that fires when it breaks. A reorder keeps both sides equal as
/// sets, so the guard above stays green through it, while every consumer that reads the list
/// positionally silently renumbers.
///
/// This is the cheap version of the check, and it is available only because the ordered list
/// already exists on both sides: `every()` is generated from the same list as `wire_name`, and the
/// schema enum is a JSON array.
#[test]
fn the_refusal_vocabulary_is_in_the_same_order_on_both_sides() {
    let from_schema: Vec<String> = envelope_schema()["$defs"]["refusalCode"]["enum"]
        .as_array()
        .expect("$defs/refusalCode/enum is an array")
        .iter()
        .map(|value| value.as_str().expect("a string").to_owned())
        .collect();
    let from_type: Vec<String> = DevelopmentRefusalCode::every()
        .iter()
        .map(|code| code.wire_name().to_owned())
        .collect();

    assert_eq!(
        from_schema, from_type,
        "the refusal vocabulary is in a different ORDER on the two sides. Appending is the only \
         permitted change; a reorder leaves the two equal as SETS and is therefore invisible to \
         the cell above."
    );
}

/// The refusal vocabulary in the order PUBLISHED at `ac6e746`, which is the anchor the pair
/// cannot move.
///
/// The cell above compares the schema to the type and is right to. But **both of its sides are
/// read out of this working tree**, so a reorder applied to BOTH in one edit — an alphabetiser, a
/// formatter, a merge resolved by sorting — leaves them equal and that cell green while every
/// consumer that reads the list positionally silently renumbers.
///
/// The property here is not "the two sides agree". It is **append-only**, which is a claim about
/// HISTORY, and history is not visible to either side of a comparison drawn entirely from the
/// working tree. This list is that history, written down once so a reorder has to argue with
/// something that did not move.
///
/// **LIMIT, stated so it is not discovered later:** a frozen prefix protects only what is in it.
/// Codes appended after this list are unordered relative to each other until someone promotes them
/// into it. That is a real gap and a smaller one than it closes — but it makes this guard a HABIT
/// rather than self-maintaining: extend this list when a lane's codes land. The alternative,
/// deriving the published order from git history inside the test, is stronger and considerably
/// more expensive.
const PUBLISHED_ORDER: [&str; 20] = [
    "unknown_major_version",
    "schema_invalid",
    "scope_mismatch",
    "binding_schema_mismatch",
    "binding_producer_mismatch",
    "binding_digest_mismatch",
    "binding_snapshot_missing",
    "artifact_too_large",
    "cardinality_violation",
    "digest_mismatch",
    "negative_claim_unverified",
    "index_stale",
    "context_budget_insufficient",
    "code_rule_conflict",
    "code_rule_precedence_unresolved",
    "code_rule_source_unavailable",
    "code_rule_waiver_invalid",
    "unsafe_compression",
    "required_citation_missing",
    "citation_unresolved",
];

/// A reorder must argue with something outside the pair.
///
/// Appending a code needs no edit here: the new code lands after this prefix and the assertion
/// still holds. Changing the order of anything already published is what forces an edit to this
/// list — which turns a reorder from a diff nobody looks at twice into a deliberate, reviewable
/// act.
#[test]
fn the_published_refusal_order_is_append_only() {
    let from_schema: Vec<String> = envelope_schema()["$defs"]["refusalCode"]["enum"]
        .as_array()
        .expect("$defs/refusalCode/enum is an array")
        .iter()
        .map(|value| value.as_str().expect("a string").to_owned())
        .collect();

    // NON-EMPTY FIRST. A schema slice that resolved to nothing would make the comparison below
    // pass against an empty head, which is the vacuous form of this whole check.
    assert!(
        !from_schema.is_empty(),
        "HARNESS-BROKE: the refusalCode enum read as empty, so the prefix comparison below would \
         compare nothing against nothing"
    );

    let published: Vec<&str> = from_schema
        .iter()
        .take(PUBLISHED_ORDER.len())
        .map(String::as_str)
        .collect();

    assert_eq!(
        published.as_slice(),
        PUBLISHED_ORDER.as_slice(),
        "the PUBLISHED prefix of the refusal vocabulary changed. Appending is the only permitted \
         change and needs no edit here; a reorder of anything already published renumbers every \
         consumer that reads this list positionally. If the change is deliberate, editing \
         PUBLISHED_ORDER is how it becomes visible."
    );
}

/// The same discipline for the nine kinds.
#[test]
fn the_kind_vocabulary_is_one_set_on_both_sides() {
    let from_schema = schema_enum(&envelope_schema(), "developmentKind");
    let from_type: BTreeSet<String> = DevelopmentKind::every()
        .iter()
        .map(|kind| kind.wire_name().to_owned())
        .collect();

    let only_in_schema: Vec<_> = from_schema.difference(&from_type).collect();
    let only_in_type: Vec<_> = from_type.difference(&from_schema).collect();
    assert!(
        only_in_schema.is_empty() && only_in_type.is_empty(),
        "the kind vocabulary diverged. only in schema: {only_in_schema:?}; only in the Rust type: \
         {only_in_type:?}"
    );
    assert_eq!(from_schema.len(), 9, "the design publishes nine kinds");
}

/// An unknown MAJOR fails closed, and an unparseable version is not silently treated as major 1.
#[test]
fn an_unknown_major_is_refused_and_an_unparseable_version_is_not_defaulted() {
    assert_eq!(
        development_api_version_major("p50.dev/development/v1alpha1"),
        Some(DEVELOPMENT_API_MAJOR),
        "the current major parses"
    );
    assert_eq!(
        development_api_version_major("p50.dev/development/v2alpha1"),
        Some(2),
        "a future major parses as itself rather than being coerced"
    );
    // The fail-closed half: none of these may come back as the current major.
    for hostile in [
        "p50.dev/development/valpha",
        "p50.dev/other/v1alpha1",
        "v1alpha1",
        "",
    ] {
        assert_ne!(
            development_api_version_major(hostile),
            Some(DEVELOPMENT_API_MAJOR),
            "{hostile:?} must not be readable as the current major"
        );
    }
}

/// The envelope schema declares every property the Rust envelope carries, with none extra.
///
/// This is the cell that is red first: it fails until the schema and the type agree on the
/// envelope's own shape, and it cannot be satisfied by the vocabulary guards above.
#[test]
fn the_envelope_shape_matches_the_schema() {
    let schema = envelope_schema();
    let declared: BTreeSet<String> = schema["properties"]
        .as_object()
        .expect("the envelope schema declares properties")
        .keys()
        .cloned()
        .collect();
    let expected: BTreeSet<String> = [
        "apiVersion",
        "kind",
        "metadata",
        "producer",
        "producerVersion",
        "bindings",
        "spec",
        "digest",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    assert_eq!(
        declared, expected,
        "the envelope's declared properties drifted from the wire type"
    );
    assert_eq!(
        schema["additionalProperties"],
        serde_json::Value::Bool(true),
        "the envelope is OPEN, because #217 requires compatible-minor unknown fields to be \
         PRESERVED. Closing it would turn a version question into an invalid-artifact refusal, \
         and a newer producer could not speak to an older validator at all."
    );
}

/// `ArtifactBinding` requires EVERY verified property, so a binding cannot be partly checked.
///
/// A binding that permits any property to be absent can be satisfied while something it was
/// supposed to pin is missing — the point is that no subset is admissible, which is a claim about
/// ALL of them rather than about how many there are.
///
/// **Required for two different reasons, and conflating them is what produced the wrong count.**
/// Measured against `verify_binding`:
///
/// * **six are COMPARED**: `scope`, `schemaId`, `schemaVersion`, `producer`, `digest`,
///   `snapshots`. These are what a check reads to decide whether a candidate matches a reference.
/// * **two are IDENTITY and compared by nothing**: `artifactId` and `documentVersion`. They say
///   WHICH artifact, at which version, the binding is about. A binding without them pins nothing
///   — but no comparison reads them, so no refusal code can name them.
///
/// An earlier version of this note said the eight collapse to five checks "because some checks
/// read more than one field". That explains six to five; it does not explain eight to six, and it
/// quietly assumed every required property exists because a check needs it. For a quarter of this
/// binding that is false. (Found by L, on the correction to the correction.)
///
/// **The count that used to be in this sentence said "five" while the binding had eight (#272).**
/// It is not corrected to "eight": a hand-maintained integer in unguarded prose is the defect,
/// not the particular integer, and the sentence never needed one. The same reasoning removed the
/// hand-listed field array below.
///
/// **The properties are DERIVED from the schema, not transcribed.** The previous form iterated a
/// literal array of eight names and asserted each was required. That is count-robust, which is
/// why it survived review, but it is a second hand-maintained copy of one vocabulary and it only
/// checks one direction: a NINTH property added to the binding and left optional satisfies every
/// one of its assertions, because none of them is about the ninth. Comparing the two sets makes
/// the assertion say what the doc above says.
#[test]
fn a_binding_cannot_be_partially_specified() {
    let schema = envelope_schema();
    let binding = &schema["$defs"]["artifactBinding"];

    let required = required_set(binding);
    let properties: BTreeSet<String> = binding["properties"]
        .as_object()
        .expect("artifactBinding declares properties")
        .keys()
        .cloned()
        .collect();

    // The floor is the binding's REAL width, not a number chosen to sit comfortably under it.
    // `> 3` was the first version and it tolerated FIVE silent removals: the set comparison below
    // stays green while both sides shrink together, so a floor with slack is exactly the check
    // that cannot see the shrinkage it exists to catch.
    //
    // Its cost is that a legitimate removal now edits this number, which is the plausible-looking
    // edit a floor is supposed to resist. So the rule beside it: **change this only in the same
    // commit as the property that caused it, and name that property here.**
    assert!(
        properties.len() >= 8,
        "HARNESS-BROKE: artifactBinding declares {} properties and this guard was written against \
         8. If a property was legitimately removed, lower this in the same commit and name it; \
         otherwise the set comparison below is reading a binding that has silently narrowed",
        properties.len()
    );
    assert_eq!(
        required,
        properties,
        "a binding may be satisfied while some verified property is absent. Optional here: \
         {:?}; required but not a declared property: {:?}",
        properties.difference(&required).collect::<Vec<_>>(),
        required.difference(&properties).collect::<Vec<_>>()
    );
}

fn required_set(definition: &serde_json::Value) -> BTreeSet<String> {
    definition["required"]
        .as_array()
        .expect("the definition declares required properties")
        .iter()
        .map(|value| value.as_str().expect("required holds strings").to_owned())
        .collect()
}

/// The derived comparison catches what the hand-listed one could not.
///
/// Without this, replacing the array with a set comparison is a refactor nobody has shown to add
/// anything — and "the new test passes" says nothing, because the old one passed too. So the
/// defect is constructed and both rules are run over it: a binding carrying a property that is
/// NOT required.
///
/// The old form is reproduced here rather than described, because a description of a deleted rule
/// is a claim nothing checks. It is run over the same counterexample and shown to accept it.
#[test]
fn the_derived_comparison_rejects_an_optional_property_the_old_field_list_accepted() {
    let sabotaged = serde_json::json!({
        "properties": {
            "artifactId": {"type": "string"},
            "schemaId": {"type": "string"},
            "documentVersion": {"type": "string"},
            "schemaVersion": {"type": "string"},
            "digest": {"type": "string"},
            "scope": {"type": "object"},
            "producer": {"type": "string"},
            "snapshots": {"type": "array"},
            // The ninth: declared, and quietly optional.
            "reviewedBy": {"type": "string"},
        },
        "required": [
            "artifactId", "schemaId", "documentVersion", "schemaVersion",
            "digest", "scope", "producer", "snapshots"
        ],
    });

    let required = required_set(&sabotaged);
    let properties: BTreeSet<String> = sabotaged["properties"]
        .as_object()
        .expect("the counterexample declares properties")
        .keys()
        .cloned()
        .collect();

    // THE OLD RULE, verbatim in behaviour: every hand-listed name is required.
    let old_rule_accepts = [
        "artifactId",
        "schemaId",
        "documentVersion",
        "schemaVersion",
        "digest",
        "scope",
        "producer",
        "snapshots",
    ]
    .iter()
    .all(|field| required.contains(*field));
    assert!(
        old_rule_accepts,
        "HARNESS-BROKE: the counterexample was supposed to satisfy the OLD rule. If it does not, \
         it proves nothing about what the new rule adds"
    );

    // THE NEW RULE rejects it.
    assert_ne!(
        required, properties,
        "the derived comparison accepted a binding with an optional property, so it adds nothing \
         over the field list it replaced"
    );
}

/// The coverage vocabulary agrees on both sides, and is CLOSED.
///
/// Requested by the downstream consumer before this shape published, with the reason that decides
/// it: each state has a different correct response to a zero result, so a boolean or an error
/// channel would destroy the distinction the fallback decision rests on.
#[test]
fn the_coverage_vocabulary_is_one_set_on_both_sides() {
    let from_schema = schema_enum(&envelope_schema(), "coverageState");
    let from_type: BTreeSet<String> = CoverageState::every()
        .iter()
        .map(|state| state.wire_name().to_owned())
        .collect();
    assert_eq!(
        from_schema, from_type,
        "the coverage vocabulary diverged between schema and type"
    );
    assert_eq!(
        from_schema.len(),
        8,
        "eight states, because eight different correct answers to a zero"
    );
}

/// The SECOND copy of that vocabulary is pinned to the same third thing (#630).
///
/// The eight tokens exist twice: `development-envelope.schema.json` `$defs/coverageState`, pinned by
/// the cell above, and `retrieval-coverage-receipt.schema.json` `$defs/coverage`, which is the one
/// actually `$ref`-ed and required on the wire. Only the first was pinned, and two copies of one
/// closed set that are pinned differently drift in exactly one direction: the live copy moves and
/// nothing says so.
///
/// **The proposal this replaces was to delete the envelope's copy as dead.** It has zero `$ref`s --
/// measured, with `#/$defs/opaqueId` at 241 as the control that proves the instrument reads -- but
/// **a `$ref` count is not a consumer count**: the cell above consumes it from Rust, and deleting it
/// would have removed the only schema-to-type pin while keeping the copy that had none. Pinning both
/// to `CoverageState::every()` collapses the drift risk without deleting a guard, and needs no
/// cross-file `$ref` fighting the envelope's deliberately opaque `spec`.
#[test]
fn the_receipt_copy_of_the_coverage_vocabulary_is_pinned_to_the_same_type() {
    let path = extension_dir().join("schemas/retrieval-coverage-receipt.schema.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("receipt schema unreadable at {}: {error}", path.display()));
    let receipt: serde_json::Value =
        serde_json::from_str(&text).expect("the receipt schema is JSON");

    let from_receipt = schema_enum(&receipt, "coverage");
    let from_type: BTreeSet<String> = CoverageState::every()
        .iter()
        .map(|state| state.wire_name().to_owned())
        .collect();
    assert_eq!(
        from_receipt, from_type,
        "the receipt schema's coverage vocabulary diverged from `CoverageState`. This is the copy \
         the wire actually uses, so a drift here is a live wire contract that no longer matches the \
         type it was written from"
    );

    // The envelope's copy is pinned to the same set one cell above. Asserting the two schema copies
    // agree WITH EACH OTHER here would be a third comparison that adds nothing: both are already
    // pinned to `CoverageState::every()`, and two things equal to a third are equal. What this adds
    // is the second edge, so neither copy can move alone.
    assert_eq!(
        from_receipt,
        schema_enum(&envelope_schema(), "coverageState"),
        "the two schema copies disagree, which cannot happen while both match the type -- suspect \
         this harness before the schemas"
    );
}

/// A snapshot binding carries TWO identities, and neither may be omitted.
///
/// This is the cell that a single opaque `snapshot` field would make inexpressible. The freshness
/// verdict is the RELATION between the identity of the bytes and the identity of what the index was
/// built from; with one field there is no relation to state, and the downstream acceptance
/// criterion "stale coordinates never slice live bytes" stops being sayable.
#[test]
fn a_snapshot_binding_carries_both_identities() {
    let schema = envelope_schema();
    let binding = &schema["$defs"]["snapshotBinding"];
    let required: BTreeSet<String> = binding["required"]
        .as_array()
        .expect("snapshotBinding declares required properties")
        .iter()
        .map(|value| value.as_str().expect("required holds strings").to_owned())
        .collect();
    let expected: BTreeSet<String> = ["repoSnapshot", "indexGeneration"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    assert_eq!(
        required, expected,
        "both snapshot identities are required: the freshness verdict is their relation, and one id alone has no relation to state"
    );
    assert_eq!(
        binding["additionalProperties"],
        serde_json::Value::Bool(false),
        "the snapshot binding is closed"
    );
}

fn reference_binding() -> ArtifactBinding {
    ArtifactBinding {
        artifact_id: ArtifactId::parse("journey-contract-1").expect("artifact id"),
        schema_id: "https://p50.dev/schemas/journey-contract.schema.json".to_owned(),
        document_version: SemanticVersion::parse("1.0.0").expect("document version"),
        schema_version: SemanticVersion::parse("1.0.0").expect("schema version"),
        digest: WireHash::parse(format!("sha256:{}", "a".repeat(64))).expect("digest"),
        scope: DevelopmentScope {
            workspace_id: WorkspaceId::parse("workspace-1").expect("workspace"),
            project_id: ProjectId::parse("project-1").expect("project"),
            subproject_id: None,
            execution_id: None,
        },
        producer: OpaqueId::parse("graphhelm-development-contracts").expect("producer"),
        snapshots: SnapshotBinding {
            repo_snapshot: OpaqueId::parse("snapshot-g").expect("repo snapshot"),
            index_generation: OpaqueId::parse("snapshot-g").expect("index generation"),
        },
    }
}

/// The control: an identical binding verifies. Without it every refusal cell below could be
/// satisfied by a verifier that refuses everything.
#[test]
fn an_identical_binding_verifies() {
    let reference = reference_binding();
    assert_eq!(verify_binding(&reference_binding(), &reference), Ok(()));
}

/// One cell per CHECK `verify_binding` performs, each with the mutation only it can catch.
///
/// "Per check", not "per property", and the distinction is the one that produced #272's finding.
///
/// The binding has EIGHT required fields and `verify_binding` makes FIVE comparisons, and the
/// arithmetic between them has two steps rather than one:
///
/// * **eight to six**: `artifactId` and `documentVersion` are compared by NOTHING. They identify
///   which artifact the binding is about; no check reads them, so no refusal names them.
/// * **six to five**: the schema comparison reads `schemaId` and `schemaVersion` together, under
///   one code.
///
/// An earlier doc a few hundred lines up read the five as a count of properties and told the
/// reader the binding had five of those. Two vocabularies share the adjective "verified", they
/// have different sizes, and — the part that is easy to miss — **different membership**: two
/// required properties are in neither check.
#[test]
fn a_scope_mismatch_is_refused_under_its_own_code() {
    let reference = reference_binding();
    let mut candidate = reference_binding();
    candidate.scope.project_id = ProjectId::parse("project-2").expect("project");
    assert_eq!(
        verify_binding(&candidate, &reference),
        Err(DevelopmentRefusalCode::ScopeMismatch)
    );
}

#[test]
fn a_schema_mismatch_is_refused_under_its_own_code() {
    let reference = reference_binding();
    let mut candidate = reference_binding();
    candidate.schema_version = SemanticVersion::parse("2.0.0").expect("schema version");
    assert_eq!(
        verify_binding(&candidate, &reference),
        Err(DevelopmentRefusalCode::BindingSchemaMismatch)
    );
}

#[test]
fn a_producer_mismatch_is_refused_under_its_own_code() {
    let reference = reference_binding();
    let mut candidate = reference_binding();
    candidate.producer = OpaqueId::parse("someone-else").expect("producer");
    assert_eq!(
        verify_binding(&candidate, &reference),
        Err(DevelopmentRefusalCode::BindingProducerMismatch)
    );
}

#[test]
fn a_digest_mismatch_is_refused_under_its_own_code() {
    let reference = reference_binding();
    let mut candidate = reference_binding();
    candidate.digest = WireHash::parse(format!("sha256:{}", "b".repeat(64))).expect("digest");
    assert_eq!(
        verify_binding(&candidate, &reference),
        Err(DevelopmentRefusalCode::BindingDigestMismatch)
    );
}

#[test]
fn a_snapshot_mismatch_is_refused_under_its_own_code() {
    let reference = reference_binding();
    let mut candidate = reference_binding();
    candidate.snapshots.index_generation = OpaqueId::parse("snapshot-h").expect("index generation");
    assert_eq!(
        verify_binding(&candidate, &reference),
        Err(DevelopmentRefusalCode::BindingSnapshotMissing)
    );
}

/// Freshness is the RELATION between the two identities, and it is derived rather than stored.
#[test]
fn freshness_is_the_relation_between_the_two_snapshot_identities() {
    let binding = reference_binding();
    assert!(
        binding.snapshots.is_fresh(),
        "equal identities mean coordinates resolve safely"
    );
    let mut stale = reference_binding();
    stale.snapshots.index_generation = OpaqueId::parse("snapshot-h").expect("index generation");
    assert!(
        !stale.snapshots.is_fresh(),
        "an index built from another snapshot is stale against these bytes"
    );
}

/// Canonical output is identical across input key order.
///
/// HONEST LIMIT, stated because it changes what this cell proves: this workspace builds serde_json
/// without preserve_order, so its object map is a BTreeMap and parsing already sorts. Measured, not
/// inferred. So key-order determinism is supplied by the dependency today and NO mutation of
/// canonical_json can be observed to break this. The cell is a regression net against someone
/// enabling preserve_order later - which would change canonical output silently and invalidate
/// every digest already published - and it is not evidence that this code implements the property.
#[test]
fn canonical_output_is_identical_across_input_key_order() {
    let first: serde_json::Value =
        serde_json::from_str(r#"{"b":1,"a":{"d":4,"c":3}}"#).expect("json");
    let second: serde_json::Value =
        serde_json::from_str(r#"{"a":{"c":3,"d":4},"b":1}"#).expect("json");
    assert_eq!(canonical_json(&first), canonical_json(&second));
    assert_eq!(canonical_json(&first), r#"{"a":{"c":3,"d":4},"b":1}"#);
}

/// A producer's platform is not part of an artifact's identity.
#[test]
fn a_windows_path_and_its_posix_twin_normalise_to_one_value() {
    let posix = "core/events/src/projection.rs";
    let windows = "core\\events\\src\\projection.rs";
    assert_ne!(posix, windows, "the two inputs really do differ as bytes");
    assert_eq!(
        normalise_path_separators(windows),
        normalise_path_separators(posix),
        "a Windows-shaped path digests as its POSIX twin: a producer's platform is not part of an \
         artifact's identity"
    );
    assert_eq!(
        normalise_path_separators(posix),
        posix,
        "and a POSIX path is unchanged, so normalisation is not silently rewriting both sides"
    );
}

/// PRESERVED — half one of the compatible-minor property.
///
/// Merely permitting an unknown field is not preservation: serde would DROP it, and a consumer that
/// reads and rewrites the artifact would silently delete a newer producer's data. Capture is what
/// makes the round-trip lossless.
#[test]
fn an_unknown_field_from_a_compatible_minor_survives_a_round_trip() {
    let text = r#"{
        "apiVersion": "p50.dev/development/v1alpha1",
        "kind": "CodeRule",
        "metadata": {
            "id": "rule-1",
            "artifactVersion": "1.0.0",
            "scope": {"workspaceId": "workspace-1", "projectId": "project-1"}
        },
        "producer": "graphhelm-development-contracts",
        "producerVersion": "0.1.0",
        "spec": {"note": "body"},
        "digest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "fieldFromANewerMinor": {"kept": true}
    }"#;
    let envelope: DevelopmentEnvelope =
        serde_json::from_str(text).expect("a compatible minor parses");
    assert!(
        envelope.additional.contains_key("fieldFromANewerMinor"),
        "the unknown field was captured rather than dropped: {:?}",
        envelope.additional
    );
    let rewritten = serde_json::to_value(&envelope).expect("re-serialises");
    assert_eq!(
        rewritten["fieldFromANewerMinor"],
        serde_json::json!({"kept": true}),
        "and it survives the rewrite byte-for-byte in value terms"
    );
}

/// NEVER AUTHORITY — half two, and a separate cell on purpose.
///
/// A single fixture that carries an unknown field and reaches the right answer satisfies both
/// halves by accident. This one asserts what preservation must NOT buy: the field cannot move the
/// digest, so it cannot change identity or any decision taken on identity.
#[test]
fn a_preserved_unknown_field_cannot_change_the_digest() {
    let base = r#"{
        "apiVersion": "p50.dev/development/v1alpha1",
        "kind": "CodeRule",
        "metadata": {
            "id": "rule-1",
            "artifactVersion": "1.0.0",
            "scope": {"workspaceId": "workspace-1", "projectId": "project-1"}
        },
        "producer": "graphhelm-development-contracts",
        "producerVersion": "0.1.0",
        "spec": {"note": "body"},
        "digest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    }"#;
    let with_extra = base.replace(
        r#""spec": {"note": "body"},"#,
        r#""spec": {"note": "body"}, "fieldFromANewerMinor": {"kept": true},"#,
    );
    let plain: DevelopmentEnvelope = serde_json::from_str(base).expect("parses");
    let extended: DevelopmentEnvelope = serde_json::from_str(&with_extra).expect("parses");
    // The precondition reports a THIRD state on purpose, and the prefix is the whole point.
    //
    // This cell and the preservation cell are not independent: any mutation that breaks capture
    // breaks this cell's ARRANGEMENT as well as that cell's PROPERTY. What was missing is not
    // independence — it is distinguishability. Marked this way, dropping the capture reads as
    // "preservation FAILED, this one never ran", while letting the capture into the digest reads
    // as "preservation fine, this one FAILED". Two mutations, two signatures.
    //
    // The limitation stays and is not repaired by this: there is still NO mutation that reddens
    // preservation alone. What this buys is diagnosis, not independence.
    assert!(
        extended.additional.contains_key("fieldFromANewerMinor"),
        "HARNESS-BROKE: the arrangement did not assemble — the extra field was not captured, so \
         nothing here measured whether capture carries authority. This is not a property failure."
    );
    assert_eq!(
        plain.digest_input(),
        extended.digest_input(),
        "an unknown field is preserved but carries no authority: it may not move the digest"
    );
}

/// Does this lockfile text show `serde_json` pulling `indexmap`?
///
/// Extracted so the DETECTION can be exercised on synthetic input. The real-file cell below cannot
/// be sabotaged: editing `Cargo.lock` by hand does not survive, because cargo REPAIRS the lockfile
/// before the test runs — measured, with the injected line present before the run and absent after.
/// A self-repairing subject turns a sabotage into silence, so the logic is proven below on input
/// nothing can repair.
fn serde_json_pulls_indexmap(lock: &str) -> bool {
    lock.split("[[package]]")
        .find(|entry| entry.contains("name = \"serde_json\""))
        .map(|block| block.contains("\"indexmap\""))
        .unwrap_or(false)
}

/// The control: the detection fires when the thing it looks for is present.
///
/// Without this, the cell below asserts an absence with no evidence that the probe could ever see a
/// presence — and an absence measured by a blind probe is not an absence.
#[test]
fn the_preserve_order_probe_can_actually_see_indexmap() {
    let clean = "[[package]]
name = \"serde_json\"
dependencies = [
 \"itoa\",
]
";
    let enabled = "[[package]]
name = \"serde_json\"
dependencies = [
 \"indexmap\",
 \"itoa\",
]
";
    assert!(
        !serde_json_pulls_indexmap(clean),
        "a clean block must not read as preserve_order"
    );
    assert!(
        serde_json_pulls_indexmap(enabled),
        "the probe must see indexmap when it is there, or the real-file cell proves nothing"
    );
}

/// Key-order determinism is NOT implemented by `canonical_json`, and this cell watches the reason.
///
/// This workspace builds `serde_json` without `preserve_order`, so its object map is a `BTreeMap`,
/// keys come out sorted from the PARSE, and a disordered map cannot be constructed at all. Measured
/// with a control rather than inferred from a missing feature flag: serde_json's dependency list
/// holds itoa, memchr, serde, serde_core and zmij and no indexmap, while indexmap IS present
/// elsewhere in the lock, so its absence there is meaningful rather than a crate nobody vendored.
///
/// So every cell about key order passes trivially, and deleting the sort inside `canonical_json`
/// reddens nothing. This one watches what CONSTRAINS THE INPUT instead: it goes red on the day
/// `preserve_order` is enabled, which is the day the property stops being free and every published
/// digest would otherwise change in silence.
///
/// **What this cell cannot do, stated rather than assumed:** it cannot be demonstrated red inside
/// this task. Enabling the feature means editing `Cargo.toml`, outside this task's file scope, and
/// hand-editing the lockfile does not survive cargo. Its LOGIC is proven by the control above; its
/// firing in the real scenario is reasoned, not observed.
///
/// Found by the consuming lane's author, by asking what constrains the input rather than by
/// sabotaging the subject: the subject was fine.
#[test]
fn serde_json_is_built_without_preserve_order() {
    let lock =
        std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock"))
            .expect("the workspace lockfile is readable");
    assert!(
        lock.contains("name = \"serde_json\""),
        "control: serde_json must be in the lockfile, or the assertion below reads an empty block"
    );
    assert!(
        !serde_json_pulls_indexmap(&lock),
        "serde_json now depends on indexmap, so preserve_order is enabled. Key order is no longer supplied by BTreeMap, canonical_json's explicit sort becomes load-bearing, and every digest published before this change was computed under different rules."
    );
}

fn fixture(relative: &str) -> serde_json::Value {
    let path = extension_dir().join("fixtures/contracts").join(relative);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("fixture unreadable at {}: {error}", path.display()));
    serde_json::from_str(&text).expect("a fixture is JSON")
}

/// Validation happens against the SCHEMA, before any typed deserialization, using the house
/// validator rather than a second one wired in for this test.
fn schema_diagnostics(value: &serde_json::Value) -> Vec<graphhelm_protocols::Diagnostic> {
    graphhelm_schema::validate_inline_value(&envelope_schema(), value, "development-contract")
        .expect("the envelope schema compiles offline")
}

/// The positive fixture validates. Without it every negative below is satisfied by a schema that
/// refuses everything.
#[test]
fn the_valid_fixture_validates() {
    let diagnostics = schema_diagnostics(&fixture("valid/code-rule-minimal.json"));
    assert!(
        diagnostics.is_empty(),
        "the positive fixture must validate, or the negatives prove nothing: {diagnostics:?}"
    );
}

/// Each negative fixture breaks exactly ONE thing, so a refusal is attributable to it.
///
/// A fixture that breaks two is satisfied by a validator that catches either one, which is the
/// composed-fixture defect at the level of test data rather than code.
#[test]
fn each_invalid_fixture_is_refused_for_its_own_reason() {
    for (name, expected_pointer) in [
        ("invalid/digest-not-a-wire-hash.json", "/digest"),
        ("invalid/kind-outside-the-closed-set.json", "/kind"),
        ("invalid/scope-missing-project.json", "/metadata/scope"),
        ("invalid/cardinality-too-many-bindings.json", "/bindings"),
        ("invalid/size-id-over-max-length.json", "/metadata/id"),
    ] {
        let diagnostics = schema_diagnostics(&fixture(name));
        assert!(
            !diagnostics.is_empty(),
            "{name} must be refused by the schema"
        );
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.path.starts_with(expected_pointer)),
            "{name} must be refused AT {expected_pointer}, not merely refused somewhere: {diagnostics:?}"
        );
    }
}

/// The unknown-major fixture is the one the SCHEMA cannot catch, and that is the point.
///
/// Its `apiVersion` matches the declared pattern, so schema validation passes. Major compatibility
/// is a separate decision taken before typed deserialization, and this cell pins that the two are
/// different mechanisms rather than one. A reader who assumes the schema covers versioning would
/// otherwise never learn it does not.
#[test]
fn the_unknown_major_fixture_passes_schema_and_is_refused_by_the_version_check() {
    let value = fixture("invalid/unknown-major.json");
    assert!(
        schema_diagnostics(&value).is_empty(),
        "the schema admits the SHAPE of a future major: refusing it here would be the wrong layer"
    );
    let api_version = value["apiVersion"]
        .as_str()
        .expect("apiVersion is a string");
    assert_ne!(
        development_api_version_major(api_version),
        Some(DEVELOPMENT_API_MAJOR),
        "and the version check is what refuses it"
    );
}

/// The four closed artifacts this task binds are UNCHANGED, asserted rather than promised.
///
/// The plan forbids copying #215's artifacts into a competing format and requires binding them
/// instead. That is only enforceable if "unchanged" has an anchor, and until this cell the anchors
/// lived only in a blueprint document — a promise with no check, which is the defect the blueprint
/// itself names.
///
/// **Anchored on git BLOB IDS, not on a hash of the working tree, and the first run is why.** It was
/// written against working-tree bytes and failed immediately on the Context Capsule schema: the pins
/// were computed from git-stored content, and that file is checked out CRLF because it sits outside
/// this package's `.gitattributes` rule. **"Byte-identical" is only well defined relative to a byte
/// space** — git-stored or working-tree — and the two differ per platform. A blob id is the
/// git-stored identity exactly, needs no hashing, and is the same on every machine.
///
/// Hashing the working tree would have passed here and failed on Linux; normalising line endings
/// before hashing would have passed everywhere while quietly measuring SEMANTIC identity instead of
/// byte identity, which is the weaker claim this cell exists to avoid making.
#[test]
fn the_bound_closed_artifacts_are_byte_identical_to_their_pins() {
    for (relative, pinned) in [
        (
            "extensions/builtin/graphhelm-jpd/schemas/journey-contract.schema.json",
            "2bd688c56c603cc28a0086e5264ffd2f40e4000f",
        ),
        (
            "extensions/builtin/graphhelm-jpd/schemas/journey-verification-result.schema.json",
            "d97288e511c1f3068e21653a9b58ebaf388def78",
        ),
        (
            "schemas/context-capsule.schema.json",
            "6d19998169701b5ff41d2a0f32ae39c0bbc322d6",
        ),
        (
            "schemas/releases/1.0.0/context-capsule.schema.json",
            "6d19998169701b5ff41d2a0f32ae39c0bbc322d6",
        ),
    ] {
        let output = std::process::Command::new("git")
            .args(["rev-parse", &format!("HEAD:{relative}")])
            .current_dir(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
            .output()
            .expect("git is available");
        assert!(
            output.status.success(),
            "control: git could not resolve {relative}, so the comparison below proves nothing"
        );
        let blob = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        assert_eq!(
            blob, pinned,
            "{relative} changed: a bound artifact was edited rather than versioned, which is what binding exists to prevent"
        );
    }
}

/// The two Context Capsule copies are the SAME blob, which is an invariant in its own right.
///
/// Two copies that agree today can drift tomorrow, and the released copy is the one nobody edits
/// deliberately — so a divergence would appear as the released schema quietly falling behind.
#[test]
fn the_two_context_capsule_copies_are_the_same_bytes() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let live = std::fs::read(repo.join("schemas/context-capsule.schema.json")).expect("live copy");
    let released = std::fs::read(repo.join("schemas/releases/1.0.0/context-capsule.schema.json"))
        .expect("released copy");
    assert_eq!(
        live, released,
        "the live and released Context Capsule schemas diverged"
    );
}

/// The released `event-envelope` copy is UNEDITED, and the live copy's divergence is DECLARED
/// (#836, H's review; #1054).
///
/// This cell was written as byte-identity between the two copies, because a second schema with two
/// copies and only one of them guarded is the shape the NEXT PR falls through -- editing the live
/// copy and forgetting the released one while the cell stayed silent because it only ever named
/// Context Capsule. **What it guards against is SILENT DRIFT, not divergence**, which is what its
/// own original words said: "the released copy is the one nobody edits deliberately -- so a
/// divergence would appear as the released schema quietly falling behind."
///
/// `#1054` made the live copy diverge ON PURPOSE: `agent_presence_declared` joins `$defs/eventKind`
/// as a disjoint `oneOf` branch and `documentVersion` moves to `1.1.0`, while
/// `schemas/releases/1.0.0/` stays frozen BY RULE. Byte-identity can no longer be asserted.
/// Deleting this cell, or editing the frozen release to satisfy it, would throw away the half that
/// still holds -- and a cell that passes because it compares nothing is worse than the cell it
/// replaced.
///
/// So it keeps BOTH halves, in the shape `core/schema-evolution/tests/baseline_origin.rs` already
/// uses for its ledger of deliberate divergences: named, exact, and joined in the same commit that
/// introduces them.
///
/// 1. **The released copy is pinned.** Any edit to `schemas/releases/1.0.0/event-envelope.schema.json`
///    fails here. That is the invariant the original cell existed for, and it is untouched.
/// 2. **The live copy may move, and must SAY SO.** A live copy that differs from the released one
///    while still declaring the released `x-graphhelm-schema-version` is drift wearing the old
///    version number -- exactly the silence this cell is against. Compare `baseline_origin.rs`,
///    which spells the same rule "evolution has to say its own name".
///
/// **Pinned by git BLOB ID, not by a hash of working-tree bytes**, for the reason the sibling cell
/// `the_bound_closed_artifacts_are_byte_identical_to_their_pins` records above: these files sit
/// outside this package's `.gitattributes` rule and are checked out CRLF, so working-tree bytes are
/// platform-dependent while a blob id is not. `hash-object` rather than `rev-parse HEAD:` is that
/// SAME identity space read one step earlier, not a third convention: it applies git's clean filter,
/// so it answers the stored id on every platform (measured on this file: `hash-object` gives
/// `b8304acd`, matching `rev-parse HEAD:`, while `--no-filters` gives `59e37c31` and would not),
/// and it reads the WORKING TREE, so an edit reddens this cell before it is committed rather than
/// after.
#[test]
fn the_released_event_envelope_copy_is_pinned_and_its_live_divergence_is_declared() {
    const RELEASED: &str = "schemas/releases/1.0.0/event-envelope.schema.json";
    // Re-pinned when #1063 corrected the pre-release 1.0.0 baseline IN PLACE under D-037
    // (`execution_form_declared` gained `name`, `objective` and `executor`). The pin is a drift
    // detector, not a claim that the file is immutable: it moves only with a declared,
    // reviewed baseline correction, never as a side effect of editing a live schema.
    const RELEASED_BLOB: &str = "81de59f93c92b50c0bc133c5aa6474f3452db03f";
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");

    let output = std::process::Command::new("git")
        .args(["hash-object", RELEASED])
        .current_dir(&repo)
        .output()
        .expect("git is available");
    // CONTROL: a git that failed answers an empty stdout, and "" != the pin would fail LOUDLY but
    // for the wrong reason -- a red that names the wrong defect sends the next reader to the wrong
    // file. Neither the exit status nor the non-emptiness is assumed.
    assert!(
        output.status.success(),
        "control: git could not hash {RELEASED}, so the pin below proves nothing"
    );
    let blob = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    assert!(
        !blob.is_empty(),
        "control: git answered no blob id for {RELEASED}, so the pin below proves nothing"
    );
    assert_eq!(
        blob, RELEASED_BLOB,
        "{RELEASED} moved off its pin: a live schema's evolution is declared against the released copy rather than written into it, and a baseline correction re-pins this constant deliberately"
    );

    // The DECLARED divergence. The live copy is allowed to differ -- and if it does, it must carry
    // a different declared version, or the change is the silent drift this cell exists for.
    let live = std::fs::read(repo.join("schemas/event-envelope.schema.json")).expect("live copy");
    let released = std::fs::read(repo.join(RELEASED)).expect("released copy");
    if live != released {
        let version = |bytes: &[u8]| -> String {
            let document: serde_json::Value =
                serde_json::from_slice(bytes).expect("the schema parses as JSON");
            document["x-graphhelm-schema-version"]
                .as_str()
                .expect("every schema declares its own version")
                .to_owned()
        };
        assert_ne!(
            version(&live),
            version(&released),
            "the live event-envelope schema diverged from the released 1.0.0 copy while still declaring its version: evolution has to say its own name"
        );
    }
}

/// serde and `wire_name()` are TWO serialisers of one closed vocabulary, and they must agree.
///
/// The three equality cells above compare `wire_name()` against the schema and never touch serde,
/// so a variant can serialise as one spelling while the schema declares another and every existing
/// cell stays green. Nothing serialises these types yet, so today the disagreement is inert — it
/// arrives with the first consuming lane that puts one in a struct, as a document this schema
/// refuses carrying a value that looked right to whoever wrote it.
#[test]
fn serde_and_wire_name_agree_on_every_vocabulary() {
    fn check<T: serde::Serialize + Copy + std::fmt::Debug>(
        every: &[T],
        wire: impl Fn(T) -> &'static str,
        vocabulary: &str,
    ) {
        for item in every {
            let serialised = serde_json::to_value(item).expect("a closed vocabulary serialises");
            assert_eq!(
                serialised,
                serde_json::Value::String(wire(*item).to_owned()),
                "{vocabulary}: serde and wire_name disagree for {item:?}. Two serialisers of one vocabulary means a producer can emit a value this schema refuses."
            );
        }
    }

    check(
        DevelopmentKind::every(),
        DevelopmentKind::wire_name,
        "DevelopmentKind",
    );
    check(
        DevelopmentRefusalCode::every(),
        DevelopmentRefusalCode::wire_name,
        "DevelopmentRefusalCode",
    );
    check(
        CoverageState::every(),
        CoverageState::wire_name,
        "CoverageState",
    );
}

/// The source-search failure vocabulary agrees between the schema (authoritative) and the type.
///
/// The mutation this exists to catch is a RENAME on one side only, which a count comparison
/// cannot see -- so this asserts set equality and prints the two differences.
///
/// It is the guard the type's own cells cannot be: `wire_name()` and `every()` are generated from
/// one literal list in `wire_vocabulary!`, so comparing them compares that list with itself. This
/// is what anchors the list to the contract, and it is the enforcement half the jurisdiction rule
/// at `core/protocols/src/development.rs:89` requires of any closed set travelling in this
/// envelope.
#[test]
fn the_source_search_failure_vocabulary_is_one_set_on_both_sides() {
    let from_schema = schema_enum(&envelope_schema(), "sourceSearchFailure");
    let from_type: BTreeSet<String> = SourceSearchError::every()
        .iter()
        .map(|failure| failure.wire_name().to_owned())
        .collect();

    let only_in_schema: Vec<_> = from_schema.difference(&from_type).collect();
    let only_in_type: Vec<_> = from_type.difference(&from_schema).collect();
    assert!(
        only_in_schema.is_empty() && only_in_type.is_empty(),
        "the source-search failure vocabulary diverged. only in schema: {only_in_schema:?}; only \
         in the Rust type: {only_in_type:?}. The schema is authoritative; the type follows it."
    );
}

/// The vocabulary is APPEND-ONLY, which is a claim about ORDER that set equality cannot make.
///
/// A reorder keeps both sides equal as sets, so the cell above stays green through it while every
/// consumer reading the list positionally silently renumbers. Same reasoning, and same shape, as
/// `the_refusal_vocabulary_is_in_the_same_order_on_both_sides`.
#[test]
fn the_source_search_failure_vocabulary_is_in_the_same_order_on_both_sides() {
    let from_schema: Vec<String> = envelope_schema()["$defs"]["sourceSearchFailure"]["enum"]
        .as_array()
        .expect("$defs/sourceSearchFailure/enum is an array")
        .iter()
        .map(|value| {
            value
                .as_str()
                .expect("$defs/sourceSearchFailure/enum holds a non-string")
                .to_owned()
        })
        .collect();
    let from_type: Vec<String> = SourceSearchError::every()
        .iter()
        .map(|failure| failure.wire_name().to_owned())
        .collect();
    assert_eq!(
        from_schema, from_type,
        "the source-search failure vocabulary is in a different ORDER on the two sides. Set \
         equality cannot see this, and a positional consumer renumbers silently."
    );
}

/// The two fallback vocabularies overlap on EXACTLY the one member, read from both schemas.
///
/// `$defs/sourceSearchFailure` here is the CHANNEL's typed failure at the compile layer;
/// `retrieval-coverage-receipt.schema.json` `$defs/fallbackOutcome` is the RECEIPT's record of a
/// fallback, owned by the receipt boundary (#655). They are different sets with different owners,
/// and they share the spelling `unavailable`.
///
/// **That sharing was reached by luck.** The spelling was chosen from neighbouring snake_case by
/// an author who did not know `fallbackOutcome` existed. An agreement reached that way is not an
/// agreement: nothing tells the next person the two lists are related, and the day one side is
/// renamed both stay green. This makes it a checked property.
///
/// **INTERSECTION, deliberately not equality.** #655 owns `fallbackOutcome` and may add a member
/// to it legitimately -- `not_required` already has no counterpart here, and a success state would
/// be another. A guard asserting equality would fire on that correct divergence, which is a guard
/// that has to be deleted the first time it speaks.
///
/// Both sides are read from the SCHEMAS and neither from a Rust literal: the schemas are the
/// authority, and a cell that took one side from the type would agree with itself the day the
/// type and its schema drifted.
#[test]
fn the_two_fallback_vocabularies_overlap_on_exactly_one() {
    let path = extension_dir().join("schemas/retrieval-coverage-receipt.schema.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("receipt schema unreadable at {}: {error}", path.display()));
    let receipt: serde_json::Value =
        serde_json::from_str(&text).expect("the receipt schema is JSON");

    let channel = schema_enum(&envelope_schema(), "sourceSearchFailure");
    let receipt_outcomes = schema_enum(&receipt, "fallbackOutcome");
    let shared: BTreeSet<_> = channel.intersection(&receipt_outcomes).cloned().collect();

    let expected: BTreeSet<String> = ["unavailable".to_owned()].into_iter().collect();
    assert_eq!(
        shared, expected,
        "the declared overlap between the channel's failure vocabulary ({channel:?}) and the \
         receipt's fallback outcomes ({receipt_outcomes:?}) changed. One condition spelled two \
         ways, or two conditions spelled one way, is how these vocabularies drift while both look \
         correct locally -- and #655 owns the receipt side, so a new member THERE is legitimate \
         while a new SHARED member is a decision someone has to make on purpose."
    );
}
