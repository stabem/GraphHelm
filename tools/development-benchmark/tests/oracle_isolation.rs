//! The arm must not be able to read the answers it is being graded against.
//!
//! Third vector in #225's threat assessment. Half of it is already closed by SHAPE rather than by
//! vigilance: a `Case` carries an `oracleId` and never the oracle itself, so a case cannot contain
//! its own answer. This is the other half -- the arm is handed a set of inputs, and none of them
//! may reach the oracle file.

use graphhelm_development_benchmark::{ArmInputs, BenchmarkRefusal, check_oracle_isolation};

const ORACLE: &str = "fixtures/benchmark/oracle.json";

#[test]
fn an_arm_given_only_its_own_inputs_is_isolated() {
    // POSITIVE CONTROL. Without it, a checker that refused every arm would pass both cells below
    // and no run could ever start.
    let arm = ArmInputs {
        paths: vec![
            "fixtures/benchmark/corpus.json".to_owned(),
            "snapshots/snap-1".to_owned(),
        ],
    };

    check_oracle_isolation(ORACLE, &arm, "compiled").expect("nothing here reaches the oracle");
}

#[test]
fn an_arm_handed_the_oracle_file_itself_is_refused() {
    let arm = ArmInputs {
        paths: vec![
            "fixtures/benchmark/corpus.json".to_owned(),
            ORACLE.to_owned(),
        ],
    };

    let refusal = check_oracle_isolation(ORACLE, &arm, "compiled").expect_err(
        "an arm was handed the answers it is graded against, and every score it produced would be \
         a measurement of nothing",
    );

    assert_eq!(
        refusal,
        BenchmarkRefusal::OracleReachable {
            arm: "compiled".to_owned(),
            path: ORACLE.to_owned(),
        }
    );
}

#[test]
fn an_arm_handed_a_directory_that_contains_the_oracle_is_refused() {
    // THE MOULD THAT MATTERS, and the one an exact-path check misses. Nobody hands an arm the
    // oracle by name -- they hand it the fixtures directory because that is where the corpus is,
    // and the oracle is sitting in it. The leak arrives as convenience, not as intent.
    let arm = ArmInputs {
        paths: vec!["fixtures/benchmark".to_owned()],
    };

    let refusal = check_oracle_isolation(ORACLE, &arm, "baseline").expect_err(
        "an arm was given a directory containing the oracle -- an exact-path check calls this \
         clean, and it is the way the leak actually happens",
    );

    assert_eq!(
        refusal,
        BenchmarkRefusal::OracleReachable {
            arm: "baseline".to_owned(),
            path: "fixtures/benchmark".to_owned(),
        },
        "the refusal names the path AS GIVEN, not the oracle: that is the entry the operator has \
         to go and remove"
    );
}

#[test]
fn a_sibling_whose_name_is_a_text_prefix_of_the_oracle_path_is_not_a_leak() {
    // THE FALSE POSITIVE, and the fixture had to be corrected to actually produce one.
    //
    // The containment test asks whether the ORACLE path starts with the GIVEN path. Written
    // without a component boundary, `fixtures/bench` reads as containing
    // `fixtures/benchmark/oracle.json` -- it is a genuine text prefix, and it is a different
    // directory. Refusing it makes the harness unusable in a way that looks like rigour.
    //
    // A first version of this cell used `fixtures/benchmark-notes`, which is NOT a prefix of the
    // oracle path at all, so it passed under both designs and discriminated nothing. Running the
    // sabotage is what exposed that; reading the cell would not have.
    let arm = ArmInputs {
        paths: vec!["fixtures/bench".to_owned()],
    };

    check_oracle_isolation(ORACLE, &arm, "compiled")
        .expect("a sibling whose name is a text prefix does not contain the oracle");
}

#[test]
fn a_path_that_walks_up_into_the_oracle_is_refused() {
    // FOUND IN REVIEW, and it is this crate's own delivery sentence biting: the leak arrives as
    // convenience, not intent -- nobody smuggles an oracle, somebody writes a path that works.
    // `..` is exactly a path that works.
    //
    // `fixtures/benchmark/cases/../oracle.json` resolves to the oracle. Compared as text it is
    // neither equal to the oracle path nor an ancestor of it, so separator normalisation and a
    // component boundary both call it clean while the arm reads the answers.
    let arm = ArmInputs {
        paths: vec!["fixtures/benchmark/cases/../oracle.json".to_owned()],
    };

    let refusal = check_oracle_isolation(ORACLE, &arm, "compiled")
        .expect_err("a path walked up into the oracle and the check called it clean");

    // NOT `OracleReachable`, and the reason is this lane's own #247. A path carrying `..` may or
    // may not reach the oracle -- this one does, another might not -- and the honest statement is
    // that it cannot be COMPARED, not that it reaches. The remedies differ: one says remove the
    // entry that reads the answers, the other says write the path without traversal. Folding them
    // would make the harness tell some operators to go and look for a leak that is not there.
    assert_eq!(
        refusal,
        BenchmarkRefusal::PathNotComparable {
            arm: "compiled".to_owned(),
            path: "fixtures/benchmark/cases/../oracle.json".to_owned(),
        },
        "named as given, because that is the entry the operator has to find and rewrite"
    );
}

#[test]
fn a_traversal_check_by_segment_does_not_refuse_an_innocent_double_dot() {
    // THE FALSE POSITIVE THE HOUSE PRECEDENT WARNS ABOUT. `core/runtime/src/retrieval.rs` checks
    // traversal by SEGMENT and says why in as many words: a substring test for `..` also rejects
    // the perfectly ordinary `src/..foo.rs`, "and a rule that fires on innocent input gets relaxed
    // by the next person who hits it".
    //
    // That relaxation is how a security check dies -- not by being argued away, but by being
    // annoying. So the cell that keeps the check narrow is part of keeping it at all.
    let arm = ArmInputs {
        paths: vec!["fixtures/..benchmarknotes/corpus.json".to_owned()],
    };

    check_oracle_isolation(ORACLE, &arm, "compiled")
        .expect("a dot-dot inside a segment name is not traversal");
}

#[test]
fn separators_do_not_decide_whether_the_oracle_is_reachable() {
    // Paths arrive from a manifest written on one platform and read on another. A leak that is
    // caught on Unix and missed on Windows is a leak, and this harness runs on both.
    let arm = ArmInputs {
        paths: vec!["fixtures\\benchmark".to_owned()],
    };

    let refusal = check_oracle_isolation(ORACLE, &arm, "compiled")
        .expect_err("the same directory written with backslashes went unnoticed");

    assert_eq!(
        refusal,
        BenchmarkRefusal::OracleReachable {
            arm: "compiled".to_owned(),
            path: "fixtures\\benchmark".to_owned(),
        }
    );
}

// ---------------------------------------------------------------------------------------------
// The `..` finding was one mouth of a class. A probe found six live ones, so the fix is not a
// special case for `..` -- it is a decision to REFUSE TO REASON about any path that is not already
// in the one comparable form.
//
// That is the same instinct as the rest of this harness: refuse rather than interpret. Out-guessing
// an ambiguous path is how a check acquires a second, weaker implementation of the filesystem.
// ---------------------------------------------------------------------------------------------

fn refuse(given: &str) -> BenchmarkRefusal {
    let arm = ArmInputs {
        paths: vec![given.to_owned()],
    };
    check_oracle_isolation(ORACLE, &arm, "compiled").expect_err(&format!(
        "{given} reaches the oracle and the check called it clean"
    ))
}

#[test]
fn an_absolute_path_is_not_comparable_to_a_relative_oracle() {
    // The oracle path is relative. An absolute path cannot be compared to it without knowing the
    // root, and guessing the root is exactly the kind of interpretation this refuses to do.
    assert!(matches!(
        refuse("/fixtures/benchmark/oracle.json"),
        BenchmarkRefusal::PathNotComparable { .. }
    ));
}

#[test]
fn a_drive_letter_is_not_comparable_either() {
    // Same reason, and it is the sibling `core/runtime/src/retrieval.rs` already rejects.
    assert!(matches!(
        refuse("C:/fixtures/benchmark/oracle.json"),
        BenchmarkRefusal::PathNotComparable { .. }
    ));
}

#[test]
fn a_current_directory_segment_is_not_comparable() {
    assert!(matches!(
        refuse("fixtures/./benchmark"),
        BenchmarkRefusal::PathNotComparable { .. }
    ));
}

#[test]
fn a_doubled_separator_is_not_comparable() {
    // An empty segment. Harmless in isolation and it defeats a text comparison, which is enough.
    assert!(matches!(
        refuse("fixtures//benchmark"),
        BenchmarkRefusal::PathNotComparable { .. }
    ));
}

#[test]
fn a_case_variant_of_the_directory_still_reaches_the_oracle() {
    // CASE IS THE ONE THAT IS NOT ABOUT FORM. `Fixtures/Benchmark` is a perfectly canonical path,
    // and on Windows it is the same directory. Refusing it as non-comparable would be wrong -- it
    // IS comparable, and the answer is that it reaches.
    //
    // So this is compared case-insensitively rather than refused, which errs toward refusing the
    // RUN: at worst a legitimately different file whose name differs only in case gets stopped,
    // and that costs a rename. The other direction costs the benchmark.
    assert!(matches!(
        refuse("Fixtures/Benchmark"),
        BenchmarkRefusal::OracleReachable { .. }
    ));
}

#[test]
fn a_case_variant_of_the_oracle_file_still_reaches_it() {
    assert!(matches!(
        refuse("fixtures/benchmark/ORACLE.JSON"),
        BenchmarkRefusal::OracleReachable { .. }
    ));
}
