//! #386 (journey-first spec §7, §9 row F): `task.*` signals carry one `graphhelm-task-event-v1`
//! document and are recorded only by the actor they name. Observed through the real CLI
//! `execution signal` door, which shares its admission with the HTTP and MCP doors. The five
//! accepted documents are the package's own fixtures, so the schema and the admission cannot drift
//! apart unseen. Cost: one held fixture run and a few CLI calls in a tempdir; no network.

use std::path::{Path, PathBuf};
use std::sync::Arc;

/// #782: an external run end uses a stable key and the lane's identity; its request never
/// reaches the operator's real Runtime. Cost: two loopback requests in a temporary server.
#[test]
fn run_ended_posts_once_per_logical_run_and_fails_open() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    let scratch = tempfile::tempdir().unwrap();
    let token = scratch.path().join("agent.token");
    std::fs::write(&token, "a".repeat(64)).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&token, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let mut ids = Vec::new();
        while ids.len() < 2 {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                .unwrap();
            let mut bytes = Vec::new();
            let mut chunk = [0_u8; 4096];
            loop {
                let n = stream.read(&mut chunk).unwrap();
                bytes.extend_from_slice(&chunk[..n]);
                if bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let request = String::from_utf8_lossy(&bytes);
            // #1240: after the run end lands, the CLI reads the tasks route to release the lane's
            // open claims; this Runtime has none to serve.
            if request.starts_with("GET /v1/executions/gh-team/tasks") {
                write!(
                    stream,
                    "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                )
                .unwrap();
                continue;
            }
            assert!(request.contains("POST /v1/executions/gh-team/signal"));
            assert!(
                request
                    .to_ascii_lowercase()
                    .contains("x-graphhelm-actor: codex-x")
            );
            let headers_end = bytes.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
            let length = request
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .and_then(|n| n.trim().parse::<usize>().ok())
                })
                .unwrap();
            while bytes.len() - headers_end < length {
                let n = stream.read(&mut chunk).unwrap();
                bytes.extend_from_slice(&chunk[..n]);
            }
            let body: serde_json::Value =
                serde_json::from_slice(&bytes[headers_end..headers_end + length]).unwrap();
            assert_eq!(body["signal"]["type"], "agent_session_ended");
            assert_eq!(body["signal"]["source"]["id"], "codex-x");
            ids.push(body["signal"]["id"].as_str().unwrap().to_owned());
            let reply =
                b"{\"ok\":true,\"command\":\"execution.signal\",\"data\":{},\"diagnostics\":[]}";
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", reply.len()).unwrap();
            stream.write_all(reply).unwrap();
        }
        assert_eq!(ids[0], ids[1]);
    });
    for _ in 0..2 {
        Command::from_std(graphhelm_process::command(assert_cmd::cargo::cargo_bin!(
            "graphhelm"
        )))
        .args([
            "tasks",
            "run-ended",
            "--lane",
            "codex-x",
            "--exit",
            "0",
            "--run-id",
            "test-run",
            "--url",
            &url,
            "--token-file",
            token.to_str().unwrap(),
        ])
        .assert()
        .success();
    }
    server.join().unwrap();
    Command::from_std(graphhelm_process::command(assert_cmd::cargo::cargo_bin!(
        "graphhelm"
    )))
    .args([
        "tasks",
        "run-ended",
        "--lane",
        "codex-x",
        "--exit",
        "0",
        "--run-id",
        "other-run",
        "--url",
        &url,
        "--token-file",
        token.to_str().unwrap(),
    ])
    .assert()
    .success();
}

/// #1516: `tasks durations --since` asks the Runtime's tasks route for that window and prints the
/// reply's `durations` block; a `--since` that is not an instant is refused before any request.
/// Cost: one loopback request in a temporary server.
#[test]
fn durations_reads_the_since_window_and_prints_the_durations_block() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    let scratch = tempfile::tempdir().unwrap();
    let token = scratch.path().join("owner.token");
    std::fs::write(&token, "a".repeat(64)).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&token, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let (mut stream, request) = loop {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                .unwrap();
            let mut bytes = Vec::new();
            let mut chunk = [0_u8; 4096];
            while !bytes.windows(4).any(|w| {
                w == b"

"
            }) {
                match stream.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => bytes.extend_from_slice(&chunk[..n]),
                }
            }
            if !bytes.is_empty() {
                break (stream, String::from_utf8_lossy(&bytes).into_owned());
            }
        };
        let reply = b"{\"ok\":true,\"command\":\"execution.tasks\",\"data\":{\"durations\":{\"slices\":[],\"summary\":{\"total\":{\"count\":2,\"medianSeconds\":600,\"p90Seconds\":900}}}},\"diagnostics\":[]}";
        write!(
            stream,
            "HTTP/1.1 200 OK
Content-Type: application/json
Content-Length: {}
Connection: close

",
            reply.len()
        )
        .unwrap();
        stream.write_all(reply).unwrap();
        request
    });
    let run = |since: &str| {
        Command::from_std(graphhelm_process::command(assert_cmd::cargo::cargo_bin!(
            "graphhelm"
        )))
        .args([
            "tasks",
            "durations",
            "--since",
            since,
            "--url",
            &url,
            "--token-file",
        ])
        .arg(&token)
        .output()
        .unwrap()
    };
    let refused = run("yesterday");
    assert!(!refused.status.success(), "a bad --since is refused");
    let output = run("2026-10-15T00:00:00Z");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let reply: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(reply["command"], "tasks.durations");
    assert_eq!(
        reply["data"]["summary"]["total"]["medianSeconds"], 600,
        "{reply}"
    );
    let request = server.join().unwrap();
    assert!(
        request.starts_with("GET /v1/executions/gh-team/tasks?since=2026-10-15T00:00:00Z "),
        "{request}"
    );
}

use assert_cmd::Command;
use chrono::TimeZone;
use graphhelm_protocols::{EventEnvelope, EventKind};
use serde_json::{Value, json};

const RUN: &str = "task-events";
/// The actor the CLI's own `execution signal` records under.
const ACTOR: &str = "owner-cli";

struct FixedClock;
impl graphhelm_protocols::Clock for FixedClock {
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        chrono::Utc
            .with_ymd_and_hms(2026, 10, 10, 12, 0, 0)
            .unwrap()
    }
}
struct ReadOnlyIds;
impl graphhelm_protocols::IdGenerator for ReadOnlyIds {
    fn next_id(&self, _: &'static str) -> String {
        panic!("reading task history must not append")
    }
}

fn history(events: &Path) -> Vec<EventEnvelope> {
    let store = graphhelm_events::LocalEventRepository::open(
        events,
        Arc::new(FixedClock),
        Arc::new(ReadOnlyIds),
    )
    .unwrap();
    let stream = store
        .list_streams()
        .unwrap()
        .into_iter()
        .find(|stream| stream.stream_id.as_str() == RUN)
        .unwrap();
    store
        .read_replay_stream(&stream.scope, &stream.stream_id)
        .unwrap()
}

fn command() -> Command {
    let mut command = Command::from_std(graphhelm_process::command(assert_cmd::cargo::cargo_bin!(
        "graphhelm"
    )));
    command.env("GRAPHHELM_EVENTS_KEY", "01".repeat(32));
    command
}

fn write(directory: &Path, name: &str, value: &Value) -> PathBuf {
    let path = directory.join(name);
    std::fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
    path
}

fn start(scratch: &Path) -> PathBuf {
    let events = scratch.join("events");
    let keyring = scratch.join("keyring");
    std::fs::create_dir(&keyring).unwrap();
    graphhelm_sealed_key_provider::SealedKeyProvider::create(
        &keyring,
        "owner-key",
        graphhelm_events::SecretBytes::new(vec![1; 32]),
    )
    .unwrap();
    let fixtures = write(scratch, "fixtures.json", &json!({"nodeOutcomes":{}}));
    let graph = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/graphs/manual-override-deploy.yaml");
    let output = command()
        .args(["execution", "start", "--events"])
        .arg(&events)
        .args(["--execution", RUN, "--file"])
        .arg(&graph)
        .arg("--fixtures")
        .arg(&fixtures)
        .args(["--mode", "manual", "--held"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    events
}

fn signal(
    scratch: &Path,
    events: &Path,
    id: &str,
    kind: &str,
    source: &str,
    document: &Value,
) -> Value {
    let envelope = json!({"id":id,"source":{"type":"user","id":source},"type":kind,"severity":"low",
        "description":document.to_string(),"evidence":["task"],"emittedAt":"2026-10-07T23:00:00Z"});
    let path = write(scratch, &format!("{id}.json"), &envelope);
    let output = command()
        .args(["execution", "signal", "--events"])
        .arg(events)
        .args(["--execution", RUN, "--signal"])
        .arg(&path)
        .arg("--evidence-out")
        .arg(scratch.join(format!("{id}-evidence.json")))
        .arg("--keyring")
        .arg(scratch.join("keyring"))
        .args(["--key-id", "owner-key"])
        .output()
        .unwrap();
    serde_json::from_slice(&output.stdout).unwrap()
}

fn package_fixture(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../extensions/builtin/graphhelm-development-contracts/fixtures/task-event/valid/{name}.json"
    ));
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

/// The identity fields of the fixture, rewritten to the actor this CLI records under.
fn as_actor(mut document: Value) -> Value {
    for key in ["lane", "merger"] {
        if document.get(key).is_some() {
            document[key] = json!(ACTOR);
        }
    }
    // A review verdict is signed by its reviewer; an assignment names someone else and stays as it
    // is, and a critic verdict (#467) has no reviewer at all.
    if document.get("verdict").is_some() && document.get("reviewer").is_some() {
        document["reviewer"] = json!(ACTOR);
    }
    document
}

/// #1149: `execution status --tasks` carries the same fold the Runtime route serves, read from the
/// records the CLI's own signal door wrote. Catches a CLI that drops the block or ignores the flag,
/// and `--tasks` offered without the keyring that opens the envelopes. Cost: one held fixture run
/// and three sealed signals in a tempdir.
#[test]
fn status_with_tasks_folds_the_recorded_task_events() {
    let scratch = tempfile::tempdir().unwrap();
    let events = start(scratch.path());
    let claimed = signal(
        scratch.path(),
        &events,
        "tasks-claim",
        "task.claimed",
        ACTOR,
        &as_actor(package_fixture("claimed")),
    );
    assert_eq!(claimed["ok"], json!(true), "{claimed}");
    let mut opened = as_actor(package_fixture("pr-opened"));
    opened["taskId"] = as_actor(package_fixture("claimed"))["taskId"].clone();
    let opened_reply = signal(
        scratch.path(),
        &events,
        "tasks-pr",
        "task.pr_opened",
        ACTOR,
        &opened,
    );
    assert_eq!(opened_reply["ok"], json!(true), "{opened_reply}");
    let mut block = as_actor(package_fixture("review-verdict"));
    block["taskId"] = opened["taskId"].clone();
    block["pr"] = opened["pr"].clone();
    block["headSha"] = opened["headSha"].clone();
    block["verdict"] = json!("BLOCK");
    let block_reply = signal(
        scratch.path(),
        &events,
        "tasks-block",
        "task.review_verdict",
        ACTOR,
        &block,
    );
    assert_eq!(block_reply["ok"], json!(true), "{block_reply}");

    let output = command()
        .args(["execution", "status", "--events"])
        .arg(&events)
        .args(["--execution", RUN, "--tasks", "--keyring"])
        .arg(scratch.path().join("keyring"))
        .args(["--key-id", "owner-key"])
        .output()
        .unwrap();
    let reply: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(reply["ok"], json!(true), "{reply}");
    let data = &reply["data"]["tasks"];
    assert_eq!(data["rejected"], 0, "{data}");
    let tasks = data["tasks"]
        .as_array()
        .unwrap_or_else(|| panic!("no tasks: {data}"));
    assert_eq!(tasks.len(), 1, "{data}");
    assert_eq!(tasks[0]["pr"], opened["pr"]);
    assert_eq!(tasks[0]["headSha"], opened["headSha"]);
    assert_eq!(tasks[0]["lane"], ACTOR);
    assert_eq!(tasks[0]["step"], "review");
    assert_eq!(tasks[0]["blockedBy"]["reviewer"], ACTOR, "{}", tasks[0]);
    let claim_sequence = history(&events)
        .iter()
        .find(|event| {
            matches!(&event.kind, EventKind::SignalRecorded(recorded) if recorded.kind == "task.claimed")
        })
        .map(|event| event.sequence)
        .expect("the claim was recorded");
    assert_eq!(
        tasks[0]["claimSequence"],
        json!(claim_sequence),
        "{}",
        tasks[0]
    );
    // #1449: `--tasks` answers the open board; the record history is behind `--full-tasks`.
    assert_eq!(data["view"], "open", "{data}");
    assert!(data.get("records").is_none(), "{data}");
    let full = command()
        .args(["execution", "status", "--events"])
        .arg(&events)
        .args(["--execution", RUN, "--tasks", "--full-tasks", "--keyring"])
        .arg(scratch.path().join("keyring"))
        .args(["--key-id", "owner-key"])
        .output()
        .unwrap();
    let full: Value = serde_json::from_slice(&full.stdout).unwrap();
    let full = &full["data"]["tasks"];
    assert_eq!(full["tasks"], data["tasks"], "{full}");
    assert!(
        full["records"]
            .as_array()
            .is_some_and(|records| !records.is_empty()),
        "{full}"
    );

    // Without the flag the status reply carries no projection, as before.
    let plain = command()
        .args(["execution", "status", "--events"])
        .arg(&events)
        .args(["--execution", RUN])
        .output()
        .unwrap();
    let plain: Value = serde_json::from_slice(&plain.stdout).unwrap();
    assert_eq!(plain["ok"], json!(true), "{plain}");
    assert!(plain["data"].get("tasks").is_none(), "{plain}");

    // `--tasks` needs the keyring that opens the envelopes: clap refuses before anything runs.
    let refused = command()
        .args(["execution", "status", "--events"])
        .arg(&events)
        .args(["--execution", RUN, "--tasks"])
        .output()
        .unwrap();
    assert!(!refused.status.success());
}

/// A heartbeat is admitted through the real sealed signal door only while this lane owns a claim.
#[test]
fn heartbeat_on_an_open_claim_is_recorded_without_changing_the_claim() {
    let scratch = tempfile::tempdir().unwrap();
    let events = start(scratch.path());
    let claim = signal(
        scratch.path(),
        &events,
        "heartbeat-claim",
        "task.claimed",
        ACTOR,
        &as_actor(package_fixture("claimed")),
    );
    assert_eq!(claim["ok"], json!(true), "{claim}");
    let before = history(&events);
    let mut heartbeat = as_actor(package_fixture("claimed"));
    heartbeat.as_object_mut().unwrap().retain(|key, _| {
        matches!(
            key.as_str(),
            "schema" | "taskId" | "revision" | "at" | "lane"
        )
    });
    let reply = signal(
        scratch.path(),
        &events,
        "heartbeat-first",
        "task.heartbeat",
        ACTOR,
        &heartbeat,
    );
    assert_eq!(reply["ok"], json!(true), "{reply}");
    let after = history(&events);
    assert_eq!(after.len(), before.len() + 1);
    assert!(
        matches!(&after.last().unwrap().kind, EventKind::SignalRecorded(recorded)
        if recorded.kind == "task.heartbeat")
    );
    for (id, patch, code) in [
        ("heartbeat-repeat", None, "GHCLI003_SIGNAL_INVALID"),
        (
            "heartbeat-forbidden-pr",
            Some(("pr", json!(42))),
            "GHCLI003_SIGNAL_INVALID",
        ),
        (
            "heartbeat-forbidden-head",
            Some(("headSha", json!("abcdef0"))),
            "GHCLI003_SIGNAL_INVALID",
        ),
        (
            "heartbeat-wrong-lane",
            Some(("lane", json!("other-lane"))),
            "GHCLI038_ACTOR_MISMATCH",
        ),
    ] {
        let mut candidate = heartbeat.clone();
        if let Some((key, value)) = patch {
            candidate[key] = value;
        }
        candidate["at"] = json!("1900-01-01T00:00:00Z");
        let head = history(&events).len();
        let refused = signal(
            scratch.path(),
            &events,
            id,
            "task.heartbeat",
            ACTOR,
            &candidate,
        );
        assert_eq!(refused["diagnostics"][0]["code"], code, "{id}: {refused}");
        assert_eq!(history(&events).len(), head, "{id} moved journal head");
    }
}

/// #1556: a waiting lane says why. A reason that is empty or carries a control character is
/// refused BEFORE any heartbeat exists (so the 300 s cooldown cannot be what refuses it), and a
/// bounded one is admitted. A heartbeat without a reason is covered by the cell above.
#[test]
fn a_heartbeat_carries_the_waiting_lanes_reason_and_refuses_a_bad_one() {
    let scratch = tempfile::tempdir().unwrap();
    let events = start(scratch.path());
    let claim = signal(
        scratch.path(),
        &events,
        "reason-claim",
        "task.claimed",
        ACTOR,
        &as_actor(package_fixture("claimed")),
    );
    assert_eq!(claim["ok"], json!(true), "{claim}");
    let mut heartbeat = as_actor(package_fixture("claimed"));
    heartbeat.as_object_mut().unwrap().retain(|key, _| {
        matches!(
            key.as_str(),
            "schema" | "taskId" | "revision" | "at" | "lane"
        )
    });
    for (id, reason) in [
        ("reason-empty", json!("")),
        ("reason-control", json!("waiting\u{7}")),
        ("reason-too-long", json!("x".repeat(501))),
    ] {
        let mut candidate = heartbeat.clone();
        candidate["reason"] = reason;
        let head = history(&events).len();
        let refused = signal(
            scratch.path(),
            &events,
            id,
            "task.heartbeat",
            ACTOR,
            &candidate,
        );
        assert_eq!(
            refused["diagnostics"][0]["code"], "GHCLI003_SIGNAL_INVALID",
            "{id}: {refused}"
        );
        assert_eq!(history(&events).len(), head, "{id} moved journal head");
    }
    heartbeat["reason"] = json!("waiting on #1670 to land");
    let reply = signal(
        scratch.path(),
        &events,
        "reason-heartbeat",
        "task.heartbeat",
        ACTOR,
        &heartbeat,
    );
    assert_eq!(reply["ok"], json!(true), "{reply}");
}

#[test]
fn heartbeat_needs_an_open_claim_or_pr_and_a_merge_ends_it() {
    let scratch = tempfile::tempdir().unwrap();
    let events = start(scratch.path());
    let mut heartbeat = as_actor(package_fixture("claimed"));
    heartbeat.as_object_mut().unwrap().retain(|key, _| {
        matches!(
            key.as_str(),
            "schema" | "taskId" | "revision" | "at" | "lane"
        )
    });
    let head = history(&events).len();
    let absent = signal(
        scratch.path(),
        &events,
        "heartbeat-absent",
        "task.heartbeat",
        ACTOR,
        &heartbeat,
    );
    assert_eq!(absent["diagnostics"][0]["code"], "GHCLI003_SIGNAL_INVALID");
    assert_eq!(history(&events).len(), head);
    let claim = signal(
        scratch.path(),
        &events,
        "heartbeat-claim-2",
        "task.claimed",
        ACTOR,
        &as_actor(package_fixture("claimed")),
    );
    assert_eq!(claim["ok"], true, "{claim}");
    let pr = signal(
        scratch.path(),
        &events,
        "heartbeat-pr",
        "task.pr_opened",
        ACTOR,
        &as_actor(package_fixture("pr-opened")),
    );
    assert_eq!(pr["ok"], true, "{pr}");
    // #1556: an open PR keeps the task heartbeat-eligible; its merge ends that. No heartbeat was
    // recorded before this one, so the refusal is the merge, not the 300 s cooldown.
    let merged = signal(
        scratch.path(),
        &events,
        "heartbeat-merged",
        "task.merged",
        ACTOR,
        &as_actor(package_fixture("merged")),
    );
    assert_eq!(merged["ok"], true, "{merged}");
    let head = history(&events).len();
    let ended = signal(
        scratch.path(),
        &events,
        "heartbeat-ended",
        "task.heartbeat",
        ACTOR,
        &heartbeat,
    );
    assert_eq!(ended["diagnostics"][0]["code"], "GHCLI003_SIGNAL_INVALID");
    assert_eq!(history(&events).len(), head);
}

/// #1556: a task at the PR step is still its author's work while the PR is open, so the PR
/// author's heartbeat is admitted there (the owner's Silent cards included tasks waiting on a
/// reached plan with no way to say they were alive). Before: refused as "requires an open pre-PR
/// claim". Cost: one held fixture run and three CLI signals.
#[test]
fn the_pr_author_may_heartbeat_a_task_whose_pr_is_open() {
    let scratch = tempfile::tempdir().unwrap();
    let events = start(scratch.path());
    let mut heartbeat = as_actor(package_fixture("claimed"));
    heartbeat.as_object_mut().unwrap().retain(|key, _| {
        matches!(
            key.as_str(),
            "schema" | "taskId" | "revision" | "at" | "lane"
        )
    });
    for (id, kind, fixture) in [
        ("pr-step-claim", "task.claimed", "claimed"),
        ("pr-step-pr", "task.pr_opened", "pr-opened"),
    ] {
        let reply = signal(
            scratch.path(),
            &events,
            id,
            kind,
            ACTOR,
            &as_actor(package_fixture(fixture)),
        );
        assert_eq!(reply["ok"], true, "{id}: {reply}");
    }
    heartbeat["reason"] = json!("waiting on the reached plan");
    let head = history(&events).len();
    let alive = signal(
        scratch.path(),
        &events,
        "pr-step-heartbeat",
        "task.heartbeat",
        ACTOR,
        &heartbeat,
    );
    assert_eq!(alive["ok"], true, "{alive}");
    assert_eq!(history(&events).len(), head + 1);
}

/// #1193 (plan A2): a claim carries its lease. Before: `leaseSeconds` was an unknown key and the
/// whole claim was refused (`GHCLI003`), so no lane could state how long its claim stays live.
/// Now a lease of 60 s to a day is admitted and one outside that range is refused with nothing
/// appended. Cost: one held fixture run and three CLI signals.
#[test]
fn a_claim_carries_a_lease_within_bounds() {
    let scratch = tempfile::tempdir().unwrap();
    let events = start(scratch.path());
    let mut claim = as_actor(package_fixture("claimed"));
    claim["leaseSeconds"] = json!(1800);
    let admitted = signal(
        scratch.path(),
        &events,
        "lease-claim",
        "task.claimed",
        ACTOR,
        &claim,
    );
    assert_eq!(admitted["ok"], true, "{admitted}");
    let head = history(&events).len();
    for (id, lease) in [
        ("lease-short", json!(299)),
        ("lease-long", json!(86401)),
        ("lease-text", json!("1800")),
    ] {
        let mut bad = claim.clone();
        bad["taskId"] = json!("issue-1194");
        bad["issue"] = json!(1194);
        bad["leaseSeconds"] = lease;
        let refused = signal(scratch.path(), &events, id, "task.claimed", ACTOR, &bad);
        assert_eq!(
            refused["diagnostics"][0]["code"], "GHCLI003_SIGNAL_INVALID",
            "{id}: {refused}"
        );
    }
    assert_eq!(
        history(&events).len(),
        head,
        "a refused lease appends nothing"
    );
}

#[test]
fn release_ends_heartbeat_eligibility_for_its_claim() {
    let scratch = tempfile::tempdir().unwrap();
    let events = start(scratch.path());
    let claim = signal(
        scratch.path(),
        &events,
        "heartbeat-release-claim",
        "task.claimed",
        ACTOR,
        &as_actor(package_fixture("claimed")),
    );
    assert_eq!(claim["ok"], true, "{claim}");
    let claim_sequence = history(&events).last().unwrap().sequence;
    let mut heartbeat = as_actor(package_fixture("claimed"));
    heartbeat.as_object_mut().unwrap().retain(|key, _| {
        matches!(
            key.as_str(),
            "schema" | "taskId" | "revision" | "at" | "lane"
        )
    });
    let mut release = heartbeat.clone();
    release["claimSequence"] = json!(claim_sequence);
    release["reason"] = json!("Stopped working on this claim");
    let done = signal(
        scratch.path(),
        &events,
        "heartbeat-release",
        "task.released",
        ACTOR,
        &release,
    );
    assert_eq!(done["ok"], true, "{done}");
    let head = history(&events).len();
    let refused = signal(
        scratch.path(),
        &events,
        "heartbeat-after-release",
        "task.heartbeat",
        ACTOR,
        &heartbeat,
    );
    assert_eq!(refused["diagnostics"][0]["code"], "GHCLI003_SIGNAL_INVALID");
    assert_eq!(history(&events).len(), head);
}

#[test]
fn another_task_does_not_end_the_claim_or_reset_its_cooldown() {
    let scratch = tempfile::tempdir().unwrap();
    let events = start(scratch.path());
    let claim = signal(
        scratch.path(),
        &events,
        "heartbeat-main-claim",
        "task.claimed",
        ACTOR,
        &as_actor(package_fixture("claimed")),
    );
    assert_eq!(claim["ok"], true, "{claim}");
    let mut other_claim = as_actor(package_fixture("claimed"));
    other_claim["taskId"] = json!("other-task");
    let other = signal(
        scratch.path(),
        &events,
        "heartbeat-other-claim",
        "task.claimed",
        ACTOR,
        &other_claim,
    );
    assert_eq!(other["ok"], true, "{other}");
    let mut other_pr = as_actor(package_fixture("pr-opened"));
    other_pr["taskId"] = json!("other-task");
    let pr = signal(
        scratch.path(),
        &events,
        "heartbeat-other-pr",
        "task.pr_opened",
        ACTOR,
        &other_pr,
    );
    assert_eq!(pr["ok"], true, "{pr}");
    let mut heartbeat = as_actor(package_fixture("claimed"));
    heartbeat.as_object_mut().unwrap().retain(|key, _| {
        matches!(
            key.as_str(),
            "schema" | "taskId" | "revision" | "at" | "lane"
        )
    });
    let first = signal(
        scratch.path(),
        &events,
        "heartbeat-main-first",
        "task.heartbeat",
        ACTOR,
        &heartbeat,
    );
    assert_eq!(first["ok"], true, "{first}");
    let renewed = signal(
        scratch.path(),
        &events,
        "heartbeat-main-renewed",
        "task.claimed",
        ACTOR,
        &as_actor(package_fixture("claimed")),
    );
    assert_eq!(renewed["ok"], true, "{renewed}");
    let head = history(&events).len();
    let repeated = signal(
        scratch.path(),
        &events,
        "heartbeat-main-repeat",
        "task.heartbeat",
        ACTOR,
        &heartbeat,
    );
    assert_eq!(
        repeated["diagnostics"][0]["code"],
        "GHCLI003_SIGNAL_INVALID"
    );
    assert_eq!(history(&events).len(), head);
}

/// Two submissions may both read the same eligible claim; the journal must admit only one.
#[test]
fn concurrent_heartbeats_append_at_most_once() {
    use std::sync::Barrier;
    let scratch = tempfile::tempdir().unwrap();
    let events = start(scratch.path());
    let claim = signal(
        scratch.path(),
        &events,
        "race-claim",
        "task.claimed",
        ACTOR,
        &as_actor(package_fixture("claimed")),
    );
    assert_eq!(claim["ok"], true, "{claim}");
    let before = history(&events).len();
    let mut heartbeat = as_actor(package_fixture("claimed"));
    heartbeat.as_object_mut().unwrap().retain(|key, _| {
        matches!(
            key.as_str(),
            "schema" | "taskId" | "revision" | "at" | "lane"
        )
    });
    let barrier = Arc::new(Barrier::new(3));
    let replies: Vec<_> = ["race-left", "race-right"]
        .into_iter()
        .map(|id| {
            let barrier = Arc::clone(&barrier);
            let events = events.clone();
            let scratch = scratch.path().to_path_buf();
            let heartbeat = heartbeat.clone();
            std::thread::spawn(move || {
                barrier.wait();
                signal(&scratch, &events, id, "task.heartbeat", ACTOR, &heartbeat)
            })
        })
        .collect();
    barrier.wait();
    let replies: Vec<_> = replies
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect();
    assert_eq!(
        replies.iter().filter(|reply| reply["ok"] == true).count(),
        1,
        "{replies:?}"
    );
    assert_eq!(history(&events).len(), before + 1);
    assert_eq!(
        history(&events)
            .iter()
            .filter(
                |event| matches!(&event.kind, EventKind::SignalRecorded(recorded)
                if recorded.kind == "task.heartbeat")
            )
            .count(),
        1
    );
}

#[test]
fn a_task_signal_naming_another_actor_is_refused_and_records_nothing() {
    let scratch = tempfile::tempdir().unwrap();
    let events = start(scratch.path());
    let document = package_fixture("pr-opened");
    let reply = signal(
        scratch.path(),
        &events,
        "spoofed",
        "task.pr_opened",
        "gh-claude-4",
        &document,
    );
    assert_eq!(reply["ok"], json!(false), "{reply}");
    assert_eq!(
        reply["diagnostics"][0]["code"],
        json!("GHCLI038_ACTOR_MISMATCH"),
        "{reply}"
    );
    assert!(
        !scratch.path().join("spoofed-evidence.json").exists(),
        "a refused task record writes no evidence"
    );
}

#[test]
fn each_task_kind_is_accepted_from_its_recording_actor() {
    let scratch = tempfile::tempdir().unwrap();
    let events = start(scratch.path());
    for (name, kind) in [
        ("claimed", "task.claimed"),
        ("planned", "task.planned"),
        ("pr-opened", "task.pr_opened"),
        ("review-assigned", "task.review_assigned"),
        ("review-verdict", "task.review_verdict"),
        ("merged", "task.merged"),
        ("closed", "task.closed"),
    ] {
        let reply = signal(
            scratch.path(),
            &events,
            name,
            kind,
            ACTOR,
            &as_actor(package_fixture(name)),
        );
        assert_eq!(reply["ok"], json!(true), "{kind}: {reply}");
    }
}

/// #1544: the Stop reconciler records a close it only observed. `observed: true` is admitted; any
/// other value is refused, and a close without it is admitted as before. Cost: one temp store and
/// three CLI calls.
#[test]
fn a_close_may_say_it_was_observed_and_only_with_true() {
    let scratch = tempfile::tempdir().unwrap();
    let events = start(scratch.path());
    for (id, observed, ok) in [
        ("observed-true", json!(true), true),
        ("observed-false", json!(false), false),
        ("observed-text", json!("yes"), false),
    ] {
        let mut closed = as_actor(package_fixture("closed"));
        closed["observed"] = observed;
        let reply = signal(scratch.path(), &events, id, "task.closed", ACTOR, &closed);
        assert_eq!(reply["ok"], json!(ok), "{id}: {reply}");
    }
}

/// #729: only the lane may release its exact pre-PR claim. Cost: one temp store and five CLI calls.
#[test]
fn a_claim_release_is_bound_to_its_claim_sequence_and_lane() {
    let scratch = tempfile::tempdir().unwrap();
    let events = start(scratch.path());
    let claim = signal(
        scratch.path(),
        &events,
        "release-claim",
        "task.claimed",
        ACTOR,
        &as_actor(package_fixture("claimed")),
    );
    assert_eq!(claim["ok"], json!(true), "{claim}");
    let claim_sequence = history(&events)
        .into_iter()
        .find(|event| {
            matches!(&event.kind,
        EventKind::SignalRecorded(recorded) if recorded.signal_id.as_str() == "release-claim")
        })
        .unwrap()
        .sequence;
    let mut released = as_actor(package_fixture("claimed"));
    released
        .as_object_mut()
        .unwrap()
        .retain(|key, _| matches!(key.as_str(), "schema" | "taskId" | "revision" | "at"));
    released["lane"] = json!(ACTOR);
    released["claimSequence"] = json!(claim_sequence);
    released["reason"] = json!("No longer working on this claim");
    let reply = signal(
        scratch.path(),
        &events,
        "release-ok",
        "task.released",
        ACTOR,
        &released,
    );
    assert_eq!(reply["ok"], json!(true), "{reply}");

    for (id, lane, sequence, code) in [
        (
            "release-foreign",
            "another-lane",
            json!(claim_sequence),
            "GHCLI038_ACTOR_MISMATCH",
        ),
        (
            "release-no-sequence",
            ACTOR,
            json!(null),
            "GHCLI003_SIGNAL_INVALID",
        ),
        (
            "release-orphan",
            ACTOR,
            json!(999),
            "GHCLI003_SIGNAL_INVALID",
        ),
    ] {
        let mut malformed = released.clone();
        malformed["lane"] = json!(lane);
        malformed["claimSequence"] = sequence;
        let reply = signal(
            scratch.path(),
            &events,
            id,
            "task.released",
            ACTOR,
            &malformed,
        );
        assert_eq!(reply["ok"], json!(false), "{id}: {reply}");
        assert_eq!(reply["diagnostics"][0]["code"], json!(code));
    }
}

/// #729: a PR on an older claim cannot prevent release of a later PR-less claim.
/// Cost: one temp store and five signal CLI calls.
#[test]
fn release_ignores_a_pr_owned_by_an_earlier_claim() {
    let scratch = tempfile::tempdir().unwrap();
    let events = start(scratch.path());
    for id in ["first-claim", "second-claim"] {
        let reply = signal(
            scratch.path(),
            &events,
            id,
            "task.claimed",
            ACTOR,
            &as_actor(package_fixture("claimed")),
        );
        assert_eq!(reply["ok"], json!(true), "{id}: {reply}");
    }
    let second_sequence = history(&events)
        .into_iter()
        .find(|event| {
            matches!(&event.kind, EventKind::SignalRecorded(recorded)
            if recorded.signal_id.as_str() == "second-claim")
        })
        .unwrap()
        .sequence;
    let opened = signal(
        scratch.path(),
        &events,
        "first-pr",
        "task.pr_opened",
        ACTOR,
        &as_actor(package_fixture("pr-opened")),
    );
    assert_eq!(opened["ok"], json!(true), "{opened}");
    let updated = signal(
        scratch.path(),
        &events,
        "first-pr-updated",
        "task.pr_opened",
        ACTOR,
        &as_actor(package_fixture("pr-opened")),
    );
    assert_eq!(updated["ok"], json!(true), "{updated}");

    let mut released = as_actor(package_fixture("claimed"));
    released
        .as_object_mut()
        .unwrap()
        .retain(|key, _| matches!(key.as_str(), "schema" | "taskId" | "revision" | "at"));
    released["lane"] = json!(ACTOR);
    released["claimSequence"] = json!(second_sequence);
    released["reason"] = json!("Stopped work on the second claim");
    let reply = signal(
        scratch.path(),
        &events,
        "release-second",
        "task.released",
        ACTOR,
        &released,
    );
    assert_eq!(reply["ok"], json!(true), "{reply}");
}

/// #729: a release selects only the latest open claim. Cost: one temp store and five signals.
#[test]
fn release_refuses_an_older_open_claim_and_one_with_a_pr() {
    let scratch = tempfile::tempdir().unwrap();
    let events = start(scratch.path());
    for id in ["old-claim", "new-claim"] {
        let reply = signal(
            scratch.path(),
            &events,
            id,
            "task.claimed",
            ACTOR,
            &as_actor(package_fixture("claimed")),
        );
        assert_eq!(reply["ok"], json!(true), "{id}: {reply}");
    }
    let claims: Vec<_> = history(&events).into_iter().filter(|event|
        matches!(&event.kind, EventKind::SignalRecorded(recorded) if recorded.kind == "task.claimed")
    ).map(|event| event.sequence).collect();
    let mut release = as_actor(package_fixture("claimed"));
    release
        .as_object_mut()
        .unwrap()
        .retain(|key, _| matches!(key.as_str(), "schema" | "taskId" | "revision" | "at"));
    release["lane"] = json!(ACTOR);
    release["reason"] = json!("Stopped");
    release["claimSequence"] = json!(claims[0]);
    let older = signal(
        scratch.path(),
        &events,
        "release-old",
        "task.released",
        ACTOR,
        &release,
    );
    assert_eq!(older["ok"], json!(false), "{older}");
    release["claimSequence"] = json!(claims[1]);
    let latest = signal(
        scratch.path(),
        &events,
        "release-new",
        "task.released",
        ACTOR,
        &release,
    );
    assert_eq!(latest["ok"], json!(true), "{latest}");
    let pr = signal(
        scratch.path(),
        &events,
        "old-pr",
        "task.pr_opened",
        ACTOR,
        &as_actor(package_fixture("pr-opened")),
    );
    assert_eq!(pr["ok"], json!(true), "{pr}");
    release["claimSequence"] = json!(claims[0]);
    let owned = signal(
        scratch.path(),
        &events,
        "release-owned",
        "task.released",
        ACTOR,
        &release,
    );
    assert_eq!(owned["ok"], json!(false), "{owned}");
}

/// #86: assignment provenance is optional, bounded text and cannot name the claimant.
/// Existing claim fixtures cover no assigner. Cost: one temp store and seven CLI calls.
#[test]
fn claimed_assigner_is_admitted_but_self_or_malformed_assigners_are_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let events = start(scratch.path());
    for (case, assigner, accepted) in [
        ("reported", json!("coord"), true),
        ("unicode-boundary", json!("é".repeat(128)), true),
        ("self", json!(ACTOR), false),
        ("long", json!("x".repeat(129)), false),
        ("empty", json!(""), false),
        ("null", Value::Null, false),
        ("control", json!("bad\nname"), false),
    ] {
        let mut document = as_actor(package_fixture("claimed"));
        document["assignedBy"] = assigner;
        let reply = signal(
            scratch.path(),
            &events,
            &format!("assigner-{case}"),
            "task.claimed",
            ACTOR,
            &document,
        );
        assert_eq!(reply["ok"], json!(accepted), "{case}: {reply}");
        if !accepted {
            assert_eq!(
                reply["diagnostics"][0]["code"],
                json!("GHCLI003_SIGNAL_INVALID")
            );
            assert!(
                !scratch
                    .path()
                    .join(format!("assigner-{case}-evidence.json"))
                    .exists()
            );
        }
    }
}

#[test]
fn a_malformed_task_document_is_refused_as_invalid() {
    let scratch = tempfile::tempdir().unwrap();
    let events = start(scratch.path());
    let mut verdict = as_actor(package_fixture("review-verdict"));
    verdict["verdict"] = json!("LGTM");
    let reply = signal(
        scratch.path(),
        &events,
        "bad-verdict",
        "task.review_verdict",
        ACTOR,
        &verdict,
    );
    assert_eq!(reply["ok"], json!(false), "{reply}");
    assert_eq!(
        reply["diagnostics"][0]["path"],
        json!("/signal/description"),
        "{reply}"
    );
    let mut extra = as_actor(package_fixture("merged"));
    extra["note"] = json!("not in the schema");
    let reply = signal(
        scratch.path(),
        &events,
        "extra-key",
        "task.merged",
        ACTOR,
        &extra,
    );
    assert_eq!(reply["ok"], json!(false), "{reply}");
}

/// #419: the recording lane is the source, but the document names another lane, reviewer or merger.
/// DELIVERY.md promises `GHCLI038_ACTOR_MISMATCH` for this too; it was refused only as a malformed
/// document (`GHCLI003`), so a lane could not tell a wrong name from a wrong shape. Cost: three
/// CLI calls on one held run.
#[test]
fn a_document_naming_another_lane_reviewer_or_merger_is_an_actor_mismatch() {
    let scratch = tempfile::tempdir().unwrap();
    let events = start(scratch.path());
    for (fixture, kind, field) in [
        ("pr-opened", "task.pr_opened", "lane"),
        ("planned", "task.planned", "lane"),
        ("review-verdict", "task.review_verdict", "reviewer"),
        ("merged", "task.merged", "merger"),
    ] {
        let mut document = as_actor(package_fixture(fixture));
        document[field] = json!("gh-claude-4");
        let id = format!("named-{field}");
        let reply = signal(scratch.path(), &events, &id, kind, ACTOR, &document);
        assert_eq!(reply["ok"], json!(false), "{field}: {reply}");
        assert_eq!(
            reply["diagnostics"][0]["code"],
            json!("GHCLI038_ACTOR_MISMATCH"),
            "{field}: {reply}"
        );
        assert_eq!(
            reply["diagnostics"][0]["path"],
            json!(format!("/signal/description/{field}")),
            "{field}: {reply}"
        );
        assert!(
            !scratch.path().join(format!("{id}-evidence.json")).exists(),
            "{field}: a refused task record writes no evidence"
        );
    }
}

/// #420: `task.claimed` and `task.pr_opened` may name the task's GitHub repository (`repo`,
/// `owner/name`) so the Studio links its issue and PR before any review exists. A malformed value
/// is refused like any malformed field, so nothing but `owner/name` reaches a link. Cost: a few
/// CLI calls on one held run.
#[test]
fn an_opening_record_may_name_its_repo_and_a_malformed_one_is_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let events = start(scratch.path());
    for (fixture, kind) in [("claimed", "task.claimed"), ("pr-opened", "task.pr_opened")] {
        for (case, repo, accepted) in [
            ("ok", "stabem/GraphHelm", true),
            ("dotfile", "stabem/.github", true),
            ("url", "https://evil.example/x/y", false),
            ("dots", "stabem/..", false),
            ("deep", "a/b/c", false),
        ] {
            let mut document = as_actor(package_fixture(fixture));
            document["repo"] = json!(repo);
            let id = format!("repo-{fixture}-{case}");
            let reply = signal(scratch.path(), &events, &id, kind, ACTOR, &document);
            assert_eq!(reply["ok"], json!(accepted), "{kind} {repo}: {reply}");
            if !accepted {
                assert_eq!(
                    reply["diagnostics"][0]["code"],
                    json!("GHCLI003_SIGNAL_INVALID"),
                    "{kind} {repo}: {reply}"
                );
            }
        }
    }
}

/// #477: an opening record may carry the issue's or the PR's title and one-line summary for the
/// Team tab. Optional; empty, oversized or control-character text is refused like any bad field.
#[test]
fn an_opening_record_may_carry_a_title_and_summary_and_a_malformed_one_is_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let events = start(scratch.path());
    for (fixture, kind) in [("claimed", "task.claimed"), ("pr-opened", "task.pr_opened")] {
        for (case, title, summary, accepted) in [
            (
                "ok",
                json!("Studio: Team tab shows task titles"),
                json!("The owner reads each task's title."),
                true,
            ),
            (
                "unicode",
                json!("Studio: título da tarefa"),
                json!("O dono lê o título."),
                true,
            ),
            // Limits count characters, as the schema and the script do, not UTF-8 bytes (#486
            // review): 200 accented characters are 220+ bytes and must still be accepted.
            (
                "accented-200",
                json!("Correção ".repeat(22).chars().take(200).collect::<String>()),
                json!("ção".repeat(100)),
                true,
            ),
            ("accented-201", json!("ç".repeat(201)), json!("x"), false),
            ("empty", json!(""), json!("x"), false),
            ("long", json!("t".repeat(201)), json!("x"), false),
            ("newline", json!("a\nb"), json!("x"), false),
            ("number", json!(7), json!("x"), false),
            ("summary-long", json!("ok"), json!("s".repeat(301)), false),
        ] {
            let mut document = as_actor(package_fixture(fixture));
            document["title"] = title;
            document["summary"] = summary;
            let id = format!("words-{fixture}-{case}");
            let reply = signal(scratch.path(), &events, &id, kind, ACTOR, &document);
            assert_eq!(reply["ok"], json!(accepted), "{kind} {case}: {reply}");
        }
    }
}

/// #480: `task.planned` carries the lane's keel plan (classes, reviews, proof, critic, summary) so
/// the Studio lights Plan, and Critic only for `critic.mode: design`. Each field is bounded like the
/// schema: a wrong class, a repeated class, an out-of-range score, an unknown critic key, a missing
/// or overlong summary are refused as invalid and record nothing. Cost: a few CLI calls on one
/// held run.
#[test]
fn a_planned_record_outside_the_schema_bounds_is_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let events = start(scratch.path());
    type Edit = fn(&mut Value);
    let cases: [(&str, Edit); 9] = [
        ("class", |d| d["classes"] = json!(["frontend"])),
        ("no-class", |d| d["classes"] = json!([])),
        ("twice", |d| d["classes"] = json!(["code", "code"])),
        ("reviews", |d| d["reviews"] = json!(0)),
        ("proof", |d| d["proof"] = json!("vibes")),
        ("score", |d| d["critic"]["passScore"] = json!(0)),
        ("critic-key", |d| d["critic"]["auto"] = json!(true)),
        ("no-summary", |d| {
            d.as_object_mut().unwrap().remove("summary");
        }),
        ("long-summary", |d| d["summary"] = json!("x".repeat(301))),
    ];
    for (case, edit) in cases {
        let mut document = as_actor(package_fixture("planned"));
        edit(&mut document);
        let id = format!("planned-{case}");
        let reply = signal(
            scratch.path(),
            &events,
            &id,
            "task.planned",
            ACTOR,
            &document,
        );
        assert_eq!(reply["ok"], json!(false), "{case}: {reply}");
        assert_eq!(
            reply["diagnostics"][0]["code"],
            json!("GHCLI003_SIGNAL_INVALID"),
            "{case}: {reply}"
        );
        assert!(
            !scratch.path().join(format!("{id}-evidence.json")).exists(),
            "{case}: a refused task record writes no evidence"
        );
    }
}

/// #514: a claim may name the issue whose work turned it up (`parent`), so the Studio draws it
/// inside that issue's card. Optional; anything but a positive number is refused.
#[test]
fn a_claim_may_name_its_parent_issue_and_a_malformed_one_is_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let events = start(scratch.path());
    for (case, parent, accepted) in [
        ("ok", json!(356), true),
        ("zero", json!(0), false),
        ("negative", json!(-1), false),
        ("text", json!("356"), false),
    ] {
        let mut document = as_actor(package_fixture("claimed"));
        document["parent"] = parent;
        let id = format!("parent-{case}");
        let reply = signal(
            scratch.path(),
            &events,
            &id,
            "task.claimed",
            ACTOR,
            &document,
        );
        assert_eq!(reply["ok"], json!(accepted), "{case}: {reply}");
    }
    // Only a claim names a parent.
    let mut opened = as_actor(package_fixture("pr-opened"));
    opened["parent"] = json!(356);
    let reply = signal(
        scratch.path(),
        &events,
        "parent-on-pr",
        "task.pr_opened",
        ACTOR,
        &opened,
    );
    assert_eq!(reply["ok"], json!(false), "{reply}");
}

/// #577: a claim may name the journeys its issue serves, so the Studio links the task from the
/// claim on. Optional; a list holding anything but ids is refused.
#[test]
fn a_claim_may_name_its_journeys_and_a_malformed_list_is_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let events = start(scratch.path());
    for (case, journeys, accepted) in [
        ("ok", json!(["studio-graph-tab"]), true),
        ("empty", json!([]), true),
        ("not-a-list", json!("studio-graph-tab"), false),
        ("bad-id", json!(["bad id!"]), false),
    ] {
        let mut document = as_actor(package_fixture("claimed"));
        document["journeys"] = journeys;
        let reply = signal(
            scratch.path(),
            &events,
            &format!("journeys-{case}"),
            "task.claimed",
            ACTOR,
            &document,
        );
        assert_eq!(reply["ok"], json!(accepted), "{case}: {reply}");
    }
}

/// #467: the design critic's round is a `task.critic_verdict` record, signed by the lane, and the
/// Runtime refuses a verdict that disagrees with its score and round, so running out of rounds can
/// never be recorded as a pass. Credible regressions: an under-threshold `pass`, a `revise` on the
/// last round (an endless loop), an `exhausted` with rounds left, or another lane signing it.
/// Cost: one held run, eleven CLI calls.
#[test]
fn a_critic_verdict_must_agree_with_its_score_and_round() {
    let scratch = tempfile::tempdir().unwrap();
    let events = start(scratch.path());
    let valid = as_actor(package_fixture("critic-verdict"));
    let reply = signal(
        scratch.path(),
        &events,
        "critic-ok",
        "task.critic_verdict",
        ACTOR,
        &valid,
    );
    assert_eq!(reply["ok"], json!(true), "{reply}");
    for (id, patch) in [
        ("pass-below", json!({"verdict": "pass", "score": 7})),
        (
            "revise-last",
            json!({"verdict": "revise", "score": 5, "round": 3}),
        ),
        (
            "exhausted-early",
            json!({"verdict": "exhausted", "score": 5, "round": 2}),
        ),
        ("round-over", json!({"verdict": "pass", "round": 4})),
    ] {
        let mut document = valid.clone();
        for (key, value) in patch.as_object().unwrap() {
            document[key] = value.clone();
        }
        let reply = signal(
            scratch.path(),
            &events,
            id,
            "task.critic_verdict",
            ACTOR,
            &document,
        );
        assert_eq!(reply["ok"], json!(false), "{id}: {reply}");
        assert_eq!(
            reply["diagnostics"][0]["code"],
            json!("GHCLI003_SIGNAL_INVALID"),
            "{id}: {reply}"
        );
    }
    // Limits count characters, as the schema does, not UTF-8 bytes (#541 review): a reason of 500
    // accented characters is 1000 bytes and must be admitted; 501 is refused, and so is a control
    // character. The same for `designRef` at 256.
    for (id, key, value, accepted) in [
        ("reason-500", "reasons", json!(["ç".repeat(500)]), true),
        ("reason-501", "reasons", json!(["ç".repeat(501)]), false),
        ("reason-newline", "reasons", json!(["a\nb"]), false),
        ("design-256", "designRef", json!("ã".repeat(256)), true),
        ("design-257", "designRef", json!("ã".repeat(257)), false),
    ] {
        let mut document = valid.clone();
        document[key] = value;
        let reply = signal(
            scratch.path(),
            &events,
            id,
            "task.critic_verdict",
            ACTOR,
            &document,
        );
        assert_eq!(reply["ok"], json!(accepted), "{id}: {reply}");
    }
    let mut other = valid.clone();
    other["lane"] = json!("gh-claude-4");
    let reply = signal(
        scratch.path(),
        &events,
        "critic-spoof",
        "task.critic_verdict",
        ACTOR,
        &other,
    );
    assert_eq!(
        reply["diagnostics"][0]["code"],
        json!("GHCLI038_ACTOR_MISMATCH"),
        "{reply}"
    );
}

/// A deliberate current-review wait is a signed task update, never a new verdict or lease.
#[test]
fn reviewer_wait_is_bound_to_the_approved_head_without_changing_its_obligation() {
    let scratch = tempfile::tempdir().unwrap();
    let events = start(scratch.path());
    let mut opened = as_actor(package_fixture("pr-opened"));
    opened["headSha"] = json!("0123456789abcdef0123456789abcdef01234567");
    opened["repo"] = json!("fixture/project");
    assert_eq!(
        signal(
            scratch.path(),
            &events,
            "wait-open",
            "task.pr_opened",
            ACTOR,
            &opened
        )["ok"],
        true
    );
    let mut assigned = package_fixture("review-assigned");
    assigned["reviewer"] = json!(ACTOR);
    assigned["headSha"] = opened["headSha"].clone();
    assert_eq!(
        signal(
            scratch.path(),
            &events,
            "wait-assign",
            "task.review_assigned",
            ACTOR,
            &assigned
        )["ok"],
        true
    );
    let mut approved = as_actor(package_fixture("review-verdict"));
    approved["headSha"] = opened["headSha"].clone();
    approved["verdict"] = json!("APPROVE-WITH-RISK");
    assert_eq!(
        signal(
            scratch.path(),
            &events,
            "wait-approve",
            "task.review_verdict",
            ACTOR,
            &approved
        )["ok"],
        true
    );
    let before = history(&events);
    let context = before
        .iter()
        .rev()
        .find(|event| {
            matches!(&event.kind,
        EventKind::SignalRecorded(record) if record.kind == "task.review_verdict")
        })
        .unwrap()
        .sequence;
    let waiting = json!({"schema":"graphhelm-task-event-v1", "taskId":opened["taskId"],
        "revision":1, "at":"2099-01-01T00:00:00Z", "repo":"fixture/project",
        "pr":opened["pr"], "headSha":opened["headSha"], "reviewer":ACTOR,
        "reviewContextSequence":context, "activity":"waiting", "reason":"Awaiting the dependency proof"});
    let reply = signal(
        scratch.path(),
        &events,
        "review-wait",
        "task.review_activity",
        ACTOR,
        &waiting,
    );
    assert_eq!(reply["ok"], true, "{reply}");
    let after = history(&events);
    assert_eq!(after.len(), before.len() + 1);
    let output = command()
        .args(["execution", "status", "--events"])
        .arg(&events)
        .args(["--execution", RUN, "--tasks", "--keyring"])
        .arg(scratch.path().join("keyring"))
        .args(["--key-id", "owner-key"])
        .output()
        .unwrap();
    let reply: Value = serde_json::from_slice(&output.stdout).unwrap();
    let task = &reply["data"]["tasks"]["tasks"][0];
    assert_eq!(task["step"], "merge");
    assert_eq!(task["currentReviewer"], ACTOR);
    assert_eq!(task["headSha"], opened["headSha"]);
    assert_eq!(task["reviewContextSequence"], context);
    assert_eq!(task["reviewActivityRevision"], 1);
    assert_eq!(task["reviewActivity"]["reason"], waiting["reason"]);
    assert_ne!(task["reviewActivity"]["occurredAt"], waiting["at"]);
    assert!(task["blockedBy"].is_null());
    // A fresh CLI process cannot trust unreadable sealed history, even with a well-shaped new
    // statement. Temporarily move only this fixture's blobs; restore before any assertion.
    let blobs = events.join("blobs");
    let saved = scratch.path().join("hidden-fixture-blobs");
    let before_unreadable = history(&events).len();
    std::fs::rename(&blobs, &saved).unwrap();
    std::fs::create_dir(&blobs).unwrap();
    let mut next = waiting.clone();
    next["revision"] = json!(2);
    let refused = signal(
        scratch.path(),
        &events,
        "unreadable-review-history",
        "task.review_activity",
        ACTOR,
        &next,
    );
    std::fs::remove_dir(&blobs).unwrap();
    std::fs::rename(saved, blobs).unwrap();
    assert_eq!(
        refused["ok"], false,
        "unreadable verified history must fail closed: {refused}"
    );
    assert_eq!(history(&events).len(), before_unreadable);
}

#[path = "support/mod.rs"]
mod review_support;

struct ReviewRuntime(std::process::Child);
impl Drop for ReviewRuntime {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn review_runtime(events: &Path, scratch: &Path) -> (ReviewRuntime, String, PathBuf) {
    use std::io::BufRead;
    let mut child = graphhelm_process::command(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args(["serve", "--events"])
        .arg(events)
        .args(["--bind", "127.0.0.1:0", "--keyring"])
        .arg(scratch.join("keyring"))
        .args(["--key-id", "owner-key", "--require-lane-credential"])
        .env("GRAPHHELM_EVENTS_KEY", "01".repeat(32))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let guard = ReviewRuntime(child);
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let read = std::io::BufReader::new(stdout).read_line(&mut line);
        sender.send((read, line)).ok();
    });
    let (read, line) = receiver
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("isolated Runtime starts within budget");
    read.unwrap();
    let reply: Value = serde_json::from_str(&line).unwrap();
    let base = format!(
        "http://{}",
        reply["data"]["address"]
            .as_str()
            .unwrap_or_else(|| panic!("isolated Runtime start refused: {reply}"))
    );
    review_support::wait_for_health(&base);
    let token = review_support::token_path(events);
    for lane in ["review-author", "review-reader", "review-other"] {
        command()
            .args([
                "tasks",
                "credential",
                "--lane",
                lane,
                "--execution",
                RUN,
                "--token-file",
            ])
            .arg(&token)
            .assert()
            .success();
    }
    (guard, base, token)
}

fn review_record(
    base: &str,
    token: &Path,
    lane: &str,
    kind: &str,
    args: &[&str],
) -> std::process::Output {
    let recorder =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/task-record/task_record.py");
    let mut child = Command::from_std(graphhelm_process::command("python"));
    child
        .arg(recorder)
        .args([
            "--lane",
            lane,
            kind,
            "--issue",
            "1745",
            "--repo",
            "fixture/project",
            "--execution",
            RUN,
            "--url",
            base,
            "--token-file",
        ])
        .arg(token)
        .arg("--no-github")
        .args(args)
        .timeout(std::time::Duration::from_secs(20))
        .output()
        .unwrap()
}

/// The fixture's own lane credential; never an installed or owner credential.
fn review_post(base: &str, credential: &str, actor: &str, key: &str, envelope: &Value) -> Value {
    review_post_body(
        base,
        credential,
        actor,
        key,
        &json!({"signal":envelope}),
        &[],
        RUN,
    )
}

fn review_post_body(
    base: &str,
    credential: &str,
    actor: &str,
    key: &str,
    body: &Value,
    headers: &[(&str, &str)],
    execution: &str,
) -> Value {
    use std::io::{Read, Write};
    let (host, port, _) = review_support::split_url(base);
    assert_eq!(host, "127.0.0.1");
    let mut stream = std::net::TcpStream::connect((host.as_str(), port)).unwrap();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .unwrap();
    stream
        .set_write_timeout(Some(std::time::Duration::from_secs(10)))
        .unwrap();
    let body = body.to_string();
    let extra: String = headers
        .iter()
        .map(|(name, value)| {
            assert!(!name.contains(['\r', '\n']) && !value.contains(['\r', '\n']));
            format!("{name}: {value}\r\n")
        })
        .collect();
    write!(stream, "POST /v1/executions/{execution}/signal HTTP/1.1\r\nHost: {host}:{port}\r\nAuthorization: Bearer {}\r\nX-GraphHelm-Actor: {actor}\r\nX-GraphHelm-Actor-Type: agent\r\nIdempotency-Key: {key}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n{body}", credential.trim(), body.len()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let response = review_support::parse_response(&response).unwrap();
    serde_json::from_str(&response.body).unwrap()
}

fn review_evidence_fingerprint(events: &Path) -> Vec<(std::ffi::OsString, String)> {
    let mut entries: Vec<_> = std::fs::read_dir(events.join("blobs"))
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            let digest =
                graphhelm_graph::raw_content_sha256(&std::fs::read(entry.path()).unwrap()).unwrap();
            (entry.file_name(), digest.as_str().to_owned())
        })
        .collect();
    entries.sort();
    entries
}

#[cfg(unix)]
struct ReviewWritePause {
    path: PathBuf,
    permissions: std::fs::Permissions,
}
#[cfg(unix)]
impl ReviewWritePause {
    fn new(path: &Path) -> Self {
        use std::os::unix::fs::PermissionsExt;
        let permissions = std::fs::metadata(path).unwrap().permissions();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o500)).unwrap();
        Self {
            path: path.to_owned(),
            permissions,
        }
    }
}
#[cfg(unix)]
impl Drop for ReviewWritePause {
    fn drop(&mut self) {
        let _ = std::fs::set_permissions(&self.path, self.permissions.clone());
    }
}

/// Actual distinct lane credentials and the ordinary recorder drive the real sealed Runtime.
/// Captured `fresh["data"]` is tasks-review-activity-route.json, before the terminal transition;
/// no timestamps or semantic fields were normalized. Capture SHA256:
/// c31c025b9edac2bee72f655b3930cd1d7cda93dd8b47e6dd650eba6be499e40f.
/// Producer source at capture: base75035a99 plus this author working tree, file SHA256
/// a2d89fdd16a00c9bed3969ef7f092509c08dbe109075f180f751f0f1d846730c.
/// Regenerate with GRAPHHELM_REVIEW_ACTIVITY_FIXTURE=<fixture-path> and the exact test filter;
/// compare binding/transition properties, not wall-clock JSON bytes. Consumers derive now from
/// captured occurredAt and the served budget. This snapshot proves waiting, not terminal state.
#[test]
fn reviewer_activity_recorder_preserves_authority_retry_history_and_revision_order() {
    let scratch = tempfile::tempdir().unwrap();
    let events = start(scratch.path());
    let (_server, base, token_file) = review_runtime(&events, scratch.path());
    let read_token = review_support::read_token(&token_file);
    let head = "0123456789abcdef0123456789abcdef01234567";
    let record = |lane: &str, kind: &str, args: &[&str]| {
        let output = review_record(&base, &token_file, lane, kind, args);
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    };
    record(
        "review-author",
        "claimed",
        &[
            "--branch",
            "fixture-review",
            "--title",
            "Fixture claim",
            "--summary",
            "Real fixture author",
        ],
    );
    record(
        "review-author",
        "pr_opened",
        &[
            "--pr",
            "17",
            "--head",
            head,
            "--reviewer",
            "review-reader",
            "--title",
            "Fixture review",
            "--summary",
            "Real fixture PR",
        ],
    );
    record(
        "review-reader",
        "review_verdict",
        &[
            "--pr",
            "17",
            "--head",
            head,
            "--verdict",
            "APPROVE-WITH-RISK",
            "--comment-url",
            "https://github.com/fixture/project/pull/17#issuecomment-1",
        ],
    );
    let read = || {
        review_support::get_json(
            &format!("{base}/v1/executions/{RUN}/tasks?full=1"),
            Some(read_token.trim()),
        )
    };
    let before = read();
    let context = before["data"]["tasks"][0]["reviewContextSequence"]
        .as_u64()
        .unwrap()
        .to_string();
    let args = [
        "--pr",
        "17",
        "--head",
        head,
        "--review-context-sequence",
        &context,
        "--revision",
        "1",
        "--activity",
        "waiting",
        "--reason",
        "Awaiting independent dependency proof",
    ];
    let dry = review_record(
        &base,
        &token_file,
        "review-reader",
        "review_activity",
        &[args.as_slice(), &["--dry-run"]].concat(),
    );
    let transport: Value = serde_json::from_slice(&dry.stdout).unwrap();
    let key = transport["headers"]["Idempotency-Key"].as_str().unwrap();
    let credential_path = token_file.with_file_name(format!(
        "{}.agent.review-reader.token",
        token_file
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .trim_end_matches(".token")
    ));
    let credential = std::fs::read_to_string(&credential_path).unwrap();
    let original_if_match = before["data"]["headSequence"].as_u64().unwrap().to_string();
    let first = review_post_body(
        &base,
        &credential,
        "review-reader",
        key,
        &transport["body"],
        &[("If-Match", &original_if_match)],
        RUN,
    );
    assert_eq!(
        first["ok"], true,
        "actual scoped HTTP commit with If-Match: {first}"
    );
    let waiting = read();
    let count = waiting["data"]["records"].as_array().unwrap().len();
    let original_sequence = waiting["data"]["tasks"][0]["reviewActivity"]["sequence"]
        .as_u64()
        .unwrap();
    let mut retry_body = transport["body"].clone();
    retry_body["signal"]["emittedAt"] = json!("2099-01-01T00:00:00Z");
    let mut retry_doc: Value =
        serde_json::from_str(retry_body["signal"]["description"].as_str().unwrap()).unwrap();
    retry_doc["at"] = json!("2099-01-01T00:00:00Z");
    retry_body["signal"]["description"] = json!(retry_doc.to_string());
    let historical = || {
        let prior = read();
        let total = history(&events).len();
        let evidence = review_evidence_fingerprint(&events);
        #[cfg(unix)]
        let pause = ReviewWritePause::new(&events);
        let reply = review_post_body(
            &base,
            &credential,
            "review-reader",
            key,
            &retry_body,
            &[
                ("If-Match", &original_if_match),
                ("X-GraphHelm-Actor-Model", "gpt-6.1-sol"),
                ("X-GraphHelm-Actor-Effort", "low"),
                (
                    "X-GraphHelm-Actor-Session",
                    "previously-unseen-historical-session",
                ),
            ],
            RUN,
        );
        #[cfg(unix)]
        drop(pause);
        assert_eq!(
            reply["ok"], true,
            "verified history bypasses NEW-write CAS/presence: {reply}"
        );
        assert_eq!(
            reply["data"]["idempotency"]["originalDecisionSequence"],
            original_sequence
        );
        assert_eq!(
            history(&events).len(),
            total,
            "no signal/presence/wake append on acknowledgement"
        );
        assert_eq!(
            review_evidence_fingerprint(&events),
            evidence,
            "no sealed evidence creation/overwrite"
        );
        let after = read();
        assert_eq!(after["data"]["tasks"], prior["data"]["tasks"]);
        assert_eq!(
            after["data"]["records"], prior["data"]["records"],
            "original trusted timestamp remains"
        );
    };
    historical();
    // A genuinely new eligible statement still refuses the same stale expected head, before writes.
    let mut stale = retry_body.clone();
    stale["signal"]["id"] = json!("new-stale-head");
    let mut stale_doc = retry_doc.clone();
    stale_doc["revision"] = json!(2);
    stale["signal"]["description"] = json!(stale_doc.to_string());
    let total = history(&events).len();
    let evidence = review_evidence_fingerprint(&events);
    let refused = review_post_body(
        &base,
        &credential,
        "review-reader",
        "new-stale-head",
        &stale,
        &[("If-Match", &original_if_match)],
        RUN,
    );
    assert_eq!(refused["ok"], false);
    assert_eq!(
        refused["diagnostics"][0]["code"],
        "GHE001_SEQUENCE_CONFLICT"
    );
    assert_eq!(history(&events).len(), total);
    assert_eq!(review_evidence_fingerprint(&events), evidence);
    // Complete body identity includes accepted options outside the signal, even null/empty ones.
    for (field, value) in [("attachments", json!([])), ("evidenceOut", Value::Null)] {
        let mut changed = retry_body.clone();
        changed[field] = value;
        assert_eq!(
            review_post_body(&base, &credential, "review-reader", key, &changed, &[], RUN)["ok"],
            false
        );
    }
    assert_eq!(
        review_post_body(
            &base,
            &credential,
            "review-reader",
            key,
            &retry_body,
            &[],
            "other-execution"
        )["ok"],
        false
    );
    assert_eq!(history(&events).len(), total);

    // Wrong binding and absent credential all refuse without append or projection mutation.
    for extras in [
        vec!["--repo", "other/project"],
        vec!["--issue", "1746"],
        vec!["--pr", "18"],
        vec!["--head", "ffffffffffffffffffffffffffffffffffffffff"],
        vec!["--review-context-sequence", "1"],
    ] {
        let mut wrong = args.to_vec();
        wrong.extend(extras);
        assert!(
            !review_record(
                &base,
                &token_file,
                "review-reader",
                "review_activity",
                &wrong
            )
            .status
            .success()
        );
        assert_eq!(read()["data"]["records"], waiting["data"]["records"]);
    }
    assert!(
        !review_record(
            &base,
            &token_file,
            "review-missing",
            "review_activity",
            &args
        )
        .status
        .success()
    );
    assert_eq!(read()["data"]["records"], waiting["data"]["records"]);
    let task = &waiting["data"]["tasks"][0];
    assert_eq!(task["lane"], "review-author");
    assert_eq!(task["currentReviewer"], "review-reader");
    assert_eq!(task["step"], "merge");
    assert_eq!(
        task["approvedWithRisk"],
        before["data"]["tasks"][0]["approvedWithRisk"]
    );
    assert_eq!(task["clock"], before["data"]["tasks"][0]["clock"]);
    assert_eq!(
        task["authorLastSequence"],
        before["data"]["tasks"][0]["authorLastSequence"]
    );
    assert_eq!(waiting["data"]["leases"], before["data"]["leases"]);
    assert_eq!(
        task["reviewActivity"]["reason"],
        "Awaiting independent dependency proof"
    );
    let mut altered = transport["body"]["signal"].clone();
    let mut content: Value =
        serde_json::from_str(altered["description"].as_str().unwrap()).unwrap();
    content["reason"] = json!("Tampered same-key statement");
    content["revision"] = json!(2);
    altered["description"] = json!(content.to_string());
    assert_eq!(
        review_post(&base, &credential, "review-reader", key, &altered)["ok"],
        false
    );
    let other_path = credential_path.with_file_name(
        credential_path
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .replace("review-reader", "review-other"),
    );
    let other = std::fs::read_to_string(other_path).unwrap();
    assert_eq!(
        review_post(
            &base,
            &other,
            "review-reader",
            key,
            &transport["body"]["signal"]
        )["ok"],
        false
    );
    std::fs::write(&credential_path, "").unwrap();
    let unreadable_credential = review_record(
        &base,
        &token_file,
        "review-reader",
        "review_activity",
        &args,
    );
    std::fs::write(&credential_path, &credential).unwrap();
    assert!(!unreadable_credential.status.success());
    assert_eq!(read()["data"]["records"], waiting["data"]["records"]);
    record("review-reader", "review_activity", &args);
    assert_eq!(read()["data"]["records"].as_array().unwrap().len(), count);
    record(
        "review-reader",
        "review_activity",
        &[
            "--pr",
            "17",
            "--head",
            head,
            "--review-context-sequence",
            &context,
            "--revision",
            "2",
            "--activity",
            "resumed",
        ],
    );
    record("review-reader", "review_activity", &args);
    historical();
    let resumed = read();
    assert_eq!(resumed["data"]["tasks"][0]["reviewActivityRevision"], 2);
    assert_eq!(
        resumed["data"]["tasks"][0]["reviewActivity"]["activity"],
        "resumed"
    );
    assert_eq!(
        resumed["data"]["records"].as_array().unwrap().len(),
        count + 1
    );
    // Simultaneous exact-key HTTP submissions either acknowledge or report a single conflict;
    // one bounded retry after joining must acknowledge the one persisted statement.
    let concurrent_args = [
        "--pr",
        "17",
        "--head",
        head,
        "--review-context-sequence",
        &context,
        "--revision",
        "3",
        "--activity",
        "waiting",
        "--reason",
        "Concurrent deliberate statement",
    ];
    let concurrent_dry = review_record(
        &base,
        &token_file,
        "review-reader",
        "review_activity",
        &[concurrent_args.as_slice(), &["--dry-run"]].concat(),
    );
    assert!(concurrent_dry.status.success());
    let concurrent_transport: Value = serde_json::from_slice(&concurrent_dry.stdout).unwrap();
    let concurrent_key = concurrent_transport["headers"]["Idempotency-Key"]
        .as_str()
        .unwrap();
    let concurrent_body = &concurrent_transport["body"];
    let barrier = Arc::new(std::sync::Barrier::new(3));
    let outputs = std::thread::scope(|scope| {
        let children: Vec<_> = (0..2)
            .map(|_| {
                let barrier = Arc::clone(&barrier);
                let base = &base;
                let credential = &credential;
                scope.spawn(move || {
                    barrier.wait();
                    review_post_body(
                        base,
                        credential,
                        "review-reader",
                        concurrent_key,
                        concurrent_body,
                        &[],
                        RUN,
                    )
                })
            })
            .collect();
        barrier.wait();
        children
            .into_iter()
            .map(|child| child.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert!(outputs.iter().any(|reply| reply["ok"] == true));
    for reply in outputs.iter().filter(|reply| reply["ok"] != true) {
        let code = reply["diagnostics"][0]["code"].as_str().unwrap();
        assert!(
            matches!(
                code,
                "GHE001_SEQUENCE_CONFLICT" | "GHE003_IDEMPOTENCY_CONFLICT"
            ),
            "{reply}"
        );
    }
    let persisted = read();
    assert_eq!(
        persisted["data"]["records"].as_array().unwrap().len(),
        count + 2
    );
    let concurrent_sequence = persisted["data"]["tasks"][0]["reviewActivity"]["sequence"].clone();
    let total = history(&events).len();
    let evidence = review_evidence_fingerprint(&events);
    let retry = review_post_body(
        &base,
        &credential,
        "review-reader",
        concurrent_key,
        concurrent_body,
        &[],
        RUN,
    );
    assert_eq!(retry["ok"], true, "{retry}");
    assert_eq!(retry["data"]["idempotency"]["recognizedRetry"], true);
    assert_eq!(
        retry["data"]["idempotency"]["originalDecisionSequence"],
        concurrent_sequence
    );
    assert_eq!(
        history(&events).len(),
        total,
        "no append/presence/wake from exact concurrent retry"
    );
    assert_eq!(review_evidence_fingerprint(&events), evidence);
    assert_eq!(
        read()["data"]["records"],
        persisted["data"]["records"],
        "original trusted time unchanged"
    );
    assert_eq!(read()["data"]["tasks"], persisted["data"]["tasks"]);
    for lane in ["review-author", "review-other"] {
        assert!(
            !review_record(&base, &token_file, lane, "review_activity", &args)
                .status
                .success()
        );
    }
    record(
        "review-author",
        "review_assigned",
        &[
            "--pr",
            "17",
            "--head",
            head,
            "--reviewer",
            "review-other",
            "--revision",
            "5",
        ],
    );
    record(
        "review-author",
        "review_assigned",
        &[
            "--pr",
            "17",
            "--head",
            head,
            "--reviewer",
            "review-reader",
            "--revision",
            "6",
        ],
    );
    historical();
    let generation = read();
    let next_context = generation["data"]["tasks"][0]["reviewContextSequence"]
        .as_u64()
        .unwrap()
        .to_string();
    assert_ne!(next_context, context);
    // Delayed FIRST delivery cannot resurrect the former generation after A -> B -> A.
    assert!(
        !review_record(
            &base,
            &token_file,
            "review-reader",
            "review_activity",
            &[
                "--pr",
                "17",
                "--head",
                head,
                "--review-context-sequence",
                &context,
                "--revision",
                "3",
                "--activity",
                "waiting",
                "--reason",
                "Old first delivery"
            ]
        )
        .status
        .success()
    );
    record("review-reader", "review_activity", &args); // historical acknowledgement only
    assert!(read()["data"]["tasks"][0]["reviewActivity"].is_null());
    record(
        "review-reader",
        "review_activity",
        &[
            "--pr",
            "17",
            "--head",
            head,
            "--review-context-sequence",
            &next_context,
            "--revision",
            "1",
            "--activity",
            "waiting",
            "--reason",
            "Awaiting independent dependency proof",
        ],
    );
    if let Some(path) = std::env::var_os("GRAPHHELM_REVIEW_ACTIVITY_FIXTURE") {
        std::fs::write(path, serde_json::to_vec_pretty(&read()["data"]).unwrap()).unwrap();
    }
    // A hyphenated header is one exact prefix, not a starts_with match against another header.
    let mut prefixed = retry_body.clone();
    prefixed["signal"]["id"] = json!("prefix-fixture");
    let mut prefixed_doc = retry_doc.clone();
    prefixed_doc["reviewContextSequence"] = json!(next_context.parse::<u64>().unwrap());
    prefixed_doc["revision"] = json!(2);
    prefixed["signal"]["description"] = json!(prefixed_doc.to_string());
    let prefix = "fixture-review-with-hyphens";
    let presence_before = history(&events)
        .iter()
        .filter(|event| matches!(event.kind, EventKind::AgentPresenceDeclared(_)))
        .count();
    let new_reply = review_post_body(
        &base,
        &credential,
        "review-reader",
        prefix,
        &prefixed,
        &[
            ("X-GraphHelm-Actor-Model", "gpt-6.1-sol"),
            ("X-GraphHelm-Actor-Effort", "low"),
            (
                "X-GraphHelm-Actor-Session",
                "fresh-activity-positive-control",
            ),
        ],
        RUN,
    );
    assert_eq!(new_reply["ok"], true, "{new_reply}");
    assert_eq!(
        new_reply["data"]["decision"], "rejected",
        "non-actionable is still a fresh durable task record"
    );
    assert_eq!(
        history(&events)
            .iter()
            .filter(|event| matches!(event.kind, EventKind::AgentPresenceDeclared(_)))
            .count(),
        presence_before + 1,
        "fresh typed outcome retains ordinary post-append presence behavior"
    );
    let prefix_count = history(&events).len();
    let exact_retry = review_post_body(
        &base,
        &credential,
        "review-reader",
        prefix,
        &prefixed,
        &[],
        RUN,
    );
    assert_eq!(exact_retry["data"]["idempotency"]["recognizedRetry"], true);
    assert_eq!(history(&events).len(), prefix_count);
    prefixed_doc["revision"] = json!(3);
    prefixed["signal"]["description"] = json!(prefixed_doc.to_string());
    let divergent = review_post_body(
        &base,
        &credential,
        "review-reader",
        prefix,
        &prefixed,
        &[],
        RUN,
    );
    assert_eq!(divergent["ok"], false);
    assert_eq!(
        divergent["diagnostics"][0]["code"],
        "GHE003_IDEMPOTENCY_CONFLICT"
    );
    assert_eq!(history(&events).len(), prefix_count);
    // A new statement also needs its own evidence id, as the real recorder provides.
    prefixed["signal"]["id"] = json!("prefix-fixture-next-statement");
    let similar = review_post_body(
        &base,
        &credential,
        "review-reader",
        "fixture-review-with-hyphens-suffix",
        &prefixed,
        &[],
        RUN,
    );
    assert_eq!(
        similar["ok"],
        true,
        "{similar}; task={}",
        read()["data"]["tasks"][0]["reviewActivityRevision"]
    );
    let fresh = read();
    assert_eq!(fresh["data"]["tasks"][0]["reviewActivityRevision"], 3);
    // The closed recorder verifies a real GitHub PR; this isolated terminal transition is owner-signed.
    let closed = json!({"schema":"graphhelm-task-event-v1", "taskId":"issue-1745", "revision":6,
        "at":"2026-01-01T00:00:00Z", "pr":17, "reason":"abandoned", "by":17});
    assert_eq!(
        signal(
            scratch.path(),
            &events,
            "fixture-owner-close",
            "task.closed",
            ACTOR,
            &closed
        )["ok"],
        true
    );
    historical();
    let closed_count = history(&events).len();
    record("review-reader", "review_activity", &args);
    assert_eq!(history(&events).len(), closed_count);
    assert!(read()["data"]["tasks"].as_array().unwrap().is_empty());
    // A cold real HTTP reader must verify the sealed history. Warm readers deliberately cache
    // envelopes by generation/root/digest; moving their directory does not invalidate that cache.
    drop(_server); // release actual fixture Runtime ownership before a cold restart
    let before_missing = history(&events).len();
    let (_cold_server, cold_base, _) = review_runtime(&events, scratch.path());
    let blobs = events.join("blobs");
    let hidden = scratch.path().join("http-hidden-blobs");
    std::fs::create_dir(&hidden).unwrap();
    let blob_names: Vec<_> = std::fs::read_dir(&blobs)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    for name in &blob_names {
        std::fs::rename(blobs.join(name), hidden.join(name)).unwrap();
    }
    let missing = review_post_body(
        &cold_base,
        &credential,
        "review-reader",
        key,
        &retry_body,
        &[],
        RUN,
    );
    for name in &blob_names {
        std::fs::rename(hidden.join(name), blobs.join(name)).unwrap();
    }
    assert_eq!(
        missing["ok"], false,
        "cold missing sealed history must not become cached success: {missing}"
    );
    assert_eq!(history(&events).len(), before_missing);
    let retirement = json!({"schema":"graphhelm-task-event-v1", "taskId":"lanes", "revision":1,
        "at":"2026-01-01T00:00:00Z", "lanes":["review-reader"], "retired":true, "reason":"Retired fixture reviewer"});
    assert_eq!(
        signal(
            scratch.path(),
            &events,
            "fixture-retire-reader",
            "task.lane_retired",
            ACTOR,
            &retirement
        )["ok"],
        true
    );
    let total = history(&events).len();
    let evidence = review_evidence_fingerprint(&events);
    let retired = review_post_body(
        &cold_base,
        &credential,
        "review-reader",
        key,
        &retry_body,
        &[],
        RUN,
    );
    assert_eq!(
        retired["ok"], false,
        "known completed key cannot bypass current actor retirement: {retired}"
    );
    assert_eq!(history(&events).len(), total);
    assert_eq!(review_evidence_fingerprint(&events), evidence);
}

/// Linux observation of the actual post-admission evidence write, before PreparedAppend. A full
/// owned FIFO blocks that write; /proc confirms the child opened it after validation. The tests
/// never change production code or depend on how quickly either writer happens to run.
#[cfg(target_os = "linux")]
#[test]
fn review_activity_append_boundary_revalidates_after_conflicts() {
    use std::io::{Read, Write};
    use std::os::unix::fs::OpenOptionsExt;
    use std::time::{Duration, Instant};
    for scenario in ["reassigned", "closed", "unrelated", "duplicate"] {
        let scratch = tempfile::tempdir().unwrap();
        let events = start(scratch.path());
        let head = "0123456789abcdef0123456789abcdef01234567";
        let opened = json!({"schema":"graphhelm-task-event-v1", "taskId":"issue-1745", "revision":1,
            "at":"2026-01-01T00:00:00Z", "repo":"fixture/project", "pr":17, "headSha":head, "lane":ACTOR, "journeys":[]});
        assert_eq!(
            signal(
                scratch.path(),
                &events,
                "race-open",
                "task.pr_opened",
                ACTOR,
                &opened
            )["ok"],
            true
        );
        let mut assigned = json!({"schema":"graphhelm-task-event-v1", "taskId":"issue-1745", "revision":1,
            "at":"2026-01-01T00:00:00Z", "pr":17, "headSha":head, "reviewer":ACTOR, "ordinal":1});
        assert_eq!(
            signal(
                scratch.path(),
                &events,
                "race-assign",
                "task.review_assigned",
                ACTOR,
                &assigned
            )["ok"],
            true
        );
        let context = history(&events).last().unwrap().sequence;
        let waiting = json!({"schema":"graphhelm-task-event-v1", "taskId":"issue-1745", "revision":1,
            "at":"2099-01-01T00:00:00Z", "repo":"fixture/project", "pr":17, "headSha":head,
            "reviewer":ACTOR, "reviewContextSequence":context, "activity":"waiting", "reason":"Actual race observer"});
        let envelope = json!({"id":"race-wait", "source":{"type":"user", "id":ACTOR}, "type":"task.review_activity",
            "severity":"low", "description":waiting.to_string(), "evidence":["task"], "emittedAt":"2026-10-07T23:00:00Z"});
        let input = write(scratch.path(), "race-wait.json", &envelope);
        let fifo = scratch.path().join("post-validation.fifo");
        assert!(
            std::process::Command::new("mkfifo")
                .arg(&fifo)
                .status()
                .unwrap()
                .success()
        );
        let mut reader = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&fifo)
            .unwrap();
        let mut seed = std::fs::OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&fifo)
            .unwrap();
        let mut seeded = 0;
        loop {
            match seed.write(&[0_u8; 4096]) {
                Ok(n) => seeded += n,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                other => panic!("cannot fill owned FIFO: {other:?}"),
            }
        }
        let output_path = scratch.path().join("race-output.json");
        let output_file = std::fs::File::create(&output_path).unwrap();
        let child = graphhelm_process::command(assert_cmd::cargo::cargo_bin!("graphhelm"))
            .env("GRAPHHELM_EVENTS_KEY", "01".repeat(32))
            .args(["execution", "signal", "--events"])
            .arg(&events)
            .args(["--execution", RUN, "--signal"])
            .arg(input)
            .arg("--evidence-out")
            .arg(&fifo)
            .arg("--keyring")
            .arg(scratch.path().join("keyring"))
            .args(["--key-id", "owner-key"])
            .stdout(output_file)
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let mut child = ReviewRuntime(child);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "writer exited before post-validation handshake: {scenario}"
            );
            let opened_fifo = std::fs::read_dir(format!("/proc/{}/fd", child.0.id()))
                .unwrap()
                .flatten()
                .any(|entry| std::fs::read_link(entry.path()).ok().as_ref() == Some(&fifo));
            if opened_fifo {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "post-validation FIFO handshake timed out: {scenario}"
            );
            std::thread::yield_now();
        }
        let before = history(&events).len();
        let interference = match scenario {
            "reassigned" => {
                assigned["reviewer"] = json!("different-reader");
                assigned["revision"] = json!(2);
                signal(
                    scratch.path(),
                    &events,
                    "race-reassigned",
                    "task.review_assigned",
                    ACTOR,
                    &assigned,
                )
            }
            "closed" => signal(
                scratch.path(),
                &events,
                "race-closed",
                "task.closed",
                ACTOR,
                &json!({"schema":"graphhelm-task-event-v1", "taskId":"issue-1745", "revision":1,
                    "at":"2026-01-01T00:00:00Z", "pr":17, "reason":"abandoned", "by":17}),
            ),
            "duplicate" => signal(
                scratch.path(),
                &events,
                "race-wait",
                "task.review_activity",
                ACTOR,
                &waiting,
            ),
            _ => signal(
                scratch.path(),
                &events,
                "race-unrelated",
                "operator_note",
                ACTOR,
                &json!({"note":"unrelated append"}),
            ),
        };
        assert_eq!(interference["ok"], true, "{scenario}: {interference}");
        let after_interference = history(&events).len();
        assert_eq!(after_interference, before + 1);
        let mut drained = Vec::new();
        loop {
            let mut bytes = [0_u8; 4096];
            match reader.read(&mut bytes) {
                Ok(n) => drained.extend_from_slice(&bytes[..n]),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                other => panic!("cannot drain owned FIFO: {other:?}"),
            }
            if child.0.try_wait().unwrap().is_some() {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "append observer timed out: {scenario}"
            );
            std::thread::yield_now();
        }
        // A child may finish once only the first pipe page is drained. Read the remaining
        // buffered evidence after exit; do not assume exit implies an empty pipe.
        loop {
            let mut bytes = [0_u8; 4096];
            match reader.read(&mut bytes) {
                Ok(0) => break,
                Ok(n) => drained.extend_from_slice(&bytes[..n]),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                other => panic!("cannot finish draining owned FIFO: {other:?}"),
            }
        }
        assert!(
            drained.len() > seeded,
            "real post-validation evidence was written: {scenario}"
        );
        let observed: Value = serde_json::from_slice(&drained[seeded..]).unwrap();
        assert_eq!(observed["type"], "task.review_activity");
        let reply: Value = serde_json::from_slice(&std::fs::read(output_path).unwrap()).unwrap();
        assert_eq!(
            reply["ok"], false,
            "stale validated append must conflict: {scenario}: {reply}"
        );
        assert_eq!(
            reply["diagnostics"][0]["code"], "GHE001_SEQUENCE_CONFLICT",
            "{reply}"
        );
        assert_eq!(
            history(&events).len(),
            after_interference,
            "no stale append: {scenario}"
        );
        let retry = signal(
            scratch.path(),
            &events,
            "race-wait",
            "task.review_activity",
            ACTOR,
            &waiting,
        );
        // CLI invocations have fresh keys; only the unrelated append leaves a new write eligible.
        // Exact same-key duplicate acknowledgement is exercised by the HTTP recorder below.
        let admitted = scenario == "unrelated";
        assert_eq!(
            retry["ok"], admitted,
            "retry revalidates {scenario}: {retry}"
        );
        assert_eq!(
            history(&events).len(),
            after_interference + usize::from(scenario == "unrelated")
        );
    }
}

/// End-to-end signed Runtime admission + seal + append, not an isolated algorithm benchmark.
/// N/10N uses one task and three provisioned fixture lanes. No timing threshold is an oracle.
#[test]
fn review_activity_admission_scales_at_fixed_task_and_lane_count() {
    let head = "0123456789abcdef0123456789abcdef01234567";
    for count in [10_u64, 100] {
        let scratch = tempfile::tempdir().unwrap();
        let events = start(scratch.path());
        let (_server, base, token_file) = review_runtime(&events, scratch.path());
        for (kind, args) in [
            (
                "claimed",
                vec![
                    "--branch",
                    "scale-fixture",
                    "--title",
                    "Scale claim",
                    "--summary",
                    "Fixed-lane admission fixture",
                ],
            ),
            (
                "pr_opened",
                vec![
                    "--pr",
                    "17",
                    "--head",
                    head,
                    "--reviewer",
                    "review-reader",
                    "--title",
                    "Scale review",
                    "--summary",
                    "Fixed-lane admission fixture",
                ],
            ),
        ] {
            assert!(
                review_record(&base, &token_file, "review-author", kind, &args)
                    .status
                    .success()
            );
        }
        let read_token = review_support::read_token(&token_file);
        let read = || {
            review_support::get_json(
                &format!("{base}/v1/executions/{RUN}/tasks?full=1"),
                Some(read_token.trim()),
            )
        };
        let before = read();
        let context = before["data"]["tasks"][0]["reviewContextSequence"]
            .as_u64()
            .unwrap();
        let credential =
            std::fs::read_to_string(token_file.with_file_name("events.agent.review-reader.token"))
                .unwrap();
        let started = std::time::Instant::now();
        for revision in 1..=count {
            let key = format!("scale-review-activity-{revision}");
            let document = json!({"schema":"graphhelm-task-event-v1", "taskId":"issue-1745", "revision":revision,
                "at":"2099-01-01T00:00:00Z", "repo":"fixture/project", "pr":17, "headSha":head,
                "reviewer":"review-reader", "reviewContextSequence":context, "activity":"waiting", "reason":"Measured actual fixture statement"});
            let envelope = json!({"id":key, "source":{"type":"user","id":"review-reader"}, "type":"task.review_activity",
                "severity":"low", "description":document.to_string(), "evidence":["task"], "emittedAt":"2099-01-01T00:00:00Z"});
            let reply = review_post(&base, &credential, "review-reader", &key, &envelope);
            assert_eq!(reply["ok"], true, "revision {revision}: {reply}");
        }
        let elapsed = started.elapsed();
        let after = read();
        assert_eq!(after["data"]["tasks"][0]["reviewActivityRevision"], count);
        assert_eq!(
            after["data"]["records"].as_array().unwrap().len(),
            before["data"]["records"].as_array().unwrap().len() + count as usize
        );
        eprintln!(
            "review_activity end_to_end requests={count} tasks=1 fixture_lanes=3 elapsed_us={} records={}",
            elapsed.as_micros(),
            after["data"]["records"].as_array().unwrap().len()
        );
    }
}
