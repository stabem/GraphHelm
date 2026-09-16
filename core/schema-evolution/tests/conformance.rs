use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use graphhelm_protocols::{Diagnostic, EventKind};
use graphhelm_schema::OfflineSchemaSet;
use graphhelm_schema_evolution::{
    ConformanceResources, ConformanceSuite, MAX_CONFORMANCE_CASES, run_conformance,
};
use serde::{Deserialize, Serialize};
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
    // 61 = the 54 resources after main's accounting cases, plus the two graph-signal-reply
    // fixtures the 1.1.0 evolution shipped, plus the three context-provenance fixtures (#1065:
    // valid, measured-estimate, traversal-source), plus the `root` refusal (#1086), plus the node
    // crew acceptance (#1049).
    declared.len() == 61 && &declared == inventory
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
fn checked_in_public_fixture_inventory_is_exact_and_rejects_an_unreferenced_extra() {
    let root = repository_root();
    let manifest: Value =
        serde_json::from_slice(&fs::read(root.join("conformance/manifest.json")).unwrap()).unwrap();
    let inventory = on_disk_public_resources(&root);
    assert_eq!(manifest["cases"].as_array().unwrap().len(), 59);
    assert_eq!(declared_public_resources(&manifest).len(), 61);
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
        "artifact-reference",
        "claim",
        "context-capsule",
        "context-provenance",
        "edge",
        "event-envelope",
        "evidence-record",
        "execution-accounting-receipt",
        "extension",
        "graph",
        "graph-signal",
        "node",
        "persisted-graph-version",
        "policy-waiver",
        "repository-scope",
        "sensitivity",
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

fn current_schema_set() -> OfflineSchemaSet {
    let root = repository_root();
    let resources = [
        "agent",
        "artifact-reference",
        "claim",
        "context-capsule",
        "context-provenance",
        "edge",
        "event-envelope",
        "evidence-record",
        "execution-accounting-receipt",
        "extension",
        "graph",
        "graph-signal",
        "node",
        "persisted-graph-version",
        "policy-waiver",
        "repository-scope",
        "sensitivity",
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
    OfflineSchemaSet::compile(resources).unwrap()
}

fn validate_current_schema(schema: &str, document: &Value) -> Vec<Diagnostic> {
    current_schema_set().validate(
        &format!("https://p50.dev/schemas/{schema}.schema.json"),
        document,
        "conformance",
    )
}

fn raw_digest() -> &'static str {
    "0000000000000000000000000000000000000000000000000000000000000000"
}

fn wire_hash() -> &'static str {
    "sha256:0000000000000000000000000000000000000000000000000000000000000000"
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SerdeEventEnvelopeProbe {
    schema_version: String,
    event_id: String,
    scope: Value,
    stream_id: String,
    sequence: u64,
    occurred_at: String,
    idempotency_key: String,
    actor: Value,
    sensitivity: String,
    kind: SerdeEventKindProbe,
    evidence_refs: Vec<Value>,
    artifact_refs: Vec<Value>,
    previous_hash: String,
    event_hash: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
enum SerdeEventKindProbe {
    SimulationStarted(Value),
}

fn serde_event_envelope_probe() -> SerdeEventEnvelopeProbe {
    SerdeEventEnvelopeProbe {
        schema_version: "1.0.0".into(),
        event_id: "event-test".into(),
        scope: json!({
            "workspaceId": "workspace-test",
            "projectId": "project-test",
            "executionId": "execution-test"
        }),
        stream_id: "stream-test".into(),
        sequence: 1,
        occurred_at: "2026-08-09T00:00:00Z".into(),
        idempotency_key: "idempotency-test".into(),
        actor: json!({"type": "system", "id": "system-test"}),
        sensitivity: "internal".into(),
        kind: SerdeEventKindProbe::SimulationStarted(simulation_started_payload()),
        evidence_refs: Vec::new(),
        artifact_refs: Vec::new(),
        previous_hash: "sha256:35c8ab0717bef1684ad07efcf3bedd4648c778a2c944cbd2c7e6a4802e2237b3"
            .into(),
        event_hash: wire_hash().into(),
    }
}

fn event_envelope(kind: &str, data: Value, project_scoped: bool) -> Value {
    let scope = if project_scoped {
        json!({"workspaceId": "workspace-test", "projectId": "project-test"})
    } else {
        json!({
            "workspaceId": "workspace-test",
            "projectId": "project-test",
            "executionId": "execution-test"
        })
    };
    json!({
        "schemaVersion": "1.0.0",
        "eventId": "event-test",
        "scope": scope,
        "streamId": "stream-test",
        "sequence": 1,
        "occurredAt": "2026-08-09T00:00:00Z",
        "idempotencyKey": "idempotency-test",
        "actor": {"type": "system", "id": "system-test"},
        "sensitivity": "internal",
        "kind": {"type": kind, "data": data},
        "evidenceRefs": [],
        "artifactRefs": [],
        "previousHash": "sha256:35c8ab0717bef1684ad07efcf3bedd4648c778a2c944cbd2c7e6a4802e2237b3",
        "eventHash": wire_hash()
    })
}

fn persisted_graph_version() -> Value {
    json!({
        "number": 2,
        "predecessor": {"number": 1, "semanticHash": wire_hash()},
        "topology": {
            "apiVersion": "p50.dev/graph/v1",
            "kind": "ExecutionGraph",
            "graphId": "graph-test",
            "executionId": "execution-test",
            "labels": {"tier": "production"},
            "entrypoints": ["start"],
            "nodes": {
                "start": {
                    "nodeType": "agent",
                    "optionality": "required",
                    "controls": [],
                    "contentSlotIds": ["slot-objective"]
                }
            },
            "edges": [],
            "budgets": {
                "maxNodes": 1,
                "maxDepth": 1,
                "maxMutations": 0,
                "maxRetriesPerNode": 0,
                "maxWallClockSeconds": 60,
                "maxApiCostUsd": 1,
                "maxParallelModelCalls": 1
            },
            "policies": [],
            "completion": {
                "controlType": "terminal_nodes",
                "identifiers": {"terminalNode": "start"},
                "digests": {},
                "integers": {},
                "flags": {"allowWaivers": false}
            }
        },
        "topologyHash": wire_hash(),
        "semanticHash": wire_hash(),
        "contentSlots": [{
            "slotId": "slot-objective",
            "ownerKind": "node",
            "ownerId": "start",
            "fieldKind": "objective",
            "ordinal": 0,
            "evidenceId": "evidence-objective",
            "contentSha256": raw_digest(),
            "sensitivity": "internal",
            "requiredForExecution": true
        }],
        "createdBy": {"type": "owner", "id": "owner-test"},
        "createdAt": "2026-08-09T00:00:00Z"
    })
}

fn safe_diagnostic() -> Value {
    json!({
        "code": "GHS002_SCHEMA",
        "severity": "error",
        "path": "/spec",
        "component": "schema",
        "sourceContentSha256": raw_digest(),
        "detailEvidenceId": "evidence-diagnostic"
    })
}

fn simulation_started_payload() -> Value {
    json!({
        "simulationId": "simulation-test",
        "graphVersion": 2,
        "graphHash": wire_hash()
    })
}

// Prevents persistence timestamps from admitting representations that normalize outside the
// four-digit wire year or lose fractional precision in the UTC-owned Rust types.
#[test]
fn persistence_schemas_share_one_canonical_utc_timestamp_profile() {
    let policy_waiver = || {
        json!({
            "id": "waiver-test",
            "requirement": "review",
            "executionId": "execution-test",
            "graphVersion": 2,
            "actor": "owner-test",
            "acknowledgedRisks": ["unreviewed change"],
            "scope": "execution",
            "createdAt": "2026-08-09T00:00:00Z",
            "expiresAt": null
        })
    };

    for timestamp in [
        "0000-01-01T00:00:00Z",
        "9999-12-31T23:59:59Z",
        "2026-08-09T01:02:03.1Z",
        "2026-08-09T01:02:03.123456789Z",
        "2026-08-09T23:59:60Z",
        "2026-08-09T23:59:60.123456789Z",
    ] {
        let mut event = event_envelope("simulation_started", simulation_started_payload(), false);
        event["occurredAt"] = json!(timestamp);
        assert!(validate_current_schema("event-envelope", &event).is_empty());

        let mut evidence = evidence_record(16, "available");
        evidence["createdAt"] = json!(timestamp);
        assert!(validate_current_schema("evidence-record", &evidence).is_empty());

        let mut graph = persisted_graph_version();
        graph["createdAt"] = json!(timestamp);
        assert!(validate_current_schema("persisted-graph-version", &graph).is_empty());

        let mut waiver = policy_waiver();
        waiver["createdAt"] = json!(timestamp);
        assert!(validate_current_schema("policy-waiver", &waiver).is_empty());
    }

    for timestamp in [
        "0000-01-01T00:00:00+23:59",
        "9999-12-31T23:59:59-23:59",
        "2026-08-09t01:02:03z",
        "2026-08-09T01:02:03+00:00",
        "2026-08-09T01:02:03.1234567890Z",
    ] {
        let mut event = event_envelope("simulation_started", simulation_started_payload(), false);
        event["occurredAt"] = json!(timestamp);
        assert!(!validate_current_schema("event-envelope", &event).is_empty());

        let mut evidence = evidence_record(16, "available");
        evidence["createdAt"] = json!(timestamp);
        assert!(!validate_current_schema("evidence-record", &evidence).is_empty());

        let mut graph = persisted_graph_version();
        graph["createdAt"] = json!(timestamp);
        assert!(!validate_current_schema("persisted-graph-version", &graph).is_empty());

        let mut waiver = policy_waiver();
        waiver["createdAt"] = json!(timestamp);
        assert!(!validate_current_schema("policy-waiver", &waiver).is_empty());
    }
}

// Prevents authoring plaintext, filesystem locations, and dynamic prose from crossing the
// append-only event boundary through any safe-projection container.
#[test]
fn event_schema_rejects_authoring_plaintext_and_paths() {
    let base = event_envelope(
        "graph_version_published",
        json!({"version": persisted_graph_version()}),
        false,
    );
    let diagnostics = validate_current_schema("event-envelope", &base);
    assert!(
        diagnostics.is_empty(),
        "safe baseline rejected: {diagnostics:?}"
    );

    for (pointer, field) in [
        ("/kind/data/version/topology", "instructions"),
        ("/kind/data/version/topology/nodes/start", "objective"),
        ("/kind/data/version/topology/nodes/start", "purpose"),
        ("/kind/data/version/topology/nodes/start", "output"),
        ("/kind/data/version/topology/nodes/start", "log"),
        ("/kind/data/version/topology/nodes/start", "credential"),
        ("/kind/data/version/topology/nodes/start", "environment"),
        ("/kind/data/version/topology/nodes/start", "message"),
        ("/kind/data/version/topology/nodes/start", "sourcePath"),
    ] {
        let mut event = base.clone();
        event
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert(field.into(), json!("secret text"));
        assert!(
            !validate_current_schema("event-envelope", &event).is_empty(),
            "accepted forbidden field {field:?} at {pointer}"
        );
    }
}

// Prevents the schema from flattening a Serde-tagged EventKind out of EventEnvelope.kind.
#[test]
fn serde_tagged_event_kind_serializes_validates_and_round_trips_nested() {
    let event = serde_event_envelope_probe();
    let document = serde_json::to_value(&event).unwrap();

    assert_eq!(document["kind"]["type"], "simulation_started");
    assert_eq!(document["kind"]["data"], simulation_started_payload());
    assert!(document.get("data").is_none());
    assert!(
        validate_current_schema("event-envelope", &document).is_empty(),
        "{:?}",
        validate_current_schema("event-envelope", &document)
    );
    assert_eq!(
        serde_json::from_value::<SerdeEventEnvelopeProbe>(document).unwrap(),
        event
    );
}

// Prevents adding a legacy/import receipt branch or silently dropping replay-critical fields from
// any safe event variant.
//
// THE NAME NO LONGER CARRIES A COUNT, and that is the repair rather than tidiness: a name that
// states a number goes stale silently, and this one had (twenty-five in the name, more in the
// enum). What the suite promises is coverage, and coverage is now asserted where it can fail.
#[test]
fn every_listed_event_variant_is_complete_closed_and_replay_safe() {
    let variants = conformance_table();

    // No length literal: a hand-maintained count is the mechanism that failed here (it
    // compared this table's length to 25 while the enum had grown past it, so unlisted
    // variants were invisible). COVERAGE is asserted against the enum instead, by name, in
    // `every_event_variant_appears_in_the_conformance_table`.
    assert!(!variants.is_empty());
    for (kind, data, project_scoped) in variants {
        let event = event_envelope(kind, data, project_scoped);
        let diagnostics = validate_current_schema("event-envelope", &event);
        assert!(diagnostics.is_empty(), "{kind}: {diagnostics:?}");

        let mut open = event;
        open["kind"]["data"]["unregistered"] = json!(true);
        assert!(
            !validate_current_schema("event-envelope", &open).is_empty(),
            "{kind} accepted an unregistered replay field"
        );
    }
}

// Prevents pre-release import formats and ambiguous source categories from re-entering the only
// writable envelope.
#[test]
fn graph_import_accepts_only_document_or_generated_provenance() {
    for source_kind in ["graph_document", "generated"] {
        let event = event_envelope(
            "graph_imported",
            json!({"sourceSha256": raw_digest(), "sourceKind": source_kind}),
            false,
        );
        assert!(validate_current_schema("event-envelope", &event).is_empty());
    }

    for source_kind in ["legacy_journal", "remote_url"] {
        let event = event_envelope(
            "graph_imported",
            json!({"sourceSha256": raw_digest(), "sourceKind": source_kind}),
            false,
        );
        assert!(!validate_current_schema("event-envelope", &event).is_empty());
    }
    let removed_receipt = event_envelope("legacy_events_imported", json!({}), false);
    assert!(!validate_current_schema("event-envelope", &removed_receipt).is_empty());
}

// Prevents a Foundation authoring document, dynamic diagnostic prose, or unsafe map key from
// masquerading as the bounded persisted projection.
#[test]
fn persisted_projection_is_closed_and_uses_only_registered_structural_maps() {
    let base = persisted_graph_version();
    assert!(validate_current_schema("persisted-graph-version", &base).is_empty());

    let authoring_graph: Value = serde_json::from_slice(
        &fs::read(repository_root().join("conformance/schemas/valid/graph.json")).unwrap(),
    )
    .unwrap();
    let raw_event = event_envelope(
        "graph_version_published",
        json!({"version": authoring_graph}),
        false,
    );
    assert!(!validate_current_schema("event-envelope", &raw_event).is_empty());

    for pointer in [
        "/topology/labels",
        "/topology/completion/identifiers",
        "/topology/completion/digests",
        "/topology/completion/integers",
        "/topology/completion/flags",
    ] {
        let mut invalid = base.clone();
        invalid
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("systemPrompt".into(), json!("private"));
        assert!(
            !validate_current_schema("persisted-graph-version", &invalid).is_empty(),
            "unsafe key accepted at {pointer}"
        );
    }

    for field in ["message", "source", "sourcePath"] {
        let mut diagnostic = safe_diagnostic();
        diagnostic[field] = json!("C:/private/graph.yaml");
        let event = event_envelope(
            "graph_validation_failed",
            json!({"diagnostics": [diagnostic]}),
            false,
        );
        assert!(
            !validate_current_schema("event-envelope", &event).is_empty(),
            "persisted diagnostic accepted {field}"
        );
    }
}

// Prevents separator insertion and casing changes from disguising content-bearing field families
// as registered structural map keys.
#[test]
fn persisted_projection_rejects_obfuscated_forbidden_map_keys() {
    let base = persisted_graph_version();
    for key in [
        "pro_mpt",
        "in__struc-tion",
        "SystemPrompt",
        "objectiveText",
        "sourcePath",
        "description",
        "displayName",
        "completionContract",
        "policyText",
        "diagnosticDetail",
        "rawContent",
        "responseText",
        "schemaComment",
        "exampleValue",
        "graphTitle",
        "freeFormProse",
        "humanNote",
    ] {
        let mut invalid = base.clone();
        invalid["topology"]["labels"] = json!({key: "private"});
        assert!(
            !validate_current_schema("persisted-graph-version", &invalid).is_empty(),
            "accepted obfuscated forbidden key {key:?}"
        );
    }
}

// Prevents wire-chain hashes from becoming raw digests and Evidence references from omitting
// either the plaintext-integrity digest or ciphertext-integrity digest.
#[test]
fn event_hashes_and_evidence_references_keep_distinct_digest_contracts() {
    let mut event = event_envelope("simulation_started", simulation_started_payload(), false);
    event["evidenceRefs"] = json!([{
        "evidenceId": "evidence-test",
        "contentSha256": raw_digest(),
        "ciphertextSha256": raw_digest()
    }]);
    assert!(validate_current_schema("event-envelope", &event).is_empty());

    for pointer in ["previousHash", "eventHash"] {
        let mut invalid = event.clone();
        invalid[pointer] = json!(raw_digest());
        assert!(!validate_current_schema("event-envelope", &invalid).is_empty());
    }
    for field in ["contentSha256", "ciphertextSha256"] {
        let mut invalid = event.clone();
        invalid["evidenceRefs"][0]
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(!validate_current_schema("event-envelope", &invalid).is_empty());
    }
}

// Prevents artifacts from naming non-content-addressed locations or zero-length content.
#[test]
fn artifact_reference_requires_canonical_locator_and_positive_byte_length() {
    let canonical_locator = format!("artifact://sha256/{}", raw_digest());
    let valid = json!({
        "artifactId": "artifact-test",
        "locator": canonical_locator,
        "contentSha256": raw_digest(),
        "mediaType": "application/json",
        "byteLength": 1,
        "sensitivity": "internal",
        "metadataVersion": "1.0.0"
    });
    assert!(validate_current_schema("artifact-reference", &valid).is_empty());

    for invalid_locator in [
        raw_digest().to_owned(),
        format!("artifact-test/{}", raw_digest()),
        format!("artifact://sha256/{}?download=1", raw_digest()),
        format!("artifact://sha256/{}", "A".repeat(64)),
    ] {
        let mut invalid = valid.clone();
        invalid["locator"] = json!(invalid_locator);
        assert!(!validate_current_schema("artifact-reference", &invalid).is_empty());
    }

    let mut empty = valid;
    empty["byteLength"] = json!(0);
    assert!(!validate_current_schema("artifact-reference", &empty).is_empty());
}

fn evidence_record(byte_length: u64, availability: &str) -> Value {
    json!({
        "evidenceId": "evidence-test",
        "scope": {
            "workspaceId": "workspace-test",
            "projectId": "project-test",
            "executionId": "execution-test"
        },
        "mediaType": "application/json",
        "sensitivity": "restricted",
        "cipherAlgorithm": "xchacha20poly1305",
        "cipherVersion": 1,
        "contentSha256": raw_digest(),
        "ciphertextSha256": raw_digest(),
        "nonce": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        "wrappedKey": {
            "keyId": "key-test",
            "algorithm": "aes-kw",
            "wrappedDekSha256": raw_digest()
        },
        "byteLength": byte_length,
        "createdAt": "2026-08-09T00:00:00Z",
        "retentionClass": "standard",
        "availability": availability
    })
}

// Prevents evidence metadata from exceeding a 16 MiB plaintext plus the fixed 16-byte AEAD tag.
#[test]
fn evidence_byte_length_is_bounded_ciphertext_length_including_aead_overhead() {
    const XCHACHA20_POLY1305_TAG_BYTES: u64 = 16;
    const MAX_PLAINTEXT_BYTES: u64 = 16 * 1024 * 1024;
    let maximum = MAX_PLAINTEXT_BYTES + XCHACHA20_POLY1305_TAG_BYTES;
    assert!(
        validate_current_schema("evidence-record", &evidence_record(16, "available")).is_empty()
    );
    assert!(
        validate_current_schema("evidence-record", &evidence_record(maximum, "available"))
            .is_empty()
    );
    assert!(
        !validate_current_schema("evidence-record", &evidence_record(15, "available")).is_empty()
    );
    assert!(
        !validate_current_schema(
            "evidence-record",
            &evidence_record(maximum + 1, "available")
        )
        .is_empty()
    );
}

// Prevents the prepared erasure state from drifting from the normative state-machine wire name.
#[test]
fn evidence_availability_uses_erasure_pending_not_erasure_requested() {
    assert!(
        validate_current_schema("evidence-record", &evidence_record(16, "erasure_pending"))
            .is_empty()
    );
    assert!(
        !validate_current_schema("evidence-record", &evidence_record(16, "erasure_requested"))
            .is_empty()
    );
}

// Prevents actor identities from inheriting the shorter opaque-ID bound or accepting unsafe ASCII.
#[test]
fn actor_id_has_its_dedicated_safe_ascii_256_byte_bound() {
    let mut event = event_envelope("simulation_started", simulation_started_payload(), false);
    event["actor"]["id"] = json!(format!("a{}", "-".repeat(255)));
    assert!(validate_current_schema("event-envelope", &event).is_empty());

    event["actor"]["id"] = json!(format!("a{}", "-".repeat(256)));
    assert!(!validate_current_schema("event-envelope", &event).is_empty());
    event["actor"]["id"] = json!("actor test");
    assert!(!validate_current_schema("event-envelope", &event).is_empty());
    event["actor"]["id"] = json!("actor/test");
    assert!(!validate_current_schema("event-envelope", &event).is_empty());
}

// Prevents the shared persistence identifier contract from narrowing Task 3's printable-ASCII
// grammar or accepting whitespace, path separators, colons, controls, non-ASCII, or overlength.
#[test]
fn shared_opaque_ids_follow_normative_printable_ascii_128_byte_grammar() {
    let schemas = [
        (
            "repository-scope",
            json!({"workspaceId": "workspace-test", "projectId": "project-test"}),
            "/workspaceId",
        ),
        (
            "event-envelope",
            event_envelope("simulation_started", simulation_started_payload(), false),
            "/eventId",
        ),
        (
            "evidence-record",
            evidence_record(16, "available"),
            "/evidenceId",
        ),
        (
            "artifact-reference",
            json!({
                "artifactId": "artifact-test",
                "locator": format!("artifact://sha256/{}", raw_digest()),
                "contentSha256": raw_digest(),
                "mediaType": "application/json",
                "byteLength": 1,
                "sensitivity": "internal",
                "metadataVersion": "1.0.0"
            }),
            "/artifactId",
        ),
    ];

    let valid_ids = [
        "id+tag".to_owned(),
        "user@example.com".to_owned(),
        "id=1".to_owned(),
        "a".repeat(128),
    ];
    for valid_id in valid_ids {
        for (schema, base, pointer) in &schemas {
            let mut document = base.clone();
            *document.pointer_mut(pointer).unwrap() = json!(&valid_id);
            assert!(
                validate_current_schema(schema, &document).is_empty(),
                "{schema} rejected valid opaque ID {valid_id:?}"
            );
        }
    }

    let invalid_ids = [
        "".to_owned(),
        "id test".to_owned(),
        "id/test".to_owned(),
        "id\\test".to_owned(),
        "id:test".to_owned(),
        "id\u{001f}test".to_owned(),
        "idé".to_owned(),
        "a".repeat(129),
    ];
    for invalid_id in invalid_ids {
        for (schema, base, pointer) in &schemas {
            let mut document = base.clone();
            *document.pointer_mut(pointer).unwrap() = json!(invalid_id);
            assert!(
                !validate_current_schema(schema, &document).is_empty(),
                "{schema} accepted invalid opaque ID {invalid_id:?}"
            );
        }
    }
}

// Prevents an erasure receipt from naming only a version that is ambiguous across policies.
#[test]
fn erasure_requested_requires_a_bounded_retention_policy_identity() {
    let valid = event_envelope(
        "evidence_erasure_requested",
        erasure_requested_payload(),
        true,
    );
    assert!(validate_current_schema("event-envelope", &valid).is_empty());

    let mut missing = valid.clone();
    missing["kind"]["data"]
        .as_object_mut()
        .unwrap()
        .remove("retentionPolicyId");
    assert!(!validate_current_schema("event-envelope", &missing).is_empty());
    let mut oversized = valid;
    oversized["kind"]["data"]["retentionPolicyId"] = json!(format!("p{}", "x".repeat(128)));
    assert!(!validate_current_schema("event-envelope", &oversized).is_empty());
}

fn erasure_requested_payload() -> Value {
    json!({
        "evidenceScope": {"workspaceId":"workspace-test","projectId":"project-test","executionId":"execution-test"},
        "operationId": "erasure-operation-test",
        "evidenceId": "evidence-test",
        "keyHandleId": "key-handle-test",
        "retentionPolicyId": "retention-standard",
        "retentionPolicyVersion": "1.0.0",
        "authority": "compliance-test",
        "reasonCode": "retention_expired",
        "priorState": "available",
        "state": "erasure_pending",
        "requestedAt": "2026-08-09T00:00:00Z"
    })
}

fn erasure_completed_payload() -> Value {
    json!({
        "evidenceScope": {"workspaceId":"workspace-test","projectId":"project-test","executionId":"execution-test"},
        "operationId": "erasure-operation-test",
        "evidenceId": "evidence-test",
        "keyHandleId": "key-handle-test",
        "retentionPolicyId": "retention-standard",
        "retentionPolicyVersion": "1.0.0",
        "authority": "compliance-test",
        "reasonCode": "retention_expired",
        "ciphertextSha256": raw_digest(),
        "providerReceiptId": "provider-receipt-test",
        "providerEpoch": 1,
        "priorState": "erasure_pending",
        "state": "erased",
        "requestedAt": "2026-08-09T00:00:00Z",
        "completedAt": "2026-08-09T01:00:00Z"
    })
}

// Prevents prepared/finalized erasure receipts from losing the identifiers and states required
// to correlate the policy decision, evidence, key revocation, and authenticated provider receipt.
#[test]
fn erasure_events_are_audit_complete_closed_and_bounded() {
    let payloads = [
        (
            "evidence_erasure_requested",
            erasure_requested_payload(),
            vec![
                "evidenceScope",
                "operationId",
                "evidenceId",
                "keyHandleId",
                "retentionPolicyId",
                "retentionPolicyVersion",
                "authority",
                "reasonCode",
                "priorState",
                "state",
                "requestedAt",
            ],
        ),
        (
            "evidence_erasure_completed",
            erasure_completed_payload(),
            vec![
                "evidenceScope",
                "operationId",
                "evidenceId",
                "keyHandleId",
                "retentionPolicyId",
                "retentionPolicyVersion",
                "authority",
                "reasonCode",
                "ciphertextSha256",
                "providerReceiptId",
                "providerEpoch",
                "priorState",
                "state",
                "requestedAt",
                "completedAt",
            ],
        ),
    ];

    for (kind, payload, required) in payloads {
        let valid = event_envelope(kind, payload.clone(), true);
        let diagnostics = validate_current_schema("event-envelope", &valid);
        assert!(diagnostics.is_empty(), "{kind}: {diagnostics:?}");

        for field in required {
            let mut missing = payload.clone();
            missing.as_object_mut().unwrap().remove(field);
            assert!(
                !validate_current_schema("event-envelope", &event_envelope(kind, missing, true))
                    .is_empty(),
                "{kind} accepted missing {field}"
            );
        }

        let mut open = payload;
        open["details"] = json!("free-form payload");
        assert!(
            !validate_current_schema("event-envelope", &event_envelope(kind, open, true))
                .is_empty(),
            "{kind} accepted an extra field"
        );
    }

    let mut oversized_operation = erasure_requested_payload();
    oversized_operation["operationId"] = json!("o".repeat(129));
    assert!(
        !validate_current_schema(
            "event-envelope",
            &event_envelope("evidence_erasure_requested", oversized_operation, true)
        )
        .is_empty()
    );

    let mut zero_epoch = erasure_completed_payload();
    zero_epoch["providerEpoch"] = json!(0);
    assert!(
        !validate_current_schema(
            "event-envelope",
            &event_envelope("evidence_erasure_completed", zero_epoch, true)
        )
        .is_empty()
    );
}

// Prevents the event schema from duplicating and drifting away from the higher-precedence public
// PolicyWaiver contract, including Serde's representation of an absent optional reason.
#[test]
fn embedded_policy_waiver_matches_the_normative_schema_exactly() {
    let root = repository_root();
    let official_valid: Value = serde_json::from_slice(
        &fs::read(root.join("conformance/schemas/valid/policy-waiver.json")).unwrap(),
    )
    .unwrap();
    let official_invalid: Value = serde_json::from_slice(
        &fs::read(root.join("conformance/schemas/invalid/policy-waiver.json")).unwrap(),
    )
    .unwrap();

    let mut with_reason = official_valid.clone();
    with_reason["reason"] = json!("owner accepted the recorded risk");
    let mut null_reason = official_valid.clone();
    null_reason["reason"] = Value::Null;
    let mut empty_risks = official_valid.clone();
    empty_risks["acknowledgedRisks"] = json!([]);
    let mut invalid_created_at = official_valid.clone();
    invalid_created_at["createdAt"] = json!("not-a-date");
    let mut string_expiry = official_valid.clone();
    string_expiry["expiresAt"] = json!("future-policy-window");
    let mut unknown_field = official_valid.clone();
    unknown_field["details"] = json!("not part of the public waiver contract");

    for (case, waiver) in [
        ("official valid with omitted reason", official_valid),
        ("official invalid graph version", official_invalid),
        ("present string reason", with_reason),
        ("null reason", null_reason),
        ("empty risks", empty_risks),
        ("invalid createdAt", invalid_created_at),
        ("string expiresAt", string_expiry),
        ("unknown field", unknown_field),
    ] {
        let normative_accepts = validate_current_schema("policy-waiver", &waiver).is_empty();
        let event = event_envelope("policy_waiver_created", json!({"waiver": waiver}), false);
        assert_eq!(
            validate_current_schema("event-envelope", &event).is_empty(),
            normative_accepts,
            "full event diverged for {case}"
        );
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
fn checked_in_public_manifest_is_complete_and_reports_are_byte_deterministic() {
    let (_, suite, resources) = public_suite_and_resources();
    assert_eq!(suite.cases.len(), 59);
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
    assert_eq!((forward.total, forward.passed, forward.failed), (59, 59, 0));
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
    assert_eq!((report.passed, report.failed), (58, 1));
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
    assert_eq!((report.total, report.passed, report.failed), (59, 58, 1));
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

/// The conformance table: one (wire name, payload, project-scoped) row per event variant.
///
/// Extracted from the test body so the COVERAGE assertion and the REPLAY-SAFETY walk read the
/// same rows. Two copies would drift, which is the defect this whole change exists to remove —
/// repeating it one level up while fixing it would be its own joke.
fn conformance_table() -> Vec<(&'static str, serde_json::Value, bool)> {
    vec![
        (
            "graph_imported",
            json!({"sourceSha256": raw_digest(), "sourceKind": "graph_document"}),
            false,
        ),
        (
            "graph_validation_failed",
            json!({"diagnostics": [safe_diagnostic()]}),
            false,
        ),
        (
            "memory_admission_refused",
            json!({"code": "secret_detected", "local": "content", "bytes": 42}),
            true,
        ),
        (
            "memory_publication_transitioned",
            json!({"recordId": "record-1", "transition": "publish", "resultingState": "published"}),
            true,
        ),
        (
            "memory_record_superseded",
            json!({
                "predecessorId": "record-1",
                "successorId": "record-2",
                "reason": "contradicted",
                "predecessorNewSemanticState": "contradicted"
            }),
            true,
        ),
        (
            "graph_version_published",
            json!({"version": persisted_graph_version()}),
            false,
        ),
        (
            "draft_proposed",
            json!({
                "draftId": "draft-test",
                "expectedVersion": 1,
                "expectedHash": wire_hash(),
                "operationCount": 1
            }),
            false,
        ),
        (
            "draft_rejected",
            json!({
                "draftId": "draft-test",
                "reasonCode": "policy_blocked",
                "diagnostics": [safe_diagnostic()],
                "detailEvidenceId": "evidence-rejection"
            }),
            false,
        ),
        (
            "draft_applied",
            json!({"draftId": "draft-test", "graphVersion": 2, "graphHash": wire_hash()}),
            false,
        ),
        (
            "policy_obligation_evaluated",
            json!({
                "draftId": "draft-test",
                "requirementId": "review",
                "status": "waived",
                "evidenceIds": ["evidence-review"],
                "reasonCode": "owner_override",
                "overrideable": true
            }),
            false,
        ),
        (
            "policy_waiver_created",
            json!({"waiver": {
                "id": "waiver-test",
                "requirement": "review",
                "executionId": "execution-test",
                "graphVersion": 2,
                "actor": "owner-test",
                "acknowledgedRisks": ["unreviewed change"],
                "scope": "execution",
                "createdAt": "2026-08-09T00:00:00Z",
                "expiresAt": null
            }}),
            false,
        ),
        ("simulation_started", simulation_started_payload(), false),
        (
            "node_state_changed",
            json!({
                "simulationId": "simulation-test",
                "nodeId": "node-test",
                "previousState": "waiting_capacity",
                "nextState": "succeeded"
            }),
            false,
        ),
        (
            "simulation_completed",
            json!({"simulationId": "simulation-test", "status": "blocked"}),
            false,
        ),
        (
            "execution_started",
            json!({
                "executionId": "execution-test",
                "graphVersion": 2,
                "graphHash": wire_hash(),
                "mode": "supervised"
            }),
            false,
        ),
        (
            "execution_mode_changed",
            json!({
                "executionId": "execution-test",
                "previousMode": null,
                "mode": "manual"
            }),
            false,
        ),
        (
            "node_outcome_recorded",
            json!({
                "executionId": "execution-test",
                "nodeId": "node-test",
                "outcome": "succeeded",
                "nextState": "succeeded"
            }),
            false,
        ),
        (
            "execution_completed",
            json!({"executionId": "execution-test", "status": "completed"}),
            false,
        ),
        (
            "signal_recorded",
            json!({
                "executionId": "execution-test",
                "signalId": "signal-test",
                "sourceKind": "node",
                "sourceId": "node-test",
                "kind": "unexpected_dependency",
                "severity": "high",
                "envelopeSha256": raw_digest()
            }),
            false,
        ),
        (
            "ghost_node_proposed",
            json!({
                "executionId": "execution-test",
                "nodeId": "ghost-test",
                "draftId": "draft-test"
            }),
            false,
        ),
        (
            "mutation_accepted",
            json!({
                "executionId": "execution-test",
                "draftId": "draft-test",
                "mode": "autopilot",
                "graphVersion": 4
            }),
            false,
        ),
        (
            "execution_paused",
            json!({"executionId": "execution-test"}),
            false,
        ),
        (
            "execution_resumed",
            json!({"executionId": "execution-test"}),
            false,
        ),
        (
            "integrity_checkpoint_created",
            json!({
                "streamId": "stream-test",
                "sequence": 1,
                "eventHash": wire_hash(),
                "repositoryFormat": "1.0.0",
                "authenticationTag": {
                    "keyId": "integrity-key",
                    "algorithm": "hmac-sha256",
                    "tagSha256": raw_digest()
                }
            }),
            true,
        ),
        (
            "evidence_erasure_requested",
            erasure_requested_payload(),
            true,
        ),
        (
            "evidence_erasure_completed",
            erasure_completed_payload(),
            true,
        ),
        (
            "evidence_ciphertext_deleted",
            json!({
                "evidenceScope": {"workspaceId":"workspace-test","projectId":"project-test","executionId":"execution-test"},
                "operationId": "erasure-operation-test",
                "evidenceId": "evidence-test",
                "ciphertextSha256": raw_digest(),
                "deletedAt": "2026-08-09T02:00:00Z"
            }),
            true,
        ),
        (
            "evidence_legal_hold_changed",
            json!({
                "evidenceScope": {"workspaceId":"workspace-test","projectId":"project-test","executionId":"execution-test"},
                "holdId": "hold-test",
                "evidenceId": "evidence-test",
                "authority": "compliance-test",
                "reasonCode": "litigation",
                "state": "placed",
                "changedAt": "2026-08-09T00:00:00Z"
            }),
            true,
        ),
        // ---------------------------------------------------------------------------------
        // The thirteen rows the coverage assertion demanded (#160, closing #167).
        //
        // Seven of these variants shipped in earlier milestones and were never listed here: the
        // old mechanism compared this table's length to the literal 25, so it could not see a
        // variant it did not already carry. Six are M11's customs family. Writing rows for
        // variants this lane does not own is safe precisely because a row is not a claim anyone
        // has to trust — the test validates each payload against the real current schema and
        // then requires it to REJECT an unregistered field, so a wrong payload here fails loudly
        // rather than recording a comfortable fiction.
        // ---------------------------------------------------------------------------------
        (
            "execution_form_declared",
            json!({
                "executionId": "execution-test",
                "nodeIds": ["start"],
                "nodeTimeoutSeconds": {"start": 900}
            }),
            false,
        ),
        (
            "execution_form_amended",
            json!({
                "executionId": "execution-test",
                "computedAtSequence": 4,
                "nodeTimeoutSeconds": {"start": 900},
                "observedSilenceSeconds": {"start": 30}
            }),
            false,
        ),
        (
            "gate_verdict",
            json!({
                "executionId": "execution-test",
                "nodeId": "start",
                "gateId": "gate-quality",
                "passed": false,
                "findings": [{
                    "severity": "high",
                    "claim": "the suite did not cover the changed branch",
                    "evidence": ["evidence-test"],
                    "remediation": "add a case that fails without the change"
                }]
            }),
            false,
        ),
        (
            "gate_certified",
            json!({
                "executionId": "execution-test",
                "gateId": "gate-quality",
                "suiteDigest": wire_hash(),
                "specimens": 3
            }),
            false,
        ),
        (
            "reuse_decision",
            json!({
                "executionId": "execution-test",
                "nodeId": "start",
                "plane": "tool_broker",
                "decision": "hit",
                "keyComponents": ["tool_version", "canonical_input"],
                "keyDigest": wire_hash(),
                "provenanceErased": false
            }),
            false,
        ),
        (
            "wake_lease",
            json!({
                "executionId": "execution-test",
                "sessionId": "session-test",
                "cursor": 0,
                "rendezvousId": "rendezvous-test",
                "maturesInSeconds": 60
            }),
            false,
        ),
        (
            "wake_lease_consumed",
            json!({
                "executionId": "execution-test",
                "sessionId": "session-test",
                "reason": "rung",
                "capturedArming": 4
            }),
            false,
        ),
        (
            "completion_claimed",
            json!({
                "executionId": "execution-test",
                "node": "implementation",
                "completesWaitSeq": 4,
                "evidence": [{
                    "kind": "patch",
                    "contentHash": wire_hash(),
                    "size": 2048
                }],
                "attestation": {
                    "asserter": "agent-claimer",
                    "mode": "operator_attested"
                }
            }),
            false,
        ),
        (
            "completion_cleared",
            json!({
                "executionId": "execution-test",
                "claimSeq": 5,
                "verifier": {"type": "machineReplay", "manifestHash": wire_hash()}
            }),
            false,
        ),
        (
            "completion_rejected",
            json!({
                "executionId": "execution-test",
                "claimSeq": 5,
                "verifier": {
                    "type": "countersign",
                    "identity": "reviewer-test",
                    "keyFingerprint": wire_hash()
                },
                "reasonCode": "evidence_did_not_replay"
            }),
            false,
        ),
        (
            "completion_refused",
            json!({
                "executionId": "execution-test",
                "node": "implementation",
                "claimedWaitSeq": 4,
                "reasonCode": "wait_superseded"
            }),
            false,
        ),
        // #161's kinds. Their Rust variants arrived from that lane; these rows are what stop the
        // coverage check being blind to the two newest variants — the exact hole that let a real
        // failure on main name thirteen and not fifteen.
        (
            "clearance_identity_registered",
            json!({
                "executionId": "execution-test",
                "identity": "auditor-a",
                "keyFingerprint": wire_hash()
            }),
            false,
        ),
        (
            "clearance_identity_revoked",
            json!({"executionId": "execution-test", "identity": "auditor-a"}),
            false,
        ),
        // #162's five. They were declared at every site the STORE checks -- the envelope schema,
        // its 1.0.0 mirror, both catalogs, the wire-name table -- and this table, which no site
        // list mentions, was not one of them. The guard above caught it, and it caught it only
        // because the neighbouring crate's suite was run at all: the branch that added these five
        // was red here from its first commit and green on every suite its author chose to run.
        (
            "sweep_performed",
            json!({
                "executionId": "execution-test",
                "asOf": "2026-08-09T00:00:00Z",
                "caller": "operator"
            }),
            false,
        ),
        (
            "overdue_exception",
            json!({
                "executionId": "execution-test",
                "nodeId": "node-1",
                "episodeSequence": 4,
                "stage": "claimed",
                "deadline": "2026-08-09T00:00:00Z"
            }),
            false,
        ),
        (
            "dlq_routed",
            json!({
                "executionId": "execution-test",
                "nodeId": "node-1",
                "episodeSequence": 4,
                "reason": "stalled"
            }),
            false,
        ),
        (
            "dlq_redrive",
            json!({
                "executionId": "execution-test",
                "nodeId": "node-1",
                "dlqEpisodeSequence": 5
            }),
            false,
        ),
        (
            "dlq_returned",
            json!({
                "executionId": "execution-test",
                "nodeId": "node-1",
                "dlqEpisodeSequence": 5,
                "waitWithinSeconds": 600
            }),
            false,
        ),
        // #1054. Execution-scoped (`false`): the envelope's top-level pairing binds this kind to
        // `scopeWithExecution`, so a project-scoped row would match zero branches.
        (
            "agent_presence_declared",
            json!({
                "actorId": "agent-planner",
                "actorType": "agent",
                "model": "claude-opus-5",
                "effort": "high",
                "session": "0123456789abcdef"
            }),
            false,
        ),
        // #1057: the SAME kind carrying a session and NO model -- how a session says it declares
        // nothing. `model` left `required` would have refused this shape, which is why the minor
        // drops it; the second row is here so the removal is exercised and not merely legal.
        (
            "agent_presence_declared",
            json!({
                "actorId": "agent-planner",
                "actorType": "agent",
                "session": "fedcba9876543210"
            }),
            false,
        ),
    ]
}

/// THE COVERAGE MECHANISM (#160): every variant the enum can produce appears in the conformance
/// table, asserted BY NAME so the failure says which one is missing.
///
/// What it replaces could not fail: `assert_eq!(variants.len(), 25)` compared this table's length
/// to a literal while the enum had grown past it, so unlisted variants were invisible to the suite
/// that claimed to cover them. (#167 asks the separate question of whether those seven are covered
/// anywhere else; this test's job is only to make the next omission impossible to miss.)
#[test]
fn every_event_variant_appears_in_the_conformance_table() {
    let listed: BTreeSet<&str> = conformance_table()
        .into_iter()
        .map(|(kind, _, _)| kind)
        .collect();
    let expected: BTreeSet<&str> = EventKind::EVERY_WIRE_NAME.iter().copied().collect();
    let missing: Vec<&str> = expected.difference(&listed).copied().collect();
    assert!(
        missing.is_empty(),
        "event variants absent from the conformance table: {missing:?}"
    );
}
