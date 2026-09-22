use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use graphhelm_schema::OfflineSchemaSet;
use graphhelm_schema_evolution::{
    MAX_CONFORMANCE_CASES, MAX_FILE_BYTES, MAX_MIGRATION_OPERATIONS, MAX_SCHEMAS, schema_digest,
};
use serde_json::{Value, json};
use tempfile::TempDir;

const SCHEMAS: [&str; 9] = [
    "agent",
    "claim",
    "context-capsule",
    "edge",
    "extension",
    "graph",
    "graph-signal",
    "node",
    "policy-waiver",
];

struct Repository {
    _directory: TempDir,
    root: PathBuf,
    catalog: PathBuf,
    baseline: PathBuf,
}

fn command() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn args(values: impl IntoIterator<Item = impl Into<OsString>>) -> Vec<OsString> {
    values.into_iter().map(Into::into).collect()
}

fn output_json(output: &std::process::Output) -> Value {
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn malformed_schema_invocations_return_one_json_envelope_on_stdout() {
    for arguments in [
        vec!["schema", "catalog"],
        vec!["schema", "catalog", "--unknown"],
        vec!["schema", "not-a-command"],
        vec!["--pretty", "schema", "catalog"],
        vec!["--json", "schema", "catalog"],
        vec!["schema", "--json", "catalog"],
    ] {
        let output = command().args(arguments).output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stderr.is_empty());
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["ok"], false);
        assert_eq!(value["command"], "schema");
        assert_eq!(value["diagnostics"][0]["code"], "GHCLI001_ARGUMENT_INVALID");
        assert_eq!(value["diagnostics"][0]["path"], "/arguments");
    }
}

fn windows_symlink_creation_is_unavailable(error: &std::io::Error) -> bool {
    error.raw_os_error() == Some(1314)
}

fn write_json(path: &Path, value: &Value) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

fn protocol_schema(name: &str, version: &str) -> Value {
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": format!("https://p50.dev/schemas/{name}.schema.json"),
        "x-graphhelm-schema-version": version,
        "type": "object",
        "additionalProperties": true
    })
}

fn migration_schema(version: &str, api_version: &str) -> Value {
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "https://p50.dev/schemas/graph.schema.json",
        "x-graphhelm-schema-version": version,
        "type": "object",
        "required": ["apiVersion"],
        "properties": {"apiVersion": {"const": api_version}},
        "additionalProperties": true
    })
}

fn write_catalog(
    root: &Path,
    catalog_relative: &str,
    release_version: &str,
    documents: &BTreeMap<String, Value>,
) -> PathBuf {
    let snapshot = catalog_relative != "schemas/catalog.json";
    let schemas = documents
        .iter()
        .map(|(name, document)| {
            let resource_relative = if snapshot {
                format!("schemas/releases/{release_version}/{name}.schema.json")
            } else {
                format!("schemas/{name}.schema.json")
            };
            write_json(&root.join(&resource_relative), document);
            (
                name.clone(),
                json!({
                    "id": format!("https://p50.dev/schemas/{name}.schema.json"),
                    "documentVersion": document["x-graphhelm-schema-version"],
                    "path": resource_relative,
                    "sha256": schema_digest(document).unwrap().as_str()
                }),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    let catalog = root.join(catalog_relative);
    write_json(
        &catalog,
        &json!({
            "formatVersion": 1,
            "releaseVersion": release_version,
            "schemas": schemas
        }),
    );
    catalog
}

fn repository_with_nine_schemas() -> Repository {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().to_path_buf();
    let documents = SCHEMAS
        .into_iter()
        .map(|name| (name.to_owned(), protocol_schema(name, "1.0.0")))
        .collect::<BTreeMap<_, _>>();
    let catalog = write_catalog(&root, "schemas/catalog.json", "1.0.0", &documents);
    let baseline = write_catalog(
        &root,
        "schemas/releases/1.0.0/catalog.json",
        "1.0.0",
        &documents,
    );
    write_json(
        &root.join("conformance/manifest.json"),
        &json!({
            "formatVersion": 1,
            "cases": [{
                "id": "schema.graph.valid.minimum",
                "kind": "schema",
                "schema": "graph",
                "input": "conformance/valid-graph.json",
                "expect": {"ok": true, "codes": []}
            }]
        }),
    );
    write_json(&root.join("conformance/valid-graph.json"), &json!({}));
    fs::create_dir_all(root.join("schemas/migrations")).unwrap();
    fs::write(root.join("schemas/CHANGELOG.md"), "# Schema changelog\n").unwrap();
    Repository {
        _directory: directory,
        root,
        catalog,
        baseline,
    }
}

struct MigrationRepository {
    _directory: TempDir,
    root: PathBuf,
    catalog: PathBuf,
    migration: PathBuf,
    input: PathBuf,
    output: PathBuf,
}

fn migration_repository() -> MigrationRepository {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().to_path_buf();
    let source = migration_schema("1.0.0", "p50.dev/graph/v1");
    let target = migration_schema("2.0.0", "p50.dev/graph/v2");
    let catalog = write_catalog(
        &root,
        "schemas/catalog.json",
        "2.0.0",
        &BTreeMap::from([("graph".into(), target.clone())]),
    );
    write_catalog(
        &root,
        "schemas/releases/1.0.0/catalog.json",
        "1.0.0",
        &BTreeMap::from([("graph".into(), source.clone())]),
    );
    let migration = root.join("schemas/migrations/graph/1.0.0--2.0.0.json");
    write_json(
        &migration,
        &json!({
            "formatVersion": 1,
            "schema": "graph",
            "fromVersion": "1.0.0",
            "toVersion": "2.0.0",
            "sourceSchemaHash": schema_digest(&source).unwrap().as_str(),
            "targetSchemaHash": schema_digest(&target).unwrap().as_str(),
            "operations": [{
                "op": "replace",
                "path": "/apiVersion",
                "value": "p50.dev/graph/v2"
            }]
        }),
    );
    let input = root.join("input.json");
    write_json(
        &input,
        &json!({"apiVersion": "p50.dev/graph/v1", "private": "do-not-echo"}),
    );
    let output = root.join("output.json");
    MigrationRepository {
        _directory: directory,
        root,
        catalog,
        migration,
        input,
        output,
    }
}

struct BreakingReleaseRepository {
    _directory: TempDir,
    baseline: PathBuf,
    candidate: PathBuf,
    migration: PathBuf,
    valid_manifest: Value,
}

fn breaking_release_repository() -> BreakingReleaseRepository {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let baseline_document = migration_schema("1.0.0", "p50.dev/graph/v1");
    let mut candidate_document = migration_schema("2.0.0", "p50.dev/graph/v2");
    candidate_document["required"] = json!(["apiVersion", "enabled"]);
    candidate_document["properties"]["enabled"] = json!({"type": "boolean"});
    let baseline = write_catalog(
        root,
        "schemas/releases/1.0.0/catalog.json",
        "1.0.0",
        &BTreeMap::from([("graph".into(), baseline_document.clone())]),
    );
    let candidate = write_catalog(
        root,
        "schemas/catalog.json",
        "2.0.0",
        &BTreeMap::from([("graph".into(), candidate_document.clone())]),
    );
    let migration = root.join("schemas/migrations/graph/1.0.0--2.0.0.json");
    let valid_manifest = json!({
        "formatVersion": 1,
        "schema": "graph",
        "fromVersion": "1.0.0",
        "toVersion": "2.0.0",
        "sourceSchemaHash": schema_digest(&baseline_document).unwrap().as_str(),
        "targetSchemaHash": schema_digest(&candidate_document).unwrap().as_str(),
        "operations": []
    });
    write_json(&migration, &valid_manifest);
    write_json(
        &root.join("conformance/manifest.json"),
        &json!({
            "formatVersion": 1,
            "cases": [{
                "id": "release.graph.breaking",
                "kind": "release",
                "input": "conformance/after.json",
                "comparison": "conformance/before-after.json",
                "schema": "graph",
                "fromVersion": "1.0.0",
                "toVersion": "2.0.0",
                "expect": {"ok": true, "codes": []}
            }]
        }),
    );
    write_json(&root.join("conformance/after.json"), &json!({}));
    write_json(&root.join("conformance/before-after.json"), &json!({}));
    fs::write(
        root.join("schemas/CHANGELOG.md"),
        "## [2.0.0]\n- BREAKING graph: requires enabled after migration\n",
    )
    .unwrap();
    BreakingReleaseRepository {
        _directory: directory,
        baseline,
        candidate,
        migration,
        valid_manifest,
    }
}

fn run_check(repository: &BreakingReleaseRepository) -> std::process::Output {
    command()
        .args(args([
            OsString::from("schema"),
            OsString::from("check"),
            OsString::from("--baseline"),
            repository.baseline.as_os_str().to_owned(),
            OsString::from("--candidate"),
            repository.candidate.as_os_str().to_owned(),
        ]))
        .output()
        .unwrap()
}

fn run_catalog(catalog: &Path) -> std::process::Output {
    command()
        .args(args([
            OsString::from("schema"),
            OsString::from("catalog"),
            OsString::from("--catalog"),
            catalog.as_os_str().to_owned(),
        ]))
        .output()
        .unwrap()
}

fn run_migration(repository: &MigrationRepository, output: &Path) -> std::process::Output {
    command()
        .args(args([
            OsString::from("schema"),
            OsString::from("migrate"),
            OsString::from("--catalog"),
            repository.catalog.as_os_str().to_owned(),
            OsString::from("--migration"),
            repository.migration.as_os_str().to_owned(),
            OsString::from("--input"),
            repository.input.as_os_str().to_owned(),
            OsString::from("--output"),
            output.as_os_str().to_owned(),
        ]))
        .output()
        .unwrap()
}

#[test]
fn five_schema_commands_return_one_json_document_with_exact_names() {
    let repository = repository_with_nine_schemas();
    let catalog = run_catalog(&repository.catalog);
    assert!(
        catalog.status.success(),
        "{}",
        String::from_utf8_lossy(&catalog.stdout)
    );
    let value = output_json(&catalog);
    assert_eq!(value["command"], "schema.catalog");
    assert_eq!(value["data"]["schemaCount"], 9);

    let check = command()
        .args(args([
            OsString::from("schema"),
            OsString::from("check"),
            OsString::from("--baseline"),
            repository.baseline.as_os_str().to_owned(),
            OsString::from("--candidate"),
            repository.catalog.as_os_str().to_owned(),
        ]))
        .output()
        .unwrap();
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stdout)
    );
    assert_eq!(output_json(&check)["command"], "schema.check");

    let conformance = command()
        .args(args([
            OsString::from("schema"),
            OsString::from("conformance"),
            OsString::from("--catalog"),
            repository.catalog.as_os_str().to_owned(),
            OsString::from("--fixtures"),
            repository
                .root
                .join("conformance/manifest.json")
                .into_os_string(),
        ]))
        .output()
        .unwrap();
    assert!(
        conformance.status.success(),
        "{}",
        String::from_utf8_lossy(&conformance.stdout)
    );
    assert_eq!(output_json(&conformance)["command"], "schema.conformance");

    let view = command()
        .args(args([
            OsString::from("schema"),
            OsString::from("view"),
            OsString::from("--catalog"),
            repository.catalog.as_os_str().to_owned(),
            OsString::from("--schema"),
            OsString::from("graph"),
        ]))
        .output()
        .unwrap();
    assert!(
        view.status.success(),
        "{}",
        String::from_utf8_lossy(&view.stdout)
    );
    assert_eq!(output_json(&view)["command"], "schema.view");

    let migration = migration_repository();
    let migrated = run_migration(&migration, &migration.output);
    assert!(
        migrated.status.success(),
        "{}",
        String::from_utf8_lossy(&migrated.stdout)
    );
    let value = output_json(&migrated);
    assert_eq!(value["command"], "schema.migrate");
    let migrated_document =
        serde_json::from_slice::<Value>(&fs::read(&migration.output).unwrap()).unwrap();
    assert_eq!(migrated_document["apiVersion"], "p50.dev/graph/v2");
    let target_schema: Value = serde_json::from_slice(
        &fs::read(migration.root.join("schemas/graph.schema.json")).unwrap(),
    )
    .unwrap();
    let target_id = target_schema["$id"].as_str().unwrap().to_owned();
    let target_validator =
        OfflineSchemaSet::compile(BTreeMap::from([(target_id.clone(), target_schema)])).unwrap();
    assert!(
        target_validator
            .validate(&target_id, &migrated_document, "migration-output")
            .is_empty()
    );
    assert_eq!(
        value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["command", "data", "diagnostics", "ok"])
    );
    assert_eq!(
        value["data"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            "fromVersion",
            "outputDigest",
            "schema",
            "sourceDigest",
            "toVersion",
        ])
    );
    assert_eq!(value["diagnostics"], json!([]));
    let stdout = String::from_utf8_lossy(&migrated.stdout);
    assert!(!stdout.contains("do-not-echo"));
    assert!(!stdout.contains("apiVersion"));
}

#[test]
fn checked_in_catalog_reports_the_additive_evolutions_as_one_minor_step() {
    // 1.1.0, and a THIRD evolution does not move it. Three independent minors stand between the
    // frozen 1.0.0 and the live set -- execution-accounting-receipt added whole, graph-signal's
    // optional addressing, and context-provenance added whole (#1065) -- and the reported version is one transition from the baseline, sized by
    // the cumulative impact, never one bump per evolution: `expected_version(1.0.0, Minor)` is
    // 1.1.0 in core/schema-evolution/src/release.rs.
    //
    // The name said `as_1_2_0` and the assertion agreed with it. Both were a guard certifying the
    // defect the release gate catches: `graphhelm schema check` answers GHC004_SEMVER_MISMATCH on
    // 1.2.0. What DOES move this number is publishing a new frozen baseline -- and the by-name
    // ledger in core/schema-evolution/tests/baseline_origin.rs is where a new evolution is named.
    let catalog = repository_root().join("schemas/catalog.json");
    let output = run_catalog(&catalog);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let value = output_json(&output);
    assert_eq!(value["data"]["releaseVersion"], "1.1.0");
    assert_eq!(value["data"]["schemaCount"], 22);
}

#[test]
fn compact_and_pretty_modes_each_emit_exactly_one_json_document() {
    let repository = repository_with_nine_schemas();
    let compact = run_catalog(&repository.catalog);
    assert!(compact.status.success());
    assert_eq!(String::from_utf8_lossy(&compact.stdout).lines().count(), 1);
    output_json(&compact);

    let pretty = command()
        .args(args([
            OsString::from("--pretty"),
            OsString::from("schema"),
            OsString::from("catalog"),
            OsString::from("--catalog"),
            repository.catalog.as_os_str().to_owned(),
        ]))
        .output()
        .unwrap();
    assert!(pretty.status.success());
    assert!(String::from_utf8_lossy(&pretty.stdout).lines().count() > 1);
    output_json(&pretty);
}

fn mutate_catalog_path(repository: &Repository, path: String) {
    let mut catalog: Value =
        serde_json::from_slice(&fs::read(&repository.catalog).unwrap()).unwrap();
    catalog["schemas"]["graph"]["path"] = Value::String(path);
    write_json(&repository.catalog, &catalog);
}

#[test]
fn unsafe_catalog_resource_paths_are_domain_failures() {
    for path in [
        "schemas/../secret.schema.json".to_owned(),
        "conformance/not-a-schema.json".to_owned(),
        "https://example.invalid/graph.schema.json".to_owned(),
        std::env::current_dir()
            .unwrap()
            .join("graph.schema.json")
            .to_string_lossy()
            .into_owned(),
    ] {
        let repository = repository_with_nine_schemas();
        mutate_catalog_path(&repository, path);
        let output = run_catalog(&repository.catalog);
        assert_eq!(output.status.code(), Some(2));
        assert_eq!(
            output_json(&output)["diagnostics"][0]["code"],
            "GHC001_CATALOG_INVALID"
        );
    }
}

#[test]
fn cross_package_catalog_path_is_rejected_before_resource_io() {
    let repository = repository_with_nine_schemas();
    mutate_catalog_path(
        &repository,
        "schemas/releases/1.0.0/graph.schema.json".into(),
    );
    fs::remove_file(
        repository
            .root
            .join("schemas/releases/1.0.0/graph.schema.json"),
    )
    .unwrap();

    let output = run_catalog(&repository.catalog);
    assert_eq!(output.status.code(), Some(2));
    let value = output_json(&output);
    assert_eq!(value["diagnostics"][0]["code"], "GHC001_CATALOG_INVALID");
    assert_eq!(value["diagnostics"][0]["path"], "/schemas/graph/path");
}

#[test]
fn remote_schema_references_are_rejected_offline() {
    let repository = repository_with_nine_schemas();
    let path = repository.root.join("schemas/graph.schema.json");
    let mut document: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    document["$ref"] = json!("https://example.invalid/secret.schema.json");
    write_json(&path, &document);
    let mut catalog: Value =
        serde_json::from_slice(&fs::read(&repository.catalog).unwrap()).unwrap();
    catalog["schemas"]["graph"]["sha256"] = json!(schema_digest(&document).unwrap().as_str());
    write_json(&repository.catalog, &catalog);

    let output = run_catalog(&repository.catalog);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(output_json(&output)["command"], "schema.catalog");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(!text.contains("example.invalid"));
    assert!(!text.contains("secret.schema.json"));
}

#[test]
fn oversized_resources_fail_before_json_parsing() {
    let repository = repository_with_nine_schemas();
    fs::write(
        repository.root.join("schemas/graph.schema.json"),
        vec![b' '; MAX_FILE_BYTES + 1],
    )
    .unwrap();
    let output = run_catalog(&repository.catalog);
    assert_eq!(output.status.code(), Some(4));
    assert_eq!(
        output_json(&output)["diagnostics"][0]["code"],
        "GHI001_INTERNAL"
    );
}

#[test]
fn aggregate_resource_budget_is_enforced_before_the_next_parse() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let padding = "x".repeat(MAX_FILE_BYTES - 256 * 1024);
    let documents = SCHEMAS
        .into_iter()
        .map(|name| {
            let mut document = protocol_schema(name, "1.0.0");
            document["description"] = json!(padding.clone());
            (name.to_owned(), document)
        })
        .collect::<BTreeMap<_, _>>();
    let catalog = write_catalog(root, "schemas/catalog.json", "1.0.0", &documents);

    let output = run_catalog(&catalog);
    assert_eq!(output.status.code(), Some(4));
    assert_eq!(output_json(&output)["command"], "schema.catalog");
}

#[test]
fn catalog_schema_count_is_rejected_before_any_resource_read() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fs::create_dir_all(root.join("schemas")).unwrap();
    let schemas = (0..=MAX_SCHEMAS)
        .map(|index| {
            let name = format!("schema{index:03}");
            (
                name.clone(),
                json!({
                    "id": format!("https://p50.dev/schemas/{name}.schema.json"),
                    "documentVersion": "1.0.0",
                    "path": format!("schemas/{name}.schema.json"),
                    "sha256": "sha256:0000000000000000000000000000000000000000000000000000000000000000"
                }),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    let catalog = root.join("schemas/catalog.json");
    write_json(
        &catalog,
        &json!({
            "formatVersion": 1,
            "releaseVersion": "1.0.0",
            "schemas": schemas
        }),
    );

    let output = run_catalog(&catalog);
    assert_eq!(output.status.code(), Some(2));
    let value = output_json(&output);
    assert_eq!(value["diagnostics"][0]["code"], "GHC001_CATALOG_INVALID");
    assert_eq!(value["diagnostics"][0]["path"], "/schemas");
}

#[test]
fn migration_evidence_directory_entries_are_bounded() {
    let repository = repository_with_nine_schemas();
    let migrations = repository.root.join("schemas/migrations");
    for index in 0..=MAX_CONFORMANCE_CASES {
        fs::create_dir(migrations.join(format!("empty-{index:04}"))).unwrap();
    }
    let output = command()
        .args(args([
            OsString::from("schema"),
            OsString::from("check"),
            OsString::from("--baseline"),
            repository.baseline.as_os_str().to_owned(),
            OsString::from("--candidate"),
            repository.catalog.as_os_str().to_owned(),
        ]))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(4));
    assert_eq!(output_json(&output)["command"], "schema.check");
}

#[test]
fn io_failures_redact_home_paths_temp_names_and_payloads() {
    let repository = repository_with_nine_schemas();
    let missing = repository.root.join("private-home-secret-catalog.json");
    let output = run_catalog(&missing);
    assert_eq!(output.status.code(), Some(4));
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(!text.contains(&missing.to_string_lossy().to_string()));
    assert!(!text.contains("private-home-secret-catalog"));
    assert!(
        !text.contains(
            repository
                .root
                .file_name()
                .unwrap()
                .to_string_lossy()
                .as_ref()
        )
    );
}

#[test]
fn windows_symlink_skip_is_limited_to_privilege_error_1314() {
    assert!(windows_symlink_creation_is_unavailable(
        &std::io::Error::from_raw_os_error(1314)
    ));
    assert!(!windows_symlink_creation_is_unavailable(
        &std::io::Error::from_raw_os_error(5)
    ));
}

#[test]
fn symlinked_catalog_resource_cannot_escape_repository() {
    let repository = repository_with_nine_schemas();
    let outside = tempfile::tempdir().unwrap();
    let outside_schema = outside.path().join("graph.schema.json");
    fs::copy(
        repository.root.join("schemas/graph.schema.json"),
        &outside_schema,
    )
    .unwrap();
    let inside_schema = repository.root.join("schemas/graph.schema.json");
    fs::remove_file(&inside_schema).unwrap();

    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside_schema, &inside_schema).unwrap();
    #[cfg(windows)]
    if let Err(error) = std::os::windows::fs::symlink_file(&outside_schema, &inside_schema) {
        if windows_symlink_creation_is_unavailable(&error) {
            eprintln!("skipped: Windows symlink creation unavailable: {error}");
            return;
        }
        panic!("Windows symlink creation failed unexpectedly: {error}");
    }

    let output = run_catalog(&repository.catalog);
    assert_eq!(output.status.code(), Some(4));
    assert_eq!(output_json(&output)["command"], "schema.catalog");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(!text.contains(outside.path().to_string_lossy().as_ref()));
}

#[test]
fn migration_refuses_same_or_existing_output_without_replacing_bytes() {
    let repository = migration_repository();
    let same = run_migration(&repository, &repository.input);
    assert_eq!(same.status.code(), Some(2));
    assert_eq!(output_json(&same)["command"], "schema.migrate");
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(&repository.input).unwrap()).unwrap()["apiVersion"],
        "p50.dev/graph/v1"
    );

    fs::write(&repository.output, b"sentinel").unwrap();
    let existing = run_migration(&repository, &repository.output);
    assert_eq!(existing.status.code(), Some(2));
    assert_eq!(output_json(&existing)["command"], "schema.migrate");
    assert_eq!(fs::read(&repository.output).unwrap(), b"sentinel");
}

#[test]
fn migration_refuses_a_broken_output_symlink_without_replacing_it() {
    let repository = migration_repository();
    let absent_target = repository.root.join("absent-target.json");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&absent_target, &repository.output).unwrap();
    #[cfg(windows)]
    if let Err(error) = std::os::windows::fs::symlink_file(&absent_target, &repository.output) {
        if windows_symlink_creation_is_unavailable(&error) {
            eprintln!("skipped: Windows symlink creation unavailable: {error}");
            return;
        }
        panic!("Windows symlink creation failed unexpectedly: {error}");
    }

    let output = run_migration(&repository, &repository.output);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(output_json(&output)["command"], "schema.migrate");
    assert!(fs::symlink_metadata(&repository.output).is_ok());
    assert!(!absent_target.exists());
}

#[test]
fn failed_migration_leaks_no_payload_and_leaves_no_temp_file() {
    let repository = migration_repository();
    let mut manifest: Value =
        serde_json::from_slice(&fs::read(&repository.migration).unwrap()).unwrap();
    manifest["operations"][0]["value"] = json!("TOP-SECRET-WRONG-VERSION");
    write_json(&repository.migration, &manifest);

    let output = run_migration(&repository, &repository.output);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(output_json(&output)["command"], "schema.migrate");
    assert!(!repository.output.exists());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("TOP-SECRET"));
    assert!(!stdout.contains("do-not-echo"));
    let leftovers = fs::read_dir(&repository.root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains("graphhelm-migrate"))
        .collect::<Vec<_>>();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn unsupported_manifest_versions_are_domain_failures_before_snapshot_io() {
    let repository = migration_repository();
    let mut manifest: Value =
        serde_json::from_slice(&fs::read(&repository.migration).unwrap()).unwrap();
    manifest["fromVersion"] = json!("3.0.0");
    write_json(&repository.migration, &manifest);

    let output = run_migration(&repository, &repository.output);
    assert_eq!(output.status.code(), Some(2));
    let value = output_json(&output);
    assert_eq!(value["command"], "schema.migrate");
    assert_eq!(
        value["diagnostics"][0]["code"],
        "GHM001_MIGRATION_UNSUPPORTED"
    );
    assert!(!repository.output.exists());
}

#[test]
fn target_catalog_mismatch_is_rejected_before_source_snapshot_and_input_io() {
    let repository = migration_repository();
    let mut manifest: Value =
        serde_json::from_slice(&fs::read(&repository.migration).unwrap()).unwrap();
    manifest["targetSchemaHash"] =
        json!("sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff");
    write_json(&repository.migration, &manifest);
    fs::remove_dir_all(repository.root.join("schemas/releases/1.0.0")).unwrap();
    fs::remove_file(&repository.input).unwrap();

    let output = run_migration(&repository, &repository.output);
    assert_eq!(output.status.code(), Some(2));
    let value = output_json(&output);
    assert_eq!(value["command"], "schema.migrate");
    assert_eq!(
        value["diagnostics"][0]["code"],
        "GHM002_SCHEMA_HASH_MISMATCH"
    );
    assert_eq!(value["diagnostics"][0]["path"], "/targetSchemaHash");
    assert!(!repository.output.exists());
    assert!(fs::read_dir(&repository.root).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("graphhelm-migrate")
    }));
}

#[test]
fn pretty_migration_output_cannot_exceed_the_file_bound() {
    let repository = migration_repository();
    let document = json!({
        "apiVersion": "p50.dev/graph/v1",
        "items": vec![0_u8; 650_000]
    });
    let compact = serde_json::to_vec(&document).unwrap();
    assert!(compact.len() < MAX_FILE_BYTES);
    assert!(serde_json::to_vec_pretty(&document).unwrap().len() > MAX_FILE_BYTES);
    fs::write(&repository.input, compact).unwrap();

    let output = run_migration(&repository, &repository.output);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(output_json(&output)["command"], "schema.migrate");
    assert!(!repository.output.exists());
    assert!(fs::read_dir(&repository.root).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("graphhelm-migrate")
    }));
}

#[test]
fn check_builds_exact_breaking_release_evidence_from_confined_files() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let baseline_document = migration_schema("1.0.0", "p50.dev/graph/v1");
    let mut candidate_document = migration_schema("2.0.0", "p50.dev/graph/v2");
    candidate_document["required"] = json!(["apiVersion", "enabled"]);
    candidate_document["properties"]["enabled"] = json!({"type": "boolean"});
    let baseline = write_catalog(
        root,
        "schemas/releases/1.0.0/catalog.json",
        "1.0.0",
        &BTreeMap::from([("graph".into(), baseline_document.clone())]),
    );
    let candidate = write_catalog(
        root,
        "schemas/catalog.json",
        "2.0.0",
        &BTreeMap::from([("graph".into(), candidate_document.clone())]),
    );
    let migration_path = root.join("schemas/migrations/graph/1.0.0--2.0.0.json");
    let valid_migration = json!({
        "formatVersion": 1,
        "schema": "graph",
        "fromVersion": "1.0.0",
        "toVersion": "2.0.0",
        "sourceSchemaHash": schema_digest(&baseline_document).unwrap().as_str(),
        "targetSchemaHash": schema_digest(&candidate_document).unwrap().as_str(),
        "operations": [
            {"op": "replace", "path": "/apiVersion", "value": "p50.dev/graph/v2"},
            {"op": "add", "path": "/enabled", "value": true}
        ]
    });
    write_json(&migration_path, &valid_migration);
    write_json(
        &root.join("conformance/manifest.json"),
        &json!({
            "formatVersion": 1,
            "cases": [{
                "id": "release.graph.breaking",
                "kind": "release",
                "input": "conformance/after.json",
                "comparison": "conformance/before-after.json",
                "schema": "graph",
                "fromVersion": "1.0.0",
                "toVersion": "2.0.0",
                "expect": {"ok": true, "codes": []}
            }]
        }),
    );
    fs::write(
        root.join("schemas/CHANGELOG.md"),
        "## [2.0.0]\n- BREAKING graph: requires enabled after migration\n",
    )
    .unwrap();

    let run = || {
        command()
            .args(args([
                OsString::from("schema"),
                OsString::from("check"),
                OsString::from("--baseline"),
                baseline.as_os_str().to_owned(),
                OsString::from("--candidate"),
                candidate.as_os_str().to_owned(),
            ]))
            .output()
            .unwrap()
    };

    let missing_fixtures = run();
    assert_eq!(missing_fixtures.status.code(), Some(4));
    assert_eq!(output_json(&missing_fixtures)["command"], "schema.check");

    write_json(
        &root.join("conformance/after.json"),
        &json!({"apiVersion": "p50.dev/graph/v2", "enabled": true}),
    );
    write_json(
        &root.join("conformance/before-after.json"),
        &json!({"apiVersion": "p50.dev/graph/v1"}),
    );
    write_json(
        &root.join("conformance/after.json"),
        &json!({"apiVersion": "p50.dev/graph/v2", "enabled": false}),
    );
    let unrelated_after = run();
    assert_eq!(unrelated_after.status.code(), Some(2));
    assert!(
        output_json(&unrelated_after)["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["path"] == "/evidence/fixturePairs/graph")
    );
    write_json(
        &root.join("conformance/after.json"),
        &json!({"apiVersion": "p50.dev/graph/v2", "enabled": true}),
    );
    fs::write(
        root.join("schemas/CHANGELOG.md"),
        "## [2.0.0]\n## [Unreleased]\n- BREAKING graph: wrong section\n",
    )
    .unwrap();
    let wrong_section = run();
    assert_eq!(wrong_section.status.code(), Some(2));
    assert_eq!(output_json(&wrong_section)["command"], "schema.check");

    fs::write(
        root.join("schemas/CHANGELOG.md"),
        "## [2.0.0] - 2026-8-08\n- BREAKING graph: malformed dated section\n",
    )
    .unwrap();
    let malformed_suffix = run();
    assert_eq!(malformed_suffix.status.code(), Some(2));
    assert_eq!(output_json(&malformed_suffix)["command"], "schema.check");

    fs::write(
        root.join("schemas/CHANGELOG.md"),
        "## [2.0.0] - 2026-08-08\n- BREAKING graph: requires enabled after migration\n",
    )
    .unwrap();
    let mut wrong_digest = valid_migration.clone();
    wrong_digest["targetSchemaHash"] =
        json!("sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff");
    write_json(&migration_path, &wrong_digest);
    let mismatched_manifest = run();
    assert_eq!(mismatched_manifest.status.code(), Some(2));
    assert_eq!(output_json(&mismatched_manifest)["command"], "schema.check");

    write_json(&migration_path, &valid_migration);
    let output = run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(output_json(&output)["data"]["release"]["ok"], true);
}

fn assert_migration_manifest_is_not_credited(
    repository: &BreakingReleaseRepository,
    manifest: &Value,
) {
    write_json(&repository.migration, manifest);
    let output = run_check(repository);
    assert_eq!(output.status.code(), Some(2));
    let value = output_json(&output);
    assert_eq!(value["command"], "schema.check");
    assert!(value["diagnostics"].as_array().unwrap().iter().any(|item| {
        item["code"] == "GHC004_SEMVER_MISMATCH" && item["path"] == "/evidence/migrations/graph"
    }));
}

#[test]
fn check_does_not_credit_a_noop_migration_and_unvalidated_fixture_pair() {
    let repository = breaking_release_repository();
    let output = run_check(&repository);
    assert_eq!(output.status.code(), Some(2));
    let value = output_json(&output);
    assert!(value["diagnostics"].as_array().unwrap().iter().any(|item| {
        item["code"] == "GHC004_SEMVER_MISMATCH" && item["path"] == "/evidence/fixturePairs/graph"
    }));
}

#[test]
fn check_does_not_credit_unsupported_migration_format() {
    let repository = breaking_release_repository();
    let mut manifest = repository.valid_manifest.clone();
    manifest["formatVersion"] = json!(2);
    assert_migration_manifest_is_not_credited(&repository, &manifest);
}

#[test]
fn check_does_not_credit_migration_with_malformed_pointer() {
    let repository = breaking_release_repository();
    let mut manifest = repository.valid_manifest.clone();
    manifest["operations"] = json!([{"op": "remove", "path": "/bad~2escape"}]);
    assert_migration_manifest_is_not_credited(&repository, &manifest);
}

#[test]
fn check_does_not_credit_migration_above_operation_limit() {
    let repository = breaking_release_repository();
    let mut manifest = repository.valid_manifest.clone();
    manifest["operations"] = Value::Array(vec![
        json!({"op": "remove", "path": "/missing"});
        MAX_MIGRATION_OPERATIONS + 1
    ]);
    assert_migration_manifest_is_not_credited(&repository, &manifest);
}

#[test]
fn conformance_uses_only_declared_version_qualified_validator_registries() {
    let repository = migration_repository();
    let source = migration_schema("1.0.0", "p50.dev/graph/v1");
    let target = migration_schema("2.0.0", "p50.dev/graph/v2");
    write_json(
        &repository
            .root
            .join("conformance/validators/graph-1.0.0.schema.json"),
        &source,
    );
    write_json(
        &repository
            .root
            .join("conformance/validators/graph-2.0.0.schema.json"),
        &target,
    );
    write_json(
        &repository.root.join("conformance/migration.json"),
        &json!({
            "manifest": {
                "formatVersion": 1,
                "schema": "graph",
                "fromVersion": "1.0.0",
                "toVersion": "2.0.0",
                "sourceSchemaHash": schema_digest(&source).unwrap().as_str(),
                "targetSchemaHash": schema_digest(&target).unwrap().as_str(),
                "operations": [{
                    "op": "replace",
                    "path": "/apiVersion",
                    "value": "p50.dev/graph/v2"
                }]
            },
            "sourceCatalogHash": schema_digest(&source).unwrap().as_str(),
            "targetCatalogHash": schema_digest(&target).unwrap().as_str(),
            "input": {"apiVersion": "p50.dev/graph/v1"},
            "expect": {
                "ok": true,
                "codes": [],
                "document": {"apiVersion": "p50.dev/graph/v2"}
            }
        }),
    );
    let manifest = repository.root.join("conformance/manifest.json");
    let suite = |include_target: bool| {
        let mut validators = serde_json::Map::from_iter([(
            "graph@1.0.0".into(),
            json!(["conformance/validators/graph-1.0.0.schema.json"]),
        )]);
        if include_target {
            validators.insert(
                "graph@2.0.0".into(),
                json!(["conformance/validators/graph-2.0.0.schema.json"]),
            );
        }
        json!({
            "formatVersion": 1,
            "validatorResources": validators,
            "cases": [{
                "id": "migration.declared-validators",
                "kind": "migration",
                "input": "conformance/migration.json",
                "expect": {"ok": true, "codes": []}
            }]
        })
    };
    let run = || {
        command()
            .args(args([
                OsString::from("schema"),
                OsString::from("conformance"),
                OsString::from("--catalog"),
                repository.catalog.as_os_str().to_owned(),
                OsString::from("--fixtures"),
                manifest.as_os_str().to_owned(),
            ]))
            .output()
            .unwrap()
    };

    write_json(&manifest, &suite(false));
    let missing = run();
    assert_eq!(missing.status.code(), Some(2));
    assert_eq!(output_json(&missing)["command"], "schema.conformance");

    write_json(&manifest, &suite(true));
    let declared = run();
    assert!(
        declared.status.success(),
        "{}",
        String::from_utf8_lossy(&declared.stdout)
    );
    let value = output_json(&declared);
    assert_eq!(value["data"]["total"], 1);
    assert_eq!(value["data"]["passed"], 1);
}

#[test]
fn conformance_rejects_a_v1_root_declared_as_the_v2_validator() {
    let repository = migration_repository();
    let source = protocol_schema("graph", "1.0.0");
    let target = protocol_schema("graph", "2.0.0");
    let source_path = "conformance/validators/graph-1.0.0.schema.json";
    write_json(&repository.root.join(source_path), &source);
    write_json(
        &repository
            .root
            .join("conformance/migration-version-binding.json"),
        &json!({
            "manifest": {
                "formatVersion": 1,
                "schema": "graph",
                "fromVersion": "1.0.0",
                "toVersion": "2.0.0",
                "sourceSchemaHash": schema_digest(&source).unwrap().as_str(),
                "targetSchemaHash": schema_digest(&target).unwrap().as_str(),
                "operations": []
            },
            "sourceCatalogHash": schema_digest(&source).unwrap().as_str(),
            "targetCatalogHash": schema_digest(&target).unwrap().as_str(),
            "input": {},
            "expect": {"ok": true, "codes": [], "document": {}}
        }),
    );
    let manifest = repository.root.join("conformance/manifest.json");
    let mut suite = json!({
        "formatVersion": 1,
        "validatorResources": {
            "graph@1.0.0": [source_path],
            "graph@2.0.0": [source_path]
        },
        "cases": [{
            "id": "migration.version-binding",
            "kind": "migration",
            "input": "conformance/migration-version-binding.json",
            "expect": {"ok": true, "codes": []}
        }]
    });
    write_json(&manifest, &suite);

    let run = || {
        command()
            .args(args([
                OsString::from("schema"),
                OsString::from("conformance"),
                OsString::from("--catalog"),
                repository.catalog.as_os_str().to_owned(),
                OsString::from("--fixtures"),
                manifest.as_os_str().to_owned(),
            ]))
            .output()
            .unwrap()
    };
    let assert_binding_rejected = |output: &std::process::Output| {
        assert_eq!(output.status.code(), Some(2));
        let value = output_json(output);
        assert_eq!(value["diagnostics"][0]["code"], "GHCONF001_FIXTURE_FAILED");
        assert_eq!(
            value["diagnostics"][0]["path"],
            "/validatorResources/graph@2.0.0"
        );
    };
    assert_binding_rejected(&run());

    let dependency_path = "conformance/validators/node-2.0.0.schema.json";
    write_json(
        &repository.root.join(dependency_path),
        &protocol_schema("node", "2.0.0"),
    );
    suite["validatorResources"]["graph@2.0.0"] = json!([dependency_path]);
    write_json(&manifest, &suite);
    assert_binding_rejected(&run());

    let target_path = "conformance/validators/graph-2.0.0.schema.json";
    let duplicate_path = "conformance/validators/graph-2.0.0-copy.schema.json";
    write_json(&repository.root.join(target_path), &target);
    write_json(&repository.root.join(duplicate_path), &target);
    suite["validatorResources"]["graph@2.0.0"] = json!([duplicate_path, target_path]);
    write_json(&manifest, &suite);
    assert_binding_rejected(&run());
}

/// `schema digest` prints the SAME digest the catalog verifier recomputes (the one a
/// mismatch refuses with GHC002_HASH_MISMATCH) — known-answer against data the repo
/// already treats as truth: the committed agent schema and its sha256 in the committed
/// catalog. If the command ever hashes raw bytes instead of the canonical form, this
/// falls: the committed documents are pretty-printed, so raw bytes != canonical bytes.
#[test]
fn digest_prints_the_digest_the_catalog_verifies() {
    let root = repository_root();
    let schema_path = root.join("schemas/agent.schema.json");
    let catalog: Value =
        serde_json::from_slice(&fs::read(root.join("schemas/catalog.json")).unwrap()).unwrap();
    let expected = catalog["schemas"]["agent"]["sha256"]
        .as_str()
        .expect("the committed catalog records a sha256 for the agent schema");

    let output = command()
        .args(args([
            "schema",
            "digest",
            "--file",
            schema_path.to_str().unwrap(),
        ]))
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let value = output_json(&output);
    assert_eq!(value["ok"], true, "{value}");
    assert_eq!(value["command"], "schema.digest", "{value}");
    assert_eq!(
        value["data"]["digest"], expected,
        "the printed digest must equal the digest the catalog verifies: {value}"
    );
}

/// The digest is of the CANONICAL form: two documents with different key order and
/// whitespace but equal content answer the same digest — the property that makes the
/// command safe to cite in rituals where the file on disk was formatted by a human.
#[test]
fn digest_is_invariant_under_key_order_and_formatting() {
    let directory = TempDir::new().unwrap();
    let a = directory.path().join("a.json");
    let b = directory.path().join("b.json");
    fs::write(&a, "{\"b\":1,\"a\":{\"y\":2,\"x\":3}}").unwrap();
    fs::write(
        &b,
        "{\n  \"a\": {\n    \"x\": 3,\n    \"y\": 2\n  },\n  \"b\": 1\n}",
    )
    .unwrap();
    let digest_of = |path: &Path| {
        let output = command()
            .args(args(["schema", "digest", "--file", path.to_str().unwrap()]))
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        output_json(&output)["data"]["digest"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let left = digest_of(&a);
    let right = digest_of(&b);
    assert_eq!(
        left, right,
        "equal content must digest equally regardless of formatting"
    );
    assert!(left.starts_with("sha256:"), "{left}");
}
