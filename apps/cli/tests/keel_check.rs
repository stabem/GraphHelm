//! `graphhelm keel check` (#1330): the scope counter reached through the installed binary.
//!
//! Every case builds a real git repository with two commits and runs the binary on the range, so
//! the git invocation, the embedded policy and the envelope are all under test, not only the core
//! function. The oracle for each case is a literal: the rule id, the exit code, the counts.

#[path = "support/mod.rs"]
mod support;
use assert_cmd::Command;
use graphhelm_events::EvidenceOpener;
use serde_json::Value;
use std::fs;
use std::path::Path;
use support::owner_token;
use tempfile::TempDir;

fn git(repo: &Path, args: &[&str]) {
    let status = graphhelm_process::command("git")
        .arg("-C")
        .arg(repo)
        .args([
            "-c",
            "user.name=keel",
            "-c",
            "user.email=keel@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

/// A repository whose `HEAD~1..HEAD` range adds `src/lib.rs` content and whatever `extra` names.
fn repository(extra: &[(&str, &str)]) -> TempDir {
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-q"]);
    fs::create_dir_all(repo.path().join("src")).unwrap();
    fs::write(repo.path().join("src/lib.rs"), "pub fn old() {}\n").unwrap();
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "-q", "-m", "base"]);
    fs::write(
        repo.path().join("src/lib.rs"),
        "pub fn old() {}\npub fn added() {}\n",
    )
    .unwrap();
    for (path, body) in extra {
        let full = repo.path().join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, body).unwrap();
    }
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "-q", "-m", "change"]);
    repo
}

fn card(dir: &Path, scope: &[&str]) -> std::path::PathBuf {
    let path = dir.join("card.json");
    let card = serde_json::json!({
        "promise": "added() exists",
        "scopePaths": scope,
        "proof": "cargo test",
        "exportedSymbols": ["added"],
    });
    fs::write(&path, serde_json::to_vec(&card).unwrap()).unwrap();
    path
}

fn run(repo: &Path, card: Option<&Path>) -> (i32, Value) {
    let mut command = Command::cargo_bin("graphhelm").unwrap();
    command.args([
        "--json",
        "keel",
        "check",
        "--diff",
        "HEAD~1..HEAD",
        "--repo",
        repo.to_str().unwrap(),
    ]);
    if let Some(card) = card {
        command.args(["--card", card.to_str().unwrap()]);
    }
    let output = command.output().unwrap();
    let reply: Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|_| panic!("not JSON: {}", String::from_utf8_lossy(&output.stdout)));
    (output.status.code().unwrap(), reply)
}

/// Contract: an explicit binding turns a blocked verdict into a pending, sealed proposal.
/// Regression: the check prints advice but never submits it, or submits the review draft.
/// Existing check tests stop at recommendation; the HTTP admission test supplies its own signal.
/// Cost: short local CLI processes, one temporary git repo and event store; no network.
#[test]
fn routed_blocked_check_records_the_selected_pending_draft() {
    let repo = repository(&[
        ("docs/notes.md", "outside scope\n"),
        ("core/events/src/journal.rs", "pub fn changed() {}\n"),
    ]);
    let scratch = tempfile::tempdir().unwrap();
    let scope_card = card(
        scratch.path(),
        &["src/lib.rs", "core/events/src/journal.rs"],
    );
    let events = scratch.path().join("events");
    let unbound_events = scratch.path().join("unbound-events");
    let keyring = scratch.path().join("keyring");
    fs::create_dir(&keyring).unwrap();
    let key = "0101010101010101010101010101010101010101010101010101010101010101";
    let minted = Command::cargo_bin("graphhelm")
        .unwrap()
        .args(["gateway", "keyring", "init", "--keyring"])
        .arg(&keyring)
        .args(["--key-id", "route-key"])
        .env("GRAPHHELM_EVENTS_KEY", key)
        .output()
        .unwrap();
    assert!(
        minted.status.success(),
        "{}",
        String::from_utf8_lossy(&minted.stdout)
    );
    let graph =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/graphs/software-feature.yaml");
    let fixtures = scratch.path().join("fixtures.json");
    fs::write(&fixtures, br#"{"nodeOutcomes":{}}"#).unwrap();
    let unbound_start = Command::cargo_bin("graphhelm")
        .unwrap()
        .args(["--json", "execution", "start", "--file"])
        .arg(&graph)
        .args(["--events"])
        .arg(&unbound_events)
        .args(["--fixtures"])
        .arg(&fixtures)
        .args([
            "--mode",
            "supervised",
            "--execution",
            "exec_feature",
            "--held",
        ])
        .output()
        .unwrap();
    assert!(
        unbound_start.status.success(),
        "{}",
        String::from_utf8_lossy(&unbound_start.stdout)
    );
    let start = Command::cargo_bin("graphhelm")
        .unwrap()
        .args(["--json", "execution", "start", "--file"])
        .arg(&graph)
        .args(["--events"])
        .arg(&events)
        .args(["--fixtures"])
        .arg(&fixtures)
        .args(["--keyring"])
        .arg(&keyring)
        .args(["--key-id", "route-key"])
        .env("GRAPHHELM_EVENTS_KEY", key)
        .args([
            "--mode",
            "supervised",
            "--execution",
            "exec_feature",
            "--held",
        ])
        .output()
        .unwrap();
    assert!(
        start.status.success(),
        "{}",
        String::from_utf8_lossy(&start.stdout)
    );
    let hashed = Command::cargo_bin("graphhelm")
        .unwrap()
        .args(["--json", "graph", "hash"])
        .arg(&graph)
        .output()
        .unwrap();
    assert!(
        hashed.status.success(),
        "{}",
        String::from_utf8_lossy(&hashed.stdout)
    );
    let hashed: Value = serde_json::from_slice(&hashed.stdout).unwrap();
    let graph_hash = hashed["data"]["hash"].as_str().unwrap();
    let head = graphhelm_process::command("git")
        .arg("-C")
        .arg(repo.path())
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    let head = String::from_utf8(head.stdout).unwrap().trim().to_owned();
    let draft = |id: &str, version: u64, hash: &str, node: &str| {
        serde_json::json!({
            "id": id, "expectedVersion": version, "expectedHash": hash,
            "operations": [{
                "op": "addNode", "path": format!("/spec/nodes/{node}"),
                "value": {
                    "type": "agent", "name": node, "objective": "Correct the blocked scope",
                    "optionality": "required",
                    "agent": {"ephemeral": {
                        "purpose": "Correct the blocked scope",
                        "capabilities": ["repository.read"],
                        "allowedTools": ["repository.read"],
                        "prohibitedActions": ["repository.write"],
                        "inputSchema": "schema://ImplementationResult@1",
                        "outputSchema": "schema://ImplementationResult@1",
                        "instructions": "Correct the blocked scope",
                        "completionContract": {"requires": ["source_locations"]},
                        "isolationMinimum": "tier_0"
                    }},
                    "completion": {"requires": [{"outputSchemaValid": true}]},
                    "timeoutSeconds": 900
                }
            }]
        })
    };
    let binding = scratch.path().join("binding.json");
    fs::write(
        &binding,
        serde_json::to_vec(&serde_json::json!({
            "expectedHead": head, "execution": "exec_feature", "task": "issue-1045",
            "node": "plan", "implementDraft": draft("draft-implement", 1, graph_hash, "route-correction"),
            "reviewDraft": draft("draft-review", 1, graph_hash, "route-review")
        }))
        .unwrap(),
    )
    .unwrap();
    let check = |binding_path: Option<&Path>, card_path: &Path| {
        let mut command = Command::cargo_bin("graphhelm").unwrap();
        let check_events = if binding_path.is_some() {
            &events
        } else {
            &unbound_events
        };
        command
            .args([
                "--json",
                "keel",
                "check",
                "--diff",
                "HEAD~1..HEAD",
                "--repo",
            ])
            .arg(repo.path())
            .args(["--card"])
            .arg(card_path)
            .args(["--events"])
            .arg(check_events)
            .args(["--execution", "exec_feature", "--keyring"])
            .arg(&keyring)
            .args(["--key-id", "route-key"])
            .env("GRAPHHELM_EVENTS_KEY", key);
        if let Some(path) = binding_path {
            command.arg("--route-proposal").arg(path);
        }
        let output = command.output().unwrap();
        let reply: Value = serde_json::from_slice(&output.stdout).unwrap();
        (output.status.code(), reply)
    };
    let (plain_code, plain) = check(None, &scope_card);
    assert_eq!(plain_code, Some(2), "{plain}");
    assert!(plain["data"]["routeProposal"].is_null());
    let (code, checked) = check(Some(&binding), &scope_card);
    assert_eq!(code, Some(2), "{checked}");
    assert_eq!(checked["data"]["recommendation"]["step"], "implement");
    assert_eq!(checked["data"]["routeProposal"]["kind"], "keel.blocked");
    assert_eq!(checked["data"]["recommendation"]["applied"], false);
    let status = Command::cargo_bin("graphhelm")
        .unwrap()
        .args(["--json", "execution", "status", "--events"])
        .arg(&events)
        .args(["--execution", "exec_feature"])
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "{}",
        String::from_utf8_lossy(&status.stdout)
    );
    let status: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert!(
        status["data"]["attentionReasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason["kind"] == "pending_draft"
                && reason["draft_id"] == "draft-implement"),
        "{status}"
    );
    assert_eq!(status["data"]["signalsRecorded"], 1);
    let replay = Command::cargo_bin("graphhelm")
        .unwrap()
        .args(["--json", "graph", "replay", "--events"])
        .arg(&events)
        .output()
        .unwrap();
    assert!(
        replay.status.success(),
        "{}",
        String::from_utf8_lossy(&replay.stdout)
    );
    let replay: Value = serde_json::from_slice(&replay.stdout).unwrap();
    assert_eq!(replay["data"]["currentGraph"]["number"], 1, "{replay}");
    let digest = status["data"]["proposalDigests"]["draft-implement"]
        .as_str()
        .expect("status exposes the exact pending proposal digest");
    let approval = Command::cargo_bin("graphhelm")
        .unwrap()
        .args(["execution", "approve", "--events"])
        .arg(&events)
        .args([
            "--execution",
            "exec_feature",
            "--node",
            "route-correction",
            "--keyring",
        ])
        .arg(&keyring)
        .args([
            "--key-id",
            "route-key",
            "--actor-id",
            "route-agent",
            "--proposal-digest",
            digest,
            "--draft-id",
            "draft-implement",
        ])
        .env("GRAPHHELM_EVENTS_KEY", key)
        .output()
        .unwrap();
    assert!(
        approval.status.success(),
        "the selected implementation proposal should apply: {}",
        String::from_utf8_lossy(&approval.stdout)
    );
    let published = Command::cargo_bin("graphhelm")
        .unwrap()
        .args(["--json", "graph", "replay", "--events"])
        .arg(&events)
        .output()
        .unwrap();
    assert!(
        published.status.success(),
        "{}",
        String::from_utf8_lossy(&published.stdout)
    );
    let published: Value = serde_json::from_slice(&published.stdout).unwrap();
    assert_eq!(
        published["data"]["currentGraph"]["number"], 2,
        "{published}"
    );
    let published_hash = published["data"]["currentGraph"]["semanticHash"]
        .as_str()
        .expect("replay exposes the approved graph hash");
    let first_status = Command::cargo_bin("graphhelm")
        .unwrap()
        .args(["--json", "execution", "status", "--events"])
        .arg(&events)
        .args(["--execution", "exec_feature"])
        .output()
        .unwrap();
    assert!(
        first_status.status.success(),
        "{}",
        String::from_utf8_lossy(&first_status.stdout)
    );
    let first_status: Value = serde_json::from_slice(&first_status.stdout).unwrap();
    assert_eq!(
        first_status["data"]["nodeStates"]["route-correction"], "ready",
        "{first_status}"
    );
    assert!(
        first_status["data"]["nodeStates"]["route-review"].is_null(),
        "{first_status}"
    );
    // Read the persisted event and decrypt its sealed envelope. A response alone could claim
    // admission while dropping the checked head or the precise scope finding.
    struct Clock;
    impl graphhelm_protocols::Clock for Clock {
        fn now(&self) -> chrono::DateTime<chrono::Utc> {
            chrono::Utc::now()
        }
    }
    struct Ids;
    impl graphhelm_protocols::IdGenerator for Ids {
        fn next_id(&self, prefix: &'static str) -> String {
            format!("{prefix}-test")
        }
    }
    let store = graphhelm_events::LocalEventRepository::open(
        &events,
        std::sync::Arc::new(Clock),
        std::sync::Arc::new(Ids),
    )
    .unwrap();
    let (stream, history) = store.read_unique_replay_stream().unwrap();
    let recorded = history
        .iter()
        .find(|event| {
            matches!(
                event.kind,
                graphhelm_protocols::EventKind::SignalRecorded(_)
            )
        })
        .unwrap();
    let reference = recorded
        .evidence_refs
        .first()
        .expect("signal has sealed evidence");
    let sealed = match store
        .sealed_evidence(&stream.scope, reference.evidence_id())
        .unwrap()
    {
        graphhelm_events::EvidenceRead::Available(sealed) => sealed,
        other => panic!("signal evidence unavailable: {other:?}"),
    };
    let provider = graphhelm_sealed_key_provider::SealedKeyProvider::open(
        &keyring,
        "route-key",
        graphhelm_events::SecretBytes::new(vec![1; 32]),
    )
    .unwrap();
    let protector = graphhelm_events::EvidenceProtector::new(provider);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let envelope = runtime
        .block_on(protector.open(stream.scope.clone(), &sealed))
        .unwrap();
    let signal: Value = envelope.expose(|bytes| serde_json::from_slice(bytes).unwrap());
    assert_eq!(signal["type"], "keel.blocked");
    assert_eq!(signal["source"]["id"], "plan");
    assert_eq!(signal["proposal"]["id"], "draft-implement");
    let evidence = signal["evidence"].as_array().unwrap();
    for exact in [
        format!("head:{head}"),
        "task:issue-1045".into(),
        "node:plan".into(),
        "finding:keel.scope.path_outside_card".into(),
        "path:docs/notes.md".into(),
    ] {
        assert!(
            evidence.iter().any(|value| value == &exact),
            "missing {exact}: {signal}"
        );
    }
    assert_eq!(
        history
            .iter()
            .filter(|event| matches!(event.kind, graphhelm_protocols::EventKind::DraftProposed(_)))
            .count(),
        1
    );
    let stale = scratch.path().join("stale.json");
    fs::write(
        &stale,
        serde_json::to_vec(&serde_json::json!({
            "expectedHead": "0000000000000000000000000000000000000000",
            "execution": "exec_feature", "task": "issue-1045", "node": "route-correction",
            "implementDraft": draft("draft-stale-implement", 1, graph_hash, "stale-implement"),
            "reviewDraft": draft("draft-stale-review", 1, graph_hash, "stale-review")
        }))
        .unwrap(),
    )
    .unwrap();
    let (stale_code, stale_reply) = check(Some(&stale), &scope_card);
    assert_eq!(stale_code, Some(3), "{stale_reply}");
    assert_eq!(stale_reply["data"]["recommendation"]["step"], "implement");
    assert_eq!(
        stale_reply["diagnostics"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()["path"],
        "/routeProposal/expectedHead"
    );
    fs::write(repo.path().join("docs/notes.md"), "corrected scope\n").unwrap();
    fs::write(
        repo.path().join("core/events/src/journal.rs"),
        "pub fn changed() {}\npub fn corrected() {}\n",
    )
    .unwrap();
    git(repo.path(), &["add", "docs/notes.md"]);
    git(repo.path(), &["add", "core/events/src/journal.rs"]);
    git(repo.path(), &["commit", "-q", "-m", "correct scope"]);
    let corrected_head = graphhelm_process::command("git")
        .arg("-C")
        .arg(repo.path())
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    let corrected_head = String::from_utf8(corrected_head.stdout)
        .unwrap()
        .trim()
        .to_owned();
    let passing_card = card(
        scratch.path(),
        &["src/lib.rs", "docs/notes.md", "core/events/src/journal.rs"],
    );
    fs::write(
        &binding,
        serde_json::to_vec(&serde_json::json!({
            "expectedHead": corrected_head,
            "execution": "exec_feature",
            "task": "issue-1045",
            "node": "route-correction",
            "implementDraft": draft("draft-implement-next", 2, published_hash, "route-correction-next"),
            "reviewDraft": draft("draft-review", 2, published_hash, "route-review")
        }))
        .unwrap(),
    )
    .unwrap();
    let (pass_code, passed) = check(Some(&binding), &passing_card);
    assert_eq!(pass_code, Some(0), "{passed}");
    assert_eq!(passed["data"]["routeProposal"]["kind"], "keel.passed");
    let before_review_approval = Command::cargo_bin("graphhelm")
        .unwrap()
        .args(["--json", "graph", "replay", "--events"])
        .arg(&events)
        .output()
        .unwrap();
    assert!(before_review_approval.status.success());
    let before_review_approval: Value =
        serde_json::from_slice(&before_review_approval.stdout).unwrap();
    assert_eq!(
        before_review_approval["data"]["currentGraph"]["number"], 2,
        "{before_review_approval}"
    );
    let status = Command::cargo_bin("graphhelm")
        .unwrap()
        .args(["--json", "execution", "status", "--events"])
        .arg(&events)
        .args(["--execution", "exec_feature"])
        .output()
        .unwrap();
    let status: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status["data"]["signalsRecorded"], 2, "{status}");
    assert!(status["data"]["attentionReasons"].as_array().unwrap().iter()
        .any(|reason| reason["kind"] == "pending_draft" && reason["draft_id"] == "draft-review"), "{status}");
    let review_digest = status["data"]["proposalDigests"]["draft-review"]
        .as_str()
        .expect("status exposes the exact pending review proposal digest");
    let review_approval = Command::cargo_bin("graphhelm")
        .unwrap()
        .args(["execution", "approve", "--events"])
        .arg(&events)
        .args([
            "--execution",
            "exec_feature",
            "--node",
            "route-review",
            "--keyring",
        ])
        .arg(&keyring)
        .args([
            "--key-id",
            "route-key",
            "--actor-id",
            "route-agent",
            "--proposal-digest",
            review_digest,
            "--draft-id",
            "draft-review",
        ])
        .env("GRAPHHELM_EVENTS_KEY", key)
        .output()
        .unwrap();
    assert!(
        review_approval.status.success(),
        "review proposal should apply after its own exact approval: {}",
        String::from_utf8_lossy(&review_approval.stdout)
    );
    let reviewed_graph = Command::cargo_bin("graphhelm")
        .unwrap()
        .args(["--json", "graph", "replay", "--events"])
        .arg(&events)
        .output()
        .unwrap();
    assert!(reviewed_graph.status.success());
    let reviewed_graph: Value = serde_json::from_slice(&reviewed_graph.stdout).unwrap();
    assert_eq!(
        reviewed_graph["data"]["currentGraph"]["number"], 3,
        "{reviewed_graph}"
    );
    let final_status = Command::cargo_bin("graphhelm")
        .unwrap()
        .args(["--json", "execution", "status", "--events"])
        .arg(&events)
        .args(["--execution", "exec_feature"])
        .output()
        .unwrap();
    assert!(final_status.status.success());
    let final_status: Value = serde_json::from_slice(&final_status.stdout).unwrap();
    assert_eq!(
        final_status["data"]["nodeStates"]["route-correction"], "ready",
        "{final_status}"
    );
    assert_eq!(
        final_status["data"]["nodeStates"]["route-review"], "ready",
        "{final_status}"
    );

    let (_, history) = store.read_unique_replay_stream().unwrap();
    assert_eq!(
        history
            .iter()
            .filter(|event| matches!(
                event.kind,
                graphhelm_protocols::EventKind::SignalRecorded(_)
            ))
            .count(),
        2
    );
    let recorded = history
        .iter()
        .filter(|event| {
            matches!(
                event.kind,
                graphhelm_protocols::EventKind::SignalRecorded(_)
            )
        })
        .nth(1)
        .unwrap();
    let reference = recorded.evidence_refs.first().unwrap();
    let sealed = match store
        .sealed_evidence(&stream.scope, reference.evidence_id())
        .unwrap()
    {
        graphhelm_events::EvidenceRead::Available(sealed) => sealed,
        other => panic!("signal evidence unavailable: {other:?}"),
    };
    let envelope = runtime
        .block_on(protector.open(stream.scope.clone(), &sealed))
        .unwrap();
    let signal: Value = envelope.expose(|bytes| serde_json::from_slice(bytes).unwrap());
    assert_eq!(signal["type"], "keel.passed");
    assert!(
        signal["evidence"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| { value == "finding:keel.invariant.persistence" }),
        "warning finding ID missing from evidence: {signal}"
    );
    assert!(
        signal["evidence"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| { value == "path:core/events/src/journal.rs" }),
        "warning finding path missing from evidence: {signal}"
    );
}

fn run_verdict(comments: &Path, head: &str, reviewer: &str) -> (i32, Value) {
    let output = Command::cargo_bin("graphhelm")
        .unwrap()
        .args([
            "--json",
            "keel",
            "verdict",
            "--head",
            head,
            "--reviewer",
            reviewer,
            "--comments",
        ])
        .arg(comments)
        .output()
        .unwrap();
    let reply: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
        panic!(
            "expected a structured verdict result, got: {}",
            String::from_utf8_lossy(&output.stdout)
        )
    });
    (output.status.code().unwrap_or(1), reply)
}

/// The offline reader admits only a matching reviewer and head, and exposes matching BLOCKs.
/// Cost: six short CLI processes and one small local JSON snapshot; no server or network.
#[test]
fn keel_verdict_matches_only_the_requested_head_and_reviewer() {
    let snapshot = tempfile::tempdir().unwrap();
    let comments = snapshot.path().join("comments.json");
    let head_a = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let head_b = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    for (body, requested_head, reviewer, comment_id, expected) in [
        (
            "APPROVE\n\nSession: codex-review · Head: aaaaaaaa",
            head_a,
            "codex-review",
            "approval-a",
            serde_json::json!([{"commentId":"approval-a","verdict":"APPROVE"}]),
        ),
        (
            "APPROVE | looks good\n\nSession: codex-review · Head: bbbbbbbb",
            head_b,
            "codex-review",
            "approval-b",
            serde_json::json!([{"commentId":"approval-b","verdict":"APPROVE"}]),
        ),
        (
            "APPROVE\n\nSession: codex-review · Head: aaaaaaaa",
            head_b,
            "codex-review",
            "approval-a",
            serde_json::json!([]),
        ),
        (
            "APPROVE\n\nSession: another-reviewer · Head: bbbbbbbb",
            head_b,
            "codex-review",
            "approval-b",
            serde_json::json!([]),
        ),
        (
            "APPROVE\n\nSession: codex-review Head: bbbbbbbb",
            head_b,
            "codex-review",
            "approval-b",
            serde_json::json!([]),
        ),
        (
            "BLOCK: stale assumption\n\nSession: codex-review · Head: bbbbbbbb",
            head_b,
            "codex-review",
            "block-b",
            serde_json::json!([{"commentId":"block-b","verdict":"BLOCK"}]),
        ),
        (
            "Session: codex-review · Head: bbbbbbbb\n\n**BLOCK**\n\nFinding",
            head_b,
            "codex-review",
            "identity-first-block",
            serde_json::json!([{"commentId":"identity-first-block","verdict":"BLOCK"}]),
        ),
        (
            "Session: codex-review · Head: bbbbbbbb\n\n**BLOCK** (reason)",
            head_b,
            "codex-review",
            "bold-block-with-reason",
            serde_json::json!([{"commentId":"bold-block-with-reason","verdict":"BLOCK"}]),
        ),
    ] {
        fs::write(
            &comments,
            serde_json::json!({"comments": [{"id": comment_id, "body": body}]}).to_string(),
        )
        .unwrap();
        let (exit, reply) = run_verdict(&comments, requested_head, reviewer);
        assert_eq!(exit, 0, "{reply}");
        assert_eq!(reply["command"], "keel.verdict");
        assert_eq!(reply["data"]["verdicts"], expected);
        assert_eq!(
            reply["data"]["matchingBlockIds"],
            if ["block-b", "identity-first-block", "bold-block-with-reason"].contains(&comment_id) {
                serde_json::json!([comment_id])
            } else {
                serde_json::json!([])
            }
        );
    }
}

/// The offline reader rejects malformed and oversized snapshots before returning evidence.
/// Cost: three short CLI processes and a local fixture up to 1 MiB; no server or network.
#[test]
fn keel_verdict_refuses_malformed_and_oversized_snapshots() {
    let snapshot = tempfile::tempdir().unwrap();
    let comments = snapshot.path().join("comments.json");
    let head = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fs::write(&comments, r#"{"comments":[],"unexpected":true}"#).unwrap();
    let (exit, reply) = run_verdict(&comments, head, "codex-review");
    assert_eq!(exit, 3);
    assert_eq!(reply["command"], "keel.verdict");
    assert_eq!(reply["diagnostics"][0]["code"], "GHCLI031_KEEL_CHECK_INPUT");

    fs::write(&comments, vec![b' '; 1024 * 1024 + 1]).unwrap();
    let (exit, reply) = run_verdict(&comments, head, "codex-review");
    assert_eq!(exit, 3);
    assert_eq!(reply["command"], "keel.verdict");
    assert_eq!(reply["diagnostics"][0]["code"], "GHCLI031_KEEL_CHECK_INPUT");

    fs::write(&comments, r#"{"comments":[]}"#).unwrap();
    let (exit, reply) = run_verdict(&comments, "not-a-full-sha", "codex-review");
    assert_eq!(exit, 3);
    assert_eq!(reply["command"], "keel.verdict");
    assert_eq!(reply["diagnostics"][0]["code"], "GHCLI031_KEEL_CHECK_INPUT");
}

/// Validation must expose an invalid discovered flow as a failure, rather than treating a
/// non-executed journey as clean coverage. This protects the user-visible negative-evidence
/// boundary; existing keel check tests do not invoke the validation supervisor.
#[test]
fn validation_reports_invalid_flow_and_no_dead_code_verdict() {
    let repo = repository(&[]);
    let journeys = repo.path().join(".graphhelm/journeys");
    fs::create_dir_all(&journeys).unwrap();
    fs::write(journeys.join("broken.journey.yaml"), "not: [valid").unwrap();
    let output = Command::cargo_bin("graphhelm")
        .unwrap()
        .args([
            "--json",
            "keel",
            "test",
            "validation",
            "--repo",
            repo.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(envelope["command"], "keel.test.validation");
    assert_eq!(envelope["data"]["deadConfirmed"], serde_json::json!([]));
    assert_eq!(envelope["data"]["failures"][0]["reason"], "flow_invalid");
}

fn codes(reply: &Value) -> Vec<String> {
    reply["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["code"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn a_diff_inside_its_card_passes_and_reports_its_surface() {
    let repo = repository(&[]);
    let scratch = tempfile::tempdir().unwrap();
    let (code, reply) = run(repo.path(), Some(&card(scratch.path(), &["src"])));
    assert_eq!(code, 0, "{reply}");
    assert_eq!(reply["ok"], true);
    assert_eq!(reply["command"], "keel");
    assert_eq!(reply["data"]["policyVersion"], "1.6.0");
    assert_eq!(reply["data"]["cardDeclared"], true);
    assert_eq!(reply["data"]["surface"]["changedFiles"], 1);
    assert_eq!(reply["data"]["surface"]["newPublicSymbols"], 1);
    assert_eq!(reply["data"]["surface"]["cardExportedSymbols"], 1);
    assert_eq!(
        reply["data"]["surface"]["undeclaredPublicSymbols"],
        serde_json::json!([])
    );
    assert!(codes(&reply).is_empty(), "{reply}");
    assert_eq!(reply["data"]["recommendation"]["step"], "review");
    assert_eq!(
        reply["data"]["recommendation"]["head"]
            .as_str()
            .unwrap()
            .len(),
        40
    );
    assert_eq!(reply["data"]["recommendation"]["applied"], false);
}

#[test]
fn a_path_outside_the_card_blocks_with_exit_2_and_keeps_the_report() {
    let repo = repository(&[("docs/notes.md", "an unplanned edit\n")]);
    let scratch = tempfile::tempdir().unwrap();
    let (code, reply) = run(repo.path(), Some(&card(scratch.path(), &["src/lib.rs"])));
    assert_eq!(code, 2, "{reply}");
    assert_eq!(reply["ok"], false);
    assert_eq!(codes(&reply), vec!["keel.scope.path_outside_card"]);
    assert_eq!(reply["diagnostics"][0]["severity"], "error");
    assert_eq!(reply["diagnostics"][0]["path"], "docs/notes.md");
    assert_eq!(reply["data"]["refused"], true);
    assert_eq!(reply["data"]["surface"]["changedFiles"], 2);
    assert_eq!(reply["data"]["recommendation"]["step"], "implement");
    assert_eq!(
        reply["data"]["recommendation"]["head"]
            .as_str()
            .unwrap()
            .len(),
        40
    );
    assert_eq!(
        reply["data"]["recommendation"]["findingIds"],
        serde_json::json!(["keel.scope.path_outside_card"])
    );
    assert_eq!(
        reply["data"]["recommendation"]["paths"],
        serde_json::json!(["docs/notes.md"])
    );
    assert_eq!(reply["data"]["recommendation"]["applied"], false);
}

#[test]
fn a_sneaked_in_test_file_is_counted_and_blocked_when_outside_the_card() {
    let repo = repository(&[("tests/extra.rs", "#[test]\nfn sneaked() {}\n")]);
    let scratch = tempfile::tempdir().unwrap();
    let (code, reply) = run(repo.path(), Some(&card(scratch.path(), &["src"])));
    assert_eq!(code, 2, "{reply}");
    assert_eq!(codes(&reply), vec!["keel.scope.path_outside_card"]);
    assert_eq!(reply["data"]["surface"]["newTestFiles"], 1);
    assert_eq!(reply["data"]["surface"]["newTests"], 1);
}

#[test]
fn without_a_card_nothing_about_scope_blocks() {
    let repo = repository(&[("docs/notes.md", "an unplanned edit\n")]);
    let (code, reply) = run(repo.path(), None);
    assert_eq!(code, 0, "{reply}");
    assert_eq!(reply["data"]["cardDeclared"], false);
    assert!(
        !codes(&reply).contains(&"keel.scope.path_outside_card".to_owned()),
        "{reply}"
    );
}

#[test]
fn check_guidance_keeps_docs_summary_only() {
    let repo = repository(&[("docs/notes.md", "documentation update\n")]);
    fs::write(repo.path().join("src/lib.rs"), "pub fn old() {}\n").unwrap();
    git(repo.path(), &["add", "src/lib.rs"]);
    git(repo.path(), &["commit", "--amend", "-q", "--no-edit"]);
    let (code, reply) = run(repo.path(), None);
    assert_eq!(code, 0, "{reply}");
    assert_eq!(reply["data"]["cardGuidance"]["kind"], "summary_only");
    assert_eq!(reply["data"]["cardGuidance"]["fulfilled"], false);
}

#[test]
fn check_guidance_names_bounded_code_card_obligations() {
    let repo = repository(&[]);
    let (code, reply) = run(repo.path(), None);
    assert_eq!(code, 0, "{reply}");
    assert_eq!(reply["data"]["cardGuidance"]["kind"], "bounded_code");
    assert_eq!(
        reply["data"]["cardGuidance"]["required"],
        serde_json::json!(["promise", "scope", "proof"])
    );
    assert_eq!(reply["data"]["cardGuidance"]["fulfilled"], false);
}

#[test]
fn check_guidance_keeps_one_line_persistence_change_on_expanded_route() {
    let repo = repository(&[("core/events/src/journal.rs", "pub fn x() {}\n")]);
    let (code, reply) = run(repo.path(), None);
    assert_eq!(code, 0, "warnings never refuse: {reply}");
    assert_eq!(reply["data"]["cardGuidance"]["kind"], "expanded");
    assert_eq!(
        reply["data"]["cardGuidance"]["required"],
        serde_json::json!(["full_card", "defect_naming_proof", "jpd"])
    );
    assert_eq!(reply["data"]["cardGuidance"]["fulfilled"], false);
    assert!(
        codes(&reply)
            .iter()
            .any(|code| code == "keel.invariant.persistence")
    );
}

#[test]
fn a_range_that_is_not_a_range_or_a_card_that_is_not_a_card_is_input_error_exit_3() {
    let repo = repository(&[]);
    let output = Command::cargo_bin("graphhelm")
        .unwrap()
        .args(["--json", "keel", "check", "--diff=--output=x", "--repo"])
        .arg(repo.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&output.stdout).contains("GHCLI031_KEEL_CHECK_INPUT"));

    let scratch = tempfile::tempdir().unwrap();
    let bad = scratch.path().join("card.json");
    fs::write(&bad, br#"{"scopePaths":["src"],"unknown":1}"#).unwrap();
    let (code, reply) = run(repo.path(), Some(&bad));
    assert_eq!(code, 3, "{reply}");
    assert_eq!(codes(&reply), vec!["GHS002_SCHEMA"]);
    assert!(reply["data"]["recommendation"].is_null());
}

#[test]
fn an_option_shaped_base_or_head_is_input_error_exit_3() {
    let repo = repository(&[]);
    for range in ["HEAD~1..-x", "-x..HEAD", "HEAD~1...-x"] {
        let output = Command::cargo_bin("graphhelm")
            .unwrap()
            .args([
                "--json",
                "keel",
                "check",
                &format!("--diff={range}"),
                "--repo",
            ])
            .arg(repo.path())
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        assert_eq!(output.status.code(), Some(3), "{range}: {stdout}");
        let reply: Value = serde_json::from_str(&stdout).unwrap();
        assert_eq!(
            reply["diagnostics"][0]["path"], "/diff",
            "{range}: {stdout}"
        );
        // A range that itself starts with `-` is refused by the earlier check; the others must be
        // refused by ours, before git can read the revision as an option.
        if !range.starts_with('-') {
            assert!(
                reply["diagnostics"][0]["message"]
                    .as_str()
                    .is_some_and(|m| m.contains("may not start with '-'")),
                "{range}: refused by us, not by git: {stdout}"
            );
        }
    }
}

#[test]
fn a_schema_invalid_card_is_rejected_with_the_failing_field_path() {
    let repo = repository(&[]);
    let scratch = tempfile::tempdir().unwrap();
    let bad = scratch.path().join("card.json");
    fs::write(
        &bad,
        br#"{"promise":"","scopePaths":["src"],"proof":"cargo test"}"#,
    )
    .unwrap();

    let (code, reply) = run(repo.path(), Some(&bad));
    assert_eq!(code, 3, "{reply}");
    assert_eq!(reply["ok"], false);
    assert_eq!(codes(&reply), vec!["GHS002_SCHEMA"]);
    assert_eq!(reply["diagnostics"][0]["path"], "/promise");
    assert_eq!(reply["diagnostics"][0]["source"], "keel-card");
}

/// #1333: `--prove-new-tests` on a fix whose inline `mod tests` adds one test that detects the bug
/// and one that asserts its own arithmetic. Only the second is a signal, the checkout is untouched,
/// and no proving worktree outlives the command.
#[test]
fn prove_new_tests_earns_the_regression_test_and_flags_the_one_green_on_the_parent() {
    let repo = tempfile::tempdir().unwrap();
    let write = |path: &str, text: &str| {
        let full = repo.path().join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, text).unwrap();
    };
    write(
        "Cargo.toml",
        "[package]\nname = \"subject\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
    );
    write(".gitignore", "/target\nCargo.lock\n");
    write(
        "src/lib.rs",
        "pub fn double(x: u32) -> u32 {\nx + x + 1\n}\n",
    );
    git(repo.path(), &["init", "-q"]);
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "-q", "-m", "base"]);
    write(
        "src/lib.rs",
        "pub fn double(x: u32) -> u32 {\nx + x\n}\n\n#[cfg(test)]\nmod tests {\nuse super::*;\n\n#[test]\nfn double_of_two_is_four() {\nassert_eq!(double(2), 4);\n}\n\n#[test]\nfn arithmetic_still_works() {\nassert_eq!(2 + 2, 4);\n}\n}\n",
    );
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "-q", "-m", "fix"]);
    let target = tempfile::tempdir().unwrap();
    let output = Command::cargo_bin("graphhelm")
        .unwrap()
        .args([
            "--json",
            "keel",
            "check",
            "--diff",
            "HEAD~1..HEAD",
            "--prove-new-tests",
            "--prove-timeout-secs",
            "300",
            "--repo",
        ])
        .arg(repo.path())
        .arg("--prove-target-dir")
        .arg(target.path())
        .output()
        .unwrap();
    let reply: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(output.status.code(), Some(0), "{reply}");
    let proofs = reply["data"]["testProof"]["proofs"].as_array().unwrap();
    let verdict_of = |name: &str| {
        proofs
            .iter()
            .find(|proof| proof["name"] == name)
            .map(|proof| {
                (
                    proof["parent"]["outcome"].clone(),
                    proof["head"]["outcome"].clone(),
                    proof["verdict"].clone(),
                )
            })
            .unwrap_or_else(|| panic!("{name} not proven: {reply}"))
    };
    assert_eq!(
        verdict_of("double_of_two_is_four"),
        ("failed".into(), "passed".into(), "earned".into()),
        "{reply}"
    );
    assert_eq!(
        verdict_of("arithmetic_still_works"),
        ("passed".into(), "passed".into(), "green_on_parent".into()),
        "{reply}"
    );
    assert_eq!(codes(&reply), vec!["keel.test.green_on_parent"], "{reply}");

    let status = graphhelm_process::command("git")
        .arg("-C")
        .arg(repo.path())
        .args(["status", "--porcelain"])
        .output()
        .unwrap();
    assert!(status.stdout.is_empty(), "the checkout was written");
    let worktrees = graphhelm_process::command("git")
        .arg("-C")
        .arg(repo.path())
        .args(["worktree", "list", "--porcelain"])
        .output()
        .unwrap();
    let listed = String::from_utf8_lossy(&worktrees.stdout);
    assert_eq!(listed.matches("worktree ").count(), 1, "{listed}");
}

#[test]
fn an_incomplete_inline_graft_does_not_claim_the_subject_is_new() {
    let repo = tempfile::tempdir().unwrap();
    let write = |text: &str| {
        fs::create_dir_all(repo.path().join("src")).unwrap();
        fs::write(repo.path().join("src/lib.rs"), text).unwrap();
    };
    fs::write(
        repo.path().join("Cargo.toml"),
        "[package]\nname = \"keel_graft_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    git(repo.path(), &["init", "-q"]);
    write("pub fn fixed() -> u32 { 1 }\n");
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "-q", "-m", "base"]);
    write(
        "pub fn fixed() -> u32 { 1 }\n\n#[cfg(test)]\nmod tests {\nuse super::*;\nfn expected_value() -> u32 { 1 }\n#[test]\nfn fixed_is_one() { assert_eq!(fixed(), expected_value()); }\n}\n",
    );
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "-q", "-m", "test"]);
    let target = tempfile::tempdir().unwrap();
    let output = Command::cargo_bin("graphhelm")
        .unwrap()
        .args(["--json", "keel", "check", "--repo"])
        .arg(repo.path())
        .args([
            "--diff",
            "HEAD~1..HEAD",
            "--prove-new-tests",
            "--prove-timeout-secs",
            "30",
            "--prove-target-dir",
        ])
        .arg(target.path())
        .output()
        .unwrap();
    let reply: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(output.status.code(), Some(0), "{reply}");
    let proof = &reply["data"]["testProof"]["proofs"][0];
    assert_eq!(proof["parent"]["outcome"], "did_not_compile");
    assert!(
        proof["parent"]["detail"]
            .as_str()
            .unwrap()
            .contains("expected_value")
    );
    assert_eq!(proof["head"]["outcome"], "passed");
    assert_eq!(proof["verdict"], "unproven");
    assert_eq!(
        reply["data"]["testProof"]["findings"][0]["rule"],
        "keel.test.unproven"
    );
}

/// #144: parameterized TypeScript tests are counted by the surface classifier and must remain
/// visible in the proof report when no TypeScript runner is available.
#[test]
fn prove_new_tests_reports_parameterized_typescript_tests_as_unproven() {
    let repo = repository(&[(
        "studio/example.test.ts",
        "it.each([[1]])('parameterized it', () => {});\ntest.each([[2]])(\"parameterized test\", () => {});\n",
    )]);
    let target = tempfile::tempdir().unwrap();
    let output = Command::cargo_bin("graphhelm")
        .unwrap()
        .args([
            "--json",
            "keel",
            "check",
            "--diff",
            "HEAD~1..HEAD",
            "--prove-new-tests",
            "--repo",
        ])
        .arg(repo.path())
        .arg("--prove-target-dir")
        .arg(target.path())
        .output()
        .unwrap();
    let reply: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(output.status.code(), Some(0), "{reply}");
    assert_eq!(reply["data"]["surface"]["newTests"], 2, "{reply}");
    let proofs = reply["data"]["testProof"]["proofs"].as_array().unwrap();
    assert_eq!(proofs.len(), 2, "{reply}");
    for name in ["parameterized it", "parameterized test"] {
        let proof = proofs
            .iter()
            .find(|proof| proof["name"] == name)
            .unwrap_or_else(|| panic!("{name} not reported: {reply}"));
        assert_eq!(proof["verdict"], "unproven", "{reply}");
        assert_eq!(proof["parent"]["outcome"], "not_run", "{reply}");
        assert_eq!(proof["head"]["outcome"], "not_run", "{reply}");
    }
}

#[test]
fn commits_that_reached_the_base_after_the_branch_was_cut_are_not_charged_to_it() {
    let repo = repository(&[]);
    git(repo.path(), &["branch", "-q", "feature"]);
    git(repo.path(), &["checkout", "-q", "-b", "main", "HEAD~1"]);
    fs::create_dir_all(repo.path().join("tests")).unwrap();
    fs::write(
        repo.path().join("tests/someone_else.rs"),
        "#[test]\nfn theirs() {}\n",
    )
    .unwrap();
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "-q", "-m", "someone else merged"]);
    let card = card(repo.path(), &["src/lib.rs"]);
    for range in ["main..feature", "main...feature"] {
        let output = Command::cargo_bin("graphhelm")
            .unwrap()
            .args(["--json", "keel", "check", "--diff", range, "--repo"])
            .arg(repo.path())
            .arg("--card")
            .arg(&card)
            .output()
            .unwrap();
        let reply: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(output.status.code(), Some(0), "{range}: {reply}");
        assert!(codes(&reply).is_empty(), "{range}: {reply}");
    }
}

/// ml-saas trial: a TypeScript regression test is proven from the runner's JUnit report, red on
/// the parent and green on the head. Only the report decides (#269 review): a runner that cannot
/// start, or one that never reports the test, is `not_run`, even for a one-letter test name.
#[cfg(unix)]
#[test]
fn prove_command_proves_a_typescript_test_from_its_junit_report() {
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-q"]);
    fs::write(repo.path().join("value.txt"), "broken\n").unwrap();
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "-q", "-m", "base"]);
    fs::write(repo.path().join("value.txt"), "fixed\n").unwrap();
    fs::create_dir_all(repo.path().join("tests")).unwrap();
    fs::write(
        repo.path().join("tests/value.test.ts"),
        "it('t', () => {});\n",
    )
    .unwrap();
    fs::write(repo.path().join("run.sh"), "exit 0\n").unwrap();
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "-q", "-m", "fix"]);
    let run = |command: &str| {
        let output = Command::cargo_bin("graphhelm")
            .unwrap()
            .args([
                "--json",
                "keel",
                "check",
                "--diff",
                "HEAD~1..HEAD",
                "--prove-new-tests",
                "--prove-command",
                command,
                "--repo",
            ])
            .arg(repo.path())
            .output()
            .unwrap();
        let reply: Value = serde_json::from_slice(&output.stdout).unwrap();
        (output.status.code().unwrap(), reply)
    };
    let proof = |command: &str| {
        let (_, reply) = run(command);
        let proof = reply["data"]["testProof"]["proofs"][0].clone();
        assert_eq!(proof["name"], "t", "{reply}");
        proof
    };
    // A runner that reports `t` failing on the parent and passing on the head.
    let junit = "if grep -q fixed value.txt; then body=''; else body='<failure/>'; fi; \
                 printf '<testsuite><testcase name=\"%s\">%s</testcase></testsuite>' {name} \"$body\" > {report}";
    let earned = proof(junit);
    assert_eq!(earned["parent"]["outcome"], "failed", "{earned}");
    assert_eq!(earned["head"]["outcome"], "passed", "{earned}");
    assert_eq!(earned["verdict"], "earned", "{earned}");
    let green = proof("printf '<testsuite><testcase name=\"%s\"/></testsuite>' {name} > {report}");
    assert_eq!(green["verdict"], "green_on_parent", "{green}");
    // Cannot start: the error text names `t` ("not found", "test") but writes no report.
    let missing = proof("no-such-runner-keel {file} {name} {report}");
    assert_eq!(missing["parent"]["outcome"], "not_run", "{missing}");
    assert_eq!(missing["verdict"], "unproven", "{missing}");
    // Script only on the head: the parent run writes no report; the head reports a pass.
    let absent = proof(
        "sh run.sh && printf '<testsuite><testcase name=\"%s\"/></testsuite>' {name} > {report}",
    );
    assert_eq!(absent["parent"]["outcome"], "not_run", "{absent}");
    assert_eq!(absent["head"]["outcome"], "passed", "{absent}");
    assert_eq!(absent["verdict"], "unproven", "{absent}");
    // A report that fails some other test, whose name contains `t`, does not fail `t`.
    let other = proof(
        "printf '<testsuite><testcase name=\"other test\"><failure/></testcase></testsuite>' > {report}",
    );
    assert_eq!(other["parent"]["outcome"], "not_run", "{other}");
    assert_eq!(other["verdict"], "unproven", "{other}");
    // A command with no report placeholder is refused before anything runs.
    let (code, reply) = run("true {file} {name}");
    assert_eq!(code, 3, "{reply}");
}

/// ml-saas trial: the card lives as prose in the PR body. A `.md` card is read from its
/// `Promise:` / `Scope:` / `Proof:` / `Exported:` lines, so the scope rule runs on it.
#[test]
fn a_card_written_in_a_pr_body_runs_the_scope_rule() {
    let repo = repository(&[("docs/notes.md", "an unplanned edit\n")]);
    let scratch = tempfile::tempdir().unwrap();
    let body = scratch.path().join("pr-body.md");
    fs::write(
        &body,
        "Before: x.\n\nKeel card:\n- **Promise:** added() exists\n- **Scope:** `src/lib.rs`\n- **Proof:** `cargo test`\n- Exported: `added`\n\nCloses #1\n",
    )
    .unwrap();
    let (code, reply) = run(repo.path(), Some(&body));
    assert_eq!(code, 2, "{reply}");
    assert_eq!(codes(&reply), vec!["keel.scope.path_outside_card"]);
    assert_eq!(reply["diagnostics"][0]["path"], "docs/notes.md");
    assert_eq!(reply["data"]["surface"]["cardScopePaths"], 1, "{reply}");
    // Without a Scope line the card is refused with the missing field named, not run unscoped.
    fs::write(&body, "- Promise: added() exists\n- Proof: `cargo test`\n").unwrap();
    let (code, reply) = run(repo.path(), Some(&body));
    assert_ne!(code, 0, "{reply}");
    assert!(reply.to_string().contains("scopePaths"), "{reply}");
}

#[test]
fn a_card_written_with_multiline_scope_lists_all_paths() {
    let repo = repository(&[
        ("src/lib.rs", "planned edit\n"),
        ("docs/notes.md", "planned edit\n"),
    ]);
    let scratch = tempfile::tempdir().unwrap();
    let body = scratch.path().join("pr-body.md");
    fs::write(
        &body,
        "Promise: added() exists\nScope:\n- `src/lib.rs`\n- `docs/notes.md`\nProof: `cargo test`\n",
    )
    .unwrap();
    let (code, reply) = run(repo.path(), Some(&body));
    assert_eq!(code, 0, "{reply}");
    assert_eq!(reply["data"]["surface"]["cardScopePaths"], 2, "{reply}");
}

#[test]
fn a_card_naming_journeys_validates_and_a_bad_journey_id_is_refused_at_its_path() {
    let repo = repository(&[]);
    let path = repo.path().join("card.json");
    let mut card = serde_json::json!({
        "promise": "added() exists", "scopePaths": ["src"], "proof": "cargo test",
        "exportedSymbols": ["added"], "journeys": ["checkout"],
    });
    fs::write(&path, serde_json::to_vec(&card).unwrap()).unwrap();
    let (code, reply) = run(repo.path(), Some(&path));
    assert_eq!(code, 0, "{reply}");
    card["journeys"] = serde_json::json!(["../etc"]);
    fs::write(&path, serde_json::to_vec(&card).unwrap()).unwrap();
    let (code, reply) = run(repo.path(), Some(&path));
    assert_eq!(code, 3, "{reply}");
    assert_eq!(reply["diagnostics"][0]["path"], "/journeys/0", "{reply}");
    card["journeys"] = serde_json::json!(["a..b"]);
    fs::write(&path, serde_json::to_vec(&card).unwrap()).unwrap();
    let (code, reply) = run(repo.path(), Some(&path));
    assert_eq!(code, 3, "{reply}");
    assert_eq!(reply["diagnostics"][0]["path"], "/journeys/0", "{reply}");
}

#[test]
fn a_card_written_in_a_pr_body_reads_its_journeys_line() {
    let repo = repository(&[]);
    let body = repo.path().join("body.md");
    fs::write(
        &body,
        "Promise: added() exists\nScope: `src`\nProof: `cargo test`\nExported: `added`\nJourneys: `checkout`\n",
    )
    .unwrap();
    let (code, reply) = run(repo.path(), Some(&body));
    assert_eq!(code, 0, "{reply}");
    assert!(
        codes(&reply).iter().any(
            |c| c == "keel.journey.no_fresh_capture" || c == "keel.journey.contract_unreadable"
        ),
        "{reply}"
    );
}

/// #382 phase A: a change under an invariant class of `keel.yaml` 1.4.0 is reported by class, so a
/// reviewer sees that a journey cannot be its proof. Credible regression: the class lists or the
/// prefix comparison stop matching and persistence changes ship with journey-only proof. Cost: one
/// temp git repository and one CLI run.
#[test]
fn a_diff_under_an_invariant_path_reports_its_class() {
    let repo = repository(&[("core/events/src/journal.rs", "pub fn x() {}\n")]);
    let scratch = tempfile::tempdir().unwrap();
    let (code, reply) = run(
        repo.path(),
        Some(&card(
            scratch.path(),
            &["src", "core/events/src/journal.rs"],
        )),
    );
    assert_eq!(code, 0, "warnings never refuse: {reply}");
    let found: Vec<&Value> = reply["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| d["code"] == "keel.invariant.persistence")
        .collect();
    assert_eq!(found.len(), 1, "{reply}");
    assert_eq!(found[0]["path"], "core/events/src/journal.rs");
    assert_eq!(found[0]["severity"], "warning");
    let (_, plain) = run(repo.path(), None);
    assert!(
        !codes(&plain).iter().any(|c| c == "keel.invariant.security"),
        "an untouched class is silent: {plain}"
    );
}

fn plan(repo: &Path, args: &[&str]) -> (i32, Value, Vec<u8>) {
    let output = Command::cargo_bin("graphhelm")
        .unwrap()
        .args(["--json", "keel", "plan", "--repo", repo.to_str().unwrap()])
        .args(args)
        .output()
        .unwrap();
    let reply: Value = serde_json::from_slice(&output.stdout).unwrap_or(Value::Null);
    (output.status.code().unwrap(), reply, output.stdout)
}

/// #382 phase B: the planner is deterministic and takes the highest class of the task's paths.
/// Credible regressions: an order- or clock-dependent record (two runs disagree), a code path
/// downgrading an invariant task, or a promise that names a permission while touching only docs
/// being planned as docs. Cost: one temp git repository, four CLI runs, no network.
#[test]
fn keel_plan_is_deterministic_and_takes_the_highest_class() {
    let repo = repository(&[("core/events/src/journal.rs", "pub fn x() {}\n")]);
    let args = [
        "--task",
        "issue-382",
        "--paths",
        "src/lib.rs",
        "core/events/src/journal.rs",
    ];
    let (code, first, first_bytes) = plan(repo.path(), &args);
    assert_eq!(code, 0, "{first}");
    let (_, _, second_bytes) = plan(repo.path(), &args);
    assert_eq!(first_bytes, second_bytes, "two runs must be byte-identical");
    let record = &first["data"]["plan"];
    assert_eq!(record["schema"], "graphhelm-task-plan-v1");
    assert_eq!(
        record["classes"],
        serde_json::json!(["code", "invariant"]),
        "{record}"
    );
    assert_eq!(
        record["invariantClasses"],
        serde_json::json!(["persistence"])
    );
    assert_eq!(record["proof"], "tests");
    assert_eq!(
        record["reviews"], 1,
        "one review for every class (owner decision)"
    );
    assert_eq!(
        record["delegation"],
        serde_json::json!({"kind": "implementer", "tier": "large", "effort": "high"})
    );
    assert_eq!(record["decidedBy"], "rules");
    assert_eq!(record["revision"].as_str().unwrap().len(), 40, "{record}");

    let (code, docs) = {
        let (code, reply, _) = plan(
            repo.path(),
            &[
                "--task",
                "issue-1",
                "--paths",
                "docs/a.md",
                "--promise",
                "fix the permission check",
            ],
        );
        (code, reply)
    };
    assert_eq!(code, 0, "{docs}");
    assert_eq!(docs["data"]["plan"]["classes"], serde_json::json!(["docs"]));
    assert_eq!(
        docs["data"]["plan"]["decidedBy"], "fallback_strict",
        "{docs}"
    );
    assert_eq!(docs["data"]["plan"]["delegation"]["tier"], "large");
    let (code, bad, _) = plan(repo.path(), &["--task", "issue-1", "--paths", "../etc"]);
    assert_eq!(code, 3, "{bad}");
}

/// #382 B2: on an ambiguous task (a promise naming a permission, docs-only paths) the planner asks
/// Jev `task_class`; an answer at or above the threshold decides, below it the plan stays strict,
/// and a missing answer is recorded, never invented. Credible regressions: Jev consulted when the
/// rules were clear, a low-confidence answer lowering the route, or a judge failure dropped
/// silently. Cost: one temp repository, a recorded judge file, four CLI runs, no network.
#[test]
fn keel_plan_asks_jev_only_when_ambiguous_and_takes_a_confident_answer() {
    let repo = repository(&[]);
    let fixture = repo.path().join("jev.json");
    let ambiguous = [
        "--task",
        "issue-2",
        "--paths",
        "docs/a.md",
        "--promise",
        "fix the permission check",
        "--judge-fixture",
        fixture.to_str().unwrap(),
    ];
    fs::write(&fixture, b"{\"answers\": {}}").unwrap();
    let (code, missing, _) = plan(repo.path(), &ambiguous);
    assert_eq!(code, 0, "{missing}");
    let record = &missing["data"]["plan"];
    assert_eq!(record["decidedBy"], "fallback_strict", "{record}");
    assert!(
        record["jev"]["error"].is_string(),
        "a missing answer is recorded: {record}"
    );
    let digest = record["jev"]["requestSha256"].as_str().unwrap().to_owned();

    let reply = |choice: &str, confidence: f64| {
        serde_json::json!({"answers": {digest.clone(): {
            "model": "jev-latest",
            "answers": {"task_class": {"type": "choice", "choice": choice,
                "probabilities": {choice: confidence}, "confidence": confidence}},
            "usage": {"input_tokens": 1, "output_tokens": 1}
        }}})
    };
    fs::write(&fixture, serde_json::to_vec(&reply("docs", 0.95)).unwrap()).unwrap();
    let (_, confident, _) = plan(repo.path(), &ambiguous);
    let record = &confident["data"]["plan"];
    assert_eq!(record["decidedBy"], "jev", "{record}");
    assert_eq!(record["jev"]["questions"]["task_class"]["answer"], "docs");
    assert_eq!(record["delegation"]["tier"], "small", "{record}");
    assert_eq!(record["proof"], "none");

    fs::write(&fixture, serde_json::to_vec(&reply("docs", 0.5)).unwrap()).unwrap();
    let (_, unsure, _) = plan(repo.path(), &ambiguous);
    assert_eq!(
        unsure["data"]["plan"]["decidedBy"], "fallback_strict",
        "{unsure}"
    );
    assert_eq!(unsure["data"]["plan"]["delegation"]["tier"], "large");

    let (_, clear, _) = plan(
        repo.path(),
        &[
            "--task",
            "issue-3",
            "--paths",
            "src/lib.rs",
            "--judge-fixture",
            fixture.to_str().unwrap(),
        ],
    );
    assert_eq!(clear["data"]["plan"]["decidedBy"], "rules", "{clear}");
    assert!(
        clear["data"]["plan"]["jev"].is_null(),
        "Jev is not asked when the rules are clear"
    );
}

/// Review of #405 (gh-claude-5's BLOCK): a path that is no invariant, no screen and no source file
/// is not plainly docs. A policy (`keel.yaml`) or a schema was planned as docs with proof `none`
/// and no keel; spec §6 calls that ambiguous and takes the stricter answer. Also: a promise word
/// must start with an invariant stem, so `trace` never reads as `race`. Cost: one temp repository,
/// three CLI runs.
#[test]
fn keel_plan_treats_unclassified_non_prose_paths_as_ambiguous() {
    let repo = repository(&[]);
    for path in [
        "extensions/builtin/graphhelm-development-contracts/policies/keel.yaml",
        "extensions/builtin/graphhelm-development-contracts/schemas/task-plan.schema.json",
    ] {
        let (code, reply, _) = plan(repo.path(), &["--task", "issue-405", "--paths", path]);
        assert_eq!(code, 0, "{reply}");
        let record = &reply["data"]["plan"];
        assert_eq!(record["decidedBy"], "fallback_strict", "{path}: {record}");
        assert_eq!(record["delegation"]["tier"], "large", "{path}: {record}");
        assert_ne!(record["proof"], "none", "{path}: {record}");
    }
    let (_, prose, _) = plan(
        repo.path(),
        &["--task", "issue-405", "--paths", "docs/guide.md"],
    );
    assert_eq!(prose["data"]["plan"]["decidedBy"], "rules", "{prose}");
    let (_, traced, _) = plan(
        repo.path(),
        &[
            "--task",
            "issue-405",
            "--paths",
            "src/lib.rs",
            "--promise",
            "add a trace line",
        ],
    );
    assert_eq!(traced["data"]["plan"]["decidedBy"], "rules", "{traced}");
}

/// Review of #406 (coordinator decision): Jev may choose among the strict classes but never below
/// the floor the paths set. A non-prose path (here `keel.yaml`) cannot become docs with proof
/// `none`, whatever Jev answers; the clamp is recorded in `keel.plan`. Credible regression: a
/// confident "docs" reply turning a policy edit into an unreviewed, unproven change. Cost: one
/// temp repository, two CLI runs, no network.
#[test]
fn keel_plan_never_lets_jev_lower_a_non_prose_path_to_docs() {
    let repo = repository(&[]);
    let fixture = repo.path().join("jev.json");
    let args = [
        "--task",
        "issue-406",
        "--paths",
        "extensions/builtin/graphhelm-development-contracts/policies/keel.yaml",
        "--judge-fixture",
        fixture.to_str().unwrap(),
    ];
    fs::write(&fixture, b"{\"answers\": {}}").unwrap();
    let (_, first, _) = plan(repo.path(), &args);
    let digest = first["data"]["plan"]["jev"]["requestSha256"]
        .as_str()
        .unwrap_or_else(|| panic!("{first}"))
        .to_owned();
    let reply = serde_json::json!({"answers": {digest: {
        "model": "jev-latest",
        "answers": {"task_class": {"type": "choice", "choice": "docs",
            "probabilities": {"docs": 0.99}, "confidence": 0.99}},
        "usage": {"input_tokens": 1, "output_tokens": 1}
    }}});
    fs::write(&fixture, serde_json::to_vec(&reply).unwrap()).unwrap();
    let (code, clamped, _) = plan(repo.path(), &args);
    assert_eq!(code, 0, "{clamped}");
    let record = &clamped["data"]["plan"];
    assert_eq!(record["decidedBy"], "jev", "{record}");
    assert_ne!(record["proof"], "none", "{record}");
    assert_ne!(record["delegation"]["tier"], "small", "{record}");
    assert_eq!(
        record["jev"]["clamp"],
        serde_json::json!({"from": "docs", "to": "code"}),
        "the clamp is recorded: {record}"
    );
}

/// #426: the generated `<id>.json` contracts are git-ignored, so a fresh clone holds only the
/// committed `*.journey.yaml` flows. Plan and check must read those flows (projected in memory as
/// `journey compile` would), and say so when a touched flow is a draft; the plan also says when no
/// journey exists at all.
/// Credible regression: the readers go back to the generated JSON and journey-first falls silent in
/// every reviewer's clone. Cost: one temp git repository, one approve, five CLI runs.
#[test]
fn a_fresh_clone_plans_and_checks_from_the_committed_flows() {
    let flow = include_str!("fixtures/journey_flow/checkout.journey.yaml");
    let draft = flow
        .replace("id: checkout", "id: browse")
        .replace("app/cart/page.tsx", "app/browse/page.tsx");
    let repo = tempfile::tempdir().unwrap();
    let root = repo.path();
    git(root, &["init", "-q"]);
    for path in [
        "app/cart/page.tsx",
        "app/checkout/page.tsx",
        "app/api/pay/route.ts",
        "app/browse/page.tsx",
    ] {
        fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
        fs::write(root.join(path), "export {}\n").unwrap();
    }
    let journeys = root.join(".graphhelm/journeys");
    fs::create_dir_all(&journeys).unwrap();
    fs::write(journeys.join("checkout.journey.yaml"), flow).unwrap();
    fs::write(journeys.join("browse.journey.yaml"), draft).unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "flows"]);
    let approve = Command::cargo_bin("graphhelm")
        .unwrap()
        .current_dir(root)
        .args([
            "--json",
            "journey",
            "approve",
            "checkout",
            "--token-file",
            &owner_token(root),
        ])
        .output()
        .unwrap();
    assert!(
        approve.status.success(),
        "{}",
        String::from_utf8_lossy(&approve.stdout)
    );
    // What a clone holds: the approved flow is committed, its generated contract is not.
    fs::remove_file(journeys.join("checkout.json")).unwrap();
    git(root, &["add", ".graphhelm/journeys/checkout.journey.yaml"]);
    git(root, &["commit", "-q", "-m", "approve"]);
    fs::write(root.join("app/cart/page.tsx"), "export const x = 1\n").unwrap();
    fs::write(root.join("app/browse/page.tsx"), "export const y = 1\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "change"]);

    let (code, reply, _) = plan(
        root,
        &[
            "--task",
            "t-1",
            "--paths",
            "app/cart/page.tsx",
            "app/browse/page.tsx",
        ],
    );
    assert_eq!(code, 0, "{reply}");
    assert_eq!(
        reply["data"]["plan"]["journeys"],
        serde_json::json!(["checkout"]),
        "{reply}"
    );
    assert_eq!(reply["data"]["plan"]["proof"], "journey", "{reply}");
    assert!(
        codes(&reply).iter().any(|c| c == "keel.journey.flow_draft"),
        "a touched draft flow is named: {reply}"
    );
    let draft = reply["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["code"] == "keel.journey.flow_draft")
        .unwrap();
    assert!(
        draft["message"]
            .as_str()
            .unwrap()
            .contains("browse: journey exists, not yet approved by the owner"),
        "a draft is a journey awaiting approval, never no journey: {draft}"
    );
    assert!(
        !codes(&reply).iter().any(|c| c == "keel.journey.none"),
        "{reply}"
    );

    let (code, reply) = run(root, None);
    assert_eq!(code, 0, "{reply}");
    assert_eq!(reply["data"]["cardGuidance"]["kind"], "bounded_code");
    assert_eq!(
        reply["data"]["cardGuidance"]["journeys"],
        serde_json::json!(["checkout"]),
        "guidance names only the touched approved journey: {reply}"
    );
    let found = codes(&reply);
    assert!(
        found
            .iter()
            .any(|c| c == "keel.journey.card_missing_journey"),
        "{reply}"
    );
    assert!(
        found.iter().any(|c| c == "keel.journey.flow_draft"),
        "{reply}"
    );

    let bare = repository(&[]);
    let (code, reply, _) = plan(bare.path(), &["--task", "t-2", "--paths", "src/lib.rs"]);
    assert_eq!(code, 0, "{reply}");
    assert!(
        codes(&reply).iter().any(|c| c == "keel.journey.none"),
        "no flow and no contract is said, not implied: {reply}"
    );
    // The plan says it; `keel check` does not repeat it on every diff of such a project.
    let (_, reply) = run(bare.path(), None);
    assert!(
        !codes(&reply).iter().any(|c| c == "keel.journey.none"),
        "{reply}"
    );
}

/// #425: a flow with a second path compiles into a branch contract (`checkout.back`), and `journey
/// replay` takes the flow id, not that contract id. The plan names flows, so every replay step it
/// prints can run, and a card naming the flow covers its branch contracts. Credible regression: the
/// plan goes back to contract ids and prints `graphhelm journey replay checkout.back`, which replay
/// refuses with `replay.flow_invalid`. Cost: one temp git repository, one approve, two CLI runs.
#[test]
fn the_plan_names_replayable_flows_and_a_card_naming_the_flow_covers_its_branches() {
    let flow = include_str!("fixtures/journey_flow/checkout.journey.yaml").replace(
        "  main: [cart.checkout, pay.submit]\n",
        "  back: [cart.checkout]\n  main: [cart.checkout, pay.submit]\n",
    );
    let repo = tempfile::tempdir().unwrap();
    let root = repo.path();
    git(root, &["init", "-q"]);
    for path in [
        "app/cart/page.tsx",
        "app/checkout/page.tsx",
        "app/api/pay/route.ts",
    ] {
        fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
        fs::write(root.join(path), "export {}\n").unwrap();
    }
    let journeys = root.join(".graphhelm/journeys");
    fs::create_dir_all(&journeys).unwrap();
    fs::write(journeys.join("checkout.journey.yaml"), flow).unwrap();
    let canonical = Command::cargo_bin("graphhelm")
        .unwrap()
        .current_dir(root)
        .args(["--json", "journey", "compile", "--fmt", "checkout"])
        .output()
        .unwrap();
    assert!(
        canonical.status.success(),
        "{}",
        String::from_utf8_lossy(&canonical.stdout)
    );
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "flow"]);
    let approve = Command::cargo_bin("graphhelm")
        .unwrap()
        .current_dir(root)
        .args([
            "--json",
            "journey",
            "approve",
            "checkout",
            "--token-file",
            &owner_token(root),
        ])
        .output()
        .unwrap();
    assert!(
        approve.status.success(),
        "{}",
        String::from_utf8_lossy(&approve.stdout)
    );
    assert!(
        journeys.join("checkout.back.json").is_file(),
        "the branch contract exists"
    );
    // What a clone holds: the flow, never its git-ignored generated contracts.
    fs::remove_file(journeys.join("checkout.json")).unwrap();
    fs::remove_file(journeys.join("checkout.back.json")).unwrap();
    git(
        root,
        &[
            "add",
            ".graphhelm/journeys/checkout.journey.yaml",
            ".gitignore",
        ],
    );
    git(root, &["commit", "-q", "-m", "approve"]);
    fs::write(root.join("app/cart/page.tsx"), "export const x = 1\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "change"]);

    let (code, reply, _) = plan(root, &["--task", "t-1", "--paths", "app/cart/page.tsx"]);
    assert_eq!(code, 0, "{reply}");
    let record = &reply["data"]["plan"];
    assert_eq!(
        record["journeys"],
        serde_json::json!(["checkout"]),
        "{record}"
    );
    let replays: Vec<&str> = record["path"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .filter(|step| step.starts_with("graphhelm journey replay"))
        .collect();
    assert_eq!(
        replays,
        vec!["graphhelm journey replay checkout"],
        "{record}"
    );

    let card = root.join("card.json");
    fs::write(
        &card,
        serde_json::to_vec(&serde_json::json!({
            "promise": "the cart shows x", "scopePaths": ["app"], "proof": "journey replay",
            "journeys": ["checkout"],
        }))
        .unwrap(),
    )
    .unwrap();
    let (code, reply) = run(root, Some(&card));
    assert_eq!(code, 0, "{reply}");
    assert!(
        !codes(&reply)
            .iter()
            .any(|c| c == "keel.journey.card_missing_journey"
                || c == "keel.journey.contract_unreadable"),
        "naming the flow covers checkout.back: {reply}"
    );
}

/// #467: the planner decides whether a task's design is graded by a critic, from `keel.yaml`
/// `plan.critic` by class. An invariant path (`core/events/src/journal.rs`, persistence) gets
/// `critic: design` with the policy's bounds and a design + critic step in its route; a plain
/// code path gets `none` and no such step. Credible regressions: the critic missing from the
/// record, or a code-only task routed through a critic it does not need. Cost: one temp
/// repository, two CLI runs.
#[test]
fn keel_plan_decides_the_design_critic_by_class() {
    let repo = repository(&[("core/events/src/journal.rs", "pub fn x() {}\n")]);
    let (code, invariant, _) = plan(
        repo.path(),
        &[
            "--task",
            "issue-467",
            "--paths",
            "core/events/src/journal.rs",
        ],
    );
    assert_eq!(code, 0, "{invariant}");
    let record = &invariant["data"]["plan"];
    assert_eq!(
        record["critic"],
        serde_json::json!({"mode": "design", "passScore": 8, "maxRounds": 3}),
        "{record}"
    );
    let route: Vec<&str> = record["path"]
        .as_array()
        .unwrap()
        .iter()
        .map(|step| step.as_str().unwrap())
        .collect();
    assert_eq!(route[..2], ["card", "design"], "{record}");
    assert!(
        route[2].starts_with("critic: blind, pass at 8/10 within 3 rounds"),
        "{record}"
    );
    let (_, plain, _) = plan(
        repo.path(),
        &["--task", "issue-467", "--paths", "src/lib.rs"],
    );
    let record = &plain["data"]["plan"];
    assert_eq!(record["critic"]["mode"], "none", "{record}");
    assert!(
        !record["path"]
            .as_array()
            .unwrap()
            .iter()
            .any(|step| step == "design"),
        "{record}"
    );
}
