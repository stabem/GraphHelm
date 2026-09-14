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
                // MOVEMENT IS NOT MAGNITUDE (#401). #399 made any version change read as a
                // declaration, so 1.0.0 -> 1.0.1 announced a BREAK and passed. A patch bump tells
                // a consumer nothing it can act on: it says "nothing you depend on moved" about a
                // change that removes something they depend on.
                //
                // AND THE MAGNITUDE IS EXACT, not "at least a major" (Codex, PR #567). The house
                // already owns this rule and this guard defers to it: `expected_version` in
                // `release.rs` maps a Major impact to `from.major + 1, 0, 0`, and its caller
                // refuses anything else as `GHC004_SEMVER_MISMATCH` -- "version does not equal the
                // exact required SemVer transition". Accepting any higher major HERE would make
                // this guard laxer than the rule it stands in for, and it stands in precisely
                // because that rule cannot fire against the mirror baseline. Two instruments
                // answering "is this declared?" differently is the divergence #229 is about.
                //
                // So a break announced at the WRONG major is not correctly declared either:
                // 1.2.3 -> 3.0.0 skips a major nobody published, and 1.2.3 -> 2.1.0 is not a
                // transition `expected_version` will ever name. Both stay undeclared, and so does
                // a downgrade.
                //
                // `checked_add`, NOT `saturating_add` (Codex P2, PR #567): `expected_version`
                // itself uses `checked_add` and returns `None` on overflow, and its caller treats
                // `None` as "no version satisfies this" -- an unconditional refusal. Saturating
                // here would make an unmoved major at `u64::MAX` equal its own "expected" value
                // and read as declared, the same laxer-than-the-rule-it-stands-in-for divergence
                // the wrong-major case above exists to close.
                (Some(landed), Some(proposed)) => match landed.major.checked_add(1) {
                    Some(next_major) => proposed != Version::new(next_major, 0, 0),
                    None => true,
                },
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

// The day the mirror broke, recorded from the other side.
//
// This test used to assert baseline == candidate (the "mirror era": no schema had ever evolved,
// so the gate's compatibility stage could not refuse anything). It fired on 2026-08-30 when
// graph-signal 1.1.0 became the first declared evolution - exactly as designed - and its job
// changed: the release baseline is now FROZEN HISTORY, and what must hold is that the live
// candidate diverges from it only compatibly, with every divergent schema declaring itself in its
// own documentVersion. The stage in ci/gate.ps1 is no longer a tautology; this is the property it
// actually gates.
//
// MERGED INTENT (rebase over main, 2026-08-30): main's version of this test pinned the deliberate
// divergence BY NAME (then: execution-accounting-receipt alone). That ledger survives below as
// the exact-set assertion - the general property alone would also admit an unlisted third
// evolution, and naming each deliberate divergence is what keeps this file the register of them.
#[test]
fn the_frozen_baseline_admits_only_declared_compatible_evolution() {
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
        (report.class, report.impact),
        (CompatibilityClass::Compatible, SemverImpact::Minor),
        "the live catalog must diverge from frozen 1.0.0 only compatibly and minor: {:#?}",
        report
            .changes
            .iter()
            .filter(|change| change.class == CompatibilityClass::Breaking)
            .collect::<Vec<_>>()
    );
    // The ledger of deliberate divergences, by name. Exactly these; a third evolution joins this
    // list in the same commit that introduces it, or this fails and says so.
    let mut divergent: Vec<&str> = report
        .changes
        .iter()
        .map(|change| change.schema.as_str())
        .collect();
    divergent.sort();
    divergent.dedup();
    assert_eq!(
        divergent,
        vec![
            "context-provenance",
            "execution-accounting-receipt",
            "graph-signal"
        ],
        "the deliberate divergences from 1.0.0 are exactly these three (#1065 added \
         context-provenance whole)"
    );
    for change in &report.changes {
        assert_ne!(
            document_version(&baseline, &change.schema),
            document_version(&candidate, &change.schema),
            "{} changed at {} ({}) without moving its documentVersion - evolution has to say \
             its own name",
            change.schema,
            change.pointer,
            change.code
        );
    }

    // Positive control: the acceptance above is the subject's, not the instrument's.
    let mut divergent = candidate.clone();
    let removed = divergent.schemas.keys().next().unwrap().clone();
    divergent.schemas.remove(&removed);
    assert_eq!(
        compare_catalogs(&baseline, &divergent).class,
        CompatibilityClass::Breaking,
        "compare_catalogs did not report a removed schema as breaking, so its verdict above says \
         nothing about the catalogs"
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

/// A breaking change carrying a PATCH bump is UNDER-DECLARED, and must still be refused (#401).
///
/// #399 gave the branch guard a declaration escape: a break whose schema moved its own
/// `documentVersion` reads as announced and passes. That escape is right for its purpose — it turns
/// silent breaks into declared ones — but it tests MOVEMENT and not MAGNITUDE, so `1.0.0 -> 1.0.1`
/// on a break announces nothing a consumer can act on and passes anyway.
///
/// **The magnitude check exists and cannot fire.** `release.rs:177` skips anything that is not
/// `SemverImpact::Major`, and `release.rs:300` raises `SEMVER_MISMATCH` against `required_version` —
/// but the gate invokes that path with `--baseline schemas/releases/1.0.0/catalog.json`, and
/// `catalog_integrity` holds the mirror byte-identical to the live catalog, so the comparison yields
/// no changes and the loop it lives in never executes. The check is not merely unexercised there; it
/// is unable to fire. So this cell belongs to the BRANCH guard, which is the only comparison that
/// can see a real divergence.
///
/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL: restoring `landed == proposed` as the declaration
/// test, or any rule that accepts a movement without weighing it against the impact.
#[test]
fn a_breaking_change_with_a_patch_bump_is_under_declared_and_still_refused() {
    let baseline = checked_in_catalog(LIVE_CATALOG);

    let mut candidate = baseline.clone();
    // A break the comparator already classifies without help: a property that was optional becomes
    // required, so every document the baseline accepted without it is now refused.
    let node = candidate.schemas.get_mut("node").unwrap();
    node.as_object_mut()
        .unwrap()
        .entry("required")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .unwrap()
        .push(json!("graphhelm401ProbeField"));
    // ...and the author announces it with a PATCH bump, which is the whole subject of this cell.
    let landed = document_version(&baseline, "node").expect("node carries a documentVersion");
    let announced = Version::new(landed.major, landed.minor, landed.patch + 1);
    candidate
        .catalog
        .schemas
        .get_mut("node")
        .unwrap()
        .document_version = announced.clone();

    let report = compare_catalogs(&baseline, &candidate);

    // ARRANGEMENT CONTROL, before the claim: the edit must really be BREAKING, and the version must
    // really have MOVED. Without both, this cell would pass for reasons that have nothing to do
    // with under-declaration.
    assert!(
        report
            .changes
            .iter()
            .any(|change| change.class == CompatibilityClass::Breaking),
        "arrangement: the fixture must produce a breaking change, got {:?}",
        report.class
    );
    assert_ne!(
        landed, announced,
        "arrangement: the candidate must actually announce something"
    );
    assert_eq!(
        (announced.major, announced.minor),
        (landed.major, landed.minor),
        "arrangement: the announcement must be a PATCH bump, or this is not under-declaration"
    );

    let undeclared = undeclared_breaking_changes(&baseline, &candidate, &report);

    assert!(
        !undeclared.is_empty(),
        "a break announced with {landed} -> {announced} passed as declared; a patch bump tells a \
         consumer nothing it can act on, and movement is not magnitude: {undeclared:#?}"
    );
}

/// A break announced at the WRONG major is not declared either (#401, Codex on PR #567).
///
/// The first cell pins that a PATCH bump declares nothing. This one pins the other side of the same
/// rule: "some major above" is not the test, the EXACT next major is. The house already decided
/// that -- expected_version in release.rs maps a Major impact to from.major + 1, 0, 0 and its
/// caller refuses anything else with GHC004_SEMVER_MISMATCH, "version does not equal the exact
/// required SemVer transition". This guard stands in for that rule on the branch, where the mirror
/// baseline stops it firing, so it must not be laxer than the rule it replaces.
///
/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL: relaxing the predicate to any higher major
/// (proposed.major > landed.major), which is the shape the first version of this fix shipped with.
#[test]
fn a_break_announced_at_the_wrong_major_is_not_declared() {
    let baseline = checked_in_catalog(LIVE_CATALOG);
    let landed = document_version(&baseline, "node").expect("node carries a documentVersion");
    let exact_next = Version::new(landed.major + 1, 0, 0);

    for announced in [
        Version::new(landed.major + 1, 1, 0), // a major, but not the transition
        Version::new(landed.major + 2, 0, 0), // skips a major nobody published
    ] {
        // ARRANGEMENT CONTROL: each announcement must really be ABOVE the landed major, or this
        // cell would be re-testing the patch case under a different name.
        assert!(
            announced.major > landed.major,
            "arrangement: {announced} must be a higher major than {landed}"
        );
        assert_ne!(
            announced, exact_next,
            "arrangement: {announced} must NOT be the exact required transition"
        );

        let mut candidate = baseline.clone();
        candidate
            .schemas
            .get_mut("node")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .entry("required")
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .unwrap()
            .push(json!("graphhelm401WrongMajorProbe"));
        candidate
            .catalog
            .schemas
            .get_mut("node")
            .unwrap()
            .document_version = announced.clone();

        let report = compare_catalogs(&baseline, &candidate);
        assert!(
            report
                .changes
                .iter()
                .any(|change| change.class == CompatibilityClass::Breaking),
            "arrangement: the fixture must produce a breaking change"
        );

        let undeclared = undeclared_breaking_changes(&baseline, &candidate, &report);
        assert!(
            !undeclared.is_empty(),
            "a break announced {landed} -> {announced} passed as declared; the exact required \
             transition is {exact_next}, and a guard laxer than the release rule it stands in for \
             is a guard that lets the release rule's own refusal through: {undeclared:#?}"
        );
    }
}

/// A breaking change at the maximum representable major never reads as declared, even when the
/// candidate's major is unchanged (#401, Codex P2 on PR #567).
///
/// The exact-transition fix above computes `landed.major.saturating_add(1)`. At
/// `landed.major == u64::MAX`, saturation returns `u64::MAX` again -- the SAME major, not the next
/// one -- so a candidate that never actually bumped the major (or even moved backward within it)
/// equals that "expected" version and reads as a correct declaration.
///
/// The production rule this guard stands in for does not saturate: `expected_version` in
/// `release.rs` uses `checked_add` and returns `None` on overflow, and its caller treats `None` as
/// "no version can satisfy this," rejecting every candidate unconditionally
/// (`expected.as_ref() != Some(to)` is `true` whenever `expected` is `None`). A guard that
/// saturates instead of rejecting is laxer than the rule it stands in for at exactly this edge --
/// the same class of divergence the wrong-major cell above exists to close.
///
/// THE PRODUCTION CHANGE THAT MAKES THIS FAIL: `saturating_add` in place of `checked_add`,
/// which is the shape this cell was added to close.
#[test]
fn a_breaking_change_at_the_maximum_major_never_reads_as_declared() {
    let mut baseline = checked_in_catalog(LIVE_CATALOG);
    let landed = Version::new(u64::MAX, 5, 3);
    baseline
        .catalog
        .schemas
        .get_mut("node")
        .unwrap()
        .document_version = landed.clone();

    // The candidate's major does NOT move -- only minor and patch reset, which is not a major
    // bump by any reading. `checked_add` overflow must still refuse it.
    let announced = Version::new(u64::MAX, 0, 0);

    // ARRANGEMENT CONTROL: the major genuinely does not change, or this cell would just be
    // re-testing the patch/minor case under a maximum-major label.
    assert_eq!(
        landed.major, announced.major,
        "arrangement: the major must be UNCHANGED, not a real bump"
    );
    assert_ne!(
        landed, announced,
        "arrangement: the candidate must actually announce something"
    );

    let mut candidate = baseline.clone();
    candidate
        .schemas
        .get_mut("node")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .entry("required")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .unwrap()
        .push(json!("graphhelm401MaxMajorProbe"));
    candidate
        .catalog
        .schemas
        .get_mut("node")
        .unwrap()
        .document_version = announced.clone();

    let report = compare_catalogs(&baseline, &candidate);
    assert!(
        report
            .changes
            .iter()
            .any(|change| change.class == CompatibilityClass::Breaking),
        "arrangement: the fixture must produce a breaking change"
    );

    let undeclared = undeclared_breaking_changes(&baseline, &candidate, &report);
    assert!(
        !undeclared.is_empty(),
        "a break announced {landed} -> {announced} passed as declared; the landed major is \
         already u64::MAX, so no version can ever satisfy the exact-next-major rule, and \
         saturating instead of rejecting let an unmoved major through: {undeclared:#?}"
    );
}
