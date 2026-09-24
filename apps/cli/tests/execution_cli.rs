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

/// #134: `dispatch` and `dispatchUnavailable` depend on the graph the COMMAND holds, not on the
/// stream -- `start` has it and publishes the view, bare `status` does not and says so -- so a
/// start-versus-status equality strips both before comparing what the stream alone determines.
/// Each command's own value is pinned where it is produced, never through this helper.
fn without_graph_derived(mut data: Value) -> Value {
    if let Some(object) = data.as_object_mut() {
        object.remove("dispatch");
        object.remove("dispatchUnavailable");
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

/// #90: `start --held` publishes and records `execution_started` WITHOUT entering the drive loop.
///
/// The operator need #79 surfaced: today the only way to stage work without running it is to
/// `start` (which dispatches) and then `pause` — a reaction racing the driver, not a precondition.
/// `mode` does not hold dispatch (`core/runtime/src/driver.rs` never reads it), so the hold has to
/// be its own axis.
///
/// The assertion that matters is the SECOND one. The first is arrangement: without it, a run that
/// failed to start at all would satisfy "nothing was dispatched" for the wrong reason.
/// #90, J reviewing: the flag's own documentation promises "every node stays `Draft` until an
/// explicit `execution resume`". THAT SENTENCE IS THE CLAIM UNDER TEST HERE, and until this cell
/// existed nothing checked it -- a flag may not document a next step that its own output cannot
/// reach. `resume_preconditions` (core/execution/src/recovery.rs) refuses with `NotPaused` unless
/// `simulation_status == Paused`, and a held start recorded no status at all.
///
/// This is the cell I owed and did not write. The previous one asserted what a held start does
/// NOT do; nothing asserted that the operator can then do the one thing the flag exists for.
#[test]
fn a_held_start_can_be_resumed_because_that_is_what_the_flag_promises() {
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
            "exec_resume",
            "--held",
        ])
        .output()
        .unwrap();
    assert!(
        start.status.success(),
        "ARRANGEMENT: the held start must succeed, or the resume below fails for the wrong reason: {}",
        String::from_utf8_lossy(&start.stdout)
    );

    // THE PROPERTY: the documented next step is reachable from the state the flag produces.
    let resume = command()
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
            "exec_resume",
        ])
        .output()
        .unwrap();
    // Asserted on the PROCESS, not on parsed JSON: a refusal may not print a JSON object at all,
    // and a cell that panics in its own parser reports "something went wrong" without saying what.
    // The raw streams go in the message so the failure NAMES the refusal.
    assert!(
        resume.status.success(),
        "a held start must be resumable: the flag documents `execution resume` as the way out of the hold, so a refusal means it documents a step its own output cannot reach. stdout: {} stderr: {}",
        String::from_utf8_lossy(&resume.stdout),
        String::from_utf8_lossy(&resume.stderr)
    );
}

#[test]
fn start_held_publishes_without_dispatching_any_node() {
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
            "exec_held",
            "--held",
        ])
        .output()
        .unwrap();
    assert!(
        start.status.success(),
        "ARRANGEMENT: the held start must SUCCEED, or the emptiness below proves nothing about dispatch: {}",
        String::from_utf8_lossy(&start.stdout)
    );
    let value = json(&start.stdout);
    assert_eq!(value["ok"], true);
    assert_eq!(value["command"], "execution.start");
    assert_eq!(value["data"]["executionId"], "exec_held");

    // THE PROPERTY, asserted where dispatch leaves a trace (M, reviewing this: assert the property,
    // not a consequence). Dispatch is what APPENDS -- so "no node was dispatched" is exactly "the
    // stream holds no node event", and that is checked here rather than in the projection.
    //
    // WHY NOT THE PROJECTION, since it is right there in the reply: a held execution has no node
    // events, so the projection materialises NO nodes at all -- `nodeStates` is `{}` and EVERY
    // count including `draft` is 0. So `draft == 2` is false here, and `succeeded == 0` and
    // `status != "completed"` are true of that empty projection for a reason that has nothing to do
    // with holding: they would hold just as well if the events were never written at all. Each has
    // a second cause. The stream does not.
    //
    // NO NAMED EVENT, A PREFIX. A driven run of this graph writes `node_outcome_recorded`
    // (measured -- the kinds are exactly `execution_started`, `execution_form_declared`,
    // `node_outcome_recorded`, `execution_completed`), and matching that ONE name would go quietly
    // vacuous the day dispatch starts writing something else first. The claim is that nothing about
    // any node happened, so the assertion is over the whole `node_` family and a new member breaks
    // it -- which is the direction a "nothing happened" guard should fail in.
    //
    // THE ZERO NEEDS A CONTROL FROM THE SAME READ: `execution_started` must be PRESENT in this very
    // list. Without it a wrong `--events` path, an unwritten journal or a renamed kind all produce
    // the same zero, and the cell would pass by reading nothing.
    let journal = journal_events(&events);
    assert_eq!(
        journal
            .iter()
            .filter(|(_, kind, _)| kind == "execution_started")
            .count(),
        1,
        "CONTROL: the publish half must be IN this stream, or the absence below is 'nothing was read' rather than 'nothing was dispatched': {journal:?}"
    );
    // ATOMICITY, which is the property a second append could not have (Codex P1 on this PR).
    // `journal_events` returns the BATCH INDEX: the store writes one line per atomic append, so
    // two events sharing an index were committed together or not at all. Adjacency would only
    // show they landed in order.
    //
    // The state this forbids is worse than either endpoint: a start with no hold shuts BOTH exits
    // -- `start --held` refuses ("an execution has already started on this stream") and the
    // `resume` this flag documents refuses (`not_paused`) -- so the operator must diagnose a
    // partial write and repair it with a `pause` nobody told them to run.
    let batch_of = |wanted: &str| {
        journal
            .iter()
            .find(|(_, kind, _)| kind == wanted)
            .map(|(batch, _, _)| *batch)
    };
    assert_eq!(
        batch_of("execution_started"),
        batch_of("execution_paused"),
        "the hold must commit in the SAME atomic batch as the start, or a crash between them leaves an execution that can be neither started nor resumed: {journal:?}"
    );
    assert!(
        batch_of("execution_paused").is_some(),
        "CONTROL: the hold must exist at all -- two `None`s would satisfy the equality above: {journal:?}"
    );

    let node_events: Vec<_> = journal
        .iter()
        .filter(|(_, kind, _)| kind.starts_with("node_"))
        .collect();
    assert!(
        node_events.is_empty(),
        "a held start must dispatch NO node, but the stream records node events: {node_events:?}"
    );

    // The projection agrees, and is kept for what it adds: the reply a caller actually reads.
    assert_ne!(
        value["data"]["status"], "completed",
        "a held start must not drive to quiescence: {value}"
    );
    assert_eq!(
        value["data"]["nodeStateCounts"]["succeeded"],
        serde_json::json!(0),
        "no node may succeed when dispatch was never entered: {value}"
    );

    // AND INDEPENDENTLY, from a separate replay of the same stream: a reader who was not told what
    // the command claimed sees the same absence.
    let status = command()
        .args([
            "execution",
            "status",
            "--events",
            events.to_str().unwrap(),
            "--execution",
            "exec_held",
        ])
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "{}",
        String::from_utf8_lossy(&status.stdout)
    );
    let status_value = json(&status.stdout);
    assert_eq!(
        status_value["data"]["nodeStateCounts"]["succeeded"],
        serde_json::json!(0),
        "an independent replay must agree that nothing was dispatched: {status_value}"
    );
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
    // #1064: the CLI's `start` is always fixture-driven, and its reply names that executor.
    assert_eq!(start_value["data"]["executor"], "fixture", "{start_value}");
    // #192: manual-override-deploy.yaml lints clean (zero errors) but with warnings (GHG101 on
    // both nodes' missing timeoutSeconds) — those warnings were silently dropped on this exact
    // success path before the fix, since `diagnostics.extend(report.warnings)` lived only inside
    // the `!errors.is_empty()` branch this run never takes.
    let start_diagnostics = start_value["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(
        start_diagnostics
            .iter()
            .any(|diagnostic| diagnostic["code"] == "GHG101_DEFAULT_TIMEOUT"),
        "a clean-but-warned lint pass must still reach the caller on a successful start: \
         {start_value}"
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
        without_graph_derived(without_head_sequence(status_value["data"].clone())),
        without_graph_derived(start_value["data"].clone()),
        "status must independently replay to the same data start reported (the graph-derived dispatch view aside: start holds the graph, bare status does not)"
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
    // #134: `ready: 1` was the over-promise -- `deploy` is Ready and the driver would never
    // dispatch it, because `implementation` is blocked. The dispatch gate now sits beside the
    // state: zero ready to dispatch, one gated, and it is named. `start` holds the graph, so the
    // view is present and `dispatchUnavailable` is null.
    assert_eq!(start_value["data"]["dispatch"]["ready"], 0);
    assert_eq!(start_value["data"]["dispatch"]["gated"], 1);
    assert_eq!(
        start_value["data"]["dispatch"]["waitingCapacity"],
        serde_json::json!([]),
        "nothing is edge-ready here, so nothing waits on capacity either"
    );
    assert_eq!(
        start_value["data"]["dispatch"]["gatedNodes"],
        serde_json::json!(["deploy"])
    );
    assert_eq!(
        start_value["data"]["dispatchUnavailable"],
        serde_json::Value::Null
    );
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
    assert_eq!(
        without_graph_derived(without_head_sequence(status_data)),
        without_graph_derived(start_value["data"].clone())
    );
}

// ---------------------------------------------------------------------------
// Shared helpers for `signal`, `approve`, `pause`, `resume`, `cancel`
// ---------------------------------------------------------------------------

/// #134: `status` holds no graph, so it cannot tell a gated `Ready` from a dispatchable one --
/// and says so, rather than publishing a zero that reads as "nothing gated". With `--file` naming
/// the graph the execution started from, it derives the same view `start` published; with a
/// different graph it refuses, because a gate computed over edges this execution never had would
/// be a fabrication wearing the field's name.
#[test]
fn status_derives_the_dispatch_gate_only_with_the_recorded_graph() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let fixtures_path = directory.path().join("fixtures.json");
    std::fs::write(
        &fixtures_path,
        serde_json::to_vec(&serde_json::json!({
            "nodeOutcomes": { "implementation": "failure" }
        }))
        .unwrap(),
    )
    .unwrap();
    let start_data = start(&events, &fixtures_path, "autopilot", "exec_134_gate");
    assert_eq!(
        start_data["nodeStateCounts"]["ready"], 1,
        "ARRANGEMENT: the dependent must be Ready-but-gated, or this cell measures nothing: {start_data}"
    );
    assert_eq!(start_data["dispatch"]["gated"], 1);

    // Without the graph: null, and the reason named. Not zero.
    let bare = status(&events, "exec_134_gate");
    assert_eq!(bare["dispatch"], serde_json::Value::Null);
    assert!(
        bare["dispatchUnavailable"]
            .as_str()
            .is_some_and(|reason| reason.contains("--file")),
        "the absence must say how to get the view: {bare}"
    );

    // With the recorded graph: the same view `start` published.
    let graph = root().join("examples/graphs/manual-override-deploy.yaml");
    let with_graph = command()
        .args([
            "execution",
            "status",
            "--events",
            events.to_str().unwrap(),
            "--execution",
            "exec_134_gate",
            "--file",
            graph.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        with_graph.status.success(),
        "{}",
        String::from_utf8_lossy(&with_graph.stdout)
    );
    let with_graph_envelope = json(&with_graph.stdout);
    // The lint warnings the file produces (#192: GHG101 on both nodes' missing timeoutSeconds)
    // reach the status envelope exactly as they reach `start`'s -- computed diagnostics are not
    // dropped on the way to a successful reply (Codex on #1014). Bare status computed none.
    assert!(
        with_graph_envelope["diagnostics"]
            .as_array()
            .is_some_and(|diagnostics| !diagnostics.is_empty()),
        "status --file must carry the graph's lint warnings: {with_graph_envelope}"
    );
    // Measured, not assumed: this graph lints to FOUR warnings today (the first draft of this
    // cell said two and was wrong). The count is the linter's to change; the cell pins that every
    // one of them is a lint code and none is an error, which is the property `status --file`
    // promises -- computed diagnostics reach the envelope, and a warning never turns into a refusal.
    assert!(
        with_graph_envelope["diagnostics"]
            .as_array()
            .is_some_and(|diagnostics| diagnostics.iter().all(|diagnostic| {
                diagnostic["code"]
                    .as_str()
                    .is_some_and(|code| code.starts_with("GHG"))
                    && diagnostic["severity"] != "error"
            })),
        "every diagnostic on a successful status --file is a lint warning: {with_graph_envelope}"
    );
    let with_graph = with_graph_envelope["data"].clone();
    assert_eq!(with_graph["dispatch"]["ready"], 0);
    assert_eq!(with_graph["dispatch"]["gated"], 1);
    assert_eq!(
        with_graph["dispatch"]["gatedNodes"],
        serde_json::json!(["deploy"])
    );
    assert_eq!(with_graph["dispatchUnavailable"], serde_json::Value::Null);

    // Paused: nothing dispatches until resume, however Ready the nodes are -- the view says so
    // instead of counting them (Codex on #1014).
    let paused = pause(&events, "exec_134_gate");
    assert_eq!(
        paused["status"], "paused",
        "ARRANGEMENT: the pause took: {paused}"
    );
    let while_paused = command()
        .args([
            "execution",
            "status",
            "--events",
            events.to_str().unwrap(),
            "--execution",
            "exec_134_gate",
            "--file",
            graph.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        while_paused.status.success(),
        "{}",
        String::from_utf8_lossy(&while_paused.stdout)
    );
    let while_paused = json(&while_paused.stdout)["data"].clone();
    assert_eq!(while_paused["dispatch"], serde_json::Value::Null);
    assert!(
        while_paused["dispatchUnavailable"]
            .as_str()
            .is_some_and(|reason| reason.contains("paused")),
        "a paused execution publishes no readiness and says why: {while_paused}"
    );

    // With another graph: refused, not rendered.
    let other = root().join("examples/graphs/software-feature.yaml");
    let refused = command()
        .args([
            "execution",
            "status",
            "--events",
            events.to_str().unwrap(),
            "--execution",
            "exec_134_gate",
            "--file",
            other.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(refused.status.code(), Some(2));
    let value = json(&refused.stdout);
    assert_eq!(value["ok"], false);
    assert_eq!(value["diagnostics"][0]["code"], "GHCLI005_EXECUTION_STATE");

    // And with `--html` beside the refused `--file`: exit 2 AND no artefact (Codex on #1014) --
    // the snapshot used to be written before the graph was checked, leaving a page that looked
    // like a success behind a command that refused.
    let snapshot = directory.path().join("refused-snapshot.html");
    let refused_with_html = command()
        .args([
            "execution",
            "status",
            "--events",
            events.to_str().unwrap(),
            "--execution",
            "exec_134_gate",
            "--file",
            other.to_str().unwrap(),
            "--html",
            snapshot.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(refused_with_html.status.code(), Some(2));
    assert!(
        !snapshot.exists(),
        "a refused status must not leave a snapshot behind: {}",
        String::from_utf8_lossy(&refused_with_html.stdout)
    );
}

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

/// #135: the refusal NAMES THE REMEDY, and the store was never the obstacle.
///
/// Filed as "a keyless store cannot record a signal at all". Measured, that is not a capability
/// gap — the cell below drives the two commands that do it — so the repair is the issue's own
/// second remedy: write the requirement's reason where the refusal is.
///
/// This asserts BOTH halves, because either alone is satisfiable by the wrong program: a refusal
/// with clap's bare "required arguments were not provided" would pass an assertion that only
/// checks the refusal, and a message naming `gateway keyring init` printed beside a RECORDED
/// signal would pass an assertion that only checks the text.
#[test]
fn a_signal_with_no_keyring_is_refused_and_told_how_to_get_one() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let fixtures = all_success_fixtures(directory.path());
    start(&events, &fixtures, "supervised", "exec_signal_nokeyring");

    let envelope_value = signal_envelope("signal-nokeyring", "no_progress");
    let envelope_path = write_json(directory.path(), "signal-nokeyring.json", &envelope_value);
    let evidence_out = directory.path().join("evidence-nokeyring.json");

    let output = command()
        .args([
            "execution",
            "signal",
            "--events",
            events.to_str().unwrap(),
            "--execution",
            "exec_signal_nokeyring",
            "--signal",
            envelope_path.to_str().unwrap(),
            "--evidence-out",
            evidence_out.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "a signal with no keyring was recorded: the seal is required and its absence must never be \
         silent: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        status(&events, "exec_signal_nokeyring")["signalsRecorded"],
        0,
        "nothing may be appended when the envelope could not be sealed"
    );

    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        said.contains("gateway keyring init"),
        "the refusal did not name the command that mints a keyring, so the operator learns only \
         that something is missing -- which is the state #135 was filed from: {said}"
    );
    assert!(
        said.contains("GRAPHHELM_EVENTS_KEY"),
        "the refusal named the command but not the environment variable it needs, which is the \
         half of the setup that is not in any argument list: {said}"
    );
    // THE STEP THE ADVICE FORGOT, and the reason it survived a review: `gateway keyring init`
    // answers "the keyring directory does not exist" for a directory it is not given, so a refusal
    // that names only the command sends the operator to a THIRD refusal. Measured on 40c38abe.
    assert!(
        said.contains("directory"),
        "the refusal names the command but not the empty directory it requires first, so following \
         it literally produces another refusal -- the defect this PR exists to repair: {said}"
    );
    // The STRUCTURED refusal, not only the prose: a caller parsing JSON reads these, and an
    // assertion on the message alone would pass while the code or the path moved.
    let refusal = json(&output.stdout);
    assert_eq!(refusal["ok"], false, "{refusal}");
    assert_eq!(
        refusal["diagnostics"][0]["code"], "GHCLI001_ARGUMENT_INVALID",
        "{refusal}"
    );
    assert_eq!(refusal["diagnostics"][0]["path"], "/keyring", "{refusal}");
}

/// #135: THE HELP TEXT IS AN OPERATOR SURFACE, AND THIS IS THE ONLY THING THAT WATCHES IT.
///
/// A `///` comment on a clap field IS the `--help` text. That is not a detail: three revisions of
/// this PR shipped rationale written for maintainers into the one surface a stuck operator reads,
/// and `signal --help` answered a person with the issue's history. Nothing in the suite could see
/// it, and the squash body for this change originally claimed no cell could -- which was false, and
/// is why this exists (J's APPROVE-WITH-RISK at `65e83216` named the gap).
///
/// COUNTS, NOT `contains`. A rationale paragraph drifting back above the `//` marker does not
/// remove these strings, it ADDS text around them -- and the failure that actually happened was
/// extra prose, not missing prose. `"REQUIRED, with"` appears once per flag and
/// `"gateway keyring init"` once in the recipe; a fourth `REQUIRED` or a second recipe means the
/// boundary moved, and an assertion on presence alone would stay green through it.
#[test]
fn the_signal_help_tells_an_operator_what_to_do_and_nothing_else() {
    let output = command()
        .args(["execution", "signal", "--help"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "`signal --help` did not succeed, so the assertions below would measure nothing"
    );
    let help = String::from_utf8_lossy(&output.stdout);

    assert_eq!(
        help.matches("REQUIRED, with").count(),
        2,
        "the two sealing flags must each be announced as required -- clap's usage line shows them \
         inside [OPTIONS], so this sentence is the only place an operator learns the contract:\n{help}"
    );
    assert_eq!(
        help.matches("gateway keyring init").count(),
        1,
        "the help must name the command that mints a keyring exactly once -- twice means the \
         rationale drifted back into the `///` block, which is the defect #135 kept reproducing:\n{help}"
    );
    assert_eq!(
        help.matches("does not create the directory").count(),
        1,
        "the recipe must keep its own first step: `gateway keyring init` answers \"the keyring \
         directory does not exist\" for a directory it is not given, and an operator following the \
         help literally would meet a second refusal:\n{help}"
    );
    // THE OTHER HALF: maintainer prose must NOT be here. `//` is invisible to clap, and the
    // marker line below moved every rationale paragraph behind it.
    assert!(
        !help.contains("#135 was filed as"),
        "the rationale is back in the help text -- an operator asking for help is being answered \
         with the issue's history:\n{help}"
    );
    assert!(
        !help.contains("real-executor.md"),
        "a maintainer's document reference reached the operator's help text:\n{help}"
    );
}

/// #135, END TO END: the two commands that unblock the operator, run as one story.
///
/// This is the measurement that retracted the first version of this change, kept as a cell so the
/// claim in the refusal above cannot rot. If `gateway keyring init` ever stops producing a keyring
/// `execution signal` can open, the refusal starts telling operators to run something that does not
/// help, and only this cell would notice — the message assertion above would still pass.
///
/// The store is created WITHOUT a keyring, which is the #135 fixture exactly.
#[test]
fn a_minted_keyring_lets_a_keyless_store_record_a_sealed_signal() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let fixtures = all_success_fixtures(directory.path());
    start(&events, &fixtures, "supervised", "exec_signal_minted");

    // Step one, IN THE ORDER THE REFUSAL GIVES IT. The `create_dir_all` here is not a test
    // convenience -- it is the first step of the advice, and it is here because the advice says so.
    // Until 40c38abe the message did not mention it while this cell did it anyway, so the cell
    // proved the COMMAND works given a directory and never that the ADVICE works. That is how the
    // omission survived: the end-to-end cell quietly supplied what the sentence left out.
    let keyring = directory.path().join("minted-keyring");
    std::fs::create_dir_all(&keyring).unwrap();
    let minted = command()
        .args([
            "gateway",
            "keyring",
            "init",
            "--keyring",
            keyring.to_str().unwrap(),
            "--key-id",
            "minted-key",
        ])
        .env("GRAPHHELM_EVENTS_KEY", SIGNAL_KEY_HEX)
        .output()
        .unwrap();
    assert!(
        minted.status.success(),
        "the command the refusal tells the operator to run did not work: {}",
        String::from_utf8_lossy(&minted.stdout)
    );

    // Step two: the same store, now with the keyring it never had.
    let envelope_value = signal_envelope("signal-minted", "no_progress");
    let envelope_path = write_json(directory.path(), "signal-minted.json", &envelope_value);
    let evidence_out = directory.path().join("evidence-minted.json");
    let output = command()
        .args([
            "execution",
            "signal",
            "--events",
            events.to_str().unwrap(),
            "--execution",
            "exec_signal_minted",
            "--signal",
            envelope_path.to_str().unwrap(),
            "--evidence-out",
            evidence_out.to_str().unwrap(),
            "--keyring",
            keyring.to_str().unwrap(),
            "--key-id",
            "minted-key",
        ])
        .env("GRAPHHELM_EVENTS_KEY", SIGNAL_KEY_HEX)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "a minted keyring did not satisfy the requirement, so the refusal's advice is wrong: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(json(&output.stdout)["ok"], true);
    assert_eq!(
        status(&events, "exec_signal_minted")["signalsRecorded"],
        1,
        "the signal was reported as admitted but never recorded"
    );
    assert!(
        evidence_out.exists(),
        "the operator copy must still be written beside the seal"
    );
}

/// CONTROL: half a keyring is refused, and the refusal names the missing half.
///
/// A `--keyring` with no `--key-id` is an operator asking for a seal and not saying which key --
/// accepting it would either seal with a guessed identity or silently skip the seal.
///
/// The name and the reason both said "making the pair optional must not make it separable" until
/// #1002's second withdrawal. Nothing is optional now, and a cell that describes a version of the
/// code that no longer exists is the defect this PR is about, sitting inside the PR (A's block on
/// `67725c20`). What it guards today is the refusal being USEFUL: the sibling above pins that the
/// no-keyring refusal names its remedy, and this one would have looked finished while still ending
/// in "or neither to record without sealing" -- an instruction that now produces a second refusal.
#[test]
fn half_a_keyring_is_refused_and_the_missing_half_is_named() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let fixtures = all_success_fixtures(directory.path());
    start(&events, &fixtures, "supervised", "exec_signal_half_keyring");

    let envelope_value = signal_envelope("signal-half", "no_progress");
    let envelope_path = write_json(directory.path(), "signal-half.json", &envelope_value);
    let evidence_out = directory.path().join("evidence-half.json");
    let keyring = signal_keyring(directory.path());

    let output = command()
        .args([
            "execution",
            "signal",
            "--events",
            events.to_str().unwrap(),
            "--execution",
            "exec_signal_half_keyring",
            "--signal",
            envelope_path.to_str().unwrap(),
            "--evidence-out",
            evidence_out.to_str().unwrap(),
            "--keyring",
            keyring.to_str().unwrap(),
        ])
        .env("GRAPHHELM_EVENTS_KEY", SIGNAL_KEY_HEX)
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "a keyring with no key-id was accepted: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let refusal = json(&output.stdout);
    assert_eq!(
        refusal["diagnostics"][0]["code"], "GHCLI001_ARGUMENT_INVALID",
        "{refusal}"
    );
    assert_eq!(refusal["diagnostics"][0]["path"], "/keyId", "{refusal}");
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        said.contains("key-id"),
        "the refusal did not name the half that is missing, so it is the bare \"something is \
         wrong\" this PR exists to stop shipping: {said}"
    );
    assert_eq!(
        status(&events, "exec_signal_half_keyring")["signalsRecorded"],
        0,
        "the half-configured request recorded a signal anyway"
    );
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

/// #92: approve makes an execution QUIETER WITHOUT MAKING IT MOVE, and the approve response is
/// where that is visible.
///
/// `drive_to_quiescence` is reached from `start` and `resume` only, so after `approve` nothing
/// will dispatch the node it just readied. Attention then goes quiet for reasons that are each
/// correct alone: `BlockedNode` left with the `Blocked` state; `Ready` raises no silence of its
/// own (never dispatched, so there is no turn to be late for); and `Ready` counts as advancing,
/// which suppresses the wedge that would otherwise have spoken. The composition is an execution
/// that reads calm with a node nothing will ever pick up.
///
/// Every existing approve cell resumes immediately and supplies the drive by hand, which is why
/// the suite has never seen this. This one stops where a human stops -- at the command the
/// `BlockedNode` message invited them to run.
///
/// CONTROL BELOW, and it is load-bearing: a negative assertion is worthless if the value can
/// never appear, so the same suite proves `can_sleep` IS what a genuinely settled execution
/// reports. Without it, `assert_ne!(..., "can_sleep")` would pass on a typo.
#[test]
fn approve_without_resume_does_not_report_the_execution_as_calm() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let fixtures = fixtures_file(
        directory.path(),
        serde_json::json!({"implementation": "failure"}),
    );
    let start_data = start(&events, &fixtures, "supervised", "exec_92_calm_but_stalled");
    assert_eq!(
        start_data["nodeStateCounts"]["blocked"], 1,
        "ARRANGEMENT: the node must be Blocked, or there is nothing to approve: {start_data}"
    );
    assert_eq!(
        start_data["attention"], "needs_you",
        "ARRANGEMENT: a blocked node must be speaking before approve, or this cell measures \
         nothing about approve silencing it: {start_data}"
    );

    let output = command()
        .args([
            "execution",
            "approve",
            "--events",
            events.to_str().unwrap(),
            "--execution",
            "exec_92_calm_but_stalled",
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

    // The state half: approve did its job.
    let projection = replay_projection(&events);
    assert_eq!(projection["nodeStates"]["implementation"], "ready");

    // The verdict lives under `data`: the raw envelope is {ok, command, data, diagnostics}
    // and `render`'s view is the `data` member. Reading `value["attention"]` compares Null to
    // a string and passes whatever the execution is doing -- which is how the first draft of
    // this cell reported the defect ABSENT. Pinned, so a shape change fails loudly instead of
    // quietly making the assertion below vacuous again.
    assert!(
        value["data"]["attention"].is_string(),
        "ARRANGEMENT: the verdict must be readable at data.attention, or the assertion below measures nothing: {value}"
    );
    // The half that is the defect. Nothing will dispatch that Ready node -- no driver runs after
    // approve -- so an execution reporting `can_sleep` here is telling the operator their remedy
    // worked when it only removed the voice.
    assert_ne!(
        value["data"]["attention"], "can_sleep",
        "#92: approve readied a node that nothing will dispatch, and reported the execution as \
         calm. The operator's remedy removed the only voice and changed nothing material: \
         {value}"
    );
}

/// The positive control for the cell above: `can_sleep` is a value this surface really produces,
/// so `assert_ne!` there is a claim about THIS execution and not about a string that never
/// appears. An all-success graph driven to completion by `start` has nothing left to say.
#[test]
fn a_settled_execution_does_report_can_sleep() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let fixtures = all_success_fixtures(directory.path());
    let start_data = start(&events, &fixtures, "supervised", "exec_92_control_settled");
    assert_eq!(
        start_data["nodeStateCounts"]["blocked"], 0,
        "ARRANGEMENT: nothing blocked in the control: {start_data}"
    );
    assert_eq!(
        start_data["attention"], "can_sleep",
        "CONTROL: a settled execution reports can_sleep, so the negative assertion above is \
         about this execution rather than about an unreachable value: {start_data}"
    );
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

/// #192, `resume`'s own cell -- G's review of #575 measured this site had zero coverage: the fix
/// there is structurally identical to `start`'s (both share `run`'s lint-check-then-publish
/// shape), but "identical shape" is a claim about the diff, not a claim this suite had verified
/// for `resume` specifically until now.
///
/// Reuses the exact start -> approve -> pause -> resume story
/// `approve_is_not_a_dead_end_once_the_condition_is_fixed` already drives (same fixture,
/// `manual-override-deploy.yaml`, `implementation`/`deploy` both lack `timeoutSeconds`), captured
/// here through the raw envelope rather than the `resume()` helper -- that helper only returns
/// `["data"]`, discarding the `diagnostics` field this test exists to check.
#[test]
fn a_warning_only_lint_pass_reaches_the_caller_on_a_successful_resume() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let failing = fixtures_file(
        directory.path(),
        serde_json::json!({"implementation": "failure"}),
    );
    start(&events, &failing, "supervised", "exec_resume_warnings");

    let approve = command()
        .args([
            "execution",
            "approve",
            "--events",
            events.to_str().unwrap(),
            "--execution",
            "exec_resume_warnings",
            "--node",
            "implementation",
        ])
        .output()
        .unwrap();
    assert!(approve.status.success(), "{}", json(&approve.stdout));

    let pause = command()
        .args([
            "execution",
            "pause",
            "--events",
            events.to_str().unwrap(),
            "--execution",
            "exec_resume_warnings",
        ])
        .output()
        .unwrap();
    assert!(pause.status.success(), "{}", json(&pause.stdout));

    let fixed = write_json(
        directory.path(),
        "resume-fixed.json",
        &serde_json::json!({"nodeOutcomes": {"implementation": "success", "deploy": "success"}}),
    );
    let graph = root().join("examples/graphs/manual-override-deploy.yaml");
    let resume = command()
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
            "exec_resume_warnings",
        ])
        .output()
        .unwrap();
    assert!(
        resume.status.success(),
        "{}",
        String::from_utf8_lossy(&resume.stdout)
    );
    let resume_value = json(&resume.stdout);
    assert_eq!(resume_value["data"]["status"], "completed");
    let resume_diagnostics = resume_value["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(
        resume_diagnostics
            .iter()
            .any(|diagnostic| diagnostic["code"] == "GHG101_DEFAULT_TIMEOUT"),
        "a clean-but-warned lint pass must still reach the caller on a successful resume: \
         {resume_value}"
    );
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

/// `execution pause --file <graph>`, raw, so a REFUSING call is a subject too (#157 repair).
fn pause_with_graph_file(events: &Path, execution: &str, graph: &Path) -> std::process::Output {
    command()
        .args([
            "execution",
            "pause",
            "--file",
            graph.to_str().unwrap(),
            "--events",
            events.to_str().unwrap(),
            "--execution",
            execution,
        ])
        .output()
        .unwrap()
}

/// `execution pause --file`, the edge-aware form (#157). Separate helper from `pause` above so
/// the two doors stay distinguishable in the tests that assert what each one holds.
fn pause_with_graph(events: &Path, execution: &str) -> Value {
    let graph = root().join("examples/graphs/manual-override-deploy.yaml");
    let output = pause_with_graph_file(events, execution, &graph);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    json(&output.stdout)["data"].clone()
}

/// #157: `pause` must hold what the EDGES leave eligible, not what the state LABEL suggests.
///
/// `implementation` fails, so it is `Blocked` and its `data` edge to `deploy` never releases.
/// `deploy` is `Ready` — the label — but `dispatch_candidates` excludes it, so the driver would
/// never run it. Before this test, `pause` held it anyway and named it in `heldNodes`, which is a
/// record claiming a hold that stopped nothing.
///
/// SCOPE, narrowed by review of this PR: `dispatch_candidates` is the EDGE-ELIGIBLE set,
/// upstream of capacity planning. It is not a promise that every node in it would actually be
/// dispatched on the next pass, so this cell asserts exclusion BY AN EDGE, the only thing that
/// set decides.
///
/// The assertion is on BOTH halves, because narrowing the list without leaving the node alone
/// would be the same defect with a shorter list: `heldNodes` is empty AND `deploy` stays `Ready`
/// rather than being recorded `Paused`.
#[test]
fn pause_holds_only_edge_eligible_candidates() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let fixtures = fixtures_file(
        directory.path(),
        serde_json::json!({"implementation": "failure", "deploy": "success"}),
    );
    let start_data = start(&events, &fixtures, "supervised", "exec_pause_gated");
    assert_eq!(start_data["nodeStateCounts"]["blocked"], 1);
    assert_eq!(start_data["nodeStateCounts"]["ready"], 1);

    let pause_data = pause_with_graph(&events, "exec_pause_gated");
    assert_eq!(
        pause_data["heldNodes"],
        serde_json::json!([]),
        "deploy is Ready but edge-gated behind a Blocked predecessor, so it is not edge-eligible and pause holds nothing: {pause_data}"
    );
    assert_eq!(pause_data["heldNodesGated"], serde_json::json!(true));
    assert_eq!(pause_data["status"], "paused");
    assert_eq!(pause_data["nodeStateCounts"]["paused"], 0);
    assert_eq!(pause_data["nodeStateCounts"]["ready"], 1);

    let projection = replay_projection(&events);
    assert_eq!(
        projection["nodeStates"]["deploy"], "ready",
        "the node pause did not hold keeps its own state: {projection}"
    );
    assert_eq!(projection["nodeStates"]["implementation"], "blocked");
}

/// #157 control, POSITIVE arm: the narrowing must still hold what IS edge-eligible.
///
/// The exclusion cell above passes on an empty list, and an empty list is also what a pause that
/// holds NOTHING produces. This cell is the other half: `approve` readies the entrypoint
/// `implementation`, which has no predecessors and is therefore edge-eligible, while `deploy`
/// stays gated behind it. One pass must hold exactly the first and leave the second alone.
#[test]
fn pause_holds_the_eligible_entrypoint_and_leaves_its_gated_successor() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let fixtures = fixtures_file(
        directory.path(),
        serde_json::json!({"implementation": "failure", "deploy": "success"}),
    );
    start(&events, &fixtures, "supervised", "exec_pause_eligible");

    let approved = command()
        .args([
            "execution",
            "approve",
            "--events",
            events.to_str().unwrap(),
            "--execution",
            "exec_pause_eligible",
            "--node",
            "implementation",
        ])
        .output()
        .unwrap();
    assert!(
        approved.status.success(),
        "{}",
        String::from_utf8_lossy(&approved.stdout)
    );
    let readied = replay_projection(&events);
    assert_eq!(readied["nodeStates"]["implementation"], "ready");
    assert_eq!(readied["nodeStates"]["deploy"], "ready");

    let pause_data = pause_with_graph(&events, "exec_pause_eligible");
    assert_eq!(
        pause_data["heldNodes"],
        serde_json::json!(["implementation"]),
        "the entrypoint has no predecessor to gate it, so the edge-aware hold must name it and must not name the successor it gates: {pause_data}"
    );
    assert_eq!(pause_data["heldNodesGated"], serde_json::json!(true));
    assert_eq!(pause_data["nodeStateCounts"]["paused"], 1);

    let projection = replay_projection(&events);
    assert_eq!(projection["nodeStates"]["implementation"], "paused");
    assert_eq!(
        projection["nodeStates"]["deploy"], "ready",
        "the gated successor is left exactly as it was: {projection}"
    );
}

/// #157 control, REFUSAL arm: a graph that is not this execution's, and a graph that is not a
/// graph, each leave the event store with byte-identical contents.
///
/// `--file` is new surface on a verb that APPENDS. The check that the supplied graph matches the
/// execution runs before any append, and the loader/linter runs before that; this cell is what
/// says so from outside, by comparing the whole journal — every atomic batch, type and payload —
/// across the refusal rather than by trusting the order of the code.
#[test]
fn pause_with_a_graph_it_cannot_trust_leaves_the_journal_untouched() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let fixtures = fixtures_file(
        directory.path(),
        serde_json::json!({"implementation": "failure", "deploy": "success"}),
    );
    start(&events, &fixtures, "supervised", "exec_pause_untrusted");
    let before = journal_bytes(&events);

    // A well-formed graph that this execution did not start from.
    let foreign = root().join("examples/graphs/provider-less-demo.yaml");
    let refused = pause_with_graph_file(&events, "exec_pause_untrusted", &foreign);
    assert!(
        !refused.status.success(),
        "a foreign graph must refuse: {}",
        String::from_utf8_lossy(&refused.stdout)
    );
    let reply = json(&refused.stdout);
    assert_eq!(reply["ok"], false);
    assert_eq!(reply["command"], "execution.pause");
    assert_eq!(reply["diagnostics"][0]["code"], "GHCLI005_EXECUTION_STATE");
    assert_eq!(
        journal_bytes(&events),
        before,
        "a refused pause appends nothing: {}",
        String::from_utf8_lossy(&refused.stdout)
    );

    // A file that is not a graph at all: the refusal must come from the loader, still with
    // nothing appended.
    let malformed = directory.path().join("not-a-graph.yaml");
    std::fs::write(&malformed, "apiVersion: p50.dev/graph/v1\nkind: [oops\n").unwrap();
    let refused_malformed = pause_with_graph_file(&events, "exec_pause_untrusted", &malformed);
    assert!(
        !refused_malformed.status.success(),
        "a malformed graph must refuse: {}",
        String::from_utf8_lossy(&refused_malformed.stdout)
    );
    assert_eq!(json(&refused_malformed.stdout)["ok"], false);
    assert_eq!(
        journal_bytes(&events),
        before,
        "a malformed graph appends nothing either: {}",
        String::from_utf8_lossy(&refused_malformed.stdout)
    );
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
    // #157: this helper passes no `--file`, so the hold is the bare-state fallback and the reply
    // says so. The `["deploy"]` above is therefore not the defect #157 names — it is the
    // fallback, now labelled. The edge-aware door holds nothing here; see
    // `pause_holds_only_edge_eligible_candidates`, which runs this exact graph and
    // fixture through `pause --file`.
    assert_eq!(pause_data["heldNodesGated"], serde_json::json!(false));
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

/// #158, and the reason it is not a display quirk. `resume` published `"lastEventAt": null` and an
/// empty `"nodeLastEventAt"` on a run that had just re-dispatched four nodes and appended events,
/// while `execution status` read the same store correctly a second later.
///
/// The cause is not a stale snapshot: `resume` passed `Liveness::default()`, so those two fields
/// were never measured on this path and could not have been non-null on any run. `status`, `list`,
/// `amend` measure with `Liveness::measured`, and `start` -- which also mutates -- reads the store.
///
/// The line this pins is the one written on `Liveness::from_store`: the silence BUDGET stays
/// unmeasured on a mutation reply, because judging it needs a clock reading and a declared bound,
/// but the INSTANT is "a fact already sitting in the log", and *a command that appended to the
/// store can honestly report when the store last moved*.
///
/// **The `status` read is a CONTROL, not decoration.** A null `lastEventAt` is equally well
/// explained by a store holding no content events at all, so without it these assertions would
/// pass for the wrong reason on the day the arrangement stops appending.
#[test]
fn resume_reports_when_the_store_last_moved() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let fixtures = fixtures_file(
        directory.path(),
        serde_json::json!({"implementation": "failure", "deploy": "success"}),
    );
    start(&events, &fixtures, "supervised", "exec_resume_liveness");
    pause(&events, "exec_resume_liveness");

    let resume_data = resume(&events, &fixtures, "exec_resume_liveness");
    let status_data = status(&events, "exec_resume_liveness");

    assert!(
        status_data["lastEventAt"].is_string(),
        "CONTROL FAILED: the command that measures sees no content events either, so this fixture cannot say anything about resume's reply: {status_data}"
    );

    assert!(
        resume_data["lastEventAt"].is_string(),
        "resume appended to this store, so it can report when the store last moved: it published {} while status read {} from the same store",
        resume_data["lastEventAt"],
        status_data["lastEventAt"]
    );
    assert!(
        resume_data["nodeLastEventAt"]
            .as_object()
            .is_some_and(|per_node| !per_node.is_empty()),
        "the per-node instants are empty on a reply whose own nodeStateCounts report nodes that moved: {resume_data}"
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
    // #157: this helper passes no `--file`, so the hold is the bare-state fallback and the reply
    // says so. The `["deploy"]` above is therefore not the defect #157 names — it is the
    // fallback, now labelled. The edge-aware door holds nothing here; see
    // `pause_holds_only_edge_eligible_candidates`, which runs this exact graph and
    // fixture through `pause --file`.
    assert_eq!(pause_data["heldNodesGated"], serde_json::json!(false));

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

/// #1083 (Codex on PR #1091): `status --execution <unknown> --html <path>` wrote the snapshot of an
/// empty fold BEFORE the unknown-id refusal, overwriting whatever incident page already stood at
/// `<path>`. The refusal now comes first: the existing file is byte-identical afterwards, a
/// missing file is never created, and the command refuses with `GHCLI028_EXECUTION_NOT_FOUND`.
/// The control is the same path under an execution that exists: it IS written, so a status that
/// never wrote anything would fail here too.
#[test]
fn status_html_for_an_unknown_execution_refuses_before_touching_the_snapshot() {
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
            "--execution",
            "exec-known-html",
        ])
        .assert()
        .success();

    let html = directory.path().join("incident.html");
    let existing = b"<html>the incident page an operator already saved</html>\n".to_vec();
    std::fs::write(&html, &existing).unwrap();

    let status_html = |execution: &str, target: &std::path::Path| {
        command()
            .args([
                "execution",
                "status",
                "--events",
                events.to_str().unwrap(),
                "--execution",
                execution,
                "--html",
                target.to_str().unwrap(),
            ])
            .output()
            .unwrap()
    };

    let refused = status_html("exec-typo-html", &html);
    assert!(!refused.status.success(), "{refused:?}");
    let refusal = json(&refused.stdout);
    assert_eq!(refusal["ok"], false, "{refusal}");
    assert_eq!(refusal["command"], "execution.status", "{refusal}");
    assert_eq!(
        refusal["diagnostics"][0]["code"], "GHCLI028_EXECUTION_NOT_FOUND",
        "{refusal}"
    );
    assert_eq!(
        std::fs::read(&html).unwrap(),
        existing,
        "a refused status must leave the existing snapshot byte-identical"
    );

    let never = directory.path().join("never.html");
    assert!(!status_html("exec-typo-html", &never).status.success());
    assert!(
        !never.exists(),
        "a refused status must not create a snapshot file"
    );

    let written = status_html("exec-known-html", &html);
    assert!(written.status.success(), "{written:?}");
    assert_ne!(
        std::fs::read(&html).unwrap(),
        existing,
        "the control: an execution that exists does write its snapshot"
    );
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

/// The exact persisted bytes of every journal file, in stable path order.
///
/// `journal_events` intentionally decodes JSON for semantic assertions. It is not a byte-level
/// witness because decoding removes whitespace and key order. Refusal tests that promise no
/// append use this helper instead, so the assertion observes the files on disk exactly as they
/// were written.
fn journal_bytes(events: &std::path::Path) -> Vec<(std::path::PathBuf, Vec<u8>)> {
    journal_paths(events)
        .into_iter()
        .map(|path| {
            let bytes = std::fs::read(&path).expect("journal bytes readable");
            (path, bytes)
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

/// #1063: a start records what the run is FOR and who was going to run it, on the declared
/// form. `name` is the document's `metadata.name`; `objective` is the first entrypoint's own
/// objective (the operator's words, where the Studio's draft keeps them); the CLI's `start` is
/// fixture-driven and says so. A briefing derived from the store alone reads these back.
#[test]
fn a_start_records_the_name_the_objective_and_the_executor_on_the_declared_form() {
    let directory = tempfile::tempdir().unwrap();
    let events = start_once(
        directory.path(),
        "examples/graphs/manual-override-deploy.yaml",
    );
    let (_, _, declared) = journal_events(&events)
        .into_iter()
        .find(|(_, kind, _)| kind == "execution_form_declared")
        .expect("a start must declare the shape of the execution");

    assert_eq!(declared["name"], "Deploy com override manual");
    assert_eq!(
        declared["objective"], "Produzir build implantável.",
        "the FIRST entrypoint's objective, verbatim: {declared}"
    );
    assert_eq!(declared["executor"], "fixture");
}

fn briefing(events: &Path, execution: &str) -> Value {
    let output = command()
        .args([
            "execution",
            "briefing",
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
    let envelope = json(&output.stdout);
    assert_eq!(envelope["command"], "execution.briefing", "{envelope}");
    envelope["data"].clone()
}

/// #1063: the resume briefing, read from the store alone after start -> approve -> pause on the
/// two-node example with a failure fixture. Each section names what happened: the objective
/// declared at start, the decisions IN ORDER with the actor that made them, the pending reason
/// while it existed, and the next step naming `resume` once the run is held.
#[test]
fn the_briefing_reports_the_decisions_in_order_with_actors_and_names_resume_when_held() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let execution = "exec-briefing";
    let blocking = fixtures_file(
        directory.path(),
        serde_json::json!({ "implementation": "failure" }),
    );
    start(&events, &blocking, "supervised", execution);

    // BEFORE the approval: the blocked node is the pending reason, and the next step is the
    // verb that answers it. Asserted here because the story resolves it two lines down, and
    // parity about an empty `pending` proves nothing.
    let blocked = briefing(&events, execution);
    assert_eq!(blocked["name"], "Deploy com override manual", "{blocked}");
    assert_eq!(blocked["objective"], "Produzir build implantável.");
    assert_eq!(blocked["executor"], "fixture");
    assert_eq!(
        blocked["pending"][0],
        serde_json::json!({"kind": "blocked_node", "node": "implementation"}),
        "{blocked}"
    );
    assert_eq!(
        blocked["nextStep"],
        serde_json::json!({"kind": "answer", "node": "implementation", "remedy": "approve"}),
        "{blocked}"
    );
    assert_eq!(
        blocked["decisions"],
        serde_json::json!([]),
        "a fixture failure is an outcome, not a decision: {blocked}"
    );

    approve(&events, execution, "implementation");
    pause(&events, execution);

    let held = briefing(&events, execution);
    let decisions: Vec<(String, String, String)> = held["decisions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|decision| {
            (
                decision["kind"].as_str().unwrap().to_owned(),
                decision["actor"]["id"].as_str().unwrap().to_owned(),
                decision["actor"]["type"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert_eq!(
        decisions,
        vec![
            (
                "approval".to_owned(),
                "owner-cli".to_owned(),
                "owner".to_owned()
            ),
            (
                "paused".to_owned(),
                "owner-cli".to_owned(),
                "owner".to_owned()
            ),
        ],
        "{held}"
    );
    assert_eq!(held["decisions"][0]["node"], "implementation");
    assert!(
        held["decisions"][0]["sequence"].as_u64().unwrap()
            < held["decisions"][1]["sequence"].as_u64().unwrap(),
        "ordered by sequence: {held}"
    );
    assert_eq!(held["nextStep"]["kind"], "resume_held", "{held}");
    let command = held["nextStep"]["command"].as_str().unwrap();
    assert!(
        command.contains("execution resume") && command.contains(execution),
        "the next step names resume and the execution: {command}"
    );
    assert_eq!(
        held["nextStep"]["nodes"],
        serde_json::json!(["deploy", "implementation"]),
        "the held nodes: {held}"
    );
    assert_eq!(held["pending"], serde_json::json!([]), "{held}");
    assert_eq!(held["workDone"], serde_json::json!([]), "{held}");

    // The hash a reader verifies the graph file against is the one the start recorded.
    let (_, _, started) = journal_events(&events)
        .into_iter()
        .find(|(_, kind, _)| kind == "execution_started")
        .unwrap();
    assert_eq!(held["graphHash"], started["graphHash"]);
    assert_eq!(held["graphVersion"], started["graphVersion"]);
    assert_eq!(
        held["asOfSequence"],
        serde_json::json!(status_head(&events, execution).unwrap()),
        "folded at the head status reports"
    );
    // The status view carries the same declared executor, so a glance says what a briefing says.
    assert_eq!(status(&events, execution)["executor"], "fixture");
}

/// #1063: `execution briefing` refuses exactly as `execution status` does when no stream can be
/// selected - same code, same pointer - under its OWN command name, so a caller reading the
/// envelope knows which verb refused.
#[test]
fn briefing_without_an_execution_refuses_with_statuss_own_code_under_its_own_name() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let refusal = |verb: &str| -> Value {
        let output = command()
            .args(["execution", verb, "--events", events.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(
            !output.status.success(),
            "{verb} must refuse an empty store"
        );
        json(&output.stdout)
    };
    let status = refusal("status");
    let briefing = refusal("briefing");
    assert_eq!(briefing["ok"], false);
    assert_eq!(briefing["command"], "execution.briefing", "{briefing}");
    assert_eq!(briefing["data"], Value::Null);
    assert_eq!(
        briefing["diagnostics"][0]["code"], status["diagnostics"][0]["code"],
        "the same refusal as status: {briefing} vs {status}"
    );
    assert_eq!(
        briefing["diagnostics"][0]["code"],
        "GHE010_STREAM_SELECTION_REQUIRED"
    );
    assert_eq!(
        briefing["diagnostics"][0]["path"],
        status["diagnostics"][0]["path"]
    );
}

/// #1063: a held start drives nothing, so it declares no executor - whoever resumes it (the CLI
/// with fixtures, or `serve` with the gateway) decides that later, and `resume` re-declares
/// nothing. A `fixture` written here would be repeated by every briefing for work the gateway
/// actually did.
#[test]
fn a_held_start_declares_no_executor() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let graph = root().join("examples/graphs/manual-override-deploy.yaml");
    let fixtures = all_success_fixtures(directory.path());
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
            "--held",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let (_, _, declared) = journal_events(&events)
        .into_iter()
        .find(|(_, kind, _)| kind == "execution_form_declared")
        .unwrap();
    assert_eq!(declared["name"], "Deploy com override manual");
    assert!(
        declared.get("executor").is_none(),
        "a held start declares no executor: {declared}"
    );
}

/// #1063: a name that LOOKS like a secret is left off the declaration, not written to the log
/// and not a refused start. The store's durable-content scan is the rule every append is held
/// to; before this field existed such a graph started, and it still does - the briefing reads
/// its name as absent.
#[test]
fn a_secret_shaped_name_is_omitted_from_the_declaration_and_the_start_proceeds() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let source =
        std::fs::read_to_string(root().join("examples/graphs/manual-override-deploy.yaml"))
            .unwrap();
    let token = format!("sk-{}", "a".repeat(40));
    let rewritten = source.replace(
        "  name: Deploy com override manual\n",
        &format!("  name: Rotate the key {token} today\n"),
    );
    assert_ne!(rewritten, source, "the example's name line must be found");
    let graph = directory.path().join("secret-name.yaml");
    std::fs::write(&graph, rewritten).unwrap();
    let fixtures = all_success_fixtures(directory.path());

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
            "--execution",
            "exec-secret-name",
        ])
        .output()
        .unwrap();
    let reply = json(&output.stdout);
    assert_eq!(reply["ok"], true, "the start proceeds: {reply}");
    let (_, _, declared) = journal_events(&events)
        .into_iter()
        .find(|(_, kind, _)| kind == "execution_form_declared")
        .unwrap();
    assert!(
        declared.get("name").is_none(),
        "the secret-shaped name never reaches the log: {declared}"
    );
    assert_eq!(
        declared["objective"], "Produzir build implantável.",
        "the safe field is still declared"
    );
    let briefing = briefing(&events, "exec-secret-name");
    assert_eq!(briefing["name"], Value::Null);
    let journal = std::fs::read_dir(&events).unwrap().count();
    assert!(journal > 0);
    assert!(
        !std::fs::read_to_string(
            journal_paths(&events)
                .first()
                .expect("a journal was written")
        )
        .unwrap()
        .contains(&token),
        "the token is in no journal line"
    );
}

fn journal_paths(events: &Path) -> Vec<PathBuf> {
    fn find(directory: &Path, found: &mut Vec<PathBuf>) {
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
    journals
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

/// Issue #89's guard, per the ratified Option B of the mode-semantics note (#89): `mode`
/// governs `decide_mutation`'s verdict ONLY (`core/governor/src/inflight.rs:112-118`, already
/// covered directly by that crate's own unit tests); dispatch is a separate axis this test
/// asserts is untouched by it. Nothing in `apps/cli/tests/` previously ran a graph in `manual`
/// mode at all — every existing dispatch-shaped test used `autopilot` or (once) `supervised` —
/// so a future change that made `ready_set`/the driver mode-sensitive had nothing here to catch
/// it. This is that catch: the SAME two-node all-success fixture graph, started fresh under each
/// of the three modes, must reach the identical terminal state regardless — a sabotage that made
/// `manual` (or `supervised`) hold dispatch would fail this by leaving a node non-terminal.
///
/// Scope, named rather than implied (D's review): this graph never proposes a mutation while
/// running, so it proves dispatch invariance only in the ABSENCE of an in-flight proposal. A
/// mode-sensitive dispatch bug that manifests only while a mutation is pending would pass this
/// unchanged — that case is outside #89's scope, not covered here, and not claimed to be.
#[test]
fn dispatch_completes_identically_regardless_of_mode() {
    let directory = tempfile::tempdir().unwrap();
    let fixtures = all_success_fixtures(directory.path());

    let mut by_mode = std::collections::BTreeMap::new();
    for mode in ["autopilot", "supervised", "manual"] {
        let events = directory.path().join(format!("events-{mode}"));
        let execution = format!("exec-mode-{mode}");
        let data = start(&events, &fixtures, mode, &execution);
        assert_eq!(
            data["status"], "completed",
            "{mode}: dispatch must complete this graph exactly as autopilot does — a mode that \
             holds dispatch would leave this short of completed: {data}"
        );
        assert_eq!(
            data["nodeStateCounts"]["succeeded"], 2,
            "{mode}: both nodes must have actually run and succeeded, not merely been \
             approved: {data}"
        );
        by_mode.insert(mode, data["nodeStateCounts"].clone());
    }

    let autopilot_counts = &by_mode["autopilot"];
    for mode in ["supervised", "manual"] {
        assert_eq!(
            &by_mode[mode], autopilot_counts,
            "{mode}'s dispatch outcome must be byte-identical to autopilot's — any difference \
             here is dispatch depending on mode, which #89's contract says must never happen"
        );
    }
}

/// #775: the `execution` SURFACE must stay scope-fixed, not just `start`.
///
/// `start` is the only path that creates a stream today, and it takes the workspace and the
/// project from constants -- so two streams cannot share an id under different scopes, and the
/// readers that locate a stream by id alone cannot reach a second candidate. That is the whole of
/// #775's reachability answer, and it rests on nothing but a function not being parameterised.
///
/// **The risk is per-SURFACE and an earlier version of this cell was per-VERB** (found by a peer
/// reviewing this change). Naming `start` explicitly protects the verb that creates streams today
/// and says nothing about a verb added tomorrow -- and a new verb is exactly the way this gets
/// reopened, because whoever adds it will not remember a cell they never read. So the subcommands
/// are ENUMERATED from the binary's own help rather than listed here.
///
/// The CONTROLS are two, because an enumerating cell has two ways to be vacuous: parse nothing and
/// assert over an empty set, or parse a set that has silently shrunk. Both are checked.
#[test]
fn no_execution_verb_takes_scope_arguments() {
    let listing = command()
        .args(["execution", "--help"])
        .output()
        .expect("the binary runs");
    let help = String::from_utf8_lossy(&listing.stdout);
    let verbs: Vec<String> = help
        .lines()
        .skip_while(|line| !line.starts_with("Commands:"))
        .skip(1)
        .take_while(|line| !line.trim().is_empty())
        .filter_map(|line| line.split_whitespace().next())
        .filter(|verb| *verb != "help")
        .map(ToOwned::to_owned)
        .collect();

    // CONTROL 1: the parse found something. An empty list would satisfy every assertion below.
    assert!(
        verbs.len() >= 8,
        "CONTROL: only {} verbs parsed out of `execution --help`; the format changed and this cell \
         is now asserting over almost nothing: {verbs:?}",
        verbs.len()
    );
    // CONTROL 2: it found the ones we know about. A parse that captured the wrong column would
    // still be non-empty.
    for known in ["start", "status", "pause", "cancel"] {
        assert!(
            verbs.iter().any(|verb| verb == known),
            "CONTROL: `{known}` is missing from the parsed verbs, so the enumeration is reading \
             something other than the command list: {verbs:?}"
        );
    }

    for verb in &verbs {
        for scope_argument in ["--workspace", "--project"] {
            let refused = command()
                .args(["execution", verb, scope_argument, "anything"])
                .output()
                .expect("the binary runs");
            let complaint = String::from_utf8_lossy(&refused.stderr).to_lowercase();
            // The SPECIFIC refusal. An earlier version asserted only a non-zero exit, and every
            // invocation was already failing on a missing required argument -- so it stayed green
            // under a sabotage that really did add `--workspace` to the verb.
            assert!(
                complaint.contains("unexpected argument") && complaint.contains(scope_argument),
                "`execution {verb}` did not reject {scope_argument} as an unknown argument. A \
                 stream can now be created or addressed under a caller-chosen scope, which makes \
                 #775's same-id collision reachable through the product -- settle the readers \
                 before this ships. It said: {}",
                String::from_utf8_lossy(&refused.stderr)
            );
        }
    }
}

struct WallClock;
impl graphhelm_protocols::Clock for WallClock {
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        chrono::Utc::now()
    }
}

#[derive(Default)]
struct Ids(std::sync::atomic::AtomicU64);
impl graphhelm_protocols::IdGenerator for Ids {
    fn next_id(&self, prefix: &'static str) -> String {
        format!(
            "{prefix}-scope-{}",
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1
        )
    }
}

fn open_store(events: &Path) -> graphhelm_events::LocalEventRepository {
    graphhelm_events::LocalEventRepository::open(
        events,
        std::sync::Arc::new(WallClock),
        std::sync::Arc::new(Ids::default()),
    )
    .unwrap()
}

/// The scope `addressable_scope` derives for `execution`, spelled out by hand.
///
/// Deliberately NOT built from `WORKSPACE`/`PROJECT`: an expectation computed from the constants
/// under test agrees with whatever rule the code happens to implement, which is a mirror rather
/// than an oracle. These two literals are the addressing rule, and changing them IS a change to
/// it -- so a rename should turn this red and be re-decided, not silently followed.
fn scope_the_id_derives(execution: &str) -> graphhelm_protocols::RepositoryScope {
    graphhelm_protocols::RepositoryScope::new(
        graphhelm_protocols::WorkspaceId::parse("workspace-local").unwrap(),
        graphhelm_protocols::ProjectId::parse("project-local").unwrap(),
        Some(graphhelm_protocols::ExecutionId::parse(execution).unwrap()),
    )
}

/// #775: stream ids are unique per REPOSITORY, and the reason is that nothing ever chooses a scope.
///
/// `addressable_scope` (`apps/cli/src/commands/execution/mod.rs:196`) does not select a scope for
/// an id, it DERIVES one from it: constant workspace, constant project, and the execution id
/// itself as the third component. A function cannot return two scopes for one id, so no two
/// streams it produces can share an id -- and every production append to this store takes its
/// scope from it, directly at `start.rs:153` or through `resolve_stream`/`load_projection` at
/// `signal.rs:162` and `amend.rs:62`.
///
/// **That is the entire reason the id-only `find`s in `serve/monitor.rs:597` and
/// `serve/wake.rs:95,213` resolve the right execution.** It held as a consequence and was written
/// down nowhere. This asserts it over the streams the SHIPPED VERBS create.
///
/// **WHAT THIS CELL PINS, EXACTLY: the scope derivation on the `start` path.** It invokes one
/// verb, twice, against a fresh store. A SEVENTH write site that chose its own scope would never
/// be invoked here, its streams would never exist, and this loop would iterate two still-correct
/// entries and pass. So this does not redden for "any writer picks its own scope" -- an earlier
/// draft of this comment claimed it did, which is a green cell being read as coverage it does not
/// have.
///
/// The structural half -- that `start`, `signal`, `amend` and the driver all reach
/// `addressable_scope`, and that `events::scope` has only read-path callers -- is an argument in
/// the PR body, not an assertion here, and nothing re-reads it when a site is added. That gap is
/// filed rather than papered over. What holds today is that the derivation above is the ONLY
/// reason those three
/// readers answer about a different execution than the `/v1` verbs do.
#[test]
fn every_stream_the_shipped_verbs_create_is_scoped_by_its_own_id() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let fixtures = fixtures_file(
        directory.path(),
        serde_json::json!({ "implementation": "success", "deploy": "success" }),
    );

    // Two, not one: a single stream cannot show that two ids stay apart.
    start(&events, &fixtures, "supervised", "exec_alpha");
    start(&events, &fixtures, "supervised", "exec_beta");

    let streams = open_store(&events).list_streams().unwrap();
    let ids: Vec<&str> = streams.iter().map(|s| s.stream_id.as_str()).collect();
    assert_eq!(
        ids.len(),
        2,
        "ARRANGEMENT: the two starts did not leave two streams, so nothing below is measured: {ids:?}"
    );

    for stream in &streams {
        assert_eq!(
            stream.scope,
            scope_the_id_derives(&stream.stream_id),
            "stream `{}` is scoped by something other than its own id, so `(scope, stream_id)` now \
             carries more than `stream_id` and an id-only `find` can reach the wrong execution",
            stream.stream_id
        );
    }
}

/// Every event's kind label paired with the actor id that wrote it, in stream order.
///
/// `raw_outcomes` above reads only `NodeOutcomeRecorded`, so reusing it for an `ExecutionPaused`
/// question would filter to an empty set and assert nothing — a green with no subject in it. This
/// reads every envelope and labels it, so one call answers about two different kinds and the two
/// halves below cannot drift apart between reads.
fn event_actors(events: &Path) -> Vec<(String, String)> {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    // FIXED, not `Utc::now()`. This reader inspects only actor ids, so a wall clock would not
    // change today's assertions -- and that is exactly the argument that lets a nondeterministic
    // dependency sit in a test until something later starts consulting it. `AGENTS.md`: "Use fixed
    // clock and ID implementations in tests." The three sibling readers in this file still take
    // `Utc::now()`; copying one of them is how this got here, and copying a violation propagates
    // it. (Codex P1 on this PR, and it is right.)
    struct TestClock;
    impl graphhelm_protocols::Clock for TestClock {
        fn now(&self) -> chrono::DateTime<chrono::Utc> {
            chrono::DateTime::from_timestamp(1_700_000_000, 0).expect("constant timestamp is valid")
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
        .filter_map(|envelope| {
            let kind = match &envelope.kind {
                graphhelm_protocols::EventKind::ExecutionPaused(_) => "ExecutionPaused",
                graphhelm_protocols::EventKind::ExecutionStarted(_) => "ExecutionStarted",
                _ => return None,
            };
            Some((kind.to_string(), envelope.actor.id().to_string()))
        })
        .collect()
}

/// #90: the hold that `--held` writes is attributed to the operator who asked for it, and the
/// `start` it rides in is not.
///
/// `start --held` emits an `ExecutionPaused` — the same kind `pause` emits, which `pause.rs` writes
/// as `owner_actor()`. `owner_actor`'s doc draws the line ("every explicit decision is recorded as
/// the owner's") and lists `pause` among owner-initiated commands. Before the fix this event
/// carried `system_actor()` on the CLI path, so the log could not tell an operator's hold from the
/// driver's own bookkeeping — and an event log is append-only, so a wrong actor is not repaired
/// later, only regretted.
///
/// BOTH HALVES ARE IN ONE ASSERTION SET, AND THE SECOND IS THE ONE THAT DOES THE WORK. Asserting
/// only that the hold is the owner's would be satisfied by a "fix" that attributed EVERY event in
/// this start to the owner — which would make the whole log wrong while turning this cell green.
/// `ExecutionStarted` staying `system-cli` is what excludes that, and it is the same shape as the
/// release/ordinary-hop pair asserted for `NodeOutcomeRecorded` further down this file.
#[test]
fn a_held_start_records_the_hold_as_the_operators_decision_and_the_start_as_the_systems() {
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
            "exec_hold_actor",
            "--held",
        ])
        .output()
        .unwrap();
    assert!(
        start.status.success(),
        "ARRANGEMENT: the held start must succeed, or both assertions below hold vacuously: {}",
        String::from_utf8_lossy(&start.stdout)
    );

    let actors = event_actors(&events);

    // HALF ONE: the hold is the operator's act — nobody reached it by a driver hop, someone typed
    // `--held`.
    let hold = actors
        .iter()
        .find(|(kind, _)| kind == "ExecutionPaused")
        .unwrap_or_else(|| panic!("a held start must record an ExecutionPaused: {actors:?}"));
    assert_eq!(
        hold.1, "owner-cli",
        "holding an execution is the operator's decision and the log must say so: {actors:?}"
    );

    // HALF TWO, in the same set so the split is tested rather than assumed: the `start` this hold
    // rides in stays the system's, exactly as `start.rs`'s own doc argues.
    let started = actors
        .iter()
        .find(|(kind, _)| kind == "ExecutionStarted")
        .unwrap_or_else(|| panic!("a start must record an ExecutionStarted: {actors:?}"));
    assert_eq!(
        started.1, "system-cli",
        "the start itself is not an owner-initiated command: {actors:?}"
    );
}
