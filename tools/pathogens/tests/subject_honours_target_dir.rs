//! The subject is the binary THIS RUN built, not the one at cargo's default path (#349).
//!
//! `measurable_binary` used to read `<root>/target/debug` unconditionally while the build location
//! is configurable. Under this repository's slot discipline every lane sets `CARGO_TARGET_DIR`, so
//! the hardcoded path was precisely the one binary the run had certainly not refreshed.
//!
//! **Both directions are asserted, and the second is the one that matters.** The failure K measured
//! was a REFUSAL — a stale default-path binary against fresh sources — and a refusal is noisy and
//! gets investigated. The quiet twin is a *fresh* binary sitting at the default path while the run
//! built elsewhere: the instrument then reports a confident green about a product this run never
//! produced. A guard that only pinned the noisy direction would leave the expensive one open.
//!
//! This file is deliberately alone in its own test binary. It mutates `CARGO_TARGET_DIR`, and
//! process-wide environment is not something to share with tests running on other threads — the
//! neighbouring `subject_refusals.rs` already paid for phases that fabricated each other's state.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use pathogens::subject::{SubjectRefusal, measurable_binary};

mod common;

fn binary_name() -> &'static str {
    if cfg!(windows) {
        "graphhelm.exe"
    } else {
        "graphhelm"
    }
}

/// Plants a file where cargo would put the binary under `target_dir`, aged to `mtime`.
fn plant(target_dir: &Path, mtime: SystemTime) -> PathBuf {
    let debug = target_dir.join("debug");
    std::fs::create_dir_all(&debug).expect("the debug directory is creatable");
    let binary = debug.join(binary_name());
    std::fs::write(
        &binary,
        b"not a real binary, and it does not need to be: the subject resolver
        answers on paths and mtimes, never on contents",
    )
    .expect("the stand-in binary is writable");
    filetime::set_file_mtime(&binary, filetime::FileTime::from_system_time(mtime))
        .expect("the stand-in mtime is settable");
    binary
}

#[test]
fn the_subject_is_the_configured_target_dir_in_both_directions() {
    // No `tempfile` here on purpose: the process id keeps each stand-in directory separate. The
    // shared file lock coordinates this test with the separate source-refusal test binary.
    let configured = std::env::temp_dir().join(format!("pathogens-349-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&configured);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    let _timeline_lock = common::SubjectTimelineLock::acquire(root);

    // SAFETY: this test binary contains exactly one test, so nothing else in this process can be
    // reading the environment while it changes. That isolation is why the file exists.
    let restore = std::env::var_os("CARGO_TARGET_DIR");
    unsafe { std::env::set_var("CARGO_TARGET_DIR", &configured) };

    // DIRECTION 1 — fresh where the run actually built: the instrument must MEASURE, and must
    // name the configured path. Before #349 it looked at <root>/target/debug and answered about a
    // file this test never created.
    let fresh = plant(&configured, SystemTime::now() + Duration::from_secs(3600));
    let measured = measurable_binary();
    let fresh_ok = matches!(&measured, Ok(path) if path == &fresh);

    // DIRECTION 2 — the quiet twin. Age the configured binary behind the sources. The instrument
    // must REFUSE, and the refusal must name the CONFIGURED path: if it were still reading the
    // default path it would find whatever happens to be there — commonly something fresh — and
    // report a green it did not earn.
    filetime::set_file_mtime(
        &fresh,
        filetime::FileTime::from_system_time(SystemTime::UNIX_EPOCH + Duration::from_secs(1)),
    )
    .expect("the stand-in mtime is settable");
    let aged = measurable_binary();

    // Restore the environment BEFORE asserting, so a failing assertion cannot leave the variable
    // pointing at a temporary directory for anything that runs afterwards.
    match restore {
        // SAFETY: same single-test isolation as above.
        Some(previous) => unsafe { std::env::set_var("CARGO_TARGET_DIR", previous) },
        None => unsafe { std::env::remove_var("CARGO_TARGET_DIR") },
    }
    let _ = std::fs::remove_dir_all(&configured);

    assert!(
        fresh_ok,
        "the subject must be the binary under CARGO_TARGET_DIR, not the one at cargo's default \
         path: expected {}, got {measured:?}",
        fresh.display()
    );

    match aged {
        Err(SubjectRefusal::Stale { path, .. }) => assert_eq!(
            path, fresh,
            "the refusal must name the CONFIGURED binary; naming another path means the \
             instrument measured a file this run never built"
        ),
        other => panic!(
            "a configured binary older than the sources must be REFUSED, or a fresh binary at the \
             default path would buy a green this run never earned: {other:?}"
        ),
    }
}
