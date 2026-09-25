use std::path::{Path, PathBuf};
use std::sync::Arc;

use assert_cmd::Command;
use chrono::TimeZone;
use serde_json::{Value, json};

fn command() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
}
fn write(directory: &Path, name: &str, value: &Value) -> PathBuf {
    let path = directory.join(name);
    std::fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
    path
}
fn data(output: std::process::Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    serde_json::from_slice::<Value>(&output.stdout).unwrap()["data"].clone()
}
fn head(events: &Path) -> Value {
    data(
        command()
            .args([
                "execution",
                "status",
                "--events",
                events.to_str().unwrap(),
                "--execution",
                "delivery-run",
            ])
            .output()
            .unwrap(),
    )["headSequence"]
        .clone()
}

fn assert_no_plaintext(directory: &Path, needle: &[u8]) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            assert_no_plaintext(&entry.path(), needle);
        } else {
            let bytes = std::fs::read(entry.path()).unwrap();
            assert!(!bytes.windows(needle.len()).any(|window| window == needle));
        }
    }
}

struct FixedClock;
impl graphhelm_protocols::Clock for FixedClock {
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        chrono::Utc.with_ymd_and_hms(2026, 9, 14, 12, 0, 0).unwrap()
    }
}
struct ReadOnlyIds;
impl graphhelm_protocols::IdGenerator for ReadOnlyIds {
    fn next_id(&self, _: &'static str) -> String {
        panic!("read-only observer must not append")
    }
}

fn assert_delivery_actor(events: &Path) {
    let store = graphhelm_events::LocalEventRepository::open(
        events,
        Arc::new(FixedClock),
        Arc::new(ReadOnlyIds),
    )
    .unwrap();
    let (_, events) = store.read_unique_replay_stream().unwrap();
    let deliveries: Vec<_> = events.into_iter().filter(|event| matches!(&event.kind, graphhelm_protocols::EventKind::SignalRecorded(record) if record.kind == "node_delivery")).collect();
    assert_eq!(deliveries.len(), 2);
    for delivery in deliveries {
        assert_eq!(
            delivery.actor.actor_type(),
            graphhelm_protocols::PersistedActorType::Agent
        );
        assert_eq!(delivery.actor.id().as_str(), "writer-agent");
    }
}

#[test]
fn progressive_deliveries_are_sealed_and_invalid_sources_never_append() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let fixtures = write(
        directory.path(),
        "fixtures.json",
        &json!({"nodeOutcomes":{}}),
    );
    let graph = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/graphs/manual-override-deploy.yaml");
    data(
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
                "manual",
                "--execution",
                "delivery-run",
            ])
            .output()
            .unwrap(),
    );
    let keyring = directory.path().join("keyring");
    std::fs::create_dir(&keyring).unwrap();
    graphhelm_sealed_key_provider::SealedKeyProvider::create(
        &keyring,
        "delivery-key",
        graphhelm_events::SecretBytes::new(vec![1; 32]),
    )
    .unwrap();
    let mut record = json!({"version":1,"summary":"Created retry rule","reason":"Explain checkout failures","documents":[{"path":"docs/prd.md","title":"Checkout","kind":"business_rule","action":"created","ruleIds":["retry"]}]});
    let delivery = write(directory.path(), "delivery.json", &record);
    let invoke = |node: &str, actor: &str| {
        command()
            .args([
                "execution",
                "delivery",
                "--events",
                events.to_str().unwrap(),
                "--execution",
                "delivery-run",
                "--node",
                node,
                "--delivery",
                delivery.to_str().unwrap(),
                "--project-directory",
                directory.path().to_str().unwrap(),
                "--keyring",
                keyring.to_str().unwrap(),
                "--key-id",
                "delivery-key",
                "--actor-id",
                actor,
            ])
            .env("GRAPHHELM_EVENTS_KEY", "01".repeat(32))
            .output()
            .unwrap()
    };
    let before = head(&events).as_u64().unwrap();
    let first = data(invoke("implementation", "writer-agent"));
    assert_eq!(first["provenance"], "reported");
    assert_eq!(first["rejectionReason"], "signal_not_actionable");
    assert_eq!(head(&events).as_u64().unwrap(), before + 1);
    record["documents"][0]["action"] = "updated".into();
    write(directory.path(), "delivery.json", &record);
    let second = data(invoke("implementation", "writer-agent"));
    assert_ne!(first["signalId"], second["signalId"]);
    assert_eq!(head(&events).as_u64().unwrap(), before + 2);
    assert_delivery_actor(&events);
    let invalid_actor = invoke("implementation", "bad actor id");
    assert!(!invalid_actor.status.success());
    let invalid_actor: Value = serde_json::from_slice(&invalid_actor.stdout).unwrap();
    assert_eq!(
        invalid_actor["diagnostics"][0]["code"],
        "GHCLI001_ARGUMENT_INVALID"
    );
    assert_eq!(head(&events).as_u64().unwrap(), before + 2);
    let secret_actor = invoke("implementation", "sk-abcdefghijklmnopqrst");
    assert!(!secret_actor.status.success());
    let secret_actor: Value = serde_json::from_slice(&secret_actor.stdout).unwrap();
    assert_eq!(
        secret_actor["diagnostics"][0]["code"],
        "GHCLI001_ARGUMENT_INVALID"
    );
    assert_eq!(head(&events).as_u64().unwrap(), before + 2);
    let missing = invoke("missing-node", "writer-agent");
    assert!(!missing.status.success());
    assert_eq!(head(&events).as_u64().unwrap(), before + 2);
    record["documents"][0]["path"] = "../escape.md".into();
    write(directory.path(), "delivery.json", &record);
    assert!(!invoke("implementation", "writer-agent").status.success());
    assert_eq!(head(&events).as_u64().unwrap(), before + 2);
    let envelope = write(
        directory.path(),
        "signal.json",
        &json!({
            "id":"bad-delivery", "source":{"type":"node","id":"implementation"},
            "type":"node_delivery", "severity":"low", "description":"not a structured record",
            "evidence":["agent-reported-delivery"], "emittedAt":"2026-09-14T12:00:00Z"
        }),
    );
    let plaintext = directory.path().join("must-not-exist.json");
    let refused = command()
        .args([
            "execution",
            "signal",
            "--events",
            events.to_str().unwrap(),
            "--execution",
            "delivery-run",
            "--signal",
            envelope.to_str().unwrap(),
            "--evidence-out",
            plaintext.to_str().unwrap(),
            "--keyring",
            keyring.to_str().unwrap(),
            "--key-id",
            "delivery-key",
        ])
        .env("GRAPHHELM_EVENTS_KEY", "01".repeat(32))
        .output()
        .unwrap();
    assert!(!refused.status.success());
    assert!(!plaintext.exists());
    assert_eq!(head(&events).as_u64().unwrap(), before + 2);
    // No document or plaintext evidence file is created by recording a reported delivery.
    assert!(!directory.path().join("docs/prd.md").exists());
    assert_no_plaintext(&events, b"Created retry rule");
    assert_no_plaintext(&events, b"Explain checkout failures");
}
