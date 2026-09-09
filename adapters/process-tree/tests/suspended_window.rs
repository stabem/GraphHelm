//! #878: the window a descendant could escape through, closed and then DETECTED.
//!
//! `create` can only assign a job to a process that already exists, so there is an interval between
//! the spawn and `AssignProcessToJobObject`. A child running in that interval can start descendants,
//! and they are outside the job: `KILL_ON_JOB_CLOSE` never reaches them and `terminate` cannot see
//! them.
//!
//! **Along the documented path the interval carries no running child.** `configure` sets
//! `CREATE_SUSPENDED` and `create` resumes only after the assignment succeeds, so the child executes
//! no instruction before it is contained and has nothing to spawn a descendant with. The race the
//! ticket asks for a cell about cannot be constructed there -- not because it is unlikely, but
//! because the child is not running.
//!
//! **What remains is a caller, and it is now refused rather than documented.** `ResumeThread`
//! returns the thread's PREVIOUS suspend count, which `create` was discarding. Zero means nothing
//! ever suspended it: `configure` was not called, the child has been running since the spawn, and
//! the group `create` would return promises a containment it cannot deliver. It answers
//! `ChildNotSuspended` instead.
//!
//! These cells ask the OPERATING SYSTEM, not the clock. An earlier version spawned a child that
//! wrote a marker file and slept 1.5 seconds before looking for it, with a second cell offered as
//! the proof that 1.5 seconds was long enough. A reviewer pointed out that the two cells are
//! separate tests and can meet different scheduler load, so the second establishes nothing about
//! the first's interval -- and on a loaded gate an unsuspended `cmd.exe` can simply fail to be
//! scheduled in time. The suspend count is the same fact with no wall clock in it.
//!
//! The same review killed a lexical sweep that counted `graphhelm_process_tree::create(` and
//! `configure(` per file and called their pairing a guard. It could not see through
//! `create_process_group`, the wrapper in `adapters/postgres-event-store/src/backup.rs`, whose
//! configuration happens at spawn sites elsewhere in the file -- deleting one of those left the
//! sweep green. A check that cannot observe the property it names is worse than no check, because
//! its green is read as evidence. The refusal below is the containment property enforced where it
//! is actually knowable: at runtime, by the OS.
//!
//! **The refusal is DESTRUCTIVE and a cell says so.** The suspend count is only knowable after
//! the job has taken the process, the job carries `KILL_ON_JOB_CLOSE`, and a process cannot
//! leave a job -- so releasing the handle on the refusal path terminates the child. An earlier
//! comment in `create` promised the opposite, and the cell below did not notice because it
//! called `kill()` without ever asking whether the child was alive. A cleanup call is not an
//! observation.

#![cfg(windows)]

/// A child that outlives `create`, so the assignment succeeds in every world this file constructs
/// and the RESULT, not a dead process, is what each assertion reads.
fn long_lived_child() -> std::process::Command {
    use std::os::windows::process::CommandExt;
    let mut command = std::process::Command::new("cmd");
    // `raw_arg`, not `arg`: Rust quotes for a C-runtime parser and `cmd.exe` does not use one.
    command.arg("/c");
    command.raw_arg("ping -n 30 127.0.0.1 > nul");
    command
}

#[test]
fn a_configured_child_is_suspended_until_the_job_holds_it() {
    let mut command = long_lived_child();
    graphhelm_process_tree::configure(&mut command);
    let child = command.spawn().expect("HARNESS-BROKE: cmd did not spawn");

    let group = graphhelm_process_tree::create(&child);

    // `create` succeeding IS the measurement: it refuses with ChildNotSuspended when the resume
    // finds a previous suspend count of zero, so an Ok here says the OS reported this child as
    // suspended at the moment the job took it.
    assert!(
        group.is_ok(),
        "a CONFIGURED child was not suspended when the job took it, so it had been running since \
         the spawn and anything it started in that interval is outside the job: {:?}",
        group.err()
    );

    let mut group = group.expect("checked above");
    graphhelm_process_tree::close(&mut group);
    let mut child = child;
    let _ = child.kill();
    let _ = child.wait();
}

/// THE OTHER SIDE, and the reason the cell above is not vacuous: the identical command with
/// `configure` NOT called. This is #878's window, and `create` now refuses to paper over it.
#[test]
fn an_unconfigured_child_is_refused_because_it_already_ran() {
    let mut command = long_lived_child();
    // deliberately NOT configured
    let child = command.spawn().expect("HARNESS-BROKE: cmd did not spawn");

    let group = graphhelm_process_tree::create(&child);

    assert_eq!(
        group.err(),
        Some(graphhelm_process_tree::ProcessTreeError::ChildNotSuspended),
        "an UNCONFIGURED child was accepted: the group returned for it promises a containment it \
         cannot deliver, because the child was free to start descendants between the spawn and the \
         assignment and none of them is in the job"
    );

    // THE REFUSAL IS DESTRUCTIVE, asserted rather than left for a caller to discover. The suspend
    // count is only knowable after the job has taken the process, the job carries
    // KILL_ON_JOB_CLOSE, and a process cannot leave a job -- so releasing the handle on this path
    // terminates the child. An earlier comment in `create` promised the opposite, and THIS CELL
    // did not notice, because it called `kill()` without ever asking whether the child was alive.
    // A cleanup call is not an observation (Codex P2 on #1027).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while graphhelm_process_tree::process_is_running(child.id())
        && std::time::Instant::now() < deadline
    {
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    assert!(
        !graphhelm_process_tree::process_is_running(child.id()),
        "the refused child is still RUNNING: this error is documented as destructive because the \
         job already holds the process and KILL_ON_JOB_CLOSE fires when the handle closes -- if it \
         survives, the documentation is wrong and the caller is left an untracked process"
    );

    let mut child = child;
    let _ = child.kill();
    let _ = child.wait();
}

/// The refusal is not a blanket "anything unusual fails". The two cells above differ in exactly one
/// call, and this pins that the distinguishing answer is the SPECIFIC error: a `JobSetup` here
/// would mean the pair measures a job that could not be built rather than a child that was never
/// suspended, and both cells would still look right.
#[test]
fn the_refusal_names_the_suspension_and_not_a_generic_failure() {
    let mut command = long_lived_child();
    let child = command.spawn().expect("HARNESS-BROKE: cmd did not spawn");

    let error = graphhelm_process_tree::create(&child).err();

    assert_ne!(
        error,
        Some(graphhelm_process_tree::ProcessTreeError::JobSetup),
        "the unconfigured child was refused for JobSetup, so the pair above is measuring a job \
         that could not be built rather than a child that was never suspended"
    );
    assert_ne!(
        error,
        Some(graphhelm_process_tree::ProcessTreeError::ProcessResume),
        "the unconfigured child was refused for ProcessResume, which is a failure to resume rather \
         than a report that there was nothing to resume"
    );

    let mut child = child;
    let _ = child.kill();
    let _ = child.wait();
}
