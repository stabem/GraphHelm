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

fn data(output: std::process::Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    serde_json::from_slice::<Value>(&output.stdout).unwrap()["data"].clone()
}

fn refusal(output: std::process::Output, code: &str) {
    assert!(!output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["diagnostics"][0]["code"], code);
}

fn write(directory: &Path, name: &str, value: &Value) -> PathBuf {
    let path = directory.join(name);
    std::fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
    path
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

fn delivery_ref(events: &Path, run: &str) -> String {
    history(events, run).into_iter().find(|event| matches!(&event.kind, EventKind::SignalRecorded(signal) if signal.kind == "node_delivery"))
        .unwrap().evidence_refs[0].evidence_id().as_str().to_owned()
}

fn status(events: &Path, run: &str) -> Value {
    data(
        command()
            .args([
                "execution",
                "status",
                "--events",
                events.to_str().unwrap(),
                "--execution",
                run,
            ])
            .output()
            .unwrap(),
    )
}

#[test]
fn edit_reaches_main_project_and_only_associated_runs_without_resuming_them() {
    let scratch = tempfile::tempdir().unwrap();
    let project = scratch.path().join("project");
    let events = project.join("runtime-data");
    let other_project = scratch.path().join("other-project");
    std::fs::create_dir_all(project.join("docs")).unwrap();
    std::fs::create_dir_all(other_project.join("docs")).unwrap();
    let file = project.join("docs/rules.md");
    std::fs::write(&file, "Retry once.\n").unwrap();
    std::fs::write(other_project.join("docs/rules.md"), "Other project.\n").unwrap();
    // An arbitrary directory name must not bypass protection of the configured keyring.
    let keyring = project.join("vaultdata");
    std::fs::create_dir(&keyring).unwrap();
    graphhelm_sealed_key_provider::SealedKeyProvider::create(
        &keyring,
        "documents-key",
        graphhelm_events::SecretBytes::new(vec![1; 32]),
    )
    .unwrap();
    let keyring_document = write(&keyring, "test.json", &json!({"fixture":"protected"}));
    let keyring_before = std::fs::read(&keyring_document).unwrap();
    let fixtures = write(scratch.path(), "fixtures.json", &json!({"nodeOutcomes":{}}));
    let graph = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/graphs/manual-override-deploy.yaml");
    let delivery = write(
        scratch.path(),
        "delivery.json",
        &json!({"version":1,"summary":"Added retry rule","reason":"Map checkout recovery","documents":[{"path":"docs/rules.md","title":"Retry policy","kind":"business_rule","action":"created","journeyIds":["checkout"],"ruleIds":["retry"]},{"path":"vaultdata/test.json","title":"Reported configuration","kind":"file","action":"reviewed"},{"path":"runtime-data/format.json","title":"Reported runtime format","kind":"file","action":"reviewed"}]}),
    );
    for run in ["documents-a", "documents-b", "documents-unrelated"] {
        data(
            command()
                .args([
                    "execution",
                    "start",
                    "--events",
                    events.to_str().unwrap(),
                    "--execution",
                    run,
                    "--file",
                    graph.to_str().unwrap(),
                    "--fixtures",
                    fixtures.to_str().unwrap(),
                    "--mode",
                    "manual",
                    "--held",
                ])
                .output()
                .unwrap(),
        );
        assert_eq!(status(&events, run)["status"], "paused");
        if run != "documents-unrelated" {
            data(
                command()
                    .args([
                        "execution",
                        "delivery",
                        "--events",
                        events.to_str().unwrap(),
                        "--execution",
                        run,
                        "--node",
                        "implementation",
                        "--project-directory",
                        project.to_str().unwrap(),
                        "--delivery",
                        delivery.to_str().unwrap(),
                        "--keyring",
                        keyring.to_str().unwrap(),
                        "--key-id",
                        "documents-key",
                    ])
                    .output()
                    .unwrap(),
            );
        }
    }
    let evidence = delivery_ref(&events, "documents-a");
    let owner_report = history(&events, "documents-a");
    assert_eq!(
        owner_report.last().unwrap().actor.actor_type(),
        graphhelm_protocols::PersistedActorType::Owner
    );
    let read = |run: &str, configured: &Path| {
        command()
            .args([
                "execution",
                "document-read",
                "--events",
                events.to_str().unwrap(),
                "--execution",
                run,
                "--project-directory",
                configured.to_str().unwrap(),
                "--evidence-id",
                &evidence,
                "--index",
                "0",
                "--keyring",
                keyring.to_str().unwrap(),
                "--key-id",
                "documents-key",
            ])
            .output()
            .unwrap()
    };
    let snapshot = data(read("documents-a", &project));
    assert_eq!(snapshot["content"], "Retry once.\n");
    assert_eq!(snapshot["target"], "main_project");
    refusal(read("documents-b", &project), "GHCLI001_ARGUMENT_INVALID");
    refusal(
        read("documents-a", &other_project),
        "GHCLI005_EXECUTION_STATE",
    );
    let before_a = history(&events, "documents-a").len();
    let before_b = history(&events, "documents-b").len();
    let unrelated_head = history(&events, "documents-unrelated").len();
    let mut edit = json!({"document":{"evidenceId":evidence,"index":0},"content":"Retry twice with confirmation.\n","expectedSha256":"0".repeat(64),"reason":"Owner clarified recovery","idempotencyKey":"owner-change-1"});
    let edit_path = write(scratch.path(), "edit.json", &edit);
    let save = || {
        command()
            .args([
                "execution",
                "document-save",
                "--events",
                events.to_str().unwrap(),
                "--execution",
                "documents-a",
                "--project-directory",
                project.to_str().unwrap(),
                "--edit",
                edit_path.to_str().unwrap(),
                "--keyring",
                keyring.to_str().unwrap(),
                "--key-id",
                "documents-key",
            ])
            .output()
            .unwrap()
    };
    edit["content"] = "invalid\0content".into();
    edit["expectedSha256"] = snapshot["contentSha256"].clone();
    write(scratch.path(), "edit.json", &edit);
    refusal(save(), "GHCLI005_EXECUTION_STATE");
    assert_eq!(history(&events, "documents-a").len(), before_a);
    assert_eq!(history(&events, "documents-b").len(), before_b);
    edit["content"] = "Retry twice with confirmation.\n".into();
    edit["expectedSha256"] = "0".repeat(64).into();
    write(scratch.path(), "edit.json", &edit);
    refusal(save(), "GHCLI005_EXECUTION_STATE");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "Retry once.\n");
    assert_eq!(history(&events, "documents-a").len(), before_a);
    edit["expectedSha256"] = snapshot["contentSha256"].clone();
    write(scratch.path(), "edit.json", &edit);
    let saved = data(save());
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "Retry twice with confirmation.\n"
    );
    assert_eq!(saved["notification"]["status"], "recorded");
    assert_eq!(
        saved["notification"]["notifiedRuns"],
        json!(["documents-a", "documents-b"])
    );
    assert_eq!(saved["notification"]["pendingRuns"], json!([]));
    assert_eq!(history(&events, "documents-a").len(), before_a + 3);
    assert_eq!(history(&events, "documents-b").len(), before_b + 1);
    assert_eq!(
        history(&events, "documents-unrelated").len(),
        unrelated_head
    );
    for run in ["documents-a", "documents-b"] {
        let notices: Vec<_> = history(&events, run).into_iter().filter(|event| matches!(&event.kind, EventKind::SignalRecorded(signal) if signal.kind == "owner_document_changed")).collect();
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].evidence_refs.len(), 1);
        assert_eq!(status(&events, run)["status"], "paused");
    }
    assert_eq!(
        data(save()),
        saved,
        "same identity/body returns the same receipt"
    );
    assert_eq!(history(&events, "documents-a").len(), before_a + 3);
    assert_eq!(history(&events, "documents-b").len(), before_b + 1);
    edit["content"] = "A different edit using the same key.\n".into();
    write(scratch.path(), "edit.json", &edit);
    refusal(save(), "GHCLI005_EXECUTION_STATE");
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "Retry twice with confirmation.\n"
    );
    assert_eq!(history(&events, "documents-a").len(), before_a + 3);
    assert_eq!(
        std::fs::read_to_string(other_project.join("docs/rules.md")).unwrap(),
        "Other project.\n"
    );
    // A receipt describes its historical edit; retrying it must not restore old bytes over a
    // later edit made by an agent or another owner session.
    edit["content"] = "Retry twice with confirmation.\n".into();
    write(scratch.path(), "edit.json", &edit);
    std::fs::write(&file, "A newer independent revision.\n").unwrap();
    assert_eq!(data(save()), saved);
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "A newer independent revision.\n"
    );
    assert_eq!(history(&events, "documents-a").len(), before_a + 3);
    assert_eq!(history(&events, "documents-b").len(), before_b + 1);

    refusal(
        command()
            .args([
                "execution",
                "document-read",
                "--events",
                events.to_str().unwrap(),
                "--execution",
                "documents-a",
                "--project-directory",
                project.to_str().unwrap(),
                "--evidence-id",
                &evidence,
                "--index",
                "1",
                "--keyring",
                keyring.to_str().unwrap(),
                "--key-id",
                "documents-key",
            ])
            .output()
            .unwrap(),
        "GHCLI005_EXECUTION_STATE",
    );
    edit["document"]["index"] = 1.into();
    edit["content"] = "{\"fixture\":\"changed\"}".into();
    edit["expectedSha256"] = graphhelm_graph::raw_content_sha256(&keyring_before)
        .unwrap()
        .as_str()
        .into();
    edit["idempotencyKey"] = "protected-directory-edit".into();
    write(scratch.path(), "edit.json", &edit);
    refusal(save(), "GHCLI005_EXECUTION_STATE");
    assert_eq!(std::fs::read(&keyring_document).unwrap(), keyring_before);
    assert_eq!(history(&events, "documents-a").len(), before_a + 3);

    // The Event Store is protected by its configured location, even with an ordinary folder
    // name and a real, registered JSON file. Refusal must leave replay usable and unchanged.
    let format_file = events.join("format.json");
    let format_before = std::fs::read(&format_file).unwrap();
    refusal(
        command()
            .args([
                "execution",
                "document-read",
                "--events",
                events.to_str().unwrap(),
                "--execution",
                "documents-a",
                "--project-directory",
                project.to_str().unwrap(),
                "--evidence-id",
                &evidence,
                "--index",
                "2",
                "--keyring",
                keyring.to_str().unwrap(),
                "--key-id",
                "documents-key",
            ])
            .output()
            .unwrap(),
        "GHCLI005_EXECUTION_STATE",
    );
    edit["document"]["index"] = 2.into();
    edit["content"] = "{}".into();
    edit["expectedSha256"] = graphhelm_graph::raw_content_sha256(&format_before)
        .unwrap()
        .as_str()
        .into();
    edit["idempotencyKey"] = "protected-event-store-edit".into();
    write(scratch.path(), "edit.json", &edit);
    refusal(save(), "GHCLI005_EXECUTION_STATE");
    assert_eq!(std::fs::read(&format_file).unwrap(), format_before);
    assert_eq!(history(&events, "documents-a").len(), before_a + 3);
    assert_eq!(history(&events, "documents-b").len(), before_b + 1);

    let malformed = write(
        scratch.path(),
        "malformed-notice.json",
        &json!({"id":"malformed-owner-notice","source":{"type":"user","id":"owner"},"type":"owner_document_changed","severity":"medium","description":"plain prose is not a typed owner change","evidence":["reported-change"],"emittedAt":"2026-09-14T12:00:00Z"}),
    );
    let evidence_out = scratch.path().join("must-not-write-notice.json");
    refusal(
        command()
            .args([
                "execution",
                "signal",
                "--events",
                events.to_str().unwrap(),
                "--execution",
                "documents-a",
                "--signal",
                malformed.to_str().unwrap(),
                "--evidence-out",
                evidence_out.to_str().unwrap(),
                "--keyring",
                keyring.to_str().unwrap(),
                "--key-id",
                "documents-key",
            ])
            .output()
            .unwrap(),
        "GHCLI003_SIGNAL_INVALID",
    );
    assert!(!evidence_out.exists());
    assert_eq!(history(&events, "documents-a").len(), before_a + 3);

    let notice_head = history(&events, "documents-a").len();
    let edit_notice = json!({
        "actor":{"type":"owner","id":"owner"},
        "projectId":"a".repeat(64),
        "path":"docs/rules.md",
        "beforeSha256":"b".repeat(64),
        "afterSha256":"c".repeat(64),
        "reason":"Owner clarified recovery",
        "document":{"evidenceId":evidence,"index":0}
    });
    for (index, (kind, id)) in [
        ("owner_document_edit_intent", "owner-edit-untrusted"),
        ("owner_document_edit_saved", "owner-edit-untrusted-saved"),
        (
            "owner_document_changed",
            "owner-change-01789387200000000000-untrusted-run",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        for (suffix, extra) in [
            ("secret", json!({"apiToken":"sk-abcdefghijklmnopqrst"})),
            ("unknown", json!({"unexpected":true})),
        ] {
            let mut description = if kind == "owner_document_edit_intent" {
                json!({"edit":edit_notice,"runs":["documents-a"]})
            } else {
                edit_notice.clone()
            };
            description
                .as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            let signal = write(
                scratch.path(),
                &format!("untrusted-owner-notice-{index}-{suffix}.json"),
                &json!({"id":id,"source":{"type":"user","id":"owner"},"type":kind,
                    "severity":"medium","description":description.to_string(),
                    "evidence":["reported-change"],"emittedAt":"2026-09-14T12:00:00Z"}),
            );
            let evidence_out = scratch
                .path()
                .join(format!("must-not-write-{index}-{suffix}.json"));
            refusal(
                command()
                    .args([
                        "execution",
                        "signal",
                        "--events",
                        events.to_str().unwrap(),
                        "--execution",
                        "documents-a",
                        "--signal",
                        signal.to_str().unwrap(),
                        "--evidence-out",
                        evidence_out.to_str().unwrap(),
                        "--keyring",
                        keyring.to_str().unwrap(),
                        "--key-id",
                        "documents-key",
                    ])
                    .output()
                    .unwrap(),
                "GHCLI003_SIGNAL_INVALID",
            );
            assert!(!evidence_out.exists());
            assert_eq!(history(&events, "documents-a").len(), notice_head);
        }
    }

    for (index, (kind, id, description)) in [
        (
            "owner_document_edit_intent",
            "owner-edit-secret-run",
            json!({"edit":edit_notice,"runs":["sk-abcdefghijklmnopqrst"]}),
        ),
        ("owner_document_edit_saved", "owner-edit-secret-path", {
            let mut edit = edit_notice.clone();
            edit["path"] = json!("docs/sk-abcdefghijklmnopqrst.md");
            edit
        }),
        (
            "owner_document_changed",
            "owner-change-01789387200000000000-secret-evidence",
            {
                let mut edit = edit_notice.clone();
                edit["document"]["evidenceId"] = json!("sk-abcdefghijklmnopqrst");
                edit
            },
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let signal = write(
            scratch.path(),
            &format!("nested-secret-owner-notice-{index}.json"),
            &json!({"id":id,"source":{"type":"user","id":"owner"},"type":kind,
                "severity":"medium","description":description.to_string(),
                "evidence":["reported-change"],"emittedAt":"2026-09-14T12:00:00Z"}),
        );
        let evidence_out = scratch.path().join(format!("nested-secret-{index}.json"));
        refusal(
            command()
                .args([
                    "execution",
                    "signal",
                    "--events",
                    events.to_str().unwrap(),
                    "--execution",
                    "documents-a",
                    "--signal",
                    signal.to_str().unwrap(),
                    "--evidence-out",
                    evidence_out.to_str().unwrap(),
                    "--keyring",
                    keyring.to_str().unwrap(),
                    "--key-id",
                    "documents-key",
                ])
                .output()
                .unwrap(),
            "GHCLI003_SIGNAL_INVALID",
        );
        assert!(!evidence_out.exists());
        assert_eq!(history(&events, "documents-a").len(), notice_head);
    }

    // Idempotency belongs to its originating run/project. Two clients in different runs may
    // legitimately choose the same key without colliding in their cross-run notices.
    let namespace_a = history(&events, "documents-a").len();
    let namespace_b = history(&events, "documents-b").len();
    let second_edit = json!({"document":{"evidenceId":delivery_ref(&events, "documents-b"),"index":0},"content":"Run B clarified the rule.\n","expectedSha256":graphhelm_graph::raw_content_sha256(&std::fs::read(&file).unwrap()).unwrap().as_str(),"reason":"A separate owner change from run B","idempotencyKey":"owner-change-1"});
    write(scratch.path(), "edit.json", &second_edit);
    let second_saved = data(
        command()
            .args([
                "execution",
                "document-save",
                "--events",
                events.to_str().unwrap(),
                "--execution",
                "documents-b",
                "--project-directory",
                project.to_str().unwrap(),
                "--edit",
                edit_path.to_str().unwrap(),
                "--keyring",
                keyring.to_str().unwrap(),
                "--key-id",
                "documents-key",
            ])
            .output()
            .unwrap(),
    );
    assert_ne!(second_saved["changeId"], saved["changeId"]);
    assert_eq!(second_saved["notification"]["status"], "recorded");
    assert_eq!(
        second_saved["notification"]["notifiedRuns"],
        json!(["documents-a", "documents-b"])
    );
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "Run B clarified the rule.\n"
    );
    assert_eq!(history(&events, "documents-a").len(), namespace_a + 1);
    assert_eq!(history(&events, "documents-b").len(), namespace_b + 3);
    for run in ["documents-a", "documents-b"] {
        let notice_ids: std::collections::BTreeSet<_> = history(&events, run)
            .into_iter()
            .filter_map(|event| match event.kind {
                EventKind::SignalRecorded(signal) if signal.kind == "owner_document_changed" => {
                    Some(signal.signal_id.as_str().to_owned())
                }
                _ => None,
            })
            .collect();
        assert_eq!(notice_ids.len(), 2);
    }

    // The JSON representation, not just the unescaped text, must fit the notice contract.
    let bounded_a = history(&events, "documents-a").len();
    let bounded_b = history(&events, "documents-b").len();
    let oversized = json!({"document":{"evidenceId":evidence,"index":0},"content":"This must never reach disk.\n","expectedSha256":second_saved["contentSha256"],"reason":"\u{0001}".repeat(2048),"idempotencyKey":"oversized-encoded-reason"});
    write(scratch.path(), "edit.json", &oversized);
    refusal(save(), "GHCLI001_ARGUMENT_INVALID");
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "Run B clarified the rule.\n"
    );
    assert_eq!(history(&events, "documents-a").len(), bounded_a);
    assert_eq!(history(&events, "documents-b").len(), bounded_b);

    // A completed historical receipt is still retryable after the current file is removed.
    let original_edit = json!({"document":{"evidenceId":evidence,"index":0},"content":"Retry twice with confirmation.\n","expectedSha256":snapshot["contentSha256"],"reason":"Owner clarified recovery","idempotencyKey":"owner-change-1"});
    write(scratch.path(), "edit.json", &original_edit);
    std::fs::remove_file(&file).unwrap();
    assert_eq!(data(save()), saved);
    assert!(
        !file.exists(),
        "a historical retry must not recreate a deleted file"
    );
    assert_eq!(history(&events, "documents-a").len(), bounded_a);
    assert_eq!(history(&events, "documents-b").len(), bounded_b);
}
