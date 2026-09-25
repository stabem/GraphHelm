use std::collections::BTreeMap;

use graphhelm_protocols::Diagnostic;
use graphhelm_schema_evolution::{
    CatalogEntry, MAX_FILE_BYTES, MAX_JSON_DEPTH, MAX_MIGRATION_OPERATIONS, MAX_POINTER_BYTES,
    MigrationCatalogs, MigrationManifest, PatchOperation, SchemaCatalog, SchemaDigest,
    apply_migration, plan_migration_chain,
};
use proptest::prelude::*;
use semver::Version;
use serde::Deserialize;
use serde_json::{Value, json};

const SOURCE_HASH: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const TARGET_HASH: &str = "sha256:2222222222222222222222222222222222222222222222222222222222222222";
const THIRD_HASH: &str = "sha256:3333333333333333333333333333333333333333333333333333333333333333";
const FOURTH_HASH: &str = "sha256:4444444444444444444444444444444444444444444444444444444444444444";
const MISMATCH_HASH: &str =
    "sha256:9999999999999999999999999999999999999999999999999999999999999999";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApplyFixture {
    manifest: MigrationManifest,
    source_catalog_hash: SchemaDigest,
    target_catalog_hash: SchemaDigest,
    input: Value,
    expect: FixtureExpectation,
}

#[derive(Deserialize)]
struct FixtureExpectation {
    ok: bool,
    codes: Vec<String>,
    #[serde(default)]
    document: Option<Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChainFixture {
    schema: String,
    from_version: Version,
    to_version: Version,
    manifests: Vec<MigrationManifest>,
    expect: FixtureExpectation,
}

fn fixture(name: &str) -> ApplyFixture {
    serde_json::from_str(include_fixture(name)).unwrap()
}

fn chain_fixture(name: &str) -> ChainFixture {
    serde_json::from_str(include_fixture(name)).unwrap()
}

fn include_fixture(name: &str) -> &'static str {
    match name {
        "success.json" => include_str!("../../../conformance/migrations/success.json"),
        "source-invalid.json" => {
            include_str!("../../../conformance/migrations/source-invalid.json")
        }
        "hash-mismatch.json" => {
            include_str!("../../../conformance/migrations/hash-mismatch.json")
        }
        "invalid-pointer.json" => {
            include_str!("../../../conformance/migrations/invalid-pointer.json")
        }
        "partial-failure.json" => {
            include_str!("../../../conformance/migrations/partial-failure.json")
        }
        "destination-invalid.json" => {
            include_str!("../../../conformance/migrations/destination-invalid.json")
        }
        "cycle.json" => include_str!("../../../conformance/migrations/cycle.json"),
        "gap.json" => include_str!("../../../conformance/migrations/gap.json"),
        "downgrade.json" => include_str!("../../../conformance/migrations/downgrade.json"),
        "remote-ref.json" => include_str!("../../../conformance/migrations/remote-ref.json"),
        _ => panic!("unknown fixture"),
    }
}

fn manifest(operations: Vec<PatchOperation>) -> MigrationManifest {
    MigrationManifest {
        format_version: 1,
        schema: "graph".into(),
        from_version: Version::parse("1.0.0").unwrap(),
        to_version: Version::parse("2.0.0").unwrap(),
        source_schema_hash: SchemaDigest::parse(SOURCE_HASH).unwrap(),
        target_schema_hash: SchemaDigest::parse(TARGET_HASH).unwrap(),
        operations,
    }
}

fn catalog(version: &Version, digest: SchemaDigest) -> SchemaCatalog {
    SchemaCatalog {
        format_version: 1,
        release_version: version.clone(),
        schemas: BTreeMap::from([(
            "graph".into(),
            CatalogEntry {
                id: "https://p50.dev/schemas/graph.schema.json".into(),
                document_version: version.clone(),
                path: "schemas/graph.schema.json".into(),
                sha256: digest,
            },
        )]),
    }
}

fn apply_fixture(name: &str) -> graphhelm_schema_evolution::MigrationResult {
    let fixture = fixture(name);
    apply_fixture_document(&fixture, &fixture.input)
}

fn apply_fixture_document(
    fixture: &ApplyFixture,
    document: &Value,
) -> graphhelm_schema_evolution::MigrationResult {
    let source = catalog(
        &fixture.manifest.from_version,
        fixture.source_catalog_hash.clone(),
    );
    let target = catalog(
        &fixture.manifest.to_version,
        fixture.target_catalog_hash.clone(),
    );
    let catalogs = MigrationCatalogs {
        source: &source,
        target: &target,
    };
    let result = apply_migration(
        document,
        &fixture.manifest,
        &catalogs,
        |document| version_validation(document, "p50.dev/graph/v1"),
        |document| version_validation(document, "p50.dev/graph/v2"),
    );
    assert_eq!(result.ok, fixture.expect.ok);
    assert_eq!(
        result
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.clone())
            .collect::<Vec<_>>(),
        fixture.expect.codes
    );
    if fixture.expect.ok {
        assert_eq!(result.document, fixture.expect.document);
    }
    result
}

fn apply(
    document: &Value,
    manifest: &MigrationManifest,
) -> graphhelm_schema_evolution::MigrationResult {
    let source = catalog(&manifest.from_version, manifest.source_schema_hash.clone());
    let target = catalog(&manifest.to_version, manifest.target_schema_hash.clone());
    apply_migration(
        document,
        manifest,
        &MigrationCatalogs {
            source: &source,
            target: &target,
        },
        |_| Vec::new(),
        |_| Vec::new(),
    )
}

fn version_validation(document: &Value, expected_api_version: &str) -> Vec<Diagnostic> {
    if document
        .get("apiVersion")
        .is_none_or(|value| value == expected_api_version)
    {
        Vec::new()
    } else {
        vec![Diagnostic::error(
            "GHS002_SCHEMA",
            "fixture validation failed",
            "/",
            "fixture",
        )]
    }
}

fn edge(from: &str, to: &str) -> MigrationManifest {
    MigrationManifest {
        from_version: Version::parse(from).unwrap(),
        to_version: Version::parse(to).unwrap(),
        source_schema_hash: version_hash(from),
        target_schema_hash: version_hash(to),
        ..manifest(Vec::new())
    }
}

fn version_hash(version: &str) -> SchemaDigest {
    SchemaDigest::parse(match version {
        "1.0.0" => SOURCE_HASH,
        "2.0.0" => TARGET_HASH,
        "3.0.0" => THIRD_HASH,
        "4.0.0" => FOURTH_HASH,
        _ => panic!("missing test digest"),
    })
    .unwrap()
}

// Prevents an implementation from silently omitting one of the six approved RFC 6902 operations
// or decoding escaped JSON Pointer tokens incorrectly.
#[test]
fn all_operations_apply_in_order_with_exact_pointer_unescaping() {
    let result = apply_fixture("success.json");
    assert!(result.ok, "{:?}", result.diagnostics);
}

// Prevents a failed later operation from publishing or mutating a partially patched document.
#[test]
fn failed_second_operation_leaves_the_caller_document_unchanged() {
    let fixture = fixture("partial-failure.json");
    let original = fixture.input.clone();
    let before = original.clone();
    let result = apply_fixture_document(&fixture, &original);
    assert!(!result.ok);
    assert_eq!(result.diagnostics[0].code, "GHM003_PATCH_INVALID");
    assert_eq!(original, before);
    assert!(result.document.is_none());
}

// Prevents source validation from being skipped before patch execution.
#[test]
fn invalid_source_document_is_rejected_without_output() {
    let result = apply_fixture("source-invalid.json");
    assert!(!result.ok);
    assert_eq!(result.diagnostics[0].path, "/source");
    assert!(result.document.is_none());
}

// Prevents either catalog digest from being substituted for the manifest's exact digest.
#[test]
fn both_source_and_target_digest_mismatches_fail_closed() {
    let source_result = apply_fixture("hash-mismatch.json");
    assert_eq!(
        source_result.diagnostics[0].code,
        "GHM002_SCHEMA_HASH_MISMATCH"
    );
    assert_eq!(source_result.diagnostics[0].path, "/sourceSchemaHash");

    let document = json!({"apiVersion": "p50.dev/graph/v1"});
    let migration = manifest(Vec::new());
    let source = catalog(
        &migration.from_version,
        migration.source_schema_hash.clone(),
    );
    let target = catalog(
        &migration.to_version,
        SchemaDigest::parse(SOURCE_HASH).unwrap(),
    );
    let target_result = apply_migration(
        &document,
        &migration,
        &MigrationCatalogs {
            source: &source,
            target: &target,
        },
        |_| Vec::new(),
        |_| Vec::new(),
    );
    assert_eq!(
        target_result.diagnostics[0].code,
        "GHM002_SCHEMA_HASH_MISMATCH"
    );
    assert_eq!(target_result.diagnostics[0].path, "/targetSchemaHash");
    assert!(target_result.document.is_none());
}

// Prevents malformed escapes, ambiguous leading-zero indexes, and non-add `-` indexes from being
// interpreted as valid JSON Pointers.
#[test]
fn invalid_pointer_escaping_and_array_indexes_are_rejected() {
    let fixture_result = apply_fixture("invalid-pointer.json");
    assert_eq!(fixture_result.diagnostics[0].code, "GHM003_PATCH_INVALID");

    let document = json!({"items": ["a"]});
    for path in ["/bad~2escape", "/items/01", "/items/-"] {
        let result = apply(
            &document,
            &manifest(vec![PatchOperation::Remove { path: path.into() }]),
        );
        assert!(!result.ok, "{path}");
        assert!(result.document.is_none(), "{path}");
    }
}

// Prevents a failed test operation from exposing its sensitive expected or actual values.
#[test]
fn failed_test_is_atomic_and_redacts_payload_values() {
    let secret = "not-for-diagnostics";
    let document = json!({"token": secret});
    let result = apply(
        &document,
        &manifest(vec![PatchOperation::Test {
            path: "/token".into(),
            value: json!("different-secret"),
        }]),
    );
    assert!(!result.ok);
    assert!(result.document.is_none());
    let diagnostic_json = serde_json::to_string(&result.diagnostics).unwrap();
    assert!(!diagnostic_json.contains(secret));
    assert!(!diagnostic_json.contains("different-secret"));
}

// Prevents RFC 6902 `test` from treating equal JSON numbers as unequal only because one was
// deserialized with an integer representation and the other with a fractional representation.
#[test]
fn rfc_numeric_test_treats_scalar_one_and_one_point_zero_as_equal() {
    let document = json!(1);
    let result = apply(
        &document,
        &manifest(vec![PatchOperation::Test {
            path: String::new(),
            value: json!(1.0),
        }]),
    );
    assert!(result.ok, "{:?}", result.diagnostics);
    assert_eq!(result.document, Some(document));
}

// Prevents representation-sensitive numeric comparison from surviving inside nested arrays and
// objects even when the complete JSON structures are numerically equal.
#[test]
fn rfc_numeric_test_recurses_through_nested_arrays_and_objects() {
    let document = json!({"items": [1, {"value": 2_u64}], "ratio": -0.0});
    let result = apply(
        &document,
        &manifest(vec![PatchOperation::Test {
            path: String::new(),
            value: json!({"items": [1.0, {"value": 2.0}], "ratio": 0}),
        }]),
    );
    assert!(result.ok, "{:?}", result.diagnostics);
    assert_eq!(result.document, Some(document));
}

// Prevents integer-to-float rounding from making distinct mathematical values pass an RFC 6902
// `test`, including beyond the exact f64 integer range.
#[test]
fn rfc_numeric_test_keeps_genuinely_unequal_numbers_distinct() {
    for (actual, expected) in [
        (json!(1), json!(1.5)),
        (
            json!(9_007_199_254_740_993_u64),
            json!(9_007_199_254_740_992.0),
        ),
    ] {
        let result = apply(
            &actual,
            &manifest(vec![PatchOperation::Test {
                path: String::new(),
                value: expected,
            }]),
        );
        assert!(!result.ok);
        assert_eq!(result.diagnostics[0].code, "GHM003_PATCH_INVALID");
        assert!(result.document.is_none());
    }
}

// Prevents operation-count and pointer-length resource limits from being enforced one item late.
#[test]
fn exact_operation_and_pointer_limits_are_bounded() {
    let document = json!({});
    let accepted = manifest(
        (0..MAX_MIGRATION_OPERATIONS)
            .map(|_| PatchOperation::Test {
                path: String::new(),
                value: json!({}),
            })
            .collect(),
    );
    assert!(apply(&document, &accepted).ok);

    let too_many = manifest(
        (0..=MAX_MIGRATION_OPERATIONS)
            .map(|_| PatchOperation::Test {
                path: String::new(),
                value: json!({}),
            })
            .collect(),
    );
    let result = apply(&document, &too_many);
    assert_eq!(result.diagnostics[0].code, "GHM003_PATCH_INVALID");
    assert!(result.document.is_none());

    let exact_pointer = format!("/{}", "a".repeat(MAX_POINTER_BYTES - 1));
    let accepted_pointer = apply(
        &document,
        &manifest(vec![PatchOperation::Add {
            path: exact_pointer,
            value: Value::Null,
        }]),
    );
    assert!(accepted_pointer.ok);

    let long_pointer = format!("/{}", "a".repeat(MAX_POINTER_BYTES));
    let result = apply(
        &document,
        &manifest(vec![PatchOperation::Add {
            path: long_pointer,
            value: Value::Null,
        }]),
    );
    assert!(!result.ok);
    assert_eq!(result.diagnostics[0].code, "GHM003_PATCH_INVALID");
    assert!(result.document.is_none());
}

// Prevents serialized input/output limits from being checked by string length or after publication.
#[test]
fn input_and_output_over_four_mib_are_rejected_atomically() {
    let oversized_input = json!({"value": "x".repeat(MAX_FILE_BYTES)});
    let input_result = apply(&oversized_input, &manifest(Vec::new()));
    assert_eq!(input_result.diagnostics[0].code, "GHM003_PATCH_INVALID");
    assert!(input_result.document.is_none());

    let small_input = json!({});
    let output_result = apply(
        &small_input,
        &manifest(vec![PatchOperation::Add {
            path: "/value".into(),
            value: json!("x".repeat(MAX_FILE_BYTES)),
        }]),
    );
    assert_eq!(output_result.diagnostics[0].code, "GHM003_PATCH_INVALID");
    assert!(output_result.document.is_none());
    assert_eq!(small_input, json!({}));
}

fn nested(depth: usize) -> Value {
    (0..depth).fold(Value::Null, |child, _| json!({"nested": child}))
}

// Prevents deeply nested input or a depth-expanding operation from bypassing the depth bound.
#[test]
fn input_and_output_depth_over_128_are_rejected() {
    assert!(apply(&nested(MAX_JSON_DEPTH), &manifest(Vec::new())).ok);
    let input_result = apply(&nested(MAX_JSON_DEPTH + 1), &manifest(Vec::new()));
    assert_eq!(input_result.diagnostics[0].code, "GHM003_PATCH_INVALID");

    let output_result = apply(
        &json!({}),
        &manifest(vec![PatchOperation::Add {
            path: "/nested".into(),
            value: nested(MAX_JSON_DEPTH),
        }]),
    );
    assert_eq!(output_result.diagnostics[0].code, "GHM003_PATCH_INVALID");
    assert!(output_result.document.is_none());
}

// Prevents a patched document from being returned before target-schema validation succeeds.
#[test]
fn invalid_destination_is_rejected_without_partial_output() {
    let result = apply_fixture("destination-invalid.json");
    assert_eq!(result.diagnostics[0].code, "GHM004_DESTINATION_INVALID");
    assert_eq!(result.diagnostics[0].path, "/destination");
    assert!(result.document.is_none());
}

// Prevents manifest and operation extensions, malformed digests, and semantically invalid headers
// from creating an implicit executable migration format.
#[test]
fn manifest_shape_and_header_are_strict() {
    for source in [
        format!(
            r#"{{"formatVersion":1,"schema":"graph","fromVersion":"1.0.0","toVersion":"2.0.0","sourceSchemaHash":"{SOURCE_HASH}","targetSchemaHash":"{TARGET_HASH}","operations":[],"script":"run"}}"#
        ),
        format!(
            r#"{{"formatVersion":1,"schema":"graph","fromVersion":"1.0.0","toVersion":"2.0.0","sourceSchemaHash":"{SOURCE_HASH}","targetSchemaHash":"{TARGET_HASH}","operations":[{{"op":"remove","path":"/x","value":1}}]}}"#
        ),
        format!(
            r#"{{"formatVersion":1,"schema":"graph","fromVersion":"1.0.0","toVersion":"2.0.0","sourceSchemaHash":"sha256:ABC","targetSchemaHash":"{TARGET_HASH}","operations":[]}}"#
        ),
    ] {
        assert!(serde_json::from_str::<MigrationManifest>(&source).is_err());
    }

    for invalid in [
        MigrationManifest {
            format_version: 2,
            ..manifest(Vec::new())
        },
        MigrationManifest {
            from_version: Version::parse("2.0.0").unwrap(),
            to_version: Version::parse("1.0.0").unwrap(),
            ..manifest(Vec::new())
        },
    ] {
        let result = apply(&json!({}), &invalid);
        assert_eq!(result.diagnostics[0].code, "GHM001_MIGRATION_UNSUPPORTED");
        assert!(result.document.is_none());
    }
}

// Prevents patch values from introducing a network-resolved schema reference into an offline API.
#[test]
fn remote_references_in_patch_values_are_rejected() {
    let result = apply_fixture("remote-ref.json");
    assert_eq!(result.diagnostics[0].code, "GHM003_PATCH_INVALID");
    assert!(result.document.is_none());
}

// Prevents non-hierarchical absolute URI schemes from bypassing the offline-reference guard.
#[test]
fn remote_reference_schemes_without_slashes_are_rejected() {
    let result = apply(
        &json!({}),
        &manifest(vec![PatchOperation::Add {
            path: "/schema".into(),
            value: json!({"$ref": "urn:example:external-schema"}),
        }]),
    );
    assert!(!result.ok);
    assert_eq!(result.diagnostics[0].code, "GHM003_PATCH_INVALID");
    assert!(result.document.is_none());
}

// Prevents chain selection from skipping an explicit intermediate migration.
#[test]
fn exact_increasing_gap_free_chain_is_selected() {
    let manifests = vec![edge("1.0.0", "2.0.0"), edge("2.0.0", "3.0.0")];
    let chain = plan_migration_chain(
        "graph",
        &Version::parse("1.0.0").unwrap(),
        &Version::parse("3.0.0").unwrap(),
        &manifests,
    )
    .unwrap();
    assert_eq!(
        chain
            .iter()
            .map(|migration| migration.to_version.to_string())
            .collect::<Vec<_>>(),
        ["2.0.0", "3.0.0"]
    );
}

// Prevents a version-adjacent chain from silently switching schema identity when one manifest's
// target digest differs from the next manifest's source digest.
#[test]
fn chain_rejects_mismatched_intermediate_schema_digest() {
    let first = edge("1.0.0", "2.0.0");
    let mut second = edge("2.0.0", "3.0.0");
    second.source_schema_hash = SchemaDigest::parse(MISMATCH_HASH).unwrap();
    let diagnostics = plan_migration_chain(
        "graph",
        &Version::parse("1.0.0").unwrap(),
        &Version::parse("3.0.0").unwrap(),
        &[first, second],
    )
    .unwrap_err();
    assert_eq!(diagnostics[0].code, "GHM001_MIGRATION_UNSUPPORTED");
    assert_eq!(diagnostics[0].path, "/chain/1/sourceSchemaHash");
}

fn assert_chain_fixture_rejected(name: &str) {
    let fixture = chain_fixture(name);
    let result = plan_migration_chain(
        &fixture.schema,
        &fixture.from_version,
        &fixture.to_version,
        &fixture.manifests,
    );
    let diagnostics = result.unwrap_err();
    assert_eq!(
        diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.clone())
            .collect::<Vec<_>>(),
        fixture.expect.codes
    );
}

// Prevents cycles, missing outgoing edges, and requested downgrades from falling back to a partial
// or best-effort chain.
#[test]
fn cycle_gap_and_downgrade_are_rejected() {
    for fixture in ["cycle.json", "gap.json", "downgrade.json"] {
        assert_chain_fixture_rejected(fixture);
    }
}

// Prevents chain selection from choosing an arbitrary branch or accepting an edge beyond target.
#[test]
fn duplicate_outgoing_edge_and_target_overshoot_are_rejected() {
    let duplicate = vec![edge("1.0.0", "2.0.0"), edge("1.0.0", "3.0.0")];
    let duplicate_result = plan_migration_chain(
        "graph",
        &Version::parse("1.0.0").unwrap(),
        &Version::parse("3.0.0").unwrap(),
        &duplicate,
    );
    assert_eq!(
        duplicate_result.unwrap_err()[0].code,
        "GHM001_MIGRATION_UNSUPPORTED"
    );

    let overshoot = vec![edge("1.0.0", "4.0.0")];
    let overshoot_result = plan_migration_chain(
        "graph",
        &Version::parse("1.0.0").unwrap(),
        &Version::parse("3.0.0").unwrap(),
        &overshoot,
    );
    assert_eq!(
        overshoot_result.unwrap_err()[0].code,
        "GHM001_MIGRATION_UNSUPPORTED"
    );
}

prop_compose! {
    fn bounded_document()(keys in prop::collection::vec("[a-z]{1,8}", 0..16), values in prop::collection::vec(any::<i32>(), 0..16)) -> Value {
        Value::Object(keys.into_iter().zip(values).map(|(key, value)| (key, json!(value))).collect())
    }
}

prop_compose! {
    fn bounded_operations()(entries in prop::collection::vec(("[a-z]{1,8}", any::<i32>(), any::<bool>()), 0..32)) -> Vec<PatchOperation> {
        entries.into_iter().map(|(key, value, remove)| {
            if remove {
                PatchOperation::Remove { path: format!("/{key}") }
            } else {
                PatchOperation::Add { path: format!("/{key}"), value: json!(value) }
            }
        }).collect()
    }
}

proptest! {
    // Prevents arbitrary bounded operation sequences from panicking, mutating borrowed input, or
    // returning a partial document on any failure path.
    #[test]
    fn bounded_random_patches_are_panic_free_and_atomic(
        document in bounded_document(),
        operations in bounded_operations(),
    ) {
        let before = document.clone();
        let result = apply(&document, &manifest(operations));
        prop_assert_eq!(&document, &before);
        if !result.ok {
            prop_assert!(result.document.is_none());
        }
    }
}
