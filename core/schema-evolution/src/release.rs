use std::collections::{BTreeMap, BTreeSet};

use graphhelm_protocols::Diagnostic;
use semver::Version;
use serde::Serialize;

use crate::{CatalogResources, CompatibilityReport, SemverImpact, sort_diagnostics};

const SEMVER_MISMATCH_CODE: &str = "GHC004_SEMVER_MISMATCH";

/// Identifies one version-to-version migration required by a breaking schema change.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationKey {
    pub schema: String,
    pub from_version: Version,
    pub to_version: Version,
}

/// Identifies one before/after conformance fixture required by a breaking schema change.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FixturePairKey {
    pub schema: String,
    pub from_version: Version,
    pub to_version: Version,
}

/// Identifies one changelog entry required by a breaking schema change.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangelogKey {
    pub schema: String,
    pub from_version: Version,
    pub to_version: Version,
}

/// Explicit, already-parsed evidence available to the release gate.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseEvidence {
    pub migrations: BTreeSet<MigrationKey>,
    pub fixture_pairs: BTreeSet<FixturePairKey>,
    pub changelog_entries: BTreeSet<ChangelogKey>,
}

/// Deterministic result of enforcing SemVer and breaking-change evidence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseReport {
    pub ok: bool,
    pub diagnostics: Vec<Diagnostic>,
}

/// Enforces exact SemVer transitions and explicit evidence without I/O.
#[must_use]
pub fn enforce_release(
    baseline: &CatalogResources,
    candidate: &CatalogResources,
    compatibility: &CompatibilityReport,
    evidence: &ReleaseEvidence,
) -> ReleaseReport {
    let source = diagnostic_source(&candidate.catalog_source);
    let impacts = schema_impacts(compatibility);
    let mut diagnostics = Vec::new();

    validate_stable_version(
        &mut diagnostics,
        &baseline.catalog.release_version,
        "/baseline/releaseVersion",
        source,
    );
    validate_stable_version(
        &mut diagnostics,
        &candidate.catalog.release_version,
        "/releaseVersion",
        source,
    );

    for (schema, baseline_entry) in &baseline.catalog.schemas {
        validate_stable_version(
            &mut diagnostics,
            &baseline_entry.document_version,
            &format!(
                "/baseline/schemas/{}/documentVersion",
                escape_pointer(schema)
            ),
            source,
        );
        if let Some(candidate_entry) = candidate.catalog.schemas.get(schema) {
            let impact = impacts.get(schema).copied().unwrap_or_default();
            validate_stable_version(
                &mut diagnostics,
                &candidate_entry.document_version,
                &format!("/schemas/{}/documentVersion", escape_pointer(schema)),
                source,
            );
            validate_exact_transition(
                &mut diagnostics,
                &baseline_entry.document_version,
                &candidate_entry.document_version,
                impact,
                &format!("/schemas/{}/documentVersion", escape_pointer(schema)),
                source,
            );
        } else {
            mismatch(
                &mut diagnostics,
                "milestone 02 does not define a schema removal or tombstone contract",
                &format!("/schemas/{}/removal", escape_pointer(schema)),
                source,
            );
        }
    }
    for (schema, candidate_entry) in &candidate.catalog.schemas {
        if !baseline.catalog.schemas.contains_key(schema) {
            validate_stable_version(
                &mut diagnostics,
                &candidate_entry.document_version,
                &format!("/schemas/{}/documentVersion", escape_pointer(schema)),
                source,
            );
            validate_initial_document_version(
                &mut diagnostics,
                &candidate_entry.document_version,
                &format!("/schemas/{}/documentVersion", escape_pointer(schema)),
                source,
            );
        }
    }

    let catalog_impact = impacts.values().copied().max().unwrap_or_default();
    validate_exact_transition(
        &mut diagnostics,
        &baseline.catalog.release_version,
        &candidate.catalog.release_version,
        catalog_impact,
        "/releaseVersion",
        source,
    );
    validate_breaking_evidence(
        &mut diagnostics,
        baseline,
        candidate,
        &impacts,
        evidence,
        source,
    );

    sort_diagnostics(&mut diagnostics);
    ReleaseReport {
        ok: diagnostics.is_empty(),
        diagnostics,
    }
}

fn schema_impacts(compatibility: &CompatibilityReport) -> BTreeMap<String, SemverImpact> {
    let mut impacts: BTreeMap<String, SemverImpact> = BTreeMap::new();
    for change in &compatibility.changes {
        impacts
            .entry(change.schema.clone())
            .and_modify(|current| *current = (*current).max(change.impact))
            .or_insert(change.impact);
    }
    impacts
}

fn validate_breaking_evidence(
    diagnostics: &mut Vec<Diagnostic>,
    baseline: &CatalogResources,
    candidate: &CatalogResources,
    impacts: &BTreeMap<String, SemverImpact>,
    evidence: &ReleaseEvidence,
    source: &str,
) {
    for (schema, impact) in impacts {
        if *impact != SemverImpact::Major {
            continue;
        }
        let Some(from_version) = baseline
            .catalog
            .schemas
            .get(schema)
            .map(|entry| &entry.document_version)
        else {
            continue;
        };
        let Some(to_version) = candidate
            .catalog
            .schemas
            .get(schema)
            .map(|entry| &entry.document_version)
        else {
            continue;
        };
        let migration = MigrationKey {
            schema: schema.clone(),
            from_version: from_version.clone(),
            to_version: to_version.clone(),
        };
        if !evidence.migrations.contains(&migration) {
            mismatch(
                diagnostics,
                "breaking schema requires an exact migration",
                &format!("/evidence/migrations/{}", escape_pointer(schema)),
                source,
            );
        }
        let fixture_pair = FixturePairKey {
            schema: schema.clone(),
            from_version: from_version.clone(),
            to_version: to_version.clone(),
        };
        if !evidence.fixture_pairs.contains(&fixture_pair) {
            mismatch(
                diagnostics,
                "breaking schema requires an exact before/after fixture pair",
                &format!("/evidence/fixturePairs/{}", escape_pointer(schema)),
                source,
            );
        }
        let changelog = ChangelogKey {
            schema: schema.clone(),
            from_version: from_version.clone(),
            to_version: to_version.clone(),
        };
        if !evidence.changelog_entries.contains(&changelog) {
            mismatch(
                diagnostics,
                "breaking schema requires an exact changelog entry",
                &format!("/evidence/changelogEntries/{}", escape_pointer(schema)),
                source,
            );
        }
    }
}

fn validate_stable_version(
    diagnostics: &mut Vec<Diagnostic>,
    version: &Version,
    path: &str,
    source: &str,
) {
    if !version.pre.is_empty() || !version.build.is_empty() {
        mismatch(
            diagnostics,
            "milestone 02 versions must not include prerelease or build metadata",
            path,
            source,
        );
    }
}

fn validate_initial_document_version(
    diagnostics: &mut Vec<Diagnostic>,
    version: &Version,
    path: &str,
    source: &str,
) {
    if version.major != 1 || version.minor != 0 || version.patch != 0 {
        mismatch(
            diagnostics,
            "new milestone 02 schemas must start at document version 1.0.0",
            path,
            source,
        );
    }
}

fn validate_exact_transition(
    diagnostics: &mut Vec<Diagnostic>,
    from: &Version,
    to: &Version,
    impact: SemverImpact,
    path: &str,
    source: &str,
) {
    let expected = expected_version(from, impact);
    if expected.as_ref() != Some(to) {
        mismatch(
            diagnostics,
            "version does not equal the exact required SemVer transition",
            path,
            source,
        );
    }
}

fn expected_version(from: &Version, impact: SemverImpact) -> Option<Version> {
    match impact {
        SemverImpact::None => Some(from.clone()),
        SemverImpact::Patch => from
            .patch
            .checked_add(1)
            .map(|patch| Version::new(from.major, from.minor, patch)),
        SemverImpact::Minor => from
            .minor
            .checked_add(1)
            .map(|minor| Version::new(from.major, minor, 0)),
        SemverImpact::Major => from
            .major
            .checked_add(1)
            .map(|major| Version::new(major, 0, 0)),
    }
}

fn mismatch(diagnostics: &mut Vec<Diagnostic>, message: &str, path: &str, source: &str) {
    diagnostics.push(Diagnostic::error(
        SEMVER_MISMATCH_CODE,
        message,
        path,
        source,
    ));
}

fn diagnostic_source(value: &str) -> &str {
    let normalized_relative = !value.is_empty()
        && !value.starts_with('/')
        && !value.contains('\\')
        && !value.contains(':')
        && value
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..");
    if normalized_relative {
        value
    } else {
        "schema-evolution"
    }
}

fn escape_pointer(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

/// #507: what two catalog evolutions from one frozen base collided on.
///
/// `schemas/catalog.json` carries one `releaseVersion`. Two branches that each bump it from the
/// same merge base can both be green in isolation and still land as one catalog carrying a
/// version nobody reviewed as a whole. The gate asks this on one captured candidate/landing/base
/// snapshot; a later publication must capture a new snapshot before making its own decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum CatalogVersionCollision {
    /// Both sides moved `releaseVersion` from `base` to the same number: two catalogs, one name.
    DuplicateVersion { base: Version, version: Version },
    /// Both sides moved `releaseVersion` from `base` to different numbers: whichever textual merge
    /// wins, the landed version was reviewed against neither sibling.
    DivergentVersions {
        base: Version,
        ours: Version,
        theirs: Version,
    },
}

/// Compares the branch's catalog version (`ours`) and the version now on the target (`theirs`)
/// against the version both grew from (`base`).
///
/// This pure predicate describes one observed snapshot only. Callers that make a merge decision
/// must capture the candidate, landing tip, and merge base first, then load all three catalogs from
/// those exact object IDs; this function cannot promise freshness for a future merge.
///
/// A side that left the version at `base` did not evolve the catalog and cannot collide; that is
/// the ordinary rebase case. Only two independent moves from one base are refused.
pub fn catalog_version_collision(
    base: &Version,
    ours: &Version,
    theirs: &Version,
) -> Option<CatalogVersionCollision> {
    if ours == base || theirs == base {
        return None;
    }
    Some(if ours == theirs {
        CatalogVersionCollision::DuplicateVersion {
            base: base.clone(),
            version: ours.clone(),
        }
    } else {
        CatalogVersionCollision::DivergentVersions {
            base: base.clone(),
            ours: ours.clone(),
            theirs: theirs.clone(),
        }
    })
}
