//! The process primitive's isolation contract: the child environment is an allowlist (the
//! register's hard constraint, observed from inside the child), homes and temp are redirected
//! into the workspace, deadlines kill, output is capped without deadlock, and the child runs
//! where the workspace is.

use std::collections::BTreeMap;
use std::io::Read as _;
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::time::Duration;

use graphhelm_tool_host::process::{CapturedProcess, ProcessLimits, run_in_workspace};

fn fake_tool() -> String {
    env!("CARGO_BIN_EXE_fake_tool").to_owned()
}

/// The grandchild's id, waited on as an EVENT and still bounded (#727).
///
/// The fixture used to write the id into a file, which a test can only ask about again later --
/// so the wait was 400 sleeps of 50 ms and the normal path's duration was set by the poll
/// interval rather than by the fixture. On a loaded gate the window expired and the cell reported
/// HARNESS-BROKE: the right colour, and still a run that decided nothing.
///
/// A connection is something a test can block on, so the wait returns the instant the id exists.
/// The bound stays, because #703's whole thesis is that a wait made deterministic by being
/// unbounded reintroduces the third colour -- `cargo test` has no per-test timeout, so a fixture
/// that never reports would become a suite that never returns rather than a refusal anyone can
/// read. What changed is that the bound is now only ever REACHED when nothing was reported at
/// all; it no longer decides the normal path.
///
/// `None` is that refusal, and it is the caller's job to say HARNESS-BROKE about it rather than to
/// read it as a verdict about a tree kill.
fn wait_for_reported_grandchild(listener: TcpListener, patience: Duration) -> Option<u32> {
    let address = listener
        .local_addr()
        .expect("the listener knows its own address");
    let (sender, receiver) = std::sync::mpsc::channel();
    let accepting = std::thread::spawn(move || {
        let reported = listener.accept().ok().and_then(|(mut stream, _)| {
            // The reader carries the same bound. Without it a fixture that connects and then never
            // writes parks this thread forever -- the hang moved rather than removed, one layer
            // further in and harder to see than the one being fixed.
            stream.set_read_timeout(Some(patience)).ok()?;
            let mut text = String::new();
            stream.read_to_string(&mut text).ok()?;
            text.trim().parse::<u32>().ok()
        });
        let _ = sender.send(reported);
    });

    let answer = receiver.recv_timeout(patience).ok().flatten();
    if answer.is_none() {
        // A thread still parked in `accept` is woken by connecting to it, rather than left to leak
        // for the life of the process. #726 is the same shape one layer out: an abandoned reader
        // is not a reaped one, and a test suite that leaks a thread per refusal is a suite whose
        // later cells run in a machine it damaged itself.
        let _ = TcpStream::connect(address);
    }
    let _ = accepting.join();
    answer
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
    // The arrangement's own budget, named because the number stopped being arbitrary. Spawning
    // costs a `CREATE_SUSPENDED` start plus a walk of the thread snapshot since #618 -- the same
    // design that closes the window a child used to escape the job through -- so the setup pays
    // real time before the property is even observable. This is SETUP time, not tolerance for the
    // property: whatever is asserted below fails with its own sentence, never by running out of
    // this budget.
    const ARRANGEMENT_POLL: Duration = Duration::from_millis(50);
    const ARRANGEMENT_ATTEMPTS: u32 = 400;
    let mut grew = false;
    for _ in 0..ARRANGEMENT_ATTEMPTS {
        if std::fs::read(&trace).map(|b| b.len()).unwrap_or(0) > 20 {
            grew = true;
            break;
        }
        std::thread::sleep(ARRANGEMENT_POLL);
    }
    assert!(
        grew,
        "the child never started writing within {:?}; nothing below measures a cancellation",
        ARRANGEMENT_POLL * ARRANGEMENT_ATTEMPTS
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

    // The arrangement's own budget, named because the number stopped being arbitrary. Spawning
    // costs a `CREATE_SUSPENDED` start plus a walk of the thread snapshot since #618 -- the same
    // design that closes the window a child used to escape the job through -- so the setup pays
    // real time before the property is even observable. This is SETUP time, not tolerance for the
    // property: whatever is asserted below fails with its own sentence, never by running out of
    // this budget.
    const ARRANGEMENT_POLL: Duration = Duration::from_millis(50);
    const ARRANGEMENT_ATTEMPTS: u32 = 400;
    let mut grew = false;
    for _ in 0..ARRANGEMENT_ATTEMPTS {
        if std::fs::metadata(&trace).map(|m| m.len()).unwrap_or(0) > 20 {
            grew = true;
            break;
        }
        std::thread::sleep(ARRANGEMENT_POLL);
    }
    assert!(
        grew,
        "the child never started writing within {:?}; nothing below measures a reap",
        ARRANGEMENT_POLL * ARRANGEMENT_ATTEMPTS
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

/// #621: the child is gone, proved by ASKING THE OS rather than by watching a file go quiet.
///
/// The cells above sample a trace file across a wall-clock window. That is what `AGENTS.md:121`
/// forbids, and the reason it cannot be repaired in place is sharper than the rule: a child that is
/// alive but DESCHEDULED for the whole window writes nothing and is indistinguishable from a dead
/// one, because both produce zero bytes. Widening the window makes the flake rarer and the suite
/// slower, and never makes the oracle able to tell the two apart.
///
/// This one asks the operating system. It is deterministic in both directions.
///
/// **Its own weakness, and which way it fails.** A process id may be recycled once its process is
/// reaped, so this proves "nothing with that id is running", not "that child is not running". The
/// recycle window is the width of one query, and a recycle would make the assertion see a LIVE id
/// and go red. A guard whose weakness pushes it toward red is the shape to prefer; the alternative
/// would have been a false green, which is what the file sampling could produce.
#[test]
fn a_cancelled_call_leaves_no_process_alive_under_that_id() {
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

    // Arrangement: the seam must have BOUND a spawn, and the OS must agree it is running. Without
    // both, the assertion below would be about a child that never started -- and "no live process"
    // is trivially true of a process that does not exist.
    // NOTHING in this loop panics, and that is the shape rather than the style (Codex, on #680).
    // The first version raised the arrangement failure here, BEFORE `cancel` and `join` — so a
    // runner slow enough to miss the window left the child and the call thread alive for the whole
    // 60 s timeout. A cancellation test that leaks a child on its own failure path is the defect it
    // exists to catch, and it is the third time this family has bitten its own harness.
    //
    // The wait polls an observable that ALREADY EXISTS — `spawned_processes()`, which the assertion
    // reads anyway — rather than a readiness signal added to `CancelSignal` for this test's benefit.
    // A `Condvar` on a production type, on the funnel every Tier 1 execution passes through, is API
    // surface bought for a harness; the poll costs nothing anyone else has to know about.
    const READINESS_POLL: Duration = Duration::from_millis(50);
    const READINESS_ATTEMPTS: u32 = 400;

    let mut bound = None;
    let mut unbindable = None;
    for _ in 0..READINESS_ATTEMPTS {
        if let Some(observation) = signal.spawned_processes().first().cloned() {
            match observation.is_running() {
                Ok(true) => {
                    bound = Some(observation);
                    break;
                }
                Ok(false) => {}
                // Below the floor the instrument REFUSES rather than falling back to a bare id, so
                // this is "I could not ask" -- a broken harness, not an answer, and it must never
                // be read as either colour.
                Err(error) => {
                    unbindable = Some(error);
                    break;
                }
            }
        }
        std::thread::sleep(READINESS_POLL);
    }

    signal.cancel();

    // Read BETWEEN the cancel and the join, and it has to be here: `cancel` promises the reap is
    // done when it returns, and joining first would wait for the child by another route and hide
    // exactly the property under test. The question goes through an identity the reap cannot
    // invalidate, so a recycled id cannot answer it.
    let liveness = bound
        .as_ref()
        .map(|observation| (observation.process_id(), observation.is_running()));

    // Cleanup happens before any verdict, so no failure path below can leave a child behind.
    let _ = handle.join().expect("the call thread returns");

    if let Some(error) = unbindable {
        panic!(
            "HARNESS-BROKE: the child could not be bound to a durable identity ({error}); this \
             cell cannot decide anything about liveness"
        );
    }
    // An exhausted deadline is HARNESS-BROKE and says so, with the bound named. The arrangement was
    // never met, which is a statement about this machine rather than about cancellation — colouring
    // it as a property failure would put a red on the gate for a slow runner, which is the shape the
    // report named.
    let Some((process_id, liveness)) = liveness else {
        panic!(
            "HARNESS-BROKE: arrangement not met in {:?} — no child was ever observed running, so \
             nothing here measures a reap and this run decides nothing about cancellation",
            READINESS_POLL * READINESS_ATTEMPTS
        );
    };
    match liveness {
        Ok(alive) => assert!(
            !alive,
            "cancel returned while process {process_id} was still running: the caller was told the \
             run stopped when it had not"
        ),
        Err(error) => panic!(
            "HARNESS-BROKE: the identity stopped answering after cancel ({error}); the cell cannot \
             tell a live child from a dead one"
        ),
    }
}

/// #618: the kill reaches the TREE, proved on a process the host never had a handle to.
///
/// **Why no earlier cell could see this.** Every other fixture is a single process — `sleep`,
/// `append-forever`, `big-output` — so "the kill reached everything" and "the kill reached the one
/// thing there was" are the same observation. The gap survived three rounds of cancellation work
/// because the fixtures could not express it, not because anyone argued it away.
///
/// The grandchild reports its OWN id, because the host never held a handle to it: that is the whole
/// difficulty, and it is why the fixture reports the id out of band instead of the test reading it
/// from anywhere in the host. It reports over a socket rather than into a file so the wait can be
/// an event rather than a poll -- see `wait_for_reported_grandchild` and #727.
///
/// **The CANCELLATION is the trigger, and the choice is about determinism rather than semantics.**
/// Both stop conditions go through the same two lines, so the subject is identical either way; the
/// deadline would make the arrangement race a clock, and a cell that fails sometimes teaches people
/// to re-run instead of to read. #180's cell is still where cancellation-SPECIFIC behaviour lives.
///
/// **This is a LINK cell.** Delete the call to `graphhelm_process_tree` from `run_in_workspace` and
/// it fails; the extracted crate's own tests would still pass. `origin/main` before this change is
/// exactly that state, which is where its red was measured.
#[test]
fn the_stop_kills_the_whole_tree_and_not_only_the_direct_child() {
    let workspace = tempfile::tempdir().unwrap();
    // Bound BEFORE the fixture is spawned, because its address is what the fixture is told to
    // report to. It replaces a file in a second temporary directory that existed only so the
    // evidence would outlive the workspace the host deletes after the call -- an id that never
    // lands on disk has nothing to outlive.
    let listener = TcpListener::bind("127.0.0.1:0").expect("the readiness listener binds loopback");
    let report_address = listener
        .local_addr()
        .expect("the readiness listener has an address")
        .to_string();

    // The stop is triggered by READINESS, not by a clock (Codex, on #703). The first version gave
    // the call a two-second deadline and hoped the fixture had spawned its grandchild and written
    // the id before it fired; on a loaded machine it may not, and the cell then fails reading a
    // missing file while the tree kill is perfectly correct -- a red about this code caused by the
    // harness. A cell that fails sometimes teaches people to re-run instead of to read, and that is
    // paid once by whoever writes it and forever by everyone else.
    //
    // `CancelSignal` fires the SAME kill and reap lines the deadline would, so nothing about the
    // subject changes: only who decides when.
    let signal = graphhelm_tool_host::process::CancelSignal::new();
    let handle = {
        let (root, report_address, signal) = (
            workspace.path().to_path_buf(),
            report_address.clone(),
            signal.clone(),
        );
        std::thread::spawn(move || {
            run_in_workspace(
                &root,
                &fake_tool(),
                &["spawn-grandchild".to_owned(), report_address],
                &BTreeMap::new(),
                &[],
                None,
                &ProcessLimits {
                    // Generous: the deadline must NOT be what stops this call.
                    timeout: Duration::from_secs(120),
                    max_output_bytes: 1024,
                },
                Some(&signal),
            )
        })
    };

    // Readiness is the grandchild's id ARRIVING, which the fixture now reports over a socket the
    // test blocks on. Bounded, and an exhausted bound is HARNESS-BROKE naming the limit rather than
    // a red about the kill.
    //
    // The bound is generous BECAUSE it is not the mechanism, exactly as the reap patience below is:
    // the wait returns the instant the id arrives, so on the passing path its size costs nothing.
    // Its predecessor was 400 sleeps of 50 ms, and there the size WAS the mechanism -- the normal
    // path paid at least one interval, and a loaded gate could exhaust the window while the tree
    // kill was perfectly correct (#727).
    const READY_PATIENCE: Duration = Duration::from_secs(20);
    // The identity is captured HERE, while the grandchild is certainly alive, and held across the
    // kill. A numeric pid is recyclable: probing it after the kill can report an unrelated
    // replacement as alive, and killing it then terminates a stranger on a busy host (Codex, on
    // #703). On Windows an open handle keeps the kernel object -- and therefore the pid -- from
    // being reused for as long as this binding lives, which is what makes the reap below safe.
    let reported = wait_for_reported_grandchild(listener, READY_PATIENCE);
    let identity = reported.map(graphhelm_process_tree::ProcessIdentity::capture);

    signal.cancel();
    let captured = handle
        .join()
        .expect("the call thread returns")
        .expect("the call returns a record for the child the cancellation stopped");

    let Some(grandchild) = reported else {
        panic!(
            "HARNESS-BROKE: the fixture never reported a grandchild id in {READY_PATIENCE:?}; \
             nothing here measures a tree kill"
        );
    };

    // Read BEFORE the property, and in its own colour. A descendant that escaped the kill holds the
    // pipes open, so the capture had to abandon its readers — an INSTRUMENT failure, and also the
    // reason the sabotaged run finishes at all instead of wedging. Sorting it under the property's
    // red would file "the harness could not read" as "the tree kill failed", and only one of those
    // two is about this code.
    //
    // This wording was briefly replaced by one describing a grace that releases the group early and
    // keeps what drained. That mechanism was written and measured — 12.34 s against this sabotage,
    // with a partial capture instead of none — and then destroyed by a `git checkout HEAD --` that
    // was undoing a sabotage in the same file, because the design was uncommitted and HEAD was the
    // commit before it. The message edit committed alone, describing code that no longer existed and
    // reading as the better engineering of the two.
    //
    // The design is #708, with both measurements and the refinement it still needs (the grace must
    // expire on SILENCE rather than elapsed time, or it cuts a descendant that is legitimately still
    // writing — a tests runner's workers are descendants and their output is the point). The rule
    // that would have prevented the loss: a sabotage and a fix must never share uncommitted state.
    // The verdict is TAKEN, then the survivor is cleaned up, and only then is anything asserted
    // (Codex, on #703). The first version asserted straight away, so on the failure it exists to
    // detect — a grandchild that outlived the kill — it panicked and left that grandchild sleeping
    // for its full hour. **A test about leaked processes that leaks a process when it catches one**,
    // and it is the second time this file has had the shape: the arrangement used to panic before
    // `cancel` and `join` for the same reason.
    //
    // The OS reports the exit; this cell does not sample for it. `TerminateJobObject` returns before
    // the job's processes have finished exiting and the `join` above waits for the DIRECT child's
    // reap, so the grandchild's death is genuinely later than this point -- but "later" is an event,
    // not a duration. A 100 x 50 ms loop turned that into an elapsed-time verdict, and on a loaded
    // gate it could leave `survived` true for a descendant that exited immediately afterwards: a
    // false red about the kill, produced by the scheduler (Codex, on #703).
    //
    // The patience is generous BECAUSE it is not the mechanism. The wait returns the instant the
    // process goes, so on the passing path its size costs nothing; it is only reached when the
    // grandchild really is still there, and the defect this cell detects leaves it sleeping for an
    // hour. The bound stays only because an unbounded wait would turn a survivor into a suite that
    // never returns -- a third colour instead of a failure.
    const REAP_PATIENCE: Duration = Duration::from_secs(30);

    // The verdict comes from the IDENTITY, and from nothing else. Where the platform has none --
    // `capture` refuses on unix that is not Linux, and on kernels before `pidfd_open` -- this cell
    // has NO adequate observer and stops saying so, rather than falling back to the pid probe.
    //
    // The fallback was mine and it was wrong (Codex, on #703). `kill(pid, 0)` succeeds against a
    // ZOMBIE, so a grandchild the tree kill correctly killed reads as alive until its adopter reaps
    // it, and this authoritative gate would fail on the OS's reaping schedule. That is #715, still
    // open, and judging with an instrument a filed defect says lies is worse than not judging:
    // AGENTS.md:121 -- "if a promised behavior has no adequate observer, stop with OBSERVER_MISSING".
    let Some(handle) = identity
        .as_ref()
        .and_then(|captured| captured.as_ref().ok())
    else {
        panic!(
            "OBSERVER_MISSING: no ProcessIdentity for the grandchild {grandchild}, and the pid \
             probe reads a zombie as alive (#715), so this run has no honest way to decide whether \
             the tree kill worked"
        );
    };

    let survived = !handle.wait_until_gone(REAP_PATIENCE).unwrap_or_else(|_| {
        panic!(
            "HARNESS-BROKE: the wait on the grandchild's identity failed; this run decides nothing \
             about the tree kill"
        )
    });

    if survived {
        // Through the IDENTITY, not through the number. This used to shell out to `taskkill /PID`
        // or `kill -9`, which is safe on Windows -- the held handle reserves the id -- and unsafe on
        // Linux, where a pidfd does not reserve it: between the liveness answer above and the signal
        // the number can be reassigned and the kill lands on a stranger. The crate now owns that
        // asymmetry (Codex, on #703), so this cell no longer has to remember which platform it is
        // on, and it spawns no process to clean one up.
        //
        // Best-effort by design: the verdict is already in `survived`, so a cleanup that fails must
        // not replace it with a panic about the cleanup.
        let _ = handle.terminate();
    }

    // Released only after the reap, because the reap goes through it.
    drop(identity);

    assert!(
        !captured.readers_abandoned,
        "HARNESS-BROKE: reader deadline reached — a descendant still holds the pipe, so this run \
         captured nothing and decides nothing about the tree kill"
    );

    assert!(
        !survived,
        "the grandchild {grandchild} outlived the kill: `Child::kill` ended the direct child and \
         left everything it spawned running, reparented and holding whatever the workspace held. \
         Still alive {:?} after the kill, waited for as an event rather than sampled",
        REAP_PATIENCE
    );
}

/// #727: a fixture that never reports is REFUSED inside the bound, rather than hanging.
///
/// This is the half of the closing criterion that the tree-kill cell above cannot show, because
/// there the fixture always reports. Without it, "the wait is bounded" is a claim about a branch
/// nothing exercises -- and an unreachable bound is a comment, not a guard.
///
/// It is driven at the seam rather than through the fixture: the patience is an argument, so this
/// runs in a quarter of a second instead of buying a twenty-second tax on every gate. The subject
/// is the same function the cell above calls; only the number differs.
///
/// The elapsed-time assertions are deliberately one-sided in strength. The LOWER bound is exact
/// -- `recv_timeout` cannot return early, so returning before the patience would mean the wait is
/// not waiting. The UPPER bound is generous to the point of being uninteresting, because its job
/// is only to tell "bounded" from "hung": `cargo test` has no per-test timeout, so the failure it
/// exists to catch is a suite that never returns, and a cell that is tight here would fail on a
/// loaded gate for the scheduler's reasons rather than for this code's.
#[test]
fn a_fixture_that_never_reports_is_refused_within_the_bound_rather_than_hanging() {
    const PATIENCE: Duration = Duration::from_millis(250);
    // Bound and then never connected to. This is the arrangement: a listener nobody speaks to is
    // exactly the state a fixture that died before spawning its grandchild leaves behind.
    let listener = TcpListener::bind("127.0.0.1:0").expect("the readiness listener binds loopback");

    let started = std::time::Instant::now();
    let reported = wait_for_reported_grandchild(listener, PATIENCE);
    let elapsed = started.elapsed();

    assert!(
        reported.is_none(),
        "nothing connected, so there is no id to report -- {reported:?} would be an id invented \
         by the wait itself"
    );
    assert!(
        elapsed >= PATIENCE,
        "the wait returned in {elapsed:?}, before its own {PATIENCE:?}: a bound that fires early \
         is not a bound, and the tree-kill cell would refuse while its fixture was still starting"
    );
    assert!(
        elapsed < Duration::from_secs(30),
        "the wait took {elapsed:?} against a {PATIENCE:?} patience. It is bounded on paper and \
         not in practice, which is the third colour #703 exists to keep out of the harness"
    );
}
