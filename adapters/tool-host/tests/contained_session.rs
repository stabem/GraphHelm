//! #552 (D-042's PRIMARY clause): a broker-owned session to an external provider process,
//! constructed so that an uncontained session is UNREPRESENTABLE.
//!
//! The three qualifiers already exist as unbypassable doorways (#538 where it writes, #540
//! which binary, #539 what it reads over). This session is their composition one floor up: its
//! constructor REQUIRES a `VerifiedExecutable`, a `PinnedSnapshot`, and the workspace root --
//! there is no other way to build one, so "checked-then-spawned" cannot regress into
//! "forgot-to-check" anywhere above it.
//!
//! Every call re-verifies the pin AT THE INSTANT OF USE (an address that moves re-verifies when
//! read), refuses BEFORE the spawn with the #544 ordering proof (observable side effect: the
//! sandbox dirs the spawn would create stay uncreated), and runs through the ONE Tier 1 spawn
//! funnel -- no second spawn path.

use std::path::{Path, PathBuf};
use std::process::Command;

use graphhelm_tool_host::process::{HostError, ProcessLimits};
use graphhelm_tool_host::session::ContainedProviderSession;
use graphhelm_tool_host::snapshot::pin_snapshot;
use graphhelm_tool_host::verified::verify_executable;
use graphhelm_tool_host::workspace::{Tier1Workspace, WorkspaceConfig};

// Duplicated from broker_end_to_end.rs on purpose: integration-test binaries do not share
// helpers, and a tiny duplicated fixture beats a shared module that couples the suites.
fn scratch_repo() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::write(
        project.join("src/lib.rs"),
        "// scratch
",
    )
    .unwrap();
    let git = |args: &[&str]| {
        let status = Command::new("git")
            .args(args)
            .current_dir(&project)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "scratch")
            .env("GIT_AUTHOR_EMAIL", "scratch@test.invalid")
            .env("GIT_COMMITTER_NAME", "scratch")
            .env("GIT_COMMITTER_EMAIL", "scratch@test.invalid")
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    };
    git(&["init", "--quiet"]);
    git(&["add", "-A"]);
    git(&["commit", "--quiet", "-m", "scratch"]);
    (dir, project)
}

/// A REAL provisioned Tier 1 workspace: the doorway type the session now requires -- a raw
/// tempdir can no longer stand in (L's #553 fold).
fn provisioned(call_id: &str) -> (tempfile::TempDir, tempfile::TempDir, Tier1Workspace) {
    let (project_dir, project) = scratch_repo();
    let staging = tempfile::tempdir().unwrap();
    let config = WorkspaceConfig::validated(&project, staging.path(), &[]).unwrap();
    let workspace = Tier1Workspace::provision(&config, call_id, None).unwrap();
    (project_dir, staging, workspace)
}

fn fake_tool() -> String {
    env!("CARGO_BIN_EXE_fake_tool").to_owned()
}

fn sha256_hex(bytes: &[u8]) -> String {
    graphhelm_tool_broker::record::digest_hex(bytes)
}

fn limits() -> ProcessLimits {
    ProcessLimits {
        timeout: std::time::Duration::from_secs(10),
        max_output_bytes: 1024 * 1024,
    }
}

fn source_index(root: &Path) {
    std::fs::create_dir_all(root.join("graph")).unwrap();
    std::fs::write(root.join("graph/nodes.bin"), b"node bytes v1").unwrap();
}

/// Everything a contained session needs, built the only way each piece can be built.
fn contained(
    workspace: &Path,
) -> (
    graphhelm_tool_host::verified::VerifiedExecutable,
    graphhelm_tool_host::snapshot::PinnedSnapshot,
) {
    let exe = fake_tool();
    let verified =
        verify_executable(Path::new(&exe), &sha256_hex(&std::fs::read(&exe).unwrap())).unwrap();
    let source = workspace.join("source-index");
    source_index(&source);
    let pinned = pin_snapshot(&source, workspace).unwrap();
    (verified, pinned)
}

/// POSITIVE CONTROL, first: a session built from the three contained pieces opens, and its
/// identity names the program, the snapshot generation, and a derived session id -- the triple
/// a receipt needs to bind "a named program in a named session".
#[test]
fn a_contained_session_opens_and_names_its_identity() {
    let (_project, _staging, workspace) = provisioned("session-a");
    let (verified, pinned) = contained(workspace.root());

    let session = ContainedProviderSession::open(&workspace, verified.clone(), pinned.clone());

    let identity = session.identity();
    assert_eq!(identity.executable_sha256, verified.sha256());
    assert_eq!(identity.snapshot_generation, pinned.generation());
    assert!(
        !identity.session_id.is_empty(),
        "the session id names THIS composition"
    );
    // Derived, deterministic: the same composition is the same session identity, so a receipt
    // can be re-derived by an auditor who trusts none of the runner's state.
    let again = ContainedProviderSession::open(&workspace, verified, pinned);
    assert_eq!(identity.session_id, again.identity().session_id);
}

/// Different compositions, different session ids -- the id is ABOUT the composition.
#[test]
fn a_different_snapshot_yields_a_different_session_id() {
    let (_pa, _sa, workspace_a) = provisioned("session-b1");
    let (verified_a, pinned_a) = contained(workspace_a.root());
    let (_pb, _sb, workspace_b) = provisioned("session-b2");
    let source_b = workspace_b.root().join("source-index");
    std::fs::create_dir_all(source_b.join("graph")).unwrap();
    std::fs::write(source_b.join("graph/nodes.bin"), b"node bytes v2").unwrap();
    let pinned_b = pin_snapshot(&source_b, workspace_b.root()).unwrap();

    let session_a = ContainedProviderSession::open(&workspace_a, verified_a.clone(), pinned_a);
    let session_b = ContainedProviderSession::open(&workspace_b, verified_a, pinned_b);

    assert_ne!(
        session_a.identity().session_id,
        session_b.identity().session_id
    );
}

/// The call path: the provider runs through the ONE spawn funnel, and the address it reads its
/// store from — `CBM_CACHE_DIR`, the only name the real provider consults (measured against
/// codebase-memory-mcp 0.10.8) — holds the PINNED bytes when the child starts. The session
/// copies the verified pin there before every spawn; the pin itself is never handed over,
/// because the provider writes into its cache dir and a written-into pin would fail its own
/// next re-verification. Observable from inside the child (the env dump names the address) and
/// from the bytes at that address.
#[test]
fn a_call_serves_the_pinned_bytes_at_the_confined_cache_dir() {
    let (_project, _staging, workspace) = provisioned("session-c");
    let (verified, pinned) = contained(workspace.root());
    let session = ContainedProviderSession::open(&workspace, verified, pinned);

    let captured = session
        .call(&["env-dump".to_owned()], None, &limits(), None)
        .expect("a contained call runs");

    let dump = String::from_utf8_lossy(&captured.stdout);
    let cache_line = dump
        .lines()
        .find(|line| line.starts_with("CBM_CACHE_DIR="))
        .expect("#538's confinement composes: the cache dir arrives via the same funnel");
    let cache_dir = Path::new(cache_line.trim_start_matches("CBM_CACHE_DIR="));
    assert!(
        cache_dir
            .canonicalize()
            .unwrap()
            .starts_with(workspace.root().canonicalize().unwrap()),
        "the cache dir the provider reads stays inside the workspace"
    );
    assert_eq!(
        std::fs::read(cache_dir.join("graph/nodes.bin")).expect("the serving copy exists"),
        b"node bytes v1",
        "the address the provider reads serves the PINNED snapshot's bytes"
    );
}

/// The pin is re-verified AT THE INSTANT OF USE: a snapshot tampered after the session opened
/// refuses the CALL, before any spawn -- proven by the #544 observable, not by the name. The
/// workspace is fresh, so a pre-spawn refusal leaves the funnel's sandbox dirs uncreated.
#[test]
fn a_tampered_snapshot_refuses_the_call_before_any_spawn() {
    let (_project, _staging, workspace) = provisioned("session-d");
    let (verified, pinned) = contained(workspace.root());
    let tamper_target = pinned.root().join("graph/nodes.bin");
    let session = ContainedProviderSession::open(&workspace, verified, pinned);

    std::fs::write(&tamper_target, b"moved under the session").unwrap();

    let refused = session
        .call(&["env-dump".to_owned()], None, &limits(), None)
        .expect_err("a call read a snapshot that no longer matches its pin");

    assert!(
        matches!(refused, HostError::SnapshotMismatch { .. }),
        "got: {refused:?}"
    );
    // The #544 ordering proof: the spawn funnel creates .home/.tmp before any child starts, so
    // their ABSENCE is the observable that the refusal preceded the spawn.
    assert!(
        !workspace.root().join(".home").exists(),
        "a pre-spawn refusal cannot have created the funnel's sandbox dirs"
    );
}

/// The identity reaches the durable record beside the executable's, and old records decode
/// with it explicitly unknown -- the #540 precedent, one field over.
#[test]
fn the_session_identity_round_trips_in_the_record_and_old_records_decode_as_none() {
    use graphhelm_tool_broker::record::{
        ContainedSessionIdentity, ToolCallRecord, ToolDisposition, digest_hex,
    };

    let record = ToolCallRecord {
        tool: "codebase-memory".to_owned(),
        action: "search".to_owned(),
        actor: "runtime".to_owned(),
        program_allowlist: std::collections::BTreeSet::new(),
        tier: graphhelm_tool_broker::effect::IsolationTier::Tier1,
        disposition: ToolDisposition::Completed { exit_code: 0 },
        stdout_sha256: digest_hex(b""),
        stdout_bytes: 0,
        stderr_sha256: digest_hex(b""),
        stderr_bytes: 0,
        truncated: false,
        reused: false,
        verified_executable: None,
        contained_session: Some(ContainedSessionIdentity {
            session_id: "s-abc".to_owned(),
            snapshot_generation: "sha256-xyz".to_owned(),
            executable_sha256: "deadbeef".to_owned(),
        }),
        commit: None,
        landed_ref: None,
        recovered_workspace: false,
    };
    let wire = serde_json::to_string(&record).unwrap();
    let back: ToolCallRecord = serde_json::from_str(&wire).unwrap();
    assert_eq!(back, record);

    let old = serde_json::json!({
        "tool": "read", "action": "file", "actor": "runtime",
        "tier": "tier_0",
        "disposition": {"kind": "completed", "exit_code": 0},
        "stdoutSha256": digest_hex(b""), "stdoutBytes": 0,
        "stderrSha256": digest_hex(b""), "stderrBytes": 0,
        "truncated": false, "reused": false
    });
    let decoded: ToolCallRecord = serde_json::from_value(old).unwrap();
    assert_eq!(decoded.contained_session, None);
}

/// A capture the readers could not read must not reach a consumer as a RESULT.
///
/// `ToolHost::invoke` has a disposition vocabulary and says `GHTOOL013_CAPTURE_LOST` with it. The
/// session has none: it hands a `CapturedProcess` straight to whoever called, and the two
/// consumers that reach the funnel this way -- `codebase-memory-mcp`'s provider and the
/// development benchmark's generator -- read `exit_code` and hash the bytes. An empty `stderr`
/// that was never read hashes to the digest of zero bytes and is then indistinguishable from a
/// tool that printed nothing (Codex, on #703). So this seam REFUSES rather than reports.
#[test]
fn a_lost_capture_is_refused_rather_than_returned() {
    let lost = graphhelm_tool_host::process::CapturedProcess {
        exit_code: Some(0),
        stdout: b"a valid response the provider would have trusted".to_vec(),
        stderr: Vec::new(),
        stdout_truncated: false,
        stderr_truncated: false,
        truncated: false,
        timed_out: false,
        readers_abandoned: true,
        reader_lost: false,
        tree_kill: None,
        cancelled: false,
    };

    // The dangerous shape precisely: a SUCCESSFUL exit code and a plausible stdout. Nothing in
    // the value itself looks wrong, which is why the flag has to be the thing that decides.
    let refused = graphhelm_tool_host::process::reject_lost_capture(lost);

    match refused {
        Err(HostError::CaptureLost { .. }) => {}
        Err(other) => panic!("refused for the wrong reason: {other}"),
        Ok(captured) => panic!(
            "a lost capture was returned as a result: exit_code={:?}, stderr={} bytes",
            captured.exit_code,
            captured.stderr.len()
        ),
    }
}

/// The other direction, so the guard is not simply "refuse everything": an ordinary capture
/// passes through UNCHANGED, bytes and all. Without this cell, a `reject_lost_capture` that
/// returned `Err` on every input would satisfy the test above.
#[test]
fn an_ordinary_capture_passes_through_untouched() {
    let ordinary = graphhelm_tool_host::process::CapturedProcess {
        exit_code: Some(3),
        stdout: b"stdout bytes".to_vec(),
        stderr: b"stderr bytes".to_vec(),
        stdout_truncated: false,
        stderr_truncated: false,
        truncated: false,
        timed_out: false,
        readers_abandoned: false,
        reader_lost: false,
        tree_kill: None,
        cancelled: false,
    };

    let passed = graphhelm_tool_host::process::reject_lost_capture(ordinary)
        .expect("a capture that was read is not refused");

    // A non-zero exit code is a RESULT, not a loss: the guard must not confuse "the tool failed"
    // with "we could not read what the tool said".
    assert_eq!(passed.exit_code, Some(3));
    assert_eq!(passed.stdout, b"stdout bytes");
    assert_eq!(passed.stderr, b"stderr bytes");
}
