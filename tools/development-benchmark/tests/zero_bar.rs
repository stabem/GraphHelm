//! The bar the acceptance criteria do not state (#225 blueprint B8): a zero must carry a control.
//!
//! *"Zero results, retries, pages, fallbacks ... remain visible"* is a visibility requirement with
//! no threshold. The trap is the flattering zero: a corpus where no case ever retries has not
//! proven retries are cheap -- it has proven the corpus does not retry. Publishing that cell as
//! `0` invites exactly the misreading the number cannot support.
//!
//! So a counter column over the whole corpus renders one of two ways: EXERCISED, with its total
//! and how many cases moved it, or NOT EXERCISED -- a different word from zero on purpose.

use graphhelm_development_benchmark::{BenchmarkRefusal, ColumnReading, read_counter_column};

/// POSITIVE CONTROL, first: a column something actually moved reports as exercised, with the
/// total and the mover count an operator needs to judge how much signal stands behind it.
#[test]
fn a_column_some_case_moved_reports_exercised_with_its_evidence() {
    let reading = read_counter_column("retrievalRetries", &[0, 3, 0, 1])
        .expect("four observed values are a readable column");

    assert_eq!(
        reading,
        ColumnReading::Exercised {
            field: "retrievalRetries".to_owned(),
            total: 4,
            nonzero_cases: 2,
        }
    );
}

/// B8 itself. All zeros is NOT a zero report -- it is the absence of a control.
#[test]
fn a_column_nothing_moved_renders_not_exercised_never_zero() {
    let reading = read_counter_column("retrievalRetries", &[0, 0, 0, 0])
        .expect("observed zeros are a readable column");

    assert_eq!(
        reading,
        ColumnReading::NotExercised {
            field: "retrievalRetries".to_owned(),
        },
        "a corpus where no case retries has not proven retries are cheap; it has proven the \
         corpus does not retry, and the rendering must say WHICH"
    );
}

/// An empty column is neither exercised nor quiet -- it is a run with no cases, and that refusal
/// already exists. Reusing it keeps one meaning per refusal.
#[test]
fn an_empty_column_refuses_as_an_empty_run() {
    let refusal = read_counter_column("retrievalRetries", &[])
        .expect_err("a column with no observations was rendered anyway");

    assert_eq!(refusal, BenchmarkRefusal::EmptyRun);
}
