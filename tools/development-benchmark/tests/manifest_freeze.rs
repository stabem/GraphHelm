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
        "manifestVersion": 2,
        "corpusDigest": digest,
        "oracleDigest": "sha256:not-checked-by-the-loader",
        "objectivesDigest": "sha256:not-checked-by-the-loader",
        "retrievalDigest": "sha256:not-checked-by-the-loader",
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

/// K's review probe on #504, kept as the red fixture it was: an unknown field on a case was
/// ACCEPTED with the digest byte-identical, because serde dropped it and the digest is taken over
/// a re-serialisation of the parsed struct -- unmodelled content invisible twice. Material, not
/// theoretical: the blueprint puts the criticality flag in the FROZEN manifest, so the day it
/// becomes a case field it would not be frozen by this digest.
#[test]
fn an_unknown_field_on_a_case_cannot_survive_the_freeze() {
    let manifest = r#"{
        "manifestVersion": 2,
        "corpusDigest": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
        "oracleDigest": "sha256:0",
        "objectivesDigest": "sha256:0",
        "retrievalDigest": "sha256:0",
        "cases": [{ "id": "a", "oracleId": "oracle-a", "criticality": "critical" }]
    }"#;

    let refusal = load_manifest(manifest)
        .expect_err("a case field the model does not carry was silently dropped by the freeze");

    assert!(
        matches!(refusal, BenchmarkRefusal::Unreadable { .. }),
        "unmodelled content is a manifest this loader cannot vouch for, not a digest question: \
         got {refusal:?}"
    );
}

/// Same property one level up: an unknown MANIFEST field must refuse, not vanish.
#[test]
fn an_unknown_field_on_the_manifest_cannot_survive_the_freeze() {
    let manifest = r#"{
        "manifestVersion": 2,
        "corpusDigest": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
        "oracleDigest": "sha256:0",
        "objectivesDigest": "sha256:0",
        "retrievalDigest": "sha256:0",
        "judgeSpread": 0.02,
        "cases": []
    }"#;

    let refusal = load_manifest(manifest).expect_err("an unknown manifest field was dropped");

    assert!(matches!(refusal, BenchmarkRefusal::Unreadable { .. }));
}

/// Codex P1-1: a case id BECOMES A PATH — `<kind>/<case id>.json` in `frozen_files_digest`, and
/// the same join in the generator's write. A manifest is untrusted input frozen by a digest that
/// is perfectly happy to bind a traversal, so an id carrying a separator or a parent hop reads
/// and WRITES outside the corpus directory.
///
/// Rejected by SHAPE at the loader, the same posture as the restore archive's blob names
/// (`apps/cli/src/commands/events/restore.rs`): an id that is not a plain file name is not an id,
/// and repairing it would be guessing at intent. At the LOADER because every consumer -- digest,
/// verify, generator, driver -- goes through it, so one guard covers them all.
#[test]
fn a_case_id_that_is_not_a_plain_file_name_refuses() {
    for hostile in [
        "../escape",
        "nested/case",
        r"..\windows-escape",
        "",
        ".",
        "..",
    ] {
        let cases = vec![serde_json::json!({
            "id": hostile, "oracleId": format!("oracle-{hostile}")
        })];
        let digest = graphhelm_development_benchmark::corpus_digest(&cases);
        let manifest = serde_json::json!({
            "manifestVersion": 2,
            "corpusDigest": digest,
            "oracleDigest": "sha256:not-checked-by-the-loader",
            "objectivesDigest": "sha256:not-checked-by-the-loader",
            "retrievalDigest": "sha256:not-checked-by-the-loader",
            "cases": cases,
        })
        .to_string();

        let refusal = load_manifest(&manifest).expect_err(&format!(
            "case id {hostile:?} became a path component and the loader accepted it"
        ));
        match refusal {
            BenchmarkRefusal::Unreadable { detail } => assert!(
                detail.contains("case id"),
                "the refusal must name WHAT is malformed, got: {detail}"
            ),
            other => panic!("shape did not decide this for {hostile:?}: {other:?}"),
        }
    }
}

/// Codex, fresh evidence beyond the traversal finding: the shape check accepted names that are
/// not usable filenames ON WINDOWS, which is where this repository is developed. `CON`, `AUX`,
/// `bad?name`, and names ending in a dot or space all pass `file_name()` and then fail — or
/// worse, RESOLVE through reserved-device semantics — at the `.json` read the id feeds.
///
/// A traversal guard is about where the path goes; this is about whether the path is a file at
/// all. Different property, same field, and the first guard was measured to miss it.
#[test]
fn a_case_id_that_is_not_a_portable_filename_refuses() {
    for hostile in [
        "CON",
        "aux",
        "NUL",
        "COM1",
        "LPT9",
        "bad?name",
        "bad:name",
        "bad*name",
        "bad|name",
        "trailing.",
        "trailing ",
        "quote\"name",
    ] {
        let cases = vec![serde_json::json!({
            "id": hostile, "oracleId": format!("oracle-{hostile}")
        })];
        let manifest = serde_json::json!({
            "manifestVersion": 2,
            "corpusDigest": graphhelm_development_benchmark::corpus_digest(&cases),
            "oracleDigest": "sha256:not-checked-by-the-loader",
            "objectivesDigest": "sha256:not-checked-by-the-loader",
            "retrievalDigest": "sha256:not-checked-by-the-loader",
            "cases": cases,
        })
        .to_string();

        let refusal = load_manifest(&manifest).expect_err(&format!(
            "case id {hostile:?} is not a usable filename on Windows and the loader accepted it"
        ));
        assert!(
            matches!(refusal, BenchmarkRefusal::Unreadable { .. }),
            "shape must decide this for {hostile:?}: {refusal:?}"
        );
    }
}

/// A duplicate id is DIGEST-VALID: the corpus digest binds the list as written, and a list can say
/// the same thing twice. Both artifacts then write to one filename and the driver reads that
/// single receipt twice, biasing the medians and inflating `cases_measured`.
///
/// This cell exists because the guard it covers was written, reported as landed, and then
/// silently dropped by an amend of mine — caught by a reviewer reading the FINAL loader instead
/// of trusting the reply. A fix with no cell is a fix with a half-life.
#[test]
fn a_duplicate_case_id_refuses_even_though_the_digest_is_valid() {
    let cases = vec![
        serde_json::json!({"id": "alpha", "oracleId": "oracle-alpha"}),
        serde_json::json!({"id": "alpha", "oracleId": "oracle-alpha"}),
    ];
    let manifest = serde_json::json!({
        "manifestVersion": 2,
        "corpusDigest": graphhelm_development_benchmark::corpus_digest(&cases),
        "oracleDigest": "sha256:not-checked-by-the-loader",
        "objectivesDigest": "sha256:not-checked-by-the-loader",
        "retrievalDigest": "sha256:not-checked-by-the-loader",
        "cases": cases,
    })
    .to_string();

    let refusal = load_manifest(&manifest)
        .expect_err("a repeated case id passed the loader; the digest cannot see it");

    match refusal {
        BenchmarkRefusal::Unreadable { detail } => assert!(
            detail.contains("more than once"),
            "the refusal must name the repetition, got: {detail}"
        ),
        other => panic!("uniqueness did not decide this: {other:?}"),
    }
}
