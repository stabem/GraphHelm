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
/// Every event in the repository the CLI just wrote, in stream order.
///
/// Extracted from `journal_kinds` so the #1184 cells can read PAYLOADS and INSTANTS from the same
/// reader that already answers "which kinds landed". Two readers over one journal is how the
/// first divergence becomes invisible.
fn journal(events: &Path) -> Vec<graphhelm_protocols::EventEnvelope> {
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
}

fn journal_kinds(events: &Path) -> Vec<String> {
    journal(events)
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

/// One event's payload, as JSON, read from the repository the CLI wrote.
///
/// The JOURNAL, never a command's own summary: a summary is the claim's author, and these cells
/// are about what was recorded.
fn journal_payload(events: &Path, wire_name: &str) -> Value {
    let history = journal(events);
    let envelope = history
        .iter()
        .find(|envelope| envelope.kind.wire_name() == wire_name)
        .unwrap_or_else(|| panic!("no {wire_name} in the stream"));
    serde_json::to_value(&envelope.kind).unwrap()["data"].clone()
}

/// The instant of the FIRST event of this wire kind whose serialized payload contains `needle`.
///
/// Keyed on the payload rather than on a position in the batch, because the batch here is written
/// by the real command: a cell that counted events would break on any unrelated append.
fn journal_occurred_at(
    events: &Path,
    wire_name: &str,
    needle: &str,
) -> chrono::DateTime<chrono::Utc> {
    let history = journal(events);
    let envelope = history
        .iter()
        .find(|envelope| {
            envelope.kind.wire_name() == wire_name
                && serde_json::to_string(&envelope.kind).is_ok_and(|text| text.contains(needle))
        })
        .unwrap_or_else(|| panic!("no {wire_name} carrying {needle}"));
    *envelope.occurred_at.as_datetime()
}

/// #1184 review BLOCK, the START half: the declaration snapshots each node's customs budgets, so
/// the wait a real `execution start` opens has a DEADLINE.
///
/// The fold cells for this live in `core/events/tests/sweep_verb.rs` and build the declaration
/// event by hand, which is the right grain for the projection and leaves exactly one thing
/// unmeasured: whether `start` writes those budgets at all. Removing the snapshot in
/// `apps/cli/src/commands/execution/start.rs` reddens nothing in that suite -- measured -- so this
/// cell exists to close that gap, through the real command and the real example graph.
///
/// `examples/graphs/customs-acting.yaml` declares `waitWithinSeconds: 86400` on `implementation`,
/// and this asserts the horizon that number produces rather than merely that a deadline exists.
/// "Is present" would pass for a horizon at the instant of entry, which is what a budget of zero
/// produces and what a presence check cannot see.
#[test]
fn a_started_execution_records_the_declared_customs_budgets_and_the_wait_gets_a_deadline() {
    let directory = tempfile::tempdir().unwrap();
    let (events, _fixtures) = start_parked(directory.path(), "exec-customs-deadline");

    // THE DECLARATION carries the budgets.
    let declared = journal_payload(&events, "execution_form_declared");
    let budgets = &declared["nodeCustomsBudgets"]["implementation"];
    assert_eq!(
        budgets["waitWithinSeconds"], 86400,
        "the graph's own declared wait budget must reach the declaration event: {declared}"
    );
    assert_eq!(
        budgets["clearanceWithinSeconds"], 3600,
        "and the clearance budget beside it: {declared}"
    );
    // `release_notes` declares customs too (with an empty `proofKinds`), so its budgets are
    // recorded as well. A snapshot that kept only the node that happened to park would be a
    // narrower rule than the declaration states.
    assert_eq!(
        declared["nodeCustomsBudgets"]["release_notes"]["waitWithinSeconds"], 86400,
        "every node that declared customs is recorded, not only the one that parked: {declared}"
    );

    // THE WAIT gets the horizon those budgets produce -- the property the review asked for, and
    // the one the declaration exists to serve. Before this, a wait opened by a real start carried
    // no deadline at all and no instant could make it overdue.
    let projection = replay_projection(&events);
    // `openWaits`, which is the FOLD's own map. `customs.nodes.<id>.openWait` is the STATUS
    // command's render of the same fact and is asserted separately below: the first read here
    // took the status path against the replay projection and got `Null` -- an absence at the
    // wrong path, which reads exactly like a missing deadline.
    let wait = &projection["openWaits"]["implementation"];
    let park_at = journal_occurred_at(&events, "node_outcome_recorded", "waiting_input");
    let expected = graphhelm_protocols::PersistedTimestamp::from_datetime(
        park_at
            .checked_add_signed(chrono::Duration::seconds(86400))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        wait["deadline"],
        serde_json::to_value(&expected).unwrap(),
        "the park instant plus the declared 86400 seconds, and nothing else: {projection}"
    );

    // AND IT REACHES THE OPERATOR'''S SURFACE. The fold computing a horizon nobody can read would
    // satisfy the sweep and tell a person nothing, and `status` is the door the Studio reads.
    let rendered = status(&events, "exec-customs-deadline");
    assert_eq!(
        rendered["customs"]["nodes"]["implementation"]["openWait"]["deadline"],
        serde_json::to_value(&expected).unwrap(),
        "the same horizon on the surface an operator actually reads: {rendered}"
    );
}

/// #1184 review (pass B): the SYNCHRONOUS driver parks too.
///
/// This repository has two dispatch drivers. `core/runtime/src/driver.rs` serves the Public
/// Runtime API; `apps/cli/src/commands/execution/driver.rs` serves `execution start` on the CLI,
/// and the park was added to the first and not the second. A customs node started from the CLI
/// reached `Succeeded` and the execution completed with the declaration inert.
///
/// THE FIXTURE HERE SUCCEEDS, and that is the whole point of a second cell. Every other cell in
/// this file scripts `implementation` to `"unknown"`, which makes the FIXTURE EXECUTOR answer
/// `NeedsInput` -- so the node parks for a reason that has nothing to do with any customs
/// declaration, and those cells would stay green with the park deleted from both drivers. With
/// `"success"` the only thing that can produce a wait is the declaration.
#[test]
fn a_successful_customs_node_parks_on_the_synchronous_cli_driver() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let fixtures = write_json(
        directory.path(),
        "fixtures.json",
        &serde_json::json!({
            "nodeOutcomes": { "implementation": "success", "release_notes": "success" }
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
            "exec-sync-park",
        ])
        .output()
        .unwrap();
    let value = json(&output.stdout);
    assert_eq!(value["ok"], true, "{value}");

    // The node RAN and then parked: the fixture said success, so anything other than
    // `waiting_input` here means the declaration was ignored on this path.
    assert_eq!(
        value["data"]["nodeStates"]["implementation"], "waiting_input",
        "a node declaring proofKinds must park even when its work succeeded: {value}"
    );

    // THE EXECUTION IS NOT OVER. Without the park it completes, and a cell that only checked the
    // node state would still pass on a completed run whose terminal node was quietly skipped.
    assert_eq!(
        value["data"]["status"], "running",
        "the execution must not complete while a node waits: {value}"
    );
    assert_eq!(
        value["data"]["nodeStates"]["release_notes"], "ready",
        "the downstream node is held behind the parked one: {value}"
    );

    // THE WAIT IS OPEN AND BOUNDED. `claim` refuses against a node with no open wait, so a park
    // without one is unanswerable; and the deadline is what makes the sweep able to call it
    // overdue. `customs-acting.yaml` declares waitWithinSeconds: 86400 on this node.
    let projection = replay_projection(&events);
    // `lastOutcome` lives on the REPLAY projection, not on the start render -- the first version
    // of this cell read it off `value["data"]` and got Null, which is an absence at the wrong
    // path rather than a missing park. The start render carries node STATES; the journal carries
    // what produced them.
    assert_eq!(
        projection["lastOutcome"]["implementation"], "needs_input",
        "the recorded outcome is the park, not a success: {projection}"
    );
    let wait = &projection["openWaits"]["implementation"];
    assert!(
        wait["atSequence"].as_u64().is_some(),
        "the park must open a wait a claim can answer: {projection}"
    );
    let park_at = journal_occurred_at(&events, "node_outcome_recorded", "waiting_input");
    let expected = graphhelm_protocols::PersistedTimestamp::from_datetime(
        park_at
            .checked_add_signed(chrono::Duration::seconds(86400))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        wait["deadline"],
        serde_json::to_value(&expected).unwrap(),
        "the park instant plus the declared 86400 seconds: {projection}"
    );

    // AND IT REACHES THE OPERATOR. A park the surface does not report is a hang with extra steps.
    let rendered = status(&events, "exec-sync-park");
    assert_eq!(
        rendered["attention"], "needs_you",
        "a parked node is something the owner must answer: {rendered}"
    );
    assert!(
        rendered["attentionReasons"]
            .as_array()
            .is_some_and(|reasons| reasons.iter().any(|reason| {
                reason["kind"] == "waiting_input_node" && reason["node"] == "implementation"
            })),
        "the reason must name the node that is waiting: {rendered}"
    );
}

/// The CONTROL for the cell above, and it is the one that makes it mean anything. Same driver,
/// same successful fixture, same command -- only the node's declaration is absent. A park here
/// would mean the driver parks on something other than the declaration.
#[test]
fn a_successful_node_with_no_customs_completes_on_the_synchronous_cli_driver() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let fixtures = write_json(
        directory.path(),
        "fixtures.json",
        &serde_json::json!({ "nodeOutcomes": { "implementation": "success" } }),
    );
    // `software-feature.yaml`'s entrypoint declares no customs block at all.
    let graph_without_customs = root().join("examples/graphs/provider-less-demo.yaml");
    let output = command()
        .args([
            "execution",
            "start",
            "--file",
            graph_without_customs.to_str().unwrap(),
            "--events",
            events.to_str().unwrap(),
            "--fixtures",
            fixtures.to_str().unwrap(),
            "--mode",
            "supervised",
            "--execution",
            "exec-sync-nocustoms",
        ])
        .output()
        .unwrap();
    let value = json(&output.stdout);
    assert_eq!(value["ok"], true, "{value}");
    assert_ne!(
        value["data"]["nodeStates"]["implementation"], "waiting_input",
        "a node that declared no customs must not park: {value}"
    );
    let projection = replay_projection(&events);
    assert!(
        projection["openWaits"]["implementation"]["atSequence"]
            .as_u64()
            .is_none(),
        "and it opens no wait: {projection}"
    );
}
