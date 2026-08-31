//! The runner grows the receipts argument its own doc promised (#225 live half, dry-run slice).
//!
//! A RUN DIRECTORY is what the paired driver will write and the only thing the runner reads:
//! `arms.json` (two `ArmDeclaration`s) and `cases/<id>.json` (per-arm `CostField`s, the receipt
//! vocabulary reused verbatim -- `provider_reported_input_tokens`, provenance included). The
//! runner still produces NO verdict from it today: it computes the ONE bar the receipts carry
//! (input ratio; blueprint SS2 defines it as a ratio of MEDIANS, not a median of ratios) and
//! refuses the bars they do not carry, by name. Partial truth prints; nothing is promoted.

use std::path::Path;

use graphhelm_development_benchmark::{
    ArmDeclaration, BenchmarkRefusal, Case, InputRatioReport, read_run_directory,
};
use graphhelm_runtime::context_accounting::CostField;

fn arm(label: &str) -> ArmDeclaration {
    ArmDeclaration {
        snapshot: "snap-1".to_owned(),
        index_snapshot: "index-snap-1".to_owned(),
        objective: "answer the corpus".to_owned(),
        permissions: vec!["read".to_owned()],
        model_route: "route-a".to_owned(),
        model_settings: "temperature=0".to_owned(),
        clean_state: true,
        order: vec!["alpha".to_owned(), "beta".to_owned(), "gamma".to_owned()],
        seed: 7,
        clock: "2026-08-31T02:00:00Z".to_owned(),
        budget: 100_000,
        acceptance_contract: "contract-1".to_owned(),
        binary_digest: "sha256:build-1".to_owned(),
        environment: format!("env-{label}"),
        cache_discipline: "cold".to_owned(),
    }
}

fn cases() -> Vec<Case> {
    ["alpha", "beta", "gamma"]
        .iter()
        .map(|id| Case {
            id: (*id).to_owned(),
            oracle_id: format!("oracle-{id}"),
        })
        .collect()
}

fn write_arms(root: &Path, baseline: &ArmDeclaration, compiled: &ArmDeclaration) {
    let value = serde_json::json!({ "baseline": baseline, "compiled": compiled });
    std::fs::write(
        root.join("arms.json"),
        serde_json::to_vec_pretty(&value).unwrap(),
    )
    .unwrap();
}

fn write_case(root: &Path, id: &str, baseline: CostField, compiled: CostField) {
    std::fs::create_dir_all(root.join("cases")).unwrap();
    let value = serde_json::json!({
        "caseId": id,
        "baseline": { "provider_reported_input_tokens": baseline, "requiredEvidenceFound": true },
        "compiled": { "provider_reported_input_tokens": compiled, "requiredEvidenceFound": true },
    });
    std::fs::write(
        root.join("cases").join(format!("{id}.json")),
        serde_json::to_vec_pretty(&value).unwrap(),
    )
    .unwrap();
}

fn complete_run(root: &Path) {
    // Same environment on both arms: the asymmetry cell below builds its own.
    write_arms(root, &arm("same"), &arm("same"));
    // Values chosen so the two candidate definitions DISAGREE:
    //   medians: baseline 2000, compiled 300  -> ratio of medians = 0.15
    //   per-case ratios {0.1, 0.6, 0.1}       -> median of ratios = 0.1
    // The fixture decides which definition was implemented.
    write_case(
        root,
        "alpha",
        CostField::measured(1000, "provider"),
        CostField::measured(100, "provider"),
    );
    write_case(
        root,
        "beta",
        CostField::measured(2000, "provider"),
        CostField::measured(1200, "provider"),
    );
    write_case(
        root,
        "gamma",
        CostField::measured(3000, "provider"),
        CostField::measured(300, "provider"),
    );
}

/// POSITIVE CONTROL, first: a complete dry run reports the one bar the receipts carry.
#[test]
fn a_complete_run_reports_the_input_ratio_of_medians_and_its_tail() {
    let directory = tempfile::tempdir().unwrap();
    complete_run(directory.path());

    let report: InputRatioReport = read_run_directory(directory.path(), &cases())
        .expect("a complete, symmetric run directory reads");

    assert!(
        (report.input_ratio - 0.15).abs() < 1e-9,
        "ratio of MEDIANS (0.15), never median of ratios (0.1); got {}",
        report.input_ratio
    );
    assert_eq!(report.cases_measured, 3);
    assert!(
        (report.worst_case_ratio - 0.6).abs() < 1e-9,
        "the tail publishes beside the median (Bar 1); got {}",
        report.worst_case_ratio
    );
    assert_eq!(report.cases_where_compiled_exceeded_baseline, 0);
}

/// One case above 1.0 must be COUNTED, not averaged away -- the tail is the anti-gaming rail.
#[test]
fn a_case_where_compiled_exceeds_baseline_is_counted_in_the_tail() {
    let directory = tempfile::tempdir().unwrap();
    write_arms(directory.path(), &arm("same"), &arm("same"));
    write_case(
        directory.path(),
        "alpha",
        CostField::measured(1000, "provider"),
        CostField::measured(100, "provider"),
    );
    write_case(
        directory.path(),
        "beta",
        CostField::measured(1000, "provider"),
        CostField::measured(1500, "provider"),
    );
    write_case(
        directory.path(),
        "gamma",
        CostField::measured(1000, "provider"),
        CostField::measured(200, "provider"),
    );

    let report = read_run_directory(directory.path(), &cases()).expect("complete and symmetric");

    assert_eq!(report.cases_where_compiled_exceeded_baseline, 1);
    assert!((report.worst_case_ratio - 1.5).abs() < 1e-9);
}

/// A missing case file is a PARTIAL RUN, which is a different corpus -- refuse naming it.
#[test]
fn a_missing_case_receipt_refuses_naming_the_case() {
    let directory = tempfile::tempdir().unwrap();
    complete_run(directory.path());
    std::fs::remove_file(directory.path().join("cases/beta.json")).unwrap();

    let refusal = read_run_directory(directory.path(), &cases())
        .expect_err("a run missing a case was read as a run at a lower N");

    match refusal {
        BenchmarkRefusal::Unreadable { detail } => {
            assert!(detail.contains("beta"), "got: {detail}");
        }
        other => panic!("readability did not decide this: {other:?}"),
    }
}

/// An EXTRA case file is the mirror hole: receipts for a case the manifest never froze.
#[test]
fn an_extra_case_receipt_refuses_naming_the_stranger() {
    let directory = tempfile::tempdir().unwrap();
    complete_run(directory.path());
    write_case(
        directory.path(),
        "delta",
        CostField::measured(1, "provider"),
        CostField::measured(1, "provider"),
    );

    let refusal = read_run_directory(directory.path(), &cases())
        .expect_err("a receipt for an unfrozen case was accepted");

    match refusal {
        BenchmarkRefusal::Unreadable { detail } => {
            assert!(detail.contains("delta"), "got: {detail}");
        }
        other => panic!("readability did not decide this: {other:?}"),
    }
}

/// The arms travel WITH the run and are compared before any number: a warm compiled arm in the
/// directory refuses exactly as it would in memory.
#[test]
fn asymmetric_arms_in_the_directory_refuse_before_any_ratio() {
    let directory = tempfile::tempdir().unwrap();
    complete_run(directory.path());
    let mut warm = arm("same");
    warm.cache_discipline = "warm".to_owned();
    write_arms(directory.path(), &arm("same"), &warm);

    let refusal = read_run_directory(directory.path(), &cases())
        .expect_err("arms differing on the cache axis were compared anyway");

    match refusal {
        BenchmarkRefusal::AsymmetricRun { fields } => {
            assert_eq!(fields, vec!["cacheDiscipline".to_owned()]);
        }
        other => panic!("symmetry did not decide this: {other:?}"),
    }
}

/// An unavailable provider count on one arm refuses through the SAME defence #222 built --
/// never read as zero, and the refusal says which arm is being flattered.
#[test]
fn an_unavailable_input_count_refuses_naming_the_arm() {
    let directory = tempfile::tempdir().unwrap();
    write_arms(directory.path(), &arm("same"), &arm("same"));
    write_case(
        directory.path(),
        "alpha",
        CostField::measured(1000, "provider"),
        CostField::measured(100, "provider"),
    );
    write_case(
        directory.path(),
        "beta",
        CostField::measured(1000, "provider"),
        CostField::unavailable("session died before usage"),
    );
    write_case(
        directory.path(),
        "gamma",
        CostField::measured(1000, "provider"),
        CostField::measured(200, "provider"),
    );

    let refusal = read_run_directory(directory.path(), &cases())
        .expect_err("a cost the provider never reported was compared anyway");

    match refusal {
        BenchmarkRefusal::CostUnavailable { field, arm } => {
            assert_eq!(field, "provider_reported_input_tokens");
            assert_eq!(arm, "compiled");
        }
        other => panic!("provenance did not decide this: {other:?}"),
    }
}

/// Codex r2 P1, and it is Bar 2 arriving: a run whose receipts record a missed required-evidence
/// case REFUSES the comparison instead of publishing a ratio about a blind arm. The count settles
/// before any number exists -- the same order evaluate_run already enforces for quality.
#[test]
fn a_case_that_missed_its_evidence_refuses_the_whole_report() {
    let directory = tempfile::tempdir().unwrap();
    write_arms(directory.path(), &arm("same"), &arm("same"));
    write_case(
        directory.path(),
        "alpha",
        CostField::measured(1000, "provider"),
        CostField::measured(100, "provider"),
    );
    write_case(
        directory.path(),
        "beta",
        CostField::measured(1000, "provider"),
        CostField::measured(100, "provider"),
    );
    write_case(
        directory.path(),
        "gamma",
        CostField::measured(1000, "provider"),
        CostField::measured(100, "provider"),
    );
    // Rewrite beta with a recorded miss on the compiled arm.
    let path = directory.path().join("cases/beta.json");
    let text = std::fs::read_to_string(&path).unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
    value["compiled"]["requiredEvidenceFound"] = serde_json::Value::Bool(false);
    std::fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();

    let refusal = read_run_directory(directory.path(), &cases())
        .expect_err("a blind case was averaged into a published ratio");

    match refusal {
        BenchmarkRefusal::RequiredEvidenceMissing { cases } => {
            assert_eq!(cases, vec!["beta".to_owned()], "the refusal names the case");
        }
        other => panic!("recall did not decide this: {other:?}"),
    }
}
