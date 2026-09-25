//! `graphhelm init` journey suite (#1062): an empty directory becomes a provisioned project, a
//! second run changes nothing, other people's harness registrations survive, malformed input is
//! refused with a code, neither secret is ever printed, and the paths it wrote are the paths
//! `serve` runs from.

mod support;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;
use support::raw_request;

const INIT_REFUSED: &str = "GHCLI027_INIT_REFUSED";
const ARGUMENT_INVALID: &str = "GHCLI001_ARGUMENT_INVALID";
const GATEWAY_CREDENTIAL: &str = "GHCLI010_GATEWAY_CREDENTIAL";

fn command() -> assert_cmd::Command {
    assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
}

fn json(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap_or_else(|error| {
        panic!(
            "stdout must be valid JSON ({error}): {:?}",
            String::from_utf8_lossy(bytes)
        )
    })
}

/// A project directory that git would call a work tree: `.git` exists.
fn git_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".git")).unwrap();
    dir
}

/// Runs `init` with both harnesses named explicitly, so the machine's own `~/.claude` or
/// `~/.codex` never decides what a test observes.
fn init(project: &Path) -> (Value, String, String) {
    let output = command()
        .args(["init", "--project"])
        .arg(project)
        .args(["--harness", "claude-code", "--harness", "codex"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    let value = json(stdout.as_bytes());
    assert_eq!(value["ok"], true, "init must succeed: {stdout}\n{stderr}");
    assert_eq!(value["command"], "init");
    (value, stdout, stderr)
}

fn is_hex64(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

struct Layout {
    root: PathBuf,
    events: PathBuf,
    token: PathBuf,
    key: PathBuf,
    keyring: PathBuf,
}

fn layout(project: &Path) -> Layout {
    let root = project.join(".graphhelm");
    Layout {
        events: root.join("events"),
        token: root.join("events.token"),
        key: root.join("serve.key"),
        keyring: root.join("keyring"),
        root,
    }
}

#[cfg(unix)]
fn assert_owner_only(path: &Path) {
    use std::os::unix::fs::MetadataExt;
    let mode = std::fs::metadata(path).unwrap().mode() & 0o777;
    assert_eq!(mode, 0o600, "{} must be 0600, is {mode:o}", path.display());
}

#[cfg(not(unix))]
fn assert_owner_only(_: &Path) {}

/// The keyring directory must be `0700`: `SealedKeyProvider` refuses anything looser, which is
/// what the clean-host rehearsal caught when `init` made it under the default umask.
#[cfg(unix)]
fn assert_owner_only_directory(path: &Path) {
    use std::os::unix::fs::MetadataExt;
    let mode = std::fs::metadata(path).unwrap().mode() & 0o777;
    assert_eq!(mode, 0o700, "{} must be 0700, is {mode:o}", path.display());
}

#[cfg(not(unix))]
fn assert_owner_only_directory(_: &Path) {}

#[test]
fn an_empty_directory_becomes_a_provisioned_project() {
    let project = git_project();
    let (data, stdout, stderr) = init(project.path());
    let data = &data["data"];
    let paths = layout(project.path());

    assert!(paths.events.is_dir());
    assert_eq!(data["events"]["state"], "created");
    // `data` shows project-relative paths with `/` on every platform (no home directory in normal
    // CLI JSON); the absolute spellings appear only inside the `next` command strings.
    assert_eq!(data["events"]["path"], ".graphhelm/events");
    assert_eq!(data["root"], ".graphhelm");
    assert_eq!(data["project"], project.path().to_str().unwrap());
    assert_eq!(data["bind"], "127.0.0.1:8791");

    let token = std::fs::read_to_string(&paths.token).unwrap();
    assert!(
        is_hex64(&token),
        "token must be 64 lowercase hex: {token:?}"
    );
    assert_eq!(data["token"]["state"], "created");
    assert_eq!(data["token"]["path"], ".graphhelm/events.token");
    assert_owner_only(&paths.token);

    let key = std::fs::read_to_string(&paths.key).unwrap();
    assert!(is_hex64(&key), "key must be 64 lowercase hex: {key:?}");
    assert_ne!(key, token);
    assert_eq!(data["key"]["state"], "created");
    assert_eq!(data["key"]["environment"], "GRAPHHELM_EVENTS_KEY");
    assert_owner_only(&paths.key);

    assert_eq!(data["keyring"]["state"], "created");
    assert_eq!(data["keyring"]["keyId"], "studio");
    assert_owner_only_directory(&paths.keyring);
    assert_eq!(data["keyring"]["path"], ".graphhelm/keyring");
    assert_eq!(data["key"]["path"], ".graphhelm/serve.key");
    assert_eq!(data["gitignore"]["path"], ".gitignore");
    // The keyring opens with the key: `gateway keyring init` pre-flights `open` and refuses to
    // replace a key it can open, so its refusal is the proof that the key in `serve.key` is the
    // key the keyring was created under.
    let reopen = command()
        .args(["gateway", "keyring", "init", "--keyring"])
        .arg(&paths.keyring)
        .args(["--key-id", "studio"])
        .env("GRAPHHELM_EVENTS_KEY", key.trim())
        .output()
        .unwrap();
    let reopen = json(&reopen.stdout);
    assert_eq!(reopen["ok"], false);
    assert_eq!(reopen["diagnostics"][0]["code"], GATEWAY_CREDENTIAL);
    assert!(
        reopen["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("already holds that key"),
        "{reopen}"
    );

    let gitignore = std::fs::read_to_string(project.path().join(".gitignore")).unwrap();
    assert!(
        gitignore.lines().any(|line| line == ".graphhelm/"),
        "{gitignore:?}"
    );
    assert_eq!(data["gitignore"]["state"], "created");

    let mcp: Value =
        serde_json::from_str(&std::fs::read_to_string(project.path().join(".mcp.json")).unwrap())
            .unwrap();
    let entry = &mcp["mcpServers"]["graphhelm"];
    // The registration names THIS binary by absolute path, so a harness finds it whether or not
    // `graphhelm` is on PATH (the "would rather not install" path of GETTING_STARTED §1).
    let registered = std::fs::canonicalize(entry["command"].as_str().unwrap()).unwrap();
    let this_binary = std::fs::canonicalize(assert_cmd::cargo::cargo_bin!("graphhelm")).unwrap();
    assert_eq!(registered, this_binary);
    let args: Vec<&str> = entry["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap())
        .collect();
    assert_eq!(
        args,
        [
            "mcp",
            "--url",
            "http://127.0.0.1:8791",
            "--token-file",
            paths.token.to_str().unwrap(),
            "--actor",
            "agent-chat",
        ]
    );
    let harnesses = data["harnesses"].as_array().unwrap();
    assert_eq!(harnesses.len(), 2);
    assert_eq!(harnesses[0]["harness"], "claude-code");
    assert_eq!(harnesses[0]["state"], "created");
    assert_eq!(harnesses[1]["harness"], "codex");
    assert_eq!(harnesses[1]["state"], "created");
    let codex = std::fs::read_to_string(paths.root.join("codex.config.toml")).unwrap();
    assert!(codex.contains("[mcp_servers.graphhelm]"), "{codex}");
    assert!(!codex.contains("command = \"graphhelm\""), "{codex}");
    assert!(codex.contains("\"--token-file\""), "{codex}");
    assert!(codex.contains("~/.codex/config.toml"), "{codex}");

    let next = data["next"].as_array().unwrap();
    assert_eq!(next.len(), 4);
    assert!(
        next[0]["powershell"]
            .as_str()
            .unwrap()
            .contains("GRAPHHELM_EVENTS_KEY")
    );
    assert!(
        next[0]["bash"]
            .as_str()
            .unwrap()
            .contains("GRAPHHELM_EVENTS_KEY")
    );
    assert!(
        next[1]["bash"]
            .as_str()
            .unwrap()
            .contains("graphhelm serve --events")
    );
    assert!(
        next[1]["bash"]
            .as_str()
            .unwrap()
            .contains("--key-id studio")
    );
    assert!(
        next[2]["powershell"]
            .as_str()
            .unwrap()
            .contains("apps/studio/tools/studio-up.ps1")
    );
    assert!(
        next[3]["bash"]
            .as_str()
            .unwrap()
            .contains("graphhelm execution start")
    );

    // Neither secret's bytes may ever reach an operator's terminal or a harness log.
    for (name, value) in [("token", token.trim()), ("key", key.trim())] {
        assert!(!stdout.contains(value), "stdout carries the {name} value");
        assert!(!stderr.contains(value), "stderr carries the {name} value");
        assert!(
            !codex.contains(value),
            "the Codex snippet carries the {name} value"
        );
        assert!(
            !mcp.to_string().contains(value),
            ".mcp.json carries the {name} value"
        );
    }
}

#[test]
fn a_second_run_finds_everything_and_changes_nothing() {
    let project = git_project();
    init(project.path());
    let paths = layout(project.path());
    let token_before = std::fs::read(&paths.token).unwrap();
    let key_before = std::fs::read(&paths.key).unwrap();
    let gitignore_before = std::fs::read_to_string(project.path().join(".gitignore")).unwrap();
    let mcp_before = std::fs::read_to_string(project.path().join(".mcp.json")).unwrap();

    let (data, _, _) = init(project.path());
    let data = &data["data"];
    for artifact in ["events", "token", "key", "keyring", "gitignore"] {
        assert_eq!(data[artifact]["state"], "existing", "{artifact}: {data}");
    }
    for harness in data["harnesses"].as_array().unwrap() {
        assert_eq!(harness["state"], "existing", "{harness}");
    }
    assert_eq!(std::fs::read(&paths.token).unwrap(), token_before);
    assert_eq!(std::fs::read(&paths.key).unwrap(), key_before);
    assert_eq!(
        std::fs::read_to_string(project.path().join(".gitignore")).unwrap(),
        gitignore_before,
        "the ignore block must not be appended twice"
    );
    assert_eq!(
        std::fs::read_to_string(project.path().join(".mcp.json")).unwrap(),
        mcp_before
    );
}

#[test]
fn an_existing_mcp_json_is_merged_and_other_servers_survive() {
    let project = git_project();
    std::fs::write(
        project.path().join(".mcp.json"),
        r#"{"mcpServers":{"other":{"command":"other-server","args":["--x"]}},"unrelated":1}"#,
    )
    .unwrap();
    let (data, _, _) = init(project.path());
    assert_eq!(data["data"]["harnesses"][0]["state"], "merged");
    let mcp: Value =
        serde_json::from_str(&std::fs::read_to_string(project.path().join(".mcp.json")).unwrap())
            .unwrap();
    assert_eq!(mcp["mcpServers"]["other"]["command"], "other-server");
    assert_eq!(
        std::fs::canonicalize(mcp["mcpServers"]["graphhelm"]["command"].as_str().unwrap()).unwrap(),
        std::fs::canonicalize(assert_cmd::cargo::cargo_bin!("graphhelm")).unwrap()
    );
    assert_eq!(mcp["unrelated"], 1);
}

#[test]
fn an_mcp_json_that_is_not_an_object_is_refused_and_left_alone() {
    let project = git_project();
    let path = project.path().join(".mcp.json");
    std::fs::write(&path, "[1, 2, 3]").unwrap();
    let output = command()
        .args(["init", "--project"])
        .arg(project.path())
        .args(["--harness", "claude-code"])
        .output()
        .unwrap();
    let value = json(&output.stdout);
    assert_eq!(value["ok"], false);
    assert_eq!(value["diagnostics"][0]["code"], INIT_REFUSED);
    assert_eq!(value["diagnostics"][0]["path"], "/mcp_json");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "[1, 2, 3]");
    // Everything before the refusal is still provisioned; a re-run after the fix is idempotent.
    assert!(layout(project.path()).token.is_file());
}

#[test]
fn a_missing_project_directory_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let output = command()
        .args(["init", "--project"])
        .arg(dir.path().join("does-not-exist"))
        .output()
        .unwrap();
    let value = json(&output.stdout);
    assert_eq!(value["ok"], false);
    assert_eq!(value["diagnostics"][0]["code"], INIT_REFUSED);
    assert_eq!(value["diagnostics"][0]["path"], "/project");
}

#[test]
fn a_non_loopback_bind_is_refused_before_anything_is_written() {
    let project = git_project();
    let output = command()
        .args(["init", "--project"])
        .arg(project.path())
        .args(["--bind", "0.0.0.0:8791"])
        .output()
        .unwrap();
    let value = json(&output.stdout);
    assert_eq!(value["ok"], false);
    assert_eq!(value["diagnostics"][0]["code"], ARGUMENT_INVALID);
    assert_eq!(value["diagnostics"][0]["path"], "/bind");
    assert!(!project.path().join(".graphhelm").exists());
}

#[test]
fn outside_a_git_work_tree_no_gitignore_is_written() {
    let project = tempfile::tempdir().unwrap();
    let (data, _, _) = init(project.path());
    assert_eq!(data["data"]["gitignore"]["state"], "not_a_git_work_tree");
    assert!(!project.path().join(".gitignore").exists());
}

#[test]
fn a_gitignore_that_already_ignores_the_directory_is_left_alone() {
    let project = git_project();
    std::fs::write(project.path().join(".gitignore"), "target/\n/.graphhelm\n").unwrap();
    let (data, _, _) = init(project.path());
    assert_eq!(data["data"]["gitignore"]["state"], "existing");
    assert_eq!(
        std::fs::read_to_string(project.path().join(".gitignore")).unwrap(),
        "target/\n/.graphhelm\n"
    );
}

#[test]
fn a_gitignore_without_a_trailing_newline_gets_the_block_on_its_own_line() {
    let project = git_project();
    std::fs::write(project.path().join(".gitignore"), "target/").unwrap();
    let (data, _, _) = init(project.path());
    assert_eq!(data["data"]["gitignore"]["state"], "appended");
    let text = std::fs::read_to_string(project.path().join(".gitignore")).unwrap();
    assert!(text.starts_with("target/\n"), "{text:?}");
    assert!(text.lines().any(|line| line == ".graphhelm/"), "{text:?}");
}

/// Detection is read from the home directory, so the test owns one: `HOME` (Unix) and
/// `USERPROFILE` (Windows) both point at a directory carrying `.codex/`, and the project carries
/// `.claude/`.
#[test]
fn harnesses_are_detected_from_the_project_and_the_home_directory() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir(home.path().join(".codex")).unwrap();
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir(project.path().join(".claude")).unwrap();
    let output = command()
        .args(["init", "--project"])
        .arg(project.path())
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .output()
        .unwrap();
    let value = json(&output.stdout);
    assert_eq!(value["ok"], true, "{value}");
    let names: Vec<&str> = value["data"]["harnesses"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["harness"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["claude-code", "codex"]);
    assert!(project.path().join(".mcp.json").is_file());
    assert!(
        !home.path().join(".codex").join("config.toml").exists(),
        "the home directory is never written"
    );

    // Neither marker: nothing registered, and that is stated rather than guessed.
    let bare_home = tempfile::tempdir().unwrap();
    let bare_project = tempfile::tempdir().unwrap();
    let output = command()
        .args(["init", "--project"])
        .arg(bare_project.path())
        .env("HOME", bare_home.path())
        .env("USERPROFILE", bare_home.path())
        .output()
        .unwrap();
    let value = json(&output.stdout);
    assert_eq!(value["ok"], true, "{value}");
    assert_eq!(value["data"]["harnesses"].as_array().unwrap().len(), 0);
    assert!(!bare_project.path().join(".mcp.json").exists());
}

/// The first stdout line, or a panic after `budget` (the child killed first): a `read_line`
/// straight on the pipe has no bound at all, so a server that never prints would hang the suite
/// rather than fail it (PR #1070 review).
fn first_stdout_line_within(child: &mut std::process::Child, budget: Duration) -> String {
    let stdout = child.stdout.take().unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        let _ = reader.read_line(&mut line);
        let _ = sender.send(line);
    });
    match receiver.recv_timeout(budget) {
        Ok(line) => line,
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            panic!("`graphhelm serve` printed nothing on stdout within {budget:?}")
        }
    }
}

/// One `/health` probe whose WHOLE life — connect, write, every read — is bounded by `deadline`.
/// `support::raw_request` bounds each read at 15 s and reads to EOF, so a listener that connects
/// and then dribbles bytes could hold it past the caller's 10 s budget with the clock consulted
/// only afterwards (AGENTS.md: "a deadline consulted once per iteration bounds the iterations,
/// not the wall time"; PR #1070 review). Here the remaining budget is recomputed before every
/// blocking call, the read is capped at 64 KiB, and the answer is "200 seen" or "not (yet)".
fn health_answers_within(base: &str, deadline: Instant) -> bool {
    let remaining = || deadline.saturating_duration_since(Instant::now());
    let (host, port, _) = support::split_url(base);
    let address = match (host.as_str(), port).to_socket_addrs_first() {
        Some(address) => address,
        None => return false,
    };
    let Ok(mut stream) =
        TcpStream::connect_timeout(&address, remaining().max(Duration::from_millis(1)))
    else {
        return false;
    };
    let request = format!("GET /health HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    if stream
        .set_write_timeout(Some(remaining().max(Duration::from_millis(1))))
        .is_err()
        || stream.write_all(request.as_bytes()).is_err()
    {
        return false;
    }
    let mut raw = Vec::new();
    let mut chunk = [0_u8; 4096];
    while raw.len() < 64 * 1024 {
        let left = remaining();
        if left.is_zero() || stream.set_read_timeout(Some(left)).is_err() {
            return false;
        }
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => raw.extend_from_slice(&chunk[..n]),
            Err(_) => return false,
        }
    }
    String::from_utf8_lossy(&raw).starts_with("HTTP/1.1 200")
}

trait FirstAddress {
    fn to_socket_addrs_first(&self) -> Option<std::net::SocketAddr>;
}

impl FirstAddress for (&str, u16) {
    fn to_socket_addrs_first(&self) -> Option<std::net::SocketAddr> {
        use std::net::ToSocketAddrs;
        self.to_socket_addrs().ok()?.next()
    }
}

/// The paths `init` wrote are the paths `serve` runs from: started with them and the key from
/// `serve.key`, the Runtime answers `/health`, refuses a bare request, and lets the token `init`
/// minted through to the router.
#[test]
fn serve_runs_from_the_paths_init_wrote() {
    let project = git_project();
    init(project.path());
    let paths = layout(project.path());
    let token = std::fs::read_to_string(&paths.token).unwrap();
    let key = std::fs::read_to_string(&paths.key).unwrap();

    let mut child = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args(["serve", "--events"])
        .arg(&paths.events)
        .args(["--bind", "127.0.0.1:0", "--keyring"])
        .arg(&paths.keyring)
        .args(["--key-id", "studio"])
        .env("GRAPHHELM_EVENTS_KEY", key.trim())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let line = first_stdout_line_within(&mut child, Duration::from_secs(30));
    let started: Value = serde_json::from_str(line.trim()).unwrap_or_else(|error| {
        let _ = child.kill();
        panic!("the startup line was not JSON ({error}): {line:?}")
    });
    assert_eq!(started["command"], "serve.started", "{started}");
    assert_eq!(started["ok"], true, "{started}");
    let base = format!("http://{}", started["data"]["address"].as_str().unwrap());

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if health_answers_within(&base, deadline) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the server never answered /health"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let bare = raw_request(&format!("{base}/does-not-exist"), None).unwrap();
    let bearer = raw_request(&format!("{base}/does-not-exist"), Some(token.trim())).unwrap();
    let _ = child.kill();
    let _ = child.wait();
    assert_eq!(bare.status, 401, "{}", bare.body);
    assert_eq!(bearer.status, 404, "{}", bearer.body);
    assert!(!line.contains(token.trim()));
    assert!(!line.contains(key.trim()));
}

/// With `--keyring` given, `serve` pre-flights the keyring at start. A key that does not open it
/// is NOT a refusal — a fixture story never seals, and `resume_atomicity.rs` relies on starting
/// with the keyring flags and no key — but it is a WARNING on the `serve.started` line naming
/// the consequence, so the operator who will seal learns at start rather than at the first send.
#[test]
fn serve_starts_with_a_warning_when_the_key_does_not_open_the_keyring() {
    let project = git_project();
    init(project.path());
    let paths = layout(project.path());
    let started_with = |configure: &dyn Fn(&mut Command)| -> Value {
        let mut command = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"));
        command
            .args(["serve", "--events"])
            .arg(&paths.events)
            .args(["--bind", "127.0.0.1:0", "--keyring"])
            .arg(&paths.keyring)
            .args(["--key-id", "studio"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        configure(&mut command);
        let mut child = command.spawn().unwrap();
        let line = first_stdout_line_within(&mut child, Duration::from_secs(30));
        let _ = child.kill();
        let _ = child.wait();
        serde_json::from_str(line.trim())
            .unwrap_or_else(|error| panic!("the startup line was not JSON ({error}): {line:?}"))
    };

    // A well-formed key that is not the keyring's.
    let started = started_with(&|command| {
        command.env("GRAPHHELM_EVENTS_KEY", "0123456789abcdef".repeat(4));
    });
    assert_eq!(started["ok"], true, "{started}");
    assert_eq!(started["command"], "serve.started");
    assert!(started["data"]["address"].is_string(), "{started}");
    let warning = &started["diagnostics"][0];
    assert_eq!(warning["severity"], "warning", "{started}");
    assert_eq!(warning["code"], "GHCLI006_SERVE_INVALID");
    assert_eq!(warning["path"], "/keyring");
    assert!(
        warning["message"]
            .as_str()
            .unwrap()
            .contains("sealed operations (messages, real executors) will refuse"),
        "{started}"
    );

    // The variable unset entirely: same warning, startup still succeeds.
    let started = started_with(&|command| {
        command.env_remove("GRAPHHELM_EVENTS_KEY");
    });
    assert_eq!(started["ok"], true, "{started}");
    assert_eq!(
        started["diagnostics"][0]["severity"], "warning",
        "{started}"
    );
    assert_eq!(started["diagnostics"][0]["path"], "/keyring");

    // The right key: no warning at all.
    let key = std::fs::read_to_string(&paths.key).unwrap();
    let started = started_with(&|command| {
        command.env("GRAPHHELM_EVENTS_KEY", key.trim());
    });
    assert_eq!(started["ok"], true, "{started}");
    assert_eq!(
        started["diagnostics"].as_array().unwrap().len(),
        0,
        "{started}"
    );
}

/// On Windows a fresh file inherits the directory ACL (every local user can read it); both
/// secrets must instead carry the keyring's protected owner-only DACL, read back through the
/// provider's own verifier.
#[cfg(windows)]
#[test]
fn on_windows_both_secrets_carry_the_protected_owner_only_acl() {
    let project = git_project();
    init(project.path());
    let paths = layout(project.path());
    for secret in [&paths.token, &paths.key] {
        let file = std::fs::File::open(secret).unwrap();
        graphhelm_sealed_key_provider::verify_owner_only(&file)
            .unwrap_or_else(|error| panic!("{} is not owner-only: {error:?}", secret.display()));
    }
    // And a file created the ordinary way, in the same directory, is NOT — so the check above
    // is discriminating rather than vacuous.
    let ordinary = paths.root.join("ordinary.txt");
    std::fs::write(&ordinary, "x").unwrap();
    let file = std::fs::File::open(&ordinary).unwrap();
    assert!(graphhelm_sealed_key_provider::verify_owner_only(&file).is_err());
}

/// Git applies the LAST matching line, so `.graphhelm/` followed by `!.graphhelm/` leaves the
/// directory tracked; `init` must see through that and append its block (which, being last, wins).
#[test]
fn a_negated_ignore_line_is_not_taken_as_ignored() {
    let project = git_project();
    std::fs::write(
        project.path().join(".gitignore"),
        ".graphhelm/\n!.graphhelm/\n",
    )
    .unwrap();
    let (data, _, _) = init(project.path());
    assert_eq!(data["data"]["gitignore"]["state"], "appended");
    let text = std::fs::read_to_string(project.path().join(".gitignore")).unwrap();
    assert!(text.ends_with(".graphhelm/\n"), "{text:?}");
    assert!(
        text.lines().filter(|line| *line == ".graphhelm/").count() == 2,
        "{text:?}"
    );
}

/// A project that is a subdirectory of a repository is inside that work tree even with no `.git`
/// of its own; the block goes into the SUBDIRECTORY's `.gitignore`, and the root's is untouched.
#[test]
fn a_project_nested_in_an_ancestor_work_tree_gets_its_own_gitignore() {
    let repository = git_project();
    std::fs::write(repository.path().join(".gitignore"), "target/\n").unwrap();
    let project = repository.path().join("services").join("api");
    std::fs::create_dir_all(&project).unwrap();
    let (data, _, _) = init(&project);
    assert_eq!(data["data"]["gitignore"]["state"], "created");
    let nested = std::fs::read_to_string(project.join(".gitignore")).unwrap();
    assert!(
        nested.lines().any(|line| line == ".graphhelm/"),
        "{nested:?}"
    );
    assert_eq!(
        std::fs::read_to_string(repository.path().join(".gitignore")).unwrap(),
        "target/\n",
        "the repository root's .gitignore must not be edited"
    );
}

/// `--key-id` is written into the printed shell commands; anything a shell would interpret is
/// refused before a byte is written.
#[test]
fn a_shell_interpreting_key_id_is_refused_before_anything_is_written() {
    let project = git_project();
    let output = command()
        .args(["init", "--project"])
        .arg(project.path())
        .args(["--key-id", "$(id)"])
        .output()
        .unwrap();
    let value = json(&output.stdout);
    assert_eq!(value["ok"], false);
    assert_eq!(value["diagnostics"][0]["code"], ARGUMENT_INVALID);
    assert_eq!(value["diagnostics"][0]["path"], "/key_id");
    assert!(!project.path().join(".graphhelm").exists());
}

/// A `.gitignore` past the 1 MiB bound is refused by its metadata, not read.
#[test]
fn an_oversize_gitignore_is_refused_without_being_read() {
    let project = git_project();
    let path = project.path().join(".gitignore");
    let file = std::fs::File::create(&path).unwrap();
    file.set_len(1024 * 1024 + 1).unwrap();
    let output = command()
        .args(["init", "--project"])
        .arg(project.path())
        .args(["--harness", "claude-code"])
        .output()
        .unwrap();
    let value = json(&output.stdout);
    assert_eq!(value["ok"], false, "{value}");
    assert_eq!(value["diagnostics"][0]["code"], INIT_REFUSED);
    assert_eq!(value["diagnostics"][0]["path"], "/gitignore");
    assert_eq!(std::fs::metadata(&path).unwrap().len(), 1024 * 1024 + 1);
}

/// A `.graphhelm` that is a symbolic link to elsewhere would carry the token, the key and a
/// `chmod 0700` outside the project; it is refused, and the link's target is untouched.
#[cfg(unix)]
#[test]
fn a_symlinked_runtime_directory_is_refused_and_its_target_untouched() {
    let elsewhere = tempfile::tempdir().unwrap();
    let project = git_project();
    std::os::unix::fs::symlink(elsewhere.path(), project.path().join(".graphhelm")).unwrap();
    let output = command()
        .args(["init", "--project"])
        .arg(project.path())
        .output()
        .unwrap();
    let value = json(&output.stdout);
    assert_eq!(value["ok"], false, "{value}");
    assert_eq!(value["diagnostics"][0]["code"], INIT_REFUSED);
    assert_eq!(value["diagnostics"][0]["path"], "/root");
    assert_eq!(std::fs::read_dir(elsewhere.path()).unwrap().count(), 0);
}
