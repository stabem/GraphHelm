use std::path::{Path, PathBuf};
use std::sync::Arc;

use assert_cmd::Command;
use chrono::TimeZone;
use graphhelm_protocols::{EventEnvelope, EventKind};
use serde_json::{Value, json};

fn command() -> Command {
    let mut command = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"));
    command.env("GRAPHHELM_EVENTS_KEY", "01".repeat(32));
    command
}

fn write(directory: &Path, name: &str, value: &Value) -> PathBuf {
    let path = directory.join(name);
    std::fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
    path
}

struct FixedClock;
impl graphhelm_protocols::Clock for FixedClock {
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        chrono::Utc.with_ymd_and_hms(2026, 10, 6, 12, 0, 0).unwrap()
    }
}
struct ReadOnlyIds;
impl graphhelm_protocols::IdGenerator for ReadOnlyIds {
    fn next_id(&self, _: &'static str) -> String {
        panic!("this reader must not append")
    }
}

fn history(events: &Path, run: &str) -> Vec<EventEnvelope> {
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
        .find(|stream| stream.stream_id.as_str() == run)
        .unwrap();
    store
        .read_replay_stream(&stream.scope, &stream.stream_id)
        .unwrap()
}

struct Fixture {
    scratch: tempfile::TempDir,
    events: PathBuf,
    keyring: PathBuf,
}

const RUN: &str = "owner-records";

fn fixture() -> Fixture {
    let scratch = tempfile::tempdir().unwrap();
    let events = scratch.path().join("runtime-data");
    let keyring = scratch.path().join("keyring");
    std::fs::create_dir(&keyring).unwrap();
    graphhelm_sealed_key_provider::SealedKeyProvider::create(
        &keyring,
        "owner-key",
        graphhelm_events::SecretBytes::new(vec![1; 32]),
    )
    .unwrap();
    let fixtures = write(scratch.path(), "fixtures.json", &json!({"nodeOutcomes":{}}));
    let graph = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/graphs/manual-override-deploy.yaml");
    let output = command()
        .args([
            "execution",
            "start",
            "--events",
            events.to_str().unwrap(),
            "--execution",
            RUN,
            "--file",
            graph.to_str().unwrap(),
            "--fixtures",
            fixtures.to_str().unwrap(),
            "--mode",
            "manual",
            "--held",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    Fixture {
        scratch,
        events,
        keyring,
    }
}

fn send(fixture: &Fixture, name: &str, signal: &Value) -> (std::process::Output, PathBuf) {
    let path = write(fixture.scratch.path(), &format!("{name}.json"), signal);
    let evidence_out = fixture.scratch.path().join(format!("{name}-evidence.json"));
    let output = command()
        .args([
            "execution",
            "signal",
            "--events",
            fixture.events.to_str().unwrap(),
            "--execution",
            RUN,
            "--signal",
            path.to_str().unwrap(),
            "--evidence-out",
            evidence_out.to_str().unwrap(),
            "--keyring",
            fixture.keyring.to_str().unwrap(),
            "--key-id",
            "owner-key",
        ])
        .output()
        .unwrap();
    (output, evidence_out)
}

fn assert_refused(fixture: &Fixture, name: &str, signal: &Value) {
    let before = history(&fixture.events, RUN).len();
    let (output, evidence_out) = send(fixture, name, signal);
    assert!(!output.status.success(), "{name} was accepted");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        value["diagnostics"][0]["code"], "GHCLI003_SIGNAL_INVALID",
        "{name}"
    );
    assert!(!evidence_out.exists(), "{name} wrote evidence");
    assert_eq!(
        history(&fixture.events, RUN).len(),
        before,
        "{name} appended"
    );
}

fn owner_signal(id: &str, kind: &str, description: &Value) -> Value {
    json!({"id":id,"source":{"type":"user","id":"owner"},"type":kind,"severity":"low",
        "description":description.to_string(),"evidence":["owner"],
        "emittedAt":"2026-10-06T12:00:00Z"})
}

#[test]
fn alias_for_an_actor_absent_from_the_run_or_for_codex_is_refused_without_writes() {
    let fixture = fixture();
    let name = json!({"protocol":"graphhelm-actor-alias-v1","displayName":"Planner"});
    for (case, to) in [("absent", "claude-absent"), ("codex", "codex")] {
        let mut signal = owner_signal(&format!("alias-{case}"), "actor_alias", &name);
        signal["to"] = json!(to);
        assert_refused(&fixture, &format!("alias-{case}"), &signal);
    }
}

#[test]
fn refusal_without_reply_to_or_replying_to_an_owner_signal_is_refused_without_writes() {
    let fixture = fixture();
    let body = json!({"protocol":"graphhelm-owner-refusal-v1","reason":"Not now"});
    assert_refused(
        &fixture,
        "refusal-no-reply",
        &owner_signal("refusal-no-reply", "owner_refusal", &body),
    );

    let (output, _) = send(
        &fixture,
        "owner-note",
        &owner_signal("owner-note", "operator_note", &json!({"note":"hello"})),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let owner_signal_id = history(&fixture.events, RUN)
        .into_iter()
        .rev()
        .find_map(|event| match event.kind {
            EventKind::SignalRecorded(signal) => Some(signal.signal_id.to_string()),
            _ => None,
        })
        .unwrap();
    let mut refusal = owner_signal("refusal-owner", "owner_refusal", &body);
    refusal["replyTo"] = json!(owner_signal_id);
    assert_refused(&fixture, "refusal-owner", &refusal);
}
