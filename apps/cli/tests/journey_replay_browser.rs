//! #347: opt-in actual replay-to-reader observer. Protects multi-act/path execution,
//! deterministic cache, sealed images, pinned arrows and truthful freshness. Existing
//! producers never drive the browser. Independent static app, server tripwire, decoded
//! PNG pixels and fresh CLI/Runtime readers; no public test-only production seam.
//! Cost: ~30s plus build, Node/Playwright/Chromium explicitly installed, local ports/Git;
//! no provider/account or network installation. Ordinary offline runs ignore this target.
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use serde_json::{Value, json};

const KEY: &str = "0101010101010101010101010101010101010101010101010101010101010101";
const RUN: &str = "replay-browser";
const SECRET: &str = "fixture_secret_831597";
const FLOW: &str = include_str!("fixtures/journey_flow/checkout.journey.yaml");
const FIXTURE: &str = include_str!("../../../tools/journey-driver/fixture-server.mjs");
const DRIVER: &[u8] = include_bytes!("../../../tools/journey-driver/driver.mjs");

struct Server {
    child: Child,
    group: graphhelm_process_tree::ProcessGroup,
    input: ChildStdin,
    replies: Receiver<Value>,
    started: Value,
}

impl Server {
    fn start(mut command: Command) -> Self {
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        graphhelm_process_tree::configure(&mut command);
        let mut child = command
            .spawn()
            .expect("OBSERVER_MISSING: fixture/server executable");
        let group = match graphhelm_process_tree::create(&child) {
            Ok(group) => group,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("server containment: {error:?}")
            }
        };
        let input = child.stdin.take().unwrap();
        let output = child.stdout.take().unwrap();
        let (tx, replies) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                let Ok(line) = line else {
                    break;
                };
                if line.len() > 65536 {
                    break;
                }
                let Ok(value) = serde_json::from_str(&line) else {
                    break;
                };
                if tx.send(value).is_err() {
                    break;
                }
            }
        });
        let started = replies
            .recv_timeout(Duration::from_secs(30))
            .expect("OBSERVER_MISSING: fixture/server startup");
        Self {
            child,
            group,
            input,
            replies,
            started,
        }
    }
    fn control(&mut self, action: &str) -> Value {
        writeln!(self.input, "{action}").unwrap();
        self.input.flush().unwrap();
        self.replies.recv_timeout(Duration::from_secs(5)).unwrap()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = graphhelm_process_tree::terminate(self.child.id(), self.group);
        graphhelm_process_tree::close(&mut self.group);
        let _ = self.child.wait();
    }
}

fn cli() -> Command {
    let mut command = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"));
    command.arg("--json").env("GRAPHHELM_EVENTS_KEY", KEY);
    command
}

fn reply(command: &mut Command) -> (i32, Value) {
    let out = command.output().unwrap();
    let value = serde_json::from_slice(&out.stdout).expect("CLI must return its JSON envelope");
    (out.status.code().unwrap(), value)
}

fn git(project: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(project)
        .args([
            "-c",
            "user.name=Replay observer",
            "-c",
            "user.email=replay@example.invalid",
            "-c",
            "core.autocrlf=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "Git fixture operation failed");
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

fn http(address: &str, path: &str, token: Option<&str>) -> (String, Vec<u8>) {
    let mut stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n"
    )
    .unwrap();
    if let Some(token) = token {
        write!(stream, "Authorization: Bearer {}\r\n", token.trim()).unwrap();
    }
    write!(stream, "\r\n").unwrap();
    stream.flush().unwrap();
    let mut bytes = Vec::new();
    stream
        .take(16 * 1024 * 1024)
        .read_to_end(&mut bytes)
        .unwrap();
    let split = bytes
        .windows(4)
        .position(|slice| slice == b"\r\n\r\n")
        .unwrap();
    let headers = String::from_utf8(bytes[..split].to_vec()).unwrap();
    assert!(headers.starts_with("HTTP/1.1 200") || headers.starts_with("HTTP/1.1 503"));
    (headers, bytes[split + 4..].to_vec())
}

fn replay(project: &Path, events: &Path, keys: &Path, record: bool) -> (i32, Value) {
    let mut command = cli();
    command
        .args(["journey", "replay", "checkout", "--project"])
        .arg(project)
        .env("GRAPHHELM_SECRET_shopper_password", SECRET)
        // Unusable credentials and a live tripwire do not become a route or lease.
        .env("OPENAI_API_KEY", "unusable-fixture-key")
        .env("NODE_OPTIONS", "");
    if record {
        command
            .arg("--events")
            .arg(events)
            .args(["--execution", RUN, "--keyring"])
            .arg(keys)
            .args(["--key-id", "owner-key"]);
    }
    reply(&mut command)
}

fn read_journeys(project: &Path, events: &Path, keys: &Path) -> Value {
    let (code, value) = reply(
        cli()
            .args(["journeys", "--events"])
            .arg(events)
            .args(["--execution", RUN, "--project"])
            .arg(project)
            .arg("--keyring")
            .arg(keys)
            .args(["--key-id", "owner-key"]),
    );
    assert_eq!(code, 0, "{value}");
    value["data"].clone()
}

#[test]
#[ignore = "requires an explicitly observer-enabled validation project"]
fn two_credential_free_paths_replay_to_canonical_cache_sealed_captures_and_fresh_arrows() {
    let toolchain = PathBuf::from(
        std::env::var_os("GRAPHHELM_JOURNEY_TOOLCHAIN_PROJECT")
            .expect("OBSERVER_MISSING: GRAPHHELM_JOURNEY_TOOLCHAIN_PROJECT"),
    );
    assert!(
        toolchain
            .join("node_modules/@playwright/test/package.json")
            .is_file(),
        "OBSERVER_MISSING: project Playwright package"
    );
    let scratch = tempfile::tempdir().unwrap();
    let project = scratch.path().join("project");
    std::fs::create_dir(&project).unwrap();
    std::fs::write(
        project.join("package.json"),
        "{\"name\":\"replay-fixture\",\"private\":true,\"type\":\"module\"}",
    )
    .unwrap();
    std::fs::write(project.join("fixture-server.mjs"), FIXTURE).unwrap();
    // Only this test-owned project links the explicitly declared installed toolchain.
    let linked=Command::new("node").args(["-e","require('node:fs').symlinkSync(process.argv[1],process.argv[2],process.platform==='win32'?'junction':'dir')"])
        .arg(toolchain.join("node_modules")).arg(project.join("node_modules")).status().unwrap();
    assert!(linked.success(), "OBSERVER_MISSING: toolchain link");
    let mut command = Command::new("node");
    command.arg(project.join("fixture-server.mjs"));
    let mut app = Server::start(command);
    let base = app.started["base"].as_str().unwrap().to_owned();
    let model = app.started["modelOrigin"]
        .as_str()
        .unwrap()
        .strip_prefix("http://")
        .unwrap()
        .to_owned();
    let (headers, _) = http(&model, "/positive-control", None);
    assert!(headers.contains("503"));
    assert_eq!(app.control("counts")["model"], 1);
    app.control("reset");

    let source = project.join(".graphhelm/journeys");
    std::fs::create_dir_all(&source).unwrap();
    let mut flow: Value = serde_yaml_ng::from_str(FLOW).unwrap();
    flow["base"] = base.clone().into();
    for screen in flow["screens"].as_array_mut().unwrap() {
        screen["scope"] = json!(["fixture-server.mjs"]);
    }
    flow["screens"][1]["expect"] = json!([{ "role":"heading","name":"Order 42" }]);
    flow["screens"].as_array_mut().unwrap().push(json!({"id":"guest","url":"/guest","state":"stable","expect":[{"role":"heading","name":"Guest checkout"}],"scope":["fixture-server.mjs"]}));
    flow["edges"][1]["acts"][1]["name"] = "Submit order".into();
    flow["edges"].as_array_mut().unwrap().extend([
        json!({"id":"cart.guest","from":"cart","to":"guest","acts":[{"kind":"navigate","role":"link","name":"Guest checkout"}]}),
        json!({"id":"guest.submit","from":"guest","to":"done","acts":[{"kind":"submit","role":"button","name":"Place guest order"}]}),
    ]);
    flow["paths"]["guest"] = json!(["cart.guest", "guest.submit"]);
    std::fs::write(
        source.join("checkout.journey.yaml"),
        serde_yaml_ng::to_string(&flow).unwrap(),
    )
    .unwrap();
    std::fs::write(
        project.join(".gitignore"),
        "node_modules/\n.graphhelm/observers/\n.graphhelm/journey-cache/\n",
    )
    .unwrap();
    let observer = project.join(".graphhelm/observers");
    std::fs::create_dir(&observer).unwrap();
    std::fs::write(observer.join("journey_driver.mjs"), DRIVER).unwrap();
    let (code, value) = reply(
        cli()
            .args([
                "journey",
                "compile",
                "checkout",
                "--fmt",
                "--include-draft",
                "--project",
            ])
            .arg(&project),
    );
    assert_eq!(code, 0, "{value}");
    git(&project, &["init", "--quiet", "--object-format=sha1"]);
    git(&project, &["add", "."]);
    git(
        &project,
        &["commit", "--quiet", "--no-verify", "-m", "fixture"],
    );
    let (code, value) = reply(
        cli()
            .args(["journey", "approve", "checkout", "--project"])
            .arg(&project),
    );
    assert_eq!(code, 0, "{value}");
    git(&project, &["add", "."]);
    git(
        &project,
        &[
            "commit",
            "--quiet",
            "--no-verify",
            "-m",
            "approved flow and projections",
        ],
    );
    assert_eq!(git(&project, &["status", "--porcelain"]), "");
    let events = scratch.path().join("events");
    let keys = scratch.path().join("keys");
    std::fs::create_dir(&keys).unwrap();
    graphhelm_sealed_key_provider::SealedKeyProvider::create(
        &keys,
        "owner-key",
        graphhelm_events::SecretBytes::new(vec![1; 32]),
    )
    .unwrap();
    let graph =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/graphs/software-feature.yaml");
    let fixtures = scratch.path().join("fixtures.json");
    std::fs::write(&fixtures, br#"{"nodeOutcomes":{}}"#).unwrap();
    let (code, value) = reply(
        cli()
            .args(["execution", "start", "--file"])
            .arg(&graph)
            .arg("--events")
            .arg(&events)
            .args(["--execution", RUN, "--fixtures"])
            .arg(&fixtures)
            .args(["--mode", "manual", "--held"]),
    );
    assert_eq!(code, 0, "{value}");
    let mut serve = cli();
    serve
        .args(["serve", "--events"])
        .arg(&events)
        .args(["--bind", "127.0.0.1:0", "--keyring"])
        .arg(&keys)
        .args(["--key-id", "owner-key"]);
    let runtime = Server::start(serve);
    let address = runtime.started["data"]["address"].as_str().unwrap();
    let token = std::fs::read_to_string(scratch.path().join("events.token")).unwrap();

    let cache = project.join(".graphhelm/journey-cache/checkout.json");
    let mut first_bytes = None;
    for pass in 0..2 {
        let (code, result) = replay(&project, &events, &keys, true);
        assert_eq!(code, 0, "{result}");
        assert_eq!(result["data"]["modelCalls"], 0);
        assert_eq!(result["data"]["cacheReused"], pass == 1);
        assert_eq!(
            result["data"]["paths"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| p["name"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["main", "guest"]
        );
        for path in result["data"]["paths"].as_array().unwrap() {
            assert_eq!(path["outcome"], "passed");
            assert_eq!(path["observedScreens"].as_array().unwrap().len(), 3);
            assert_eq!(path["capturedSignalIds"].as_array().unwrap().len(), 3);
            assert_eq!(path["walkedPairs"].as_array().unwrap().len(), 2);
            assert_eq!(path["unresolvedSteps"], json!([]));
        }
        let bytes = std::fs::read(&cache).unwrap();
        assert!(
            !bytes
                .windows(SECRET.len())
                .any(|slice| slice == SECRET.as_bytes())
        );
        let document: Value = serde_json::from_slice(&bytes).unwrap();
        // Reviewed literal oracle, independently hashed from the static app's
        // role/name skeleton. Only the dynamic approved source digest is bound
        // from the already-approved input, never copied from replay output.
        let mut expected: Value =
            serde_json::from_str(include_str!("fixtures/journey_replay/checkout-cache.json"))
                .unwrap();
        let approved: Value = serde_yaml_ng::from_slice(
            &std::fs::read(source.join("checkout.journey.yaml")).unwrap(),
        )
        .unwrap();
        expected["flowDigest"] = approved["approved"]["digest"].clone();
        assert_eq!(
            document, expected,
            "cache disagrees with independent static-app oracle"
        );
        assert_eq!(document["edges"]["pay.submit"].as_array().unwrap().len(), 2);
        assert_eq!(
            document["screens"]["cart"]["controls"],
            json!([{ "role":"button","name":"Checkout" },{"role":"heading","name":"Cart"},{"role":"link","name":"Guest checkout"},{"role":"main","name":""}])
        );
        if let Some(first) = &first_bytes {
            assert_eq!(&bytes, first);
        } else {
            first_bytes = Some(bytes);
        }
        let readback = read_journeys(&project, &events, &keys);
        assert_eq!(readback["ignoredRecords"], 0);
        assert_eq!(readback["journeys"].as_array().unwrap().len(), 2);
        for journey in readback["journeys"].as_array().unwrap() {
            assert!(
                ["checkout", "checkout.guest"].contains(&journey["contractId"].as_str().unwrap())
            );
            for step in journey["steps"].as_array().unwrap() {
                assert_eq!(step["capture"]["freshness"], "fresh", "{readback}");
            }
            for arrow in journey["arrows"].as_array().unwrap() {
                assert_eq!(arrow["state"], "walked", "{readback}");
            }
        }
        assert_eq!(app.control("counts")["model"], 0);
        assert_eq!(git(&project, &["status", "--porcelain"]), "");
        println!("replay browser pass {}: {}", pass + 1, result["data"]);
    }
    // Public Runtime opens the sealed PNG referenced by the independent CLI reader.
    let readback = read_journeys(&project, &events, &keys);
    let checkout = readback["journeys"]
        .as_array()
        .unwrap()
        .iter()
        .find(|j| j["contractId"] == "checkout")
        .unwrap();
    let pay = checkout["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["stepId"] == "pay")
        .unwrap();
    let image = pay["capture"]["imageEvidenceId"].as_str().unwrap();
    let (headers, png) = http(
        address,
        &format!("/v1/executions/{RUN}/evidence/{image}"),
        Some(&token),
    );
    assert!(headers.to_lowercase().contains("content-type: image/png"));
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 1280);
    assert_eq!(u32::from_be_bytes(png[20..24].try_into().unwrap()), 720);
    let image_file = scratch.path().join("sealed-pay.png");
    std::fs::write(&image_file, png).unwrap();
    let decoded=Command::new("node").args(["-e",concat!(
        "(async()=>{const fs=require('node:fs'),{createRequire}=require('node:module');",
        "const {chromium}=createRequire(require('node:path').join(process.argv[1],'package.json'))('@playwright/test');",
        "const b=await chromium.launch({headless:true});try{const p=await b.newPage();",
        "const pixel=await p.evaluate(async data=>{const i=new Image();i.src='data:image/png;base64,'+data;await i.decode();",
        "const c=document.createElement('canvas');c.width=i.width;c.height=i.height;const x=c.getContext('2d');x.drawImage(i,0,0);",
        "return Array.from(x.getImageData(60,100,1,1).data);},fs.readFileSync(process.argv[2]).toString('base64'));",
        "if(JSON.stringify(pixel)!=='[255,0,255,255]')throw Error('secret field was not masked');",
        "console.log(JSON.stringify({pixel,browser:b.version()}));}finally{await b.close()}})().catch(()=>process.exit(1))"
    )]).arg(&toolchain).arg(&image_file).output().unwrap();
    assert!(decoded.status.success(), "sealed PNG pixel observer failed");
    println!(
        "sealed image observer: {}",
        String::from_utf8(decoded.stdout).unwrap().trim()
    );
    // No recording means assertions + cache only, with no new durable capture/arrow.
    let before = readback.clone();
    let mut stale: Value = serde_json::from_slice(&std::fs::read(&cache).unwrap()).unwrap();
    stale["flowDigest"] = format!("sha256:{}", "0".repeat(64)).into();
    std::fs::write(&cache, serde_json::to_vec_pretty(&stale).unwrap()).unwrap();
    let (code, result) = replay(&project, &events, &keys, false);
    assert_eq!(code, 0, "{result}");
    assert_eq!(result["data"]["recording"], "not_requested");
    assert_eq!(result["data"]["cacheReused"], false);
    assert_eq!(std::fs::read(&cache).unwrap(), first_bytes.unwrap());
    assert_eq!(read_journeys(&project, &events, &keys), before);
    // A cooperating writer cannot be raced; refusal preserves independent prior bytes.
    let previous_bytes = std::fs::read(&cache).unwrap();
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(cache.with_extension("lock"))
        .unwrap();
    fs2::FileExt::try_lock_exclusive(&lock).unwrap();
    let (code, result) = replay(&project, &events, &keys, false);
    assert_eq!(code, 3, "{result}");
    assert!(
        result["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "replay.cache_busy")
    );
    assert_eq!(std::fs::read(&cache).unwrap(), previous_bytes);
    fs2::FileExt::unlock(&lock).unwrap();
    drop(lock);
    // Real missing secret is not an action failure or a browser pass.
    let (code, result) = reply(
        cli()
            .args(["journey", "replay", "checkout", "--project"])
            .arg(&project)
            .env_remove("GRAPHHELM_SECRET_shopper_password"),
    );
    assert_eq!(code, 3, "{result}");
    assert!(
        result["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "driver.secret_missing")
    );
    assert_eq!(std::fs::read(&cache).unwrap(), previous_bytes);
    // Public recording refusal must not publish a replacement or invent walked arrows.
    let unknown_events = scratch.path().join("unstarted-events");
    let (code, result) = replay(&project, &unknown_events, &keys, true);
    assert_ne!(code, 0, "{result}");
    assert_eq!(result["data"]["cachePublished"], false);
    assert_eq!(result["data"]["paths"][0]["walkedPairs"], json!([]));
    assert_eq!(std::fs::read(&cache).unwrap(), previous_bytes);
    assert_eq!(read_journeys(&project, &events, &keys), before);
    // A destination failure retains the first sealed capture but invents no walked pair.
    assert_eq!(
        app.control(&json!({"kind":"missing-checkout"}).to_string())["armed"],
        true
    );
    let (code, result) = replay(&project, &events, &keys, true);
    assert_eq!(code, 1, "{result}");
    assert!(
        result["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "driver.expectation_failed"),
        "{result}"
    );
    assert_eq!(
        result["data"]["paths"][0]["capturedSignalIds"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(result["data"]["paths"][0]["walkedPairs"], json!([]));
    assert_eq!(result["data"]["cachePublished"], false);
    assert_eq!(std::fs::read(&cache).unwrap(), previous_bytes);
    assert_eq!(app.control("reset")["reset"], true);
    // An actual canonical-source edit during a browser action cannot publish a cache.
    let flow_path = project.join(".graphhelm/journeys/checkout.journey.yaml");
    let approved_bytes = std::fs::read(&flow_path).unwrap();
    assert_eq!(
        app.control(&json!({"kind":"edit-flow","path":flow_path}).to_string())["armed"],
        true
    );
    let (code, result) = replay(&project, &events, &keys, false);
    assert_eq!(code, 2, "{result}");
    assert!(
        result["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "replay.source_changed")
    );
    assert_eq!(result["data"]["cachePublished"], false);
    assert_ne!(
        std::fs::read(&flow_path).unwrap(),
        approved_bytes,
        "source-edit fault was not reached"
    );
    assert_eq!(std::fs::read(&cache).unwrap(), previous_bytes);
    std::fs::write(&flow_path, approved_bytes).unwrap();
    assert_eq!(app.control("reset")["reset"], true);
    #[cfg(windows)]
    {
        // Windows replacement refusal reaches publication after real browser execution.
        let original_permissions = std::fs::metadata(&cache).unwrap().permissions();
        let mut readonly = original_permissions.clone();
        readonly.set_readonly(true);
        std::fs::set_permissions(&cache, readonly).unwrap();
        let (code, result) = replay(&project, &events, &keys, false);
        std::fs::set_permissions(&cache, original_permissions).unwrap();
        assert_eq!(code, 1, "{result}");
        assert!(
            result["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|d| d["code"] == "replay.cache_write_refused")
        );
        assert!(
            result["data"]["paths"]
                .as_array()
                .unwrap()
                .iter()
                .all(|p| p["outcome"] == "passed"),
            "publication fault not reached"
        );
        assert_eq!(result["data"]["cachePublished"], false);
        assert_eq!(std::fs::read(&cache).unwrap(), previous_bytes);
        // A junction must not make a project cache writer reach an external owned directory.
        let original = cache.parent().unwrap();
        let external = scratch.path().join("linked-cache");
        std::fs::rename(original, &external).unwrap();
        assert!(
            Command::new("node")
                .args([
                    "-e",
                    "require('node:fs').symlinkSync(process.argv[1],process.argv[2],'junction')"
                ])
                .arg(&external)
                .arg(original)
                .status()
                .unwrap()
                .success()
        );
        let (code, result) = replay(&project, &events, &keys, false);
        assert_eq!(code, 2, "{result}");
        assert!(
            result["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|d| d["code"] == "replay.cache_invalid")
        );
        assert_eq!(
            std::fs::read(external.join("checkout.json")).unwrap(),
            previous_bytes
        );
        std::fs::remove_dir(original).unwrap();
        std::fs::rename(&external, original).unwrap();
    }
    // A dirty actual page source remains unknown; it cannot be promoted to fresh.
    std::fs::write(
        project.join("fixture-server.mjs"),
        format!("{FIXTURE}\n// dirty observer-owned source\n"),
    )
    .unwrap();
    assert_ne!(
        git(&project, &["status", "--porcelain"]),
        "",
        "dirty fixture edit was not visible to Git"
    );
    // Ruling 4 prefers an older clean capture over a newer dirty capture. Use a
    // separate store with no clean history to observe this run's unknown state;
    // do not change the established project reader to force it to choose dirty.
    let dirty_events = scratch.path().join("dirty-events");
    let (code, value) = reply(
        cli()
            .args(["execution", "start", "--file"])
            .arg(&graph)
            .arg("--events")
            .arg(&dirty_events)
            .args(["--execution", RUN, "--fixtures"])
            .arg(&fixtures)
            .args(["--mode", "manual", "--held"]),
    );
    assert_eq!(code, 0, "{value}");
    let (code, result) = replay(&project, &dirty_events, &keys, true);
    assert_eq!(code, 0, "{result}");
    let dirty = read_journeys(&project, &dirty_events, &keys);
    for journey in dirty["journeys"].as_array().unwrap() {
        for step in journey["steps"].as_array().unwrap() {
            assert_eq!(step["capture"]["freshness"], "unknown");
        }
    }
    assert_eq!(app.control("counts")["model"], 0);
}
