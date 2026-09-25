use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use graphhelm_schema_evolution::{
    ChangelogKey, ConformanceCase, ConformanceSuite, FixturePairKey, MigrationCatalogs,
    MigrationKey, MigrationManifest, ReleaseEvidence, apply_migration, canonical_json,
    compare_catalogs, enforce_release, validate_migration_manifest,
};
use semver::Version;

use crate::output::Outcome;

use super::io::{Error, ReadBudget, failure, load_catalog};

const COMMAND: &str = "schema.check";

pub(crate) fn run(baseline: &Path, candidate: &Path) -> Outcome {
    match execute(baseline, candidate) {
        Ok(data) => Outcome::success(COMMAND, data),
        Err(error) => failure(COMMAND, error),
    }
}

fn execute(baseline: &Path, candidate: &Path) -> Result<serde_json::Value, Error> {
    let mut budget = ReadBudget::new();
    let baseline = load_catalog(baseline, &mut budget)?;
    let candidate = load_catalog(candidate, &mut budget)?;
    let compatibility = compare_catalogs(&baseline.resources, &candidate.resources);
    let evidence = release_evidence(&baseline, &candidate, &mut budget)?;
    let release = enforce_release(
        &baseline.resources,
        &candidate.resources,
        &compatibility,
        &evidence,
    );
    if !release.ok {
        return Err(Error::Domain(release.diagnostics));
    }
    Ok(serde_json::json!({
        "compatibility": compatibility,
        "release": release
    }))
}

fn release_evidence(
    baseline: &super::io::LoadedCatalog,
    candidate: &super::io::LoadedCatalog,
    budget: &mut ReadBudget,
) -> Result<ReleaseEvidence, Error> {
    let mut migration_manifests = BTreeMap::new();
    for path in candidate.repository.walk_json("schemas/migrations")? {
        let manifest: MigrationManifest = candidate.repository.read_confined_json(
            &path,
            budget,
            "GHM001_MIGRATION_UNSUPPORTED",
            "schema-check",
        )?;
        if validate_migration_manifest(
            &manifest,
            Some(&baseline.resources.catalog),
            &candidate.resources.catalog,
        )
        .is_ok()
        {
            let key = MigrationKey {
                schema: manifest.schema.clone(),
                from_version: manifest.from_version.clone(),
                to_version: manifest.to_version.clone(),
            };
            migration_manifests.insert(key, manifest);
        }
    }
    let migrations = migration_manifests.keys().cloned().collect();

    let suite: ConformanceSuite = candidate.repository.read_declared_json(
        "conformance/manifest.json",
        budget,
        "GHCONF001_FIXTURE_FAILED",
        "/",
        "schema-check",
    )?;
    let mut fixture_pairs = BTreeSet::new();
    for case in &suite.cases {
        if let ConformanceCase::Release {
            input,
            comparison: Some(comparison),
            schema: Some(schema),
            from_version: Some(from_version),
            to_version: Some(to_version),
            ..
        } = case
        {
            let after = candidate.repository.read_declared_value(
                input,
                budget,
                "GHCONF001_FIXTURE_FAILED",
                "/evidence/fixturePairs/input",
                "schema-check",
            )?;
            let before = candidate.repository.read_declared_value(
                comparison,
                budget,
                "GHCONF001_FIXTURE_FAILED",
                "/evidence/fixturePairs/comparison",
                "schema-check",
            )?;
            let key = MigrationKey {
                schema: schema.clone(),
                from_version: from_version.clone(),
                to_version: to_version.clone(),
            };
            if migration_manifests.get(&key).is_some_and(|manifest| {
                validates_exact_fixture_pair(baseline, candidate, manifest, &before, &after)
            }) {
                fixture_pairs.insert(FixturePairKey {
                    schema: schema.clone(),
                    from_version: from_version.clone(),
                    to_version: to_version.clone(),
                });
            }
        }
    }

    let changelog = candidate
        .repository
        .read_declared_text("schemas/CHANGELOG.md", budget)?;
    let changelog_entries = changelog_evidence(&changelog, baseline, candidate);
    Ok(ReleaseEvidence {
        migrations,
        fixture_pairs,
        changelog_entries,
    })
}

fn validates_exact_fixture_pair(
    baseline: &super::io::LoadedCatalog,
    candidate: &super::io::LoadedCatalog,
    manifest: &MigrationManifest,
    before: &serde_json::Value,
    after: &serde_json::Value,
) -> bool {
    let Some(source_entry) = baseline.resources.catalog.schemas.get(&manifest.schema) else {
        return false;
    };
    let Some(target_entry) = candidate.resources.catalog.schemas.get(&manifest.schema) else {
        return false;
    };
    if source_entry.document_version != manifest.from_version
        || target_entry.document_version != manifest.to_version
        || !baseline
            .validators
            .validate(&source_entry.id, before, "schema-check")
            .is_empty()
        || !candidate
            .validators
            .validate(&target_entry.id, after, "schema-check")
            .is_empty()
    {
        return false;
    }

    let migrated = apply_migration(
        before,
        manifest,
        &MigrationCatalogs {
            source: &baseline.resources.catalog,
            target: &candidate.resources.catalog,
        },
        |document| {
            baseline
                .validators
                .validate(&source_entry.id, document, "schema-check")
        },
        |document| {
            candidate
                .validators
                .validate(&target_entry.id, document, "schema-check")
        },
    );
    migrated.document.as_ref().is_some_and(|document| {
        matches!(
            (canonical_json(document), canonical_json(after)),
            (Ok(actual), Ok(expected)) if actual == expected
        )
    })
}

fn changelog_evidence(
    changelog: &str,
    baseline: &super::io::LoadedCatalog,
    candidate: &super::io::LoadedCatalog,
) -> BTreeSet<ChangelogKey> {
    let mut section = None;
    let mut entries = BTreeSet::new();
    for line in changelog.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## ") {
            section = changelog_heading_version(trimmed);
            continue;
        }
        let Some(schema) = trimmed
            .strip_prefix("- BREAKING ")
            .and_then(|value| value.split_once(':'))
            .filter(|(_, detail)| !detail.trim().is_empty())
            .map(|(schema, _)| schema.trim())
        else {
            continue;
        };
        let (Some(from), Some(to)) = (
            baseline.resources.catalog.schemas.get(schema),
            candidate.resources.catalog.schemas.get(schema),
        ) else {
            continue;
        };
        if section.as_ref() == Some(&candidate.resources.catalog.release_version) {
            entries.insert(ChangelogKey {
                schema: schema.to_owned(),
                from_version: from.document_version.clone(),
                to_version: to.document_version.clone(),
            });
        }
    }
    entries
}

fn changelog_heading_version(heading: &str) -> Option<Version> {
    let value = heading.strip_prefix("## [")?;
    let closing = value.find(']')?;
    let version_text = &value[..closing];
    let suffix = &value[closing + 1..];
    if !suffix.is_empty() {
        let date = suffix.strip_prefix(" - ")?;
        if date.len() != 10
            || date.as_bytes()[4] != b'-'
            || date.as_bytes()[7] != b'-'
            || !date
                .bytes()
                .enumerate()
                .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
            || chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_err()
        {
            return None;
        }
    }
    let version = Version::parse(version_text).ok()?;
    (version.pre.is_empty() && version.build.is_empty() && version.to_string() == version_text)
        .then_some(version)
}
