//! A paired run is only a comparison if both arms were given the same thing.
//!
//! The issue lists ten items the run fixes: snapshots, objectives, permissions, model
//! route/settings, clean state, order, seeds, clock, budgets, and acceptance contracts. The
//! implementation checks ELEVEN fields, because `model route/settings` is one item naming two
//! things that can differ independently -- a run can hold the route and move the temperature.
//!
//! Each of these must be CHECKABLE, not merely configurable. Setting a seed and recording that you
//! set it are different acts, and only the second survives a review of the run afterwards.

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
fn arms_given_the_same_inputs_compare() {
    // POSITIVE CONTROL, first because everything below is a refusal and a comparer that refused
    // everything would pass all of them while making the benchmark impossible to run.
    compare_arms(&arm(), &arm()).expect("nothing differs between these two");
}

#[test]
fn a_differing_seed_refuses_and_names_the_field() {
    let baseline = arm();
    let mut compiled = arm();
    compiled.seed = 8;

    let refusal = compare_arms(&baseline, &compiled)
        .expect_err("the arms ran under different seeds and the runner compared them anyway");

    match refusal {
        BenchmarkRefusal::AsymmetricRun { fields } => {
            assert_eq!(
                fields,
                vec!["seed".to_owned()],
                "the refusal must NAME the field. A boolean asymmetric sends the operator to \
                 bisect a benchmark run, which is the most expensive bisection here"
            );
        }
        other => panic!("symmetry did not decide this: {other:?}"),
    }
}

#[test]
fn every_differing_field_is_named_at_once_not_one_per_run() {
    // THE POINT OF THE VEC. A refusal that reports the first difference and stops costs one full
    // benchmark run per field -- fix the seed, rerun, discover the clock, fix the clock, rerun.
    // That is the iteration cost this lane filed as #247, and a benchmark run is the most
    // expensive unit to pay it in.
    let baseline = arm();
    let mut compiled = arm();
    compiled.seed = 8;
    compiled.clock = "2026-08-24T13:00:00Z".to_owned();
    compiled.budget = 200_000;

    let refusal = compare_arms(&baseline, &compiled).expect_err("three fields differ");

    match refusal {
        BenchmarkRefusal::AsymmetricRun { fields } => {
            assert_eq!(
                fields,
                vec!["budget".to_owned(), "clock".to_owned(), "seed".to_owned()],
                "all three must be named, and in a stable order -- a refusal whose field list \
                 depends on struct layout makes two identical runs produce different messages"
            );
        }
        other => panic!("symmetry did not decide this: {other:?}"),
    }
}

#[test]
fn a_reordered_corpus_is_an_asymmetry_even_though_the_two_sets_are_equal() {
    // A DIFFERENT MOULD. Every cell above changes a SCALAR. Order is a sequence, and two arms
    // running the same cases in different orders are equal as sets while measuring different
    // things -- cache warmth, budget exhaustion, and early-stop all depend on what came first.
    //
    // A comparer written against scalars alone passes this, which is precisely why it is here:
    // the moulds differ, not just the values.
    let baseline = arm();
    let mut compiled = arm();
    compiled.order = vec!["hard-1".to_owned(), "easy-1".to_owned()];

    let refusal = compare_arms(&baseline, &compiled)
        .expect_err("the arms ran the corpus in different orders and the runner compared them");

    match refusal {
        BenchmarkRefusal::AsymmetricRun { fields } => {
            assert_eq!(fields, vec!["order".to_owned()]);
        }
        other => panic!("symmetry did not decide this: {other:?}"),
    }
}

#[test]
fn a_differing_clean_state_refuses() {
    // A THIRD MOULD: a bool. Cheap, and it is the field the cache-asymmetry attack moves --
    // baseline cold, compiled warm, and the ratio measures the cache rather than the retrieval.
    let baseline = arm();
    let mut compiled = arm();
    compiled.clean_state = false;

    let refusal = compare_arms(&baseline, &compiled)
        .expect_err("one arm started clean and the other did not");

    match refusal {
        BenchmarkRefusal::AsymmetricRun { fields } => {
            assert_eq!(fields, vec!["cleanState".to_owned()]);
        }
        other => panic!("symmetry did not decide this: {other:?}"),
    }
}
