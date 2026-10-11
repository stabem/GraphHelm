//! The development-contract CLI surface (#223), family 3: `development memory-status`.
//!
//! The command reports the governed memory vocabulary and the moves policy allows. It reads the
//! shipped `memory-transition.yaml` â€” a declared, digest-bound contribution of the built-in package
//! â€” rather than restating the vocabulary, so this adapter never becomes a second producer of one
//! closed set.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use graphhelm_governor::{
    MemoryPublicationState, MemoryPublicationTransition, MemoryRecord, MemorySemanticState,
    apply_publication_transition,
};
use graphhelm_protocols::OpaqueId;
use serde_json::Value;

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn shipped_policy() -> Value {
    // Read at RUN time, deliberately, rather than `include_str!`d. The production path embeds the
    // same file; this reads what is on disk, so the two guards below compare the SHIPPED artifact
    // against the running code rather than comparing the binary with itself.
    let path = repository_root()
        .join("extensions/builtin/graphhelm-development-contracts/policies/memory-transition.yaml");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("the shipped policy is unreadable at {path:?}: {error}"));
    serde_yaml_ng::from_str(&text).unwrap_or_else(|error| panic!("the policy is not YAML: {error}"))
}

fn run_memory_status() -> Value {
    let output = std::process::Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args(["development", "memory-status"])
        .output()
        .expect("the built binary runs `development memory-status`");
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout was not the JSON envelope ({error}): {:?}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

fn run_accounting() -> Value {
    let output = std::process::Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args(["development", "accounting"])
        .output()
        .expect("the built binary runs `development accounting`");
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout was not the JSON envelope ({error}): {:?}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

/// The provenance field on the wire is the type's own spelling, not `Debug`'s.
///
/// #381: `run_accounting`'s only real branch (#223's existence-slice has no execution to account
/// for yet) always reports `CostProvenance::Unavailable`. Before this issue, the adapter rendered
/// it with `format!("{:?}", ...)`, so this field's value on CLI stdout, the MCP tool result, and
/// `GET /v1/development/accounting` was `"Unavailable"` -- Rust's `Debug` spelling, changing with
/// any rename or hand-written `Debug` impl and pinned by nothing. This hardcodes the wire spelling
/// independently of `CostProvenance::Unavailable.wire_name()`'s own definition (pinned separately
/// in `core/runtime/tests/context_accounting.rs`), so the two have to agree rather than one
/// deriving the other -- a rename on either side fails the pin that did not move.
#[test]
fn accounting_reports_the_provenance_wire_spelling_not_the_debug_rendering() {
    let envelope = run_accounting();
    assert_eq!(
        envelope["ok"], true,
        "the command did not succeed: {envelope}"
    );
    assert_eq!(envelope["command"], "development.accounting");
    assert_eq!(
        envelope["data"]["totalTokens"]["provenance"], "unavailable",
        "the provenance field is not `CostProvenance::Unavailable`'s wire spelling. If this reads \
         `\"Unavailable\"`, the adapter is still rendering `Debug` output instead of `wire_name()`."
    );
}

/// The command answers with the shipped policy, not a restatement of it.
///
/// The production change this catches: transcribing the vocabulary into the adapter. A hand-copied
/// list stays green on the day the policy changes and reports the old answer confidently, which is
/// worse than failing â€” it is the shape of a second producer of one vocabulary.
#[test]
fn memory_status_reports_the_shipped_policy_rather_than_a_copy_of_it() {
    let envelope = run_memory_status();
    assert_eq!(
        envelope["ok"], true,
        "the command did not succeed: {envelope}"
    );
    assert_eq!(envelope["command"], "development.memory-status");

    let policy = shipped_policy();

    // Landmark: the policy really carries the fields compared below. Without this, a policy that
    // lost one of them would make the comparison pass over an absent value.
    for field in [
        "policyVersion",
        "semanticStates",
        "publicationStates",
        "publicationTransitions",
        "allowedPublicationTransitions",
        "supersessionReasons",
    ] {
        assert!(
            !policy[field].is_null(),
            "HARNESS-BROKE: the shipped policy has no {field:?}, so comparing it below would \
             compare two nothings"
        );
    }

    assert_eq!(
        envelope["data"], policy,
        "the command's answer is not the shipped policy"
    );
}

/// **The shipped policy and the Runtime's own check are two copies of one rule, and nothing bound
/// them until this test.**
///
/// `policies/memory-transition.yaml` says *"Only the VERDICT is written here"*.
/// `apply_publication_transition` says *"The allowed set is written out because it is POLICY."*
/// Both are right, and both are authoritative-sounding, which is the problem: a tuple added to one
/// and forgotten in the other leaves the document and the enforcement disagreeing with nobody
/// watching.
///
/// The comparison is DERIVED on both sides. The runtime half is obtained by asking
/// `apply_publication_transition` about every publication-state/transition pair â€” the one oracle,
/// probed, never a transcription of it â€” and the policy half by reading the shipped document. A
/// hand-written expectation on either side would be a third copy.
///
/// Scoped to the PUBLICATION axis (ADR-032): the shipped policy's `allowedPublicationTransitions`
/// only ever governs that axis. The semantic axis moves through `supersede`, a relationship
/// between two records rather than a transition one policy tuple can express, and is out of this
/// bind's scope by construction.
///
/// The production change this catches: adding an allowed move to the policy without teaching the
/// Runtime, or the reverse. Both are silent today.
#[test]
fn the_shipped_policy_and_the_runtime_allow_exactly_the_same_publication_moves() {
    let policy = shipped_policy();

    let declared: BTreeSet<(String, String, String)> = policy["allowedPublicationTransitions"]
        .as_array()
        .expect("the policy carries an `allowedPublicationTransitions` sequence")
        .iter()
        .map(|entry| {
            (
                entry["from"].as_str().expect("`from`").to_owned(),
                entry["transition"]
                    .as_str()
                    .expect("`transition`")
                    .to_owned(),
                entry["to"].as_str().expect("`to`").to_owned(),
            )
        })
        .collect();

    let record_id = OpaqueId::parse("rec-cli-publication-matrix")
        .expect("HARNESS-BROKE: the fixture label is not a legal OpaqueId");

    let mut enforced = BTreeSet::new();
    let mut refused = 0_usize;
    for publication in MemoryPublicationState::every() {
        for transition in MemoryPublicationTransition::every() {
            let mut record = MemoryRecord::at(
                record_id.clone(),
                MemorySemanticState::Candidate,
                *publication,
            );
            if apply_publication_transition(&mut record, *transition).is_ok() {
                enforced.insert((
                    publication.wire_name().to_owned(),
                    transition.wire_name().to_owned(),
                    record.publication().wire_name().to_owned(),
                ));
            } else {
                refused += 1;
            }
        }
    }

    // Landmarks, both directions: a matrix that allowed everything, or nothing, would make the
    // equality below say nothing about which moves are governed.
    assert!(
        !enforced.is_empty(),
        "HARNESS-BROKE: the Runtime allowed no transition at all"
    );
    assert!(
        refused > 0,
        "HARNESS-BROKE: the Runtime allowed every pair, so this comparison cannot distinguish a \
         governed matrix from an ungoverned one"
    );

    assert_eq!(
        enforced, declared,
        "the shipped policy and the Runtime disagree about which publication moves are allowed. \
         Left is what `apply_publication_transition` actually does, right is what \
         memory-transition.yaml declares."
    );
}

/// Contract: explicit indexing must refuse dirty tracked bytes before invoking the provider,
/// remove its disposable worktree, and leave the original checkout unwritten. A regression that
/// hashes the original tree but indexes HEAD, or runs the sandbox funnel in the original checkout,
/// fails this cell. Existing provider fixtures receive caller-asserted source identities and miss
/// this assembly defect. No test-only production seam: the verified CLI binary is deliberately an
/// invalid provider and must never be invoked. Cost: one executable hash and local Git worktree.
#[test]
fn code_index_refuses_dirty_tracked_source_without_writing_the_checkout() {
    use sha2::{Digest as _, Sha256};
    use std::io::Read as _;
    use std::process::Command;
    let temporary = tempfile::tempdir().unwrap();
    let project = temporary.path().join("project");
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::write(project.join(".gitattributes"), "* -text\n").unwrap();
    std::fs::write(project.join("src/lib.rs"), "pub fn original() {}\n").unwrap();
    for arguments in [
        vec!["init", "--quiet"],
        vec!["add", "-A"],
        vec!["commit", "--quiet", "-m", "source"],
    ] {
        let output = Command::new("git")
            .args(["-c", "core.hooksPath=NUL"])
            .args(arguments)
            .current_dir(&project)
            .env("GIT_AUTHOR_NAME", "cbm-cli-test")
            .env("GIT_AUTHOR_EMAIL", "cbm@test.invalid")
            .env("GIT_COMMITTER_NAME", "cbm-cli-test")
            .env("GIT_COMMITTER_EMAIL", "cbm@test.invalid")
            .output()
            .unwrap();
        assert!(output.status.success(), "local Git fixture failed");
    }
    std::fs::write(project.join("src/lib.rs"), "pub fn dirty() {}\n").unwrap();
    let executable = assert_cmd::cargo::cargo_bin!("graphhelm");
    let mut file = std::fs::File::open(executable).unwrap();
    let mut hasher = Sha256::new();
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut chunk).unwrap();
        if count == 0 {
            break;
        }
        hasher.update(&chunk[..count]);
    }
    let staging = temporary.path().join("staging");
    let bundle = temporary.path().join("bundle");
    let output = Command::new(executable)
        .args([
            "--json",
            "tool",
            "code-index",
            "build",
            "--actor",
            "cbm-cli-test",
        ])
        .arg("--project")
        .arg(&project)
        .arg("--staging")
        .arg(&staging)
        .arg("--bundle")
        .arg(&bundle)
        .arg("--executable")
        .arg(executable)
        .arg("--sha256")
        .arg(hex::encode(hasher.finalize()))
        .env_remove("GRAPHHELM_ACTOR")
        .output()
        .unwrap();
    let envelope: Value = serde_json::from_slice(&output.stdout).expect("CLI JSON refusal");
    assert!(!output.status.success());
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["diagnostics"][0]["code"], "GHTOOL_CBM_BUILD");
    assert_eq!(envelope["diagnostics"][0]["pointer"], "/");
    assert!(
        envelope["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("source"),
        "the refusal must name source mismatch: {envelope}"
    );
    assert!(
        !bundle.exists(),
        "a refused build must not publish a bundle"
    );
    for name in [".home", ".tmp", ".cbm-cache"] {
        assert!(
            !project.join(name).exists(),
            "sandbox scratch leaked into original checkout"
        );
    }
    let listing = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(&project)
        .output()
        .unwrap();
    assert!(listing.status.success());
    assert_eq!(
        String::from_utf8(listing.stdout)
            .unwrap()
            .lines()
            .filter(|line| line.starts_with("worktree "))
            .count(),
        1,
        "the refused build must remove only its own disposable worktree"
    );
}
