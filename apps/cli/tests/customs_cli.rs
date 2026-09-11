//! The acting half of M11 on the CLI (#159): a node that parked at `waiting_input` is finished
//! honestly through `execution claim` and `execution clear`, and nothing else finishes it.
//!
//! THE CHAIN IS THE CELL. #159's sealed acceptance is "acting 4/4": park, claim refused with a
//! named code, claim accepted with the downstream still held, a wrong digest rejected with the
//! downstream still held, the right digest cleared and the graph driven to completion. Any link
//! removed leaves a surface that either releases work on testimony alone or never releases it.
//!
//! EVERY JOURNAL ASSERTION READS THE STORE, not the command's own answer, the way `sweep_cli.rs`
//! does: a verb that reports what it MEANT to append agrees with itself whatever it appended.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

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

fn write_json(directory: &Path, name: &str, value: &Value) -> PathBuf {
    let path = directory.join(name);
    std::fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
    path
}

/// `graph replay`'s `data`: the fold's own projection, serialized.
fn replay_projection(events: &Path) -> Value {
    json(&replay_output(events))["data"].clone()
}

/// `graph replay`'s raw stdout, for the byte-identity cell.
fn replay_output(events: &Path) -> Vec<u8> {
    let output = command()
        .args(["graph", "replay", "--events", events.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    output.stdout
}

/// Every event's wire name, in stream order, read from the repository rather than from any
/// command's summary.
fn journal_kinds(events: &Path) -> Vec<String> {
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

fn graph() -> PathBuf {
    root().join("examples/graphs/customs-acting.yaml")
}

/// The arrangement every cell below starts from: `implementation` ran, answered `unknown`
/// (the fixture executor's `NeedsInput`), and parked at `waiting_input`; `release_notes` is
/// scripted to succeed the moment the drive lets it run.
fn start_parked(directory: &Path, execution: &str) -> (PathBuf, PathBuf) {
    let events = directory.join("events");
    let fixtures = write_json(
        directory,
        "fixtures.json",
        &serde_json::json!({
            "nodeOutcomes": { "implementation": "unknown", "release_notes": "success" }
        }),
    );
    let output = command()
        .args([
            "execution",
            "start",
            "--file",
            graph().to_str().unwrap(),
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
    let value = json(&output.stdout);
    assert_eq!(value["ok"], true, "{value}");
    assert_eq!(
        value["data"]["nodeStates"]["implementation"], "waiting_input",
        "PRECONDITION: the node parked"
    );
    (events, fixtures)
}

fn evidence_file(directory: &Path, name: &str, kinds: &[&str]) -> PathBuf {
    let items: Vec<Value> = kinds
        .iter()
        .map(|kind| {
            serde_json::json!({
                "kind": kind,
                "contentHash": format!("sha256:{}", "d".repeat(64)),
                "size": 42,
            })
        })
        .collect();
    write_json(directory, name, &Value::Array(items))
}

fn claim_with_graph(
    graph: &Path,
    events: &Path,
    execution: &str,
    evidence: &Path,
    wait_seq: Option<u64>,
) -> Value {
    let mut args: Vec<String> = vec![
        "execution".into(),
        "claim".into(),
        "--file".into(),
        graph.to_string_lossy().into_owned(),
        "--events".into(),
        events.to_string_lossy().into_owned(),
        "--execution".into(),
        execution.to_owned(),
        "--node".into(),
        "implementation".into(),
        "--evidence".into(),
        evidence.to_string_lossy().into_owned(),
    ];
    if let Some(seq) = wait_seq {
        args.push("--wait-seq".into());
        args.push(seq.to_string());
    }
    let output = command().args(&args).output().unwrap();
    json(&output.stdout)
}

fn claim(events: &Path, execution: &str, evidence: &Path, wait_seq: Option<u64>) -> Value {
    claim_with_graph(&graph(), events, execution, evidence, wait_seq)
}

fn clear_args(events: &Path, fixtures: &Path, execution: &str, claim_seq: u64) -> Vec<String> {
    vec![
        "execution".into(),
        "clear".into(),
        "--file".into(),
        graph().to_string_lossy().into_owned(),
        "--events".into(),
        events.to_string_lossy().into_owned(),
        "--fixtures".into(),
        fixtures.to_string_lossy().into_owned(),
        "--execution".into(),
        execution.to_owned(),
        "--claim-seq".into(),
        claim_seq.to_string(),
    ]
}

/// `execution clear --evidence <bundle>`: the digest is computed by the verb from the bundle
/// the operator holds — which is what a machine replay IS.
fn clear(
    events: &Path,
    fixtures: &Path,
    execution: &str,
    claim_seq: u64,
    evidence: &Path,
) -> Value {
    let mut args = clear_args(events, fixtures, execution, claim_seq);
    args.push("--evidence".into());
    args.push(evidence.to_string_lossy().into_owned());
    let output = command().args(&args).output().unwrap();
    json(&output.stdout)
}

/// `execution clear --manifest-hash <digest>`: a digest computed elsewhere, presented as-is.
fn clear_with_hash(
    events: &Path,
    fixtures: &Path,
    execution: &str,
    claim_seq: u64,
    hash: &str,
) -> Value {
    let mut args = clear_args(events, fixtures, execution, claim_seq);
    args.push("--manifest-hash".into());
    args.push(hash.to_owned());
    let output = command().args(&args).output().unwrap();
    json(&output.stdout)
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
    let value = json(&output.stdout);
    assert_eq!(value["ok"], true, "{value}");
    value["data"].clone()
}

/// The `completion_*` kinds in journal order — the family this slice appends, and nothing else.
fn customs_kinds(events: &Path) -> Vec<String> {
    journal_kinds(events)
        .into_iter()
        .filter(|kind| kind.starts_with("completion_"))
        .collect()
}

/// THE SEALED ACCEPTANCE (#159): acting 4/4 on one surface.
#[test]
fn a_parked_node_is_claimed_cleared_and_the_graph_finishes() {
    let directory = tempfile::tempdir().unwrap();
    let (events, fixtures) = start_parked(directory.path(), "exec-acting");

    // 1. Refused when the budget is unmet — named code, node untouched.
    let short = evidence_file(directory.path(), "short.json", &["diff"]);
    let refused = claim(&events, "exec-acting", &short, None);
    assert_eq!(refused["ok"], true, "{refused}");
    assert_eq!(refused["data"]["claim"]["outcome"], "refused");
    assert_eq!(
        refused["data"]["claim"]["reasonCode"],
        "evidence_budget_unmet"
    );
    assert_eq!(refused["data"]["claim"]["claimSeq"], Value::Null);
    assert_eq!(
        refused["data"]["nodeStates"]["implementation"],
        "waiting_input"
    );

    // 2. Claimed — quarantine visible, the node still parked, downstream not released.
    let full = evidence_file(directory.path(), "full.json", &["test_report"]);
    let claimed = claim(&events, "exec-acting", &full, None);
    assert_eq!(claimed["data"]["claim"]["outcome"], "claimed", "{claimed}");
    let claim_seq = claimed["data"]["claim"]["claimSeq"].as_u64().unwrap();
    assert_eq!(
        claimed["data"]["customs"]["quarantinedNodes"],
        serde_json::json!(["implementation"])
    );
    // The quarantined node renders `waiting_input` with its latest scan at `claimed`, and the
    // claim the operator will have to clear is named by its sequence.
    assert_eq!(
        claimed["data"]["nodeStates"]["implementation"],
        "waiting_input"
    );
    let scans = claimed["data"]["customs"]["nodes"]["implementation"]["scans"]
        .as_array()
        .unwrap();
    assert_eq!(scans.last().unwrap()["stage"], "claimed");
    assert_eq!(
        claimed["data"]["customs"]["nodes"]["implementation"]["openClaim"]["claimSeq"],
        serde_json::json!(claim_seq)
    );
    assert_ne!(claimed["data"]["nodeStates"]["release_notes"], "succeeded");

    // 3. A wrong digest is REJECTED, spends the claim, releases nothing.
    let rejected = clear_with_hash(
        &events,
        &fixtures,
        "exec-acting",
        claim_seq,
        &format!("sha256:{}", "0".repeat(64)),
    );
    assert_eq!(
        rejected["data"]["clearance"]["outcome"], "rejected",
        "{rejected}"
    );
    assert_eq!(rejected["data"]["clearance"]["reasonCode"], "hash_mismatch");
    assert_eq!(
        rejected["data"]["clearance"]["claimSeq"],
        serde_json::json!(claim_seq)
    );
    assert_ne!(rejected["data"]["nodeStates"]["release_notes"], "succeeded");
    assert_eq!(
        rejected["data"]["nodeStates"]["implementation"],
        "waiting_input"
    );
    assert_eq!(
        rejected["data"]["customs"]["quarantinedNodes"],
        serde_json::json!([])
    );

    // 4. Claim again (the wait survived), clear with the bundle → the drive finishes the graph.
    let claimed_again = claim(&events, "exec-acting", &full, None);
    assert_eq!(
        claimed_again["data"]["claim"]["outcome"], "claimed",
        "{claimed_again}"
    );
    let claim_seq = claimed_again["data"]["claim"]["claimSeq"].as_u64().unwrap();
    let cleared = clear(&events, &fixtures, "exec-acting", claim_seq, &full);
    assert_eq!(
        cleared["data"]["clearance"]["outcome"], "cleared",
        "{cleared}"
    );
    assert_eq!(cleared["data"]["clearance"]["reasonCode"], Value::Null);
    assert_eq!(cleared["data"]["nodeStates"]["implementation"], "succeeded");
    assert_eq!(cleared["data"]["nodeStates"]["release_notes"], "succeeded");
    assert_eq!(cleared["data"]["status"], "completed");
    assert_eq!(
        cleared["data"]["customs"]["quarantinedNodes"],
        serde_json::json!([])
    );

    // The journal, read directly: the family in order, and the scan history that replays it.
    assert_eq!(
        customs_kinds(&events),
        [
            "completion_refused",
            "completion_claimed",
            "completion_cleared",
            "completion_claimed",
            "completion_cleared",
        ]
    );
    let replayed = replay_projection(&events);
    let stages: Vec<&str> = replayed["customsScans"]["implementation"]
        .as_array()
        .unwrap()
        .iter()
        .map(|scan| scan["stage"].as_str().unwrap())
        .collect();
    assert_eq!(
        stages,
        [
            "parked", "refused", "claimed", "rejected", "claimed", "cleared"
        ]
    );
    // What `status` reads afterwards is what the verb replied with — one `render()`.
    assert_eq!(
        status(&events, "exec-acting")["customs"],
        cleared["data"]["customs"]
    );
}

/// THE TRAP GUARD on this surface: a claim that names a wait other than the node's open one is
/// REFUSED, never redirected to "whichever wait is open now", and the node stays parked.
///
/// The superseded-wait arrangement itself (a node that parked twice, so the first wait is a
/// `Parked` scan that is no longer open — `stale_rendezvous`) is not constructible from the CLI
/// alone: `execution resume` never re-dispatches a `waiting_input` node, and no verb re-parks
/// one. That cell lives at the verb level, where the journal can be arranged directly:
/// `core/events/tests/customs_verbs.rs::a_claim_naming_a_superseded_wait_is_refused_stale_rendezvous_and_the_node_stays_parked`.
/// This surface pins the other arm of the same decision — a sequence no scan of this node ever
/// parked at (`1`, the `execution_started` envelope) — so a CLI that quietly answered the open
/// wait would fail here whichever of the two codes it lost.
#[test]
fn a_claim_naming_a_superseded_wait_is_refused_and_the_node_stays_parked() {
    let directory = tempfile::tempdir().unwrap();
    let (events, _fixtures) = start_parked(directory.path(), "exec-stale");
    let open_wait = status(&events, "exec-stale")["customs"]["nodes"]["implementation"]["openWait"]
        ["atSequence"]
        .as_u64()
        .unwrap();
    assert_ne!(
        open_wait, 1,
        "PRECONDITION: sequence 1 is not the open wait"
    );

    let full = evidence_file(directory.path(), "full.json", &["test_report"]);
    let refused = claim(&events, "exec-stale", &full, Some(1));
    assert_eq!(refused["ok"], true, "{refused}");
    assert_eq!(refused["data"]["claim"]["outcome"], "refused");
    assert_eq!(refused["data"]["claim"]["reasonCode"], "unknown_wait");
    assert_eq!(refused["data"]["claim"]["waitSeq"], serde_json::json!(1));
    assert_eq!(
        refused["data"]["nodeStates"]["implementation"],
        "waiting_input"
    );
    assert_eq!(
        refused["data"]["customs"]["quarantinedNodes"],
        serde_json::json!([])
    );
    assert_eq!(
        journal_kinds(&events).last().map(String::as_str),
        Some("completion_refused")
    );
    // The open wait is untouched, and a claim that names it still goes through.
    let claimed = claim(&events, "exec-stale", &full, Some(open_wait));
    assert_eq!(claimed["data"]["claim"]["outcome"], "claimed", "{claimed}");
}

/// D1: `countersign` is refused AT THE DOOR — before the store is opened — with a stable
/// diagnostic naming the decision that will supply the signature, and nothing is appended.
#[test]
fn a_countersign_clearance_is_refused_at_the_door_and_appends_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let (events, fixtures) = start_parked(directory.path(), "exec-countersign");
    let full = evidence_file(directory.path(), "full.json", &["test_report"]);
    let claimed = claim(&events, "exec-countersign", &full, None);
    let claim_seq = claimed["data"]["claim"]["claimSeq"].as_u64().unwrap();
    let before = journal_kinds(&events);

    let mut args = clear_args(&events, &fixtures, "exec-countersign", claim_seq);
    args.extend([
        "--verifier".to_owned(),
        "countersign".to_owned(),
        "--manifest-hash".to_owned(),
        format!("sha256:{}", "d".repeat(64)),
    ]);
    let output = command().args(&args).output().unwrap();
    let value = json(&output.stdout);
    assert_eq!(value["ok"], false, "{value}");
    assert_eq!(value["diagnostics"][0]["code"], "GHCLI005_EXECUTION_STATE");
    let message = value["diagnostics"][0]["message"].as_str().unwrap();
    assert!(message.contains("#529"), "{message}");

    assert_eq!(journal_kinds(&events), before, "nothing was appended");
    // The claim is still open: the refusal spent nothing.
    assert_eq!(
        status(&events, "exec-countersign")["customs"]["quarantinedNodes"],
        serde_json::json!(["implementation"])
    );
}

/// D5: a clearance naming a sequence that is not an open claim is refused WITHOUT a journal
/// entry, because the fold reads such a record as `Corrupt`.
#[test]
fn a_clearance_naming_a_sequence_that_is_not_an_open_claim_is_refused_without_a_journal_entry() {
    let directory = tempfile::tempdir().unwrap();
    let (events, fixtures) = start_parked(directory.path(), "exec-no-claim");
    let before = journal_kinds(&events);

    let value = clear_with_hash(
        &events,
        &fixtures,
        "exec-no-claim",
        999,
        &format!("sha256:{}", "d".repeat(64)),
    );
    assert_eq!(value["ok"], false, "{value}");
    assert_eq!(value["diagnostics"][0]["code"], "GHCLI005_EXECUTION_STATE");
    assert_eq!(journal_kinds(&events), before, "nothing was appended");
}

/// D2: both verbs go through the file-trust seam `resume` established — a graph whose content
/// hash is not the one this execution recorded is refused before anything is appended.
#[test]
fn a_claim_against_a_graph_that_is_not_the_one_the_execution_started_from_is_refused() {
    let directory = tempfile::tempdir().unwrap();
    let (events, fixtures) = start_parked(directory.path(), "exec-wrong-graph");
    let before = journal_kinds(&events);
    let other = root().join("examples/graphs/manual-override-deploy.yaml");
    let full = evidence_file(directory.path(), "full.json", &["test_report"]);

    let value = claim_with_graph(&other, &events, "exec-wrong-graph", &full, None);
    assert_eq!(value["ok"], false, "{value}");
    assert_eq!(value["diagnostics"][0]["code"], "GHCLI005_EXECUTION_STATE");
    assert_eq!(journal_kinds(&events), before, "nothing was appended");

    // The same seam on `clear`, with a claim under it so the seam is what refuses.
    let claimed = claim(&events, "exec-wrong-graph", &full, None);
    let claim_seq = claimed["data"]["claim"]["claimSeq"].as_u64().unwrap();
    let before = journal_kinds(&events);
    let mut args = clear_args(&events, &fixtures, "exec-wrong-graph", claim_seq);
    args[3] = other.to_string_lossy().into_owned();
    args.push("--evidence".into());
    args.push(full.to_string_lossy().into_owned());
    let output = command().args(&args).output().unwrap();
    let value = json(&output.stdout);
    assert_eq!(value["ok"], false, "{value}");
    assert_eq!(value["diagnostics"][0]["code"], "GHCLI005_EXECUTION_STATE");
    assert_eq!(journal_kinds(&events), before, "nothing was appended");
}

/// Replay is a pure function of the journal: two replays are byte-identical, and a rejected
/// clearance replays as `refused` every time rather than being re-judged.
#[test]
fn replay_of_the_acting_journal_is_byte_identical_and_a_rejected_clearance_replays_as_rejected() {
    let directory = tempfile::tempdir().unwrap();
    let (events, fixtures) = start_parked(directory.path(), "exec-replay");
    let full = evidence_file(directory.path(), "full.json", &["test_report"]);
    let claimed = claim(&events, "exec-replay", &full, None);
    let claim_seq = claimed["data"]["claim"]["claimSeq"].as_u64().unwrap();
    let rejected = clear_with_hash(
        &events,
        &fixtures,
        "exec-replay",
        claim_seq,
        &format!("sha256:{}", "0".repeat(64)),
    );
    assert_eq!(
        rejected["data"]["clearance"]["outcome"], "rejected",
        "{rejected}"
    );

    let first = replay_output(&events);
    let second = replay_output(&events);
    assert_eq!(first, second, "replay must not read a clock or a map order");
    let replayed: Value = serde_json::from_slice(&first).unwrap();
    let verdict = &replayed["data"]["clearances"][claim_seq.to_string()];
    assert_eq!(verdict["type"], "refused", "{replayed}");
    assert_eq!(verdict["reasonCode"], "hash_mismatch");
    assert_eq!(
        replayed["data"]["nodeStates"]["implementation"],
        "waiting_input"
    );
}
