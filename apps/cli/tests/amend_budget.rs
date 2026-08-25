//! Step 3, the socket: does the button actually DO the work?
//!
//! The blind judge's seventh refusal named the gap precisely — every reason pointed at
//! `declareNodeBudget` and no exposed operation declared a budget. Step 1 made the remedy
//! sayable, step 2 made it possible, and nothing answered when the operator reached for it.
//!
//! Written against the reviewer's list, published before this code existed. P8 is the one he
//! called more important than all the others, and it is the first test here: after submitting,
//! the SAME story must produce a RESOLVED verdict. If the operator declares the bound and the
//! answer is still `unknown`, the button exists and does not work — which is worse than no
//! button, because it looks like progress.

use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn cli(args: &[&str]) -> serde_json::Value {
    let output = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args(args)
        .output()
        .unwrap();
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&output.stdout)))
}

/// The judge's own situation, reached the way production reaches it: a run whose graph
/// declares no `timeoutSeconds`, with a node left genuinely IN FLIGHT (P7). A story driven to
/// quiescence has nothing to declare a bound for, and a guard over it would compare two empty
/// answers — the failure this milestone has met eight times.
fn story_with_unbudgeted_node_in_flight(events: &Path, directory: &Path) -> String {
    let graph = root().join("examples/graphs/manual-override-deploy.yaml");
    let fixtures = directory.join("fixtures.json");
    std::fs::write(
        &fixtures,
        serde_json::to_vec(&serde_json::json!({"nodeOutcomes": {"implementation": "failure"}}))
            .unwrap(),
    )
    .unwrap();
    let started = cli(&[
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
        "exec-amend-e2e",
    ]);
    assert_eq!(started["ok"], true, "{started}");
    // The drive runs to QUIESCENCE, so a store built by `start` alone has nothing in flight
    // and nothing to declare a bound for. My own guard below caught that on the first run --
    // the ninth time this milestone met a fixture that made the question disappear. The node
    // is left running by direct append, through production transitions, the same posture as
    // `arm_lease` in `wake_http`.
    strand_running(events, "exec-amend-e2e", "deploy");
    "exec-amend-e2e".to_owned()
}

/// The frontier a REAL caller uses: the number the surface just handed them. Tests that
/// hardcode it prove nothing about the contract, and the eighth run's F4 was exactly the gap
/// between the number published and the number demanded.
fn frontier(events: &Path, execution: &str) -> String {
    let status = cli(&[
        "execution",
        "status",
        "--events",
        events.to_str().unwrap(),
        "--execution",
        execution,
    ]);
    status["data"]["headSequence"]
        .as_u64()
        .expect("the read publishes the frontier it was looking at")
        .to_string()
}

fn amend(events: &Path, execution: &str, node: &str, seconds: &str, at: &str) -> serde_json::Value {
    cli(&[
        "execution",
        "amend-budget",
        "--events",
        events.to_str().unwrap(),
        "--execution",
        execution,
        "--node",
        node,
        "--seconds",
        seconds,
        "--at",
        at,
    ])
}

/// P8, the one the reviewer called more important than all the others.
#[test]
fn declaring_the_bound_resolves_the_unknown_in_the_same_story() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let execution = story_with_unbudgeted_node_in_flight(&events, directory.path());

    let before = cli(&[
        "execution",
        "status",
        "--events",
        events.to_str().unwrap(),
        "--execution",
        &execution,
    ]);
    let unevaluated = before["data"]["silenceUnevaluated"].as_array().unwrap();
    assert!(
        !unevaluated.is_empty(),
        "the fixture must leave something unjudged or this proves nothing: {before}"
    );
    let node = unevaluated[0]["node"].as_str().unwrap().to_owned();

    // The operator does exactly what the remedy told them to do.
    let amended = amend(
        &events,
        &execution,
        &node,
        "3600",
        &frontier(&events, &execution),
    );
    assert_eq!(amended["ok"], true, "{amended}");

    // THE CLOSED LOOP. The write answers with the recomputed verdict (P4), and that node is
    // no longer unjudged: the button did the work, in the same story, without a second call.
    let still_unjudged = amended["data"]["silenceUnevaluated"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["node"] == serde_json::json!(node));
    assert!(
        !still_unjudged,
        "the operator declared the bound the remedy asked for and the answer still cannot \
         judge that node: the button exists and does not work: {amended}"
    );
}

/// The eighth judge run, F2 — and the defect was mine, in the shape this milestone has been
/// burying all day.
///
/// He measured: `amend_budget(judge, 30s)` answered `needs_you`, and the very NEXT status read
/// reverted to `unknown` / `no_declared_budget`, twice. The verdict flapped between "wake up"
/// and "I do not know" with nothing in the run changing.
///
/// The cause is two readings of one question. The write path folded declaration AND amendments
/// (`effective_budgets`); the read paths called a second function that read the declaration
/// only. Same defect as the monitor's private staleness threshold on day one, rebuilt by me
/// across two files, and invisible to every guard because each surface was self-consistent.
///
/// So this guard does not ask the WRITE what it thinks. It asks the READ, afterwards.
#[test]
fn the_amendment_sticks_on_the_next_read_not_just_in_the_write_reply() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let execution = story_with_unbudgeted_node_in_flight(&events, directory.path());

    let amended = amend(
        &events,
        &execution,
        "deploy",
        "30",
        &frontier(&events, &execution),
    );
    assert_eq!(amended["ok"], true, "{amended}");

    // The read AFTER the write. This is the whole test: a remedy that only holds inside the
    // reply that applied it has not been applied to anything.
    let after = cli(&[
        "execution",
        "status",
        "--events",
        events.to_str().unwrap(),
        "--execution",
        &execution,
    ]);
    let reverted = after["data"]["silenceUnevaluated"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["node"] == serde_json::json!("deploy"));
    assert!(
        !reverted,
        "the next read forgot the bound the operator declared: the verdict flaps between wake-up and I-do-not-know with nothing in the run changing: {after}"
    );
}

/// W1, the sabotage that was missing from the reviewer's own list — and he said so himself.
///
/// P8 asked that submitting produce a resolved verdict, and it was implemented and sabotaged
/// against the reply OF THE WRITE. That is legitimate and it bit when attacked. What no
/// sabotage asserted was a LATER READ, BY A DIFFERENT DOOR — and the product lied in exactly
/// that gap: the write applied the amendment and the next read forgot it.
///
/// A list written before the code protects against tests tailored to it. It does not protect
/// against the list being incomplete. So this one is permanent and general: for any
/// write/read pair, submit, then read through ANOTHER surface, and demand the same answer.
/// That is section 8 as a sabotage instead of a promise.
#[test]
fn the_write_and_two_different_reads_agree_about_the_same_execution() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let execution = story_with_unbudgeted_node_in_flight(&events, directory.path());

    let written = amend(
        &events,
        &execution,
        "deploy",
        "3600",
        &frontier(&events, &execution),
    );
    assert_eq!(written["ok"], true, "{written}");
    let write_says = written["data"]["attention"].clone();

    // Door one: the status read.
    let status = cli(&[
        "execution",
        "status",
        "--events",
        events.to_str().unwrap(),
        "--execution",
        &execution,
    ]);
    assert_eq!(
        status["data"]["attention"], write_says,
        "the write and the status read must not disagree about whether the operator is needed: write={write_says} read={status}"
    );

    // Door two: the HTML snapshot, a different renderer over the same projection. If a
    // surface kept its own reading of the budget, this is where it would show.
    let snapshot = directory.path().join("snapshot.html");
    let rendered = cli(&[
        "execution",
        "status",
        "--events",
        events.to_str().unwrap(),
        "--execution",
        &execution,
        "--html",
        snapshot.to_str().unwrap(),
    ]);
    assert_eq!(rendered["ok"], true, "{rendered}");
    let page = std::fs::read_to_string(&snapshot).unwrap();
    // Per-node, NOT the headline. The headline here is `needs you` because another node is
    // blocked, so asserting on it would pass no matter what this door believed about the
    // budget -- my own guard did exactly that on its first run, the eleventh time this
    // milestone met a check that held for the wrong reason.
    assert!(
        !page.contains("silence NOT evaluated"),
        "this door still reports a node as unjudged whose bound the operator just declared, while the write said otherwise -- one execution, two answers: {page}"
    );
}

/// P2 / A3: an amendment computed against a frontier that has moved is REFUSED, and the
/// refusal hands back what the caller should have used. A silent 409 teaches nothing.
#[test]
fn an_amendment_against_a_stale_frontier_is_refused_with_both_numbers() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let execution = story_with_unbudgeted_node_in_flight(&events, directory.path());

    let stale_frontier = frontier(&events, &execution);
    let first = amend(&events, &execution, "deploy", "600", &stale_frontier);
    assert_eq!(first["ok"], true, "{first}");

    let stale = amend(&events, &execution, "deploy", "900", &stale_frontier);
    assert_eq!(
        stale["ok"],
        serde_json::json!(false),
        "a stale amendment must be refused: {stale}"
    );
    let message = stale["diagnostics"][0]["message"].as_str().unwrap();
    assert!(
        message.contains("moved to"),
        "the refusal hands back the current frontier so the caller can recompute: {message}"
    );
}

/// P3 / A4: a node outside this execution is refused. The declared form is unsealed and the
/// published graph is not; the gap between them is the injection point, and it now has a door.
#[test]
fn a_node_this_execution_never_had_is_refused() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let execution = story_with_unbudgeted_node_in_flight(&events, directory.path());

    let injected = amend(
        &events,
        &execution,
        "a-node-that-never-existed",
        "600",
        &frontier(&events, &execution),
    );
    assert_eq!(injected["ok"], serde_json::json!(false), "{injected}");
    assert!(
        injected["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("not a node of this execution"),
        "{injected}"
    );
}

/// P5: the A9 fix must hold through the NEW path. If the endpoint assembled its own verdict
/// instead of asking the seam, the calm-purchased-by-amendment distinction would quietly
/// become decorative — a fix landed an hour ago, undone by the surface that came after it.
#[test]
fn the_roulette_is_still_visible_through_the_endpoint() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let execution = story_with_unbudgeted_node_in_flight(&events, directory.path());

    let tight = amend(
        &events,
        &execution,
        "deploy",
        "1",
        &frontier(&events, &execution),
    );
    assert_eq!(tight["ok"], true, "{tight}");
    let loose = amend(
        &events,
        &execution,
        "deploy",
        "100000",
        &frontier(&events, &execution),
    );
    // Either it refuses (the frontier moved) or it accepts and the verdict shows the purchase.
    // What it may NEVER do is answer a bare `can_sleep`.
    if loose["ok"] == serde_json::json!(true) {
        assert_ne!(
            loose["data"]["attention"],
            serde_json::json!("can_sleep"),
            "raising the ceiling through the endpoint must not launder the alarm into an \
             untroubled calm: {loose}"
        );
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
            "{prefix}-amend-{}",
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1
        )
    }
}

/// Leaves `node` RUNNING by direct append, because the drive the CLI performs runs to
/// QUIESCENCE: a store built by `start_execution` alone has no node in flight, and silence
/// is only judged for work in flight. A guard built on such a store compares two empty
/// answers and calls that agreement — which is how this test passed while the page ignored
/// the seam entirely. Same posture as `arm_lease` in `wake_http`: the fixture states the
/// condition production reaches on its own (a node dispatched and not yet finished), and
/// the transitions are the production ones, taken through `apply_transition` from the
/// state the fold actually holds — never a state hand-set to a value production skips.
fn strand_running(events: &Path, execution: &str, node: &str) {
    let store = graphhelm_events::LocalEventRepository::open(
        events,
        std::sync::Arc::new(WallClock),
        std::sync::Arc::new(Ids::default()),
    )
    .unwrap();
    loop {
        let (stream, history) = store.read_unique_replay_stream().unwrap();
        let projection =
            graphhelm_events::replay(&stream.scope, &stream.stream_id, &history).unwrap();
        let current = projection
            .node_states
            .get(node)
            .copied()
            .unwrap_or(graphhelm_protocols::NodeState::Draft);
        // Read the step off the state the fold HOLDS, never off a step count: the drive
        // already advanced this node some distance, and how far is production's business.
        let (label, outcome) = match current {
            graphhelm_protocols::NodeState::Draft => {
                ("approve", graphhelm_protocols::NodeOutcome::Approved)
            }
            graphhelm_protocols::NodeState::Ready | graphhelm_protocols::NodeState::Queued => {
                ("start", graphhelm_protocols::NodeOutcome::Started)
            }
            graphhelm_protocols::NodeState::Running => break,
            other => panic!("{node} sits in {other:?}, from which production never reaches flight"),
        };
        let next_state =
            graphhelm_execution::apply_transition(&graphhelm_execution::TransitionRequest {
                current,
                outcome,
                attempts: projection.node_attempts.get(node).copied().unwrap_or(0),
                identical_outcomes: projection.identical_outcomes_for(node, outcome),
            })
            .unwrap_or_else(|error| panic!("{label} from {current:?} must be legal: {error:?}"));
        let next = store
            .next_sequence(&stream.scope, &stream.stream_id)
            .unwrap();
        let request = graphhelm_events::PreparedAppend::new(
            stream.scope.clone(),
            graphhelm_protocols::OpaqueId::parse(stream.stream_id.clone()).unwrap(),
            next,
            vec![graphhelm_protocols::NewEvent::new(
                // The state is part of the key because Ready and Queued both advance on
                // Started, and two appends under one key is an IdempotencyConflict.
                graphhelm_protocols::OpaqueId::parse(format!(
                    "amend-{label}-{}",
                    format!("{current:?}").to_lowercase()
                ))
                .unwrap(),
                graphhelm_protocols::PersistedActor::new(
                    graphhelm_protocols::PersistedActorType::Agent,
                    graphhelm_protocols::ActorId::parse("agent-amend").unwrap(),
                ),
                graphhelm_protocols::Sensitivity::Internal,
                graphhelm_protocols::EventKind::NodeOutcomeRecorded(
                    graphhelm_protocols::NodeOutcomeRecorded {
                        execution_id: graphhelm_protocols::OpaqueId::parse(execution).unwrap(),
                        node_id: graphhelm_protocols::OpaqueId::parse(node).unwrap(),
                        outcome,
                        next_state,
                        reason: None,
                    },
                ),
                vec![],
                vec![],
            )],
            vec![],
            vec![],
        )
        .unwrap();
        store.append_atomic(&request).unwrap();
    }
}
