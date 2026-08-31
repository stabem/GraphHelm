//! The process primitive's isolation contract: the child environment is an allowlist (the
//! register's hard constraint, observed from inside the child), homes and temp are redirected
//! into the workspace, deadlines kill, output is capped without deadlock, and the child runs
//! where the workspace is.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use graphhelm_tool_host::process::{CapturedProcess, ProcessLimits, run_in_workspace};

fn fake_tool() -> String {
    env!("CARGO_BIN_EXE_fake_tool").to_owned()
}

fn limits() -> ProcessLimits {
    ProcessLimits {
        timeout: Duration::from_secs(10),
        max_output_bytes: 1024 * 1024,
    }
}

fn run(root: &Path, args: &[&str], limits: &ProcessLimits) -> CapturedProcess {
    run_in_workspace(
        root,
        &fake_tool(),
        &args.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>(),
        &BTreeMap::new(),
        &[],
        None,
        limits,
    )
    .unwrap()
}

#[test]
fn the_child_environment_is_an_allowlist_and_never_carries_host_secrets() {
    // The register's hard constraint, observed from inside the child. The sentinels must sit
    // in the PARENT process environment, and `std::env::set_var` is unsafe and cross-test-racy
    // — so this test is a two-piece wrapper: the OUTER run (marker unset) re-executes this
    // test binary filtered to this test's own name with the sentinels planted on the child's
    // environment; the INNER run (marker set) is the real assertion, whose parent environment
    // now genuinely carries the sentinels.
    if std::env::var_os("GH_TOOL_HOST_INNER").is_none() {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "the_child_environment_is_an_allowlist_and_never_carries_host_secrets",
                "--nocapture",
            ])
            .env("GH_TOOL_HOST_INNER", "1")
            .env("GRAPHHELM_EVENTS_KEY", "SENTINEL-events-passphrase")
            .env("GRAPHHELM_GATEWAY_KEY", "SENTINEL-gateway-passphrase")
            .env("FAKE_SECRET", "SENTINEL-ambient-token")
            .status()
            .expect("re-executing the test binary");
        assert!(status.success(), "the inner assertion run must pass");
        return;
    }

    let workspace = tempfile::tempdir().unwrap();
    let captured = run(workspace.path(), &["env-dump"], &limits());
    let dump = String::from_utf8_lossy(&captured.stdout);
    assert!(
        dump.lines().any(|l| l.starts_with("PATH=")),
        "PATH must survive"
    );
    for forbidden in [
        "GRAPHHELM_EVENTS_KEY",
        "GRAPHHELM_GATEWAY_KEY",
        "FAKE_SECRET",
        "SENTINEL",
    ] {
        assert!(
            !dump.contains(forbidden),
            "{forbidden} leaked into the Tier 1 child"
        );
    }
    assert!(
        !dump.lines().any(|l| l.starts_with("APPDATA=")),
        "APPDATA is not allowlisted"
    );
}

#[test]
fn home_and_temp_are_redirected_into_the_workspace() {
    let workspace = tempfile::tempdir().unwrap();
    let captured = run(workspace.path(), &["env-dump"], &limits());
    let dump = String::from_utf8_lossy(&captured.stdout);
    let expect = |name: &str| {
        let line = dump
            .lines()
            .find(|l| l.starts_with(&format!("{name}=")))
            .unwrap();
        assert!(
            Path::new(line.split_once('=').unwrap().1).starts_with(workspace.path()),
            "{name} must point inside the workspace, got {line}"
        );
    };
    for name in ["HOME", "USERPROFILE", "TEMP", "TMP"] {
        expect(name);
    }
    for fixed in [
        "GIT_CONFIG_NOSYSTEM=1",
        "GIT_TERMINAL_PROMPT=0",
        "GIT_OPTIONAL_LOCKS=0",
        // The synthetic commit identity (review finding 1): env_clear plus an empty redirected
        // HOME leaves git with no user.name/user.email anywhere, and `git commit` refuses with
        // "Please tell me who you are". A fixed identity in the child environment is the
        // config-free way to supply one, and it is deterministic across machines.
        "GIT_AUTHOR_NAME=GraphHelm Tool Broker",
        "GIT_AUTHOR_EMAIL=tools@graphhelm.invalid",
        "GIT_COMMITTER_NAME=GraphHelm Tool Broker",
        "GIT_COMMITTER_EMAIL=tools@graphhelm.invalid",
    ] {
        assert!(dump.contains(fixed), "{fixed} missing");
    }
}

#[test]
fn path_prepend_directories_lead_the_child_path() {
    // Host configuration (review finding 2): a test needs the fake_tool's directory — and an
    // operator may need a pinned toolchain directory — resolvable WITHOUT mutating the parent
    // process's PATH (racy across parallel tests) and without weakening the bare-name rule.
    // path_prepend is the host-side answer: directories joined ahead of the inherited PATH in
    // the CHILD only.
    let workspace = tempfile::tempdir().unwrap();
    let tool_dir = Path::new(&fake_tool()).parent().unwrap().to_path_buf();
    let captured = run_in_workspace(
        workspace.path(),
        &fake_tool(),
        &["env-dump".to_owned()],
        &BTreeMap::new(),
        std::slice::from_ref(&tool_dir),
        None,
        &limits(),
    )
    .unwrap();
    let dump = String::from_utf8_lossy(&captured.stdout);
    let path_line = dump.lines().find(|l| l.starts_with("PATH=")).unwrap();
    assert!(
        path_line["PATH=".len()..].starts_with(&tool_dir.display().to_string()),
        "prepended directory must lead PATH, got {path_line}"
    );
}

#[test]
fn extra_env_is_validated_and_recorded_shape_only() {
    use graphhelm_tool_host::process::HostError;
    // Declared extras exist for a tests runner that needs CARGO_HOME/RUSTUP_HOME pointing at a
    // credential-free toolchain home. GRAPHHELM_* names are structurally refused so the host's
    // own passphrases can never be handed back in — and (review finding 6) so is EVERY name
    // the host itself defines: the INHERITED allowlist (PATH, PATHEXT, ...), the redirected
    // names (HOME, USERPROFILE, TEMP, TMP) and the fixed GIT_* set. An extra_env PATH would
    // otherwise swap program resolution out from under the lease's allowlist.
    let workspace = tempfile::tempdir().unwrap();
    for denied in [
        "GRAPHHELM_EVENTS_KEY",
        "PATH",
        "PATHEXT",
        "HOME",
        "GIT_CONFIG_NOSYSTEM",
    ] {
        let mut extra = BTreeMap::new();
        extra.insert(denied.to_owned(), "x".to_owned());
        let refused = run_in_workspace(
            workspace.path(),
            &fake_tool(),
            &["env-dump".to_owned()],
            &extra,
            &[],
            None,
            &limits(),
        );
        assert!(
            matches!(refused.unwrap_err(), HostError::ExtraEnvDenied { .. }),
            "{denied} must be refused as an extra_env name"
        );
    }

    let mut ok = BTreeMap::new();
    ok.insert(
        "CARGO_HOME".to_owned(),
        workspace.path().join("ch").display().to_string(),
    );
    let captured = run_in_workspace(
        workspace.path(),
        &fake_tool(),
        &["env-dump".to_owned()],
        &ok,
        &[],
        None,
        &limits(),
    )
    .unwrap();
    assert!(String::from_utf8_lossy(&captured.stdout).contains("CARGO_HOME="));
}

#[test]
fn a_hung_child_is_killed_at_the_deadline() {
    let workspace = tempfile::tempdir().unwrap();
    let limits = ProcessLimits {
        timeout: Duration::from_secs(2),
        max_output_bytes: 1024,
    };
    let started = std::time::Instant::now();
    let captured = run(workspace.path(), &["sleep"], &limits);
    assert!(captured.timed_out);
    assert!(started.elapsed() < Duration::from_secs(30));
}

#[test]
fn oversize_output_is_capped_and_marked_truncated_without_deadlock() {
    let workspace = tempfile::tempdir().unwrap();
    let limits = ProcessLimits {
        timeout: Duration::from_secs(30),
        max_output_bytes: 64 * 1024,
    };
    let captured = run(workspace.path(), &["big-output"], &limits);
    assert!(captured.truncated);
    assert!(captured.stdout.len() <= 64 * 1024);
    assert_eq!(
        captured.exit_code,
        Some(0),
        "the child still ran to completion"
    );
}

#[test]
fn a_stdout_cut_is_distinguishable_from_a_stderr_cut() {
    // #177: `CapturedProcess` fused the two into `stdout_truncated || stderr_truncated`, so a stage
    // whose stderr was cut and whose stdout was whole was indistinguishable from the reverse. The
    // per-stream values already existed as locals in `run_in_workspace` and died at the struct
    // boundary -- this is the boundary, not a new measurement.
    let cap = 64 * 1024;
    let limits = ProcessLimits {
        timeout: Duration::from_secs(30),
        max_output_bytes: cap,
    };
    let out_workspace = tempfile::tempdir().unwrap();
    let err_workspace = tempfile::tempdir().unwrap();
    let out_cut = run(out_workspace.path(), &["big-output"], &limits);
    let err_cut = run(err_workspace.path(), &["big-stderr"], &limits);

    // ARRANGEMENT CONTROL, before the claim: the two runs must really have cut DIFFERENT streams.
    // Without this the assertion below could pass on two runs that cut the same one, and the test
    // would be about nothing.
    assert!(
        out_cut.stdout.len() >= cap && out_cut.stderr.is_empty(),
        "big-output must overflow stdout and leave stderr empty: {} / {}",
        out_cut.stdout.len(),
        out_cut.stderr.len()
    );
    assert!(
        err_cut.stderr.len() >= cap && err_cut.stdout.len() < cap,
        "big-stderr must overflow stderr and leave stdout short: {} / {}",
        err_cut.stdout.len(),
        err_cut.stderr.len()
    );

    // The fused flag this replaces says the SAME thing about both, and that identity is the defect
    // restated: it is kept as a derived value, so this line keeps measuring the loss it caused.
    assert_eq!(
        out_cut.truncated, err_cut.truncated,
        "the derived flag is still the OR of the two, so both cuts still read alike through it"
    );

    // The claim: the record now says WHICH stream was cut.
    assert_eq!(
        (out_cut.stdout_truncated, out_cut.stderr_truncated),
        (true, false),
        "stdout was the cut stream"
    );
    assert_eq!(
        (err_cut.stdout_truncated, err_cut.stderr_truncated),
        (false, true),
        "stderr was the cut stream"
    );
}

#[test]
fn the_child_runs_in_the_workspace_directory() {
    let workspace = tempfile::tempdir().unwrap();
    let captured = run(workspace.path(), &["cwd"], &limits());
    let reported = String::from_utf8_lossy(&captured.stdout);
    let reported = Path::new(reported.trim());
    assert_eq!(
        reported.canonicalize().unwrap(),
        workspace.path().canonicalize().unwrap()
    );
}
