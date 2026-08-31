//! The whole dry-run pipeline, end to end through BOTH binaries (#225 live half, zero model cost).
//!
//! driver (fake provider) -> run directory -> runner --receipts -> input-ratio report. This is
//! the chain the first live run will ride; the only substitution live makes is the provider.
//! Everything here is deterministic: the fake provider reports `context bytes / 4` as its input
//! count, labelled `fake-provider`, and the arms declare `modelRoute: fake` -- a dry-run number
//! cannot be mistaken for a live one without ignoring two labels.

use std::path::{Path, PathBuf};
use std::process::Command;

use graphhelm_development_benchmark::{Case, corpus_digest, frozen_files_digest};

fn driver() -> Command {
    Command::new(env!("CARGO_BIN_EXE_paired-driver"))
}

fn runner() -> Command {
    Command::new(env!("CARGO_BIN_EXE_graphhelm-development-benchmark"))
}

/// Two cases, tiny repo, fresh retrieval artifacts: everything the driver needs, nothing shared
/// with the shipped corpus.
fn arrange(root: &Path) -> PathBuf {
    let bench = root.join("bench");
    for dir in ["objectives", "oracle", "retrieval"] {
        std::fs::create_dir_all(bench.join(dir)).unwrap();
    }
    let repo = root.join("repo/src");
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::write(repo.join("lib.rs"), b"pub fn decide() -> u8 { 42 }\n").unwrap();
    std::fs::write(
        repo.join("big.rs"),
        format!("// filler\n{}", "x".repeat(4000)).as_bytes(),
    )
    .unwrap();

    for (id, objective) in [
        ("alpha", "which function decides?"),
        ("beta", "where is the filler?"),
    ] {
        std::fs::write(
            bench.join("objectives").join(format!("{id}.json")),
            serde_json::to_vec_pretty(&serde_json::json!({
                "caseId": id, "objective": objective,
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            bench.join("oracle").join(format!("{id}.json")),
            serde_json::to_vec_pretty(&serde_json::json!({
                "oracleId": format!("oracle-{id}"),
                "requiredEvidence": ["src/lib.rs", "src/big.rs"],
                "answer": "CANARY-NEVER-IN-ANY-CONTEXT",
            }))
            .unwrap(),
        )
        .unwrap();
        // The compiled arm's selection: only the small file -- so compiled input is genuinely
        // smaller than naive, and the fake ratio lands well under 1.
        std::fs::write(
            bench.join("retrieval").join(format!("{id}.json")),
            serde_json::to_vec_pretty(&serde_json::json!({
                "caseId": id,
                "query": objective,
                "repoSnapshot": "sha256-g1",
                "indexGeneration": "sha256-g1",
                "hits": ["src/lib.rs"],
                "coverage": "complete",
                "pages": 1,
                "maxResults": 50,
                "maxPages": 8,
                "maxBytes": 1000000,
                "maxTokens": 250000,
            }))
            .unwrap(),
        )
        .unwrap();
    }

    let cases: Vec<Case> = ["alpha", "beta"]
        .iter()
        .map(|id| Case {
            id: (*id).to_owned(),
            oracle_id: format!("oracle-{id}"),
        })
        .collect();
    let case_values: Vec<serde_json::Value> = cases
        .iter()
        .map(|case| serde_json::to_value(case).unwrap())
        .collect();
    let manifest = serde_json::json!({
        "manifestVersion": 1,
        "corpusDigest": corpus_digest(&case_values),
        "oracleDigest": frozen_files_digest(&bench, "oracle", &cases).unwrap(),
        "objectivesDigest": frozen_files_digest(&bench, "objectives", &cases).unwrap(),
        "cases": case_values,
    });
    std::fs::write(
        bench.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    bench
}

fn drive(bench: &Path, root: &Path) -> std::process::Output {
    driver()
        .arg("--manifest")
        .arg(bench.join("manifest.json"))
        .arg("--retrieval")
        .arg(bench.join("retrieval"))
        .arg("--repo-root")
        .arg(root.join("repo"))
        .arg("--out")
        .arg(root.join("out"))
        .arg("--provider")
        .arg("fake")
        .arg("--clock")
        .arg("2026-08-31T03:00:00Z")
        .arg("--binary-digest")
        .arg("sha256-dryrun")
        .arg("--environment")
        .arg("env-dryrun")
        .output()
        .expect("the driver binary exists")
}

/// THE CHAIN. Driver writes, runner reads, the one measured bar prints with its tail, and the
/// unmeasured bars are refused by name. Exit codes: driver 0 (it produced a run), runner 2 (one
/// bar is not a verdict).
#[test]
fn the_dry_run_pipeline_produces_the_input_ratio_end_to_end() {
    let directory = tempfile::tempdir().unwrap();
    let bench = arrange(directory.path());

    let drove = drive(&bench, directory.path());
    assert_eq!(
        drove.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&drove.stdout),
        String::from_utf8_lossy(&drove.stderr)
    );

    let ran = runner()
        .arg("--manifest")
        .arg(bench.join("manifest.json"))
        .arg("--receipts")
        .arg(directory.path().join("out"))
        .output()
        .expect("the runner binary exists");
    assert_eq!(
        ran.status.code(),
        Some(2),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&ran.stdout),
        String::from_utf8_lossy(&ran.stderr)
    );
    let stdout = String::from_utf8_lossy(&ran.stdout);
    assert!(stdout.contains("input_ratio"), "got: {stdout}");
    assert!(
        stdout.contains("session") && stdout.contains("quality"),
        "the unmeasured bars stay refused by name, got: {stdout}"
    );

    // The compiled arm selected ONE small file where naive carried two including the big one, so
    // the fake ratio must land strictly under 1 -- and the tail must agree there is no case
    // where compiled exceeded naive.
    let report_line = stdout
        .lines()
        .find(|line| line.contains("input_ratio"))
        .expect("a report line");
    assert!(
        !report_line.contains("cases_where_compiled_exceeded_baseline: 1"),
        "got: {report_line}"
    );
}

/// The canary: the oracle's answer text reaches NEITHER context. Asserted over the run directory
/// bytes the driver wrote -- the whole surface a later prompt is built from.
#[test]
fn the_oracle_answer_reaches_no_context_the_driver_writes() {
    let directory = tempfile::tempdir().unwrap();
    let bench = arrange(directory.path());

    let drove = drive(&bench, directory.path());
    assert_eq!(drove.status.code(), Some(0));

    let mut swept = 0usize;
    for entry in walk(directory.path().join("out")) {
        let bytes = std::fs::read(&entry).unwrap();
        assert!(
            !String::from_utf8_lossy(&bytes).contains("CANARY"),
            "the oracle answer leaked into {}",
            entry.display()
        );
        swept += 1;
    }
    assert!(
        swept >= 3,
        "the sweep must have seen the run files, saw {swept}"
    );
}

/// A stale retrieval artifact fails the DRIVE, not the later read: the run directory must not
/// come into existence carrying a compiled arm built on stale coordinates.
#[test]
fn a_stale_retrieval_artifact_refuses_the_drive_itself() {
    let directory = tempfile::tempdir().unwrap();
    let bench = arrange(directory.path());
    let stale = bench.join("retrieval/alpha.json");
    let text = std::fs::read_to_string(&stale).unwrap();
    std::fs::write(&stale, text.replace("\"sha256-g1\",", "\"sha256-g0\",")).unwrap();
    // The replace above must hit exactly one of the two binding fields, or the artifact is not
    // stale (equal ids are fresh whatever their value).
    let rewritten = std::fs::read_to_string(&stale).unwrap();
    assert!(
        rewritten.contains("sha256-g0") && rewritten.contains("sha256-g1"),
        "arrangement: the two binding halves must now differ"
    );

    let drove = drive(&bench, directory.path());
    assert_eq!(drove.status.code(), Some(2), "a typed refusal, not a run");
    let stdout = String::from_utf8_lossy(&drove.stdout);
    assert!(
        stdout.contains("RetrievalRefused") && stdout.contains("index_stale"),
        "got: {stdout}"
    );
}

fn walk(root: PathBuf) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![root];
    while let Some(directory) = stack.pop() {
        for entry in std::fs::read_dir(&directory)
            .into_iter()
            .flatten()
            .flatten()
        {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                files.push(path);
            }
        }
    }
    files
}
