//! The generator's ARMING SITE, guarded where the arming happens (K's re-review of #579).
//!
//! Two pins were repaired in this PR and neither had a cell here, and K named exactly why that
//! matters: **`verify_executable` was never the problem — the arming site was.** A fix at the
//! arming site with no guard at the arming site leaves the next person free to write
//! `let exe_sha256 = digest_hex(&exe_bytes)` again with everything green, which is precisely how
//! the vacuous pin survived my writing it, my own review, and everyone else's.
//!
//! So both cells drive the REAL `generate-retrieval` binary through its arguments — the shape the
//! retrieval-directory cell used, and the one K asked to see everywhere.

use std::path::Path;
use std::process::Command;

fn generator() -> Command {
    Command::new(env!("CARGO_BIN_EXE_generate-retrieval"))
}

fn fake_provider() -> String {
    env!("CARGO_BIN_EXE_fake-index-provider").to_owned()
}

/// A corpus small enough to reach the checks under test, with a store the generator can pin.
fn arrange(root: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let bench = root.join("bench");
    for dir in ["objectives", "oracle", "retrieval"] {
        std::fs::create_dir_all(bench.join(dir)).unwrap();
    }
    std::fs::write(
        bench.join("objectives/alpha.json"),
        br#"{"caseId":"alpha","objective":"which function decides?"}"#,
    )
    .unwrap();
    std::fs::write(
        bench.join("oracle/alpha.json"),
        br#"{"oracleId":"oracle-alpha","requiredEvidence":["src/lib.rs"],"answer":"x"}"#,
    )
    .unwrap();
    std::fs::write(
        bench.join("retrieval/alpha.json"),
        br#"{"caseId":"alpha","query":"which function decides?","repoSnapshot":"t","indexGeneration":"t","hits":[],"coverage":"partial","pages":1,"maxResults":5,"maxPages":8,"maxBytes":1000000,"maxTokens":250000}"#,
    )
    .unwrap();
    let cases = vec![serde_json::json!({"id": "alpha", "oracleId": "oracle-alpha"})];
    let manifest = serde_json::json!({
        "manifestVersion": 2,
        "corpusDigest": graphhelm_development_benchmark::corpus_digest(&cases),
        "oracleDigest": "sha256:unchecked-by-this-path",
        "objectivesDigest": "sha256:unchecked-by-this-path",
        "retrievalDigest": "sha256:unchecked-by-this-path",
        "cases": cases,
    });
    std::fs::write(
        bench.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    // A store with bytes in it: `pin_snapshot` refuses an empty tree, so an empty directory would
    // fail for the wrong reason and the cell would pass without reaching its subject.
    let store = root.join("store");
    std::fs::create_dir_all(&store).unwrap();
    std::fs::write(store.join("graph.db"), b"store bytes").unwrap();
    (bench, store)
}

/// THE ARMING SITE, first pin: the expected executable digest comes from the OPERATOR.
///
/// A digest re-derived from the file being pinned can never reject anything. This cell hands a
/// wrong one and requires a refusal — so re-introducing `hash(&file)` at the arming site turns
/// this red instead of leaving it green.
#[test]
fn a_wrong_executable_digest_refuses_the_generation() {
    let directory = tempfile::tempdir().unwrap();
    let (bench, store) = arrange(directory.path());

    let ran = generator()
        .arg("--manifest")
        .arg(bench.join("manifest.json"))
        .arg("--store")
        .arg(&store)
        .arg("--exe")
        .arg(fake_provider())
        .arg("--exe-sha256")
        .arg("0000000000000000000000000000000000000000000000000000000000000000")
        .arg("--staging")
        .arg(directory.path().join("staging"))
        .arg("--project")
        .arg("any")
        .arg("--repo-snapshot")
        .arg("t")
        .arg("--store-head-sha")
        .arg("1111111111111111111111111111111111111111")
        .output()
        .expect("the generator binary exists");

    assert_eq!(ran.status.code(), Some(2), "a wrong pin must refuse");
    let stderr = String::from_utf8_lossy(&ran.stderr);
    assert!(
        stderr.contains("verify_executable") || stderr.contains("does not match"),
        "the refusal must come from the PIN, not from something incidental: {stderr}"
    );
}

/// THE ARMING SITE, second pin, same shape: the store's revision is DECLARED by the operator and
/// CHECKED against what the provider reports.
///
/// The fake reports `1111...`; this cell declares something else and requires the refusal to name
/// the reported value, so a future edit that drops the comparison — or re-derives the expectation
/// from the provider's own answer — goes red here.
#[test]
fn a_store_head_sha_the_provider_contradicts_refuses_the_generation() {
    let directory = tempfile::tempdir().unwrap();
    let (bench, store) = arrange(directory.path());
    let exe = fake_provider();
    let digest = graphhelm_development_benchmark::sha256_hex_of(&std::fs::read(&exe).unwrap());

    let ran = generator()
        .arg("--manifest")
        .arg(bench.join("manifest.json"))
        .arg("--store")
        .arg(&store)
        .arg("--exe")
        .arg(&exe)
        .arg("--exe-sha256")
        .arg(&digest)
        .arg("--staging")
        .arg(directory.path().join("staging"))
        .arg("--project")
        .arg("any")
        .arg("--repo-snapshot")
        .arg("t")
        .arg("--store-head-sha")
        .arg("2222222222222222222222222222222222222222")
        .output()
        .expect("the generator binary exists");

    assert_eq!(
        ran.status.code(),
        Some(2),
        "a contradicted store must refuse"
    );
    let stderr = String::from_utf8_lossy(&ran.stderr);
    assert!(
        stderr.contains("1111111111111111111111111111111111111111"),
        "the refusal must name what the provider REPORTED, so an operator can see which side is \
         wrong: {stderr}"
    );
}
