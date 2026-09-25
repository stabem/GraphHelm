//! Work shifted outside the count must not be counted as zero.
//!
//! Second vector in #225's threat assessment. The attack: the compiled arm warms an index, or
//! hands part of the job to something the counter cannot see, and the tokens land nowhere. If the
//! harness reads a missing number as 0, that arm wins by not being measured.
//!
//! #222 already built the defence and this consumes it rather than reinventing it:
//! `CostProvenance::Unavailable` says *"the runtime cannot see this number. **Not zero.**"* and
//! `CostField::unavailable` carries no value at all, so `observed()` returns `None`. The only way
//! to turn it into a zero is to write `.unwrap_or(0)`, which is exactly what a first pass writes.

use graphhelm_development_benchmark::{BenchmarkRefusal, compare_cost};
use graphhelm_runtime::context_accounting::{AccountingReceipt, CostField, CostProvenance};

fn receipt(field: &str, cost: CostField) -> AccountingReceipt {
    AccountingReceipt::new()
        .with_field(field, cost)
        .expect("one field is always addable")
}

#[test]
fn both_arms_measured_compares_and_says_so() {
    // POSITIVE CONTROL, first: a comparer that refused everything would pass both refusal cells
    // below while making the benchmark unable to report anything at all.
    let baseline = receipt("inputTokens", CostField::measured(1000, "baseline-runner"));
    let compiled = receipt("inputTokens", CostField::measured(400, "compiled-runner"));

    let comparison =
        compare_cost("inputTokens", &baseline, &compiled).expect("both arms observed this");

    assert!((comparison.ratio - 0.4).abs() < f64::EPSILON);
    assert_eq!(
        comparison.provenance,
        CostProvenance::Measured,
        "a ratio of two observed numbers is itself observed, and the report has to be able to say \
         that without the reader inferring it"
    );
}

#[test]
fn an_unavailable_cost_on_either_arm_refuses_instead_of_reading_as_zero() {
    let baseline = receipt("inputTokens", CostField::measured(1000, "baseline-runner"));
    let compiled = receipt(
        "inputTokens",
        CostField::unavailable("the index was warmed out of band"),
    );

    let refusal = compare_cost("inputTokens", &baseline, &compiled).expect_err(
        "a cost the runtime cannot see was compared anyway -- read as zero it makes the compiled \
         arm look free, which is the whole shape of shifting work outside the count",
    );

    match refusal {
        BenchmarkRefusal::CostUnavailable { field, arm } => {
            assert_eq!(field, "inputTokens");
            assert_eq!(
                arm, "compiled",
                "the refusal must say WHICH arm could not see it; both are possible and they mean \
                 opposite things about who is being flattered"
            );
        }
        other => panic!("provenance did not decide this: {other:?}"),
    }
}

#[test]
fn an_unavailable_cost_on_the_baseline_refuses_too_and_names_that_arm() {
    // THE MIRROR, and it is not symmetry for its own sake. An unavailable BASELINE cost flatters
    // the baseline, so a harness that only checked the compiled arm would be blind in exactly the
    // direction that makes the compiled work look worse -- and nobody audits a disappointing
    // result as hard as a flattering one.
    let baseline = receipt("inputTokens", CostField::unavailable("no counter here"));
    let compiled = receipt("inputTokens", CostField::measured(400, "compiled-runner"));

    let refusal = compare_cost("inputTokens", &baseline, &compiled)
        .expect_err("the baseline cost was unobservable and the comparison proceeded");

    match refusal {
        BenchmarkRefusal::CostUnavailable { field, arm } => {
            assert_eq!(field, "inputTokens");
            assert_eq!(arm, "baseline");
        }
        other => panic!("provenance did not decide this: {other:?}"),
    }
}

#[test]
fn a_baseline_of_zero_refuses_rather_than_publishing_an_infinity() {
    // SEEN AND ZERO is a different fact from NOT SEEN, with a different remedy: the runtime
    // observed the baseline doing no work at all, so there is nothing for the compiled arm to be a
    // fraction of. The arithmetic answer is an infinity or a NaN, and a NaN that reaches a report
    // is the class of thing this harness exists to prevent -- it prints, it looks like a value,
    // and nothing downstream distinguishes it from a measurement.
    let baseline = receipt("inputTokens", CostField::measured(0, "baseline-runner"));
    let compiled = receipt("inputTokens", CostField::measured(400, "compiled-runner"));

    let refusal = compare_cost("inputTokens", &baseline, &compiled)
        .expect_err("a ratio against a zero baseline is not a number anyone should read");

    assert_eq!(
        refusal,
        BenchmarkRefusal::BaselineZero {
            field: "inputTokens".to_owned()
        },
        "and it is NOT the unavailable refusal: that one says the runtime could not see the \
         number, this one says it saw the number and it was zero. Opposite things to go and fix."
    );
}

#[test]
fn a_compiled_arm_of_zero_compares_normally() {
    // THE OTHER DIRECTION, and it must NOT refuse. A compiled arm that spent nothing is a real
    // result -- the best possible one -- and a harness that refused it would refuse exactly the
    // outcome the work is trying to produce. The zero check belongs on the denominator alone.
    let baseline = receipt("inputTokens", CostField::measured(1000, "baseline-runner"));
    let compiled = receipt("inputTokens", CostField::measured(0, "compiled-runner"));

    let comparison = compare_cost("inputTokens", &baseline, &compiled)
        .expect("spending nothing is a result, not an error");

    assert!((comparison.ratio - 0.0).abs() < f64::EPSILON);
}

#[test]
fn a_derived_cost_compares_but_the_ratio_is_marked_derived() {
    // NOT FLATTENED INTO EITHER EXTREME. `Derived` is neither `Measured` nor `Unavailable`: the
    // number exists and is usable, but no observer stands behind it. Refusing it would throw away
    // a usable comparison; calling it measured would launder a computed number into an observed
    // one. Collapsing the three into two is the defect this lane filed as #247, and #222 already
    // declined to make that mistake -- this declines to undo it.
    let baseline = receipt("inputTokens", CostField::measured(1000, "baseline-runner"));
    let compiled = receipt(
        "inputTokens",
        CostField::derived(400, "prompt bytes / bytes-per-token"),
    );

    let comparison = compare_cost("inputTokens", &baseline, &compiled)
        .expect("a derived number is still a number");

    assert!((comparison.ratio - 0.4).abs() < f64::EPSILON);
    assert_eq!(
        comparison.provenance,
        CostProvenance::Derived,
        "the weaker provenance has to win: a ratio is only as observed as its least observed term"
    );
}
