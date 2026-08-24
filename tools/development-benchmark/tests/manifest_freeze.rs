//! The manifest is FROZEN before any run, and the freeze is what stops the corpus being edited
//! into a shape that wins.
//!
//! This is the first vector in #225's threat assessment -- "benchmark gaming can omit hard cases"
//! -- and it is the one where the edit is smallest and the result most flattering: delete the ten
//! queries that lose, report on the forty that win, publish a ratio that is arithmetically correct
//! about a corpus nobody agreed to.

use graphhelm_development_benchmark::{BenchmarkRefusal, load_manifest};

/// A manifest whose `corpusDigest` genuinely covers its `cases`.
fn frozen_manifest(case_ids: &[&str]) -> String {
    let cases: Vec<serde_json::Value> = case_ids
        .iter()
        .map(|id| serde_json::json!({ "id": id, "oracleId": format!("oracle-{id}") }))
        .collect();
    let digest = graphhelm_development_benchmark::corpus_digest(&cases);
    serde_json::json!({
        "manifestVersion": 1,
        "corpusDigest": digest,
        "cases": cases,
    })
    .to_string()
}

/// The gaming edit: remove a case and leave the frozen digest untouched.
fn with_case_removed(manifest: &str, drop_id: &str) -> String {
    let mut value: serde_json::Value = serde_json::from_str(manifest).expect("manifest is JSON");
    let cases = value["cases"].as_array().expect("cases").clone();
    value["cases"] = serde_json::Value::Array(
        cases
            .into_iter()
            .filter(|case| case["id"].as_str() != Some(drop_id))
            .collect(),
    );
    value.to_string()
}

#[test]
fn a_case_dropped_after_freezing_refuses_instead_of_reporting_on_the_rest() {
    let frozen = frozen_manifest(&["easy-1", "easy-2", "hard-1"]);
    let gamed = with_case_removed(&frozen, "hard-1");

    let refusal = load_manifest(&gamed).expect_err(
        "a corpus was edited after freezing and the manifest loaded anyway -- every number \
         computed from it would be correct arithmetic about a corpus nobody agreed to",
    );

    match refusal {
        BenchmarkRefusal::CorpusDigestMismatch { cases, .. } => {
            assert_eq!(
                cases, 2,
                "the refusal must carry the count it actually found, because that number is what \
                 tells an operator a case went missing rather than a case being edited"
            );
        }
        other => panic!("the corpus digest did not decide this: {other:?}"),
    }
}

/// The other mould: edit a case IN PLACE and leave the count alone.
fn with_oracle_repointed(manifest: &str, case_id: &str, to: &str) -> String {
    let mut value: serde_json::Value = serde_json::from_str(manifest).expect("manifest is JSON");
    for case in value["cases"].as_array_mut().expect("cases") {
        if case["id"].as_str() == Some(case_id) {
            case["oracleId"] = serde_json::Value::String(to.to_owned());
        }
    }
    value.to_string()
}

#[test]
fn a_case_repointed_at_a_different_oracle_refuses_even_though_the_count_is_unchanged() {
    // A SECOND MOULD, not a second value. Every cell above builds its invalid manifest by REMOVING
    // a case, so a guard that only counted cases would pass all of them -- which is exactly the
    // weaker guard the sabotage for this file swaps in. Repointing a case at an oracle that says
    // what the compiled arm happens to answer is the sharper attack anyway: the corpus still has
    // fifty cases, and one of them now grades against the wrong answer.
    let frozen = frozen_manifest(&["easy-1", "easy-2", "hard-1"]);
    let gamed = with_oracle_repointed(&frozen, "hard-1", "oracle-easy-1");

    let refusal = load_manifest(&gamed).expect_err(
        "a case was repointed at a different oracle and the manifest loaded -- the corpus is the \
         agreed size and one case now grades against an answer nobody agreed to",
    );

    match refusal {
        BenchmarkRefusal::CorpusDigestMismatch { cases, .. } => {
            assert_eq!(
                cases, 3,
                "the count is UNCHANGED here, and that is the signal: a differing digest with the \
                 same count means a case was edited in place rather than added or removed"
            );
        }
        other => panic!("the corpus digest did not decide this: {other:?}"),
    }
}

#[test]
fn an_untouched_frozen_manifest_loads() {
    // POSITIVE CONTROL. Without it, a loader that refused every manifest would pass the cell above
    // and the harness would be unable to run at all -- which is the failure mode that looks most
    // like safety.
    let frozen = frozen_manifest(&["easy-1", "easy-2", "hard-1"]);

    let manifest = load_manifest(&frozen).expect("nothing was edited after the freeze");

    assert_eq!(manifest.cases.len(), 3);
}
