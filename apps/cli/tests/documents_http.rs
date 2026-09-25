//! The Studio document routes must honor `serve --project` on a fixture-only Runtime (#1099).
//!
//! The project root used to be read out of the TOOL half of the executor wiring, so a Runtime
//! started with the documented `--project` plus a keyring but without `--staging`/`--allow-program`
//! answered every document read with "requires an explicit --project". The document root is now
//! carried in server state on its own; this test pins that with the smallest real server.
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

const EVENTS_KEY: &str = "0101010101010101010101010101010101010101010101010101010101010101";

fn cli(args: &[&str]) -> Value {
    let output = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .env("GRAPHHELM_EVENTS_KEY", EVENTS_KEY)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    serde_json::from_slice::<Value>(&output.stdout).unwrap()["data"].clone()
}

fn write(directory: &Path, name: &str, value: &Value) -> PathBuf {
    let path = directory.join(name);
    std::fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
    path
}

fn delivery_evidence(events: &Path, run: &str) -> String {
    let store = graphhelm_events::LocalEventRepository::open(
        events,
        std::sync::Arc::new(FixedClock),
        std::sync::Arc::new(ReadOnlyIds),
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
        .into_iter()
        .find(|event| {
            matches!(&event.kind, graphhelm_protocols::EventKind::SignalRecorded(signal) if signal.kind == "node_delivery")
        })
        .unwrap()
        .evidence_refs[0]
        .evidence_id()
        .as_str()
        .to_owned()
}

struct FixedClock;
impl graphhelm_protocols::Clock for FixedClock {
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        use chrono::TimeZone;
        chrono::Utc.with_ymd_and_hms(2026, 9, 15, 12, 0, 0).unwrap()
    }
}
struct ReadOnlyIds;
impl graphhelm_protocols::IdGenerator for ReadOnlyIds {
    fn next_id(&self, _: &'static str) -> String {
        panic!("this reader must not append")
    }
}

struct Server {
    child: std::process::Child,
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn serve(events: &Path, extra: &[&str]) -> (Server, String, String) {
    let mut child = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .env("GRAPHHELM_EVENTS_KEY", EVENTS_KEY)
        .args([
            "serve",
            "--events",
            events.to_str().unwrap(),
            "--bind",
            "127.0.0.1:0",
        ])
        .args(extra)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut startup = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut startup)
        .unwrap();
    let mut server = Server { child };
    let started: Value = serde_json::from_str(&startup).unwrap_or_else(|_| {
        let mut stderr = String::new();
        let _ = server
            .child
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut stderr);
        panic!("serve printed no startup envelope: {startup:?}\n{stderr}");
    });
    assert_eq!(started["ok"], true, "{started}");
    let base = format!("http://{}", started["data"]["address"].as_str().unwrap());
    let mut token_name = events.file_name().unwrap().to_os_string();
    token_name.push(".token");
    let token_path = events.with_file_name(token_name);
    let deadline = Instant::now() + Duration::from_secs(10);
    let token = loop {
        if let Ok(token) = std::fs::read_to_string(&token_path)
            && !token.is_empty()
        {
            break token;
        }
        assert!(Instant::now() < deadline, "no token at {token_path:?}");
        std::thread::sleep(Duration::from_millis(20));
    };
    (server, base, token)
}

fn post_json(base: &str, token: &str, path: &str, body: &Value) -> (u16, Value) {
    let authority = base.strip_prefix("http://").unwrap();
    let mut stream = TcpStream::connect(authority).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    let payload = serde_json::to_vec(body).unwrap();
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        payload.len()
    );
    stream.write_all(request.as_bytes()).unwrap();
    stream.write_all(&payload).unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    let text = String::from_utf8_lossy(&raw);
    let (head, body) = text.split_once("\r\n\r\n").unwrap();
    let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    (status, serde_json::from_str(body).unwrap())
}

#[test]
fn fixture_only_runtime_reads_registered_documents_from_the_configured_project() {
    let scratch = tempfile::tempdir().unwrap();
    let project = scratch.path().join("project");
    let events = project.join("runtime-data");
    std::fs::create_dir_all(project.join("docs")).unwrap();
    std::fs::write(project.join("docs/rules.md"), "Retry once.\n").unwrap();
    let keyring = project.join("vaultdata");
    std::fs::create_dir(&keyring).unwrap();
    graphhelm_sealed_key_provider::SealedKeyProvider::create(
        &keyring,
        "documents-key",
        graphhelm_events::SecretBytes::new(vec![1; 32]),
    )
    .unwrap();
    let fixtures = write(scratch.path(), "fixtures.json", &json!({"nodeOutcomes":{}}));
    let graph = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/graphs/manual-override-deploy.yaml");
    let delivery = write(
        scratch.path(),
        "delivery.json",
        &json!({"version":1,"summary":"Added retry rule","reason":"Map checkout recovery","documents":[{"path":"docs/rules.md","title":"Retry policy","kind":"business_rule","action":"created","journeyIds":["checkout"],"ruleIds":["retry"]}]}),
    );
    let events_arg = events.to_str().unwrap();
    cli(&[
        "execution",
        "start",
        "--events",
        events_arg,
        "--execution",
        "documents-a",
        "--file",
        graph.to_str().unwrap(),
        "--fixtures",
        fixtures.to_str().unwrap(),
        "--mode",
        "manual",
        "--held",
    ]);
    cli(&[
        "execution",
        "delivery",
        "--events",
        events_arg,
        "--execution",
        "documents-a",
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
    ]);
    let evidence = delivery_evidence(&events, "documents-a");

    // Fixture-only: `--project` and the keyring, no `--staging`/`--allow-program`, no model half.
    let (_server, base, token) = serve(
        &events,
        &[
            "--project",
            project.to_str().unwrap(),
            "--keyring",
            keyring.to_str().unwrap(),
            "--key-id",
            "documents-key",
        ],
    );
    let (status, reply) = post_json(
        &base,
        &token,
        "/v1/executions/documents-a/documents/read",
        &json!({"evidenceId": evidence, "index": 0}),
    );
    assert_eq!(status, 200, "{reply}");
    assert_eq!(reply["ok"], true, "{reply}");
    assert_eq!(reply["data"]["content"], "Retry once.\n", "{reply}");
    assert_eq!(reply["data"]["target"], "main_project", "{reply}");
}
