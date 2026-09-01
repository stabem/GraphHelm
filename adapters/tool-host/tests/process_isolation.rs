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
        None,
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
        None,
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
            None,
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
        None,
    )
    .unwrap();
    assert!(String::from_utf8_lossy(&captured.stdout).contains("CARGO_HOME="));
}

/// #180: a cancelled call leaves no live child, proved by EFFECT rather than by PID.
///
/// `sleep` would let this be faked: a parent that stops waiting looks the same as a child that
/// died, because the only observable is the call returning. `append-forever` writes OUTSIDE the
/// process, so "the child is gone" is measured by the file no longer growing -- a claim about the
/// machine rather than about the parent's own bookkeeping.
///
/// The deadline is 60 s and this finishes in well under one: if the cancel path were removed the
/// call would sit there writing, which is the state `main` was in before this commit.
#[test]
fn a_cancelled_call_leaves_no_live_child() {
    let workspace = tempfile::tempdir().unwrap();
    let root = workspace.path().to_path_buf();
    let trace = root.join("trace.txt");
    let signal = graphhelm_tool_host::process::CancelSignal::new();

    let handle = {
        let (root, trace, signal) = (root.clone(), trace.clone(), signal.clone());
        std::thread::spawn(move || {
            run_in_workspace(
                &root,
                &fake_tool(),
                &["append-forever".to_owned(), trace.display().to_string()],
                &BTreeMap::new(),
                &[],
                None,
                &ProcessLimits {
                    timeout: Duration::from_secs(60),
                    max_output_bytes: 1024 * 1024,
                },
                Some(&signal),
            )
        })
    };

    // ARRANGEMENT CONTROL: the child must really be running and really writing, or "it stopped"
    // is a statement about a process that never started.
    let mut grew = false;
    for _ in 0..100 {
        if std::fs::read(&trace).map(|b| b.len()).unwrap_or(0) > 20 {
            grew = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        grew,
        "the child never started writing; nothing below measures a cancellation"
    );

    signal.cancel();
    let captured = handle
        .join()
        .expect("the call thread returns")
        .expect("a record");

    // A WITNESS, and NOT the discriminator -- credited correctly after M measured which assertion
    // actually fires (#609). Under `main` this cannot fail: the call returns from `join` only when
    // `run_in_workspace` does, which under the old behaviour is at the 60 s deadline, AFTER it has
    // killed and reaped. So `settled` is sampled once the child is already dead, by the very
    // deadline that masks the defect. It is kept because it is the only thing here that speaks
    // about the MACHINE rather than about a field, and because under a future wrong fix that
    // returns early without killing it is the one that would notice.
    let settled = std::fs::metadata(&trace).map(|m| m.len()).unwrap_or(0);
    std::thread::sleep(Duration::from_millis(400));
    let later = std::fs::metadata(&trace).map(|m| m.len()).unwrap_or(0);
    assert_eq!(
        settled, later,
        "the trace file kept growing after the call returned: a child survived the cancellation"
    );

    // THE CLAIM, and the only assertion here that separates the fix from `main`: a cancelled call
    // must not be recorded as a timeout. Under the old behaviour the 60 s deadline is what stopped
    // the child, so `timed_out` is true and the journal blames a clock for a decision. Sixty
    // seconds did not elapse from the caller's point of view -- it cancelled in under one.
    assert!(
        !captured.timed_out,
        "a cancelled call recorded as TIMED OUT puts a false cause in the journal"
    );
}

/// `cancel` RETURNS ONLY AFTER THE REAP — the difference between signalling and guaranteeing.
///
/// #180's criterion is "a cancelled execution leaves no live child", and a caller can only rely on
/// that if it holds AT THE RETURN. A `cancel` that stored a flag and returned left a child running
/// for up to one poll interval, so the caller had to invent its own wait — which is the bookkeeping
/// the signal exists to hold (Codex, #609).
///
/// This cell deliberately does NOT join the call thread before measuring. Joining would wait for
/// the child by another route and hide exactly the property under test: the previous cell passes
/// under both behaviours because it joins first, which is why it could not have caught this.
#[test]
fn cancel_returns_only_after_the_child_is_reaped() {
    let workspace = tempfile::tempdir().unwrap();
    let root = workspace.path().to_path_buf();
    let trace = root.join("trace.txt");
    let signal = graphhelm_tool_host::process::CancelSignal::new();

    let handle = {
        let (root, trace, signal) = (root.clone(), trace.clone(), signal.clone());
        std::thread::spawn(move || {
            run_in_workspace(
                &root,
                &fake_tool(),
                &["append-forever".to_owned(), trace.display().to_string()],
                &BTreeMap::new(),
                &[],
                None,
                &ProcessLimits {
                    timeout: Duration::from_secs(60),
                    max_output_bytes: 1024 * 1024,
                },
                Some(&signal),
            )
        })
    };

    let mut grew = false;
    for _ in 0..100 {
        if std::fs::metadata(&trace).map(|m| m.len()).unwrap_or(0) > 20 {
            grew = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        grew,
        "the child never started writing; nothing below measures a reap"
    );

    signal.cancel();

    // No join between the cancel and this measurement. If `cancel` merely signalled, the child is
    // still inside its poll interval here and the file grows across these two samples.
    let at_return = std::fs::metadata(&trace).map(|m| m.len()).unwrap_or(0);
    std::thread::sleep(Duration::from_millis(300));
    let after = std::fs::metadata(&trace).map(|m| m.len()).unwrap_or(0);
    assert_eq!(
        at_return, after,
        "the child was still writing when cancel returned: the caller was told the run stopped \
         while it had not"
    );

    let _ = handle.join().expect("the call thread returns");
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
fn a_capped_stream_keeps_its_end_where_the_failing_assertion_lives() {
    // #177, the tail half. The reader kept the HEAD and discarded everything after the cap. In a
    // red test suite the assertion that failed, and the `test result: FAILED` line, are at the END
    // -- so the capture was structurally biased against the one thing a triager opens the log for.
    //
    // Both ends matter, and each has a live instance from this repository's own work:
    //   HEAD -- a toolchain failure (`invalid metadata for crate core`) prints as the build starts;
    //   TAIL -- the failing assertion and the result line print last.
    // So the shape is head + tail with the middle elided, not tail-only.
    let workspace = tempfile::tempdir().unwrap();
    let cap = 64 * 1024;
    let limits = ProcessLimits {
        timeout: Duration::from_secs(30),
        max_output_bytes: cap,
    };
    let captured = run(workspace.path(), &["marked-output"], &limits);

    // ARRANGEMENT CONTROL: the stream must really have overflowed, or "the tail survived" is a
    // statement about a stream that was never cut.
    assert!(
        captured.stdout_truncated,
        "marked-output must overflow the cap, or this proves nothing about truncation"
    );
    assert!(
        captured.stdout.len() <= cap,
        "the capture must stay within its budget: {}",
        captured.stdout.len()
    );

    let text = String::from_utf8_lossy(&captured.stdout);
    assert!(
        text.contains("TAIL-SENTINEL"),
        "the END of the stream must survive the cap -- that is where a red suite puts the failure"
    );
    assert!(
        text.contains("HEAD-SENTINEL"),
        "the START must survive too: a toolchain or setup failure prints before anything else"
    );
}

/// #582 finding 1 (D): a stream that passes `head_cap` but loses NOTHING must not claim loss.
///
/// The first version set `truncated` as soon as any byte went past the head, so under the
/// production 8 MiB cap every stream over 4 MiB was recorded as truncated whether or not anything
/// was elided. #177 exists because the record did not say what happened; a slice of it must not add
/// a field that says something that did not happen.
#[test]
fn a_stream_that_loses_nothing_is_not_recorded_as_truncated() {
    let workspace = tempfile::tempdir().unwrap();
    // 12 MiB cap over 8 MiB of output: past the 6 MiB head, nothing dropped.
    let limits = ProcessLimits {
        timeout: Duration::from_secs(30),
        max_output_bytes: 12 * 1024 * 1024,
    };
    let captured = run(workspace.path(), &["big-output"], &limits);

    // ARRANGEMENT: the stream really did pass head_cap, or the claim is about nothing.
    assert!(
        captured.stdout.len() > 6 * 1024 * 1024,
        "the stream must exceed head_cap for this to test anything: {}",
        captured.stdout.len()
    );
    assert!(
        !captured.stdout_truncated,
        "every byte survived, so the record must not claim data loss"
    );
    assert!(
        !captured
            .stdout
            .windows(9)
            .any(|w| w == b"elided ..".as_slice()),
        "nothing was elided, so no marker may appear"
    );
}

/// #582 finding 2 (D): the marker's own length must never carry the capture past the cap.
///
/// Two shapes, one cause — the budgeted length and the emitted length came from different values of
/// `elided`. Both caps below are D's measured boundaries, not invented ones.
#[test]
fn the_elision_marker_never_pushes_the_capture_over_its_cap() {
    let cases = [
        (
            64 * 1024_usize,
            "an ordinary cap, the control that this is about boundaries",
        ),
        (
            7_388_638,
            "the digit-carry boundary: measured one byte over",
        ),
        (
            40,
            "a cap smaller than the marker itself: measured twelve bytes over",
        ),
    ];
    for (cap, why) in cases {
        let workspace = tempfile::tempdir().unwrap();
        let limits = ProcessLimits {
            timeout: Duration::from_secs(30),
            max_output_bytes: cap,
        };
        let captured = run(workspace.path(), &["big-output"], &limits);
        // EQUALITY, not `<= cap`, and the difference is not pedantry. A one-sided bound is
        // satisfied by an EMPTY capture, so it cannot tell "the marker fit" from "nothing was
        // captured at all" -- measured: a broken build of this loop produced len=0 here and the
        // `<=` version passed. The stream is 8 MiB against every cap below, so a correct capture
        // fills its budget exactly.
        assert_eq!(
            captured.stdout.len(),
            cap,
            "cap {cap} must be filled exactly, not exceeded and not left short ({why})"
        );
    }
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

/// #538 (D-042's last clause): the provider cache directory is CONFINED — set by the host to a
/// path inside the workspace, exactly as HOME and TEMP already are. A broker-run index process
/// must have nowhere to write except its sandbox, and nowhere to read a host cache from.
#[test]
fn cbm_cache_dir_is_redirected_into_the_workspace() {
    let workspace = tempfile::tempdir().unwrap();
    let captured = run(workspace.path(), &["env-dump"], &limits());
    let dump = String::from_utf8_lossy(&captured.stdout);

    let line = dump
        .lines()
        .find(|line| line.starts_with("CBM_CACHE_DIR="))
        .unwrap_or_else(|| {
            panic!("CBM_CACHE_DIR must be SET for the child, inside the workspace; dump:\n{dump}")
        });
    let value = line.trim_start_matches("CBM_CACHE_DIR=");
    let canonical_root = workspace.path().canonicalize().unwrap();
    let canonical_value = Path::new(value)
        .canonicalize()
        .expect("the confined cache dir must exist before the child runs");
    assert!(
        canonical_value.starts_with(&canonical_root),
        "the cache dir must live INSIDE the workspace root: {value}"
    );
}

/// The refusal half: a caller cannot smuggle its own cache path through `extra_env` — the name
/// is refused BEFORE anything runs, case-insensitively (Windows env names are case-insensitive,
/// so `cbm_cache_dir` would shadow the confinement just as surely).
#[test]
fn an_extra_env_cbm_cache_dir_is_refused_before_the_process_starts() {
    use graphhelm_tool_host::process::HostError;

    let workspace = tempfile::tempdir().unwrap();
    for name in ["CBM_CACHE_DIR", "cbm_cache_dir"] {
        let mut extra = BTreeMap::new();
        extra.insert(name.to_owned(), "C:/somewhere/outside".to_owned());
        let refused = run_in_workspace(
            workspace.path(),
            &fake_tool(),
            &["env-dump".to_owned()],
            &extra,
            &[],
            None,
            &limits(),
            None,
        )
        .expect_err("a caller-supplied cache path must be refused, not honoured");
        assert!(
            matches!(refused, HostError::ExtraEnvDenied { name: denied } if denied == name),
            "the refusal names the exact key handed in"
        );
        // L's #544 pin: the refusal precedes the workspace preparation, so a pre-spawn refusal
        // cannot have created the sandbox dirs -- this is what makes "before the process starts"
        // an ASSERTION instead of a name, and it reddens the moment the check moves later.
        assert!(
            !workspace.path().join(".cbm-cache").exists(),
            "a PRE-SPAWN refusal cannot have created the sandbox dirs"
        );
    }
}

/// The host's own cache path is unreachable: a parent carrying CBM_CACHE_DIR (as the operator's
/// shell realistically does) never leaks it — the child sees the CONFINED value, not the host's.
/// Same two-piece wrapper as the secrets cell: the sentinel must sit in a REAL parent process
/// environment.
#[test]
fn a_host_cbm_cache_dir_never_reaches_the_child() {
    if std::env::var_os("GH_TOOL_HOST_INNER_CBM").is_none() {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "a_host_cbm_cache_dir_never_reaches_the_child",
                "--nocapture",
            ])
            .env("GH_TOOL_HOST_INNER_CBM", "1")
            .env("CBM_CACHE_DIR", "C:/SENTINEL-host-cache")
            .status()
            .expect("re-executing the test binary");
        assert!(status.success(), "the inner assertion run must pass");
        return;
    }

    let workspace = tempfile::tempdir().unwrap();
    let captured = run(workspace.path(), &["env-dump"], &limits());
    let dump = String::from_utf8_lossy(&captured.stdout);
    assert!(
        !dump.contains("SENTINEL-host-cache"),
        "the host's cache path leaked into the Tier 1 child"
    );
    assert!(
        dump.lines().any(|line| line.starts_with("CBM_CACHE_DIR=")),
        "the confined value must be present in its place"
    );
}

/// #609, Codex round 3. `cancel` waits only for the children it has already counted, so any spawn
/// that registers AFTER it returned outlives the guarantee it just gave. The gap has two shapes and
/// they close together: between `Command::spawn` and the registration, and between the two
/// subprocesses of `RepositoryTool::commit`, where the count is legitimately zero in between.
///
/// The observable here is the REFUSAL rather than the race. Catching the window itself would take a
/// sleep inside the library; asserting that a raised signal admits no further spawn is
/// deterministic, and it is exactly the property that closes both shapes.
#[test]
fn a_raised_signal_refuses_the_next_spawn() {
    let workspace = tempfile::tempdir().unwrap();
    let signal = graphhelm_tool_host::process::CancelSignal::new();
    signal.cancel();

    let refused = run_in_workspace(
        workspace.path(),
        &fake_tool(),
        &["echo".to_owned(), "hello".to_owned()],
        &BTreeMap::new(),
        &[],
        None,
        &limits(),
        Some(&signal),
    );

    match refused {
        Err(graphhelm_tool_host::process::HostError::Cancelled) => {}
        Err(other) => panic!("the spawn was refused for the wrong reason: {other}"),
        Ok(captured) => panic!(
            "a raised signal let another child start; it exited {:?}",
            captured.exit_code
        ),
    }
}

/// #609, Codex round 3. `timed_out` was `expired` inside `if expired || cancelled`, so a cancel
/// raised inside the last poll interval before the deadline left BOTH true and the record blamed
/// the clock for a decision the caller had made — the one field this change calls its
/// discriminator.
///
/// The coincidence is not reachable by picking one instant, so this SWEEPS the raise time instead
/// of guessing it. The first version of this cell did guess — cancel at 280 ms against a 300 ms
/// deadline — and it passed under the sabotage, proving nothing. The reason is a reference point:
/// the deadline is computed INSIDE the call, after the spawn, so an externally timed raise lands
/// earlier than intended by the whole cost of creating a process, and the control I had written
/// (the call returned at or after the deadline) could not see that, because the returned-at instant
/// includes the kill, the reap, and both reader joins.
///
/// The sweep's adequacy is not asserted, it is MEASURED: with `timed_out = expired` restored, some
/// step of this sweep must report a timeout. That is the check that makes the green below mean
/// something, and it is recorded in the PR rather than left as a claim.
#[test]
fn a_cancel_inside_the_last_poll_is_not_recorded_as_a_timeout() {
    let workspace = tempfile::tempdir().unwrap();
    let root = workspace.path().to_path_buf();
    let timeout = Duration::from_millis(300);

    // The sweep stops BELOW the nominal timeout, and that bound is the assertion's licence rather
    // than caution. The deadline is computed inside the call, after the spawn, so the internal
    // deadline is always at or later than `timeout` measured from out here: any raise before
    // `timeout` is therefore guaranteed to precede it, and `timed_out` must be false for every one
    // of them. Past `timeout` that guarantee is gone — the deadline's own poll can fire before the
    // cancel exists, and a timeout is then the TRUE record. The first version of this sweep ran to
    // 310 ms and failed on the fixed code for exactly that reason: it demanded a cancellation
    // verdict from runs the caller had not yet cancelled.
    let mut blamed_the_clock = Vec::new();
    for raise_at in (240..300).step_by(5) {
        let signal = graphhelm_tool_host::process::CancelSignal::new();
        let raiser = {
            let signal = signal.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(raise_at));
                signal.cancel();
            })
        };

        let captured = run_in_workspace(
            &root,
            &fake_tool(),
            &["sleep".to_owned()],
            &BTreeMap::new(),
            &[],
            None,
            &ProcessLimits {
                timeout,
                max_output_bytes: 1024,
            },
            Some(&signal),
        )
        .expect("a cancelled call still returns a record for the child that stopped");
        raiser.join().expect("the raising thread returns");

        if captured.timed_out {
            blamed_the_clock.push(raise_at);
        }
    }

    assert!(
        blamed_the_clock.is_empty(),
        "the caller cancelled and the record blamed the clock; raise offsets that did it: {:?}ms \
         against a {timeout:?} deadline",
        blamed_the_clock
    );
}
