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

const RUN: &str = "image-evidence";

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

const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3];
const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 4, 5];
const WEBP: &[u8] = b"RIFF\x04\0\0\0WEBPVP8 ";

fn image(fixture: &Fixture, name: &str, bytes: &[u8]) -> PathBuf {
    let path = fixture.scratch.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

fn note(id: &str) -> Value {
    json!({"id":id,"source":{"type":"user","id":"owner"},"type":"operator_note","severity":"low",
        "description":"see the screenshot","evidence":["owner"],
        "emittedAt":"2026-10-06T12:00:00Z"})
}

fn send(fixture: &Fixture, id: &str, attach: &[PathBuf]) -> (std::process::Output, PathBuf) {
    let path = write(fixture.scratch.path(), &format!("{id}.json"), &note(id));
    let evidence_out = fixture.scratch.path().join(format!("{id}-evidence.json"));
    let mut command = command();
    command.args([
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
    ]);
    for file in attach {
        command.arg("--attach").arg(file);
    }
    (command.output().unwrap(), evidence_out)
}

fn last_signal_refs(events: &Path) -> Vec<(String, String)> {
    history(events, RUN)
        .into_iter()
        .rev()
        .find(|event| matches!(event.kind, EventKind::SignalRecorded(_)))
        .unwrap()
        .evidence_refs
        .iter()
        .map(|reference| {
            (
                reference.evidence_id().as_str().to_owned(),
                reference.content_sha256().as_str().to_owned(),
            )
        })
        .collect()
}

#[test]
fn each_image_type_records_as_its_own_sealed_evidence_in_request_order() {
    let fixture = fixture();
    let files = [
        image(&fixture, "a.bin", PNG),
        image(&fixture, "b.png", JPEG),
        image(&fixture, "c.jpg", WEBP),
    ];
    let (output, _) = send(&fixture, "shots", &files);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let reply: Value = serde_json::from_slice(&output.stdout).unwrap();
    let attachments = reply["data"]["attachments"].as_array().unwrap();
    let expected = [
        ("image/png", PNG),
        ("image/jpeg", JPEG),
        ("image/webp", WEBP),
    ];
    for (index, (media_type, bytes)) in expected.iter().enumerate() {
        assert_eq!(
            attachments[index]["evidenceId"],
            format!("signal-shots-image-{}", index + 1)
        );
        assert_eq!(attachments[index]["mediaType"], *media_type);
        assert_eq!(attachments[index]["bytes"], bytes.len());
    }
    let refs = last_signal_refs(&fixture.events);
    let ids: Vec<&str> = refs.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "signal-shots",
            "signal-shots-image-1",
            "signal-shots-image-2",
            "signal-shots-image-3"
        ]
    );
    for (index, (_, bytes)) in expected.iter().enumerate() {
        let digest = graphhelm_graph::raw_content_sha256(bytes).unwrap();
        assert_eq!(refs[index + 1].1, digest.as_str());
    }
}

#[test]
fn a_signal_without_attachments_has_no_attachments_key() {
    let fixture = fixture();
    let (output, _) = send(&fixture, "plain", &[]);
    assert!(output.status.success());
    let reply: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(reply["data"].get("attachments").is_none());
    assert_eq!(last_signal_refs(&fixture.events).len(), 1);
}

fn assert_refused(fixture: &Fixture, id: &str, attach: &[PathBuf]) {
    let before = history(&fixture.events, RUN).len();
    let (output, evidence_out) = send(fixture, id, attach);
    assert!(!output.status.success(), "{id} was accepted");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        value["diagnostics"][0]["code"], "GHCLI003_SIGNAL_INVALID",
        "{id}"
    );
    assert!(!evidence_out.exists(), "{id} wrote evidence");
    assert_eq!(history(&fixture.events, RUN).len(), before, "{id} appended");
}

#[test]
fn wrong_magic_svg_oversized_and_too_many_are_refused_without_writes() {
    let fixture = fixture();
    let mut big = PNG.to_vec();
    big.resize(8 * 1024 * 1024 + 1, 0);
    let png = image(&fixture, "ok.png", PNG);
    let cases = [
        ("magic", vec![image(&fixture, "fake.png", b"GIF89a....")]),
        (
            "svg",
            vec![image(
                &fixture,
                "x.svg",
                b"<svg xmlns='http://www.w3.org/2000/svg'/>",
            )],
        ),
        ("big", vec![image(&fixture, "big.png", &big)]),
        ("five", vec![png.clone(); 5]),
    ];
    for (id, files) in cases {
        assert_refused(&fixture, id, &files);
    }
}

#[test]
fn a_valid_image_followed_by_an_invalid_one_appends_nothing() {
    let fixture = fixture();
    let files = [
        image(&fixture, "good.png", PNG),
        image(&fixture, "bad.png", b"not an image"),
    ];
    assert_refused(&fixture, "partial", &files);
}
