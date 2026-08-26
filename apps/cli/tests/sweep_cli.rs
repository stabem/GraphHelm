//! `execution sweep` on the CLI: it writes a record even when it finds nothing, and it refuses to
//! be asked about the future.
//!
//! THE REFUSAL IS THE HALF WORTH TESTING AT THIS SURFACE. The sweep's own module states why it
//! refuses a future `as_of` before reading, computing or writing anything: a future-dated answer is
//! indistinguishable from a real one in the journal while permanently SPENDING the episodes it
//! touched, so the honest sweep arriving later finds nothing left to raise. That contract lives in
//! `core/events`, and a surface can lose it in one line — by defaulting `--as-of` to something the
//! operator typed, by parsing it against a different clock, or by catching the error and reporting
//! success. Testing the refusal HERE asks whether the contract survived the trip out, which is a
//! different question from whether it exists.
//!
//! An empty verdict still writes the `sweep_performed`, and the first test pins that: without the
//! record, "no exceptions were found" and "no sweep ever ran" are the same absence in the log, and
//! an operator reading the journal cannot tell a clean stream from an unswept one.
//!
//! BOTH TESTS READ THE JOURNAL ITSELF rather than the command's own answer. A command that reports
//! what it MEANT to write agrees with itself whatever it actually wrote; the appended events are
//! the fact under test, so they are read back through the repository directly.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use assert_cmd::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn command() -> Command {
    Command::cargo_bin("graphhelm").expect("the binary under test builds")
}

/// A started execution over the same example graph the other execution tests drive.
///
/// Nothing about a sweep depends on what the nodes did — it needs a stream with an execution on it,
/// and reusing the established arrangement keeps this file's fixtures from becoming a second
/// definition of "a normal stream" that can drift from the first.
fn started_execution(directory: &Path, execution: &str) -> PathBuf {
    let events = directory.join("events");
    let graph = root().join("examples/graphs/manual-override-deploy.yaml");
    let fixtures = directory.join("fixtures.json");
    std::fs::write(
        &fixtures,
        serde_json::to_vec(&serde_json::json!({
            "nodeOutcomes": { "implementation": "success", "deploy": "success" }
        }))
        .expect("fixture serialises"),
    )
    .expect("fixture is writable");

    let output = command()
        .args([
            "execution",
            "start",
            "--file",
            graph.to_str().expect("path is utf-8"),
            "--events",
            events.to_str().expect("path is utf-8"),
            "--fixtures",
            fixtures.to_str().expect("path is utf-8"),
            "--mode",
            "autopilot",
            "--execution",
            execution,
        ])
        .output()
        .expect("the start command runs");
    assert!(
        output.status.success(),
        "the arrangement must succeed before the sweep is measured: {}",
        String::from_utf8_lossy(&output.stdout)
    );

    events
}

/// Every event's wire name, in stream order, read from the repository rather than from any
/// command's summary.
fn event_kinds(events: &Path) -> Vec<String> {
    struct TestClock;
    impl graphhelm_protocols::Clock for TestClock {
        fn now(&self) -> chrono::DateTime<chrono::Utc> {
            chrono::Utc::now()
        }
    }
    #[derive(Default)]
    struct TestIds(AtomicU64);
    impl graphhelm_protocols::IdGenerator for TestIds {
        fn next_id(&self, prefix: &'static str) -> String {
            format!("{prefix}-{}", self.0.fetch_add(1, Ordering::SeqCst) + 1)
        }
    }

    let repository = graphhelm_events::LocalEventRepository::open(
        events,
        Arc::new(TestClock),
        Arc::new(TestIds::default()),
    )
    .expect("the repository the CLI just wrote is openable");
    let (_selection, history) = repository
        .read_unique_replay_stream()
        .expect("the arrangement leaves exactly one stream");
    history
        .iter()
        .map(|envelope| envelope.kind.wire_name().to_owned())
        .collect()
}

#[test]
fn sweep_records_that_it_ran_even_when_it_finds_nothing() {
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let events = started_execution(directory.path(), "sweep-empty");

    let before = event_kinds(&events);

    let output = command()
        .args([
            "execution",
            "sweep",
            "--events",
            events.to_str().expect("path is utf-8"),
            "--execution",
            "sweep-empty",
        ])
        .output()
        .expect("the sweep command runs");
    assert!(
        output.status.success(),
        "a sweep over a clean stream is a success, not an error: {}",
        String::from_utf8_lossy(&output.stdout)
    );

    let after = event_kinds(&events);
    let added: Vec<&str> = after
        .iter()
        .skip(before.len())
        .map(String::as_str)
        .collect();
    assert_eq!(
        added,
        vec!["sweep_performed"],
        "a sweep that found nothing must still leave exactly its own record; \
         before={before:?} after={after:?}"
    );
}

#[test]
fn sweep_refuses_a_future_as_of_and_writes_nothing() {
    let directory = tempfile::tempdir().expect("a temporary directory is available");
    let events = started_execution(directory.path(), "sweep-future");

    let before = event_kinds(&events);

    let output = command()
        .args([
            "execution",
            "sweep",
            "--events",
            events.to_str().expect("path is utf-8"),
            "--execution",
            "sweep-future",
            // Far enough ahead that no skew between this process's clock and the store's can make
            // it legal. A minute would make the test's verdict depend on scheduling.
            "--as-of",
            "2999-01-01T00:00:00Z",
        ])
        .output()
        .expect("the sweep command runs");

    assert!(
        !output.status.success(),
        "a sweep asked about the future must be refused at the surface too: {}",
        String::from_utf8_lossy(&output.stdout)
    );

    // AND THE REFUSAL MUST BE THIS COMMAND'S OWN, which the exit code alone cannot say. MEASURED:
    // before `execution sweep` existed, this test PASSED — clap refused an unknown subcommand,
    // nothing was written, and both assertions below were satisfied by a feature that did not
    // exist. A test green before its subject is built cannot go red when its subject breaks. The
    // envelope names the command that refused, so the assertion asks for that instead.
    let envelope: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
        panic!(
            "the refusal must be this command's own JSON envelope, not an argument-parser error: \
             stdout={:?} stderr={:?}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    assert_eq!(
        envelope
            .pointer("/command")
            .and_then(serde_json::Value::as_str),
        Some("execution.sweep"),
        "the refusal came from something other than the sweep itself: {envelope}"
    );

    // THE REFUSAL AND THE SILENCE ARE TWO CLAIMS AND BOTH ARE ASSERTED. A surface that reported the
    // refusal after appending the record would satisfy the exit code alone, and the damage this
    // contract exists to prevent is the WRITING, not the answer.
    assert_eq!(
        event_kinds(&events),
        before,
        "a refused sweep must leave the journal exactly as it found it"
    );
}
