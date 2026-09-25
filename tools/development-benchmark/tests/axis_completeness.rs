//! The three axes the eleven-field comparer never held (#225 blueprint B3, B10, B11).
//!
//! B10 is F's finding folded into the blueprint: no row fixed WHICH COMPILED ARTIFACT each arm
//! executes, and B9 (determinism inside one binary) does not cover a rebuild BETWEEN arms. B11 is
//! its extension: a digest fixes the code, not what the code reads at runtime. B3 is the axis the
//! blueprint calls the one a paired benchmark dies of quietly: warmth.
//!
//! B10 and B11 are deliberately two cells, not one. A single "arms are identical" guard would go
//! green on a digest match and be read as covering the environment too.

use graphhelm_development_benchmark::{ArmDeclaration, BenchmarkRefusal, compare_arms};

fn arm() -> ArmDeclaration {
    ArmDeclaration {
        snapshot: "snap-1".to_owned(),
        index_snapshot: "index-snap-1".to_owned(),
        objective: "answer the corpus".to_owned(),
        permissions: vec!["read".to_owned()],
        model_route: "route-a".to_owned(),
        model_settings: "temperature=0".to_owned(),
        clean_state: true,
        order: vec!["easy-1".to_owned(), "hard-1".to_owned()],
        seed: 7,
        clock: "2026-08-24T12:00:00Z".to_owned(),
        budget: 100_000,
        acceptance_contract: "contract-1".to_owned(),
        binary_digest: "sha256:build-1".to_owned(),
        environment: "env-record-1".to_owned(),
        cache_discipline: "cold".to_owned(),
    }
}

#[test]
fn equal_arms_still_compare_with_the_new_axes_present() {
    // POSITIVE CONTROL, first: a comparer that refused everything would pass every cell below.
    compare_arms(&arm(), &arm()).expect("nothing differs between these two");
}

/// B10. The refusal must land BEFORE any ratio exists, at the axis refusal, naming the field.
#[test]
fn arms_from_different_builds_refuse_and_name_the_binary() {
    let baseline = arm();
    let mut compiled = arm();
    compiled.binary_digest = "sha256:build-2".to_owned();

    let refusal = compare_arms(&baseline, &compiled)
        .expect_err("the arms executed different builds and the runner compared them anyway");

    match refusal {
        BenchmarkRefusal::AsymmetricRun { fields } => {
            assert_eq!(
                fields,
                vec!["binaryDigest".to_owned()],
                "the refusal must NAME the build axis; a difference explainable by a rebuild is \
                 not a difference attributable to the treatment"
            );
        }
        other => panic!("symmetry did not decide this: {other:?}"),
    }
}

/// B11. Same build, different environment record: a digest match must not be read as covering it.
#[test]
fn arms_under_different_environments_refuse_and_name_the_environment() {
    let baseline = arm();
    let mut compiled = arm();
    compiled.environment = "env-record-2".to_owned();

    let refusal = compare_arms(&baseline, &compiled)
        .expect_err("the arms ran under different environment records with an identical binary");

    match refusal {
        BenchmarkRefusal::AsymmetricRun { fields } => {
            assert_eq!(fields, vec!["environment".to_owned()]);
        }
        other => panic!("symmetry did not decide this: {other:?}"),
    }
}

/// B3. A warm compiled arm against a cold baseline manufactures a good ratio and leaves no trace
/// in a mean. The refusal is the cache axis, before any number.
#[test]
fn a_warm_compiled_arm_against_a_cold_baseline_refuses_on_the_cache_axis() {
    let baseline = arm();
    let mut compiled = arm();
    compiled.cache_discipline = "warm".to_owned();

    let refusal = compare_arms(&baseline, &compiled)
        .expect_err("one arm was warm and the runner produced a comparison anyway");

    match refusal {
        BenchmarkRefusal::AsymmetricRun { fields } => {
            assert_eq!(fields, vec!["cacheDiscipline".to_owned()]);
        }
        other => panic!("symmetry did not decide this: {other:?}"),
    }
}

/// The three new axes report TOGETHER with the old ones, not first-difference-and-stop.
#[test]
fn new_and_old_axes_are_named_in_one_refusal() {
    let baseline = arm();
    let mut compiled = arm();
    compiled.seed = 8;
    compiled.binary_digest = "sha256:build-2".to_owned();
    compiled.cache_discipline = "warm".to_owned();

    let refusal = compare_arms(&baseline, &compiled).expect_err("three axes differ");

    match refusal {
        BenchmarkRefusal::AsymmetricRun { fields } => {
            assert_eq!(
                fields,
                vec![
                    "binaryDigest".to_owned(),
                    "cacheDiscipline".to_owned(),
                    "seed".to_owned()
                ],
                "sorted, so two runs differing the same way produce the same message"
            );
        }
        other => panic!("symmetry did not decide this: {other:?}"),
    }
}

/// K's finding 3 on #504: the blueprint's own SS3 table splits "snapshots" in two -- repository
/// and index -- because index freshness is not repository freshness (#219's INDEX_STALE), and a
/// run where only one arm has an index is not a pairing. The fourteen-axis arithmetic counted
/// from the ISSUE's list and dropped the one row the blueprint added outside B10/B11/B3.
#[test]
fn arms_with_different_index_snapshots_refuse_and_name_the_index() {
    let baseline = arm();
    let mut compiled = arm();
    compiled.index_snapshot = "index-snap-2".to_owned();

    let refusal = compare_arms(&baseline, &compiled)
        .expect_err("one arm ran against a different index and the runner compared them anyway");

    match refusal {
        BenchmarkRefusal::AsymmetricRun { fields } => {
            assert_eq!(fields, vec!["indexSnapshot".to_owned()]);
        }
        other => panic!("symmetry did not decide this: {other:?}"),
    }
}
