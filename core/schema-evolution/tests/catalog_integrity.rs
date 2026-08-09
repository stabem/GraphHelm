use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use graphhelm_schema::OfflineSchemaSet;
use graphhelm_schema_evolution::{
    CatalogEntry, CatalogResources, MAX_FILE_BYTES, MAX_RESOURCE_BYTES, MAX_SCHEMAS, SchemaCatalog,
    canonical_view, schema_digest, validate_catalog,
};
use semver::Version;
use serde_json::{Value, json};

fn schema(id: &str, version: &str) -> Value {
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": id,
        "x-graphhelm-schema-version": version,
        "type": "object"
    })
}

fn one_schema_catalog(name: &str, version: &str) -> CatalogResources {
    let id = format!("https://p50.dev/schemas/{name}.schema.json");
    let document = schema(&id, version);
    let mut schemas = BTreeMap::new();
    schemas.insert(name.to_owned(), document.clone());
    let mut entries = BTreeMap::new();
    entries.insert(
        name.to_owned(),
        CatalogEntry {
            id,
            document_version: Version::parse(version).unwrap(),
            path: format!("schemas/{name}.schema.json"),
            sha256: schema_digest(&document).unwrap(),
        },
    );
    CatalogResources {
        catalog_source: "schemas/catalog.json".into(),
        catalog: SchemaCatalog {
            format_version: 1,
            release_version: Version::parse(version).unwrap(),
            schemas: entries,
        },
        schemas,
    }
}

fn catalog_with_equivalent_reformatted_schema() -> CatalogResources {
    let mut resources = one_schema_catalog("graph", "1.0.0");
    resources.schemas.insert(
        "graph".into(),
        serde_json::from_str(
            r#"{
              "type": "object",
              "x-graphhelm-schema-version": "1.0.0",
              "$id": "https://p50.dev/schemas/graph.schema.json",
              "$schema": "https://json-schema.org/draft/2020-12/schema"
            }"#,
        )
        .unwrap(),
    );
    resources
}

fn release_resources() -> CatalogResources {
    let documents = [
        ("agent", include_str!("../../../schemas/agent.schema.json")),
        ("claim", include_str!("../../../schemas/claim.schema.json")),
        (
            "context-capsule",
            include_str!("../../../schemas/context-capsule.schema.json"),
        ),
        ("edge", include_str!("../../../schemas/edge.schema.json")),
        (
            "extension",
            include_str!("../../../schemas/extension.schema.json"),
        ),
        (
            "graph-signal",
            include_str!("../../../schemas/graph-signal.schema.json"),
        ),
        ("graph", include_str!("../../../schemas/graph.schema.json")),
        ("node", include_str!("../../../schemas/node.schema.json")),
        (
            "policy-waiver",
            include_str!("../../../schemas/policy-waiver.schema.json"),
        ),
    ];
    let mut schemas = BTreeMap::new();
    let mut entries = BTreeMap::new();
    for (name, source) in documents {
        let mut document: Value = serde_json::from_str(source).unwrap();
        document["x-graphhelm-schema-version"] = json!("1.0.0");
        let id = document["$id"].as_str().unwrap().to_owned();
        entries.insert(
            name.to_owned(),
            CatalogEntry {
                id,
                document_version: Version::parse("1.0.0").unwrap(),
                path: format!("schemas/{name}.schema.json"),
                sha256: schema_digest(&document).unwrap(),
            },
        );
        schemas.insert(name.to_owned(), document);
    }
    CatalogResources {
        catalog_source: "schemas/catalog.json".into(),
        catalog: SchemaCatalog {
            format_version: 1,
            release_version: Version::parse("1.0.0").unwrap(),
            schemas: entries,
        },
        schemas,
    }
}

fn refresh_schema_digest(resources: &mut CatalogResources, name: &str) {
    resources.catalog.schemas.get_mut(name).unwrap().sha256 =
        schema_digest(&resources.schemas[name]).unwrap();
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[derive(Clone, Copy)]
enum RepositoryPackage {
    Current,
    Release1_0_0,
}

const REPOSITORY_SCHEMA_NAMES: [&str; 9] = [
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

impl RepositoryPackage {
    fn catalog_path(self) -> &'static str {
        match self {
            Self::Current => "schemas/catalog.json",
            Self::Release1_0_0 => "schemas/releases/1.0.0/catalog.json",
        }
    }

    fn schema_directory(self) -> &'static str {
        match self {
            Self::Current => "schemas",
            Self::Release1_0_0 => "schemas/releases/1.0.0",
        }
    }

    fn schema_path(self, name: &str) -> String {
        format!("{}/{}.schema.json", self.schema_directory(), name)
    }
}

fn package_layout_is_exact(catalog: &SchemaCatalog, package: RepositoryPackage) -> bool {
    catalog.schemas.len() == REPOSITORY_SCHEMA_NAMES.len()
        && catalog
            .schemas
            .keys()
            .map(String::as_str)
            .eq(REPOSITORY_SCHEMA_NAMES)
        && REPOSITORY_SCHEMA_NAMES
            .iter()
            .all(|name| catalog.schemas[*name].path == package.schema_path(name))
}

fn package_schema_inventory(root: &Path, package: RepositoryPackage) -> Vec<String> {
    let mut inventory = fs::read_dir(root.join(package.schema_directory()))
        .unwrap()
        .map(|entry| entry.unwrap())
        .filter(|entry| entry.file_type().unwrap().is_file())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".schema.json"))
        .collect::<Vec<_>>();
    inventory.sort();
    inventory
}

fn expected_schema_inventory() -> Vec<String> {
    let mut inventory = REPOSITORY_SCHEMA_NAMES
        .iter()
        .map(|name| format!("{name}.schema.json"))
        .collect::<Vec<_>>();
    inventory.sort();
    inventory
}

fn load_repo_catalog(package: RepositoryPackage) -> CatalogResources {
    let root = repository_root();
    let catalog_path = package.catalog_path();
    let catalog: SchemaCatalog =
        serde_json::from_slice(&fs::read(root.join(catalog_path)).unwrap()).unwrap();
    assert!(package_layout_is_exact(&catalog, package));
    assert_eq!(
        package_schema_inventory(&root, package),
        expected_schema_inventory()
    );
    let schemas = REPOSITORY_SCHEMA_NAMES
        .iter()
        .map(|name| {
            let document =
                serde_json::from_slice(&fs::read(root.join(package.schema_path(name))).unwrap())
                    .unwrap();
            ((*name).to_owned(), document)
        })
        .collect();
    CatalogResources {
        catalog_source: catalog_path.into(),
        catalog,
        schemas,
    }
}

// Prevents a root catalog from silently loading immutable release files, or an immutable release
// catalog from silently loading mutable root files.
#[test]
fn repository_package_layout_rejects_cross_package_schema_paths() {
    let mut current = load_repo_catalog(RepositoryPackage::Current);
    current.catalog.schemas.get_mut("agent").unwrap().path =
        "schemas/releases/1.0.0/agent.schema.json".into();
    let current_report = validate_catalog(&current);
    assert!(!current_report.ok);
    assert_eq!(current_report.diagnostics.len(), 1);
    assert_eq!(current_report.diagnostics[0].code, "GHC001_CATALOG_INVALID");
    assert_eq!(current_report.diagnostics[0].path, "/schemas/agent/path");

    let mut release = load_repo_catalog(RepositoryPackage::Release1_0_0);
    release.catalog.schemas.get_mut("agent").unwrap().path = "schemas/agent.schema.json".into();
    let release_report = validate_catalog(&release);
    assert!(!release_report.ok);
    assert_eq!(release_report.diagnostics.len(), 1);
    assert_eq!(release_report.diagnostics[0].code, "GHC001_CATALOG_INVALID");
    assert_eq!(release_report.diagnostics[0].path, "/schemas/agent/path");
}

#[test]
fn release_catalog_directory_must_match_its_release_version() {
    let mut release = load_repo_catalog(RepositoryPackage::Release1_0_0);
    release.catalog_source = "schemas/releases/2.0.0/catalog.json".into();
    let report = validate_catalog(&release);
    assert!(!report.ok);
    assert_eq!(report.diagnostics[0].code, "GHC001_CATALOG_INVALID");
    assert_eq!(report.diagnostics[0].path, "/releaseVersion");
}

// Prevents the mutable current package or immutable 1.0.0 snapshot from being incomplete,
// internally invalid, or semantically different from one another.
#[test]
fn checked_in_current_and_1_0_0_release_are_complete_and_equivalent() {
    let current = load_repo_catalog(RepositoryPackage::Current);
    let release = load_repo_catalog(RepositoryPackage::Release1_0_0);

    assert_eq!(current.catalog.schemas.len(), 9);
    assert!(validate_catalog(&current).ok);
    assert!(validate_catalog(&release).ok);
    assert_eq!(
        current.catalog.release_version,
        Version::parse("1.0.0").unwrap()
    );
    assert_eq!(
        release.catalog.release_version,
        current.catalog.release_version
    );
    assert_eq!(
        release.catalog.schemas.keys().collect::<Vec<_>>(),
        current.catalog.schemas.keys().collect::<Vec<_>>()
    );
    for name in current.catalog.schemas.keys() {
        let current_entry = &current.catalog.schemas[name];
        let release_entry = &release.catalog.schemas[name];
        assert_eq!(release_entry.id, current_entry.id, "{name}");
        assert_eq!(
            release_entry.document_version, current_entry.document_version,
            "{name}"
        );
        assert_eq!(release_entry.sha256, current_entry.sha256, "{name}");
        assert_eq!(release.schemas[name], current.schemas[name], "{name}");
    }
}

// Prevents the immutable release from compiling differently or enforcing different public
// conformance outcomes than the current package.
#[test]
fn current_and_1_0_0_release_enforce_identical_public_schema_contracts() {
    let root = repository_root();
    let current = load_repo_catalog(RepositoryPackage::Current);
    let release = load_repo_catalog(RepositoryPackage::Release1_0_0);
    let current_validators = OfflineSchemaSet::compile(current.schemas).unwrap();
    let release_validators = OfflineSchemaSet::compile(release.schemas).unwrap();
    let manifest: Value =
        serde_json::from_slice(&fs::read(root.join("conformance/manifest.json")).unwrap()).unwrap();
    let schema_cases = manifest["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| case["kind"] == "schema")
        .collect::<Vec<_>>();

    assert_eq!(schema_cases.len(), 18);
    for case in schema_cases {
        let name = case["schema"].as_str().unwrap();
        let input = case["input"].as_str().unwrap();
        let document: Value = serde_json::from_slice(&fs::read(root.join(input)).unwrap()).unwrap();
        let schema_id = format!("https://p50.dev/schemas/{name}.schema.json");
        let current_diagnostics = current_validators.validate(&schema_id, &document, "conformance");
        let release_diagnostics = release_validators.validate(&schema_id, &document, "conformance");
        let current_signature = current_diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.clone(), diagnostic.path.clone()))
            .collect::<Vec<_>>();
        let release_signature = release_diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.clone(), diagnostic.path.clone()))
            .collect::<Vec<_>>();
        let expected_codes = case["expect"]["codes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|code| code.as_str().unwrap())
            .collect::<Vec<_>>();
        let expected_paths = case["expect"]
            .get("paths")
            .and_then(Value::as_array)
            .map(|paths| {
                paths
                    .iter()
                    .map(|path| path.as_str().unwrap())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        assert_eq!(release_signature, current_signature, "{}", case["id"]);
        assert_eq!(
            current_diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code.as_str())
                .collect::<Vec<_>>(),
            expected_codes,
            "{}",
            case["id"]
        );
        assert_eq!(
            current_diagnostics
                .iter()
                .map(|diagnostic| diagnostic.path.as_str())
                .collect::<Vec<_>>(),
            expected_paths,
            "{}",
            case["id"]
        );
    }
}

// Prevents generated views from becoming a second source of truth or including remote resources.
#[test]
fn graph_view_is_canonical_and_lists_only_local_dependencies() {
    let view = canonical_view(&release_resources(), "graph").unwrap();
    assert_eq!(view.name, "graph");
    assert_eq!(view.schema_id, "https://p50.dev/schemas/graph.schema.json");
    assert_eq!(view.catalog_format_version, 1);
    assert_eq!(
        view.catalog_release_version,
        Version::parse("1.0.0").unwrap()
    );
    assert_eq!(view.document_version, Version::parse("1.0.0").unwrap());
    assert_eq!(
        view.sha256,
        schema_digest(&release_resources().schemas["graph"]).unwrap()
    );
    assert_eq!(
        view.dependencies,
        [
            "https://p50.dev/schemas/edge.schema.json",
            "https://p50.dev/schemas/node.schema.json",
        ]
    );
    assert!(view.schema.is_object());
}

// Prevents distinct local fragments from duplicating their shared dependency identifier.
#[test]
fn view_deduplicates_dependency_ids_across_local_fragments() {
    let mut resources = release_resources();
    resources.schemas.get_mut("graph").unwrap()["$defs"] = json!({
        "name": {"$ref": "node.schema.json#/properties/name"},
        "objective": {"$ref": "node.schema.json#/properties/objective"}
    });
    resources.catalog.schemas.get_mut("graph").unwrap().sha256 =
        schema_digest(&resources.schemas["graph"]).unwrap();

    let view = canonical_view(&resources, "graph").unwrap();
    assert_eq!(
        view.dependencies,
        [
            "https://p50.dev/schemas/edge.schema.json",
            "https://p50.dev/schemas/node.schema.json",
        ]
    );
}

// Prevents schema-valued contentSchema branches from bypassing local dependency inventory.
#[test]
fn view_collects_local_dependencies_beneath_content_schema() {
    let mut resources = release_resources();
    resources.schemas.get_mut("graph").unwrap()["contentSchema"] =
        json!({"$ref": "agent.schema.json"});
    refresh_schema_digest(&mut resources, "graph");

    let view = canonical_view(&resources, "graph").unwrap();
    assert_eq!(
        view.dependencies,
        [
            "https://p50.dev/schemas/agent.schema.json",
            "https://p50.dev/schemas/edge.schema.json",
            "https://p50.dev/schemas/node.schema.json",
        ]
    );
}

// Prevents contentSchema from bypassing the offline reference boundary.
#[test]
fn view_rejects_remote_or_unresolved_references_beneath_content_schema() {
    for reference in [
        "https://example.invalid/remote.schema.json",
        "missing.schema.json",
    ] {
        let mut resources = release_resources();
        resources.schemas.get_mut("graph").unwrap()["contentSchema"] = json!({"$ref": reference});
        refresh_schema_digest(&mut resources, "graph");

        let diagnostics = canonical_view(&resources, "graph").unwrap_err();
        assert_eq!(diagnostics.len(), 1, "{reference}");
        assert_eq!(diagnostics[0].path, "/contentSchema/$ref", "{reference}");
        assert!(!diagnostics[0].message.contains(reference), "{reference}");
    }
}

// Prevents URI-encoded JSON Pointer fragments from being treated as literal percent sequences.
#[test]
fn view_resolves_percent_decoded_json_pointer_fragments() {
    let mut resources = release_resources();
    resources.schemas.get_mut("agent").unwrap()[" "] = json!({"type": "string"});
    refresh_schema_digest(&mut resources, "agent");
    resources.schemas.get_mut("graph").unwrap()["contentSchema"] =
        json!({"$ref": "agent.schema.json#/%20"});
    refresh_schema_digest(&mut resources, "graph");

    let view = canonical_view(&resources, "graph").unwrap();
    assert!(
        view.dependencies
            .contains(&"https://p50.dev/schemas/agent.schema.json".to_owned())
    );
}

// Prevents malformed fragment encodings from being accepted when matching literal object keys.
#[test]
fn view_rejects_malformed_json_pointer_fragments() {
    for reference in [
        "agent.schema.json#/%ZZ",
        "agent.schema.json#/%FF",
        "agent.schema.json#/bad~2",
    ] {
        let mut resources = release_resources();
        resources.schemas.get_mut("agent").unwrap()["%ZZ"] = json!({"type": "string"});
        resources.schemas.get_mut("agent").unwrap()["%FF"] = json!({"type": "string"});
        resources.schemas.get_mut("agent").unwrap()["bad~2"] = json!({"type": "string"});
        refresh_schema_digest(&mut resources, "agent");
        resources.schemas.get_mut("graph").unwrap()["contentSchema"] = json!({"$ref": reference});
        refresh_schema_digest(&mut resources, "graph");

        let diagnostics = canonical_view(&resources, "graph").unwrap_err();
        assert_eq!(diagnostics.len(), 1, "{reference}");
        assert_eq!(diagnostics[0].path, "/contentSchema/$ref", "{reference}");
        assert!(
            !serde_json::to_string(&diagnostics)
                .unwrap()
                .contains(reference)
        );
    }
}

// Prevents root fragments from being rejected while retaining their local dependency identity.
#[test]
fn view_accepts_an_empty_root_fragment() {
    let mut resources = release_resources();
    resources.schemas.get_mut("graph").unwrap()["contentSchema"] =
        json!({"$ref": "agent.schema.json#"});
    refresh_schema_digest(&mut resources, "graph");

    let view = canonical_view(&resources, "graph").unwrap();
    assert!(
        view.dependencies
            .contains(&"https://p50.dev/schemas/agent.schema.json".to_owned())
    );
}

// Prevents arbitrary requested names from turning into an on-disk lookup or leaking catalog paths.
#[test]
fn view_rejects_a_schema_absent_from_the_supplied_catalog() {
    let diagnostics = canonical_view(&release_resources(), "missing").unwrap_err();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code, "GHC001_CATALOG_INVALID");
    assert_eq!(diagnostics[0].path, "/schemas");
    assert_eq!(diagnostics[0].source, "schema-evolution");
    assert!(!diagnostics[0].message.contains("catalog.json"));
}

// Prevents a view from attesting to a digest that does not match its generated schema JSON.
#[test]
fn view_rejects_a_schema_with_a_stale_catalog_digest() {
    let mut resources = release_resources();
    resources.schemas.get_mut("graph").unwrap()["description"] = json!("changed");

    let diagnostics = canonical_view(&resources, "graph").unwrap_err();
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "GHC002_HASH_MISMATCH" && diagnostic.path == "/schemas/graph/sha256"
    }));
}

// Prevents view diagnostics from disclosing the catalog source supplied by an I/O adapter.
#[test]
fn view_redacts_catalog_validation_sources() {
    for source in ["schemas/catalog.json", r"C:\private-marker\catalog.json"] {
        let mut resources = release_resources();
        resources.catalog_source = source.into();
        resources.schemas.get_mut("graph").unwrap()["description"] = json!("changed");

        let diagnostics = canonical_view(&resources, "graph").unwrap_err();
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "GHC002_HASH_MISMATCH" && diagnostic.path == "/schemas/graph/sha256"
        }));
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.source == "schema-view")
        );
        assert!(
            !serde_json::to_string(&diagnostics)
                .unwrap()
                .contains(source)
        );
    }
}

// Prevents generated views from resolving a reference outside the supplied in-memory catalog.
#[test]
fn view_rejects_remote_or_unresolved_schema_references() {
    for reference in [
        "https://example.invalid/remote.schema.json",
        "missing.schema.json",
    ] {
        let mut resources = release_resources();
        resources.schemas.get_mut("graph").unwrap()["properties"]["spec"]["properties"]["nodes"]
            ["additionalProperties"]["$ref"] = json!(reference);
        resources.catalog.schemas.get_mut("graph").unwrap().sha256 =
            schema_digest(&resources.schemas["graph"]).unwrap();

        let diagnostics = canonical_view(&resources, "graph").unwrap_err();
        assert_eq!(diagnostics.len(), 1, "{reference}");
        assert_eq!(diagnostics[0].code, "GHC001_CATALOG_INVALID", "{reference}");
        assert_eq!(
            diagnostics[0].path, "/properties/spec/properties/nodes/additionalProperties/$ref",
            "{reference}"
        );
        assert_eq!(diagnostics[0].source, "schema-evolution", "{reference}");
        assert!(!diagnostics[0].message.contains(reference), "{reference}");
    }
}

// Prevents JSON map order or resource paths from affecting the generated-only public view.
#[test]
fn view_json_is_stable_and_omits_catalog_paths() {
    let resources = release_resources();
    let first = serde_json::to_vec(&canonical_view(&resources, "graph").unwrap()).unwrap();
    let second = serde_json::to_vec(&canonical_view(&resources, "graph").unwrap()).unwrap();
    assert_eq!(first, second);

    let view: Value = serde_json::from_slice(&first).unwrap();
    assert!(view["schema"].is_object());
    assert!(view.get("path").is_none());
    assert!(view.get("catalogSource").is_none());
}

// Prevents an entry from naming a different schema than the resource it attests to.
#[test]
fn catalog_rejects_key_id_version_and_digest_disagreement() {
    let mut resources = one_schema_catalog("graph", "1.0.0");
    resources.catalog.schemas.get_mut("graph").unwrap().id =
        "https://p50.dev/schemas/node.schema.json".into();
    let report = validate_catalog(&resources);
    assert_eq!(report.diagnostics[0].code, "GHC001_CATALOG_INVALID");
    assert_eq!(report.diagnostics[0].path, "/schemas/graph/id");
}

// Prevents a fully coherent node schema from being attested under the graph key.
#[test]
fn catalog_rejects_schema_identity_swapped_under_a_different_key() {
    let node_id = "https://p50.dev/schemas/node.schema.json";
    for path in [
        "schemas/node.schema.json",
        "schemas/releases/1.0.0/node.schema.json",
    ] {
        let mut resources = one_schema_catalog("graph", "1.0.0");
        let node = schema(node_id, "1.0.0");
        let entry = resources.catalog.schemas.get_mut("graph").unwrap();
        entry.id = node_id.into();
        entry.path = path.into();
        entry.sha256 = schema_digest(&node).unwrap();
        resources.schemas.insert("graph".into(), node);

        let report = validate_catalog(&resources);
        assert_eq!(
            report.diagnostics[0].code, "GHC001_CATALOG_INVALID",
            "{path}"
        );
        assert_eq!(report.diagnostics[0].path, "/schemas/graph/id", "{path}");
    }
}

// Prevents whitespace-only rewrites from invalidating canonical catalog digests.
#[test]
fn formatting_only_change_keeps_catalog_hash_valid() {
    let resources = catalog_with_equivalent_reformatted_schema();
    assert!(validate_catalog(&resources).ok);
}

// Prevents parsers from accepting an incompatible catalog wire format.
#[test]
fn catalog_rejects_unsupported_format_version() {
    let mut resources = one_schema_catalog("graph", "1.0.0");
    resources.catalog.format_version = 2;
    let report = validate_catalog(&resources);
    assert_eq!(report.diagnostics[0].code, "GHC001_CATALOG_INVALID");
    assert_eq!(report.diagnostics[0].path, "/formatVersion");
}

// Prevents an absolute caller path from leaking through catalog diagnostics.
#[test]
fn catalog_diagnostics_redact_unsafe_catalog_source() {
    let mut resources = one_schema_catalog("graph", "1.0.0");
    resources.catalog_source = r"C:\Users\gabri\secret\catalog.json".into();
    resources.catalog.format_version = 2;
    let report = validate_catalog(&resources);
    assert_eq!(report.diagnostics[0].source, "schema-evolution");
    assert!(!report.diagnostics[0].message.contains("gabri"));
}

// Prevents path-unsafe or ambiguous schema names from reaching later processing.
#[test]
fn catalog_rejects_invalid_schema_name() {
    let mut resources = one_schema_catalog("graph", "1.0.0");
    let entry = resources.catalog.schemas.remove("graph").unwrap();
    let document = resources.schemas.remove("graph").unwrap();
    resources
        .catalog
        .schemas
        .insert("Graph/../node".into(), entry);
    resources.schemas.insert("Graph/../node".into(), document);
    let report = validate_catalog(&resources);
    assert_eq!(report.diagnostics[0].code, "GHC001_CATALOG_INVALID");
    assert_eq!(report.diagnostics[0].path, "/schemas/Graph~1..~1node");
}

// Prevents catalog paths from escaping the checked-in schemas tree or becoming remote locators.
#[test]
fn catalog_rejects_non_normalized_schema_paths() {
    for path in [
        "/schemas/graph.schema.json",
        "C:/schemas/graph.schema.json",
        "https://example.com/graph.schema.json",
        "schemas/../graph.schema.json",
    ] {
        let mut resources = one_schema_catalog("graph", "1.0.0");
        resources.catalog.schemas.get_mut("graph").unwrap().path = path.into();
        let report = validate_catalog(&resources);
        assert_eq!(
            report.diagnostics[0].code, "GHC001_CATALOG_INVALID",
            "{path}"
        );
        assert_eq!(report.diagnostics[0].path, "/schemas/graph/path", "{path}");
    }
}

// Prevents schemas without the required version extension from joining a catalog.
#[test]
fn catalog_rejects_missing_document_version_extension() {
    let mut resources = one_schema_catalog("graph", "1.0.0");
    resources
        .schemas
        .get_mut("graph")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .remove("x-graphhelm-schema-version");
    let report = validate_catalog(&resources);
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "GHC001_CATALOG_INVALID"
            && diagnostic.path == "/schemas/graph/x-graphhelm-schema-version"
    }));
}

// Prevents unbounded catalog cardinality.
#[test]
fn catalog_rejects_more_than_maximum_entries() {
    let mut resources = CatalogResources {
        catalog_source: "schemas/catalog.json".into(),
        catalog: SchemaCatalog {
            format_version: 1,
            release_version: Version::parse("1.0.0").unwrap(),
            schemas: BTreeMap::new(),
        },
        schemas: BTreeMap::new(),
    };
    for index in 0..=MAX_SCHEMAS {
        let name = format!("schema-{index}");
        let id = format!("https://p50.dev/schemas/{name}.schema.json");
        let document = schema(&id, "1.0.0");
        resources.catalog.schemas.insert(
            name.clone(),
            CatalogEntry {
                id,
                document_version: Version::parse("1.0.0").unwrap(),
                path: format!("schemas/{name}.schema.json"),
                sha256: schema_digest(&document).unwrap(),
            },
        );
        resources.schemas.insert(name, document);
    }
    let report = validate_catalog(&resources);
    assert_eq!(report.diagnostics[0].code, "GHC001_CATALOG_INVALID");
    assert_eq!(report.diagnostics[0].path, "/schemas");
}

// Prevents two catalog entries from claiming the same schema identity.
#[test]
fn catalog_rejects_duplicate_schema_ids() {
    let mut resources = one_schema_catalog("graph", "1.0.0");
    let document = resources.schemas["graph"].clone();
    let entry = resources.catalog.schemas["graph"].clone();
    resources.catalog.schemas.insert("copy".into(), entry);
    resources.schemas.insert("copy".into(), document);
    let report = validate_catalog(&resources);
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "GHC001_CATALOG_INVALID"
            && diagnostic.path == "/schemas/graph/id"
            && diagnostic.message == "schema id must be unique"
    }));
}

// Prevents malformed digests from becoming a weak integrity boundary during catalog parsing.
#[test]
fn catalog_deserialization_rejects_malformed_digest() {
    let resources = one_schema_catalog("graph", "1.0.0");
    let mut catalog = serde_json::to_value(resources.catalog).unwrap();
    catalog["schemas"]["graph"]["sha256"] = Value::String("sha256:NOT-LOWERCASE".into());
    assert!(serde_json::from_value::<SchemaCatalog>(catalog).is_err());
}

// Prevents catalog and schema payloads from exceeding bounded in-memory processing limits.
#[test]
fn catalog_rejects_catalog_and_resource_size_overflow() {
    let id = format!("https://p50.dev/schemas/{}", "x".repeat(MAX_FILE_BYTES));
    let document = schema(&id, "1.0.0");
    let catalog = CatalogResources {
        catalog_source: "schemas/catalog.json".into(),
        catalog: SchemaCatalog {
            format_version: 1,
            release_version: Version::parse("1.0.0").unwrap(),
            schemas: BTreeMap::from([(
                "graph".into(),
                CatalogEntry {
                    id,
                    document_version: Version::parse("1.0.0").unwrap(),
                    path: "schemas/graph.schema.json".into(),
                    sha256: schema_digest(&document).unwrap(),
                },
            )]),
        },
        schemas: BTreeMap::from([("graph".into(), document)]),
    };
    let catalog_report = validate_catalog(&catalog);
    assert_eq!(catalog_report.diagnostics[0].code, "GHC001_CATALOG_INVALID");

    let padding = "x".repeat(MAX_RESOURCE_BYTES / 9);
    let mut entries = BTreeMap::new();
    let mut documents = BTreeMap::new();
    for index in 0..9 {
        let name = format!("schema-{index}");
        let id = format!("https://p50.dev/schemas/{name}.schema.json");
        let mut document = schema(&id, "1.0.0");
        document["description"] = Value::String(padding.clone());
        entries.insert(
            name.clone(),
            CatalogEntry {
                id,
                document_version: Version::parse("1.0.0").unwrap(),
                path: format!("schemas/{name}.schema.json"),
                sha256: schema_digest(&document).unwrap(),
            },
        );
        documents.insert(name, document);
    }
    let resources = CatalogResources {
        catalog_source: "schemas/catalog.json".into(),
        catalog: SchemaCatalog {
            format_version: 1,
            release_version: Version::parse("1.0.0").unwrap(),
            schemas: entries,
        },
        schemas: documents,
    };
    let resource_report = validate_catalog(&resources);
    assert_eq!(
        resource_report.diagnostics[0].code,
        "GHC001_CATALOG_INVALID"
    );
    assert_eq!(resource_report.diagnostics[0].path, "/schemas");
}

#[test]
fn catalog_rejects_one_schema_above_four_mebibytes_before_digest_work() {
    let mut resources = one_schema_catalog("graph", "1.0.0");
    resources.schemas.get_mut("graph").unwrap()["description"] =
        Value::String("x".repeat(MAX_FILE_BYTES));

    let report = validate_catalog(&resources);
    assert!(!report.ok);
    assert_eq!(report.diagnostics.len(), 1);
    assert_eq!(report.diagnostics[0].code, "GHC001_CATALOG_INVALID");
    assert_eq!(report.diagnostics[0].path, "/schemas/graph/bytes");
}
