//! #229: where the compatibility baseline comes from, and what it can therefore refuse.
//!
//! `ci/gate.ps1` runs one stage as `schema check --baseline schemas/releases/1.0.0/catalog.json
//! --candidate schemas/catalog.json`. Read alone that is a release gate. Measured, its two inputs
//! held BYTE-IDENTICAL for the fifteen schemas published in `1.0.0` by a sibling test in the same
//! gate. The current catalog is now a real candidate: `1.1.0` adds the execution-accounting receipt
//! schema while the frozen release directory stays untouched. The compatibility stage must report
//! that one additive minor change and still refuse a breaking change to any shared schema.
//!
//! So this file does two separate things, and they are not the same claim:
//!
//! 1. `the_gate_baseline_records_the_one_additive_current_schema` pins the exact intended delta.
//! 2. `no_silent_breaking_change_against_what_landed_on_main` supplies a baseline the branch
//!    CANNOT edit -- the merge base with `origin/main`, read out of git -- so that a comparison
//!    which can actually refuse exists somewhere in the gate.
//!
//! The byte-identity sibling remains strict for every schema already published in `1.0.0`; the new
//! schema is absent from that immutable snapshot by design.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use graphhelm_schema_evolution::{
    CatalogResources, CompatibilityChange, CompatibilityClass, CompatibilityReport, SchemaCatalog,
    SemverImpact, compare_catalogs,
};
use semver::Version;
use serde_json::{Value, json};

const LIVE_CATALOG: &str = "schemas/catalog.json";
const RELEASE_CATALOG: &str = "schemas/releases/1.0.0/catalog.json";
const D037_CUSTOMS_BUDGET_POINTERS: [&str; 3] = [
    "/properties/completion/properties/customs/properties/budgets/properties/waitWithinSeconds/maximum",
    "/properties/completion/properties/customs/properties/budgets/properties/clearanceWithinSeconds/maximum",
    "/properties/completion/properties/customs/properties/budgets/properties/dlqWithinSeconds/maximum",
];

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A catalog plus every schema it names, read from the working tree at the catalog's own paths.
fn checked_in_catalog(catalog_path: &str) -> CatalogResources {
    let root = repository_root();
    let catalog: SchemaCatalog =
        serde_json::from_slice(&fs::read(root.join(catalog_path)).unwrap()).unwrap();
    let schemas = catalog
        .schemas
        .iter()
        .map(|(name, entry)| {
            let document: Value =
                serde_json::from_slice(&fs::read(root.join(&entry.path)).unwrap()).unwrap();
            (name.clone(), document)
        })
        .collect();
    CatalogResources {
        catalog_source: catalog_path.into(),
        catalog,
        schemas,
    }
}

fn git(root: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .ok()?;
    output.status.success().then_some(output.stdout)
}

fn merge_base(root: &Path, reference: &str) -> Option<String> {
    let stdout = git(root, &["merge-base", "HEAD", reference])?;
    let base = String::from_utf8(stdout).ok()?.trim().to_owned();
    (!base.is_empty()).then_some(base)
}

/// The commit this branch grew from. `origin/main` first, local `main` only as a fallback: a
/// stale local ref is a cache, never the authority.
///
/// Refuses rather than skips. A gate whose baseline could not be resolved has not compared
/// anything, and reporting that as a pass is the exact defect this file exists to remove -- an
/// unanswerable question is not evidence of a compatible branch.
fn branch_point(root: &Path) -> String {
    merge_base(root, "origin/main")
        .or_else(|| merge_base(root, "main"))
        .unwrap_or_else(|| {
            panic!(
                "cannot ask whether this branch breaks the landed schemas: git answered neither \
                 `merge-base HEAD origin/main` nor `merge-base HEAD main`. Refusing rather than \
                 passing."
            )
        })
}

/// A baseline with an origin the branch cannot edit: the catalog and schema blobs as they stand
/// at `commit`, addressed by the paths THAT COMMIT's catalog records rather than today's.
///
/// Parsed as JSON, never compared as bytes -- `git show` yields the blob as stored while the
/// working tree may carry CRLF, and a line-ending difference is not a schema change.
fn catalog_at(root: &Path, commit: &str) -> CatalogResources {
    let source = format!("{commit}:{LIVE_CATALOG}");
    let raw = git(root, &["show", &source]).unwrap_or_else(|| {
        panic!(
            "git could not read {source}; refusing to compare against a baseline that was never \
             loaded"
        )
    });
    let catalog: SchemaCatalog = serde_json::from_slice(&raw).unwrap();
    let schemas = catalog
        .schemas
        .iter()
        .map(|(name, entry)| {
            let blob = format!("{commit}:{}", entry.path);
            let raw = git(root, &["show", &blob]).unwrap_or_else(|| {
                panic!("git could not read {blob}, named by the catalog at {commit}")
            });
            let document: Value = serde_json::from_slice(&raw).unwrap();
            (name.clone(), document)
        })
        .collect();
    CatalogResources {
        catalog_source: source,
        catalog,
        schemas,
    }
}

fn document_version(resources: &CatalogResources, schema: &str) -> Option<Version> {
    resources
        .catalog
        .schemas
        .get(schema)
        .map(|entry| entry.document_version.clone())
}

fn candidate_with_customs_budget_maximum(
    baseline: &CatalogResources,
    pointers: &[&str],
    maximum: u64,
) -> CatalogResources {
    let mut candidate = baseline.clone();
    let node = candidate.schemas.get_mut("node").unwrap();
    for pointer in pointers {
        let (parent, keyword) = pointer.rsplit_once('/').unwrap();
        node.pointer_mut(parent)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert(keyword.to_owned(), json!(maximum));
    }
    candidate
}

fn d037_post_correction_fixture(checked_in: &CatalogResources) -> CatalogResources {
    let mut post_d037_live = checked_in.clone();
    let node = post_d037_live.schemas.get_mut("node").unwrap();
    for pointer in D037_CUSTOMS_BUDGET_POINTERS {
        let (parent, keyword) = pointer.rsplit_once('/').unwrap();
        let budget = node.pointer_mut(parent).unwrap().as_object_mut().unwrap();
        if let Some(existing) = budget.get(keyword) {
            assert_eq!(
                existing,
                &json!(315_576_000_u64),
                "D-037 live fixture carries an unapproved maximum at {pointer}"
            );
        } else {
            budget.insert(keyword.to_owned(), json!(315_576_000_u64));
        }
    }
    post_d037_live
}

/// Exercises both repository states without overwriting either: before #250 the first pass adds
/// the approved maxima, while after #250 both passes validate the values already checked in.
fn d037_live_after_correction_fixture(checked_in: &CatalogResources) -> CatalogResources {
    let simulated_live = d037_post_correction_fixture(checked_in);
    d037_post_correction_fixture(&simulated_live)
}

/// Reconstructs the node schema immediately before D-037 from a candidate that already carries
/// the approved correction. This keeps the exception tests stable when the checked-in live schema
/// advances to that candidate while still refusing to normalize any other value.
fn d037_pre_correction_baseline(post_d037_live: &CatalogResources) -> CatalogResources {
    let mut baseline = post_d037_live.clone();
    let node = baseline.schemas.get_mut("node").unwrap();
    for pointer in D037_CUSTOMS_BUDGET_POINTERS {
        let (parent, keyword) = pointer.rsplit_once('/').unwrap();
        let removed = node
            .pointer_mut(parent)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove(keyword);
        assert_eq!(
            removed,
            Some(json!(315_576_000_u64)),
            "D-037 post-correction fixture must carry the approved maximum at {pointer}"
        );
    }
    baseline
}

/// D-037/ADR-023 allows this one correction to the unpublished 1.0.0 baseline. Match the
/// comparison identity and the actual JSON values; summaries are diagnostic prose, not authority.
fn is_d037_customs_budget_baseline_correction(
    baseline: &CatalogResources,
    candidate: &CatalogResources,
    breaking: &[&CompatibilityChange],
) -> bool {
    if breaking.len() != D037_CUSTOMS_BUDGET_POINTERS.len()
        || breaking.iter().any(|change| {
            change.schema != "node"
                || change.code != "GHC003_BREAKING_CHANGE"
                || !D037_CUSTOMS_BUDGET_POINTERS.contains(&change.pointer.as_str())
        })
    {
        return false;
    }

    let baseline_node = baseline.schemas.get("node").unwrap();
    let candidate_node = candidate.schemas.get("node").unwrap();
    D037_CUSTOMS_BUDGET_POINTERS.iter().all(|pointer| {
        breaking.iter().any(|change| change.pointer == *pointer)
            && baseline_node.pointer(pointer).is_none()
            && candidate_node.pointer(pointer) == Some(&json!(315_576_000_u64))
    })
}

fn undeclared_breaking_changes<'a>(
    baseline: &CatalogResources,
    candidate: &CatalogResources,
    report: &'a CompatibilityReport,
) -> Vec<&'a CompatibilityChange> {
    let breaking = report
        .changes
        .iter()
        .filter(|change| change.class == CompatibilityClass::Breaking)
        .collect::<Vec<_>>();
    let d037_correction =
        is_d037_customs_budget_baseline_correction(baseline, candidate, &breaking);

    breaking
        .into_iter()
        .filter(|change| {
            // "Declared" means the schema moved its OWN documentVersion to announce the break. A
            // removed schema has no version left to declare anything in, so `(Some, None)` is
            // undeclared -- comparing the two options directly would read `Some(1.0.0) != None`
            // as a declaration and wave the most breaking change there is straight through.
            match (
                document_version(baseline, &change.schema),
                document_version(candidate, &change.schema),
            ) {
                (Some(landed), Some(proposed)) => landed == proposed,
                _ => true,
            }
        })
        .filter(|_| !d037_correction)
        .collect()
}

#[test]
#[should_panic(expected = "D-037 live fixture carries an unapproved maximum")]
fn d037_fixture_rejects_an_existing_unapproved_live_maximum() {
    let checked_in = checked_in_catalog(LIVE_CATALOG);
    let drifted_live = candidate_with_customs_budget_maximum(
        &checked_in,
        &D037_CUSTOMS_BUDGET_POINTERS[..1],
        315_576_001,
    );

    let _ = d037_post_correction_fixture(&drifted_live);
}

#[test]
fn d037_accepts_only_the_exact_three_customs_budget_maxima() {
    let checked_in = checked_in_catalog(LIVE_CATALOG);
    let post_d037_live = d037_live_after_correction_fixture(&checked_in);
    let baseline = d037_pre_correction_baseline(&post_d037_live);
    let candidate = post_d037_live;
    let report = compare_catalogs(&baseline, &candidate);
    let undeclared = undeclared_breaking_changes(&baseline, &candidate, &report);

    assert!(
        undeclared.is_empty(),
        "D-037's exact unpublished baseline correction was rejected: {undeclared:#?}"
    );
}

#[test]
fn d037_customs_budget_exception_rejects_every_near_miss() {
    let checked_in = checked_in_catalog(LIVE_CATALOG);
    let post_d037_live = d037_live_after_correction_fixture(&checked_in);
    let baseline = d037_pre_correction_baseline(&post_d037_live);
    let oversized = candidate_with_customs_budget_maximum(
        &baseline,
        &D037_CUSTOMS_BUDGET_POINTERS,
        315_576_001,
    );
    let partial = candidate_with_customs_budget_maximum(
        &baseline,
        &D037_CUSTOMS_BUDGET_POINTERS[..2],
        315_576_000,
    );
    let mut fourth = candidate_with_customs_budget_maximum(
        &baseline,
        &D037_CUSTOMS_BUDGET_POINTERS,
        315_576_000,
    );
    fourth
        .schemas
        .get_mut("node")
        .unwrap()
        .pointer_mut("/properties/timeoutSeconds")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .insert("maximum".into(), json!(315_576_000_u64));
    let mut removed = candidate_with_customs_budget_maximum(
        &baseline,
        &D037_CUSTOMS_BUDGET_POINTERS,
        315_576_000,
    );
    removed.schemas.remove("agent");

    for (name, candidate, expected_breaking) in [
        ("315576001", oversized, 3),
        ("only two fields", partial, 2),
        ("fourth breaking change", fourth, 4),
        ("schema removal", removed, 5),
    ] {
        let report = compare_catalogs(&baseline, &candidate);
        let undeclared = undeclared_breaking_changes(&baseline, &candidate, &report);
        assert_eq!(
            undeclared.len(),
            expected_breaking,
            "D-037 exception accepted sabotage cell {name}: {undeclared:#?}"
        );
    }
}

// Pins the deliberate divergence: one additive schema and no mutation of the published set.
#[test]
fn the_gate_baseline_records_the_one_additive_current_schema() {
    let baseline = checked_in_catalog(RELEASE_CATALOG);
    let candidate = checked_in_catalog(LIVE_CATALOG);

    assert!(
        !baseline.schemas.is_empty() && !candidate.schemas.is_empty(),
        "both sides must carry documents before their agreement means anything"
    );
    assert_eq!(
        baseline.schemas.len(),
        baseline.catalog.schemas.len(),
        "baseline catalog names entries whose documents did not load"
    );

    let report = compare_catalogs(&baseline, &candidate);
    assert_eq!(
        (report.class, report.impact, report.changes.len()),
        (CompatibilityClass::Compatible, SemverImpact::Minor, 1),
        "the current catalog must differ from 1.0.0 by exactly one additive minor schema"
    );
    assert_eq!(report.changes[0].schema, "execution-accounting-receipt");
    assert_eq!(report.changes[0].pointer, "/");

    // Positive control: the silence above is the subject's, not the instrument's.
    let mut divergent = candidate.clone();
    let removed = divergent.schemas.keys().next().unwrap().clone();
    divergent.schemas.remove(&removed);
    assert_eq!(
        compare_catalogs(&baseline, &divergent).class,
        CompatibilityClass::Breaking,
        "compare_catalogs did not report a removed schema as breaking, so its additive verdict \
         above says nothing about the published contracts"
    );
}

// Prevents a branch from breaking a schema that already landed without declaring it in the
// schema's own documentVersion.
#[test]
fn no_silent_breaking_change_against_what_landed_on_main() {
    let root = repository_root();
    let base = branch_point(&root);
    let baseline = catalog_at(&root, &base);
    let candidate = checked_in_catalog(LIVE_CATALOG);

    // Subset-of-empty: an empty or partial baseline makes every candidate schema read as ADDED,
    // which compare_catalogs classes as compatible. A failed extraction would pass this test
    // silently, so the extraction is checked before its result is trusted. See
    // `an_empty_baseline_reads_as_compatible_so_the_extraction_is_checked_first`.
    assert!(
        !baseline.schemas.is_empty(),
        "no baseline schemas were read out of {base}"
    );
    assert_eq!(
        baseline.schemas.len(),
        baseline.catalog.schemas.len(),
        "the catalog at {base} names entries whose documents did not load"
    );

    let report = compare_catalogs(&baseline, &candidate);
    let undeclared = undeclared_breaking_changes(&baseline, &candidate, &report)
        .into_iter()
        .map(|change| {
            format!(
                "{} at {} ({}): {} -> {}",
                change.schema,
                change.pointer,
                change.code,
                change.baseline_summary,
                change.candidate_summary
            )
        })
        .collect::<Vec<_>>();

    assert!(
        undeclared.is_empty(),
        "this branch breaks {} schema contract(s) that landed at {base}, without moving the \
         schema's documentVersion to say so:\n  {}",
        undeclared.len(),
        undeclared.join("\n  ")
    );
}

// Names why the test above counts its baseline before trusting it.
#[test]
fn an_empty_baseline_reads_as_compatible_so_the_extraction_is_checked_first() {
    let candidate = checked_in_catalog(LIVE_CATALOG);
    let empty = CatalogResources {
        catalog_source: "<no baseline was loaded>".into(),
        catalog: SchemaCatalog {
            format_version: candidate.catalog.format_version,
            release_version: candidate.catalog.release_version.clone(),
            schemas: BTreeMap::new(),
        },
        schemas: BTreeMap::new(),
    };

    let report = compare_catalogs(&empty, &candidate);
    assert!(
        report.compatible && report.class != CompatibilityClass::Breaking,
        "an empty baseline no longer reads as compatible; the count assertions guarding the \
         extraction can be re-argued, but not silently dropped"
    );
}
