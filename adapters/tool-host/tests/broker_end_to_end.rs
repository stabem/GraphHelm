//! The composed broker, end to end: authorize (pure) → route by tier → execute → digest →
//! remove → record. Tier 0 never provisions; Tier 1 is born and dies inside one call; every
//! refusal, timeout and host error still produces a record — 05d externalizes what happened
//! without a side channel.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use graphhelm_tool_broker::call::{RepositoryAction, ShellAction, TestsAction, ToolCall};
use graphhelm_tool_broker::effect::IsolationTier;
use graphhelm_tool_broker::lease::{Capability, MAX_PROGRAM_ALLOWLIST_MEMBERS, ToolLease};
use graphhelm_tool_broker::path::RelativePath;
use graphhelm_tool_broker::record::{ToolDisposition, digest_hex};
use graphhelm_tool_host::host::{HostConfig, ToolHost};
use graphhelm_tool_host::process::ProcessLimits;
use graphhelm_tool_host::workspace::WorkspaceConfig;

// Duplicated from workspace_containment.rs on purpose: integration-test binaries do not share
// helpers, and a tiny duplicated fixture beats a shared module that couples the suites.
fn scratch_repo() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::write(project.join("src/lib.rs"), "// scratch\n").unwrap();
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

fn host(project: &Path, staging: &Path) -> ToolHost {
    host_with(project, staging, Duration::from_secs(30), "fake_tool")
}

fn host_with(project: &Path, staging: &Path, timeout: Duration, tests_runner: &str) -> ToolHost {
    // tests_runner stays a bare name ("fake_tool"); its directory reaches the child through
    // HostConfig::path_prepend (the Task 5 mechanism) — no parent-PATH mutation, no bare-name
    // exception.
    let tool_dir = Path::new(env!("CARGO_BIN_EXE_fake_tool"))
        .parent()
        .unwrap()
        .to_path_buf();
    ToolHost::new(HostConfig {
        workspace: WorkspaceConfig::validated(project, staging, &[]).unwrap(),
        limits: ProcessLimits {
            timeout,
            max_output_bytes: 1024 * 1024,
        },
        tests_runner: tests_runner.to_owned(),
        tests_runner_env: BTreeMap::new(),
        path_prepend: vec![tool_dir],
        keep_workspace: false,
    })
}

fn full_lease(actor: &str) -> ToolLease {
    ToolLease {
        actor: actor.to_owned(),
        capabilities: [
            Capability::RepositoryRead,
            Capability::RepositoryWrite,
            Capability::ShellExecute,
            Capability::TestsExecute,
        ]
        .into_iter()
        .collect(),
        programs: ["git", "cargo"].map(str::to_owned).into_iter().collect(),
    }
}

fn staging_entries(staging: &Path) -> Vec<String> {
    match std::fs::read_dir(staging) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            // Task 9b: the read cache lives under staging BY the plan's own text ("under the
            // host's staging area") and persists across calls by design — it is host data,
            // not a leaked workspace. The empty-staging contract these tests pin is about
            // WORKSPACES (write capabilities) not outliving their call.
            .filter(|name| name != "ghtool-read-cache")
            .collect(),
        Err(_) => Vec::new(),
    }
}

const PATCH: &str = "\
diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1 +1,2 @@
 // scratch
+// patched
";

#[test]
fn a_tier_0_read_touches_the_project_and_never_provisions_a_workspace() {
    let (_dir, project) = scratch_repo();
    let staging = tempfile::tempdir().unwrap();
    let host = host(&project, staging.path());
    let call = ToolCall::Repository(RepositoryAction::ReadFile {
        path: RelativePath::parse("src/lib.rs").unwrap(),
    });
    let (record, streams) = host.invoke(&call, &full_lease("agent-reader"), "agent-reader");
    assert_eq!(record.tier, IsolationTier::Tier0);
    assert!(matches!(
        record.disposition,
        ToolDisposition::Completed { exit_code: 0 }
    ));
    let file_bytes = std::fs::read(project.join("src/lib.rs")).unwrap();
    assert_eq!(record.stdout_sha256, digest_hex(&file_bytes));
    assert_eq!(streams.stdout, file_bytes);
    // Tier 0 is workspace-free by construction, not by cleanup: nothing was EVER created.
    assert!(
        staging_entries(staging.path()).is_empty(),
        "tier 0 must not provision"
    );
}

#[test]
fn the_program_allowlist_in_force_is_recorded_canonically() {
    let (_dir, project) = scratch_repo();
    let staging = tempfile::tempdir().unwrap();
    let host = host(&project, staging.path());
    let mut lease = full_lease("agent-auditor");
    lease.programs = ["rustc", "cargo", "git"]
        .map(str::to_owned)
        .into_iter()
        .collect();
    let call = ToolCall::Repository(RepositoryAction::ReadFile {
        path: RelativePath::parse("src/lib.rs").unwrap(),
    });

    let (record, _) = host.invoke(&call, &lease, "agent-auditor");

    assert_eq!(
        record.program_allowlist.into_iter().collect::<Vec<_>>(),
        ["cargo", "git", "rustc"],
        "the durable record must name the complete canonical authority set"
    );

    lease.programs = ["python", "git"].map(str::to_owned).into_iter().collect();
    let (reused, _) = host.invoke(&call, &lease, "agent-auditor");
    assert!(
        reused.reused,
        "the second snapshot-closed read must hit cache"
    );
    assert_eq!(
        reused.program_allowlist.into_iter().collect::<Vec<_>>(),
        ["git", "python"],
        "a cache hit must record current authority, not the cache writer's authority"
    );
}

#[test]
fn an_invalid_allowlist_member_is_refused_without_entering_the_record() {
    let (_dir, project) = scratch_repo();
    let staging = tempfile::tempdir().unwrap();
    let host = host(&project, staging.path());
    let mut lease = full_lease("agent-auditor");
    let sentinel = "TOKEN=secret";
    lease.programs.insert(sentinel.to_owned());
    let call = ToolCall::Repository(RepositoryAction::ReadFile {
        path: RelativePath::parse("src/lib.rs").unwrap(),
    });

    let (record, _) = host.invoke(&call, &lease, "agent-auditor");

    assert!(matches!(
        record.disposition,
        ToolDisposition::Denied { ref rule } if rule == "program_allowlist_invalid"
    ));
    assert!(record.program_allowlist.is_empty());
    assert!(!serde_json::to_string(&record).unwrap().contains(sentinel));
    assert!(staging_entries(staging.path()).is_empty());
}

#[test]
fn an_oversized_allowlist_is_refused_before_a_write_effect() {
    let (_dir, project) = scratch_repo();
    let staging = tempfile::tempdir().unwrap();
    let host = host(&project, staging.path());
    let before = std::fs::read(project.join("src/lib.rs")).unwrap();
    let mut lease = full_lease("agent-writer");
    lease.programs = (0..=MAX_PROGRAM_ALLOWLIST_MEMBERS)
        .map(|index| format!("p{index}"))
        .collect();
    let call = ToolCall::Repository(RepositoryAction::ApplyPatch {
        patch: PATCH.to_owned(),
    });

    let (record, streams) = host.invoke(&call, &lease, "agent-writer");

    assert!(matches!(
        record.disposition,
        ToolDisposition::Denied { ref rule } if rule == "program_allowlist_invalid"
    ));
    assert!(record.program_allowlist.is_empty());
    assert!(streams.stdout.is_empty());
    assert!(streams.stderr.is_empty());
    assert_eq!(std::fs::read(project.join("src/lib.rs")).unwrap(), before);
    assert!(staging_entries(staging.path()).is_empty());
}

#[test]
fn a_timeout_record_keeps_the_complete_program_allowlist() {
    let (_dir, project) = scratch_repo();
    let staging = tempfile::tempdir().unwrap();
    let host = host_with(
        &project,
        staging.path(),
        Duration::from_millis(100),
        "fake_tool",
    );
    let mut lease = full_lease("agent-sleeper");
    lease.programs.insert("fake_tool".to_owned());
    let call = ToolCall::Shell(ShellAction {
        program: "fake_tool".to_owned(),
        arguments: vec!["sleep".to_owned()],
    });

    let (record, _) = host.invoke(&call, &lease, "agent-sleeper");

    assert_eq!(record.disposition, ToolDisposition::TimedOut);
    assert_eq!(record.program_allowlist, lease.programs);
}

/// #609, Codex: the durable record must NAME the cause, not merely withhold the wrong one.
///
/// `timed_out = expired && !cancelled` keeps the clock from being blamed. It does not put the
/// cancellation anywhere, and the disposition then falls through to the exit code — which a killed
/// child HAS. On Windows that made a cancelled call read as `Completed { exit_code: 1 }`, a tool
/// that ran and failed, which `ToolFailureSemantics::RetryEligible` maps to `RetryableFailure`. The
/// record invited a retry of something an operator had deliberately stopped.
///
/// The arrangement control is the load-bearing half here, because the two routes to
/// `GHTOOL011_CANCELLED` are indistinguishable in the record: a spawn REFUSED under a raised signal
/// carries the same code. The child is therefore observed writing OUTSIDE the process before the
/// cancel, so this cell measures a killed child rather than a refused spawn.
#[test]
fn a_cancelled_invocation_names_the_cancellation_instead_of_a_tool_failure() {
    let (_dir, project) = scratch_repo();
    let staging = tempfile::tempdir().unwrap();
    let evidence = tempfile::tempdir().unwrap();
    let trace = evidence.path().join("trace.txt");
    // A generous deadline on purpose: the clock must not be what stops this child.
    let host = host_with(
        &project,
        staging.path(),
        Duration::from_secs(60),
        "fake_tool",
    );
    // `full_lease` allows git and cargo only; without this the call is DENIED before any spawn,
    // which the arrangement control below caught by naming the disposition.
    let mut lease = full_lease("agent-sleeper");
    lease.programs.insert("fake_tool".to_owned());
    let call = ToolCall::Shell(ShellAction {
        program: "fake_tool".to_owned(),
        arguments: vec!["append-forever".to_owned(), trace.display().to_string()],
    });

    let record = std::thread::scope(|scope| {
        let running = scope.spawn(|| host.invoke(&call, &lease, "agent-sleeper"));

        // Tier 1 provisions a git worktree before it spawns, and this machine runs many
        // toolchains at once, so the window is generous. It fails toward RED either way: a
        // window that expires reports that nothing was measured rather than certifying a kill.
        let mut grew = false;
        for _ in 0..400 {
            if std::fs::metadata(&trace).map(|m| m.len()).unwrap_or(0) > 0 {
                grew = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        host.cancel_all();
        let (record, streams) = running.join().expect("the invoking thread returns");
        assert!(
            grew,
            "arrangement: no child was observed writing, so the cancel refused a spawn rather \
             than killing a child -- and a refusal carries this same code by the other route, \
             which would make the assertion below vacuous. disposition: {:?}; stderr: {}",
            record.disposition,
            String::from_utf8_lossy(&streams.stderr)
        );
        record
    });

    assert_eq!(
        record.disposition,
        ToolDisposition::HostError {
            code: "GHTOOL011_CANCELLED".to_owned()
        },
        "a cancelled call must name the cancellation; recording the kill's exit code says the tool \
         ran and failed, which is a different false cause from the one this change removed"
    );
}

#[test]
fn a_host_error_record_keeps_the_complete_program_allowlist() {
    let (_dir, project) = scratch_repo();
    let staging = tempfile::tempdir().unwrap();
    let host = host_with(
        &project,
        staging.path(),
        Duration::from_secs(30),
        "INVALID/RUNNER",
    );
    let lease = full_lease("agent-tester");
    let call = ToolCall::Tests(TestsAction {
        arguments: Vec::new(),
    });

    let (record, _) = host.invoke(&call, &lease, "agent-tester");

    assert!(matches!(
        record.disposition,
        ToolDisposition::HostError { .. }
    ));
    assert_eq!(record.program_allowlist, lease.programs);
}

#[test]
fn a_write_call_runs_in_an_ephemeral_worktree_and_the_project_is_untouched() {
    let (_dir, project) = scratch_repo();
    let staging = tempfile::tempdir().unwrap();
    let host = host(&project, staging.path());
    let before = std::fs::read(project.join("src/lib.rs")).unwrap();
    let call = ToolCall::Repository(RepositoryAction::ApplyPatch {
        patch: PATCH.to_owned(),
    });
    let (record, streams) = host.invoke(&call, &full_lease("agent-writer"), "agent-writer");
    assert!(
        matches!(
            record.disposition,
            ToolDisposition::Completed { exit_code: 0 }
        ),
        "apply must complete, got {:?}; stderr: {}",
        record.disposition,
        String::from_utf8_lossy(&streams.stderr)
    );
    assert_eq!(record.tier, IsolationTier::Tier1);
    // The write landed in the worktree, which was removed after capture; the project's file
    // is byte-identical to before.
    let after = std::fs::read(project.join("src/lib.rs")).unwrap();
    assert_eq!(before, after, "the project must be untouched");
    assert!(
        staging_entries(staging.path()).is_empty(),
        "staging must be empty again after the call"
    );
}

#[test]
fn commit_inside_the_workspace_never_moves_the_project_head() {
    let (_dir, project) = scratch_repo();
    let staging = tempfile::tempdir().unwrap();
    let host = host(&project, staging.path());
    let head_before = Command::new("git")
        .args(["-C", &project.display().to_string(), "rev-parse", "HEAD"])
        .output()
        .unwrap()
        .stdout;

    let apply = ToolCall::Repository(RepositoryAction::ApplyPatch {
        patch: PATCH.to_owned(),
    });
    let (apply_record, _) = host.invoke(&apply, &full_lease("agent-writer"), "agent-writer");
    assert!(matches!(
        apply_record.disposition,
        ToolDisposition::Completed { exit_code: 0 }
    ));
    let commit = ToolCall::Repository(RepositoryAction::Commit {
        message: "workspace commit that must die with the worktree".to_owned(),
    });
    let (commit_record, commit_streams) =
        host.invoke(&commit, &full_lease("agent-writer"), "agent-writer");
    assert!(
        matches!(
            commit_record.disposition,
            ToolDisposition::Completed { exit_code: 0 }
        ),
        "commit must complete, got {:?}; stderr: {}",
        commit_record.disposition,
        String::from_utf8_lossy(&commit_streams.stderr)
    );

    let head_after = Command::new("git")
        .args(["-C", &project.display().to_string(), "rev-parse", "HEAD"])
        .output()
        .unwrap()
        .stdout;
    assert_eq!(head_before, head_after, "the project HEAD must not move");
}

/// #247, through the host: a malformed program name records its OWN rule, so the operator reading
/// the disposition is told to fix the name rather than the lease.
#[test]
fn a_malformed_program_name_records_the_shape_rule_end_to_end() {
    let (_dir, project) = scratch_repo();
    let staging = tempfile::tempdir().unwrap();
    let host = host(&project, staging.path());
    let call = ToolCall::Shell(ShellAction {
        program: "bin/curl".to_owned(),
        arguments: vec!["https://example.com".to_owned()],
    });
    let (record, _streams) = host.invoke(&call, &full_lease("agent-sneaky"), "agent-sneaky");
    match &record.disposition {
        ToolDisposition::Denied { rule } => assert_eq!(rule, "program_name_invalid"),
        other => panic!("expected Denied, got {other:?}"),
    }
}

#[test]
fn the_shell_tool_respects_the_lease_allowlist_end_to_end() {
    let (_dir, project) = scratch_repo();
    let staging = tempfile::tempdir().unwrap();
    let host = host(&project, staging.path());
    let call = ToolCall::Shell(ShellAction {
        program: "curl".to_owned(),
        arguments: vec!["https://example.com".to_owned()],
    });
    let (record, _streams) = host.invoke(&call, &full_lease("agent-sneaky"), "agent-sneaky");
    match &record.disposition {
        ToolDisposition::Denied { rule } => assert_eq!(rule, "program_denied"),
        other => panic!("expected Denied, got {other:?}"),
    }
    assert_eq!(
        record.program_allowlist,
        ["cargo", "git"].map(str::to_owned).into_iter().collect(),
        "a refusal still needs its complete presented authorization context"
    );
    // authorize refuses before the host routes: NOTHING was provisioned.
    assert!(
        staging_entries(staging.path()).is_empty(),
        "a denial must not touch the filesystem"
    );
}

#[test]
fn the_tests_tool_reports_pass_and_fail_by_exit_code() {
    let (_dir, project) = scratch_repo();
    let staging = tempfile::tempdir().unwrap();
    let host = host(&project, staging.path());
    for (code, expected) in [(0_i32, 0_i32), (101, 101)] {
        let call = ToolCall::Tests(TestsAction {
            arguments: vec!["exit-code".to_owned(), code.to_string()],
        });
        let (record, _streams) = host.invoke(&call, &full_lease("agent-tester"), "agent-tester");
        match record.disposition {
            ToolDisposition::Completed { exit_code } => assert_eq!(exit_code, expected),
            other => panic!("expected Completed, got {other:?}"),
        }
    }
}

#[test]
fn tier_0_structurally_rejects_a_write_action() {
    // Defense in depth: hand the host a forged BrokerPlan (tier_0 + a write call) through the
    // public forged-plan entry the host exposes for exactly this test. Even if authorize were
    // bypassed entirely, the routing layer refuses to run a write-effect call outside Tier 1.
    let (_dir, project) = scratch_repo();
    let staging = tempfile::tempdir().unwrap();
    let host = host(&project, staging.path());
    let call = ToolCall::Repository(RepositoryAction::ApplyPatch {
        patch: PATCH.to_owned(),
    });
    let forged = graphhelm_tool_broker::lease::BrokerPlan {
        capability: Capability::RepositoryWrite,
        effect: call.effect(),
        tier: IsolationTier::Tier0,
    };
    let refused = host.execute_plan_for_test(&call, &forged, "agent-forger");
    assert!(
        matches!(
            refused.unwrap_err(),
            graphhelm_tool_host::process::HostError::TierViolation
        ),
        "a forged tier-0 write plan must be refused"
    );
}

#[test]
fn credentials_are_demonstrably_absent_from_the_tier_1_workspace() {
    // The register's hard constraint and the design's §8 criterion as one named test. The
    // sentinels must sit in the PARENT environment, so this reuses the Task 5 two-piece
    // wrapper: the OUTER run re-executes this binary filtered to this test with the
    // sentinels planted; the INNER run does the work.
    if std::env::var_os("GH_TOOL_HOST_INNER").is_none() {
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "credentials_are_demonstrably_absent_from_the_tier_1_workspace",
            ])
            .env("GH_TOOL_HOST_INNER", "1")
            .env("GRAPHHELM_EVENTS_KEY", "SENTINEL-events-passphrase")
            .env("GRAPHHELM_GATEWAY_KEY", "SENTINEL-gateway-passphrase")
            .env("FAKE_SECRET", "SENTINEL-ambient-token")
            .status()
            .unwrap();
        assert!(status.success(), "inner sentinel run failed");
        return;
    }

    // Arrange — a worst-case operator machine: passphrases in the env (above) and a keyring
    // directory whose FILE CONTENT is a third sentinel, registered as protected.
    let (_dir, project) = scratch_repo();
    let staging = tempfile::tempdir().unwrap();
    let keyring = tempfile::tempdir().unwrap();
    std::fs::write(
        keyring.path().join("keyring.bin"),
        b"SENTINEL-keyring-material",
    )
    .unwrap();

    let tool_dir = std::path::Path::new(env!("CARGO_BIN_EXE_fake_tool"))
        .parent()
        .unwrap()
        .to_path_buf();
    let config = HostConfig {
        workspace: WorkspaceConfig::validated(
            &project,
            staging.path(),
            &[keyring.path().to_path_buf()],
        )
        .unwrap(),
        limits: ProcessLimits {
            timeout: std::time::Duration::from_secs(30),
            max_output_bytes: 1024 * 1024,
        },
        tests_runner: "fake_tool".to_owned(),
        tests_runner_env: std::collections::BTreeMap::new(),
        path_prepend: vec![tool_dir],
        // Scan-while-alive: the workspaces survive capture so the filesystem assertion below
        // inspects the real trees, then this test deletes them itself.
        keep_workspace: true,
    };
    let host = ToolHost::new(config);
    let mut lease = full_lease("agent-prover");
    lease.programs.insert("fake_tool".to_owned());

    // Act — through the full invoke path: an env dump (what does a child SEE) and a write
    // (is the sandbox real — an absence proven in a workspace that refuses writes would be
    // vacuous).
    let (dump_record, dump_streams) = host.invoke(
        &ToolCall::Shell(graphhelm_tool_broker::call::ShellAction {
            program: "fake_tool".to_owned(),
            arguments: vec!["env-dump".to_owned()],
        }),
        &lease,
        "agent-prover",
    );
    let (write_record, write_streams) = host.invoke(
        &ToolCall::Shell(graphhelm_tool_broker::call::ShellAction {
            program: "fake_tool".to_owned(),
            arguments: vec!["write-file".to_owned(), "proof.txt".to_owned()],
        }),
        &lease,
        "agent-prover",
    );
    assert!(
        matches!(
            dump_record.disposition,
            graphhelm_tool_broker::record::ToolDisposition::Completed { exit_code: 0 }
        ) && matches!(
            write_record.disposition,
            graphhelm_tool_broker::record::ToolDisposition::Completed { exit_code: 0 }
        ),
        "the sandbox must actually run for its absences to mean anything"
    );

    const SENTINELS: &[&str] = &[
        "SENTINEL-events-passphrase",
        "SENTINEL-gateway-passphrase",
        "SENTINEL-ambient-token",
        "SENTINEL-keyring-material",
    ];

    // A. no sentinel value and no GRAPHHELM_* name reaches any captured stream.
    let dump_text = String::from_utf8_lossy(&dump_streams.stdout);
    for sentinel in SENTINELS {
        assert!(
            !dump_text.contains(sentinel),
            "{sentinel} visible to the child"
        );
    }
    assert!(
        !dump_text.contains("GRAPHHELM_"),
        "a GRAPHHELM_* name leaked into the child environment"
    );
    let _ = write_streams;

    // B. the kept workspaces accept writes AND contain no sentinel bytes anywhere.
    let mut scanned = 0_usize;
    let mut proof_seen = false;
    let mut stack: Vec<std::path::PathBuf> = vec![staging.path().to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path.is_file() {
                scanned += 1;
                if path.file_name().is_some_and(|name| name == "proof.txt") {
                    proof_seen = true;
                }
                let bytes = std::fs::read(&path).unwrap_or_default();
                let text = String::from_utf8_lossy(&bytes);
                for sentinel in SENTINELS {
                    assert!(
                        !text.contains(sentinel),
                        "{sentinel} found inside the workspace at {path:?}"
                    );
                }
            }
        }
    }
    assert!(
        proof_seen,
        "the write must have landed for the scan to mean anything"
    );
    assert!(scanned > 0, "the scan must have covered real files");

    // C. structural separation: the keyring shares no prefix with any workspace, either way.
    let keyring_canonical = keyring.path().canonicalize().unwrap();
    let staging_canonical = staging.path().canonicalize().unwrap();
    assert!(
        !keyring_canonical.starts_with(&staging_canonical)
            && !staging_canonical.starts_with(&keyring_canonical)
    );

    // D. the CLI-visible records carry no sentinel.
    for record in [&dump_record, &write_record] {
        let json = serde_json::to_string(record).unwrap();
        for sentinel in SENTINELS {
            assert!(!json.contains(sentinel), "{sentinel} inside a record");
        }
    }

    // keep_workspace: the kept trees are this test's to delete.
    for entry in std::fs::read_dir(staging.path()).unwrap() {
        let _ = std::fs::remove_dir_all(entry.unwrap().path());
    }
}

#[test]
fn an_identical_snapshot_closed_read_is_served_from_cache_without_executing() {
    let (_dir, project) = scratch_repo();
    let staging = tempfile::tempdir().unwrap();
    let host = host(&project, staging.path());
    let lease = full_lease("agent-builder");
    let diff = ToolCall::Repository(RepositoryAction::Diff);

    let (first, first_streams) = host.invoke(&diff, &lease, "agent-builder");
    assert!(!first.reused, "the first call must execute");
    let (second, second_streams) = host.invoke(&diff, &lease, "agent-builder");
    assert!(
        second.reused,
        "an identical clean-tree read must be a cache hit"
    );
    assert_eq!(first.stdout_sha256, second.stdout_sha256);
    assert_eq!(first_streams.stdout, second_streams.stdout);

    // A new commit is a new snapshot: the key must include HEAD, so the next call misses.
    std::fs::write(project.join("src/lib.rs"), "// changed\n").unwrap();
    let commit = Command::new("git")
        .args(["commit", "-aqm", "change"])
        .current_dir(&project)
        .env("GIT_AUTHOR_NAME", "scratch")
        .env("GIT_AUTHOR_EMAIL", "scratch@test.invalid")
        .env("GIT_COMMITTER_NAME", "scratch")
        .env("GIT_COMMITTER_EMAIL", "scratch@test.invalid")
        .status()
        .unwrap();
    assert!(commit.success());
    let (third, _) = host.invoke(&diff, &lease, "agent-builder");
    assert!(!third.reused, "a new HEAD must be a miss");
}

#[test]
fn a_dirty_working_tree_is_never_served_from_cache() {
    // The re-review's critical finding, as a test: Tier 0 reads touch the LIVE tree, and HEAD
    // does not pin uncommitted changes — a HEAD-keyed hit on a dirty tree would serve stale
    // bytes under the "provably exact" flag.
    let (_dir, project) = scratch_repo();
    let staging = tempfile::tempdir().unwrap();
    let host = host(&project, staging.path());
    let lease = full_lease("agent-builder");
    let read = ToolCall::Repository(RepositoryAction::ReadFile {
        path: graphhelm_tool_broker::path::RelativePath::parse("src/lib.rs").unwrap(),
    });

    // Prime on a clean tree.
    let (primed, _) = host.invoke(&read, &lease, "agent-builder");
    assert!(!primed.reused);
    let (hit, _) = host.invoke(&read, &lease, "agent-builder");
    assert!(hit.reused, "clean tree: hit is legal");

    // Dirty the tracked file WITHOUT committing: the live bytes win, never the cache.
    let original = std::fs::read(project.join("src/lib.rs")).unwrap();
    std::fs::write(project.join("src/lib.rs"), "// dirty edit\n").unwrap();
    let (dirty, dirty_streams) = host.invoke(&read, &lease, "agent-builder");
    assert!(!dirty.reused, "a dirty tree must bypass the cache");
    assert_eq!(
        dirty_streams.stdout, b"// dirty edit\n",
        "the live bytes must be served, not the cached ones"
    );

    // Revert (tree clean again): a hit is legal once more.
    std::fs::write(project.join("src/lib.rs"), original).unwrap();
    let (again, _) = host.invoke(&read, &lease, "agent-builder");
    assert!(
        again.reused,
        "reverted tree: the original entry may serve again"
    );
}

#[test]
fn cache_entries_die_with_their_evidence() {
    use graphhelm_tool_host::cache::ReadCache;
    let (_dir, project) = scratch_repo();
    let staging = tempfile::tempdir().unwrap();
    let host = host(&project, staging.path());
    let lease = full_lease("agent-builder");
    let diff = ToolCall::Repository(RepositoryAction::Diff);

    let (first, _) = host.invoke(&diff, &lease, "agent-builder");
    assert!(!first.reused);
    let (hit, _) = host.invoke(&diff, &lease, "agent-builder");
    assert!(hit.reused, "entry primed");

    // Tag the entry with its (future) evidence and erase that evidence: the entry must die —
    // a cache must never serve cryptographically erased evidence. The tag walk mirrors what
    // the 05d executor does after externalization.
    let cache = ReadCache::new(staging.path().join("ghtool-read-cache").parent().unwrap());
    let cache_dir = staging.path().join("ghtool-read-cache");
    for entry in std::fs::read_dir(&cache_dir).unwrap() {
        let key = entry.unwrap().file_name().to_string_lossy().into_owned();
        cache.tag_evidence(&key, "evidence-tool-1");
    }
    cache.invalidate_evidence("evidence-tool-1");
    let (after, _) = host.invoke(&diff, &lease, "agent-builder");
    assert!(
        !after.reused,
        "an erased entry must never serve again — the erasure hard constraint's cache half"
    );
}
