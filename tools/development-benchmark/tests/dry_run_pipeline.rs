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
        // The compiled arm selects BOTH required files (recall true -- Bar 2 gates now) but at
        // function grain: one line of each, so compiled input stays genuinely smaller than the
        // naive arm's full files and the fake ratio lands well under 1.
        std::fs::write(
            bench.join("retrieval").join(format!("{id}.json")),
            serde_json::to_vec_pretty(&serde_json::json!({
                "caseId": id,
                "query": objective,
                "repoSnapshot": "sha256-g1",
                "indexGeneration": "sha256-g1",
                "hits": [{"path": "src/lib.rs", "lines": "1-1"},
                         {"path": "src/big.rs", "lines": "1-1"}],
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

    // The repo fixture is a real git checkout now, because the driver DERIVES the identity of the
    // tree it reads instead of trusting the artifact's declaration (Codex P1: coordinates produced
    // against one source tree must not slice another). The artifacts declare `sha256-g1`, so the
    // arrangement rewrites that to the tree this checkout actually hashes to.
    let git = |args: &[&str]| {
        let status = Command::new("git")
            .args(args)
            .current_dir(root.join("repo"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@test.invalid")
            .env("GIT_COMMITTER_NAME", "fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@test.invalid")
            .status()
            .expect("git runs");
        assert!(status.success(), "git {args:?} failed");
    };
    git(&["init", "--quiet"]);
    git(&["add", "-A"]);
    git(&["commit", "--quiet", "-m", "fixture"]);
    let tree = String::from_utf8(
        Command::new("git")
            .args(["rev-parse", "HEAD^{tree}"])
            .current_dir(root.join("repo"))
            .output()
            .expect("git runs")
            .stdout,
    )
    .expect("a tree id")
    .trim()
    .to_owned();
    for id in ["alpha", "beta"] {
        let path = bench.join("retrieval").join(format!("{id}.json"));
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, text.replace("sha256-g1", &tree)).unwrap();
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
        "manifestVersion": 2,
        "corpusDigest": corpus_digest(&case_values),
        "oracleDigest": frozen_files_digest(&bench, "oracle", &cases).unwrap(),
        "objectivesDigest": frozen_files_digest(&bench, "objectives", &cases).unwrap(),
        "retrievalDigest": frozen_files_digest(&bench, "retrieval", &cases).unwrap(),
        "cases": case_values,
    });
    std::fs::write(
        bench.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    bench
}

/// Re-freeze the manifest after a test deliberately edits a frozen file: these cells exist to
/// prove the refusal DOWNSTREAM of the freeze (stale bindings, partial drives), so the freeze
/// itself must agree with the edited bytes or it intercepts first and the downstream gate is
/// never exercised.
fn refreeze(bench: &Path) {
    let cases: Vec<Case> = ["alpha", "beta"]
        .iter()
        .map(|id| Case {
            id: (*id).to_owned(),
            oracle_id: format!("oracle-{id}"),
        })
        .collect();
    let text = std::fs::read_to_string(bench.join("manifest.json")).unwrap();
    let mut manifest: serde_json::Value = serde_json::from_str(&text).unwrap();
    manifest["oracleDigest"] =
        serde_json::json!(frozen_files_digest(bench, "oracle", &cases).unwrap());
    manifest["objectivesDigest"] =
        serde_json::json!(frozen_files_digest(bench, "objectives", &cases).unwrap());
    manifest["retrievalDigest"] =
        serde_json::json!(frozen_files_digest(bench, "retrieval", &cases).unwrap());
    std::fs::write(
        bench.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
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
    std::fs::write(
        &stale,
        text.replacen("\"indexGeneration\": \"", "\"indexGeneration\": \"g0", 1),
    )
    .unwrap();
    // The replace above must hit exactly one of the two binding fields, or the artifact is not
    // stale (equal ids are fresh whatever their value).
    let rewritten = std::fs::read_to_string(&stale).unwrap();
    assert!(
        rewritten.contains("\"g0"),
        "arrangement: the two binding halves must now differ"
    );
    refreeze(&bench);

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

/// Codex r2 P1 (persist-before-later-calls): a drive that fails PART WAY leaves the receipts of
/// every completed case on disk -- paid live evidence survives -- while the run stays INELIGIBLE
/// for comparison because `arms.json` is written only at the end and the reader requires it.
#[test]
fn a_partial_drive_persists_completed_cases_but_stays_unreadable_as_a_run() {
    let directory = tempfile::tempdir().unwrap();
    let bench = arrange(directory.path());
    // Break the SECOND case's artifact (stale binding); alpha must complete first in manifest
    // order, so its receipt exists by the time beta refuses.
    let stale = bench.join("retrieval/beta.json");
    let text = std::fs::read_to_string(&stale).unwrap();
    std::fs::write(
        &stale,
        text.replacen("\"indexGeneration\": \"", "\"indexGeneration\": \"g0", 1),
    )
    .unwrap();
    let rewritten = std::fs::read_to_string(&stale).unwrap();
    assert!(
        rewritten.contains("\"g0"),
        "arrangement: beta's binding halves must differ"
    );
    refreeze(&bench);

    let drove = drive(&bench, directory.path());
    assert_eq!(drove.status.code(), Some(2), "the drive refuses on beta");

    let out = directory.path().join("out");
    assert!(
        out.join("cases/alpha.json").is_file(),
        "alpha's completed receipt must SURVIVE the later failure -- paid evidence is not discarded"
    );
    assert!(
        !out.join("arms.json").exists(),
        "arms.json marks a COMPLETE run and must be absent from a partial one"
    );

    let ran = runner()
        .arg("--manifest")
        .arg(bench.join("manifest.json"))
        .arg("--receipts")
        .arg(&out)
        .output()
        .expect("the runner binary exists");
    assert_eq!(ran.status.code(), Some(2));
    let stdout = String::from_utf8_lossy(&ran.stdout);
    assert!(
        stdout.contains("Unreadable") && stdout.contains("arms.json"),
        "an incomplete run is ineligible for comparison, got: {stdout}"
    );
}

/// K's #531 hold (Codex P1 confirmed): a DISABLED route is the operator's kill switch, and this
/// driver was the only manifest consumer that ignored it -- the one that spends money. The
/// refusal lands BEFORE the key is read and BEFORE the broker opens, so the cell needs no
/// credential at all.
#[test]
fn a_disabled_route_refuses_before_key_or_broker() {
    let directory = tempfile::tempdir().unwrap();
    let bench = arrange(directory.path());
    let manifest = serde_json::json!({
        "manifestVersion": 2,
        "routes": [{
            "id": "anthropic_prod",
            "provider": "anthropic",
            "transport": "direct_api",
            "authentication": "api_key",
            "billingMode": "per_token",
            "baseUrl": "http://127.0.0.1:9",
            "model": "test-model",
            "credentialRef": "secret_test",
            "profiles": ["balanced_reasoning"],
            "enabled": false
        }]
    });
    let gateway = directory.path().join("routes.json");
    std::fs::write(&gateway, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();

    let output = driver()
        .arg("--manifest")
        .arg(bench.join("manifest.json"))
        .arg("--retrieval")
        .arg(bench.join("retrieval"))
        .arg("--repo-root")
        .arg(directory.path().join("repo"))
        .arg("--out")
        .arg(directory.path().join("out"))
        .arg("--provider")
        .arg("live")
        .arg("--clock")
        .arg("2026-08-31T07:00:00Z")
        .arg("--binary-digest")
        .arg("sha256-x")
        .arg("--environment")
        .arg("env-x")
        .arg("--gateway-manifest")
        .arg(&gateway)
        .arg("--route")
        .arg("anthropic_prod")
        .arg("--broker")
        .arg(directory.path().join("nonexistent-broker"))
        .arg("--keyring")
        .arg(directory.path().join("nonexistent-keyring"))
        .arg("--key-id")
        .arg("k1")
        .output()
        .expect("the driver binary exists");

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("disabled"),
        "the refusal must name the kill switch, not stumble into a missing key: {stderr}"
    );
    assert!(
        !stderr.contains("GRAPHHELM_GATEWAY_KEY"),
        "the key must never have been consulted: {stderr}"
    );
}

/// Codex P2, K-confirmed ordering: the 256 KiB manifest bound must hold AT the boundary -- a
/// refusal by name before the allocation, not an allocator failure after it.
#[test]
fn an_oversized_gateway_manifest_refuses_before_it_is_read() {
    let directory = tempfile::tempdir().unwrap();
    let bench = arrange(directory.path());
    let gateway = directory.path().join("routes.json");
    std::fs::write(&gateway, vec![b'x'; 300 * 1024]).unwrap();

    let output = driver()
        .arg("--manifest")
        .arg(bench.join("manifest.json"))
        .arg("--retrieval")
        .arg(bench.join("retrieval"))
        .arg("--repo-root")
        .arg(directory.path().join("repo"))
        .arg("--out")
        .arg(directory.path().join("out"))
        .arg("--provider")
        .arg("live")
        .arg("--clock")
        .arg("2026-08-31T07:00:00Z")
        .arg("--binary-digest")
        .arg("sha256-x")
        .arg("--environment")
        .arg("env-x")
        .arg("--gateway-manifest")
        .arg(&gateway)
        .arg("--route")
        .arg("anthropic_prod")
        .arg("--broker")
        .arg(directory.path().join("b"))
        .arg("--keyring")
        .arg(directory.path().join("k"))
        .arg("--key-id")
        .arg("k1")
        .output()
        .expect("the driver binary exists");

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    // The parser downstream ALSO names its 256 KiB limit, so a message test alone cannot see
    // the ordering K's confirmation is about. The pre-read check is the only producer of this
    // exact phrase, which makes the phrase the observable for "refused before the allocation".
    assert!(
        stderr.contains("refused before reading"),
        "the refusal must come from the pre-read bound, not from the parser after the \
         allocation: {stderr}"
    );
}

/// Codex P1 (post-rebase): the freeze verified one directory and the drive read ANOTHER.
///
/// `verify_frozen_files` hashes `<manifest parent>/retrieval`; the drive then read artifacts from
/// the independent `--retrieval` argument. An approved frozen directory passed while modified
/// artifacts from somewhere else determined the compiled contexts — the fourth digest verified,
/// and verified something nobody consumed.
///
/// Closed by REFUSING the divergence rather than by verifying the argument: two paths that must
/// agree are a state that can disagree, and the operator who pointed elsewhere gets told, instead
/// of having their argument silently ignored.
#[test]
fn a_retrieval_directory_other_than_the_verified_one_refuses_the_drive() {
    let directory = tempfile::tempdir().unwrap();
    let bench = arrange(directory.path());

    // A byte-identical COPY: identical content, different address. The freeze cannot tell the
    // difference, which is precisely why the address has to be the thing that is checked.
    let elsewhere = directory.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    for case in ["alpha", "beta"] {
        std::fs::copy(
            bench.join("retrieval").join(format!("{case}.json")),
            elsewhere.join(format!("{case}.json")),
        )
        .unwrap();
    }

    let drove = driver()
        .arg("--manifest")
        .arg(bench.join("manifest.json"))
        .arg("--retrieval")
        .arg(&elsewhere)
        .arg("--repo-root")
        .arg(directory.path().join("repo"))
        .arg("--out")
        .arg(directory.path().join("out"))
        .arg("--provider")
        .arg("fake")
        .arg("--clock")
        .arg("2026-08-31T03:00:00Z")
        .arg("--binary-digest")
        .arg("sha256-dryrun")
        .arg("--environment")
        .arg("env-dryrun")
        .output()
        .expect("the driver binary exists");

    assert_eq!(drove.status.code(), Some(2), "a typed refusal, not a run");
    let stdout = String::from_utf8_lossy(&drove.stdout);
    assert!(
        stdout.contains("retrieval"),
        "the refusal must name what diverged, got: {stdout}"
    );
}
