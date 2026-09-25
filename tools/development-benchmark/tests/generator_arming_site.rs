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
        .arg("--repo")
        .arg(directory.path())
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
        .arg("--repo")
        .arg(directory.path())
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

/// A minimal git repository with one commit, returning `(head sha, tree sha)`.
///
/// The #637 cells need a head that actually RESOLVES: the fake provider's fixed synthetic sha
/// can never name a tree, so these cells override the head it reports with a real commit.
fn fixture_repository(root: &std::path::Path) -> (String, String) {
    let repo = root.join("subject-repo");
    std::fs::create_dir_all(&repo).unwrap();
    let git = |arguments: &[&str]| {
        let ran = std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(arguments)
            .env("GIT_AUTHOR_NAME", "cell")
            .env("GIT_AUTHOR_EMAIL", "cell@test")
            .env("GIT_COMMITTER_NAME", "cell")
            .env("GIT_COMMITTER_EMAIL", "cell@test")
            .output()
            .expect("git runs");
        assert!(
            ran.status.success(),
            "git {arguments:?}: {}",
            String::from_utf8_lossy(&ran.stderr)
        );
        String::from_utf8_lossy(&ran.stdout).trim().to_owned()
    };
    git(&["init", "--quiet"]);
    std::fs::write(repo.join("evidence.rs"), "// subject\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "--quiet", "-m", "subject"]);
    let head = git(&["rev-parse", "HEAD"]);
    let tree = git(&["rev-parse", "HEAD^{tree}"]);
    (head, tree)
}

/// THE ARMING SITE, third binding (#637): the store's head must RESOLVE to the declared tree,
/// derived from the repository's own object database and compared -- never declared on both
/// sides. The shipped corpus froze coordinates produced against tree `15456c4d` while every
/// artifact declared `136440ae`; this cell reproduces exactly that shape (head checks out,
/// tree does not) and requires the refusal to name BOTH trees, so dropping the derivation or
/// re-deriving the expectation from the provider's answer goes red here.
#[test]
fn a_head_that_resolves_to_a_tree_the_artifacts_would_not_name_refuses() {
    let directory = tempfile::tempdir().unwrap();
    let (bench, store) = arrange(directory.path());
    let (head, tree) = fixture_repository(directory.path());
    // The override travels INSIDE the store: the funnel sanitises the child's environment, so
    // a file beside `graph.db` is the one channel that reaches the fake -- the same road the
    // real provider's own head takes.
    std::fs::write(
        store.join("head.txt"),
        format!(
            "{head}
"
        ),
    )
    .unwrap();
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
        .arg("0000000000000000000000000000000000000000")
        .arg("--store-head-sha")
        .arg(&head)
        .arg("--repo")
        .arg(directory.path().join("subject-repo"))
        .output()
        .expect("the generator binary exists");

    assert_eq!(
        ran.status.code(),
        Some(2),
        "a head that resolves to an undeclared tree must refuse"
    );
    let stderr = String::from_utf8_lossy(&ran.stderr);
    assert!(
        stderr.contains(&tree) && stderr.contains("0000000000000000000000000000000000000000"),
        "the refusal must name the DERIVED tree and the declared one, so the operator sees \
         which side to fix: {stderr}"
    );
}

/// The same binding when the head cannot be derived at all: a commit the repository does not
/// know refuses rather than assumes. Without this arm, an operator pointing `--repo` at the
/// wrong checkout would sail past the derivation exactly the way the shipped corpus did.
#[test]
fn a_head_the_repository_cannot_resolve_refuses_the_generation() {
    let directory = tempfile::tempdir().unwrap();
    let (bench, store) = arrange(directory.path());
    let (_head, _tree) = fixture_repository(directory.path());
    let exe = fake_provider();
    let digest = graphhelm_development_benchmark::sha256_hex_of(&std::fs::read(&exe).unwrap());

    // The fake reports its fixed synthetic head, which no repository contains.
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
        .arg("1111111111111111111111111111111111111111")
        .arg("--repo")
        .arg(directory.path().join("subject-repo"))
        .output()
        .expect("the generator binary exists");

    assert_eq!(
        ran.status.code(),
        Some(2),
        "an underivable binding must refuse"
    );
    let stderr = String::from_utf8_lossy(&ran.stderr);
    assert!(
        stderr.contains("does not resolve"),
        "the refusal must say the DERIVATION failed, not something incidental: {stderr}"
    );
}

/// A fixture repo with TWO commits of different content, plus a `refs/replace/<first>` pointing
/// the first commit at the second. Returns `(reported_head, real_tree, replacement_tree)`:
/// `git rev-parse reported_head^{tree}` yields `replacement_tree` WITH replacements applied and
/// `real_tree` WITHOUT.
fn fixture_with_replacement(root: &std::path::Path) -> (String, String, String) {
    let repo = root.join("subject-repo");
    std::fs::create_dir_all(&repo).unwrap();
    let git = |arguments: &[&str]| {
        let ran = std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(arguments)
            .env("GIT_AUTHOR_NAME", "cell")
            .env("GIT_AUTHOR_EMAIL", "cell@test")
            .env("GIT_COMMITTER_NAME", "cell")
            .env("GIT_COMMITTER_EMAIL", "cell@test")
            .output()
            .expect("git runs");
        assert!(
            ran.status.success(),
            "git {arguments:?}: {}",
            String::from_utf8_lossy(&ran.stderr)
        );
        String::from_utf8_lossy(&ran.stdout).trim().to_owned()
    };
    git(&["init", "--quiet"]);
    // The REAL commit the store indexed: its tree is what the artifacts must be about.
    std::fs::write(repo.join("evidence.rs"), "// the bytes the store indexed\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "--quiet", "-m", "real"]);
    let reported_head = git(&["rev-parse", "HEAD"]);
    let real_tree = git(&["rev-parse", "HEAD^{tree}"]);
    // A DIFFERENT commit whose tree we will pass off as the declared snapshot.
    std::fs::write(repo.join("evidence.rs"), "// entirely different bytes\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "--quiet", "-m", "decoy"]);
    let decoy_head = git(&["rev-parse", "HEAD"]);
    let replacement_tree = git(&["rev-parse", "HEAD^{tree}"]);
    // Redirect the real commit to the decoy: now `rev-parse reported_head^{tree}` follows the
    // replacement to `replacement_tree` unless replacements are disabled.
    git(&["replace", &reported_head, &decoy_head]);
    (reported_head, real_tree, replacement_tree)
}

/// THE ARMING SITE, third binding's SIBLING (#675, K): the head-to-tree derivation must read the
/// object database UNREPLACED. A `refs/replace/<reported>` would silently redirect `rev-parse` to
/// another commit's tree; if that replacement tree equals the declared snapshot, generation would
/// pass while the REAL commit names different bytes -- the wrong-coordinate corpus reborn through
/// a git mechanism instead of a wrong checkout. The declared snapshot here is the REPLACEMENT's
/// tree, so a derivation that honours the replacement would ACCEPT; the guard runs
/// `--no-replace-objects` / `GIT_NO_REPLACE_OBJECTS=1` and must REFUSE, naming the real tree.
#[test]
fn a_replacement_ref_cannot_redirect_the_head_to_tree_derivation() {
    let directory = tempfile::tempdir().unwrap();
    let (bench, store) = arrange(directory.path());
    let (reported_head, real_tree, replacement_tree) = fixture_with_replacement(directory.path());
    // The fake reports the REAL head; the store carries it (see `head.txt`).
    std::fs::write(store.join("head.txt"), format!("{reported_head}\n")).unwrap();
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
        // The DECLARED snapshot is the replacement's tree: the only way this passes is if the
        // derivation follows the replacement, which is exactly the redirection under test.
        .arg("--repo-snapshot")
        .arg(&replacement_tree)
        .arg("--store-head-sha")
        .arg(&reported_head)
        .arg("--repo")
        .arg(directory.path().join("subject-repo"))
        .output()
        .expect("the generator binary exists");

    assert_ne!(
        real_tree, replacement_tree,
        "arrangement: the two commits must have different trees, or nothing is redirected"
    );
    assert_eq!(
        ran.status.code(),
        Some(2),
        "a replacement-redirected derivation must refuse, not accept the redirected tree"
    );
    let stderr = String::from_utf8_lossy(&ran.stderr);
    assert!(
        stderr.contains(&real_tree),
        "the refusal must name the REAL (unreplaced) tree, proving the derivation ignored the \
         replacement: {stderr}"
    );
}
