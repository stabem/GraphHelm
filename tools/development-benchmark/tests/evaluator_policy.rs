//! The thresholds live in the shipped evaluator, and the verdict names the version that judged it.
//!
//! A published benchmark result that does not carry the rules it was judged under cannot be
//! audited afterwards: loosening a threshold and re-publishing the same headline looks exactly like
//! improving the work. `context-utilization.yaml`'s version travels with an accounting receipt for
//! the same reason, and this follows that shape rather than inventing one.

use graphhelm_development_benchmark::{EfficiencyPolicy, Medians, judge, load_policy};

/// The evaluator this package actually ships, read from disk the way `core/runtime`'s retrieval
/// tests read the shipped admission policy.
fn shipped_policy() -> EfficiencyPolicy {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../extensions/builtin/graphhelm-development-contracts/evaluators/token-efficiency.yaml"
    );
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("the shipped evaluator is unreadable at {path}: {error}"));
    load_policy(&text).expect("the shipped evaluator parses")
}

#[test]
fn the_shipped_evaluator_parses_and_carries_a_version() {
    let policy = shipped_policy();

    assert!(
        !policy.version.is_empty(),
        "a policy with no version produces verdicts that cannot be traced to the rules that made \
         them"
    );
    assert!((policy.max_median_input_ratio - 0.40).abs() < 1e-9);
}

#[test]
fn the_same_run_passes_under_one_threshold_and_fails_under_another() {
    // THE CELL THAT DISCRIMINATES, and it had to be built this way on purpose. A cell that only
    // checked "a 0.30 ratio passes the shipped policy" would pass identically against an
    // implementation that IGNORED the policy and hard-coded 0.40 -- the fixture and the defect
    // would share a shape, and the cell would be inert while looking thorough.
    //
    // Judging one run under two policies is what separates "reads the threshold" from "happens to
    // agree with it".
    let run = Medians {
        input_ratio: 0.30,
        session_ratio: 0.95,
        mean_quality_drop: 0.01,
    };

    let generous = EfficiencyPolicy {
        version: "generous".to_owned(),
        max_median_input_ratio: 0.40,
        max_median_session_ratio: 1.00,
        max_mean_quality_drop: 0.02,
    };
    let strict = EfficiencyPolicy {
        version: "strict".to_owned(),
        max_median_input_ratio: 0.10,
        ..generous.clone()
    };

    assert!(judge(&generous, &run).passed, "0.30 is inside 0.40");
    assert!(!judge(&strict, &run).passed, "0.30 is outside 0.10");
}

#[test]
fn a_verdict_names_the_policy_version_that_produced_it() {
    let policy = shipped_policy();
    let run = Medians {
        input_ratio: 0.30,
        session_ratio: 0.95,
        mean_quality_drop: 0.01,
    };

    let verdict = judge(&policy, &run);

    assert_eq!(
        verdict.policy_version, policy.version,
        "the verdict must carry the version, or a result and the rules that judged it can drift \
         apart without either looking wrong"
    );
}

#[test]
fn a_failing_verdict_names_every_threshold_it_broke_not_the_first() {
    // Same economics as the asymmetry refusal: one threshold per run means one benchmark run per
    // threshold. This run breaks all three at once.
    let policy = shipped_policy();
    let run = Medians {
        input_ratio: 0.90,
        session_ratio: 1.50,
        mean_quality_drop: 0.20,
    };

    let verdict = judge(&policy, &run);

    assert!(!verdict.passed);
    assert_eq!(
        verdict.broken,
        vec![
            "maxMeanQualityDrop".to_owned(),
            "maxMedianInputRatio".to_owned(),
            "maxMedianSessionRatio".to_owned(),
        ],
        "all three, sorted -- an unsorted list makes two identical failures print differently"
    );
}
