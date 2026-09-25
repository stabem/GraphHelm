//! The shipped benchmark corpus binds: manifest, objectives, oracles, and the leak boundary.
//!
//! This is the file #225's scope named and nothing had created. It does NOT run a benchmark --
//! nothing can, until #222 fills the accounting receipts -- it proves the corpus the runner will
//! one day read is the corpus that was frozen: every case resolves to an objective both arms
//! receive and an oracle neither arm can reach, and an edit to any of it is red HERE rather than
//! in whichever future run happens to notice.

use std::path::PathBuf;

use graphhelm_development_benchmark::{
    ArmInputs, check_oracle_isolation, load_manifest, verify_frozen_files,
};

fn benchmark_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tools/development-benchmark/corpus")
}

fn shipped_manifest() -> graphhelm_development_benchmark::Manifest {
    let path = benchmark_root().join("manifest.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "the frozen manifest must be readable at {}: {error}",
            path.display()
        )
    });
    load_manifest(&text).expect("the shipped manifest must load against its own frozen digest")
}

/// The digest check is the whole point of the loader; this makes the SHIPPED file pass it, so a
/// case edited after the freeze is red here with the declared-vs-actual pair in the message.
#[test]
fn the_shipped_manifest_loads_against_its_frozen_digest() {
    let manifest = shipped_manifest();
    assert_eq!(
        manifest.cases.len(),
        12,
        "the corpus was frozen at twelve cases; a different count means the freeze moved"
    );
}

/// Every case resolves to bytes on disk: an objective for the arms, an oracle for the judge.
#[test]
fn every_case_has_its_objective_and_its_oracle() {
    let root = benchmark_root();
    for case in shipped_manifest().cases {
        let objective = root.join("objectives").join(format!("{}.json", case.id));
        assert!(
            objective.is_file(),
            "case `{}` names no objective at {}",
            case.id,
            objective.display()
        );
        let text = std::fs::read_to_string(&objective).expect("objective is readable");
        let value: serde_json::Value =
            serde_json::from_str(&text).expect("objective parses as JSON");
        assert_eq!(
            value["caseId"].as_str(),
            Some(case.id.as_str()),
            "the objective file must carry its own case id, so a copy-paste that forgot to \
             retarget is caught by the corpus and not by a confused run"
        );

        let expected_oracle = format!("oracle-{}", case.id);
        assert_eq!(
            case.oracle_id, expected_oracle,
            "oracle ids follow the derivable convention so a case cannot silently point at \
             another case's answers"
        );
        let oracle = root.join("oracle").join(format!("{}.json", case.id));
        assert!(
            oracle.is_file(),
            "case `{}` names no oracle at {}",
            case.id,
            oracle.display()
        );
        let text = std::fs::read_to_string(&oracle).expect("oracle is readable");
        let value: serde_json::Value = serde_json::from_str(&text).expect("oracle parses as JSON");
        assert_eq!(value["oracleId"].as_str(), Some(case.oracle_id.as_str()));
        assert!(
            value["requiredEvidence"]
                .as_array()
                .is_some_and(|entries| !entries.is_empty()),
            "an oracle with no required evidence makes recall vacuously perfect for its case"
        );
    }
}

/// The three freezes bind TOGETHER on the shipped tree: case list, oracle bytes, objective
/// bytes. The corpus-level red for a softened oracle lives in the crate's own `runner_cli` and
/// `frozen_files` suites; this cell is the green half proving the SHIPPED digests are the digests
/// of the SHIPPED bytes -- the pair a release ships must agree, or the first paired run refuses
/// on day one.
#[test]
fn the_shipped_frozen_file_digests_match_the_shipped_bytes() {
    let manifest = shipped_manifest();
    verify_frozen_files(&manifest, &benchmark_root())
        .expect("the shipped oracle and objective bytes must hash to the frozen digests");
}

/// The leak boundary, checked with the shipped instrument: an arm reading the objectives
/// directory cannot reach the oracle directory. The NEGATIVE control carries the full payload --
/// an arm that DOES read the oracle is refused -- so the green above is about the boundary and
/// not about the checker ignoring everything.
#[test]
fn the_objectives_are_readable_and_the_oracle_is_not() {
    let arm = ArmInputs {
        paths: vec!["tools/development-benchmark/corpus/objectives".to_owned()],
    };
    let oracle = "tools/development-benchmark/corpus/oracle";
    check_oracle_isolation(oracle, &arm, "baseline")
        .expect("an arm reading only objectives does not reach the oracle");

    let leaking = ArmInputs {
        paths: vec![oracle.to_owned()],
    };
    check_oracle_isolation(oracle, &leaking, "baseline")
        .expect_err("an arm reading the oracle directory must be refused, or this test is blind");
}
