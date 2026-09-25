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

// Prevents a superseded pre-release package or partial fixture inventory from becoming a second
// persistence baseline.
#[test]
fn repository_has_one_safe_initial_release() {
    let root = repository_root();
    let catalog: SchemaCatalog =
        serde_json::from_slice(&fs::read(root.join("schemas/catalog.json")).unwrap()).unwrap();
    let manifest: Value =
        serde_json::from_slice(&fs::read(root.join("conformance/manifest.json")).unwrap()).unwrap();
    let declared_resources = manifest["cases"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|case| [case.get("input"), case.get("comparison")])
        .flatten()
        .map(|path| path.as_str().unwrap())
        .chain(
            manifest["validatorResources"]
                .as_object()
                .unwrap()
                .values()
                .flat_map(|paths| paths.as_array().unwrap())
                .map(|path| path.as_str().unwrap()),
        )
        .collect::<std::collections::BTreeSet<_>>();

    // 1.1.0, and the number does NOT count evolutions. Three independent minors stand between the
    // frozen 1.0.0 and the live set -- execution-accounting-receipt added whole, graph-signal's
    // optional addressing, and context-provenance added whole (#1065) -- and all land in ONE
    // transition, because the release rule measures a
    // single step from the frozen baseline sized by the CUMULATIVE impact:
    // `expected_version(1.0.0, Minor) == 1.1.0` in core/schema-evolution/src/release.rs.
    //
    // This assertion said 1.2.0, stacking the second minor on top of the first. The refutation is
    // two lines below, written by the same comment: `schemas/releases/1.1.0` does not exist. There
    // is no 1.1.0 to stack on. A minor accumulates against the last RELEASE, not against the last
    // change, and `graphhelm schema check --baseline schemas/releases/1.0.0/catalog.json` refuses
    // 1.2.0 with GHC004_SEMVER_MISMATCH -- which is how this was found.
    assert_eq!(catalog.release_version, Version::new(1, 1, 0));
    assert_eq!(catalog.schemas.len(), CURRENT_SCHEMA_NAMES.len());
    assert!(!root.join("schemas/releases/1.1.0").exists());
    assert!(!root.join("schemas/releases/1.2.0").exists());
    // 59 cases / 61 resources: #1086 added the context-provenance `root` refusal, #1049 the
    // node crew acceptance.
    assert_eq!(manifest["cases"].as_array().unwrap().len(), 59);
    assert_eq!(declared_resources.len(), 61);
}

#[test]
fn current_catalog_adds_execution_accounting_without_backfilling_release_1_0_0() {
    let root = repository_root();
    let current: SchemaCatalog =
        serde_json::from_slice(&fs::read(root.join("schemas/catalog.json")).unwrap()).unwrap();
    let release: SchemaCatalog = serde_json::from_slice(
        &fs::read(root.join("schemas/releases/1.0.0/catalog.json")).unwrap(),
    )
    .unwrap();

    // 1.1.0: one transition from the frozen baseline, sized by cumulative impact -- see the
    // reasoning at the release_version assertion above.
    assert_eq!(current.release_version, Version::new(1, 1, 0));
    assert_eq!(current.schemas.len(), CURRENT_SCHEMA_NAMES.len());
    assert!(current.schemas.contains_key("execution-accounting-receipt"));
    assert_eq!(release.release_version, Version::new(1, 0, 0));
    assert_eq!(release.schemas.len(), 15);
    assert!(!release.schemas.contains_key("execution-accounting-receipt"));
    assert!(
        !root
            .join("schemas/releases/1.0.0/execution-accounting-receipt.schema.json")
            .exists()
    );
}

#[test]
fn execution_accounting_schema_accepts_the_receipt_and_rejects_unbounded_or_extra_data() {
    const SCHEMA_ID: &str = "https://p50.dev/schemas/execution-accounting-receipt.schema.json";
    let root = repository_root();
    let schema: Value = serde_json::from_slice(
        &fs::read(root.join("schemas/execution-accounting-receipt.schema.json")).unwrap(),
    )
    .unwrap();
    let validators = OfflineSchemaSet::compile(BTreeMap::from([(SCHEMA_ID.to_owned(), schema)]))
        .expect("the new schema compiles offline");
    let field =
        |name: &str, value: Option<u64>, provenance: &str, producer: Option<&str>, note: &str| {
            json!({
                "name": name,
                "value": value,
                "provenance": provenance,
                "producer": producer,
                "note": note
            })
        };
    let binding = json!({
        "bindingKind": "execution_started_event",
        "schemaId": "https://p50.dev/schemas/event-envelope.schema.json",
        "schemaVersion": "1.0.0",
        "eventId": "execution-start-event-1",
        "eventHash": format!("sha256:{}", "a".repeat(64)),
        "scope": {
            "workspaceId": "workspace-accounting",
            "projectId": "project-accounting",
            "executionId": "execution-accounting-fixed"
        },
        "producerActor": {"type": "system", "id": "runtime-driver"}
    });
    let mut receipt = json!({
        "schemaVersion": "1.0.0",
        "executionBinding": binding,
        "fields": [
            field("orientation_tokens", None, "unavailable", None, "not measured by this runtime slice"),
            field("zero_result_queries", None, "unavailable", None, "not measured by this runtime slice"),
            field("retrieval_pages", None, "unavailable", None, "not measured by this runtime slice"),
            field("retrieval_retries", None, "unavailable", None, "not measured by this runtime slice"),
            field("retrieval_fallbacks", None, "unavailable", None, "not measured by this runtime slice"),
            field("summary_tokens", None, "unavailable", None, "not measured by this runtime slice"),
            field("provider_reported_input_tokens", Some(12), "measured", Some("model_gateway"), "raw WorkSummary.input_tokens reported by the provider boundary; may exclude cache read and cache creation tokens"),
            field("provider_total_input_tokens", None, "unavailable", None, "provider total input is unavailable because WorkSummary may exclude cache read and cache creation tokens"),
            field("compiled_input_tokens", None, "unavailable", None, "not measured by this runtime slice"),
            field("output_tokens", Some(5), "measured", Some("model_gateway"), ""),
            field("formatting_tokens", None, "unavailable", None, "not measured by this runtime slice"),
            field("index_cost_cold", None, "unavailable", None, "not measured by this runtime slice"),
            field("index_cost_amortized", None, "unavailable", None, "not measured by this runtime slice")
        ]
    });
    assert!(
        validators
            .validate(SCHEMA_ID, &receipt, "receipt")
            .is_empty()
    );

    let measured_provider_input = receipt["fields"][6].clone();
    let measured_output = receipt["fields"][9].clone();
    receipt["fields"][6]["value"] = Value::Null;
    receipt["fields"][6]["provenance"] = json!("unavailable");
    receipt["fields"][6]["producer"] = Value::Null;
    receipt["fields"][6]["note"] =
        json!("provider-reported input tokens were not reported by the model boundary");
    receipt["fields"][9]["value"] = Value::Null;
    receipt["fields"][9]["provenance"] = json!("unavailable");
    receipt["fields"][9]["producer"] = Value::Null;
    receipt["fields"][9]["note"] = json!("output_tokens was not reported by the model boundary");
    assert!(
        validators
            .validate(SCHEMA_ID, &receipt, "receipt")
            .is_empty(),
        "provider fields may be unavailable, but never synthetic zero"
    );
    receipt["fields"][6] = measured_provider_input;
    receipt["fields"][9] = measured_output;

    receipt["executionBinding"]["producerActor"]["id"] = json!("a".repeat(256));
    assert!(
        validators
            .validate(SCHEMA_ID, &receipt, "receipt")
            .is_empty(),
        "the exact persisted actor identity may use the full ActorId bound"
    );
    receipt["executionBinding"]["producerActor"]["id"] = json!("producer:forbidden");
    assert!(
        !validators
            .validate(SCHEMA_ID, &receipt, "receipt")
            .is_empty(),
        "the schema must reject an ID that ActorId cannot deserialize"
    );
    receipt["executionBinding"]["producerActor"]["id"] = json!("runtime-driver");

    for (field, wrong) in [
        ("bindingKind", json!("artifact_binding")),
        (
            "schemaId",
            json!("https://p50.dev/schemas/context-capsule.schema.json"),
        ),
        ("eventHash", json!(format!("sha512:{}", "a".repeat(64)))),
    ] {
        let original = receipt["executionBinding"][field].clone();
        receipt["executionBinding"][field] = wrong;
        assert!(
            !validators
                .validate(SCHEMA_ID, &receipt, "receipt")
                .is_empty(),
            "the closed execution binding must reject wrong {field}"
        );
        receipt["executionBinding"][field] = original;
    }

    let closed_binding = receipt["executionBinding"].clone();
    receipt["executionBinding"] = json!({
        "artifactId": "execution-start-event-1",
        "schemaId": "https://p50.dev/schemas/event-envelope.schema.json",
        "documentVersion": "1.0.0",
        "schemaVersion": "1.0.0",
        "digest": format!("sha256:{}", "a".repeat(64)),
        "scope": {
            "workspaceId": "workspace-accounting",
            "projectId": "project-accounting",
            "executionId": "execution-accounting-fixed"
        },
        "producer": "runtime-driver",
        "snapshots": {
            "repoSnapshot": "repo-snapshot",
            "indexGeneration": "repo-snapshot"
        }
    });
    assert!(
        !validators
            .validate(SCHEMA_ID, &receipt, "receipt")
            .is_empty(),
        "a generic ArtifactBinding must not masquerade as execution identity"
    );
    receipt["executionBinding"] = closed_binding;

    receipt["fields"][6]["value"] = json!(9_007_199_254_740_992_u64);
    assert!(
        !validators
            .validate(SCHEMA_ID, &receipt, "receipt")
            .is_empty(),
        "token counts above JSON safe integer must be rejected"
    );
    receipt["fields"][6]["value"] = json!(12);

    receipt["fields"][0]["note"] = json!("PRIVATE-PROMPT-SENTINEL");
    assert!(
        !validators
            .validate(SCHEMA_ID, &receipt, "receipt")
            .is_empty(),
        "receipt notes are a closed vocabulary, not a secret-bearing free-text channel"
    );
    receipt["fields"][0]["note"] = json!("not measured by this runtime slice");

    let unavailable_compiled = receipt["fields"][8].clone();
    receipt["fields"][8]["value"] = json!(12);
    receipt["fields"][8]["provenance"] = json!("measured");
    receipt["fields"][8]["producer"] = json!("model_gateway");
    receipt["fields"][8]["note"] = json!("");
    assert!(
        !validators
            .validate(SCHEMA_ID, &receipt, "receipt")
            .is_empty(),
        "compiled input must remain unavailable until its complete producer exists"
    );
    receipt["fields"][8] = unavailable_compiled;

    receipt["secretPrompt"] = json!("must never enter Evidence");
    assert!(
        !validators
            .validate(SCHEMA_ID, &receipt, "receipt")
            .is_empty()
    );
    receipt.as_object_mut().unwrap().remove("secretPrompt");
    receipt["fields"][0]["note"] = json!("x".repeat(2049));
    assert!(
        !validators
            .validate(SCHEMA_ID, &receipt, "receipt")
            .is_empty()
    );
}

#[derive(Clone, Copy)]
enum RepositoryPackage {
    Current,
    Release1_0_0,
}

const RELEASE_1_0_0_SCHEMA_NAMES: [&str; 15] = [
    "agent",
    "artifact-reference",
    "claim",
    "context-capsule",
    "edge",
    "event-envelope",
    "evidence-record",
    "extension",
    "graph",
    "graph-signal",
    "node",
    "persisted-graph-version",
    "policy-waiver",
    "repository-scope",
    "sensitivity",
];

const CURRENT_SCHEMA_NAMES: [&str; 22] = [
    "activation-receipt",
    "adoption-journal",
    "adoption-plan",
    "adoption-receipt",
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
    "restore-plan",
    "sensitivity",
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

    fn schema_names(self) -> &'static [&'static str] {
        match self {
            Self::Current => &CURRENT_SCHEMA_NAMES,
            Self::Release1_0_0 => &RELEASE_1_0_0_SCHEMA_NAMES,
        }
    }
}

fn package_layout_is_exact(catalog: &SchemaCatalog, package: RepositoryPackage) -> bool {
    let schema_names = package.schema_names();
    catalog.schemas.len() == schema_names.len()
        && catalog
            .schemas
            .keys()
            .map(String::as_str)
            .eq(schema_names.iter().copied())
        && schema_names
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

fn expected_schema_inventory(package: RepositoryPackage) -> Vec<String> {
    let mut inventory = package
        .schema_names()
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
        expected_schema_inventory(package)
    );
    let schemas = package
        .schema_names()
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

// Prevents a root catalog from loading immutable release files, and vice versa.
#[test]
fn repository_package_layout_rejects_cross_package_schema_paths() {
    let mut current = load_repo_catalog(RepositoryPackage::Current);
    current.catalog.schemas.get_mut("agent").unwrap().path =
        "schemas/releases/1.0.0/agent.schema.json".into();
    let current_report = validate_catalog(&current);
    assert!(!current_report.ok);
    assert_eq!(current_report.diagnostics[0].path, "/schemas/agent/path");

    let mut release = load_repo_catalog(RepositoryPackage::Release1_0_0);
    release.catalog.schemas.get_mut("agent").unwrap().path = "schemas/agent.schema.json".into();
    let release_report = validate_catalog(&release);
    assert!(!release_report.ok);
    assert_eq!(release_report.diagnostics[0].path, "/schemas/agent/path");
}

#[test]
fn release_catalog_directory_must_match_its_release_version() {
    let mut release = load_repo_catalog(RepositoryPackage::Release1_0_0);
    release.catalog_source = "schemas/releases/2.0.0/catalog.json".into();
    let report = validate_catalog(&release);
    assert!(!report.ok);
    assert_eq!(report.diagnostics[0].path, "/releaseVersion");
}

// Prevents the public baseline from being partial, and pins the rule that replaced byte-identity
// the day the first schema evolved.
//
// #508: this test was RED on main from `14f82b2` until the hotfix that carries this comment. The
// `event-envelope` entry in BOTH catalogs — live and the frozen `releases/1.0.0/` snapshot —
// recorded `sha256:214527df…` for bytes whose canonical digest is `sha256:d6041b65…`. The hotfix
// re-derived both entries with the house instrument (`schema_digest` over `canonical_json`, NOT a
// sha over raw file bytes).
//
// WHY EDITING THE FROZEN SNAPSHOT'S ENTRY IS LEGAL, and it is the whole argument — do not revert
// this as vandalism. What this guard protects is the AUTHORED artifact: the schema BYTES. Those are
// untouched, and the hotfix's diff proves it (two catalog files changed, zero `*.schema.json`).
// A digest entry is not authored content, it is a DERIVATION over those bytes, and this one was
// derived wrong at authoring time. Re-deriving corrects the record to describe the bytes it always
// claimed to describe. The alternative — a 1.0.1 that leaves 1.0.0 self-inconsistent forever —
// would need a permanent exemption cell right here: a standing lie with a standing waiver.
//
// PROVENANCE, measured rather than guessed. `14f82b2` ("feat(memory): persist safe admission
// refusal events") edited the schema and wrote the entry in the same commit. Its parent was
// CONSISTENT (recorded `b45ddada…` == actual `b45ddada…`), so this is not a stale pin someone
// forgot to bump: a stale pin would still read `b45ddada…`. The recorded value matches neither the
// before-bytes nor the after-bytes. Five candidate derivations of the shipped bytes were tried —
// raw bytes as stored, raw bytes with CRLF, canonical JSON, `json.dumps` defaults, and canonical
// JSON with key order preserved — and NONE produces `214527df…`. So HOW it was computed is not
// established; the likeliest story, marked as inference and not measurement, is a digest taken over
// an intermediate draft during that commit and never recomputed against the bytes that shipped.
//
// AND the byte-identity rule this test carried changed the same day, for a second reason:
// Until 2026-08-30 this test asserted current == release, byte for byte, for every schema - the
// guard that caught #339's silent drift. That mirror could hold only while NOTHING had ever
// evolved; graph-signal 1.1.0 (optional `to`/`replyTo`, the multi-agent conversation's addressing)
// is the first declared evolution, which takes the decision #229 reserved: `schemas/releases/1.0.0`
// is a FROZEN SNAPSHOT of history, and the live root moves ahead of it.
//
// The #339 class stays caught, in a sharper form: a schema whose documentVersion still equals the
// release's MUST remain byte-identical to the snapshot (drift without a declaration), and a schema
// whose version moved MUST actually differ (a declaration without a change is the same lie
// mirrored). Retiring the check outright would have been [dont-retire-the-check-that-catches-you];
// this is its replacement, and it can still go red on both sides of the line.
#[test]
fn the_release_snapshot_is_frozen_and_divergence_is_version_declared() {
    let root = repository_root();
    let current = load_repo_catalog(RepositoryPackage::Current);
    let release = load_repo_catalog(RepositoryPackage::Release1_0_0);

    // 1.1.0, for the reason written at the first release_version assertion in this file.
    assert_eq!(current.catalog.release_version, Version::new(1, 1, 0));
    assert_eq!(release.catalog.release_version, Version::new(1, 0, 0));
    assert_eq!(current.catalog.schemas.len(), CURRENT_SCHEMA_NAMES.len());
    assert_eq!(release.catalog.schemas.len(), 15);
    for (label, resources) in [("current", &current), ("release", &release)] {
        let report = validate_catalog(resources);
        assert!(
            report.ok,
            "{label} catalog failed validation: {}",
            report
                .diagnostics
                .iter()
                .map(|d| format!("{} at {}: {}", d.code, d.path, d.message))
                .collect::<Vec<_>>()
                .join("; ")
        );
    }

    // MERGED INTENT: main's loop asserted every released schema byte-identical (its world had
    // only an ADDED schema); this branch's declared evolution needs the finer #339 form - equal
    // bytes exactly where the documentVersion did not move, different bytes exactly where it did.
    // The finer form implies main's for every undeclared schema, so nothing main gated is lost.
    let mut evolved = 0usize;
    for name in RELEASE_1_0_0_SCHEMA_NAMES {
        let current_entry = &current.catalog.schemas[name];
        let release_entry = &release.catalog.schemas[name];
        let current_bytes =
            fs::read(root.join(RepositoryPackage::Current.schema_path(name))).unwrap();
        let release_bytes =
            fs::read(root.join(RepositoryPackage::Release1_0_0.schema_path(name))).unwrap();

        if current_entry.document_version == release_entry.document_version {
            // The #339 guard, alive: same declared version, so the bytes may not have moved.
            assert_eq!(
                current_bytes, release_bytes,
                "{name} drifted from the release snapshot without moving its documentVersion"
            );
            assert_eq!(current_entry.sha256, release_entry.sha256, "{name}");
        } else {
            evolved += 1;
            assert!(
                current_entry.document_version > release_entry.document_version,
                "{name} declares a version behind the released one"
            );
            assert_ne!(
                current_bytes, release_bytes,
                "{name} moved its documentVersion without changing a byte - a declaration \
                 without a change is the mirrored form of the drift this test exists to refuse"
            );
        }
    }
    // The live release_version may only move ahead of the snapshot when something actually
    // changed - a schema evolved, or one was ADDED (main's accounting receipt moved 1.1.0 on an
    // addition alone, a legitimate cause this check must admit); a bumped catalog over an
    // identical set is the catalog telling a story on its own.
    let added = current.catalog.schemas.len() > release.catalog.schemas.len();
    if current.catalog.release_version > release.catalog.release_version {
        assert!(
            evolved > 0 || added,
            "the live catalog's releaseVersion moved but every schema is still at the snapshot"
        );
    } else {
        assert!(
            evolved == 0 && !added,
            "the schema set changed but the catalog's releaseVersion did not move"
        );
    }
}

#[test]
fn customs_budget_seconds_are_bounded_in_both_schema_packages() {
    const MAX_BUDGET_SECONDS: u64 = 315_576_000;
    let schema_id = "https://p50.dev/schemas/node.schema.json";

    for (package_name, package) in [
        ("current", RepositoryPackage::Current),
        ("release", RepositoryPackage::Release1_0_0),
    ] {
        let validators = OfflineSchemaSet::compile(load_repo_catalog(package).schemas).unwrap();

        for field in [
            "waitWithinSeconds",
            "clearanceWithinSeconds",
            "dlqWithinSeconds",
        ] {
            let node = |value| {
                let mut budgets = json!({
                    "waitWithinSeconds": 1,
                    "clearanceWithinSeconds": 1,
                    "dlqWithinSeconds": 1
                });
                budgets[field] = json!(value);
                json!({
                    "type": "gate",
                    "name": "bounded-customs-budget",
                    "objective": "Prove the authoring limit",
                    "optionality": "required",
                    "completion": {"customs": {"budgets": budgets}}
                })
            };

            let accepted = validators.validate(
                schema_id,
                &node(MAX_BUDGET_SECONDS),
                &format!("{package_name}-{field}-accepted"),
            );
            assert!(
                accepted.is_empty(),
                "{package_name} rejected {field} at the maximum: {accepted:?}"
            );

            let rejected = validators.validate(
                schema_id,
                &node(MAX_BUDGET_SECONDS + 1),
                &format!("{package_name}-{field}-rejected"),
            );
            assert_eq!(rejected.len(), 1, "{package_name} accepted {field}");
            assert_eq!(rejected[0].code, "GHS002_SCHEMA");
            assert_eq!(
                rejected[0].path,
                format!("/completion/customs/budgets/{field}")
            );
        }
    }
}

#[test]
fn path_content_slots_are_identical_closed_1_0_0_contracts() {
    let root = repository_root();
    let current = load_repo_catalog(RepositoryPackage::Current);
    let release = load_repo_catalog(RepositoryPackage::Release1_0_0);
    let current_validators = OfflineSchemaSet::compile(current.schemas.clone()).unwrap();
    let release_validators = OfflineSchemaSet::compile(release.schemas.clone()).unwrap();
    let schema_id = "https://p50.dev/schemas/persisted-graph-version.schema.json";
    let fixture: Value = serde_json::from_slice(
        &fs::read(root.join("conformance/schemas/valid/persisted-graph-version.json")).unwrap(),
    )
    .unwrap();

    for field_kind in ["context_path", "permission_path", "isolation_path"] {
        let mut document = fixture.clone();
        document["contentSlots"][0]["fieldKind"] = json!(field_kind);
        assert!(
            current_validators
                .validate(schema_id, &document, "current")
                .is_empty(),
            "root rejected {field_kind}"
        );
        assert!(
            release_validators
                .validate(schema_id, &document, "release")
                .is_empty(),
            "release rejected {field_kind}"
        );
    }

    let mut foreign = fixture;
    foreign["contentSlots"][0]["fieldKind"] = json!("filesystem_path");
    assert!(
        !current_validators
            .validate(schema_id, &foreign, "current")
            .is_empty()
    );
    assert!(
        !release_validators
            .validate(schema_id, &foreign, "release")
            .is_empty()
    );

    // DELIBERATE (M08): the pinned digest moved because the persisted node gained the
    // OPTIONAL timeoutSeconds the user already declares and the linter already demands —
    // a declaration that used to be dropped at persistence. The pin exists so a schema
    // edit is a decision someone defends in a diff, which is what this comment is.
    //
    // DELIBERATE (#162, 2026-08-24): it moved again, and for the same shape of reason one
    // milestone later. The persisted node gained the OPTIONAL `customs` block that #160 added
    // to `PersistedNode` and that nothing could ever store: `$defs.node` closes with
    // `additionalProperties: false`, so every `graph_version_published` whose node declared
    // budgets was refused by `validate_envelope` with a bare `Invalid` naming no field. No
    // budget could persist, so no stage deadline could be computed, so the overdue sweep was
    // structurally empty. Measured as a controlled pair on one envelope: with customs, one
    // schema error at /kind; without customs, none — before and after, with only the second
    // column unchanged.
    //
    // `customs` is NOT in `required`, and that is the half worth defending: absence has to
    // stay absence, or every version published before customs existed becomes invalid
    // retroactively. The required/optional split inside the block is read off the Rust type
    // rather than chosen — `waitWithinSeconds` and `clearanceWithinSeconds` are plain `u64`;
    // `dlqWithinSeconds` is `Option<u64>` WITH `skip_serializing_if`, which means
    // optional-and-not-nullable rather than required-and-nullable.
    //
    // This assertion is why the change is defended here at all. It is the FIFTH site of a
    // delta whose scope was measured as four, and the only one that is not a schema or a
    // catalog — a test holding the digest as a tripwire. It fired.
    //
    // It also fired GREEN over a broken version of this very comment: the first attempt to
    // write it went through a shell, whose backticks ate every identifier above and left the
    // prose gutted. The suite passed anyway, because the tripwire guards the DIGEST and
    // nothing guards the DEFENCE. A justification is checked by a reader or not at all.
    //
    // DELIBERATE (#288, 2026-08-25): it moved a third time, because the persisted node's
    // `nodeType` enum gained `dead_letter` — the dead-letter node, declared so the sweep lane
    // has a name to route to. LEGAL BUT NOT PRODUCED in v1: nothing emits it, no driver
    // dispatches it, and `classify::work_kind` refuses it in the structural set.
    //
    // The authoring schema's `type` enum gained it in the same change, and that pairing is the
    // trap worth recording: the two schemas name the SAME concept with DIFFERENT keys, so a
    // sweep for one key finds two of the four files and reports success. This lane's first
    // enumeration missed the authoring schema for exactly that reason.
    //
    // What is new this time is that a guard now ASKS. `NodeType` had no equivalent of
    // `EventKind::EVERY_WIRE_NAME`, so nothing compared the Rust enum to the schema lists: a
    // variant the type accepted and the schemas refused would have produced no symptom at all,
    // which is the shape of the #162 `customs` defect one vocabulary over. The enum now derives
    // its list and `as_str` from one macro, and
    // `core/protocols/tests/node_type_vocabularies_agree.rs` compares that list against all four
    // files. It was written FIRST and observed RED on `node.schema.json` with
    // `missing from the schema: ["dead_letter"]` before any schema was touched.
    assert_eq!(
        schema_digest(&current.schemas["persisted-graph-version"])
            .unwrap()
            .as_str(),
        "sha256:1a6980bd47eeecf8eb2bb624db0a640b0d0abb5391d1a5cb4bbd39a8215dadfb"
    );
}

// Prevents the immutable snapshot from enforcing different public outcomes for shared schemas.
#[test]
fn current_and_1_0_0_release_enforce_identical_shared_public_schema_contracts() {
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

    // 39 = main's 32, plus the two graph-signal-reply cases the 1.1.0 evolution shipped, plus
    // the three context-provenance cases (#1065: valid, measured-estimate, traversal-source),
    // plus the `root` refusal (#1086), plus the node crew acceptance (#1049).
    //
    // #1049 ships ONE node case here and not the usual valid/invalid pair, and the asymmetry
    // rule below is the reason. `node` carries `additionalProperties: true`, so the frozen 1.0.0
    // validator ACCEPTS a node whose `agents` key holds anything at all, an empty list included.
    // A refusal fixture for the new constraints would therefore land in the forbidden quadrant --
    // live refusing what the release accepted -- and this test would be right to fail. The crew
    // refusals are proven against the live validator alone, where they belong, in
    // `core/schema/tests/node_agents.rs`.
    assert_eq!(schema_cases.len(), 39);
    let current_catalog = load_repo_catalog(RepositoryPackage::Current).catalog;
    let release_catalog = load_repo_catalog(RepositoryPackage::Release1_0_0).catalog;
    for case in schema_cases {
        let name = case["schema"].as_str().unwrap();
        // Schemas the frozen 1.0.0 release never held have no release-side validator to agree
        // with: the receipt (added whole after 1.0.0) and context-provenance (#1065, likewise).
        if name == "execution-accounting-receipt" || name == "context-provenance" {
            continue;
        }
        let input = case["input"].as_str().unwrap();
        let document: Value = serde_json::from_slice(&fs::read(root.join(input)).unwrap()).unwrap();
        let schema_id = format!("https://p50.dev/schemas/{name}.schema.json");
        let current_diagnostics = current_validators.validate(&schema_id, &document, "conformance");
        let release_diagnostics = release_validators.validate(&schema_id, &document, "conformance");
        let signature = |diagnostics: &[graphhelm_protocols::Diagnostic]| {
            diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code.clone(), diagnostic.path.clone()))
                .collect::<Vec<_>>()
        };
        if current_catalog.schemas[name].document_version
            == release_catalog.schemas[name].document_version
        {
            assert_eq!(
                signature(&release_diagnostics),
                signature(&current_diagnostics),
                "{}",
                case["id"]
            );
        } else {
            // An evolved schema's contract is ASYMMETRIC by design: additive evolution means
            // every document the frozen 1.0.0 validator accepts must still be accepted by the
            // live one (old data never goes invalid), while a document using the new fields is
            // legitimately refused by the snapshot that predates them. The forbidden quadrant is
            // the live validator refusing what the release accepted - that would be evolution
            // invalidating history while calling itself minor.
            assert!(
                !release_diagnostics.is_empty() || current_diagnostics.is_empty(),
                "{}: the evolved {name} validator refuses a document the frozen 1.0.0 one \
                 accepts - a backward break wearing a minor version",
                case["id"]
            );
        }
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
    let report = validate_catalog(&resources);
    assert!(
        report.ok,
        "a formatting-only rewrite must not invalidate the canonical digest: {}",
        report
            .diagnostics
            .iter()
            .map(|d| format!("{} at {}: {}", d.code, d.path, d.message))
            .collect::<Vec<_>>()
            .join("; ")
    );
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
    resources.catalog_source = r"C:\Users\example\secret\catalog.json".into();
    resources.catalog.format_version = 2;
    let report = validate_catalog(&resources);
    assert_eq!(report.diagnostics[0].source, "schema-evolution");
    assert!(!report.diagnostics[0].message.contains("example"));
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

#[test]
fn adoption_contracts_are_catalogued_without_rewriting_the_frozen_release() {
    let root = repository_root();
    let catalog: SchemaCatalog =
        serde_json::from_slice(&fs::read(root.join("schemas/catalog.json")).unwrap()).unwrap();
    let release: SchemaCatalog = serde_json::from_slice(
        &fs::read(root.join("schemas/releases/1.0.0/catalog.json")).unwrap(),
    )
    .unwrap();
    for name in [
        "activation-receipt",
        "adoption-plan",
        "adoption-receipt",
        "adoption-journal",
        "restore-plan",
    ] {
        let entry = catalog
            .schemas
            .get(name)
            .expect("adoption wire contract is catalogued");
        let schema: Value =
            serde_json::from_slice(&fs::read(root.join(&entry.path)).unwrap()).unwrap();
        assert_eq!(entry.sha256, schema_digest(&schema).unwrap());
        assert!(!release.schemas.contains_key(name));
    }
}
