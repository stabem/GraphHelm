//! A critical miss is a hard failure and never a term in an average.
//!
//! Fifth vector in #225's threat assessment, and the only one where the number the benchmark
//! publishes is ARITHMETICALLY CORRECT. The other four produce a wrong number; this one produces a
//! right number about the wrong population. One required-evidence miss, thirty-nine slight
//! improvements, and the mean goes up.
//!
//! The acceptance criteria keep the two apart on purpose: *"required-evidence recall is 100%"* is a
//! COUNT, and *"blind mean quality falls by no more than 0.02"* is a mean. The count is not a
//! threshold the mean can compensate for.

use graphhelm_development_benchmark::{BenchmarkRefusal, CaseOutcome, evaluate_run};

fn found(case_id: &str, quality: f64) -> CaseOutcome {
    CaseOutcome {
        case_id: case_id.to_owned(),
        required_evidence_found: true,
        quality,
    }
}

fn missed(case_id: &str, quality: f64) -> CaseOutcome {
    CaseOutcome {
        case_id: case_id.to_owned(),
        required_evidence_found: false,
        quality,
    }
}

#[test]
fn a_run_with_full_recall_reports_its_mean() {
    // POSITIVE CONTROL, first: an evaluator that refused every run would pass the cells below while
    // making the benchmark unable to report at all.
    let outcomes = vec![found("a", 0.90), found("b", 0.80)];

    let verdict = evaluate_run(&outcomes).expect("every case found its required evidence");

    assert!((verdict.mean_quality - 0.85).abs() < 1e-9);
}

#[test]
fn one_missed_case_fails_the_run_even_when_it_scored_the_highest() {
    // THE FIXTURE IS THE ARGUMENT. The case that missed its required evidence carries the BEST
    // quality score in the run, so it drags the mean UP rather than down. A harness that ranked by
    // quality would call it the strongest answer in the corpus.
    //
    // That is the realistic shape of the attack, not a contrived one: an answer with the evidence
    // missing is fluent and confident precisely because nothing contradicted it. Scoring it well is
    // what a blind grader does.
    let mut outcomes: Vec<CaseOutcome> = (0..39)
        .map(|index| found(&format!("case-{index:02}"), 0.90))
        .collect();
    outcomes.push(missed("case-critical", 0.99));

    let refusal = evaluate_run(&outcomes).expect_err(
        "a case missed its required evidence and the run still produced a verdict -- the mean is \
         correct arithmetic about a population that excludes the only case that mattered",
    );

    match refusal {
        BenchmarkRefusal::RequiredEvidenceMissing { cases } => {
            assert_eq!(
                cases,
                vec!["case-critical".to_owned()],
                "the refusal names WHICH case, because that is what an operator goes and looks at"
            );
        }
        other => panic!("recall did not decide this: {other:?}"),
    }
}

#[test]
fn every_missed_case_is_named_at_once_and_in_a_stable_order() {
    // Same reason as the asymmetry refusal: one name per run costs a benchmark run per case. And
    // sorted, so two runs that missed the same cases print the same message.
    let outcomes = vec![
        missed("zulu", 0.99),
        found("mid", 0.90),
        missed("alpha", 0.95),
    ];

    let refusal = evaluate_run(&outcomes).expect_err("two cases missed");

    match refusal {
        BenchmarkRefusal::RequiredEvidenceMissing { cases } => {
            assert_eq!(cases, vec!["alpha".to_owned(), "zulu".to_owned()]);
        }
        other => panic!("recall did not decide this: {other:?}"),
    }
}

#[test]
fn an_empty_run_refuses_rather_than_reporting_a_perfect_score() {
    // ZERO CASES IS NOT FULL RECALL. An empty corpus satisfies "every case found its evidence"
    // vacuously, and the mean of nothing is a NaN that would print as a value. Both readings are
    // flattering, which is why the empty run is checked rather than left to fall through the
    // happy path.
    let refusal = evaluate_run(&[])
        .expect_err("a run with no cases produced a verdict, which no corpus earned");

    assert_eq!(refusal, BenchmarkRefusal::EmptyRun);
}
