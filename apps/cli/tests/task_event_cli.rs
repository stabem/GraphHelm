//! #386 (journey-first spec §7, §9 row F): `task.*` signals carry one `graphhelm-task-event-v1`
//! document and are recorded only by the actor they name. Observed through the real CLI
//! `execution signal` door, which shares its admission with the HTTP and MCP doors. The five
//! accepted documents are the package's own fixtures, so the schema and the admission cannot drift
//! apart unseen. Cost: one held fixture run and a few CLI calls in a tempdir; no network.

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use serde_json::{Value, json};

const RUN: &str = "task-events";
/// The actor the CLI's own `execution signal` records under.
const ACTOR: &str = "owner-cli";

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
    // A verdict is signed by its reviewer; an assignment names someone else and stays as it is.
    if document.get("verdict").is_some() {
        document["reviewer"] = json!(ACTOR);
    }
    document
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
fn each_of_the_five_task_kinds_is_accepted_from_its_recording_actor() {
    let scratch = tempfile::tempdir().unwrap();
    let events = start(scratch.path());
    for (name, kind) in [
        ("claimed", "task.claimed"),
        ("pr-opened", "task.pr_opened"),
        ("review-assigned", "task.review_assigned"),
        ("review-verdict", "task.review_verdict"),
        ("merged", "task.merged"),
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
            ("ok", json!("Studio: Team tab shows task titles"), json!("The owner reads each task's title."), true),
            ("unicode", json!("Studio: título da tarefa"), json!("O dono lê o título."), true),
            // Limits count characters, as the schema and the script do, not UTF-8 bytes (#486
            // review): 200 accented characters are 220+ bytes and must still be accepted.
            ("accented-200", json!("Correção ".repeat(22).chars().take(200).collect::<String>()), json!("ção".repeat(100)), true),
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
