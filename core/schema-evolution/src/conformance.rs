use std::collections::{BTreeMap, BTreeSet};

use graphhelm_protocols::Diagnostic;
use semver::Version;
use serde::{Deserialize, Deserializer, Serialize, de};
use serde_json::{Map, Value};

use crate::{
    CatalogEntry, CatalogResources, ChangelogKey, CompatibilityChange, CompatibilityClass,
    CompatibilityReport, FixturePairKey, MAX_CONFORMANCE_CASES, MAX_SCHEMAS, MigrationCatalogs,
    MigrationKey, MigrationManifest, ReleaseEvidence, SchemaCatalog, SchemaDigest, SemverImpact,
    apply_migration, compare_catalogs, enforce_release, plan_migration_chain, schema_digest,
};

/// A strict, bounded manifest for public protocol conformance fixtures.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConformanceSuite {
    pub format_version: u32,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub validator_resources: BTreeMap<String, Vec<String>>,
    pub cases: Vec<ConformanceCase>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawConformanceSuite {
    format_version: u32,
    #[serde(default)]
    validator_resources: BTreeMap<String, Vec<String>>,
    cases: Vec<ConformanceCase>,
}

impl<'de> Deserialize<'de> for ConformanceSuite {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawConformanceSuite::deserialize(deserializer)?;
        if raw.format_version != 1 {
            return Err(de::Error::custom("formatVersion must equal 1"));
        }
        if raw.cases.len() > MAX_CONFORMANCE_CASES {
            return Err(de::Error::custom("conformance case limit exceeded"));
        }
        if raw.validator_resources.len() > MAX_SCHEMAS {
            return Err(de::Error::custom("validator registry limit exceeded"));
        }
        let mut validator_resource_count = 0usize;
        for (target, paths) in &raw.validator_resources {
            if !versioned_validation_target(target) || paths.is_empty() || !sorted_unique(paths) {
                return Err(de::Error::custom(
                    "versioned validator declaration is invalid",
                ));
            }
            validator_resource_count = validator_resource_count.saturating_add(paths.len());
            if validator_resource_count > MAX_SCHEMAS
                || paths.iter().any(|path| !safe_relative_path(path))
            {
                return Err(de::Error::custom(
                    "versioned validator resource declaration is invalid",
                ));
            }
        }
        for (index, case) in raw.cases.iter().enumerate() {
            if !canonical_case_id(case.id()) {
                return Err(de::Error::custom("conformance case id is invalid"));
            }
            if index > 0 && raw.cases[index - 1].id() >= case.id() {
                return Err(de::Error::custom(
                    "conformance case ids must be unique and lexicographically sorted",
                ));
            }
            if case
                .paths()
                .into_iter()
                .any(|path| !safe_relative_path(path))
            {
                return Err(de::Error::custom(
                    "conformance fixture path must be repository-relative and normalized",
                ));
            }
            if !sorted_unique(&case.expect().codes) || !sorted_unique(&case.expect().paths) {
                return Err(de::Error::custom(
                    "expected diagnostic codes and paths must be unique and sorted",
                ));
            }
        }
        Ok(Self {
            format_version: raw.format_version,
            validator_resources: raw.validator_resources,
            cases: raw.cases,
        })
    }
}

/// One supported pure conformance operation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ConformanceCase {
    Schema {
        id: String,
        schema: String,
        input: String,
        expect: ConformanceExpectation,
    },
    Compatibility {
        id: String,
        input: String,
        expect: ConformanceExpectation,
    },
    Release {
        id: String,
        input: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        comparison: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        schema: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from_version: Option<Version>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to_version: Option<Version>,
        expect: ConformanceExpectation,
    },
    Migration {
        id: String,
        input: String,
        expect: ConformanceExpectation,
    },
}

impl ConformanceCase {
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Schema { id, .. }
            | Self::Compatibility { id, .. }
            | Self::Release { id, .. }
            | Self::Migration { id, .. } => id,
        }
    }

    fn expect(&self) -> &ConformanceExpectation {
        match self {
            Self::Schema { expect, .. }
            | Self::Compatibility { expect, .. }
            | Self::Release { expect, .. }
            | Self::Migration { expect, .. } => expect,
        }
    }

    fn paths(&self) -> Vec<&str> {
        match self {
            Self::Schema { input, .. }
            | Self::Compatibility { input, .. }
            | Self::Migration { input, .. } => vec![input],
            Self::Release {
                input, comparison, ..
            } => comparison
                .iter()
                .map(String::as_str)
                .chain(std::iter::once(input.as_str()))
                .collect(),
        }
    }
}

/// The exact outcome declared by one conformance case.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConformanceExpectation {
    pub ok: bool,
    #[serde(default)]
    pub codes: Vec<String>,
    #[serde(default)]
    pub paths: Vec<String>,
}

/// Already-parsed fixture documents supplied by the I/O-owning caller.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConformanceResources {
    pub fixtures: BTreeMap<String, Value>,
}

/// Deterministic metadata for one conformance dispatch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConformanceCaseMetadata {
    pub kind: String,
}

/// Payload-free result of one manifest case.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConformanceCaseResult {
    pub id: String,
    pub passed: bool,
    pub diagnostic_codes: Vec<String>,
    pub diagnostic_paths: Vec<String>,
    pub metadata: ConformanceCaseMetadata,
}

/// Deterministic aggregate conformance report.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConformanceReport {
    pub ok: bool,
    pub total: u32,
    pub passed: u32,
    pub failed: u32,
    pub cases: Vec<ConformanceCaseResult>,
    pub diagnostics: Vec<Diagnostic>,
}

struct ActualOutcome {
    ok: bool,
    codes: Vec<String>,
    paths: Vec<String>,
}

/// Runs a strict suite using only already-parsed resources and a caller-owned validator.
///
/// Schema cases pass their stable schema name to `validate`. Migration cases pass the
/// deterministic version-qualified target `{schema}@{semver}` for both source and destination so
/// callers can select exact version registries even though protocol schemas preserve stable `$id`s.
#[must_use]
pub fn run_conformance<V>(
    suite: &ConformanceSuite,
    resources: &ConformanceResources,
    validate: V,
) -> ConformanceReport
where
    V: Fn(&str, &Value) -> Vec<Diagnostic>,
{
    if suite.cases.len() > MAX_CONFORMANCE_CASES {
        return ConformanceReport {
            ok: false,
            total: 0,
            passed: 0,
            failed: 0,
            cases: Vec::new(),
            diagnostics: vec![Diagnostic::error(
                "GHCONF001_FIXTURE_FAILED",
                "conformance suite exceeds the case limit",
                "/cases",
                "conformance",
            )],
        };
    }
    let mut cases = Vec::with_capacity(suite.cases.len());
    let mut diagnostics = Vec::new();
    let mut ordered_cases = suite.cases.iter().collect::<Vec<_>>();
    ordered_cases.sort_by(|left, right| left.id().cmp(right.id()));
    for (index, case) in ordered_cases.into_iter().enumerate() {
        let kind = case.kind();
        let id_is_canonical = canonical_case_id(case.id());
        let actual = if id_is_canonical {
            match case {
                ConformanceCase::Schema { schema, input, .. } => resources
                    .fixtures
                    .get(input)
                    .map(|document| outcome(validate(schema, document))),
                ConformanceCase::Compatibility { input, .. } => resources
                    .fixtures
                    .get(input)
                    .and_then(compatibility_outcome),
                ConformanceCase::Release {
                    input,
                    comparison,
                    schema,
                    from_version,
                    to_version,
                    ..
                } => release_outcome(
                    resources,
                    input,
                    comparison.as_deref(),
                    schema.as_deref(),
                    from_version.as_ref(),
                    to_version.as_ref(),
                ),
                ConformanceCase::Migration { input, .. } => resources
                    .fixtures
                    .get(input)
                    .and_then(|fixture| migration_outcome(fixture, &validate)),
            }
        } else {
            None
        };
        let expected = case.expect();
        let dispatch_succeeded = actual.is_some();
        let actual = actual.unwrap_or_else(empty_failure);
        let passed = dispatch_succeeded
            && actual.ok == expected.ok
            && actual.codes == expected.codes
            && (expected.paths.is_empty() || actual.paths == expected.paths);
        let mut codes = actual.codes;
        let mut paths = actual.paths;
        if !passed {
            let path = format!("/cases/{index}");
            diagnostics.push(Diagnostic::error(
                "GHCONF001_FIXTURE_FAILED",
                "fixture actual result differs from its declared expectation",
                &path,
                "conformance",
            ));
            codes.push("GHCONF001_FIXTURE_FAILED".into());
            paths.push(path);
            codes.sort();
            codes.dedup();
            paths.sort();
            paths.dedup();
        }
        cases.push(ConformanceCaseResult {
            id: if id_is_canonical {
                case.id().to_owned()
            } else {
                "invalid-case".into()
            },
            passed,
            diagnostic_codes: codes,
            diagnostic_paths: paths,
            metadata: ConformanceCaseMetadata { kind: kind.into() },
        });
    }
    let total = u32::try_from(cases.len()).expect("suite is bounded to u32");
    let passed = u32::try_from(cases.iter().filter(|case| case.passed).count())
        .expect("suite is bounded to u32");
    ConformanceReport {
        ok: diagnostics.is_empty(),
        total,
        passed,
        failed: total - passed,
        cases,
        diagnostics,
    }
}

fn empty_failure() -> ActualOutcome {
    ActualOutcome {
        ok: false,
        codes: Vec::new(),
        paths: Vec::new(),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ComparisonFixture {
    baseline: Value,
    candidate: Value,
}

fn compatibility_outcome(fixture: &Value) -> Option<ActualOutcome> {
    let fixture = serde_json::from_value::<ComparisonFixture>(fixture.clone()).ok()?;
    let baseline = comparison_resources("1.0.0", fixture.baseline)?;
    let candidate = comparison_resources("1.0.0", fixture.candidate)?;
    let report = compare_catalogs(&baseline, &candidate);
    Some(ActualOutcome {
        ok: report.compatible,
        codes: report
            .changes
            .iter()
            .map(|change| change.code.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
        paths: report
            .changes
            .iter()
            .map(|change| change.pointer.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SemverFixture {
    baseline_version: Version,
    candidate_version: Version,
    impact: String,
    expect: FixtureExpectation,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MissingEvidenceFixture {
    schema: String,
    from_version: Version,
    to_version: Version,
    missing: String,
    expect: FixtureExpectation,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureExpectation {
    ok: bool,
    codes: Vec<String>,
    #[serde(default)]
    document: Option<Value>,
}

fn release_outcome(
    resources: &ConformanceResources,
    input: &str,
    comparison: Option<&str>,
    schema: Option<&str>,
    from_version: Option<&Version>,
    to_version: Option<&Version>,
) -> Option<ActualOutcome> {
    let fixture = resources.fixtures.get(input)?;
    if fixture.get("baselineVersion").is_some() {
        let fixture = serde_json::from_value::<SemverFixture>(fixture.clone()).ok()?;
        let impact = parse_impact(&fixture.impact)?;
        let baseline = versioned_resources(
            "graph",
            &fixture.baseline_version,
            Value::Object(Map::new()),
        )?;
        let candidate = versioned_resources(
            "graph",
            &fixture.candidate_version,
            Value::Object(Map::new()),
        )?;
        let report = enforce_release(
            &baseline,
            &candidate,
            &compatibility_for_impact("graph", impact),
            &ReleaseEvidence::default(),
        );
        let actual = outcome(report.diagnostics);
        return fixture.expect.matches(&actual).then_some(actual);
    }

    let fixture = serde_json::from_value::<MissingEvidenceFixture>(fixture.clone()).ok()?;
    if schema != Some(fixture.schema.as_str())
        || from_version != Some(&fixture.from_version)
        || to_version != Some(&fixture.to_version)
    {
        return None;
    }
    let comparison = resources.fixtures.get(comparison?)?;
    let comparison = serde_json::from_value::<ComparisonFixture>(comparison.clone()).ok()?;
    let baseline =
        versioned_resources(&fixture.schema, &fixture.from_version, comparison.baseline)?;
    let candidate =
        versioned_resources(&fixture.schema, &fixture.to_version, comparison.candidate)?;
    let compatibility = compare_catalogs(&baseline, &candidate);
    let mut evidence = exact_evidence(&fixture.schema, &fixture.from_version, &fixture.to_version);
    match fixture.missing.as_str() {
        "migration" => evidence.migrations.clear(),
        "fixturePair" => evidence.fixture_pairs.clear(),
        "changelog" => evidence.changelog_entries.clear(),
        _ => return None,
    }
    let actual =
        outcome(enforce_release(&baseline, &candidate, &compatibility, &evidence).diagnostics);
    fixture.expect.matches(&actual).then_some(actual)
}

fn parse_impact(value: &str) -> Option<SemverImpact> {
    match value {
        "none" => Some(SemverImpact::None),
        "patch" => Some(SemverImpact::Patch),
        "minor" => Some(SemverImpact::Minor),
        "major" => Some(SemverImpact::Major),
        _ => None,
    }
}

fn compatibility_for_impact(schema: &str, impact: SemverImpact) -> CompatibilityReport {
    let (class, code) = match impact {
        SemverImpact::None => (CompatibilityClass::Unchanged, "GHC103_COMPATIBLE_CHANGE"),
        SemverImpact::Patch => (CompatibilityClass::Annotation, "GHC101_ANNOTATION_CHANGED"),
        SemverImpact::Minor => (
            CompatibilityClass::Compatible,
            "GHC102_OPTIONAL_PROPERTY_ADDED",
        ),
        SemverImpact::Major => (CompatibilityClass::Breaking, "GHC003_BREAKING_CHANGE"),
    };
    let changes = (impact != SemverImpact::None)
        .then(|| CompatibilityChange {
            schema: schema.into(),
            code: code.into(),
            pointer: "/".into(),
            baseline_summary: "baseline constraint".into(),
            candidate_summary: "candidate constraint".into(),
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

fn exact_evidence(schema: &str, from: &Version, to: &Version) -> ReleaseEvidence {
    ReleaseEvidence {
        migrations: BTreeSet::from([MigrationKey {
            schema: schema.into(),
            from_version: from.clone(),
            to_version: to.clone(),
        }]),
        fixture_pairs: BTreeSet::from([FixturePairKey {
            schema: schema.into(),
            from_version: from.clone(),
            to_version: to.clone(),
        }]),
        changelog_entries: BTreeSet::from([ChangelogKey {
            schema: schema.into(),
            from_version: from.clone(),
            to_version: to.clone(),
        }]),
    }
}

fn comparison_resources(version: &str, document: Value) -> Option<CatalogResources> {
    versioned_resources("graph", &Version::parse(version).ok()?, document)
}

fn versioned_resources(
    schema: &str,
    version: &Version,
    document: Value,
) -> Option<CatalogResources> {
    let mut document = document.as_object().cloned()?;
    let id = format!("https://p50.dev/schemas/{schema}.schema.json");
    document.insert("$id".into(), Value::String(id.clone()));
    document.insert(
        "x-graphhelm-schema-version".into(),
        Value::String(version.to_string()),
    );
    let document = Value::Object(document);
    let digest = schema_digest(&document).ok()?;
    Some(CatalogResources {
        catalog_source: "conformance/catalog.json".into(),
        catalog: SchemaCatalog {
            format_version: 1,
            release_version: version.clone(),
            schemas: BTreeMap::from([(
                schema.into(),
                CatalogEntry {
                    id,
                    document_version: version.clone(),
                    path: format!("schemas/{schema}.schema.json"),
                    sha256: digest,
                },
            )]),
        },
        schemas: BTreeMap::from([(schema.into(), document)]),
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ApplyMigrationFixture {
    manifest: MigrationManifest,
    source_catalog_hash: SchemaDigest,
    target_catalog_hash: SchemaDigest,
    input: Value,
    expect: FixtureExpectation,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ChainMigrationFixture {
    schema: String,
    from_version: Version,
    to_version: Version,
    manifests: Vec<MigrationManifest>,
    expect: FixtureExpectation,
}

fn migration_outcome<V>(fixture: &Value, validate: &V) -> Option<ActualOutcome>
where
    V: Fn(&str, &Value) -> Vec<Diagnostic>,
{
    if fixture.get("manifests").is_some() {
        let fixture = serde_json::from_value::<ChainMigrationFixture>(fixture.clone()).ok()?;
        let diagnostics = plan_migration_chain(
            &fixture.schema,
            &fixture.from_version,
            &fixture.to_version,
            &fixture.manifests,
        )
        .err()
        .unwrap_or_default();
        let actual = outcome(diagnostics);
        return fixture.expect.matches(&actual).then_some(actual);
    }
    let fixture = serde_json::from_value::<ApplyMigrationFixture>(fixture.clone()).ok()?;
    let source = migration_catalog(
        &fixture.manifest.schema,
        &fixture.manifest.from_version,
        fixture.source_catalog_hash,
    );
    let target = migration_catalog(
        &fixture.manifest.schema,
        &fixture.manifest.to_version,
        fixture.target_catalog_hash,
    );
    let source_validation_target = format!(
        "{}@{}",
        fixture.manifest.schema, fixture.manifest.from_version
    );
    let target_validation_target = format!(
        "{}@{}",
        fixture.manifest.schema, fixture.manifest.to_version
    );
    let result = apply_migration(
        &fixture.input,
        &fixture.manifest,
        &MigrationCatalogs {
            source: &source,
            target: &target,
        },
        |document| validate(&source_validation_target, document),
        |document| validate(&target_validation_target, document),
    );
    let document_matches = fixture
        .expect
        .document
        .as_ref()
        .is_none_or(|expected| result.document.as_ref() == Some(expected));
    let actual = outcome(result.diagnostics);
    (document_matches && fixture.expect.matches(&actual)).then_some(actual)
}

impl FixtureExpectation {
    fn matches(&self, actual: &ActualOutcome) -> bool {
        self.ok == actual.ok && self.codes == actual.codes
    }
}

fn migration_catalog(schema: &str, version: &Version, digest: SchemaDigest) -> SchemaCatalog {
    SchemaCatalog {
        format_version: 1,
        release_version: version.clone(),
        schemas: BTreeMap::from([(
            schema.into(),
            CatalogEntry {
                id: format!("https://p50.dev/schemas/{schema}.schema.json"),
                document_version: version.clone(),
                path: format!("schemas/{schema}.schema.json"),
                sha256: digest,
            },
        )]),
    }
}

fn outcome(diagnostics: Vec<Diagnostic>) -> ActualOutcome {
    let codes = diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let paths = diagnostics
        .iter()
        .map(|diagnostic| diagnostic.path.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    ActualOutcome {
        ok: diagnostics.is_empty(),
        codes,
        paths,
    }
}

fn safe_relative_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains(':')
        && path
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

fn sorted_unique(values: &[String]) -> bool {
    values.windows(2).all(|pair| pair[0] < pair[1])
}

fn versioned_validation_target(target: &str) -> bool {
    let Some((schema, version)) = target.split_once('@') else {
        return false;
    };
    if schema.is_empty()
        || schema.contains('@')
        || !schema
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        || !schema.as_bytes()[0].is_ascii_lowercase()
    {
        return false;
    }
    Version::parse(version).is_ok_and(|version| version.pre.is_empty() && version.build.is_empty())
}

fn canonical_case_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    if bytes.is_empty() || bytes.len() > 128 {
        return false;
    }
    let alphanumeric = |byte: u8| byte.is_ascii_lowercase() || byte.is_ascii_digit();
    let separator = |byte: u8| matches!(byte, b'.' | b'-' | b'_');
    if !alphanumeric(bytes[0]) || !alphanumeric(bytes[bytes.len() - 1]) {
        return false;
    }
    let mut previous_separator = false;
    for &byte in bytes {
        if alphanumeric(byte) {
            previous_separator = false;
        } else if separator(byte) && !previous_separator {
            previous_separator = true;
        } else {
            return false;
        }
    }
    true
}

impl ConformanceCase {
    fn kind(&self) -> &'static str {
        match self {
            Self::Schema { .. } => "schema",
            Self::Compatibility { .. } => "compatibility",
            Self::Release { .. } => "release",
            Self::Migration { .. } => "migration",
        }
    }
}
