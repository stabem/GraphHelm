use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use graphhelm_protocols::Diagnostic;
use graphhelm_schema::OfflineSchemaSet;
use graphhelm_schema_evolution::{
    ConformanceResources, ConformanceSuite, MAX_CONFORMANCE_CASES, run_conformance,
};
use serde_json::{Value, json};

fn suite(source: Value) -> Result<ConformanceSuite, serde_json::Error> {
    serde_json::from_value(source)
}

fn schema_case(id: &str, input: &str) -> Value {
    json!({
        "id": id,
        "kind": "schema",
        "schema": "graph",
        "input": input,
        "expect": {"ok": true, "codes": []}
    })
}

// Prevents an ambiguous, extensible-by-accident manifest from becoming executable input.
#[test]
fn manifest_is_strict_sorted_and_path_confined() {
    let valid = json!({
        "formatVersion": 1,
        "cases": [
            schema_case("schema.graph.invalid.kind", "conformance/schemas/invalid/graph.json"),
            schema_case("schema.graph.valid.minimum", "conformance/schemas/valid/graph.json")
        ]
    });
    assert!(suite(valid.clone()).is_ok());

    let mut unknown_root = valid.clone();
    unknown_root["extra"] = json!(true);
    assert!(suite(unknown_root).is_err());

    let mut unknown_case = valid.clone();
    unknown_case["cases"][0]["extra"] = json!(true);
    assert!(suite(unknown_case).is_err());

    let mut unknown_kind = valid.clone();
    unknown_kind["cases"][0]["kind"] = json!("shell");
    assert!(suite(unknown_kind).is_err());

    let mut duplicate = valid.clone();
    duplicate["cases"][1]["id"] = duplicate["cases"][0]["id"].clone();
    assert!(suite(duplicate).is_err());

    let mut unsorted = valid.clone();
    unsorted["cases"].as_array_mut().unwrap().reverse();
    assert!(suite(unsorted).is_err());

    for unsafe_path in [
        "../secret.json",
        "/absolute.json",
        "C:/secret.json",
        "a\\b.json",
    ] {
        let mut unsafe_suite = valid.clone();
        unsafe_suite["cases"][0]["input"] = json!(unsafe_path);
        assert!(suite(unsafe_suite).is_err(), "{unsafe_path}");
    }
}

// Prevents the filesystem adapter from inventing version-qualified migration validators or
// loading undeclared resources. Each registry is an explicit, bounded manifest contract.
#[test]
fn manifest_declares_confined_versioned_validator_resources() {
    let declared = suite(json!({
        "formatVersion": 1,
        "validatorResources": {
            "graph@1.0.0": ["conformance/migrations/schemas/graph-1.0.0.schema.json"],
            "graph@2.0.0": ["conformance/migrations/schemas/graph-2.0.0.schema.json"]
        },
        "cases": [schema_case("schema.graph.valid", "fixture.json")]
    }))
    .unwrap();
    assert_eq!(
        declared.validator_resources["graph@1.0.0"],
        ["conformance/migrations/schemas/graph-1.0.0.schema.json"]
    );

    for invalid in [
        json!({
            "formatVersion": 1,
            "validatorResources": {"graph@1.0.0": ["../secret.schema.json"]},
            "cases": [schema_case("schema.graph.valid", "fixture.json")]
        }),
        json!({
            "formatVersion": 1,
            "validatorResources": {"graph": ["conformance/graph.schema.json"]},
            "cases": [schema_case("schema.graph.valid", "fixture.json")]
        }),
        json!({
            "formatVersion": 1,
            "validatorResources": {"graph@1.0.0": []},
            "cases": [schema_case("schema.graph.valid", "fixture.json")]
        }),
    ] {
        assert!(suite(invalid).is_err());
    }
}

// Prevents a manifest from exceeding the fixed work bound by one case.
#[test]
fn manifest_case_count_is_bounded_exactly() {
    let cases = (0..=MAX_CONFORMANCE_CASES)
        .map(|index| schema_case(&format!("schema.graph.valid.{index:04}"), "fixture.json"))
        .collect::<Vec<_>>();
    assert!(suite(json!({"formatVersion": 1, "cases": cases})).is_err());
}

#[test]
fn directly_constructed_suite_is_bounded_before_dispatch() {
    let mut direct = suite(json!({
        "formatVersion": 1,
        "cases": [schema_case("schema.graph.valid.0000", "fixture.json")]
    }))
    .unwrap();
    direct.cases = vec![direct.cases[0].clone(); MAX_CONFORMANCE_CASES + 1];
    let resources = ConformanceResources {
        fixtures: BTreeMap::from([("fixture.json".into(), json!({"kind":"ExecutionGraph"}))]),
    };
    let dispatches = std::cell::Cell::new(0usize);
    let report = run_conformance(&direct, &resources, |_, _| {
        dispatches.set(dispatches.get() + 1);
        Vec::new()
    });

    assert!(!report.ok);
    assert_eq!(dispatches.get(), 0);
    assert_eq!((report.total, report.passed, report.failed), (0, 0, 0));
    assert_eq!(report.diagnostics.len(), 1);
    assert_eq!(report.diagnostics[0].code, "GHCONF001_FIXTURE_FAILED");
    assert_eq!(report.diagnostics[0].path, "/cases");
}

// Prevents case IDs from becoming an unbounded payload or path/URL/control-text channel.
#[test]
fn manifest_case_id_has_a_bounded_canonical_grammar() {
    let valid_id = "release.graph_v1-migration.2";
    assert!(
        suite(json!({
            "formatVersion": 1,
            "cases": [schema_case(valid_id, "fixture.json")]
        }))
        .is_ok()
    );

    let overlength = format!("a{}z", "b".repeat(127));
    let invalid_ids = [
        "/unix/path",
        "c:/windows/path",
        "https://example.invalid/case",
        "line\nbreak",
        "Uppercase",
        "two..separators",
        "starts.with-",
        "-starts.with",
        "secret=do-not-echo",
        overlength.as_str(),
    ];
    for invalid_id in invalid_ids {
        let error = suite(json!({
            "formatVersion": 1,
            "cases": [schema_case(invalid_id, "fixture.json")]
        }))
        .unwrap_err()
        .to_string();
        assert!(!error.contains(invalid_id), "rejected id leaked: {error}");
    }
}

// Prevents programmatic suite construction from bypassing ID validation before report
// serialization.
#[test]
fn runner_redacts_an_invalid_programmatic_case_id() {
    let secret_id = "secret=must-not-appear";
    let mut suite = suite(json!({
        "formatVersion": 1,
        "cases": [schema_case("schema.graph.valid", "fixture.json")]
    }))
    .unwrap();
    match &mut suite.cases[0] {
        graphhelm_schema_evolution::ConformanceCase::Schema { id, .. } => {
            *id = secret_id.into();
        }
        _ => unreachable!(),
    }
    let resources = ConformanceResources {
        fixtures: BTreeMap::from([("fixture.json".into(), json!({"kind":"ExecutionGraph"}))]),
    };
    let report = run_conformance(&suite, &resources, validation);
    assert!(!report.ok);
    assert_eq!(report.cases[0].id, "invalid-case");
    assert_eq!(report.diagnostics[0].path, "/cases/0");
    assert_eq!(report.diagnostics[0].source, "conformance");
    assert!(!serde_json::to_string(&report).unwrap().contains(secret_id));
}

fn validation(schema: &str, document: &Value) -> Vec<Diagnostic> {
    if schema == "graph" && document.get("kind") == Some(&json!("ExecutionGraph")) {
        Vec::new()
    } else {
        vec![Diagnostic::error(
            "GHS002_SCHEMA",
            "fixture does not satisfy schema",
            "/kind",
            "fixture",
        )]
    }
}

// Prevents expected schema rejection from being reported as a conformance-suite failure.
#[test]
fn schema_cases_dispatch_through_the_supplied_validator() {
    let suite = suite(json!({
        "formatVersion": 1,
        "cases": [
            {
                "id": "schema.graph.invalid.kind",
                "kind": "schema",
                "schema": "graph",
                "input": "conformance/schemas/invalid/graph.json",
                "expect": {"ok": false, "codes": ["GHS002_SCHEMA"], "paths": ["/kind"]}
            },
            {
                "id": "schema.graph.valid.minimum",
                "kind": "schema",
                "schema": "graph",
                "input": "conformance/schemas/valid/graph.json",
                "expect": {"ok": true, "codes": [], "paths": []}
            }
        ]
    }))
    .unwrap();
    let resources = ConformanceResources {
        fixtures: BTreeMap::from([
            (
                "conformance/schemas/valid/graph.json".into(),
                json!({"kind": "ExecutionGraph", "private": "must-not-appear"}),
            ),
            (
                "conformance/schemas/invalid/graph.json".into(),
                json!({"kind": "Wrong", "private": "must-not-appear"}),
            ),
        ]),
    };

    let report = run_conformance(&suite, &resources, validation);
    assert!(report.ok);
    assert_eq!((report.total, report.passed, report.failed), (2, 2, 0));
    assert_eq!(report.cases[0].diagnostic_codes, ["GHS002_SCHEMA"]);
    assert_eq!(report.cases[0].diagnostic_paths, ["/kind"]);
    let serialized = serde_json::to_string(&report).unwrap();
    assert!(!serialized.contains("must-not-appear"));
    assert!(!serialized.contains("private"));
}

fn fixture(path: &str) -> Value {
    serde_json::from_str(match path {
        "conformance/compatibility/breaking-required-property.json" => {
            include_str!("../../../conformance/compatibility/breaking-required-property.json")
        }
        "conformance/compatibility/breaking-missing-migration.json" => {
            include_str!("../../../conformance/compatibility/breaking-missing-migration.json")
        }
        "conformance/migrations/success.json" => {
            include_str!("../../../conformance/migrations/success.json")
        }
        _ => panic!("unknown fixture"),
    })
    .unwrap()
}

// Prevents conformance from replacing the real compatibility, release, or migration engines with
// fixture-name assertions.
#[test]
fn compatibility_release_and_migration_cases_use_the_pure_apis() {
    let suite = suite(json!({
        "formatVersion": 1,
        "cases": [
            {
                "id": "compatibility.breaking.required",
                "kind": "compatibility",
                "input": "conformance/compatibility/breaking-required-property.json",
                "expect": {"ok": false, "codes": ["GHC003_BREAKING_CHANGE"]}
            },
            {
                "id": "migration.success",
                "kind": "migration",
                "input": "conformance/migrations/success.json",
                "expect": {"ok": true, "codes": []}
            },
            {
                "id": "release.breaking.missing-migration",
                "kind": "release",
                "input": "conformance/compatibility/breaking-missing-migration.json",
                "comparison": "conformance/compatibility/breaking-required-property.json",
                "schema": "graph",
                "fromVersion": "1.0.0",
                "toVersion": "2.0.0",
                "expect": {"ok": false, "codes": ["GHC004_SEMVER_MISMATCH"]}
            }
        ]
    }))
    .unwrap();
    let paths = [
        "conformance/compatibility/breaking-required-property.json",
        "conformance/migrations/success.json",
        "conformance/compatibility/breaking-missing-migration.json",
    ];
    let resources = ConformanceResources {
        fixtures: paths
            .into_iter()
            .map(|path| (path.into(), fixture(path)))
            .collect(),
    };

    let report = run_conformance(&suite, &resources, |_, _| Vec::new());
    assert!(report.ok, "{:?}", report.diagnostics);
    assert_eq!((report.total, report.passed, report.failed), (3, 3, 0));
    assert_eq!(report.cases[0].diagnostic_codes, ["GHC003_BREAKING_CHANGE"]);
    assert!(report.cases[1].diagnostic_codes.is_empty());
    assert_eq!(report.cases[2].diagnostic_codes, ["GHC004_SEMVER_MISMATCH"]);
}

fn migration_schema_set(expected_api_version: &str) -> OfflineSchemaSet {
    OfflineSchemaSet::compile(BTreeMap::from([(
        "graph".into(),
        json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": "https://p50.dev/schemas/graph.schema.json",
            "type": "object",
            "properties": {"apiVersion": {"const": expected_api_version}},
            "additionalProperties": true
        }),
    )]))
    .unwrap()
}

// Prevents migration validation from trusting fixture booleans instead of exact versioned schema
// registries supplied by the caller.
#[test]
fn migration_uses_version_qualified_real_validators_for_source_and_target() {
    let mut migration: Value =
        serde_json::from_str(include_str!("../../../conformance/migrations/success.json")).unwrap();
    migration["manifest"]["operations"][1]["value"] = json!("wrong");
    migration["expect"] = json!({"ok": false, "codes": ["GHM004_DESTINATION_INVALID"]});
    let suite = suite(json!({
        "formatVersion": 1,
        "cases": [{
            "id": "migration.real-validator-rejects-destination",
            "kind": "migration",
            "input": "conformance/migrations/real-validator-reject.json",
            "expect": {"ok": false, "codes": ["GHM004_DESTINATION_INVALID"]}
        }]
    }))
    .unwrap();
    let resources = ConformanceResources {
        fixtures: BTreeMap::from([(
            "conformance/migrations/real-validator-reject.json".into(),
            migration,
        )]),
    };
    let source = migration_schema_set("p50.dev/graph/v1");
    let target = migration_schema_set("p50.dev/graph/v2");
    let seen = RefCell::new(Vec::new());
    let report = run_conformance(&suite, &resources, |validation_target, document| {
        seen.borrow_mut().push(validation_target.to_owned());
        let registry = match validation_target {
            "graph@1.0.0" => &source,
            "graph@2.0.0" => &target,
            _ => panic!("unexpected validation target"),
        };
        registry.validate(
            "https://p50.dev/schemas/graph.schema.json",
            document,
            "conformance",
        )
    });
    assert!(report.ok, "{:?}", report.diagnostics);
    assert_eq!(
        seen.into_inner(),
        ["graph@1.0.0".to_owned(), "graph@2.0.0".to_owned()]
    );
    assert_eq!(
        report.cases[0].diagnostic_codes,
        ["GHM004_DESTINATION_INVALID"]
    );
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn declared_public_resources(manifest: &Value) -> BTreeSet<String> {
    let mut declared = manifest["cases"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|case| [case.get("input"), case.get("comparison")])
        .flatten()
        .map(|path| path.as_str().unwrap().to_owned())
        .collect::<BTreeSet<_>>();
    for paths in manifest["validatorResources"].as_object().unwrap().values() {
        declared.extend(
            paths
                .as_array()
                .unwrap()
                .iter()
                .map(|path| path.as_str().unwrap().to_owned()),
        );
    }
    declared
}

fn on_disk_public_resources(root: &Path) -> BTreeSet<String> {
    let mut pending = vec![root.join("conformance")];
    let mut inventory = BTreeSet::new();
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                pending.push(entry.path());
            } else if entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "json")
            {
                let relative = entry
                    .path()
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                if relative != "conformance/manifest.json" {
                    inventory.insert(relative);
                }
            }
        }
    }
    inventory
}

fn exact_public_fixture_inventory(manifest: &Value, inventory: &BTreeSet<String>) -> bool {
    let declared = declared_public_resources(manifest);
    declared.len() == 40 && &declared == inventory
}

fn public_suite_and_resources() -> (Value, ConformanceSuite, ConformanceResources) {
    let root = repository_root();
    let manifest_value: Value =
        serde_json::from_slice(&fs::read(root.join("conformance/manifest.json")).unwrap()).unwrap();
    assert!(exact_public_fixture_inventory(
        &manifest_value,
        &on_disk_public_resources(&root)
    ));
    let suite = suite(manifest_value.clone()).unwrap();
    let mut paths = manifest_value["cases"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|case| [case.get("input"), case.get("comparison")])
        .flatten()
        .map(|path| path.as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    let fixtures = paths
        .into_iter()
        .map(|path| {
            let value = serde_json::from_slice(&fs::read(root.join(&path)).unwrap()).unwrap();
            (path, value)
        })
        .collect();
    (manifest_value, suite, ConformanceResources { fixtures })
}

#[test]
fn public_fixture_inventory_is_exact_and_rejects_an_unreferenced_extra() {
    let root = repository_root();
    let manifest: Value =
        serde_json::from_slice(&fs::read(root.join("conformance/manifest.json")).unwrap()).unwrap();
    let inventory = on_disk_public_resources(&root);
    assert_eq!(manifest["cases"].as_array().unwrap().len(), 38);
    assert_eq!(declared_public_resources(&manifest).len(), 40);
    assert!(exact_public_fixture_inventory(&manifest, &inventory));

    let mut with_extra = inventory;
    with_extra.insert("conformance/unreferenced-extra.json".into());
    assert!(!exact_public_fixture_inventory(&manifest, &with_extra));
}

struct PublicValidators {
    current: OfflineSchemaSet,
    versioned: BTreeMap<String, OfflineSchemaSet>,
}

fn public_schema_set(suite: &ConformanceSuite) -> PublicValidators {
    let root = repository_root();
    let resources = [
        "agent",
        "claim",
        "context-capsule",
        "edge",
        "extension",
        "graph",
        "graph-signal",
        "node",
        "policy-waiver",
    ]
    .into_iter()
    .map(|name| {
        let document = serde_json::from_slice(
            &fs::read(root.join(format!("schemas/{name}.schema.json"))).unwrap(),
        )
        .unwrap();
        (name.to_owned(), document)
    })
    .collect();
    PublicValidators {
        current: OfflineSchemaSet::compile(resources).unwrap(),
        versioned: suite
            .validator_resources
            .iter()
            .map(|(target, paths)| {
                let resources = paths
                    .iter()
                    .map(|path| {
                        let document =
                            serde_json::from_slice(&fs::read(root.join(path)).unwrap()).unwrap();
                        (path.clone(), document)
                    })
                    .collect();
                (
                    target.clone(),
                    OfflineSchemaSet::compile(resources).unwrap(),
                )
            })
            .collect(),
    }
}

fn public_validation(
    validators: &PublicValidators,
    validation_target: &str,
    document: &Value,
) -> Vec<Diagnostic> {
    let (schema, registry) = match validation_target.split_once('@') {
        Some((schema, _)) => match validators.versioned.get(validation_target) {
            Some(registry) => (schema, registry),
            None => {
                return vec![Diagnostic::error(
                    "GHS002_SCHEMA",
                    "versioned schema registry is not available",
                    "/",
                    "conformance",
                )];
            }
        },
        None => (validation_target, &validators.current),
    };
    registry.validate(
        &format!("https://p50.dev/schemas/{schema}.schema.json"),
        document,
        "conformance",
    )
}

// Prevents fixture-map insertion order, clocks, paths, or payloads from changing public evidence.
#[test]
fn public_manifest_is_complete_and_reports_are_byte_deterministic() {
    let (_, suite, resources) = public_suite_and_resources();
    assert_eq!(suite.cases.len(), 38);
    let schema_set = public_schema_set(&suite);
    let validate =
        |schema: &str, document: &Value| public_validation(&schema_set, schema, document);
    let forward = run_conformance(&suite, &resources, validate);

    let mut entries = resources.fixtures.iter().collect::<Vec<_>>();
    entries.reverse();
    let reverse = ConformanceResources {
        fixtures: entries
            .into_iter()
            .map(|(path, value)| (path.clone(), value.clone()))
            .collect(),
    };
    let backward = run_conformance(&suite, &reverse, validate);
    let mut reordered_suite = suite.clone();
    reordered_suite.cases.reverse();
    let reordered = run_conformance(&reordered_suite, &resources, validate);
    assert!(forward.ok, "{:?} {:?}", forward.diagnostics, forward.cases);
    assert_eq!((forward.total, forward.passed, forward.failed), (38, 38, 0));
    assert_eq!(
        serde_json::to_vec(&forward).unwrap(),
        serde_json::to_vec(&backward).unwrap()
    );
    assert_eq!(
        serde_json::to_vec(&forward).unwrap(),
        serde_json::to_vec(&reordered).unwrap()
    );
    let report = serde_json::to_string(&forward).unwrap();
    for forbidden in ["apiVersion", "executionId", "C:\\", "duration", "elapsed"] {
        assert!(!report.contains(forbidden), "{forbidden}");
    }
}

// Prevents the public fixture gate from drifting into a hand-picked subset of schema rules.
#[test]
fn public_fixture_gate_enforces_every_current_schema_constraint() {
    let (_, suite, mut resources) = public_suite_and_resources();
    resources
        .fixtures
        .get_mut("conformance/schemas/valid/agent.json")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .remove("capabilities");
    let schema_set = public_schema_set(&suite);
    let report = run_conformance(&suite, &resources, |schema, document| {
        public_validation(&schema_set, schema, document)
    });
    assert!(!report.ok);
    assert_eq!((report.passed, report.failed), (37, 1));
    let failed = report.cases.iter().find(|case| !case.passed).unwrap();
    assert_eq!(failed.id, "schema.agent.valid.minimum");
    assert!(
        failed
            .diagnostic_codes
            .iter()
            .any(|code| code == "GHS002_SCHEMA")
    );
}

// Prevents one stale expectation from failing adjacent cases or destabilizing aggregate counts.
#[test]
fn wrong_expected_code_fails_only_its_sorted_case() {
    let (mut manifest, _, resources) = public_suite_and_resources();
    manifest["cases"][0]["expect"]["codes"] = json!(["WRONG_CODE"]);
    let suite = suite(manifest).unwrap();
    let schema_set = public_schema_set(&suite);
    let report = run_conformance(&suite, &resources, |schema, document| {
        public_validation(&schema_set, schema, document)
    });
    assert!(!report.ok);
    assert_eq!((report.total, report.passed, report.failed), (38, 37, 1));
    assert!(!report.cases[0].passed);
    assert!(report.cases.iter().skip(1).all(|case| case.passed));
    assert_eq!(report.diagnostics.len(), 1);
    assert_eq!(report.diagnostics[0].code, "GHCONF001_FIXTURE_FAILED");
    assert_eq!(report.diagnostics[0].path, "/cases/0");
}

// Prevents an absent resource from masquerading as the expected failure of a real validator.
#[test]
fn missing_resource_always_fails_closed() {
    let suite = suite(json!({
        "formatVersion": 1,
        "cases": [{
            "id": "schema.graph.missing",
            "kind": "schema",
            "schema": "graph",
            "input": "conformance/schemas/invalid/missing.json",
            "expect": {"ok": false, "codes": []}
        }]
    }))
    .unwrap();
    let report = run_conformance(&suite, &ConformanceResources::default(), |_, _| Vec::new());
    assert!(!report.ok);
    assert_eq!((report.passed, report.failed), (0, 1));
    assert_eq!(report.diagnostics[0].code, "GHCONF001_FIXTURE_FAILED");
}
