//! #339: real offline CLI observer of flow schema, canonicalization, generated contracts and
//! approval. Credible regressions: silently ignored YAML, stale approvals and unsafe overwrites.
//! Existing journey tests accept JSON contracts only. Cost: subprocesses/tempdirs, no network,
//! browser, model, credentials or production seams; seconds after the build.
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

const EXAMPLE: &str = include_str!("fixtures/journey_flow/checkout.journey.yaml");

fn graphhelm() -> Command {
    let mut command = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"));
    command.env(
        "GRAPHHELM_EVENTS_KEY",
        "0101010101010101010101010101010101010101010101010101010101010101",
    );
    command
}

fn project(flow: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for file in [
        "app/cart/page.tsx",
        "app/checkout/page.tsx",
        "app/api/pay/route.ts",
    ] {
        let path = dir.path().join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "export {}").unwrap();
    }
    std::fs::create_dir_all(dir.path().join(".graphhelm/journeys")).unwrap();
    write_flow(dir.path(), flow);
    dir
}

fn write_flow(project: &Path, flow: &str) {
    std::fs::write(
        project.join(".graphhelm/journeys/checkout.journey.yaml"),
        flow,
    )
    .unwrap();
}

fn run(project: &Path, args: &[&str]) -> (Output, Value) {
    let out = graphhelm()
        .args(["--json", "journey"])
        .args(args)
        .arg("--project")
        .arg(project)
        .output()
        .unwrap();
    let json = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
    (out, json)
}

/// #534: approving is the owner's. `graphhelm init` makes the project's owner store and its owner
/// token (with the owner-only ACL); the token's path is what `approve --token-file` takes.
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

fn approve(project: &Path) -> (Output, Value) {
    let token = owner_token(project);
    run(project, &["approve", "checkout", "--token-file", &token])
}

fn finding(reply: &Value, code: &str) -> bool {
    reply["data"]["files"].as_array().is_some_and(|files| {
        files.iter().any(|file| {
            file["findings"]
                .as_array()
                .is_some_and(|findings| findings.iter().any(|f| f["code"] == code))
        })
    })
}

#[test]
fn spec_example_is_valid_and_unknown_keys_are_refused() {
    let dir = project(EXAMPLE);
    let (out, reply) = run(dir.path(), &["validate", "--all"]);
    assert_eq!(out.status.code(), Some(0), "{reply}");
    assert_eq!(reply["data"]["checked"], 1);
    for text in [
        format!("{EXAMPLE}extra: 1\n"),
        EXAMPLE.replace(
            concat!("  ", "  ", "url: /cart"),
            concat!("  ", "  ", "extra: 1\n", "  ", "  ", "url: /cart"),
        ),
        EXAMPLE.replace("kind: activate,", "extra: 1, kind: activate,"),
        EXAMPLE.replace("scope: unknown", "scope: []"),
    ] {
        write_flow(dir.path(), &text);
        let (out, reply) = run(dir.path(), &["validate", "--all"]);
        assert_eq!(out.status.code(), Some(2), "{reply}");
        assert!(finding(&reply, "flow.schema_invalid"), "{reply}");
    }
}

#[test]
fn semantic_sabotage_is_refused_at_the_boundary() {
    for (flow, code) in [
        (
            EXAMPLE.replace("id: checkout", "id: another"),
            "flow.id_mismatch",
        ),
        (EXAMPLE.replace("id: done", "id: cart"), "flow.duplicate_id"),
        (
            EXAMPLE.replace("to: pay", "to: ghost"),
            "flow.unknown_screen",
        ),
        (
            EXAMPLE.replace("[cart.checkout, pay.submit]", "[cart.checkout, ghost]"),
            "flow.unknown_edge",
        ),
        (
            EXAMPLE.replace("secret: shopper_password", "secret: ghost"),
            "flow.unknown_secret",
        ),
        (
            EXAMPLE.replace("from: pay", "from: cart"),
            "flow.path_disconnected",
        ),
        (
            EXAMPLE.replace("to: done", "to: cart"),
            "flow.path_revisits_screen",
        ),
        (
            EXAMPLE.replace("http://localhost:3000", "https://example.com"),
            "flow.base_not_local",
        ),
        (
            EXAMPLE.replace("app/cart/page.tsx", "app/missing.tsx"),
            "flow.scope_path_missing",
        ),
        (
            EXAMPLE.replace("app/cart/page.tsx", "app/../../outside.tsx"),
            "flow.scope_path_outside_project",
        ),
        (format!("{EXAMPLE}{}", " ".repeat(32768)), "flow.too_large"),
        (
            EXAMPLE.replace(
                "title: Shopper pays for the cart",
                "title: &label Shopper pays for the cart",
            ),
            "flow.not_yaml",
        ),
        (
            EXAMPLE.replace("id: checkout", "id: checkout\nid: checkout"),
            "flow.not_yaml",
        ),
    ] {
        let dir = project(&flow);
        let (out, reply) = run(dir.path(), &["validate", "--all"]);
        assert_eq!(out.status.code(), Some(2), "{code}: {reply}");
        assert!(finding(&reply, code), "{code}: {reply}");
        let pointer = match code {
            "flow.id_mismatch" => "/id",
            "flow.duplicate_id" => "/screens/1/id",
            "flow.unknown_screen" => "/edges/0/to",
            "flow.unknown_edge" => "/paths/main/1",
            "flow.unknown_secret" => "/edges/1/acts/0/secret",
            "flow.path_disconnected" | "flow.path_revisits_screen" => "/paths/main/1",
            "flow.base_not_local" => "/base",
            "flow.scope_path_missing" | "flow.scope_path_outside_project" => "/screens/0/scope/0",
            "flow.too_large" | "flow.not_yaml" => "",
            _ => unreachable!("every sabotage has an independently specified pointer"),
        };
        assert!(
            reply["data"]["files"][0]["findings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["code"] == code && f["pointer"] == pointer),
            "{reply}"
        );
        assert!(
            reply["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|d| d["code"] == "GHCLI034_JOURNEY_FLOW_INVALID")
        );
    }
}

#[test]
fn canonical_bytes_reject_style_order_crlf_and_missing_newline() {
    let (head, rest) = EXAMPLE.split_once("screens:\n").unwrap();
    let (screens, tail) = rest.split_once("edges:\n").unwrap();
    let mut screens = screens.split("  - id: ").skip(1).collect::<Vec<_>>();
    screens.reverse();
    let reordered = format!(
        "{head}screens:\n{}edges:\n{tail}",
        screens
            .iter()
            .map(|s| format!("  - id: {s}"))
            .collect::<String>()
    );
    for flow in [
        reordered,
        EXAMPLE.replace('\n', "\r\n"),
        EXAMPLE.trim_end().to_owned(),
        EXAMPLE.replace("actors: [shopper]", "actors:\n  - shopper"),
        EXAMPLE.replace(
            "schema: graphhelm.journey-flow/1\nid: checkout",
            "id: checkout\nschema: graphhelm.journey-flow/1",
        ),
    ] {
        let dir = project(&flow);
        let (out, reply) = run(dir.path(), &["validate", "--all"]);
        assert_eq!(out.status.code(), Some(2), "{reply}");
        assert!(finding(&reply, "flow.not_canonical"), "{reply}");
        let (out, reply) = run(dir.path(), &["compile", "--fmt", "--include-draft"]);
        assert_eq!(out.status.code(), Some(0), "{reply}");
        assert_eq!(
            std::fs::read_to_string(dir.path().join(".graphhelm/journeys/checkout.journey.yaml"))
                .unwrap(),
            EXAMPLE
        );
    }
}

#[test]
fn compile_formats_drafts_and_preserves_handwritten_contracts() {
    let dir = project(&EXAMPLE.replace('\n', "\r\n"));
    let (out, reply) = run(dir.path(), &["compile", "--fmt", "--include-draft"]);
    assert_eq!(out.status.code(), Some(0), "{reply}");
    assert_eq!(reply["data"]["written"], 2, "{reply}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join(".graphhelm/journeys/checkout.journey.yaml"))
            .unwrap(),
        EXAMPLE
    );
    let path = dir.path().join(".graphhelm/journeys/checkout.json");
    let first = std::fs::read(&path).unwrap();
    assert_eq!(
        first,
        include_str!("fixtures/journey_flow/checkout.json")
            .replace("\r\n", "\n")
            .as_bytes()
    );
    let (out, reply) = run(dir.path(), &["validate", "--all"]);
    assert_eq!(out.status.code(), Some(0), "{reply}");
    assert_eq!(reply["data"]["checked"], 2);
    let (out, reply) = run(dir.path(), &["compile", "--include-draft"]);
    assert_eq!(out.status.code(), Some(0), "{reply}");
    assert_eq!(std::fs::read(&path).unwrap(), first);
    std::fs::write(&path, b"{\"handwritten\":true}\n").unwrap();
    for args in [
        &["compile", "--check", "--include-draft"][..],
        &["compile", "--include-draft"][..],
        &["validate", "--all"][..],
    ] {
        let (out, reply) = run(dir.path(), args);
        assert_eq!(out.status.code(), Some(2), "{reply}");
        assert!(reply.to_string().contains("flow.contract_stale"), "{reply}");
        assert_eq!(std::fs::read(&path).unwrap(), b"{\"handwritten\":true}\n");
    }
    let (out, reply) = run(dir.path(), &["compile", "--include-draft", "--force"]);
    assert_eq!(out.status.code(), Some(0), "{reply}");
    assert_eq!(std::fs::read(&path).unwrap(), first);
    let draft = project(EXAMPLE);
    let (out, reply) = run(draft.path(), &["compile"]);
    assert_eq!(out.status.code(), Some(0), "{reply}");
    assert_eq!(reply["data"]["skipped"][0]["reason"], "draft");
    assert!(
        !draft
            .path()
            .join(".graphhelm/journeys/checkout.json")
            .exists()
    );
}

#[test]
fn approve_binds_the_projection_and_refuses_invalid_input_without_writes() {
    use sha2::{Digest, Sha256};
    let dir = project(EXAMPLE);
    for args in [
        vec!["init", "-q", "--object-format=sha1"],
        vec!["add", "-A"],
        vec![
            "-c",
            "user.name=Flow Test",
            "-c",
            "user.email=flow@example.test",
            "commit",
            "-q",
            "-m",
            "fixture",
        ],
    ] {
        let out = Command::new("git")
            .current_dir(dir.path())
            .args(args)
            .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00Z")
            .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00Z")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let (out, reply) = approve(dir.path());
    assert_eq!(out.status.code(), Some(0), "{reply}");
    let approved =
        std::fs::read_to_string(dir.path().join(".graphhelm/journeys/checkout.journey.yaml"))
            .unwrap();
    let value: Value = serde_yaml_ng::from_str(&approved).unwrap();
    assert_eq!(value["status"], "approved");
    assert_eq!(value["drift"], serde_json::json!([]));
    let projection = EXAMPLE
        .lines()
        .filter(|line| {
            !line.starts_with("status:")
                && !line.starts_with("approved:")
                && !line.starts_with("drift:")
        })
        .map(|line| format!("{line}\n"))
        .collect::<String>();
    assert_eq!(
        value["approved"]["digest"],
        format!(
            "sha256:{}",
            hex::encode(Sha256::digest(projection.as_bytes()))
        )
    );
    let head = Command::new("git")
        .current_dir(dir.path())
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    assert_eq!(
        value["approved"]["revision"],
        String::from_utf8(head.stdout).unwrap().trim()
    );
    let (out, reply) = run(dir.path(), &["validate", "--all"]);
    assert_eq!(out.status.code(), Some(0), "{reply}");
    // The real reader must consume the compiler's artifact, not just a schema proxy.
    let events = dir.path().join("runtime-data");
    let keyring = dir.path().join("keyring");
    std::fs::create_dir(&keyring).unwrap();
    graphhelm_sealed_key_provider::SealedKeyProvider::create(
        &keyring,
        "flow-test",
        graphhelm_events::SecretBytes::new(vec![1; 32]),
    )
    .unwrap();
    let fixtures = dir.path().join("node-fixtures.json");
    std::fs::write(&fixtures, br#"{"nodeOutcomes":{}}"#).unwrap();
    let graph = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/graphs/manual-override-deploy.yaml");
    let started = graphhelm()
        .env(
            "GRAPHHELM_EVENTS_KEY",
            "0101010101010101010101010101010101010101010101010101010101010101",
        )
        .args(["--json", "execution", "start", "--events"])
        .arg(&events)
        .args(["--execution", "flow-test", "--file"])
        .arg(graph)
        .arg("--fixtures")
        .arg(fixtures)
        .args(["--mode", "manual", "--held"])
        .output()
        .unwrap();
    assert!(
        started.status.success(),
        "{}",
        String::from_utf8_lossy(&started.stdout)
    );
    let mapped = graphhelm()
        .args(["--json", "journeys", "--events"])
        .arg(events)
        .args(["--execution", "flow-test", "--project"])
        .arg(dir.path())
        .arg("--keyring")
        .arg(keyring)
        .args(["--key-id", "flow-test"])
        .output()
        .unwrap();
    let map: Value = serde_json::from_slice(&mapped.stdout).unwrap();
    assert!(mapped.status.success(), "{map}");
    let checkout = map["data"]["journeys"]
        .as_array()
        .unwrap()
        .iter()
        .find(|j| j["contractId"] == "checkout")
        .expect("generated checkout is readable");
    assert_eq!(
        checkout["steps"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["stepId"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["cart", "pay", "done"]
    );
    write_flow(dir.path(), &approved.replace("name: Cart", "name: Basket"));
    let before = std::fs::read(dir.path().join(".graphhelm/journeys/checkout.json")).unwrap();
    let (out, reply) = run(dir.path(), &["validate", "--all"]);
    assert_eq!(out.status.code(), Some(2), "{reply}");
    assert!(finding(&reply, "flow.approval_stale"), "{reply}");
    let (out, reply) = run(dir.path(), &["compile"]);
    assert_eq!(out.status.code(), Some(2), "{reply}");
    assert_eq!(
        std::fs::read(dir.path().join(".graphhelm/journeys/checkout.json")).unwrap(),
        before
    );
    let invalid = approved.replace("app/cart/page.tsx", "app/missing.tsx");
    write_flow(dir.path(), &invalid);
    let (out, reply) = approve(dir.path());
    assert_eq!(out.status.code(), Some(2), "{reply}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join(".graphhelm/journeys/checkout.journey.yaml"))
            .unwrap(),
        invalid
    );
    assert_eq!(
        std::fs::read(dir.path().join(".graphhelm/journeys/checkout.json")).unwrap(),
        before
    );
    let non_git = project(EXAMPLE);
    let (out, reply) = approve(non_git.path());
    assert_eq!(out.status.code(), Some(3), "{reply}");
    assert_eq!(reply["diagnostics"][0]["code"], "GHCLI001_ARGUMENT_INVALID");
    // The source schema pins SHA-1 approvals even though capture records also accept SHA-256.
    for args in [
        vec!["init", "-q", "--object-format=sha256"],
        vec!["add", "."],
        vec![
            "-c",
            "user.name=Flow Test",
            "-c",
            "user.email=flow@example.test",
            "commit",
            "-q",
            "-m",
            "fixture",
        ],
    ] {
        let out = Command::new("git")
            .current_dir(non_git.path())
            .args(args)
            .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00Z")
            .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00Z")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let (out, reply) = approve(non_git.path());
    assert_eq!(out.status.code(), Some(3), "{reply}");
    assert_eq!(
        std::fs::read_to_string(
            non_git
                .path()
                .join(".graphhelm/journeys/checkout.journey.yaml")
        )
        .unwrap(),
        EXAMPLE
    );
    assert!(
        !non_git
            .path()
            .join(".graphhelm/journeys/checkout.json")
            .exists()
    );
}

#[test]
fn missing_screen_coverage_warns_and_composed_ids_stay_bounded() {
    let dir = project(&EXAMPLE.replace(
        "  main: [cart.checkout, pay.submit]",
        "  main: [cart.checkout]",
    ));
    let (out, reply) = run(dir.path(), &["validate", "--all"]);
    assert_eq!(out.status.code(), Some(0), "{reply}");
    assert!(finding(&reply, "flow.unreachable_screen"), "{reply}");
    assert_eq!(reply["diagnostics"][0]["severity"], "warning");
    let padding = " ".repeat(4);
    for (flow,code) in [
        (EXAMPLE.replace("id: cart","id: cart/escape"),"flow.schema_invalid"),
        (EXAMPLE.replace("id: cart.checkout","id: cart..checkout"),"flow.schema_invalid"),
        (EXAMPLE.replace("  main: [cart.checkout, pay.submit]",&format!("  main: [cart.checkout, pay.submit]\n  {}: [cart.checkout]","a".repeat(124))),"flow.composed_id_invalid"),
        (EXAMPLE.replace("status: draft","status: approved").replace("drift: []", &format!("drift:\n  - edge: cart.checkout\n{padding}act: 0\n{padding}code: drift.locator_missing\n{padding}seen: missing\n{padding}at: 0000000000000000000000000000000000000000")),"flow.approved_with_drift")
    ] {
        let dir=project(&flow);let (out,reply)=run(dir.path(), &["validate","--all"]);
        assert_eq!(out.status.code(),Some(2),"{reply}");assert!(finding(&reply,code),"{reply}");
    }
}

#[test]
fn branches_compile_and_output_conflicts_never_partially_publish() {
    let check_dir = project(EXAMPLE);
    let check_output = check_dir.path().join(".graphhelm/journeys/checkout.json");
    std::fs::create_dir(&check_output).unwrap();
    let (out, reply) = run(check_dir.path(), &["compile", "--check", "--include-draft"]);
    assert_eq!(out.status.code(), Some(2), "{reply}");
    assert!(finding(&reply, "flow.contract_stale"), "{reply}");
    assert!(check_output.is_dir());
    let flow_file = check_dir
        .path()
        .join(".graphhelm/journeys/checkout.journey.yaml");
    let (out, reply) = run(check_dir.path(), &["validate", flow_file.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(2), "{reply}");
    assert!(finding(&reply, "flow.contract_stale"), "{reply}");
    let flow = EXAMPLE.replace(
        "  main: [cart.checkout, pay.submit]",
        "  main: [cart.checkout, pay.submit]\n  guest: [cart.checkout]",
    );
    let dir = project(&flow);
    let output = dir.path().join(".graphhelm/journeys/checkout.json");
    std::fs::create_dir(&output).unwrap();
    let (out, reply) = run(dir.path(), &["compile", "--include-draft"]);
    assert!(!out.status.success(), "{reply}");
    assert!(
        !dir.path()
            .join(".graphhelm/journeys/checkout.guest.json")
            .exists(),
        "earlier output leaked from a failed batch"
    );
    std::fs::remove_dir(output).unwrap();
    let (out, reply) = run(dir.path(), &["compile", "--include-draft"]);
    assert_eq!(out.status.code(), Some(0), "{reply}");
    let branch: Value = serde_json::from_slice(
        &std::fs::read(dir.path().join(".graphhelm/journeys/checkout.guest.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(branch["contractId"], "checkout.guest");
    assert_eq!(branch["steps"].as_array().unwrap().len(), 2);
}

#[test]
fn unrepresentable_expectations_are_refused_without_weakening_promises() {
    let expectations = (0..7)
        .map(|_| format!("{{role: heading, name: {}}}", "a".repeat(230)))
        .chain(std::iter::once(
            "{role: button, name: LAST_OBLIGATION}".to_owned(),
        ))
        .collect::<Vec<_>>()
        .join(", ");
    let flow = EXAMPLE.replace(
        "[{role: heading, name: Cart}, {role: button, name: Checkout}]",
        &format!("[{expectations}]"),
    );
    let dir = project(&flow);
    for args in [
        vec!["compile", "--fmt", "--include-draft"],
        vec!["validate", "--all"],
    ] {
        let (out, reply) = run(dir.path(), &args);
        assert_eq!(out.status.code(), Some(2), "{reply}");
        assert!(finding(&reply, "flow.promise_too_long"), "{reply}");
        if args[0] == "compile" {
            assert_eq!(reply["data"]["written"], 0, "{reply}");
        }
        assert_eq!(
            reply["data"]["files"][0]["findings"][0]["pointer"],
            "/screens/0/expect"
        );
        assert!(
            !dir.path()
                .join(".graphhelm/journeys/checkout.json")
                .exists()
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join(".graphhelm/journeys/checkout.journey.yaml"))
                .unwrap(),
            flow
        );
    }
}

/// #354: the journey-map skill teaches agents to write this flow. Credible regression: the skill's
/// worked example drifts from the format (or from its fixture) and every agent copying it starts
/// from a file `validate` refuses. Cost: three subprocesses in one tempdir, seconds after the build.
#[test]
fn journey_map_worked_example_validates_and_compiles() {
    let fixture =
        include_str!("fixtures/journey_flow/first-login.journey.yaml").replace("\r\n", "\n");
    let skill =
        include_str!("../../../extensions/builtin/graphhelm-jpd/skills/journey-map/SKILL.md")
            .replace("\r\n", "\n");
    let start = skill
        .find("```yaml\nschema: graphhelm.journey-flow/1\n")
        .expect("journey-map carries a worked flow example")
        + "```yaml\n".len();
    let end = start + skill[start..].find("```").unwrap();
    assert_eq!(
        &skill[start..end],
        fixture,
        "skill example and fixture differ"
    );

    let dir = tempfile::tempdir().unwrap();
    for file in [
        "apps/web/app/dashboard/page.tsx",
        "apps/web/app/entrar/page.tsx",
        "apps/web/app/onboarding/page.tsx",
        "apps/web/src/screens/entrar-screen.tsx",
    ] {
        let path = dir.path().join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "export {}").unwrap();
    }
    let journeys = dir.path().join(".graphhelm/journeys");
    std::fs::create_dir_all(&journeys).unwrap();
    std::fs::write(journeys.join("first-login.journey.yaml"), &fixture).unwrap();

    let (out, reply) = run(dir.path(), &["validate", "--all"]);
    assert_eq!(out.status.code(), Some(0), "{reply}");
    let (out, reply) = run(dir.path(), &["compile", "--include-draft"]);
    assert_eq!(out.status.code(), Some(0), "{reply}");
    assert!(journeys.join("first-login.json").exists(), "{reply}");
    let (out, reply) = run(dir.path(), &["validate", "--all"]);
    assert_eq!(out.status.code(), Some(0), "{reply}");
    assert_eq!(reply["data"]["checked"], 2, "{reply}");
}

/// The fixture's last act line, without its indentation (the text before it keeps that).
const PAY_NOW: &str = "- {kind: submit, role: button, name: Pay now}\n";

/// That act line followed by the mark the canonical renderer writes under it.
fn marked_line(digest: &str) -> String {
    format!("{PAY_NOW}{}safe: {{digest: {digest}}}\n", " ".repeat(4))
}

/// #518 (`keel.invariant.permissions`, `.persistence`): the owner's safe mark on a draft edge.
/// Defects named: a mark that survives an edit of the acts it covered (an agent rewrites
/// "Pay now" into "Delete account" under the owner's mark); a mark that follows its edge to another
/// screen or URL; a mark that blesses an act the owner was not shown; a mark written on a non-canonical, unknown or approved target; a mark that
/// makes an approved flow's digest depend on it. Existing tests know no `safe` field. Cost: one
/// tempdir, a dozen subprocesses, one git commit; no network or browser.
#[test]
fn the_owner_marks_a_draft_edge_safe_and_editing_its_acts_voids_the_mark() {
    let dir = project(EXAMPLE);
    let path = dir.path().join(".graphhelm/journeys/checkout.journey.yaml");
    let text = || std::fs::read_to_string(&path).unwrap();
    let stale = |reply: &Value| finding(reply, "flow.safe_stale");

    // An unknown edge and a non-canonical flow are refused, and nothing is written.
    let (out, reply) = run(dir.path(), &["mark-safe", "checkout", "pay.nope"]);
    assert_eq!(out.status.code(), Some(2), "{reply}");
    assert!(finding(&reply, "flow.edge_unknown"), "{reply}");
    write_flow(dir.path(), &EXAMPLE.replace("drift: []\n", "drift: []\n\n"));
    let (out, reply) = run(dir.path(), &["mark-safe", "checkout", "pay.submit"]);
    assert_eq!(out.status.code(), Some(2), "{reply}");
    write_flow(dir.path(), EXAMPLE);

    // The mark names every act it covers and lands on that edge only, in canonical bytes.
    let (out, reply) = run(dir.path(), &["mark-safe", "checkout", "pay.submit"]);
    assert_eq!(out.status.code(), Some(0), "{reply}");
    assert_eq!(
        reply["data"]["acts"].as_array().map(Vec::len),
        Some(2),
        "{reply}"
    );
    let digest = reply["data"]["safe"]["digest"].as_str().unwrap().to_owned();
    assert!(
        digest.starts_with("sha256:") && digest.len() == 71,
        "{digest}"
    );
    let marked = text();
    assert_eq!(marked, EXAMPLE.replace(PAY_NOW, &marked_line(&digest),));
    let (out, reply) = run(dir.path(), &["validate", "--all"]);
    assert_eq!(out.status.code(), Some(0), "{reply}");
    assert!(!stale(&reply), "{reply}");

    // Editing a covered act voids the mark: validate says so as a warning, never an error.
    write_flow(
        dir.path(),
        &marked.replace("name: Pay now", "name: Delete account"),
    );
    let (out, reply) = run(dir.path(), &["validate", "--all"]);
    assert_eq!(out.status.code(), Some(0), "{reply}");
    assert!(stale(&reply), "{reply}");
    // So does adding an act beside the covered ones.
    write_flow(
        dir.path(),
        &marked.replace(
            PAY_NOW,
            &format!(
                "{PAY_NOW}{}- {{kind: activate, role: button, name: Delete account}}\n",
                " ".repeat(6)
            ),
        ),
    );
    assert!(stale(&run(dir.path(), &["validate", "--all"]).1));
    // So does playing the same acts somewhere else: the screen the edge leaves now opens another
    // URL, the flow now opens another app, or the edge now reaches another screen. "Pay now" marked safe on /checkout is not
    // "Pay now" on /account.
    for moved in [
        marked.replace("url: /checkout", "url: /account"),
        // Another app on this machine: the mark was given for the one at port 3000.
        marked.replace("base: http://localhost:3000", "base: http://localhost:9000"),
        marked.replace("to: done", "to: cart"),
    ] {
        assert_ne!(moved, marked);
        write_flow(dir.path(), &moved);
        let (_, reply) = run(dir.path(), &["validate", "--all"]);
        assert!(stale(&reply), "{reply}");
    }
    write_flow(
        dir.path(),
        &marked.replace(
            PAY_NOW,
            &format!(
                "{PAY_NOW}{}- {{kind: activate, role: button, name: Delete account}}\n",
                " ".repeat(6)
            ),
        ),
    );
    // Marking again binds the edge as it is now.
    let (out, reply) = run(dir.path(), &["mark-safe", "checkout", "pay.submit"]);
    assert_eq!(out.status.code(), Some(0), "{reply}");
    assert_ne!(reply["data"]["safe"]["digest"], digest.as_str(), "{reply}");
    assert_eq!(
        reply["data"]["acts"].as_array().map(Vec::len),
        Some(3),
        "{reply}"
    );
    assert!(!stale(&run(dir.path(), &["validate", "--all"]).1));

    // Approve drops the mark, so the approval binds no mark and is not stale.
    write_flow(dir.path(), &marked);
    for args in [
        vec!["init", "-q", "--object-format=sha1"],
        vec!["add", "-A"],
        vec![
            "-c",
            "user.name=Flow Test",
            "-c",
            "user.email=flow@example.test",
            "commit",
            "-q",
            "-m",
            "fixture",
        ],
    ] {
        let out = Command::new("git")
            .current_dir(dir.path())
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let (out, reply) = approve(dir.path());
    assert_eq!(out.status.code(), Some(0), "{reply}");
    let approved = text();
    assert!(!approved.contains("safe:"), "{approved}");
    let (out, reply) = run(dir.path(), &["validate", "--all"]);
    assert_eq!(out.status.code(), Some(0), "{reply}");

    // An approved flow takes no mark, by the door or by hand.
    let (out, reply) = run(dir.path(), &["mark-safe", "checkout", "pay.submit"]);
    assert_eq!(out.status.code(), Some(2), "{reply}");
    assert!(finding(&reply, "flow.safe_not_draft"), "{reply}");
    assert_eq!(text(), approved);
    write_flow(
        dir.path(),
        &approved.replace(PAY_NOW, &marked_line(&digest)),
    );
    let (out, reply) = run(dir.path(), &["validate", "--all"]);
    assert_eq!(out.status.code(), Some(2), "{reply}");
    assert!(finding(&reply, "flow.safe_not_draft"), "{reply}");
}

/// #534 (found on #518): an approval used to be only YAML, `approved: {revision, digest}`, and the
/// digest is a plain sha256 anyone can compute. Approving now needs the owner's token and records
/// an owner-only signal naming the flow and digest; validate (and replay) trust a YAML approval
/// only with that record. Credible regressions: an approval with no credential, a copied or
/// hand-written approval counted as the owner's, a missing store read as "approved".
fn committed(flow: &str) -> tempfile::TempDir {
    let dir = project(flow);
    for args in [
        vec!["init", "-q", "--object-format=sha1"],
        vec!["add", "-A"],
        vec![
            "-c",
            "user.name=Flow Test",
            "-c",
            "user.email=flow@example.test",
            "commit",
            "-q",
            "-m",
            "fixture",
        ],
    ] {
        let out = Command::new("git")
            .current_dir(dir.path())
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    dir
}

fn validate(project: &Path) -> Value {
    run(project, &["validate", "--all"]).1
}

#[test]
fn approve_without_the_owner_token_is_refused_and_writes_nothing() {
    let dir = committed(EXAMPLE);
    owner_token(dir.path());
    let before =
        std::fs::read(dir.path().join(".graphhelm/journeys/checkout.journey.yaml")).unwrap();
    let (out, reply) = run(dir.path(), &["approve", "checkout"]);
    assert_ne!(out.status.code(), Some(0), "{reply}");
    let other = tempfile::tempdir().unwrap();
    let wrong = owner_token(other.path());
    let (out, reply) = run(dir.path(), &["approve", "checkout", "--token-file", &wrong]);
    assert_ne!(
        out.status.code(),
        Some(0),
        "a token from another store is not this owner's: {reply}"
    );
    assert_eq!(
        std::fs::read(dir.path().join(".graphhelm/journeys/checkout.journey.yaml")).unwrap(),
        before
    );
}

#[test]
fn the_owner_approval_is_recorded_and_a_copied_approval_is_unsigned() {
    let dir = committed(EXAMPLE);
    let (out, reply) = approve(dir.path());
    assert_eq!(out.status.code(), Some(0), "{reply}");
    let signed = validate(dir.path());
    assert!(
        !finding(&signed, "flow.approval_unsigned")
            && !finding(&signed, "flow.approval_unverifiable"),
        "{signed}"
    );
    // The same approved YAML (and contracts) in another project whose owner never approved it.
    let other = committed(EXAMPLE);
    owner_token(other.path());
    for name in ["checkout.journey.yaml", "checkout.json"] {
        std::fs::copy(
            dir.path().join(".graphhelm/journeys").join(name),
            other.path().join(".graphhelm/journeys").join(name),
        )
        .unwrap();
    }
    let copied = validate(other.path());
    assert!(finding(&copied, "flow.approval_unsigned"), "{copied}");
}

#[test]
fn an_approval_with_no_owner_store_cannot_be_verified() {
    let dir = committed(EXAMPLE);
    approve(dir.path());
    std::fs::remove_dir_all(dir.path().join(".graphhelm/events")).unwrap();
    assert!(finding(&validate(dir.path()), "flow.approval_unverifiable"));
}

/// #534 slice 3: approvals made before the owner's record existed read as unsigned on the owner's
/// machine. `sign-legacy` lists them with the commit that introduced each (writing nothing) and
/// signs one flow per `--id`; there is no bulk form, because a lane can write an `approved` block
/// that looks exactly like a legacy one (#597 review). An approval no commit introduced, or a flow
/// edited after its approval, is not signable. Credible defects: signing without the owner, signing
/// a forged or edited approval, a dry run that writes. Cost: tempdirs and git, seconds.
fn legacy() -> (tempfile::TempDir, String) {
    let source = committed(EXAMPLE);
    let (out, reply) = approve(source.path());
    assert_eq!(out.status.code(), Some(0), "{reply}");
    let dir = committed(EXAMPLE);
    let token = owner_token(dir.path());
    for name in ["checkout.journey.yaml", "checkout.json"] {
        std::fs::copy(
            source.path().join(".graphhelm/journeys").join(name),
            dir.path().join(".graphhelm/journeys").join(name),
        )
        .unwrap();
    }
    // The legacy approval was committed, as an approval made before #534 would have been.
    for args in [
        vec!["add", ".graphhelm/journeys"],
        vec![
            "-c",
            "user.name=Owner",
            "-c",
            "user.email=owner@example.test",
            "commit",
            "-q",
            "-m",
            "approve checkout",
        ],
    ] {
        let out = Command::new("git")
            .current_dir(dir.path())
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    assert!(finding(&validate(dir.path()), "flow.approval_unsigned"));
    (dir, token)
}

#[test]
fn sign_legacy_lists_with_the_introducing_commit_and_signs_only_named_flows_with_the_owner() {
    let (dir, token) = legacy();
    let (out, refused) = run(dir.path(), &["sign-legacy", "--id", "checkout"]);
    assert_ne!(out.status.code(), Some(0), "{refused}");
    let (out, bulk) = run(
        dir.path(),
        &["sign-legacy", "--token-file", &token, "--all"],
    );
    assert_ne!(out.status.code(), Some(0), "there is no bulk form: {bulk}");
    let (out, listed) = run(dir.path(), &["sign-legacy", "--token-file", &token]);
    assert_eq!(out.status.code(), Some(0), "{listed}");
    assert_eq!(listed["data"]["unsigned"][0]["id"], "checkout", "{listed}");
    assert_eq!(
        listed["data"]["unsigned"][0]["introducedBy"]["subject"], "approve checkout",
        "{listed}"
    );
    assert_eq!(listed["data"]["signed"], serde_json::json!([]), "{listed}");
    assert!(
        finding(&validate(dir.path()), "flow.approval_unsigned"),
        "a list signed"
    );
    let (out, signed) = run(
        dir.path(),
        &["sign-legacy", "--token-file", &token, "--id", "checkout"],
    );
    assert_eq!(out.status.code(), Some(0), "{signed}");
    assert_eq!(
        signed["data"]["signed"],
        serde_json::json!(["checkout"]),
        "{signed}"
    );
    assert!(!finding(&validate(dir.path()), "flow.approval_unsigned"));
    let (_, again) = run(dir.path(), &["sign-legacy", "--token-file", &token]);
    assert_eq!(again["data"]["unsigned"], serde_json::json!([]), "{again}");
}

#[test]
fn sign_legacy_never_signs_an_approval_no_commit_introduced() {
    // A lane writes an approved block into the working tree after #534: never committed.
    let source = committed(EXAMPLE);
    approve(source.path());
    let dir = committed(EXAMPLE);
    let token = owner_token(dir.path());
    for name in ["checkout.journey.yaml", "checkout.json"] {
        std::fs::copy(
            source.path().join(".graphhelm/journeys").join(name),
            dir.path().join(".graphhelm/journeys").join(name),
        )
        .unwrap();
    }
    let (out, reply) = run(
        dir.path(),
        &["sign-legacy", "--token-file", &token, "--id", "checkout"],
    );
    assert_eq!(out.status.code(), Some(0), "{reply}");
    assert_eq!(reply["data"]["signed"], serde_json::json!([]), "{reply}");
    assert_eq!(reply["data"]["notSignable"][0]["id"], "checkout", "{reply}");
    assert!(
        reply["data"]["notSignable"][0]["findings"]
            .to_string()
            .contains("flow.approval_uncommitted"),
        "{reply}"
    );
    assert!(finding(&validate(dir.path()), "flow.approval_unsigned"));
}

#[test]
fn sign_legacy_never_signs_a_flow_edited_after_its_approval() {
    let (dir, token) = legacy();
    let path = dir.path().join(".graphhelm/journeys/checkout.journey.yaml");
    let edited = std::fs::read_to_string(&path)
        .unwrap()
        .replace("title: ", "title: Edited ");
    std::fs::write(&path, edited).unwrap();
    let (out, reply) = run(
        dir.path(),
        &["sign-legacy", "--token-file", &token, "--id", "checkout"],
    );
    assert_eq!(out.status.code(), Some(0), "{reply}");
    assert_eq!(reply["data"]["signed"], serde_json::json!([]), "{reply}");
    assert_eq!(reply["data"]["notSignable"][0]["id"], "checkout", "{reply}");
}
