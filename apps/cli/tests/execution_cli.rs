use std::path::{Path, PathBuf};

use assert_cmd::Command;
use serde_json::Value;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn command() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
}

fn json(output: &[u8]) -> Value {
    serde_json::from_slice(output).unwrap()
}

/// `execution status`'s `data` carries `headSequence` (Milestone 05a Task 2's one shared-surface
/// change: `execution status` and the Public Runtime API's `GET /v1/executions/{id}` now share the
/// exact same `execute()`, and the CLI command gains the field "for free" as a result); `execution
/// start`'s own response does not carry it — `start.rs` is untouched by that task, deliberately, so
/// as not to ripple into `pause.rs`/`resume.rs`/`cancel.rs`, which share `render()` with it and are
/// out of scope until Milestone 05a Task 4. Strips `headSequence` back out so a `status` reply can
/// still be asserted byte-for-byte against a `start` reply; callers assert `headSequence` itself
/// separately at each call site.
/// Every `NodeOutcomeRecorded` in the stream, IN RECORDED ORDER, as `(node, outcome, next_state)`.
///
/// Reads the store directly instead of going through `graph replay`, because the projection is a
/// FOLD: it answers "what state did each node end in" and deliberately forgets the order they were
/// reached in. #80's flagship story is a claim about ORDER — `deploy` ran only AFTER
/// `implementation` succeeded — and no field of the projection can carry that. The reviewer's
/// finding that made this necessary: in the pre-fix world, running the SAME rewritten script,
/// `deploy` also enters `Running` exactly once and also ends `Succeeded`, so both the state and the
/// attempt count are identical either side of the fix. Only the order differs.
/// Every `NodeOutcomeRecorded` in recorded order as `(node, outcome, next_state, actor_id)`.
///
/// #123 needs the ACTOR as well as the order, and it needs it from the raw stream for a reason
/// worth stating: the node-outcome actor is read by essentially nothing in the product. The
/// projection fold's `NodeOutcomeRecorded` arm never touches it; attention never reads an actor at
/// all. Its only consumer is a human reading the stream back — which is exactly why uniform
/// mis-attribution would break no behaviour and fail no other test, and why a guard is the only
/// thing anywhere that would notice. A property whose sole consumer is a reader gets a guard
/// `execution approve --node`, as the existing tests spell it inline.
fn approve(events: &Path, execution: &str, node: &str) {
    let output = command()
        .args([
            "execution",
            "approve",
            "--events",
            events.to_str().unwrap(),
            "--execution",
            execution,
            "--node",
            node,
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

/// The stream head, read from `execution status` — the one surface that carries `headSequence`
/// (`graph replay` does not), so a growth measurement has a source rather than an inference.
fn status_head(events: &Path, execution: &str) -> Option<u64> {
    let output = command()
        .args([
            "execution",
            "status",
            "--events",
            events.to_str().unwrap(),
            "--execution",
            execution,
        ])
        .output()
        .unwrap();
    json(&output.stdout)["data"]["headSequence"].as_u64()
}

/// BECAUSE nothing else defends it, not although.
fn recorded_outcomes_with_actor(events: &Path) -> Vec<(String, String, String, String)> {
    raw_outcomes(events)
}

fn recorded_outcomes(events: &Path) -> Vec<(String, String, String)> {
    raw_outcomes(events)
        .into_iter()
        .map(|(node, outcome, next, _actor)| (node, outcome, next))
        .collect()
}

fn raw_outcomes(events: &Path) -> Vec<(String, String, String, String)> {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

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
    .unwrap();
    let (_stream, history) = repository.read_unique_replay_stream().unwrap();
    history
        .iter()
        .filter_map(|envelope| match &envelope.kind {
            graphhelm_protocols::EventKind::NodeOutcomeRecorded(payload) => Some((
                payload.node_id.to_string(),
                format!("{:?}", payload.outcome),
                format!("{:?}", payload.next_state),
                envelope.actor.id().to_string(),
            )),
            _ => None,
        })
        .collect()
}

/// The position at which `node` ENTERED RUNNING — the one event the attempt counter is derived
/// from (`core/events/src/projection.rs:896-899`: `Started` with `next_state == Running`).
fn entered_running_at(outcomes: &[(String, String, String)], node: &str) -> usize {
    outcomes
        .iter()
        .position(|(recorded, outcome, next)| {
            recorded == node && outcome == "Started" && next == "Running"
        })
        .unwrap_or_else(|| panic!("{node} never entered Running: {outcomes:?}"))
}

fn without_head_sequence(mut data: Value) -> Value {
    if let Some(object) = data.as_object_mut() {
        object.remove("headSequence");
    }
    data
}

/// Both nodes of the two-node fixture graph succeed, so the driver runs it to completion in one
/// pass over `implementation` and one over `deploy`.
fn all_success_fixtures(directory: &Path) -> PathBuf {
    let path = directory.join("fixtures.json");
    std::fs::write(
        &path,
        serde_json::to_vec(&serde_json::json!({
            "nodeOutcomes": {
                "implementation": "success",
                "deploy": "success",
            }
        }))
        .unwrap(),
    )
    .unwrap();
    path
}

/// The plan's Step 1 test: `execution start` on a small graph publishes, starts and drives to
/// quiescence, reporting the final aggregate status and per-state node counts; a following
/// `execution status` replays the same stream independently and must report identical data.
#[test]
fn start_drives_a_two_node_graph_to_completion_and_status_reports_it_independently() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let graph = root().join("examples/graphs/manual-override-deploy.yaml");
    let fixtures = all_success_fixtures(directory.path());

    let start = command()
        .args([
            "execution",
            "start",
            "--file",
            graph.to_str().unwrap(),
            "--events",
            events.to_str().unwrap(),
            "--fixtures",
            fixtures.to_str().unwrap(),
            "--mode",
            "supervised",
            "--execution",
            "exec_override",
        ])
        .output()
        .unwrap();
    assert!(
        start.status.success(),
        "{}",
        String::from_utf8_lossy(&start.stdout)
    );
    let start_value = json(&start.stdout);
    assert_eq!(start_value["ok"], true);
    assert_eq!(start_value["command"], "execution.start");
    assert_eq!(start_value["data"]["executionId"], "exec_override");
    assert_eq!(start_value["data"]["status"], "completed");
    assert_eq!(start_value["data"]["nodeStateCounts"]["succeeded"], 2);
    assert_eq!(
        start_value["data"]["untriagedInterruptions"],
        serde_json::json!([])
    );

    let status = command()
        .args([
            "execution",
            "status",
            "--events",
            events.to_str().unwrap(),
            "--execution",
            "exec_override",
        ])
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "{}",
        String::from_utf8_lossy(&status.stdout)
    );
    let status_value = json(&status.stdout);
    assert_eq!(status_value["ok"], true);
    assert_eq!(status_value["command"], "execution.status");
    assert!(
        status_value["data"]["headSequence"].as_u64().unwrap() > 0,
        "a finished execution's stream must have a positive head: {status_value}"
    );
    assert_eq!(
        without_head_sequence(status_value["data"].clone()),
        start_value["data"],
        "status must independently replay to the same data start reported"
    );
}

/// `execution start` refuses a stream that already has an `execution_started` on it, before the
/// fold would call the second one corrupt.
#[test]
fn start_refuses_a_stream_that_already_has_an_execution_started() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let graph = root().join("examples/graphs/manual-override-deploy.yaml");
    let fixtures = all_success_fixtures(directory.path());

    let arguments = [
        "execution",
        "start",
        "--file",
        graph.to_str().unwrap(),
        "--events",
        events.to_str().unwrap(),
        "--fixtures",
        fixtures.to_str().unwrap(),
        "--mode",
        "supervised",
        "--execution",
        "exec_override",
    ];
    let first = command().args(arguments).output().unwrap();
    assert!(first.status.success());

    let second = command().args(arguments).output().unwrap();
    assert_eq!(second.status.code(), Some(2));
    let value = json(&second.stdout);
    assert_eq!(value["ok"], false);
    assert_eq!(value["diagnostics"][0]["code"], "GHCLI005_EXECUTION_STATE");
}

/// An always-retryable node exercises the redispatch path `ready_set` alone cannot reach (a
/// `Queued`, retry-pending node) and the no-progress bound Task 1 made reachable: three
/// consecutive `RetryableFailure`s block the node at `MAX_IDENTICAL_OUTCOMES`, well short of
/// `MAX_NODE_ATTEMPTS`, and its dependent is left `Ready` (never dispatched, never terminal) so
/// the execution stays open rather than completing.
#[test]
fn start_blocks_a_no_progress_node_and_leaves_its_dependent_ready() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let graph = root().join("examples/graphs/manual-override-deploy.yaml");
    let fixtures_path = directory.path().join("fixtures.json");
    std::fs::write(
        &fixtures_path,
        serde_json::to_vec(&serde_json::json!({
            "nodeOutcomes": { "implementation": "failure" }
        }))
        .unwrap(),
    )
    .unwrap();

    let start = command()
        .args([
            "execution",
            "start",
            "--file",
            graph.to_str().unwrap(),
            "--events",
            events.to_str().unwrap(),
            "--fixtures",
            fixtures_path.to_str().unwrap(),
            "--mode",
            "autopilot",
            "--execution",
            "exec_override",
        ])
        .output()
        .unwrap();
    assert!(
        start.status.success(),
        "{}",
        String::from_utf8_lossy(&start.stdout)
    );
    let start_value = json(&start.stdout);
    assert_eq!(start_value["ok"], true);
    // DELIBERATE INVERSION (M07 Task 6, forced by the blind judge's re-judgement): this
    // assert used to pin `null` here, on the reasoning that only `execution_completed`
    // writes a status. That reasoning was correct about the events and wrong about the
    // operator: a live run reporting `null` in the one field named for the verdict is the
    // F1 defect one level down. `deploy` still never leaves `Ready` and nothing appends
    // `execution_completed` — and the run says so, because it IS running.
    assert_eq!(start_value["data"]["status"], "running");
    assert_eq!(start_value["data"]["nodeStateCounts"]["blocked"], 1);
    assert_eq!(start_value["data"]["nodeStateCounts"]["ready"], 1);
    assert_eq!(
        start_value["data"]["untriagedInterruptions"],
        serde_json::json!([]),
        "blocked by no-progress, not by an unrecovered interruption"
    );

    let status = command()
        .args([
            "execution",
            "status",
            "--events",
            events.to_str().unwrap(),
            "--execution",
            "exec_override",
        ])
        .output()
        .unwrap();
    assert!(status.status.success());
    let status_data = json(&status.stdout)["data"].clone();
    assert!(
        status_data["headSequence"].as_u64().unwrap() > 0,
        "an open execution's stream must still have a positive head: {status_data}"
    );
    assert_eq!(without_head_sequence(status_data), start_value["data"]);
}

// ---------------------------------------------------------------------------
// Shared helpers for `signal`, `approve`, `pause`, `resume`, `cancel`
// ---------------------------------------------------------------------------

fn write_json(directory: &Path, name: &str, value: &serde_json::Value) -> PathBuf {
    let path = directory.join(name);
    std::fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
    path
}

fn fixtures_file(directory: &Path, node_outcomes: serde_json::Value) -> PathBuf {
    write_json(
        directory,
        "fixtures.json",
        &serde_json::json!({ "nodeOutcomes": node_outcomes }),
    )
}

/// Runs `execution start` against the shared two-node fixture graph and returns its `data`,
/// panicking on a non-zero exit so a bad fixture setup fails loudly where it was built rather
/// than at a later, confusing assertion.
fn start(events: &Path, fixtures: &Path, mode: &str, execution: &str) -> Value {
    let graph = root().join("examples/graphs/manual-override-deploy.yaml");
    let output = command()
        .args([
            "execution",
            "start",
            "--file",
            graph.to_str().unwrap(),
            "--events",
            events.to_str().unwrap(),
            "--fixtures",
            fixtures.to_str().unwrap(),
            "--mode",
            mode,
            "--execution",
            execution,
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    json(&output.stdout)["data"].clone()
}

fn status(events: &Path, execution: &str) -> Value {
    let output = command()
        .args([
            "execution",
            "status",
            "--events",
            events.to_str().unwrap(),
            "--execution",
            execution,
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    json(&output.stdout)["data"].clone()
}

/// The full replayed projection — not just the CLI's summary shape `render()` produces — via
/// `graph replay` against the repository's one stream. Used whenever a specific node's exact
/// state (rather than just the aggregate counts `execution status` reports) needs checking.
fn replay_projection(events: &Path) -> Value {
    let output = command()
        .args(["graph", "replay", "--events", events.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    json(&output.stdout)["data"].clone()
}

/// 64 lowercase hex characters — the out-of-band key material `GRAPHHELM_EVENTS_KEY`
/// carries (Milestone 05d Task 6: the signal command refuses to run without a keyring).
const SIGNAL_KEY_HEX: &str = "0101010101010101010101010101010101010101010101010101010101010101";

/// Creates the sealed keyring the signal command's mandatory `--keyring` flag names.
fn signal_keyring(directory: &Path) -> PathBuf {
    let keyring = directory.join("signal-keyring");
    std::fs::create_dir_all(&keyring).unwrap();
    let material: Vec<u8> = vec![1; 32];
    graphhelm_sealed_key_provider::SealedKeyProvider::create(
        &keyring,
        "signal-key",
        graphhelm_events::SecretBytes::new(material),
    )
    .unwrap();
    keyring
}

fn signal_envelope(id: &str, kind: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "source": {"type": "node", "id": "implementation"},
        "type": kind,
        "severity": "high",
        "description": "the deploy stage needs a manual review",
        "evidence": ["exec-1"],
        "emittedAt": "2026-08-13T00:00:00Z"
    })
}

// ---------------------------------------------------------------------------
// Task 4: `signal`
// ---------------------------------------------------------------------------

/// Scenario 1 of the plan's Step 1: a valid signal envelope is admitted, its evidence
/// externalized to `--evidence-out` first, `signals_recorded` incremented (verified
/// independently via `status`, not the signal command's own report), and `decide_mutation`'s
/// verdict reported as `data` — `requires_approval` under Supervised.
#[test]
fn signal_admits_a_valid_envelope_and_reports_the_governance_verdict() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let fixtures = all_success_fixtures(directory.path());
    start(&events, &fixtures, "supervised", "exec_signal_valid");

    let envelope_value = signal_envelope("signal-1", "no_progress");
    let envelope_path = write_json(directory.path(), "signal.json", &envelope_value);
    let evidence_out = directory.path().join("evidence.json");
    let keyring = signal_keyring(directory.path());

    let output = command()
        .args([
            "execution",
            "signal",
            "--events",
            events.to_str().unwrap(),
            "--execution",
            "exec_signal_valid",
            "--signal",
            envelope_path.to_str().unwrap(),
            "--evidence-out",
            evidence_out.to_str().unwrap(),
            "--keyring",
            keyring.to_str().unwrap(),
            "--key-id",
            "signal-key",
        ])
        .env("GRAPHHELM_EVENTS_KEY", SIGNAL_KEY_HEX)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let value = json(&output.stdout);
    assert_eq!(value["ok"], true);
    assert_eq!(value["command"], "execution.signal");
    assert_eq!(value["data"]["decision"], "requires_approval");
    assert_eq!(value["data"]["mayProposeMutation"], true);

    assert!(
        evidence_out.exists(),
        "the admitted signal's evidence must be externalized"
    );
    let written: Value = serde_json::from_slice(&std::fs::read(&evidence_out).unwrap()).unwrap();
    assert_eq!(written["id"], "signal-1");

    assert_eq!(status(&events, "exec_signal_valid")["signalsRecorded"], 1);
}

/// Scenario 2: a garbage envelope is refused as `GHCLI003_SIGNAL_INVALID`, writes no evidence,
/// and records no signal.
#[test]
fn signal_rejects_a_garbage_envelope_without_writing_or_appending() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let fixtures = all_success_fixtures(directory.path());
    start(&events, &fixtures, "supervised", "exec_signal_garbage");

    let envelope_path = write_json(
        directory.path(),
        "garbage.json",
        &serde_json::json!({"not": "a signal"}),
    );
    let evidence_out = directory.path().join("evidence.json");
    let keyring = signal_keyring(directory.path());

    let output = command()
        .args([
            "execution",
            "signal",
            "--events",
            events.to_str().unwrap(),
            "--execution",
            "exec_signal_garbage",
            "--signal",
            envelope_path.to_str().unwrap(),
            "--evidence-out",
            evidence_out.to_str().unwrap(),
            "--keyring",
            keyring.to_str().unwrap(),
            "--key-id",
            "signal-key",
        ])
        .env("GRAPHHELM_EVENTS_KEY", SIGNAL_KEY_HEX)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let value = json(&output.stdout);
    assert_eq!(value["ok"], false);
    assert_eq!(value["diagnostics"][0]["code"], "GHCLI003_SIGNAL_INVALID");
    assert!(
        !evidence_out.exists(),
        "a garbage envelope must not write evidence"
    );
    assert_eq!(status(&events, "exec_signal_garbage")["signalsRecorded"], 0);
}

/// Scenario 3: a schema-valid envelope whose `id` cannot be represented on the wire is refused
/// as `GHCLI004_SIGNAL_UNRECORDABLE`, but its evidence **is** written — preserved even though
/// the record is not — and the diagnostic says so. Nothing is appended.
#[test]
fn signal_preserves_evidence_for_an_unrecordable_identity_and_refuses() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let fixtures = all_success_fixtures(directory.path());
    start(&events, &fixtures, "supervised", "exec_signal_unrecordable");

    let mut envelope_value = signal_envelope("signal-1", "no_progress");
    envelope_value["id"] = serde_json::json!("");
    let envelope_path = write_json(directory.path(), "unrecordable.json", &envelope_value);
    let raw_bytes = std::fs::read(&envelope_path).unwrap();
    let evidence_out = directory.path().join("evidence.json");
    let keyring = signal_keyring(directory.path());

    let output = command()
        .args([
            "execution",
            "signal",
            "--events",
            events.to_str().unwrap(),
            "--execution",
            "exec_signal_unrecordable",
            "--signal",
            envelope_path.to_str().unwrap(),
            "--evidence-out",
            evidence_out.to_str().unwrap(),
            "--keyring",
            keyring.to_str().unwrap(),
            "--key-id",
            "signal-key",
        ])
        .env("GRAPHHELM_EVENTS_KEY", SIGNAL_KEY_HEX)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let value = json(&output.stdout);
    assert_eq!(value["ok"], false);
    assert_eq!(
        value["diagnostics"][0]["code"],
        "GHCLI004_SIGNAL_UNRECORDABLE"
    );
    assert!(
        value["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("preserved"),
        "the diagnostic must say the evidence was preserved: {value}"
    );
    assert!(
        evidence_out.exists(),
        "the envelope must still be written as evidence"
    );
    assert_eq!(std::fs::read(&evidence_out).unwrap(), raw_bytes);
    assert_eq!(
        status(&events, "exec_signal_unrecordable")["signalsRecorded"],
        0
    );
}

/// Milestone 05d Task 6: the envelope now ALSO seals into the encrypted Evidence store —
/// the `signal_recorded` event carries the sealed reference, the store holds the blob, and
/// `envelopeSha256` equals the sealed plaintext's digest (the reference's content digest),
/// binding record and Evidence to the same bytes. `--evidence-out` is kept: operator copy.
#[test]
fn a_signal_envelope_is_sealed_beside_its_record() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let fixtures = all_success_fixtures(directory.path());
    start(&events, &fixtures, "supervised", "exec_signal_sealed");

    let envelope_path = write_json(
        directory.path(),
        "signal.json",
        &signal_envelope("signal-sealed-1", "no_progress"),
    );
    let evidence_out = directory.path().join("evidence.json");
    let keyring = signal_keyring(directory.path());

    let output = command()
        .args([
            "execution",
            "signal",
            "--events",
            events.to_str().unwrap(),
            "--execution",
            "exec_signal_sealed",
            "--signal",
            envelope_path.to_str().unwrap(),
            "--evidence-out",
            evidence_out.to_str().unwrap(),
            "--keyring",
            keyring.to_str().unwrap(),
            "--key-id",
            "signal-key",
        ])
        .env("GRAPHHELM_EVENTS_KEY", SIGNAL_KEY_HEX)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(evidence_out.exists(), "the operator copy is kept");

    struct WallClock;
    impl graphhelm_protocols::Clock for WallClock {
        fn now(&self) -> chrono::DateTime<chrono::Utc> {
            chrono::Utc::now()
        }
    }
    #[derive(Default)]
    struct Ids(AtomicU64);
    impl graphhelm_protocols::IdGenerator for Ids {
        fn next_id(&self, prefix: &'static str) -> String {
            format!("{prefix}-{}", self.0.fetch_add(1, Ordering::SeqCst) + 1)
        }
    }
    let repository = graphhelm_events::LocalEventRepository::open(
        &events,
        Arc::new(WallClock),
        Arc::new(Ids::default()),
    )
    .unwrap();
    let (stream, history) = repository.read_unique_replay_stream().unwrap();
    let scope = stream.scope.clone();
    let recorded = history
        .iter()
        .find_map(|envelope| match &envelope.kind {
            graphhelm_protocols::EventKind::SignalRecorded(record) => Some((envelope, record)),
            _ => None,
        })
        .expect("a signal_recorded event exists");
    let (envelope, record) = recorded;
    assert_eq!(
        envelope.evidence_refs.len(),
        1,
        "the record carries its sealed reference"
    );
    let reference = &envelope.evidence_refs[0];
    assert!(
        repository
            .evidence_exists(&scope, reference.evidence_id())
            .unwrap(),
        "the store holds the sealed envelope"
    );
    assert_eq!(
        reference.content_sha256().as_str(),
        record.envelope_sha256.as_str(),
        "envelopeSha256 equals the sealed plaintext's digest"
    );
}

/// The scenario Step 4's sabotage targets directly: if `--evidence-out` cannot be written,
/// nothing may be appended, because an event whose evidence was not preserved would violate the
/// fail-closed contract. Pointing `--evidence-out` at a directory makes the write fail.
#[test]
fn signal_fails_closed_when_the_evidence_path_is_unwritable() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let fixtures = all_success_fixtures(directory.path());
    start(&events, &fixtures, "supervised", "exec_signal_unwritable");

    let envelope_path = write_json(
        directory.path(),
        "signal.json",
        &signal_envelope("signal-1", "no_progress"),
    );
    let evidence_out = directory.path().join("evidence-directory");
    std::fs::create_dir_all(&evidence_out).unwrap();
    let keyring = signal_keyring(directory.path());

    let output = command()
        .args([
            "execution",
            "signal",
            "--events",
            events.to_str().unwrap(),
            "--execution",
            "exec_signal_unwritable",
            "--signal",
            envelope_path.to_str().unwrap(),
            "--evidence-out",
            evidence_out.to_str().unwrap(),
            "--keyring",
            keyring.to_str().unwrap(),
            "--key-id",
            "signal-key",
        ])
        .env("GRAPHHELM_EVENTS_KEY", SIGNAL_KEY_HEX)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let value = json(&output.stdout);
    assert_eq!(value["ok"], false);
    assert_eq!(value["diagnostics"][0]["code"], "GHCLI005_EXECUTION_STATE");
    assert_eq!(
        status(&events, "exec_signal_unwritable")["signalsRecorded"],
        0,
        "nothing may be appended when evidence was not preserved"
    );
}

// ---------------------------------------------------------------------------
// Task 4: `approve`
// ---------------------------------------------------------------------------

/// Approving a `Blocked` node readies it — the owner's resume path out of a no-progress block —
/// and does not auto-drive (D-020): nothing else changes.
///
/// The plan's Step 3 also asks for a ghost approval test, from "a governance fixture stream".
/// Constructing one requires governance events (`ghost_node_proposed`) the CLI cannot yet
/// produce, so building the fixture stream would mean hand-assembling a hash-chained history
/// through the library inside this test — disproportionate to what it would prove, since
/// `apply_transition`'s `(Ghost, Approved) -> Ready` arm and the fold's ghost-birth handling are
/// already pinned at the library level (`core/execution/src/transition.rs`,
/// `core/events/src/projection.rs`). Ghost approval is exercised there, not here; this test
/// covers the `Blocked` path plus the refusal path below.
#[test]
fn approve_readies_a_blocked_node() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let fixtures = fixtures_file(
        directory.path(),
        serde_json::json!({"implementation": "failure"}),
    );
    let start_data = start(&events, &fixtures, "supervised", "exec_approve_blocked");
    assert_eq!(start_data["nodeStateCounts"]["blocked"], 1);

    let output = command()
        .args([
            "execution",
            "approve",
            "--events",
            events.to_str().unwrap(),
            "--execution",
            "exec_approve_blocked",
            "--node",
            "implementation",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let value = json(&output.stdout);
    assert_eq!(value["ok"], true);
    assert_eq!(value["command"], "execution.approve");

    let projection = replay_projection(&events);
    assert_eq!(projection["nodeStates"]["implementation"], "ready");
    // deploy was already Ready (blocked only by its predecessor) and stays untouched: approve
    // never auto-drives.
    assert_eq!(projection["nodeStates"]["deploy"], "ready");
}

/// Approval must not be a dead end. The final review of this milestone found the driver
/// fabricating a `RetryableFailure` the executor never produced whenever `classify_progress`
/// predicted a bound - which made an approved node re-block forever without the executor ever
/// being consulted again. The driver now always asks the executor; this proves an approved node
/// whose underlying condition is fixed (new fixtures on resume) genuinely runs and succeeds.
#[test]
fn approve_is_not_a_dead_end_once_the_condition_is_fixed() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let failing = fixtures_file(
        directory.path(),
        serde_json::json!({"implementation": "failure"}),
    );
    let start_data = start(&events, &failing, "supervised", "exec_approve_recovers");
    assert_eq!(start_data["nodeStateCounts"]["blocked"], 1);

    let approve = command()
        .args([
            "execution",
            "approve",
            "--events",
            events.to_str().unwrap(),
            "--execution",
            "exec_approve_recovers",
            "--node",
            "implementation",
        ])
        .output()
        .unwrap();
    assert!(approve.status.success());

    let pause_data = pause(&events, "exec_approve_recovers");
    assert_eq!(pause_data["status"], "paused");

    let fixed = write_json(
        directory.path(),
        "fixed.json",
        &serde_json::json!({
            "nodeOutcomes": {"implementation": "success", "deploy": "success"}
        }),
    );
    let resume_data = resume(&events, &fixed, "exec_approve_recovers");
    assert_eq!(
        resume_data["nodeStateCounts"]["succeeded"], 2,
        "the approved node must run for real and succeed: {resume_data}"
    );
    assert_eq!(resume_data["status"], "completed");
}

/// Approving anything that is not `Ghost` or `Blocked` is refused, naming the actual state.
#[test]
fn approve_refuses_a_node_that_is_not_ghost_or_blocked() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let fixtures = all_success_fixtures(directory.path());
    start(&events, &fixtures, "supervised", "exec_approve_refused");

    let output = command()
        .args([
            "execution",
            "approve",
            "--events",
            events.to_str().unwrap(),
            "--execution",
            "exec_approve_refused",
            "--node",
            "implementation",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let value = json(&output.stdout);
    assert_eq!(value["ok"], false);
    assert_eq!(value["diagnostics"][0]["code"], "GHCLI005_EXECUTION_STATE");
    assert!(
        value["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("succeeded"),
        "the message must name the actual state: {value}"
    );
}

// ---------------------------------------------------------------------------
// Task 5: `pause`, `resume`, `cancel`
// ---------------------------------------------------------------------------

fn pause(events: &Path, execution: &str) -> Value {
    let output = command()
        .args([
            "execution",
            "pause",
            "--events",
            events.to_str().unwrap(),
            "--execution",
            execution,
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    json(&output.stdout)["data"].clone()
}

fn resume(events: &Path, fixtures: &Path, execution: &str) -> Value {
    let graph = root().join("examples/graphs/manual-override-deploy.yaml");
    let output = command()
        .args([
            "execution",
            "resume",
            "--file",
            graph.to_str().unwrap(),
            "--events",
            events.to_str().unwrap(),
            "--fixtures",
            fixtures.to_str().unwrap(),
            "--execution",
            execution,
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    json(&output.stdout)["data"].clone()
}

/// The plan's Step 1 story, corrected by #80. `start` leaves `deploy` `Ready`: its predecessor
/// `implementation` blocks on no-progress (Task 3's own third test proves this exact fixture blocks
/// a predecessor and leaves its dependent `Ready`) rather than ever reaching a success-like state.
/// `pause` holds `deploy` `Paused`, naming it in `heldNodes`; `resume` records `Started` for it,
/// which `(Paused, Started) => Queued` puts in the driver's retry chain.
///
/// AND THERE IT STAYS. This test asserted the opposite until #80: it required `deploy` to SUCCEED
/// while `implementation` was still `Blocked` — a `data` edge delivering a payload its source never
/// produced. That was the defect, not the feature. The retry chain was a bare `state == Queued`
/// filter with no edge check, so the one node pause held was the one node that could reach dispatch
/// with its dependency unmet.
///
/// The doc comment this replaces credited `deploy.userOverrideAllowed: true` for the behaviour. No
/// execution-lane consumer of that field exists — the string "override" appears nowhere in
/// `core/execution` or `core/runtime`. Nothing authorised the override; three missing checks
/// produced it, and no event recorded it.
///
/// Getting a blocked release moving is still a supported story and still tested — through the
/// recorded path that exists (`approve` then `resume`, see
/// `approve_is_not_a_dead_end_once_the_condition_is_fixed` and the Task 6 story). What this test
/// now pins is that the UNRECORDED path does not.
#[test]
fn resume_does_not_run_held_work_whose_predecessor_never_finished() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let fixtures = fixtures_file(
        directory.path(),
        serde_json::json!({"implementation": "failure", "deploy": "success"}),
    );
    let start_data = start(&events, &fixtures, "supervised", "exec_pause_resume");
    assert_eq!(start_data["nodeStateCounts"]["blocked"], 1);
    assert_eq!(start_data["nodeStateCounts"]["ready"], 1);

    let pause_data = pause(&events, "exec_pause_resume");
    assert_eq!(pause_data["status"], "paused");
    assert_eq!(pause_data["heldNodes"], serde_json::json!(["deploy"]));
    assert_eq!(pause_data["nodeStateCounts"]["blocked"], 1);
    assert_eq!(pause_data["nodeStateCounts"]["paused"], 1);

    let resume_data = resume(&events, &fixtures, "exec_pause_resume");
    assert_eq!(resume_data["status"], "running");
    assert_eq!(resume_data["nodeStateCounts"]["blocked"], 1);
    // Zero, not absent: `state_counts` zero-fills every bucket on purpose (mod.rs:594 — "absence
    // must be visible as `0`, not inferred from silence"), so asserting null here would fail for
    // a reason that has nothing to do with #80.
    assert_eq!(
        resume_data["nodeStateCounts"]["succeeded"], 0,
        "nothing may succeed here: the only node resume touched depends on one that never \
         finished: {resume_data}"
    );
    // #123 MOVED THIS — the second time this test has moved, for the same reason each time: it
    // asserts WHERE the held node waits, and the answer got more honest. #80 stopped it being
    // dispatched (it waited `queued`); #123 stops it being re-queued at all, because a resume that
    // re-queues a node whose edges are unmet starts a hold/re-hold loop that appends forever. It
    // now waits HELD, which is what it actually is.
    assert_eq!(resume_data["nodeStateCounts"]["queued"], 0);
    assert_eq!(resume_data["nodeStateCounts"]["paused"], 1);

    let projection = replay_projection(&events);
    assert_eq!(
        projection["nodeStates"]["deploy"], "paused",
        "deploy is in the retry chain but edge-gated, so it waits rather than running on an \
         input that never arrived"
    );
    assert_eq!(projection["nodeStates"]["implementation"], "blocked");
    // Present and zero, which is the sharpest form this assertion has. `deploy` HAS recorded
    // outcomes (`Paused`, then `Started`), so the fold creates its entry
    // (`projection.rs:896`) — but attempts count entries into `Running`, not reports
    // (`projection.rs:892-895`). So `0` says exactly "the driver was told about this node twice
    // and never once ran it", which a state assertion alone would not: `Queued` is also where a
    // node lands after running and being returned to the queue.
    assert_eq!(
        projection["nodeAttempts"]["deploy"], 0,
        "deploy must never have entered Running at all"
    );
}

/// `pause` states its own precondition before the fold would call a second pause corrupt: refused
/// unless the aggregate status is `None` or `Running`.
#[test]
fn pause_refuses_a_second_time_once_already_paused() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let fixtures = fixtures_file(
        directory.path(),
        serde_json::json!({"implementation": "failure"}),
    );
    start(&events, &fixtures, "supervised", "exec_pause_twice");

    let first = command()
        .args([
            "execution",
            "pause",
            "--events",
            events.to_str().unwrap(),
            "--execution",
            "exec_pause_twice",
        ])
        .output()
        .unwrap();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stdout)
    );

    let second = command()
        .args([
            "execution",
            "pause",
            "--events",
            events.to_str().unwrap(),
            "--execution",
            "exec_pause_twice",
        ])
        .output()
        .unwrap();
    assert_eq!(second.status.code(), Some(2));
    let value = json(&second.stdout);
    assert_eq!(value["ok"], false);
    assert_eq!(value["diagnostics"][0]["code"], "GHCLI005_EXECUTION_STATE");
}

/// `cancel` on a fresh, still-open execution cancels every non-terminal node then completes the
/// execution as `Cancelled`; a second `cancel` is refused, already terminal.
#[test]
fn cancel_terminates_every_non_terminal_node_then_refuses_a_second_call() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    // No fixtures at all: `implementation` dispatches straight to `WaitingInput` (an absent
    // fixture means nobody said what this node does), and `deploy` never leaves `Ready` because
    // its predecessor never reaches a success-like state. Both are non-terminal.
    let fixtures = fixtures_file(directory.path(), serde_json::json!({}));
    let start_data = start(&events, &fixtures, "supervised", "exec_cancel");
    assert_eq!(start_data["nodeStateCounts"]["waiting_input"], 1);
    assert_eq!(start_data["nodeStateCounts"]["ready"], 1);

    let first = command()
        .args([
            "execution",
            "cancel",
            "--events",
            events.to_str().unwrap(),
            "--execution",
            "exec_cancel",
        ])
        .output()
        .unwrap();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stdout)
    );
    let value = json(&first.stdout);
    assert_eq!(value["ok"], true);
    assert_eq!(value["data"]["status"], "cancelled");
    assert_eq!(value["data"]["nodeStateCounts"]["cancelled"], 2);

    let projection = replay_projection(&events);
    assert_eq!(projection["nodeStates"]["implementation"], "cancelled");
    assert_eq!(projection["nodeStates"]["deploy"], "cancelled");

    let second = command()
        .args([
            "execution",
            "cancel",
            "--events",
            events.to_str().unwrap(),
            "--execution",
            "exec_cancel",
        ])
        .output()
        .unwrap();
    assert_eq!(second.status.code(), Some(2));
    let second_value = json(&second.stdout);
    assert_eq!(second_value["ok"], false);
    assert_eq!(
        second_value["diagnostics"][0]["code"],
        "GHCLI005_EXECUTION_STATE"
    );
}

/// The sabotage 04e named: resume must re-dispatch exactly the nodes the pause held (`Paused`),
/// never a node that is legitimately still waiting on something that has not changed.
/// `implementation`'s `unknown` fixture maps to `NeedsInput`, landing it in `WaitingInput`, which
/// `pause` does not touch (only `Ready`/`Queued` pause). `deploy` — held `Paused` — is the only
/// node resume may re-dispatch; `nodeAttempts.implementation` staying at 1 is the direct measure
/// that it was not.
///
/// #80 sharpened what "may re-dispatch" means and this test kept its own point either way. Resume
/// re-QUEUES `deploy`; whether it then RUNS is the driver's decision, and the driver now checks
/// edges. `implementation` is `WaitingInput`, which never satisfies a dependent, so `deploy` waits.
/// The two `nodeAttempts` assertions below now measure the same property from both ends: the
/// waiting node was not redispatched (1, unchanged), and the held node was never dispatched at all
/// (0). Before the gate the second one was 1 and `deploy` read `succeeded`.
#[test]
fn resume_never_redispatches_a_waiting_node() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let fixtures = fixtures_file(
        directory.path(),
        serde_json::json!({"implementation": "unknown", "deploy": "success"}),
    );
    let start_data = start(&events, &fixtures, "supervised", "exec_resume_waiting");
    assert_eq!(start_data["nodeStateCounts"]["waiting_input"], 1);
    assert_eq!(start_data["nodeStateCounts"]["ready"], 1);

    let pause_data = pause(&events, "exec_resume_waiting");
    assert_eq!(pause_data["heldNodes"], serde_json::json!(["deploy"]));

    let resume_data = resume(&events, &fixtures, "exec_resume_waiting");
    assert_eq!(resume_data["nodeStateCounts"]["succeeded"], 0);
    assert_eq!(resume_data["nodeStateCounts"]["waiting_input"], 1);

    let projection = replay_projection(&events);
    // #80 then #123: `implementation` is `WaitingInput`, which does not satisfy a dependent, so
    // `deploy` neither runs nor is re-queued. Before #80's gate this read `succeeded` — it ran on
    // an input that never arrived. Between #80 and #123 it read `queued` — held in a retry chain
    // it could never leave, and re-held by every later pause, which is the churn #123 removes. It
    // now reads `paused`, the state that matches the fact.
    assert_eq!(projection["nodeStates"]["deploy"], "paused");
    assert_eq!(projection["nodeAttempts"]["deploy"], 0);
    // Unchanged, and still this test's own point: the WAITING node was never redispatched.
    assert_eq!(projection["nodeStates"]["implementation"], "waiting_input");
    assert_eq!(projection["nodeAttempts"]["implementation"], 1);
}

// ---------------------------------------------------------------------------
// Task 6: the operator story, replayed end to end
// ---------------------------------------------------------------------------

/// Asserts `stdout` is exactly one JSON object carrying the standard envelope's four keys — `ok`,
/// `command`, `data`, `diagnostics` — and nothing else. `serde_json::from_slice` already refuses
/// trailing content after the parsed value (`Deserializer::end`), so a successful parse plus this
/// exact key-set check is the plan's "JSON only, nothing else on stdout" in full.
fn assert_envelope(stdout: &[u8]) -> Value {
    let value: Value = serde_json::from_slice(stdout).unwrap_or_else(|error| {
        panic!(
            "stdout must be a single JSON value ({error}): {:?}",
            String::from_utf8_lossy(stdout)
        )
    });
    let object = value
        .as_object()
        .unwrap_or_else(|| panic!("stdout must be a JSON object: {value}"));
    let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec!["command", "data", "diagnostics", "ok"],
        "the envelope must carry exactly ok, command, data, diagnostics: {value}"
    );
    assert!(object["ok"].is_boolean(), "ok must be a boolean: {value}");
    assert!(
        object["command"].is_string(),
        "command must be a string: {value}"
    );
    assert!(
        object["diagnostics"].is_array(),
        "diagnostics must be an array: {value}"
    );
    value
}

/// The plan's Task 6 story, driven entirely through the compiled binary. `start` (Supervised)
/// leaves `implementation` `Blocked` at the no-progress bound and `deploy` `Ready`-but-undispatched.
/// `signal` records a governance-relevant envelope and reports `requires_approval` under Supervised.
/// `pause` holds `deploy`. `approve` clears the blocked predecessor. `resume` drives, and the whole
/// graph finishes. Every command's stdout is checked against the bare four-key envelope, and two
/// independent `graph replay` runs over the finished stream are asserted byte-identical — the
/// operator-visible form of the milestone's replay guarantee.
///
/// #80 CHANGED THE MIDDLE OF THIS STORY AND KEPT ITS POINT. It used to have no `approve` step:
/// `pause` held `deploy`, `resume` force-dispatched it, and the release "shipped" while
/// `implementation` was still `Blocked` — a `data` edge delivering a payload its source never
/// produced, with no event naming the override and no actor attached to it. The story's doc comment
/// credited `deploy.userOverrideAllowed: true` for that, "the scenario this graph is named for". No
/// execution-lane consumer of that field exists; the string "override" appears nowhere in
/// `core/execution` or `core/runtime`. Three missing checks produced the behaviour and a field name
/// explained it after the fact.
///
/// So the story now tells the same thing — AN OPERATOR GETS A BLOCKED RELEASE MOVING — through the
/// mechanism the product actually has, and every step of it is recorded with an actor: approve the
/// blocked node, fix what broke it, resume. That is a better story than the one it replaces,
/// because the old one could not distinguish a release the operator authorised from one that
/// escaped.
///
/// The FIXED fixtures at resume are load-bearing, not convenience. Approving a node does not change
/// why it failed; redispatching it against the original `failure` fixture would simply re-block it.
/// The operator's real act is "I fixed the cause and re-ran", and the second fixtures file is that
/// act — the same shape `approve_is_not_a_dead_end_once_the_condition_is_fixed` established.
///
/// Ordering note: `approve` runs INSIDE the pause, which is legal because `approve` gates on the
/// node's own state (`Ghost`/`Blocked`) and never on the aggregate. That keeps `heldNodes` at
/// exactly `["deploy"]`, so the pause assertions still say what they always said.
#[test]
fn the_operator_story_runs_end_to_end_and_replays_byte_identical() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let graph = root().join("examples/graphs/manual-override-deploy.yaml");
    let fixtures = fixtures_file(
        directory.path(),
        serde_json::json!({"implementation": "failure", "deploy": "success"}),
    );
    let execution = "exec_operator_story";

    // start
    let start_output = command()
        .args([
            "execution",
            "start",
            "--file",
            graph.to_str().unwrap(),
            "--events",
            events.to_str().unwrap(),
            "--fixtures",
            fixtures.to_str().unwrap(),
            "--mode",
            "supervised",
            "--execution",
            execution,
        ])
        .output()
        .unwrap();
    assert!(
        start_output.status.success(),
        "{}",
        String::from_utf8_lossy(&start_output.stdout)
    );
    let start_value = assert_envelope(&start_output.stdout);
    assert_eq!(start_value["command"], "execution.start");
    // M07 Task 6: a live run says so (see the inversion above); it no longer reports `null`
    // in the one field an operator reads to decide whether to go back to sleep.
    assert_eq!(start_value["data"]["status"], "running");
    assert_eq!(start_value["data"]["nodeStateCounts"]["blocked"], 1);
    assert_eq!(start_value["data"]["nodeStateCounts"]["ready"], 1);

    // signal — Supervised admits it and reports that it requires approval, without touching any
    // node state (signal-to-draft translation is undesigned; the plan's own boundary).
    let envelope_path = write_json(
        directory.path(),
        "signal.json",
        &signal_envelope("story-signal-1", "no_progress"),
    );
    let evidence_out = directory.path().join("evidence.json");
    let keyring = signal_keyring(directory.path());
    let signal_output = command()
        .args([
            "execution",
            "signal",
            "--events",
            events.to_str().unwrap(),
            "--execution",
            execution,
            "--signal",
            envelope_path.to_str().unwrap(),
            "--evidence-out",
            evidence_out.to_str().unwrap(),
            "--keyring",
            keyring.to_str().unwrap(),
            "--key-id",
            "signal-key",
        ])
        .env("GRAPHHELM_EVENTS_KEY", SIGNAL_KEY_HEX)
        .output()
        .unwrap();
    assert!(
        signal_output.status.success(),
        "{}",
        String::from_utf8_lossy(&signal_output.stdout)
    );
    let signal_value = assert_envelope(&signal_output.stdout);
    assert_eq!(signal_value["command"], "execution.signal");
    assert_eq!(signal_value["data"]["decision"], "requires_approval");
    assert_eq!(signal_value["data"]["mayProposeMutation"], true);
    assert!(
        evidence_out.exists(),
        "the admitted signal's evidence must be externalized"
    );

    // pause — holds `deploy`, the only Ready/Queued node; `implementation` is Blocked, not held.
    let pause_output = command()
        .args([
            "execution",
            "pause",
            "--events",
            events.to_str().unwrap(),
            "--execution",
            execution,
        ])
        .output()
        .unwrap();
    assert!(
        pause_output.status.success(),
        "{}",
        String::from_utf8_lossy(&pause_output.stdout)
    );
    let pause_value = assert_envelope(&pause_output.stdout);
    assert_eq!(pause_value["command"], "execution.pause");
    assert_eq!(pause_value["data"]["status"], "paused");
    assert_eq!(
        pause_value["data"]["heldNodes"],
        serde_json::json!(["deploy"])
    );
    assert_eq!(
        pause_value["data"]["signalsRecorded"], 1,
        "the signal recorded before the pause must still be visible in it"
    );

    // Read before the operator intervenes, so the redispatch assertion at the end compares against
    // a measured baseline instead of a hardcoded retry count.
    let attempts_at_pause = replay_projection(&events)["nodeAttempts"]["implementation"].clone();

    // approve — the operator clears the blocked predecessor. `(Blocked, Approved) => Ready`
    // (transition.rs:79). This is the step #80 revealed the story was missing: without it, `deploy`
    // used to run anyway, on an input `implementation` never produced. `approve` gates on the NODE
    // state alone, never on the aggregate, so a paused execution approves fine — and it does not
    // drive, which is why `resume` below is still the step that makes anything run.
    let approve_output = command()
        .args([
            "execution",
            "approve",
            "--events",
            events.to_str().unwrap(),
            "--execution",
            execution,
            "--node",
            "implementation",
        ])
        .output()
        .unwrap();
    assert!(
        approve_output.status.success(),
        "{}",
        String::from_utf8_lossy(&approve_output.stdout)
    );
    let approve_value = assert_envelope(&approve_output.stdout);
    assert_eq!(approve_value["command"], "execution.approve");
    assert_eq!(
        approve_value["data"]["nodeStateCounts"]["ready"], 1,
        "approve returns implementation to Ready without dispatching it: {approve_value}"
    );
    assert_eq!(
        approve_value["data"]["status"], "paused",
        "approving inside a pause must not resume the execution: {approve_value}"
    );

    // resume — the only command besides `start` that drives (resume.rs:87). `implementation` is
    // Ready and rootless, so it dispatches first; its `Succeeded` satisfies `deploy`'s data edge,
    // and `deploy` — queued by the `Started` this resume recorded — dispatches on a later pass of
    // the same drive. Both finish, so the aggregate completes.
    //
    // The FIXED fixtures are the honest half of the story: the operator did not merely approve the
    // failure, they fixed what caused it and re-ran. Approving alone would redispatch
    // `implementation` into the same `failure` and re-block it. Same shape as
    // `approve_is_not_a_dead_end_once_the_condition_is_fixed`.
    let fixed = write_json(
        directory.path(),
        "fixed.json",
        &serde_json::json!({
            "nodeOutcomes": {"implementation": "success", "deploy": "success"}
        }),
    );
    let resume_output = command()
        .args([
            "execution",
            "resume",
            "--file",
            graph.to_str().unwrap(),
            "--events",
            events.to_str().unwrap(),
            "--fixtures",
            fixed.to_str().unwrap(),
            "--execution",
            execution,
        ])
        .output()
        .unwrap();
    assert!(
        resume_output.status.success(),
        "{}",
        String::from_utf8_lossy(&resume_output.stdout)
    );
    let resume_value = assert_envelope(&resume_output.stdout);
    assert_eq!(resume_value["command"], "execution.resume");
    assert_eq!(
        resume_value["data"]["status"], "completed",
        "the release the operator unblocked must actually ship: {resume_value}"
    );
    assert_eq!(
        resume_value["data"]["nodeStateCounts"]["succeeded"], 2,
        "both nodes run: implementation because it was approved and fixed, deploy because \
         implementation's success finally satisfied its edge: {resume_value}"
    );
    assert_eq!(resume_value["data"]["nodeStateCounts"]["blocked"], 0);
    assert_eq!(resume_value["data"]["nodeStateCounts"]["queued"], 0);
    assert_eq!(resume_value["data"]["signalsRecorded"], 1);

    // The story's own point, MEASURED AS ORDER, which is the only thing that separates this run
    // from the pre-#80 one.
    //
    // The obvious assertions do not discriminate, and the reviewer caught me shipping one that
    // did not. Run this exact script against the UNFIXED code: `deploy` still enters `Running`
    // exactly once, still ends `Succeeded`, and `implementation` still succeeds — because this
    // script gives `implementation` a route to success, the end state is the same on both sides
    // of the fix. Final states match. Attempt counts match. What differs is WHEN `deploy` ran:
    // unfixed, the ungated retry chain could dispatch it before `implementation` ever succeeded;
    // fixed, its edge cannot release until that success is recorded.
    //
    // So the guard reads the raw stream, where order survives, rather than the projection, which
    // folds it away.
    //
    // WHAT THE RED RESTS ON, stated so nobody has to re-derive it. Removing the gate makes this
    // assertion fail because `deploy` is then dispatched BEFORE `implementation` succeeds — and
    // that is DETERMINED, not scheduling luck. `dispatch_plan` is attempt-fair (fewer attempts
    // first, lexicographic only as a tiebreak: `dispatch.rs`), and at the resume drive's first
    // pass `deploy` has 0 attempts while `implementation` has already spent at least one failing
    // at start. So `deploy` sorts first on the attempt key, and with this graph's
    // `maxParallelModelCalls: 1` it is dispatched alone in that pass. The tiebreak never runs, so
    // the red does NOT depend on "deploy" sorting before "implementation" alphabetically.
    //
    // The red SURVIVES any attempt-count change, because `deploy`'s count at this point is 0 and 0
    // is minimal — it cannot lose the attempt key. Equal counts do not defeat it either: the
    // tiebreak is lexicographic and "deploy" < "implementation", so `deploy` still leads.
    //
    // THE ONE THING THAT WOULD DEFEAT IT IS A RENAME. If both nodes ever sit on the same attempt
    // key AND the predecessor is renamed to sort before "deploy" (or "deploy" renamed to sort
    // after it), the ungated world would dispatch the predecessor first, it would succeed, and
    // this assertion would pass without the gate — green for a reason it does not name. **The two
    // node ids in `examples/graphs/manual-override-deploy.yaml` are load-bearing for dispatch
    // order here, not merely labels.**
    //
    // The order-INDEPENDENT guards are the two smaller tests, where `implementation` never
    // succeeds at all and `deploy` therefore cannot run under any dispatch order or naming.
    let outcomes = recorded_outcomes(&events);
    let deploy_ran = entered_running_at(&outcomes, "deploy");
    let implementation_succeeded = outcomes
        .iter()
        .position(|(node, _, next)| node == "implementation" && next == "Succeeded")
        .expect("implementation must reach Succeeded in this story");
    assert!(
        deploy_ran > implementation_succeeded,
        "deploy must not enter Running until implementation has SUCCEEDED — it ran at {deploy_ran}, \
         implementation succeeded at {implementation_succeeded}: {outcomes:?}"
    );

    let ordered = replay_projection(&events);
    assert_eq!(ordered["nodeStates"]["implementation"], "succeeded");
    assert_eq!(ordered["nodeStates"]["deploy"], "succeeded");
    // Kept as a supporting fact, NOT as the discriminator — it holds identically without the fix.
    // It rules out a different failure (deploy thrashing through several attempts), which the
    // order assertion above does not cover.
    assert_eq!(ordered["nodeAttempts"]["deploy"], 1);
    // Measured against its own earlier value rather than asserted as a constant: how many times
    // the driver retries `implementation` before the no-progress bound blocks it is the retry
    // policy's business, not this story's, and pinning a number here would make this test fail on
    // a change it is not about. What the story claims is only that approving made it run AGAIN.
    let attempts_before = attempts_at_pause
        .as_u64()
        .unwrap_or_else(|| panic!("implementation must have a recorded attempt count at pause"));
    let attempts_after = ordered["nodeAttempts"]["implementation"]
        .as_u64()
        .unwrap_or_else(|| panic!("implementation must have a recorded attempt count at the end"));
    assert!(
        attempts_after > attempts_before,
        "approve must actually redispatch implementation, not merely relabel it: \
         {attempts_before} attempts at pause, {attempts_after} at completion"
    );

    // completion, replayed twice: the finished stream — the signal recorded, `implementation`
    // blocked then approved and rerun to success, `deploy` held then released by that success —
    // must replay to byte-identical output both times, independent of any in-process state, since
    // each invocation is its own fresh process.
    let replay_args = ["graph", "replay", "--events", events.to_str().unwrap()];
    let replay_first = command().args(replay_args).output().unwrap();
    assert!(
        replay_first.status.success(),
        "{}",
        String::from_utf8_lossy(&replay_first.stdout)
    );
    let replay_first_value = assert_envelope(&replay_first.stdout);
    assert_eq!(replay_first_value["command"], "graph.replay");
    assert_eq!(
        replay_first_value["data"]["nodeStates"]["deploy"],
        "succeeded"
    );
    assert_eq!(
        replay_first_value["data"]["nodeStates"]["implementation"],
        "succeeded"
    );
    assert_eq!(replay_first_value["data"]["signalsRecorded"], 1);

    let replay_second = command().args(replay_args).output().unwrap();
    assert!(
        replay_second.status.success(),
        "{}",
        String::from_utf8_lossy(&replay_second.stdout)
    );
    assert_envelope(&replay_second.stdout);

    assert_eq!(
        replay_first.stdout, replay_second.stdout,
        "two independent replays of the same finished stream must be byte-identical"
    );
}

/// Milestone 05d Task 7 (the M04 ledger's resume file-trust seam): resume derives the supplied
/// file's content hash exactly as start does and refuses a mismatch against the published
/// `current_graph` BEFORE any recovery append — the store is untouched by a refused resume.
#[test]
fn resume_refuses_a_graph_file_that_does_not_match_the_started_hash() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let fixtures = fixtures_file(
        directory.path(),
        serde_json::json!({"implementation": "failure", "deploy": "success"}),
    );
    start(&events, &fixtures, "supervised", "exec_resume_hash");
    pause(&events, "exec_resume_hash");
    let head_before = status(&events, "exec_resume_hash")["headSequence"].clone();

    // Tamper one byte of a node's objective in a COPY of the graph file.
    let pristine = root().join("examples/graphs/manual-override-deploy.yaml");
    let tampered_text =
        std::fs::read_to_string(&pristine)
            .unwrap()
            .replacen("objective:", "objective: X", 1);
    let tampered = directory.path().join("tampered.yaml");
    std::fs::write(&tampered, tampered_text).unwrap();

    let refused = command()
        .args([
            "execution",
            "resume",
            "--file",
            tampered.to_str().unwrap(),
            "--events",
            events.to_str().unwrap(),
            "--fixtures",
            fixtures.to_str().unwrap(),
            "--execution",
            "exec_resume_hash",
        ])
        .output()
        .unwrap();
    assert!(!refused.status.success(), "a tampered graph must refuse");
    let reply = json(&refused.stdout);
    assert_eq!(reply["diagnostics"][0]["code"], "GHCLI005_EXECUTION_STATE");
    assert_eq!(reply["diagnostics"][0]["path"], "/execution/graph");

    // BEFORE any recovery append: the head did not move under the refusal.
    let head_after = status(&events, "exec_resume_hash")["headSequence"].clone();
    assert_eq!(head_before, head_after, "a refused resume appends nothing");

    // The pristine file still resumes.
    resume(&events, &fixtures, "exec_resume_hash");
}

/// Milestone 05f Task 5: `status --html` writes the monitor page as a frozen incident
/// snapshot — no refresh tag, no script, node table present — while the JSON envelope
/// still prints to stdout unchanged. The snapshot IS the live renderer (the byte-equality
/// with the live page is pinned at the unit level in `serve/monitor.rs`); this test pins
/// the CLI surface: flag, file, and the untouched stdout contract.
#[test]
fn status_html_writes_a_frozen_snapshot_and_keeps_the_envelope() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let graph = root().join("examples/graphs/manual-override-deploy.yaml");
    let fixtures = all_success_fixtures(directory.path());

    command()
        .args([
            "execution",
            "start",
            "--file",
            graph.to_str().unwrap(),
            "--events",
            events.to_str().unwrap(),
            "--fixtures",
            fixtures.to_str().unwrap(),
            "--mode",
            "autopilot",
        ])
        .assert()
        .success();

    let html = directory.path().join("incident.html");
    let output = command()
        .args([
            "execution",
            "status",
            "--events",
            events.to_str().unwrap(),
            "--html",
            html.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let envelope = json(&output.stdout);
    assert_eq!(envelope["ok"], true, "{envelope}");
    assert_eq!(envelope["command"], "execution.status");
    assert!(
        envelope["data"]["headSequence"].as_u64().unwrap() > 0,
        "the stdout contract is untouched: {envelope}"
    );

    let page = std::fs::read_to_string(&html).unwrap();
    assert!(!page.contains("http-equiv=\"refresh\""), "frozen: {page}");
    assert!(!page.to_ascii_lowercase().contains("<script"), "{page}");
    assert!(page.contains("<h2>nodes</h2>"), "{page}");

    // A directory as --html target is a refusal, not a panic and not a silent skip.
    let refused = command()
        .args([
            "execution",
            "status",
            "--events",
            events.to_str().unwrap(),
            "--html",
            directory.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!refused.status.success());
    let refusal = json(&refused.stdout);
    assert_eq!(refusal["ok"], false, "{refusal}");
}

/// Every event the journal holds, as `(batch index, type, data)`.
///
/// The batch index is the point: the local store writes one line per ATOMIC append, so two
/// events sharing an index were committed together or not at all. Adjacency in a flat list
/// would only show they happened to land in order.
fn journal_events(events: &std::path::Path) -> Vec<(usize, String, serde_json::Value)> {
    fn find(directory: &std::path::Path, found: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(directory).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                find(&path, found);
            } else if path.file_name().is_some_and(|name| name == "journal.jsonl") {
                found.push(path);
            }
        }
    }
    let mut journals = Vec::new();
    find(events, &mut journals);
    journals.sort();
    assert!(
        !journals.is_empty(),
        "no journal was written under {events:?}"
    );
    journals
        .iter()
        .flat_map(|path| {
            std::fs::read_to_string(path)
                .expect("journal readable")
                .lines()
                .filter(|line| !line.trim().is_empty())
                .map(|line| {
                    serde_json::from_str::<serde_json::Value>(line).expect("journal line is JSON")
                })
                .collect::<Vec<_>>()
        })
        .enumerate()
        .flat_map(|(batch, value)| {
            value["events"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter_map(move |event| {
                    Some((
                        batch,
                        event["kind"]["type"].as_str()?.to_owned(),
                        event["kind"]["data"].clone(),
                    ))
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn start_once(directory: &std::path::Path, graph: &str) -> std::path::PathBuf {
    let events = directory.join("events");
    let graph = root().join(graph);
    let fixtures = all_success_fixtures(directory);
    let output = command()
        .args([
            "execution",
            "start",
            "--file",
            graph.to_str().unwrap(),
            "--events",
            events.to_str().unwrap(),
            "--fixtures",
            fixtures.to_str().unwrap(),
            "--mode",
            "supervised",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "start failed: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    events
}

/// A start records the shape the operator declared, and it is readable without a seal.
///
/// This is the event whose absence left two separate rules mute: the attention seam had no
/// deadline to compare a node's silence against, and M07's wedge rule had no node set to call
/// complete. One missing event, two rules unable to speak.
#[test]
fn a_start_declares_the_shape_of_the_execution() {
    let directory = tempfile::tempdir().unwrap();
    let events = start_once(directory.path(), "examples/graphs/software-feature.yaml");
    let declared = journal_events(&events)
        .into_iter()
        .find(|(_, kind, _)| kind == "execution_form_declared")
        .expect("a start must declare the shape of the execution");

    let node_ids: Vec<&str> = declared.2["nodeIds"]
        .as_array()
        .expect("the declared shape names its node set")
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect();
    // The completeness set: every node the graph declares, so a rule can ask whether
    // everything has been accounted for without a published topology to consult.
    assert!(node_ids.contains(&"plan"), "node set: {node_ids:?}");
    assert!(
        node_ids.contains(&"map_repository"),
        "node set: {node_ids:?}"
    );

    // The deadlines the operator wrote, carried as themselves.
    assert_eq!(declared.2["nodeTimeoutSeconds"]["map_repository"], 900);
    assert_eq!(declared.2["nodeTimeoutSeconds"]["tests"], 1800);
}

/// The declared shape and the start are ONE append, so a history holding the second without the
/// first is an impossible prefix rather than an unlikely one.
///
/// Stated exactly, because the difference matters and cannot be papered over: this is a promise
/// about what this version WRITES. Histories written before this event existed hold an
/// `execution_started` with no declaration, and they must keep replaying — five of them are
/// committed in `docs/acceptance/`. The projection reads their missing declaration as
/// UNDECLARED, never as calm, which is the same answer the seam gives for a budget it was
/// never handed.
#[test]
fn the_declared_shape_and_the_start_are_one_append() {
    let directory = tempfile::tempdir().unwrap();
    let events = start_once(directory.path(), "examples/graphs/software-feature.yaml");
    let all = journal_events(&events);
    let batch_of = |wanted: &str| {
        all.iter()
            .find(|(_, kind, _)| kind == wanted)
            .map(|(batch, _, _)| *batch)
    };
    let started = batch_of("execution_started");
    let declared = batch_of("execution_form_declared");
    assert!(
        started.is_some() && declared.is_some(),
        "a start writes both events or neither: {:?}",
        all.iter().map(|(_, kind, _)| kind).collect::<Vec<_>>()
    );
    // The SAME atomic batch, not merely the next line. The store commits a batch whole or not
    // at all, so there is no instant at which a reader could see the start without the shape.
    assert_eq!(
        declared, started,
        "the declaration must be committed in the same atomic append as the start"
    );
}

/// A node that declares no deadline arrives ABSENT — not zero, not a default.
///
/// Zero is the specific lie this guards: it is what a dropped field looks like once someone
/// "helpfully" defaults it, and it would make every undeclared node permanently overdue, which
/// reads to an operator as a system screaming about work that is perfectly fine.
#[test]
fn a_node_without_a_declared_deadline_arrives_absent_never_zero() {
    let directory = tempfile::tempdir().unwrap();
    let events = start_once(directory.path(), "examples/graphs/software-feature.yaml");
    let declared = journal_events(&events)
        .into_iter()
        .find(|(_, kind, _)| kind == "execution_form_declared")
        .expect("a start must declare the shape of the execution")
        .2;
    let timeouts = declared["nodeTimeoutSeconds"]
        .as_object()
        .expect("the declared deadlines are an object");
    assert!(
        !timeouts.contains_key("plan"),
        "`plan` declares no deadline, so it must have no entry at all: {timeouts:?}"
    );
    // ...and the absence is CONDITIONAL: the same map does carry the nodes that declared one.
    // Without this, an empty map would satisfy the assertion above and hide a total failure.
    assert!(
        timeouts.contains_key("map_repository"),
        "a declared deadline must still arrive: {timeouts:?}"
    );
}

// ---------------------------------------------------------------------------
// #123: the churn the #80 gate created, and the sovereignty of its release.
//
// TRAP GUARDS, WRITTEN BEFORE THE FIX. Two shapes died this way already: a guard whose fixture
// cannot be constructed is telling you the fix is wrong, and it says so before any code exists.
// ---------------------------------------------------------------------------

/// #123's defect, measured as a SLOPE rather than a state.
///
/// After #80's gate, a node held by `pause` and force-`Started` by `resume` lands `Queued` and
/// stays there — correctly, its edges are unmet. But `pause` filters on BARE STATE
/// (`Ready | Queued`, `pause.rs:124`), so the next pause re-holds it, the next resume re-starts
/// it, and the pair appends two events per round forever with no terminal state to stop it.
/// Measured on this exact graph: 2 events/round before the gate, 4 after — and the storm's own
/// runs append ~22% more, with each round costing more than the last because every append
/// lengthens the journal that every open must load.
///
/// The fix is not "append less": it is that a resume must not re-queue a node whose dependencies
/// are still unmet. This asserts the CONSEQUENCE (the loop stops) rather than the mechanism, so it
/// survives any implementation that genuinely stops it.
#[test]
fn repeated_pause_resume_does_not_churn_a_node_whose_edges_are_unmet() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let fixtures = fixtures_file(
        directory.path(),
        serde_json::json!({"implementation": "failure"}),
    );
    start(&events, &fixtures, "supervised", "exec_churn");

    let mut heads = Vec::new();
    for _ in 0..4 {
        pause(&events, "exec_churn");
        resume(&events, &fixtures, "exec_churn");
        heads.push(status_head(&events, "exec_churn").expect("status carries a head sequence"));
    }

    let steps: Vec<u64> = heads.windows(2).map(|w| w[1] - w[0]).collect();
    assert!(
        steps.iter().all(|step| *step <= 2),
        "each pause+resume round on a gated node must append at most the pause/resume pair's own \
         two events; a larger step means the node is being re-queued and re-held forever: \
         heads={heads:?} steps={steps:?}"
    );

    let projection = replay_projection(&events);
    assert_eq!(
        projection["nodeStates"]["deploy"], "paused",
        "a node whose edges are still unmet stays HELD rather than being re-queued: {projection}"
    );
    assert_eq!(projection["nodeAttempts"]["deploy"], 0);
}

/// L's condition 2, both halves in one assertion: the RELEASE is the owner's act, the driver's
/// ordinary hops are not — and the release happens at the right TIME.
///
/// This guard is the only thing anywhere that would notice uniform mis-attribution. The
/// node-outcome actor is read by essentially nothing in the product: the projection fold's
/// `NodeOutcomeRecorded` arm never touches it and attention never reads an actor at all. Its only
/// consumer is a human reading the stream back. A property whose sole consumer is a reader gets a
/// guard BECAUSE nothing else defends it, not although.
#[test]
fn a_gated_nodes_release_is_the_owners_act_and_the_drivers_own_hops_are_not() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let failing = fixtures_file(
        directory.path(),
        serde_json::json!({"implementation": "failure"}),
    );
    start(&events, &failing, "supervised", "exec_release_actor");
    pause(&events, "exec_release_actor");
    approve(&events, "exec_release_actor", "implementation");
    let fixed = write_json(
        directory.path(),
        "fixed.json",
        &serde_json::json!({"nodeOutcomes": {"implementation": "success", "deploy": "success"}}),
    );
    resume(&events, &fixed, "exec_release_actor");

    let outcomes = recorded_outcomes_with_actor(&events);
    let implementation_succeeded = outcomes
        .iter()
        .position(|(node, _, next, _)| node == "implementation" && next == "Succeeded")
        .expect("implementation must succeed in this story");
    let (release_at, release_actor) = outcomes
        .iter()
        .enumerate()
        .find_map(|(index, (node, outcome, next, actor))| {
            (node == "deploy" && outcome == "Started" && next == "Queued")
                .then(|| (index, actor.clone()))
        })
        .expect("deploy must be released into the retry chain exactly once");

    // TIMING: the release cannot precede the success that satisfies the edge.
    assert!(
        release_at > implementation_succeeded,
        "deploy was released at {release_at}, before implementation succeeded at \
         {implementation_succeeded}: {outcomes:?}"
    );
    // HALF ONE: the release is the OWNER's act — D-019 sovereignty, not machinery.
    assert_eq!(
        release_actor, "owner-cli",
        "releasing a held node is the owner's decision and the log must say so: {outcomes:?}"
    );
    // HALF TWO, in the same assertion set so the split is tested rather than assumed: an ORDINARY
    // driver hop in the SAME drive stays the system's. Without this, passing the owner's actor to
    // every hop would satisfy half one while making the whole log wrong.
    // LAST, not first — and this is not a detail. `find` returns `implementation`'s hop from the
    // START drive, which is system-attributed no matter what `resume` does, so the assertion would
    // hold on a build where every resume hop is wrongly owner-attributed. The sabotage L required
    // (pass the owner's actor to every hop) caught exactly that: the guard passed 23/23 while
    // attribution was uniformly wrong. `rev()` picks the hop from the RESUME drive, which is the
    // one under test.
    let ordinary_hop = outcomes
        .iter()
        .rev()
        .find(|(node, outcome, next, _)| {
            node == "implementation" && outcome == "Started" && next == "Running"
        })
        .expect("the resume drive must have dispatched implementation itself");
    assert_eq!(
        ordinary_hop.3, "system-cli",
        "the driver's own dispatch hops stay machinery: {outcomes:?}"
    );
}
