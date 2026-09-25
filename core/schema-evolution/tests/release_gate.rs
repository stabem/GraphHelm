use std::collections::{BTreeMap, BTreeSet};

use graphhelm_schema_evolution::{
    CatalogEntry, CatalogResources, ChangelogKey, CompatibilityChange, CompatibilityClass,
    CompatibilityReport, FixturePairKey, MigrationKey, ReleaseEvidence, SchemaCatalog,
    SemverImpact, compare_catalogs, enforce_release, schema_digest,
};
use semver::Version;
use serde_json::json;

fn resources(version: &str, document_version: &str, path: &str) -> CatalogResources {
    let document = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "https://p50.dev/schemas/graph.schema.json",
        "x-graphhelm-schema-version": document_version,
        "type": "object"
    });
    CatalogResources {
        catalog_source: "schemas/catalog.json".into(),
        catalog: SchemaCatalog {
            format_version: 1,
            release_version: Version::parse(version).unwrap(),
            schemas: BTreeMap::from([(
                "graph".into(),
                CatalogEntry {
                    id: "https://p50.dev/schemas/graph.schema.json".into(),
                    document_version: Version::parse(document_version).unwrap(),
                    path: path.into(),
                    sha256: schema_digest(&document).unwrap(),
                },
            )]),
        },
        schemas: BTreeMap::from([("graph".into(), document)]),
    }
}

fn add_schema(resources: &mut CatalogResources, name: &str, version: &str) {
    let id = format!("https://p50.dev/schemas/{name}.schema.json");
    let document = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": id,
        "x-graphhelm-schema-version": version,
        "type": "object"
    });
    resources.catalog.schemas.insert(
        name.into(),
        CatalogEntry {
            id,
            document_version: Version::parse(version).unwrap(),
            path: format!("schemas/{name}.schema.json"),
            sha256: schema_digest(&document).unwrap(),
        },
    );
    resources.schemas.insert(name.into(), document);
}

fn report(impact: SemverImpact) -> CompatibilityReport {
    let class = match impact {
        SemverImpact::None => CompatibilityClass::Unchanged,
        SemverImpact::Patch => CompatibilityClass::Annotation,
        SemverImpact::Minor => CompatibilityClass::Compatible,
        SemverImpact::Major => CompatibilityClass::Breaking,
    };
    let changes = (impact != SemverImpact::None)
        .then(|| CompatibilityChange {
            schema: "graph".into(),
            code: "GHC003_BREAKING_CHANGE".into(),
            pointer: "/required".into(),
            baseline_summary: "baseline".into(),
            candidate_summary: "candidate".into(),
            class,
            impact,
        })
        .into_iter()
        .collect();
    CompatibilityReport {
        compatible: class != CompatibilityClass::Breaking,
        class,
        impact,
        changes,
    }
}

fn evidence() -> ReleaseEvidence {
    ReleaseEvidence {
        migrations: BTreeSet::from([MigrationKey {
            schema: "graph".into(),
            from_version: Version::parse("1.2.3").unwrap(),
            to_version: Version::parse("2.0.0").unwrap(),
        }]),
        fixture_pairs: BTreeSet::from([FixturePairKey {
            schema: "graph".into(),
            from_version: Version::parse("1.2.3").unwrap(),
            to_version: Version::parse("2.0.0").unwrap(),
        }]),
        changelog_entries: BTreeSet::from([ChangelogKey {
            schema: "graph".into(),
            from_version: Version::parse("1.2.3").unwrap(),
            to_version: Version::parse("2.0.0").unwrap(),
        }]),
    }
}

fn assert_gate(from: &str, to: &str, impact: SemverImpact, expected_ok: bool) {
    let baseline = resources(from, from, "schemas/graph.schema.json");
    let candidate = resources(to, to, "schemas/graph.schema.json");
    let release = enforce_release(&baseline, &candidate, &report(impact), &evidence());
    assert_eq!(release.ok, expected_ok, "{from} -> {to} for {impact:?}");
}

fn diagnostic_signature(
    report: &graphhelm_schema_evolution::ReleaseReport,
) -> Vec<(String, String)> {
    report
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.code.clone(), diagnostic.path.clone()))
        .collect()
}

// Prevents release publication from accepting a lower impact or a skipped SemVer version.
#[test]
fn exact_impact_is_required_without_version_skips() {
    assert_gate("1.2.3", "1.2.3", SemverImpact::None, true);
    assert_gate("1.2.3", "1.2.4", SemverImpact::Patch, true);
    assert_gate("1.2.3", "1.3.0", SemverImpact::Minor, true);
    assert_gate("1.2.3", "2.0.0", SemverImpact::Major, true);
    assert_gate("1.2.3", "1.4.0", SemverImpact::Minor, false);
    assert_gate("1.2.3", "2.1.0", SemverImpact::Major, false);
}

fn breaking_release_without(missing: &str) -> graphhelm_schema_evolution::ReleaseReport {
    let mut release_evidence = evidence();
    let key = MigrationKey {
        schema: "graph".into(),
        from_version: Version::parse("1.2.3").unwrap(),
        to_version: Version::parse("2.0.0").unwrap(),
    };
    match missing {
        "migration" => release_evidence.migrations.remove(&key),
        "fixturePair" => release_evidence.fixture_pairs.remove(&FixturePairKey {
            schema: "graph".into(),
            from_version: Version::parse("1.2.3").unwrap(),
            to_version: Version::parse("2.0.0").unwrap(),
        }),
        "changelog" => release_evidence.changelog_entries.remove(&ChangelogKey {
            schema: "graph".into(),
            from_version: Version::parse("1.2.3").unwrap(),
            to_version: Version::parse("2.0.0").unwrap(),
        }),
        _ => unreachable!(),
    };
    enforce_release(
        &resources("1.2.3", "1.2.3", "schemas/graph.schema.json"),
        &resources("2.0.0", "2.0.0", "schemas/graph.schema.json"),
        &report(SemverImpact::Major),
        &release_evidence,
    )
}

// Prevents a breaking change from being published without every independently auditable artifact.
#[test]
fn breaking_release_needs_all_three_evidence_kinds() {
    for (missing, expected_path) in [
        ("migration", "/evidence/migrations/graph"),
        ("fixturePair", "/evidence/fixturePairs/graph"),
        ("changelog", "/evidence/changelogEntries/graph"),
    ] {
        let release = breaking_release_without(missing);
        assert_eq!(
            diagnostic_signature(&release),
            vec![("GHC004_SEMVER_MISMATCH".into(), expected_path.into())]
        );
    }
}

// Prevents SemVer metadata from being accepted when it is not a stable milestone 02 version.
#[test]
fn prerelease_and_build_metadata_are_rejected() {
    for version in ["1.2.4-rc.1", "1.2.4+build.5"] {
        let release = enforce_release(
            &resources("1.2.3", "1.2.3", "schemas/graph.schema.json"),
            &resources(version, version, "schemas/graph.schema.json"),
            &report(SemverImpact::Patch),
            &ReleaseEvidence::default(),
        );
        assert!(!release.ok, "{version}");
        assert_eq!(
            diagnostic_signature(&release),
            vec![
                ("GHC004_SEMVER_MISMATCH".into(), "/releaseVersion".into()),
                ("GHC004_SEMVER_MISMATCH".into(), "/releaseVersion".into()),
                (
                    "GHC004_SEMVER_MISMATCH".into(),
                    "/schemas/graph/documentVersion".into(),
                ),
                (
                    "GHC004_SEMVER_MISMATCH".into(),
                    "/schemas/graph/documentVersion".into(),
                ),
            ],
            "{version}"
        );
    }
}

// Prevents a numerically exhausted SemVer segment from silently becoming an unchanged version.
#[test]
fn exhausted_semver_segments_are_rejected_instead_of_saturating() {
    let exhausted = "1.2.18446744073709551615";
    let release = enforce_release(
        &resources(exhausted, exhausted, "schemas/graph.schema.json"),
        &resources(exhausted, exhausted, "schemas/graph.schema.json"),
        &report(SemverImpact::Patch),
        &ReleaseEvidence::default(),
    );
    assert!(!release.ok);
    assert_eq!(
        diagnostic_signature(&release),
        vec![
            ("GHC004_SEMVER_MISMATCH".into(), "/releaseVersion".into()),
            (
                "GHC004_SEMVER_MISMATCH".into(),
                "/schemas/graph/documentVersion".into(),
            ),
        ]
    );
}

// Prevents newly introduced schemas from publishing arbitrary historical or future document versions.
#[test]
fn added_schemas_require_the_milestone_initial_document_version() {
    for (document_version, expected_ok) in [("1.0.0", true), ("0.1.0", false), ("999.0.0", false)] {
        let baseline = resources("1.0.0", "1.0.0", "schemas/graph.schema.json");
        let mut candidate = resources("1.1.0", "1.0.0", "schemas/graph.schema.json");
        add_schema(&mut candidate, "node", document_version);
        let compatibility = compare_catalogs(&baseline, &candidate);
        assert_eq!(compatibility.impact, SemverImpact::Minor);

        let release = enforce_release(
            &baseline,
            &candidate,
            &compatibility,
            &ReleaseEvidence::default(),
        );
        assert_eq!(release.ok, expected_ok, "{document_version}");
        if !expected_ok {
            assert_eq!(
                diagnostic_signature(&release),
                vec![(
                    "GHC004_SEMVER_MISMATCH".into(),
                    "/schemas/node/documentVersion".into(),
                )]
            );
        }
    }
}

fn removal_evidence() -> ReleaseEvidence {
    let from_version = Version::parse("1.0.0").unwrap();
    let to_version = Version::parse("2.0.0").unwrap();
    ReleaseEvidence {
        migrations: BTreeSet::from([MigrationKey {
            schema: "graph".into(),
            from_version: from_version.clone(),
            to_version: to_version.clone(),
        }]),
        fixture_pairs: BTreeSet::from([FixturePairKey {
            schema: "graph".into(),
            from_version: from_version.clone(),
            to_version: to_version.clone(),
        }]),
        changelog_entries: BTreeSet::from([ChangelogKey {
            schema: "graph".into(),
            from_version,
            to_version,
        }]),
    }
}

// Prevents a removed schema from borrowing the aggregate release version as a made-up target document version.
#[test]
fn removed_schemas_fail_closed_even_with_complete_evidence() {
    let baseline = resources("1.0.0", "1.0.0", "schemas/graph.schema.json");
    let mut candidate = resources("2.0.0", "2.0.0", "schemas/graph.schema.json");
    candidate.catalog.schemas.remove("graph");
    candidate.schemas.remove("graph");
    let compatibility = compare_catalogs(&baseline, &candidate);
    assert_eq!(compatibility.impact, SemverImpact::Major);

    let release = enforce_release(&baseline, &candidate, &compatibility, &removal_evidence());
    assert!(!release.ok);
    assert_eq!(
        diagnostic_signature(&release),
        vec![(
            "GHC004_SEMVER_MISMATCH".into(),
            "/schemas/graph/removal".into(),
        )]
    );
}

// Prevents a snapshot path from being misclassified as an operational schema change.
#[test]
fn unchanged_current_and_snapshot_catalogs_keep_the_release_version() {
    let baseline = resources("1.0.0", "1.0.0", "schemas/releases/1.0.0/graph.schema.json");
    let candidate = resources("1.0.0", "1.0.0", "schemas/graph.schema.json");
    let compatibility = compare_catalogs(&baseline, &candidate);
    assert_eq!(compatibility.impact, SemverImpact::None);
    let release = enforce_release(
        &baseline,
        &candidate,
        &compatibility,
        &ReleaseEvidence::default(),
    );
    assert!(release.ok, "{:?}", release.diagnostics);
    assert!(release.diagnostics.is_empty());
}
