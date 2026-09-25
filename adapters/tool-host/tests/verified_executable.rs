//! #540 (D-042's "verified executable" clause): the broker must know WHICH binary it is about
//! to run, and refuse if it cannot say.
//!
//! Two decisions carry the property. The path must be ABSOLUTE -- program resolution
//! (PATH/PATHEXT, CreateProcess search order) is exactly how the same name runs two different
//! binaries, so this mechanism refuses to reason about names at all. And the digest is compared
//! BEFORE anything spawns: a mismatch refuses with both values, and the refusal path never
//! constructs a process.
//!
//! The verify->spawn window is DECLARED, not hidden: the digest proves which bytes were at the
//! path at verification time, and the spawn uses that same absolute path. Closing the window
//! entirely needs an OS handle-based exec this crate does not have; D-042's sentence ("verified
//! before the broker runs it") is met at its letter and the residue is named in the module docs.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use graphhelm_tool_host::process::{HostError, ProcessLimits, run_verified_in_workspace};
use graphhelm_tool_host::verified::{VerifiedExecutable, verify_executable};

fn fake_tool() -> String {
    env!("CARGO_BIN_EXE_fake_tool").to_owned()
}

fn limits() -> ProcessLimits {
    ProcessLimits {
        timeout: Duration::from_secs(10),
        max_output_bytes: 1024 * 1024,
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    graphhelm_tool_broker::record::digest_hex(bytes)
}

/// POSITIVE CONTROL, first: the real test binary, pinned by its real digest, verifies -- and
/// the identity carries both the absolute path and the digest, which is what the record needs.
#[test]
fn a_correctly_pinned_absolute_path_verifies_and_carries_its_identity() {
    let path = fake_tool();
    let expected = sha256_hex(&std::fs::read(&path).expect("the fake tool exists"));

    let verified = verify_executable(Path::new(&path), &expected)
        .expect("the pinned digest matches the bytes at the path");

    assert_eq!(verified.sha256(), expected);
    assert!(verified.path().is_absolute());
}

/// A digest that does not match refuses with BOTH values -- and never falls back to running.
#[test]
fn a_mismatched_digest_refuses_naming_expected_and_actual() {
    let path = fake_tool();
    let wrong = sha256_hex(b"not the binary");

    let refused = verify_executable(Path::new(&path), &wrong)
        .expect_err("a binary nobody identified was allowed to run");

    match refused {
        HostError::ExecutableMismatch { expected, actual } => {
            assert_eq!(expected, wrong);
            assert_eq!(
                actual,
                sha256_hex(&std::fs::read(&path).unwrap()),
                "the refusal carries what WAS at the path, so the operator can decide whether \
                 to re-pin or investigate"
            );
        }
        other => panic!("verification did not decide this: {other:?}"),
    }
}

/// A relative name is refused BEFORE any digest is read: resolving it would re-implement the
/// OS's search order, and being wrong there is silent -- the same name can be two binaries.
#[test]
fn a_relative_program_name_is_refused_without_reading_anything() {
    let refused = verify_executable(Path::new("fake_tool.exe"), &sha256_hex(b"x"))
        .expect_err("a name the OS would resolve is not an identity");

    match refused {
        HostError::ExecutableNotPinned { rule } => {
            assert_eq!(rule, "program path must be absolute");
        }
        other => panic!("the path rule did not decide this: {other:?}"),
    }
}

/// The verified spawn: the child that runs IS the pinned binary, end to end through the
/// workspace primitive, and the identity travels back to the caller for the record.
#[test]
fn a_verified_spawn_runs_the_pinned_binary_in_the_workspace() {
    let path = fake_tool();
    let expected = sha256_hex(&std::fs::read(&path).unwrap());
    let verified: VerifiedExecutable =
        verify_executable(Path::new(&path), &expected).expect("pinned");

    let workspace = tempfile::tempdir().unwrap();
    let captured = run_verified_in_workspace(
        workspace.path(),
        &verified,
        &["env-dump".to_owned()],
        &BTreeMap::new(),
        &[],
        None,
        &limits(),
        None,
    )
    .expect("a verified binary runs");

    assert_eq!(captured.exit_code, Some(0));
    assert!(
        String::from_utf8_lossy(&captured.stdout).contains("PATH="),
        "the child genuinely ran and dumped its environment"
    );
}

/// The swap sabotage from the acceptance, as a cell: bytes changed after the pin was recorded
/// refuse at verification -- the pin is about the BYTES, not the name that survived the swap.
#[test]
fn a_swapped_binary_reddens_at_verification_by_name() {
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("provider.exe");
    std::fs::copy(fake_tool(), &target).unwrap();
    let pinned = sha256_hex(&std::fs::read(&target).unwrap());

    // The swap: same name, different bytes.
    std::fs::write(&target, b"a different program entirely").unwrap();

    let refused = verify_executable(&target, &pinned)
        .expect_err("the swapped binary passed the pin recorded for the original");

    assert!(
        matches!(refused, HostError::ExecutableMismatch { .. }),
        "got: {refused:?}"
    );
}

/// The identity reaches the durable record and survives the wire -- and a record written BEFORE
/// the field existed still decodes, as None: absent is explicitly unknown, never invented.
#[test]
fn the_identity_round_trips_and_old_records_decode_as_none() {
    use graphhelm_tool_broker::record::{
        ToolCallRecord, ToolDisposition, VerifiedExecutableIdentity, digest_hex,
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
        verified_executable: Some(VerifiedExecutableIdentity {
            path: "C:/tier1/provider.exe".to_owned(),
            sha256: digest_hex(b"the provider bytes"),
        }),
        contained_session: None,
        commit: None,
        landed_ref: None,
        recovered_workspace: false,
    };
    let wire = serde_json::to_string(&record).expect("encodes");
    let back: ToolCallRecord = serde_json::from_str(&wire).expect("decodes");
    assert_eq!(back, record, "the identity survives the wire");

    // A record from before the field: decodes, with the identity explicitly unknown.
    let old = serde_json::json!({
        "tool": "read", "action": "file", "actor": "runtime",
        "tier": "tier_0",
        "disposition": {"kind": "completed", "exit_code": 0},
        "stdoutSha256": digest_hex(b""), "stdoutBytes": 0,
        "stderrSha256": digest_hex(b""), "stderrBytes": 0,
        "truncated": false, "reused": false
    });
    let decoded: ToolCallRecord = serde_json::from_value(old).expect("an old record still decodes");
    assert_eq!(decoded.verified_executable, None);
}
