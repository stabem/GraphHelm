//! #229: where the compatibility baseline comes from, and what it can therefore refuse.
//!
//! `ci/gate.ps1` runs one stage as `schema check --baseline schemas/releases/1.0.0/catalog.json
//! --candidate schemas/catalog.json`. Read alone that is a release gate. Measured, its two inputs
//! are held BYTE-IDENTICAL by a sibling test in the same gate --
//! `catalog_integrity.rs::checked_in_1_0_0_release_is_complete_and_raw_byte_identical` asserts
//! `fs::read(live) == fs::read(released)` for all fifteen schemas, plus digest equality. The
//! divergence that stage exists to find is therefore not merely unobserved: it is FORBIDDEN
//! upstream, and any branch that produced it would go red on the sibling first. That happened
//! once, live -- #339 diverged `extension.schema.json`, and hotfix #365 reverted it to the frozen
//! bytes rather than repin.
//!
//! So this file does two separate things, and they are not the same claim:
//!
//! 1. `the_gate_baseline_is_a_mirror_the_repo_itself_enforces` records the impossibility with a
//!    positive control, and stands as the tripwire for the day it stops being true.
//! 2. `no_silent_breaking_change_against_what_landed_on_main` supplies a baseline the branch
//!    CANNOT edit -- the merge base with `origin/main`, read out of git -- so that a comparison
//!    which can actually refuse exists somewhere in the gate.
//!
//! What is deliberately NOT done here: nothing weakens or retires the byte-identity sibling.
//! Whether `schemas/releases/1.0.0/` should become a genuinely frozen snapshot is the milestone
//! decision #229 reserved, and taking it means retiring the guard that caught #339.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use graphhelm_schema_evolution::{
    CatalogResources, CompatibilityClass, SchemaCatalog, compare_catalogs,
};
use semver::Version;
use serde_json::Value;

const LIVE_CATALOG: &str = "schemas/catalog.json";
const RELEASE_CATALOG: &str = "schemas/releases/1.0.0/catalog.json";

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

// Records that the gate's checked-in baseline tracks its own candidate, so that stage cannot
// refuse -- and fires the day that stops being true.
#[test]
fn the_gate_baseline_is_a_mirror_the_repo_itself_enforces() {
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
        (report.class, report.changes.len()),
        (CompatibilityClass::Unchanged, 0),
        "the two checked-in catalogs have diverged. That is not a bug in this test: it means the \
         mirror #229 measured no longer holds, so the `schema baseline compatibility` stage in \
         ci/gate.ps1 has become capable of refusing, and its --baseline argument and the \
         byte-identity sibling in catalog_integrity.rs both need revisiting."
    );

    // Positive control: the silence above is the subject's, not the instrument's.
    let mut divergent = candidate.clone();
    let removed = divergent.schemas.keys().next().unwrap().clone();
    divergent.schemas.remove(&removed);
    assert_eq!(
        compare_catalogs(&baseline, &divergent).class,
        CompatibilityClass::Breaking,
        "compare_catalogs did not report a removed schema as breaking, so its `unchanged` verdict \
         above says nothing about the catalogs"
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
    let undeclared = report
        .changes
        .iter()
        .filter(|change| change.class == CompatibilityClass::Breaking)
        .filter(|change| {
            // "Declared" means the schema moved its OWN documentVersion to announce the break. A
            // removed schema has no version left to declare anything in, so `(Some, None)` is
            // undeclared -- comparing the two options directly would read `Some(1.0.0) != None`
            // as a declaration and wave the most breaking change there is straight through.
            match (
                document_version(&baseline, &change.schema),
                document_version(&candidate, &change.schema),
            ) {
                (Some(landed), Some(proposed)) => landed == proposed,
                _ => true,
            }
        })
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
