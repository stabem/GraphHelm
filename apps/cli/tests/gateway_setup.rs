//! `graphhelm gateway setup` journey suite (#1139): after `init`, one command with the key on
//! stdin leaves a manifest route, a sealed credential, a green probe and the next commands — and
//! the key nowhere else. Spawned as the compiled binary like every other CLI suite.
//!
//! The fake provider server exists to be NAMED by `--base-url` (an `http://` loopback base is the
//! one non-TLS form the manifest admits) and to prove, by staying silent, that the probe places no
//! network call: `gateway probe` is quota-free by design (§18) — it proves the credential leases
//! from the broker and never dials the provider. Its `/v1/systemone` (typesafe) and
//! `/v1/messages` (anthropic) paths are what the real adapters would post to.

use std::io::Write;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use assert_cmd::Command;
use serde_json::Value;

/// A distinctive key planted on stdin. Must never appear in any output or in any file except
/// the broker's sealed store — and there only sealed, so the raw bytes match nowhere at all.
const SENTINEL: &str = "sk-SENTINEL-1139-0123456789abcdef";
const SECOND_SENTINEL: &str = "sk-SENTINEL-1139-SECOND-fedcba9876543210";
const GATEWAY_INVALID: &str = "GHCLI009_GATEWAY_INVALID";

fn command() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
}

fn json(output: &[u8]) -> Value {
    serde_json::from_slice(output).unwrap_or_else(|error| {
        panic!(
            "stdout must be valid JSON ({error}): {:?}",
            String::from_utf8_lossy(output)
        )
    })
}

fn combined_output(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// A project directory that git would call a work tree: `.git` exists.
fn git_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".git")).unwrap();
    dir
}

/// `init` with both harnesses named, so the machine's own `~/.claude`/`~/.codex` never decide.
fn init(project: &Path) -> Value {
    let output = command()
        .args(["init", "--project"])
        .arg(project)
        .args(["--harness", "claude-code", "--harness", "codex"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", combined_output(&output));
    json(&output.stdout)
}

/// A loopback listener that answers nothing. Its address is the route's `baseUrl`; after the
/// command, [`FakeProvider::connections`] says whether anything dialed it.
struct FakeProvider {
    listener: TcpListener,
    base_url: String,
}

fn fake_provider() -> FakeProvider {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let base_url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    FakeProvider { listener, base_url }
}

impl FakeProvider {
    fn connections(&self) -> usize {
        let mut count = 0;
        while self.listener.accept().is_ok() {
            count += 1;
        }
        count
    }
}

fn setup(project: &Path, provider: &str, extra: &[&str], stdin: &str) -> std::process::Output {
    command()
        .args(["gateway", "setup", "--provider", provider, "--project"])
        .arg(project)
        .args(extra)
        .write_stdin(stdin)
        .output()
        .unwrap()
}

fn manifest_path(project: &Path) -> PathBuf {
    project.join(".graphhelm").join("manifest.json")
}

fn manifest_bytes(project: &Path) -> Vec<u8> {
    std::fs::read(manifest_path(project)).unwrap()
}

fn routes(project: &Path) -> Vec<Value> {
    let value: Value = serde_json::from_slice(&manifest_bytes(project)).unwrap();
    value["routes"].as_array().unwrap().clone()
}

#[test]
fn setup_and_route_set_serialize_the_same_manifest_transaction() {
    let project = git_project();
    init(project.path());
    let provider = fake_provider();
    let binary = assert_cmd::cargo::cargo_bin!("graphhelm");
    let mut setup = std::process::Command::new(binary)
        .args(["gateway", "setup", "--provider", "typesafe", "--project"])
        .arg(project.path())
        .args(["--base-url", &provider.base_url])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let keyring = project.path().join(".graphhelm").join("keyring");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !keyring.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(keyring.exists(), "setup never reached its stdin wait");

    let manifest = manifest_path(project.path());
    let route = std::process::Command::new(binary)
        .args(["gateway", "route", "set", "--manifest"])
        .arg(&manifest)
        .args([
            "--id",
            "deepseek_official",
            "--provider",
            "openai",
            "--base-url",
            "https://api.deepseek.com",
            "--model",
            "deepseek-v4-pro",
        ])
        .output()
        .unwrap();
    assert!(route.status.success(), "{}", combined_output(&route));

    setup
        .stdin
        .take()
        .unwrap()
        .write_all(format!("{SENTINEL}\n").as_bytes())
        .unwrap();
    let setup = setup.wait_with_output().unwrap();
    assert!(setup.status.success(), "{}", combined_output(&setup));

    let ids = routes(project.path())
        .into_iter()
        .map(|route| route["id"].as_str().unwrap().to_owned())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        ids,
        ["deepseek_official".to_owned(), "judge".to_owned()].into()
    );
}

/// Every file under `root`, recursively, with its raw bytes.
fn files_under(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let bytes = std::fs::read(&path).unwrap();
                found.push((path, bytes));
            }
        }
    }
    found
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle.as_bytes())
}

/// The sentinel must be in NO file of the project: the broker's store is sealed, so even there
/// the raw bytes do not match. Asserted over every file, not just the ones the command names.
fn assert_sentinel_nowhere_on_disk(project: &Path, sentinel: &str) {
    let files = files_under(project);
    assert!(
        files
            .iter()
            .any(|(path, _)| path.starts_with(project.join(".graphhelm").join("broker"))),
        "the broker directory must hold the sealed store"
    );
    for (path, bytes) in &files {
        assert!(
            !contains(bytes, sentinel),
            "the key must not be readable in {}",
            path.display()
        );
    }
}

// ---------------------------------------------------------------------------
// Test 1: after `init`, the typesafe route, the credential, the probe and the next commands.
// ---------------------------------------------------------------------------

#[test]
fn a_piped_key_after_init_wires_the_typesafe_route_and_the_key_appears_nowhere() {
    let project = git_project();
    init(project.path());
    let provider = fake_provider();

    let output = setup(
        project.path(),
        "typesafe",
        &["--base-url", &provider.base_url],
        &format!("{SENTINEL}\n"),
    );
    assert!(output.status.success(), "{}", combined_output(&output));
    let value = json(&output.stdout);
    assert_eq!(value["ok"], true);
    assert_eq!(value["command"], "gateway.setup");
    let data = &value["data"];
    assert_eq!(data["manifest"]["path"], ".graphhelm/manifest.json");
    assert_eq!(data["manifest"]["state"], "created");
    assert_eq!(data["route"]["id"], "judge");
    assert_eq!(data["route"]["model"], "jev-latest");
    assert_eq!(data["route"]["baseUrl"], provider.base_url);
    assert_eq!(data["provider"], "typesafe");
    assert_eq!(data["credentialRef"], "secret_typesafe");
    assert_eq!(data["credential"]["usableBy"], serde_json::json!(["judge"]));
    // `init` already made both; setup reuses them and never rotates.
    assert_eq!(data["key"]["state"], "existing", "{data}");
    assert_eq!(data["keyring"]["state"], "existing", "{data}");
    assert_eq!(data["keyring"]["keyId"], "studio");
    assert_eq!(data["gitignore"]["state"], "existing", "{data}");

    // The probe's own reply, verbatim: the credential leases for this route.
    assert_eq!(data["probe"]["route"], "judge");
    assert_eq!(data["probe"]["health"], "available", "{data}");
    assert_eq!(
        data["probe"]["checks"],
        serde_json::json!([{"name": "credential", "ok": true}])
    );
    assert_eq!(
        provider.connections(),
        0,
        "the probe is quota-free: nothing may dial the provider"
    );

    // The manifest: the documented route, LF, no BOM, one trailing newline.
    let bytes = manifest_bytes(project.path());
    assert!(!bytes.starts_with(&[0xEF, 0xBB, 0xBF]), "no BOM");
    assert!(!bytes.contains(&b'\r'), "LF only");
    assert!(bytes.ends_with(b"}\n"), "one trailing newline");
    let manifest: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(manifest["manifestVersion"], 1);
    let routes = routes(project.path());
    assert_eq!(routes.len(), 1, "{manifest}");
    assert_eq!(
        routes[0],
        serde_json::json!({
            "id": "judge",
            "provider": "typesafe",
            "transport": "direct_api",
            "authentication": "api_key",
            "billingMode": "per_token",
            "baseUrl": provider.base_url,
            "model": "jev-latest",
            "credentialRef": "secret_typesafe",
            "profiles": ["balanced_reasoning"],
            "enabled": true
        })
    );

    // The key: in no output, in no file.
    let combined = combined_output(&output);
    assert!(!combined.contains(SENTINEL), "{combined}");
    assert_sentinel_nowhere_on_disk(project.path(), SENTINEL);
    let gitignore = std::fs::read_to_string(project.path().join(".gitignore")).unwrap();
    assert!(gitignore.contains(".graphhelm/"), "{gitignore}");
    assert!(!gitignore.contains(SENTINEL));

    // The next commands name the route on the judge door and carry no key.
    let next = data["next"].as_array().unwrap();
    assert_eq!(next.len(), 3, "{data}");
    let synthesize = next[2]["bash"].as_str().unwrap();
    assert!(synthesize.contains("--judge-route judge"), "{synthesize}");
    assert!(synthesize.contains("--key-id studio"), "{synthesize}");
    assert!(next[0]["bash"].as_str().unwrap().contains("serve.key"));
    assert!(!data.to_string().contains(SENTINEL));

    // What setup stored is what the standalone `probe` leases, under serve.key as the passphrase:
    // the same broker, the same keyring, the same key id.
    let key = std::fs::read_to_string(project.path().join(".graphhelm").join("serve.key")).unwrap();
    let probe = command()
        .args(["gateway", "probe", "--manifest"])
        .arg(manifest_path(project.path()))
        .args(["--route", "judge", "--broker"])
        .arg(project.path().join(".graphhelm").join("broker"))
        .arg("--keyring")
        .arg(project.path().join(".graphhelm").join("keyring"))
        .args(["--key-id", "studio"])
        .env("GRAPHHELM_GATEWAY_KEY", key.trim())
        .output()
        .unwrap();
    assert!(probe.status.success(), "{}", combined_output(&probe));
    assert_eq!(json(&probe.stdout)["data"]["health"], "available");
}

// ---------------------------------------------------------------------------
// Test 2: a second run without `--replace` refuses before reading the key and changes nothing;
// `--replace` swaps the route.
// ---------------------------------------------------------------------------

#[test]
fn a_second_run_refuses_without_replace_and_swaps_the_route_with_it() {
    let project = git_project();
    init(project.path());
    let provider = fake_provider();
    let first = setup(
        project.path(),
        "typesafe",
        &["--base-url", &provider.base_url],
        &format!("{SENTINEL}\n"),
    );
    assert!(first.status.success(), "{}", combined_output(&first));
    let before = manifest_bytes(project.path());
    let broker_before = files_under(&project.path().join(".graphhelm").join("broker"));

    let refused = setup(
        project.path(),
        "typesafe",
        &["--base-url", &provider.base_url],
        &format!("{SECOND_SENTINEL}\n"),
    );
    assert!(!refused.status.success());
    let value = json(&refused.stdout);
    assert_eq!(value["ok"], false);
    assert_eq!(value["diagnostics"][0]["code"], GATEWAY_INVALID, "{value}");
    assert_eq!(value["diagnostics"][0]["path"], "/route", "{value}");
    assert!(
        value["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("--replace"),
        "{value}"
    );
    assert_eq!(
        manifest_bytes(project.path()),
        before,
        "the manifest must be byte-identical after a refusal"
    );
    assert_eq!(
        files_under(&project.path().join(".graphhelm").join("broker")),
        broker_before,
        "the refusal happens before the key is read: the store is untouched"
    );
    assert!(!combined_output(&refused).contains(SECOND_SENTINEL));

    let replaced = setup(
        project.path(),
        "typesafe",
        &[
            "--base-url",
            &provider.base_url,
            "--model",
            "jev-2",
            "--replace",
        ],
        &format!("{SECOND_SENTINEL}\n"),
    );
    assert!(replaced.status.success(), "{}", combined_output(&replaced));
    let value = json(&replaced.stdout);
    assert_eq!(value["data"]["manifest"]["state"], "replaced", "{value}");
    assert_eq!(value["data"]["probe"]["health"], "available", "{value}");
    let routes = routes(project.path());
    assert_eq!(routes.len(), 1, "{routes:?}");
    assert_eq!(routes[0]["id"], "judge");
    assert_eq!(routes[0]["model"], "jev-2");
    assert_sentinel_nowhere_on_disk(project.path(), SENTINEL);
    assert_sentinel_nowhere_on_disk(project.path(), SECOND_SENTINEL);
    assert_eq!(provider.connections(), 0);
}

// ---------------------------------------------------------------------------
// Test 3: an empty key is refused, and nothing is written.
// ---------------------------------------------------------------------------

#[test]
fn an_empty_key_is_refused_and_no_manifest_is_written() {
    let project = git_project();
    init(project.path());
    let blank = format!("{}\r\n", " ".repeat(3));
    for stdin in ["", "\n", blank.as_str()] {
        let output = setup(project.path(), "typesafe", &[], stdin);
        assert!(!output.status.success(), "{stdin:?}");
        let value = json(&output.stdout);
        assert_eq!(value["ok"], false);
        assert_eq!(value["diagnostics"][0]["code"], GATEWAY_INVALID, "{value}");
        assert_eq!(value["diagnostics"][0]["path"], "/stdin", "{value}");
        assert!(
            !manifest_path(project.path()).exists(),
            "a refused key must not leave a route behind"
        );
        assert!(
            !project.path().join(".graphhelm").join("broker").exists(),
            "a refused key must not create the store"
        );
    }
}

// ---------------------------------------------------------------------------
// Test 4: the anthropic provider — `--model` required, then a route on the chat door.
// ---------------------------------------------------------------------------

#[test]
fn the_anthropic_provider_requires_a_model_and_wires_a_chat_route() {
    let project = git_project();
    init(project.path());
    let provider = fake_provider();

    let without_model = setup(
        project.path(),
        "anthropic",
        &["--base-url", &provider.base_url],
        &format!("{SENTINEL}\n"),
    );
    assert!(!without_model.status.success());
    let value = json(&without_model.stdout);
    assert_eq!(value["diagnostics"][0]["code"], GATEWAY_INVALID, "{value}");
    assert_eq!(value["diagnostics"][0]["path"], "/model", "{value}");
    assert!(!manifest_path(project.path()).exists());

    let output = setup(
        project.path(),
        "anthropic",
        &[
            "--base-url",
            &provider.base_url,
            "--model",
            "claude-sonnet-5",
        ],
        &format!("{SENTINEL}\n"),
    );
    assert!(output.status.success(), "{}", combined_output(&output));
    let value = json(&output.stdout);
    let data = &value["data"];
    assert_eq!(data["route"]["id"], "anthropic");
    assert_eq!(data["route"]["provider"], "anthropic");
    assert_eq!(data["route"]["model"], "claude-sonnet-5");
    assert_eq!(data["credentialRef"], "secret_anthropic");
    assert_eq!(data["probe"]["health"], "available", "{data}");
    let synthesize = data["next"][2]["bash"].as_str().unwrap();
    assert!(synthesize.contains("--route anthropic"), "{synthesize}");
    assert!(!synthesize.contains("--judge-route"), "{synthesize}");
    let routes = routes(project.path());
    assert_eq!(routes[0]["provider"], "anthropic");
    assert_eq!(routes[0]["credentialRef"], "secret_anthropic");
    assert!(!combined_output(&output).contains(SENTINEL));
    assert_sentinel_nowhere_on_disk(project.path(), SENTINEL);
    assert_eq!(provider.connections(), 0);
}

// ---------------------------------------------------------------------------
// Test 5: without a prior `init`, setup provisions the key and keyring the way `init` does —
// and `init` afterwards finds them `existing`. Same files, same functions.
// ---------------------------------------------------------------------------

#[test]
fn setup_before_init_makes_the_same_key_and_keyring_init_then_finds_existing() {
    let project = git_project();
    let output = setup(project.path(), "typesafe", &[], &format!("{SENTINEL}\n"));
    assert!(output.status.success(), "{}", combined_output(&output));
    let data = json(&output.stdout)["data"].clone();
    assert_eq!(data["key"]["state"], "created", "{data}");
    assert_eq!(data["keyring"]["state"], "created", "{data}");
    assert_eq!(data["gitignore"]["state"], "created", "{data}");
    assert_eq!(data["route"]["baseUrl"], "https://api.typesafe.ai");
    assert_eq!(data["probe"]["health"], "available", "{data}");
    let key_before = std::fs::read(project.path().join(".graphhelm").join("serve.key")).unwrap();

    let init_data = init(project.path())["data"].clone();
    assert_eq!(init_data["key"]["state"], "existing", "{init_data}");
    assert_eq!(init_data["keyring"]["state"], "existing", "{init_data}");
    assert_eq!(init_data["gitignore"]["state"], "existing", "{init_data}");
    assert_eq!(
        std::fs::read(project.path().join(".graphhelm").join("serve.key")).unwrap(),
        key_before,
        "init never rotates the key setup made"
    );
    assert_sentinel_nowhere_on_disk(project.path(), SENTINEL);
}

// ---------------------------------------------------------------------------
// Test 6: an existing manifest with another route is merged, not clobbered.
// ---------------------------------------------------------------------------

#[test]
fn an_existing_manifest_keeps_its_other_routes() {
    let project = git_project();
    init(project.path());
    let existing = serde_json::json!({
        "manifestVersion": 1,
        "routes": [{
            "id": "chat",
            "provider": "anthropic",
            "transport": "direct_api",
            "authentication": "api_key",
            "billingMode": "per_token",
            "baseUrl": "https://api.anthropic.com",
            "model": "claude-sonnet-5",
            "credentialRef": "secret_anthropic",
            "profiles": ["critical_reasoning"],
            "enabled": true
        }]
    });
    std::fs::write(
        manifest_path(project.path()),
        serde_json::to_vec_pretty(&existing).unwrap(),
    )
    .unwrap();

    let output = setup(project.path(), "typesafe", &[], &format!("{SENTINEL}\n"));
    assert!(output.status.success(), "{}", combined_output(&output));
    assert_eq!(json(&output.stdout)["data"]["manifest"]["state"], "merged");
    let routes = routes(project.path());
    assert_eq!(routes.len(), 2, "{routes:?}");
    assert_eq!(routes[0], existing["routes"][0]);
    assert_eq!(routes[1]["id"], "judge");
    assert!(
        !project
            .path()
            .join(".graphhelm")
            .read_dir()
            .unwrap()
            .any(|entry| entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp"))
    );
}

/// #1141 review (MEDIUM): `--key-id` is interpolated UNQUOTED into the copy-paste commands this
/// command prints, so an id carrying a shell metacharacter would hand the operator a command that
/// runs something else — the #1070 class, on a new surface. It is refused before any file is
/// touched: no manifest, no credential, and the key never leaves stdin.
#[test]
fn a_key_id_carrying_a_shell_metacharacter_is_refused_before_anything_is_written() {
    let project = git_project();
    init(project.path());
    let provider = fake_provider();

    let output = setup(
        project.path(),
        "typesafe",
        &["--base-url", &provider.base_url, "--key-id", "x;id"],
        &format!(
            "{SENTINEL}
"
        ),
    );
    assert!(!output.status.success(), "{}", combined_output(&output));
    let combined = combined_output(&output);
    assert!(
        combined.contains("--key-id"),
        "the refusal names the flag: {combined}"
    );
    assert!(
        !manifest_path(project.path()).exists(),
        "no manifest is written"
    );
    assert!(
        !project.path().join(".graphhelm/broker").exists(),
        "no credential is stored"
    );
    // Not `assert_sentinel_nowhere_on_disk`: that helper also asserts the sealed store EXISTS,
    // which is the happy path's claim. Here nothing was provisioned, so the claim is only that
    // the key reached no file at all.
    for (path, bytes) in files_under(project.path()) {
        assert!(
            !contains(&bytes, SENTINEL),
            "the key reached {}",
            path.display()
        );
    }
}
