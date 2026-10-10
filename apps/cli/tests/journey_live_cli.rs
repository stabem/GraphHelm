//! #398: `journey open` / `journey act` / `journey close` refuse before any browser I/O when the
//! walk could not be deterministic or the session does not exist. Credible defects: opening a
//! flow with no replay cache (or a void one) and walking it some other way, a step no path
//! reaches, and `act` reaching a session that is not there. Real CLI/tempdir/Git boundary, the
//! installed observer is a tripwire that must never start; cost: seconds after build, offline,
//! no Node/browser/provider.
#[path = "support/time_scale.rs"]
mod time_scale;
use time_scale::scaled;

use std::path::Path;
use std::process::Command;

use serde_json::Value;

const FLOW: &str = include_str!("fixtures/journey_flow/checkout.journey.yaml");
const CACHE: &str = include_str!("fixtures/journey_replay/checkout-cache.json");

fn cli(project: &Path, args: &[&str]) -> (i32, Value) {
    let out = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args(["--json", "journey"])
        .args(args)
        .arg("--project")
        .arg(project)
        .env("GRAPHHELM_SECRET_shopper_password", "live_canary_398")
        .output()
        .unwrap();
    (
        out.status.code().unwrap(),
        serde_json::from_slice(&out.stdout).unwrap_or(Value::Null),
    )
}

/// An approved `checkout` flow in a committed project, with a tripwire observer installed.
fn approved() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for path in [
        "app/cart/page.tsx",
        "app/checkout/page.tsx",
        "app/api/pay/route.ts",
    ] {
        let file = dir.path().join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, "export {}").unwrap();
    }
    std::fs::create_dir_all(dir.path().join(".graphhelm/journeys")).unwrap();
    std::fs::write(
        dir.path().join(".graphhelm/journeys/checkout.journey.yaml"),
        FLOW,
    )
    .unwrap();
    let (code, reply) = cli(
        dir.path(),
        &["compile", "checkout", "--fmt", "--include-draft"],
    );
    assert_eq!(code, 0, "{reply}");
    for args in [
        vec!["init", "--quiet", "--object-format=sha1"],
        vec!["config", "user.email", "live@example.invalid"],
        vec!["config", "user.name", "Live observer"],
        vec!["add", "."],
        vec!["commit", "--quiet", "--no-verify", "-m", "fixture"],
    ] {
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }
    let token = owner_token(dir.path());
    let (code, reply) = cli(dir.path(), &["approve", "checkout", "--token-file", &token]);
    assert_eq!(code, 0, "{reply}");
    tripwire(dir.path());
    dir
}

/// The same project with the flow left a DRAFT (never approved), and the same tripwire.
fn draft() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for path in [
        "app/cart/page.tsx",
        "app/checkout/page.tsx",
        "app/api/pay/route.ts",
    ] {
        let file = dir.path().join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, "export {}").unwrap();
    }
    std::fs::create_dir_all(dir.path().join(".graphhelm/journeys")).unwrap();
    std::fs::write(
        dir.path().join(".graphhelm/journeys/checkout.journey.yaml"),
        FLOW,
    )
    .unwrap();
    let (code, reply) = cli(
        dir.path(),
        &["compile", "checkout", "--fmt", "--include-draft"],
    );
    assert_eq!(code, 0, "{reply}");
    tripwire(dir.path());
    dir
}

fn tripwire(project: &Path) {
    let observer = project.join(".graphhelm/observers");
    std::fs::create_dir_all(&observer).unwrap();
    std::fs::write(
        observer.join("journey_driver.mjs"),
        "import {writeFileSync} from 'node:fs';writeFileSync('DRIVER_STARTED','unsafe');",
    )
    .unwrap();
}

fn refuses(project: &Path, args: &[&str], command: &str, exit: i32, code: &str) -> Value {
    let (actual, reply) = cli(project, args);
    assert_eq!(actual, exit, "{reply}");
    assert_eq!(reply["command"], command, "{reply}");
    assert!(
        reply["diagnostics"]
            .as_array()
            .is_some_and(|ds| ds.iter().any(|d| d["code"] == code)),
        "expected {code}: {reply}"
    );
    assert!(!project.join("DRIVER_STARTED").exists());
    reply
}

#[test]
fn open_without_a_current_replay_cache_is_refused_naming_replay() {
    let dir = approved();
    let reply = refuses(
        dir.path(),
        &["open", "checkout", "--step", "pay"],
        "journey.open",
        2,
        "replay.cache_missing",
    );
    assert!(
        reply.to_string().contains("journey replay"),
        "the refusal names the remedy: {reply}"
    );

    // A cache whose flow digest is not the approved flow's is void: still refused.
    let mut cache: Value = serde_json::from_str(CACHE).unwrap();
    cache["flowDigest"] = format!("sha256:{}", "0".repeat(64)).into();
    let directory = dir.path().join(".graphhelm/journey-cache");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("checkout.json"), cache.to_string()).unwrap();
    refuses(
        dir.path(),
        &["open", "checkout", "--step", "pay"],
        "journey.open",
        2,
        "replay.cache_missing",
    );
}

#[test]
fn a_step_no_path_reaches_and_a_partial_recording_bundle_are_refused() {
    let dir = approved();
    refuses(
        dir.path(),
        &["open", "checkout", "--step", "nowhere"],
        "journey.open",
        2,
        "live.step_unreachable",
    );
    refuses(
        dir.path(),
        &["open", "checkout", "--step", "pay", "--path", "express"],
        "journey.open",
        2,
        "live.step_unreachable",
    );
    refuses(
        dir.path(),
        &["open", "checkout", "--step", "pay", "--execution", "x"],
        "journey.open",
        3,
        "live.recording_incomplete",
    );
}

#[test]
fn act_and_close_without_a_live_session_are_refused() {
    let dir = approved();
    let (code, listed) = cli(dir.path(), &["sessions"]);
    assert_eq!(code, 0, "{listed}");
    assert_eq!(listed["command"], "journey.sessions");
    assert_eq!(
        listed["data"]["sessions"],
        serde_json::json!([]),
        "{listed}"
    );
    refuses(
        dir.path(),
        &[
            "act",
            "live-0123456789abcdef",
            "--kind",
            "activate",
            "--role",
            "button",
            "--name",
            "Checkout",
        ],
        "journey.act",
        2,
        "live.session_gone",
    );
    refuses(
        dir.path(),
        &["close", "../outside"],
        "journey.close",
        3,
        "live.request_invalid",
    );
}

/// `journey watch` plays a flow before its owner approves it, so a DRAFT must get past both gates
/// that refuse `journey open` (approval and replay cache) and stop only where a browser would be
/// needed (the tripwire observer is refused as missing, so no driver starts). It writes no
/// session record when it fails, and a pace slower than its bound is refused before anything.
/// Credible defect: watch inheriting open's `flow.not_approved` / `replay.cache_missing`, which
/// makes the owner's look-before-approve impossible. Cost: seconds, no browser.
#[test]
fn watch_plays_a_draft_past_the_gates_that_refuse_open() {
    let dir = draft();
    refuses(
        dir.path(),
        &["open", "checkout", "--step", "pay"],
        "journey.open",
        2,
        "flow.not_approved",
    );
    let reply = refuses(
        dir.path(),
        &["watch", "checkout"],
        "journey.watch",
        3,
        "replay.observer_missing",
    );
    assert_eq!(reply["data"]["mode"], "watch", "{reply}");
    assert_eq!(reply["data"]["proof"], false, "{reply}");
    let sessions = dir.path().join(".graphhelm/journey-sessions");
    assert!(
        std::fs::read_dir(&sessions)
            .map(|entries| entries.count() == 0)
            .unwrap_or(true),
        "a failed watch leaves no session record"
    );
    refuses(
        dir.path(),
        &["watch", "checkout", "--pace-ms", "20000"],
        "journey.watch",
        3,
        "watch.pace_invalid",
    );
    refuses(
        dir.path(),
        &["watch", "checkout", "--path", "nowhere"],
        "journey.watch",
        2,
        "watch.path_unknown",
    );
}

/// The owner only clicks Watch, so an app under test that is down is started from the project's
/// launcher, and with none declared watch says what is missing instead of failing in the browser.
/// A declaration that points outside the project is refused before anything runs. Uses a real
/// observer file so the gates before the browser are all passed. Cost: seconds, no browser (the
/// flow's base is a closed local port).
#[test]
fn watch_starts_a_down_app_only_from_a_launcher_inside_the_project() {
    let dir = draft();
    // The fixture flow's base is localhost:3000, which a developer box may well be serving.
    // Point it at a port this test just reserved and released, so "down" is what it measures.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let flow = dir.path().join(".graphhelm/journeys/checkout.journey.yaml");
    let text = std::fs::read_to_string(&flow)
        .unwrap()
        .replace("http://localhost:3000", &format!("http://127.0.0.1:{port}"));
    assert!(
        text.contains(&format!("127.0.0.1:{port}")),
        "ARRANGEMENT: the base was rewritten"
    );
    std::fs::write(&flow, text).unwrap();
    std::fs::write(
        dir.path().join(".graphhelm/observers/journey_driver.mjs"),
        include_bytes!("../../../tools/journey-driver/driver.mjs"),
    )
    .unwrap();
    refuses(
        dir.path(),
        &["watch", "checkout"],
        "journey.watch",
        2,
        "watch.app_down",
    );
    std::fs::write(
        dir.path().join(".graphhelm/journey-fixture.json"),
        r#"{"schema":"graphhelm-journey-fixture/1","script":"../outside.sh"}"#,
    )
    .unwrap();
    refuses(
        dir.path(),
        &["watch", "checkout"],
        "journey.watch",
        2,
        "watch.launcher_invalid",
    );
}

/// #585: flow-specific fixture seeds require the selected flow id at the launcher boundary.
/// Existing launcher checks cover confinement, not its environment. The real shell records
/// the id before deliberately failing, so no browser or long readiness wait is needed.
/// Cost: two CLI/shell launches, seconds, offline; no production seam.
#[test]
fn watch_and_preview_pass_the_selected_flow_to_the_launcher() {
    let dir = draft();
    std::fs::write(
        dir.path().join(".graphhelm/observers/journey_driver.mjs"),
        include_bytes!("../../../tools/journey-driver/driver.mjs"),
    )
    .unwrap();
    std::fs::write(
        dir.path().join(".graphhelm/journey-fixture.json"),
        r#"{"schema":"graphhelm-journey-fixture/1","script":"fixture.sh","isolated":true}"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("fixture.sh"),
        "if [ \"$1\" = up ]; then printf '%s' \"${GRAPHHELM_JOURNEY_FLOW:-}\" > selected-flow; exit 1; fi\n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.path().join(".graphhelm/journey-previews/checkout")).unwrap();
    for args in [
        vec!["watch", "checkout"],
        vec!["preview", "checkout", "--run"],
    ] {
        let (_, reply) = cli(dir.path(), &args);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("selected-flow"))
                .unwrap_or_else(|error| panic!("{args:?}: {reply}; {error}")),
            "checkout",
            "{args:?}: {reply}"
        );
        std::fs::remove_file(dir.path().join("selected-flow")).unwrap();
    }
}

/// R6: a fixture must get its chance to mint a declared secret before preflight reads it.
/// The existing launcher test supplies its secret in the parent environment and misses this.
/// Cost: two real CLI/shell launches, seconds, offline; no browser or production seam.
#[test]
fn watch_and_preview_launch_before_refusing_a_fixture_minted_secret() {
    let dir = draft();
    let flow = dir.path().join(".graphhelm/journeys/checkout.journey.yaml");
    std::fs::write(&flow, FLOW.replace("shopper_password", "r6_fixture_token")).unwrap();
    std::fs::write(
        dir.path().join(".graphhelm/observers/journey_driver.mjs"),
        include_bytes!("../../../tools/journey-driver/driver.mjs"),
    )
    .unwrap();
    std::fs::write(
        dir.path().join(".graphhelm/journey-fixture.json"),
        r#"{"schema":"graphhelm-journey-fixture/1","script":"fixture.sh","isolated":true}"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("fixture.sh"),
        "if [ \"$1\" = up ]; then echo called > launched; exit 1; fi\n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.path().join(".graphhelm/journey-previews/checkout")).unwrap();
    for args in [
        vec!["watch", "checkout"],
        vec!["preview", "checkout", "--run"],
    ] {
        let (_, reply) = cli(dir.path(), &args);
        assert!(dir.path().join("launched").exists(), "{args:?}: {reply}");
        std::fs::remove_file(dir.path().join("launched")).unwrap();
    }
}

/// R6: uppercase launcher keys supply lowercase flow ids, and the existing literal-secret
/// refusal still runs after launch. Poisoned non-secret keys must not change the runner.
/// Existing CLI preflight tests only supply parent environment values, not a launcher file.
/// Cost: two CLI/shell launches and local listeners, seconds, offline; no browser or new seam.
#[test]
fn launcher_secrets_reach_preflight_without_exporting_other_keys_or_values() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use std::time::{Duration, Instant};

    for args in [
        vec!["watch", "checkout"],
        vec!["preview", "checkout", "--run"],
    ] {
        let dir = draft();
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let flow = FLOW
            .replace("shopper_password", "r6_fixture_token")
            .replace("http://localhost:3000", &format!("http://127.0.0.1:{port}"))
            .replace("Shopper pays for the cart", "r6_literal_canary_585");
        std::fs::write(
            dir.path().join(".graphhelm/journeys/checkout.journey.yaml"),
            flow,
        )
        .unwrap();
        std::fs::write(
            dir.path().join(".graphhelm/observers/journey_driver.mjs"),
            include_bytes!("../../../tools/journey-driver/driver.mjs"),
        )
        .unwrap();
        std::fs::write(
            dir.path().join(".graphhelm/journey-fixture.json"),
            r#"{"schema":"graphhelm-journey-fixture/1","script":"fixture.sh"}"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("fixture.sh"),
            "if [ \"$1\" = up ]; then\nmkdir -p \"$2/.graphhelm\"\nprintf '%s\\n' 'PATH=not-a-path' 'NODE_OPTIONS=--invalid' 'GRAPHHELM_SECRET_R6_FIXTURE_TOKEN=r6_literal_canary_585' > \"$2/.graphhelm/secrets.env\"\necho ready > ready\nfi\n"
        ).unwrap();
        std::fs::create_dir_all(dir.path().join(".graphhelm/journey-previews/checkout")).unwrap();
        let ready = dir.path().join("ready");
        let stop = Arc::new(AtomicBool::new(false));
        let done = stop.clone();
        let server = std::thread::spawn(move || {
            let deadline = Instant::now() + scaled(Duration::from_secs(15));
            while !ready.exists() && !done.load(Ordering::Relaxed) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            if done.load(Ordering::Relaxed) || !ready.exists() {
                return;
            }
            let listener = std::net::TcpListener::bind(("127.0.0.1", port)).unwrap();
            listener.set_nonblocking(true).unwrap();
            while !done.load(Ordering::Relaxed) && Instant::now() < deadline {
                let _ = listener.accept();
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        let (_, reply) = cli(dir.path(), &args);
        stop.store(true, Ordering::Relaxed);
        server.join().unwrap();
        assert!(
            !reply.to_string().contains("r6_literal_canary_585"),
            "secret value leaked"
        );
        let reason = if args[0] == "watch" {
            reply["diagnostics"][0]["code"].clone()
        } else {
            let state: Value = serde_json::from_slice(
                &std::fs::read(
                    dir.path()
                        .join(".graphhelm/journey-previews/checkout/state.json"),
                )
                .unwrap(),
            )
            .unwrap();
            assert!(
                !state.to_string().contains("r6_literal_canary_585"),
                "secret persisted"
            );
            state["reason"].clone()
        };
        assert_eq!(reason, "driver.secret_literal");
    }
}

/// #490: a generated contract left over from an earlier compile and now stale (the flow changed
/// since) makes `validate` report `flow.contract_stale`, and must not stop `journey watch`, which
/// never reads it: the draft still passes every gate up to the observer. Cost: seconds, no browser.
#[test]
fn a_stale_generated_contract_does_not_stop_watch() {
    let dir = draft();
    let flow = dir.path().join(".graphhelm/journeys/checkout.journey.yaml");
    let text = std::fs::read_to_string(&flow).unwrap();
    let edited = text.replacen("title: ", "title: Edited ", 1);
    assert_ne!(text, edited, "ARRANGEMENT: the flow title was edited");
    std::fs::write(&flow, edited).unwrap();
    let (_, validated) = cli(dir.path(), &["validate", "--all"]);
    assert!(
        validated.to_string().contains("flow.contract_stale"),
        "ARRANGEMENT: the generated contract is now stale: {validated}"
    );
    refuses(
        dir.path(),
        &["watch", "checkout"],
        "journey.watch",
        3,
        "replay.observer_missing",
    );
}

/// #515: a DRAFT is written by agents, so watching it must not be able to pay or delete. The
/// watch names, before any browser starts, the acts it will not perform (`data.guarded`): the
/// fixture's `Pay now` submit, and not the `Checkout` click or the password field. An APPROVED
/// flow carries the owner's approval of its acts and is played whole (`guarded` empty). Credible
/// defect: the guard is dropped, or widened to approved flows, and nothing else goes red because
/// every other watch cell stops before the first act. Cost: seconds, offline, no browser (the
/// tripwire observer never starts).
#[test]
fn watch_names_the_destructive_acts_of_a_draft_it_will_not_perform() {
    let dir = draft();
    let reply = refuses(
        dir.path(),
        &["watch", "checkout"],
        "journey.watch",
        3,
        "replay.observer_missing",
    );
    assert_eq!(
        reply["data"]["guarded"],
        serde_json::json!([{"edge":"pay.submit","actIndex":1,"kind":"submit","role":"button","name":"Pay now","would":"pay"}]),
        "{reply}"
    );
    let dir = approved();
    let reply = refuses(
        dir.path(),
        &["watch", "checkout"],
        "journey.watch",
        3,
        "replay.observer_missing",
    );
    assert_eq!(reply["data"]["guarded"], serde_json::json!([]), "{reply}");
}

/// #518 (`keel.invariant.permissions`): a watch plays a guarded act of a DRAFT only on an edge
/// the owner marked safe, and only while the edge is the one that was marked. Defects named: the
/// watch ignoring the mark (the owner's word does nothing), and the watch honouring a mark the
/// edge outgrew - an agent rewrites the marked "Pay now" into "Delete account", or points the
/// flow at another app, and the act is played under the old mark. Cost: seconds, offline, no
/// browser (the tripwire observer never starts).
#[test]
fn watch_plays_a_guarded_act_only_under_a_mark_that_still_binds_its_edge() {
    let dir = draft();
    let path = dir.path().join(".graphhelm/journeys/checkout.journey.yaml");
    let guarded = |project: &Path| {
        refuses(
            project,
            &["watch", "checkout"],
            "journey.watch",
            3,
            "replay.observer_missing",
        )["data"]["guarded"]
            .clone()
    };
    assert_eq!(guarded(dir.path()).as_array().map(Vec::len), Some(1));

    let owner = owner_token(dir.path());
    let (code, reply) = cli(
        dir.path(),
        &[
            "mark-safe",
            "checkout",
            "pay.submit",
            "--token-file",
            &owner,
        ],
    );
    assert_eq!(code, 0, "{reply}");
    assert_eq!(guarded(dir.path()), serde_json::json!([]));

    // The marked act is rewritten under the mark: the mark is void and the act is guarded again.
    let marked = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        &path,
        marked.replace("name: Pay now", "name: Delete account"),
    )
    .unwrap();
    assert_eq!(
        guarded(dir.path()),
        serde_json::json!([{"edge":"pay.submit","actIndex":1,"kind":"submit","role":"button","name":"Delete account","would":"delete"}])
    );
    // The same acts on another app: void as well.
    std::fs::write(
        &path,
        marked.replace("base: http://localhost:3000", "base: http://localhost:9000"),
    )
    .unwrap();
    assert_eq!(guarded(dir.path()).as_array().map(Vec::len), Some(1));
    // Restored, the mark binds again.
    std::fs::write(&path, &marked).unwrap();
    assert_eq!(guarded(dir.path()), serde_json::json!([]));
}

/// #534: approving is the owner's; `graphhelm init` makes the project's owner store and token.
fn owner_token(project: &Path) -> String {
    // init ignores all of `.graphhelm/`; a real project keeps its flows tracked and its owner
    // store (events, token, keys, owner records) out of git, so the test ignores exactly that.
    let gitignore = project.join(".gitignore");
    let kept = std::fs::read(&gitignore).ok();
    let out = std::process::Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args(["--json", "init", "--project"])
        .arg(project)
        .args(["--harness", "claude-code"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let mut ignore = kept.unwrap_or_default();
    ignore.extend_from_slice(
        b"
.graphhelm/*
!.graphhelm/journeys/
/.mcp.json
",
    );
    std::fs::write(&gitignore, ignore).unwrap();
    project
        .join(".graphhelm/events.token")
        .to_string_lossy()
        .into_owned()
}

/// #534 slice 2: a watch plays a guarded act only under a mark the OWNER recorded. A marked YAML
/// copied into a project whose owner never marked it leaves the act guarded. Defect named: the
/// watch trusting a mark anyone could write into the file. Cost: seconds, offline, no browser.
#[test]
fn watch_guards_an_act_under_a_mark_the_owner_never_recorded() {
    let marked = draft();
    let owner = owner_token(marked.path());
    let (code, reply) = cli(
        marked.path(),
        &[
            "mark-safe",
            "checkout",
            "pay.submit",
            "--token-file",
            &owner,
        ],
    );
    assert_eq!(code, 0, "{reply}");
    let guarded = |project: &Path| {
        refuses(
            project,
            &["watch", "checkout"],
            "journey.watch",
            3,
            "replay.observer_missing",
        )["data"]["guarded"]
            .clone()
    };
    assert_eq!(guarded(marked.path()), serde_json::json!([]));
    let copied = draft();
    owner_token(copied.path());
    std::fs::copy(
        marked
            .path()
            .join(".graphhelm/journeys/checkout.journey.yaml"),
        copied
            .path()
            .join(".graphhelm/journeys/checkout.journey.yaml"),
    )
    .unwrap();
    assert_eq!(guarded(copied.path()).as_array().map(Vec::len), Some(1));
}
