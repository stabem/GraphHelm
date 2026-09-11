//! `graphhelm graph synthesize` — the road from a goal to a document that starts and completes
//! (spec §4, §5 "CLI" cells). The model in every test is the recorded door (`--fixture`): no
//! network, no credentials, no clock.

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use serde_json::Value;

const REFUSED_CODE: &str = "GHCLI026_ARCHITECT_REFUSED";
const ARGUMENT_CODE: &str = "GHCLI001_ARGUMENT_INVALID";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn command() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
}

fn fixtures() -> PathBuf {
    root().join("core/architect/fixtures")
}

/// The goal the first-compile fixture was recorded with: read from the one file the crate test
/// reads too, so exactly one copy of the string exists.
fn first_compile_goal() -> String {
    std::fs::read_to_string(fixtures().join("first-compile/GOAL.txt"))
        .unwrap()
        .trim_end()
        .to_owned()
}

fn envelope(args: &[&str]) -> Value {
    let output = command().args(args).output().unwrap();
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{error}: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn write_json(directory: &Path, name: &str, value: &Value) -> PathBuf {
    let path = directory.join(name);
    std::fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
    path
}

fn synthesize(goal: &str, out: &Path, fixture: &Path) -> Value {
    envelope(&[
        "graph",
        "synthesize",
        "--goal",
        goal,
        "--out",
        out.to_str().unwrap(),
        "--allow-program",
        "cargo",
        "--fixture",
        fixture.to_str().unwrap(),
    ])
}

/// THE FIRST COMPILE (PRD §25 step 5, made falsifiable): one command, keyless, produces a
/// document that lints clean, starts, and completes.
#[test]
fn a_goal_becomes_a_document_that_starts_and_completes() {
    let directory = tempfile::tempdir().unwrap();
    let out = directory.path().join("first-compile.json");
    let fixture = fixtures().join("first-compile/replies.json");
    let value = synthesize(&first_compile_goal(), &out, &fixture);
    assert_eq!(value["ok"], true, "{value}");
    assert_eq!(value["command"], "graph.synthesize");
    assert_eq!(
        value["data"]["stampedCustoms"],
        serde_json::json!(["build_check", "summarize"])
    );
    assert_eq!(value["data"]["rounds"], 1);
    assert_eq!(value["data"]["out"], out.to_str().unwrap());
    assert_eq!(value["data"]["promptSha256s"].as_array().unwrap().len(), 1);
    assert!(value["data"]["templateSha256"].as_str().unwrap().len() == 64);
    assert!(
        value["data"].get("usage").is_none(),
        "a recording reports no usage"
    );

    // The file IS the document the reply carries, pretty-printed with a trailing newline.
    let written = std::fs::read(&out).unwrap();
    let mut expected = serde_json::to_vec_pretty(&value["data"]["document"]).unwrap();
    expected.push(b'\n');
    assert_eq!(written, expected);

    // lint: zero GHG102 — the #183 property, measured on the file the operator would use
    let lint = envelope(&["graph", "lint", out.to_str().unwrap()]);
    assert_eq!(lint["ok"], true, "{lint}");
    assert!(!lint.to_string().contains("GHG102"), "{lint}");

    // execute it: the same road every authored graph takes
    let events = directory.path().join("events");
    let fixtures = write_json(
        directory.path(),
        "fixtures.json",
        &serde_json::json!({"nodeOutcomes": {"build_check": "success", "summarize": "success"}}),
    );
    let started = envelope(&[
        "execution",
        "start",
        "--file",
        out.to_str().unwrap(),
        "--events",
        events.to_str().unwrap(),
        "--fixtures",
        fixtures.to_str().unwrap(),
        "--mode",
        "supervised",
        "--execution",
        "exec-first-compile",
    ]);
    assert_eq!(started["ok"], true, "{started}");
    assert_eq!(started["data"]["status"], "completed", "{started}");
}

#[test]
fn the_document_is_byte_identical_across_two_runs() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = fixtures().join("first-compile/replies.json");
    let goal = first_compile_goal();
    let first = directory.path().join("one.json");
    let second = directory.path().join("two.json");
    let one = synthesize(&goal, &first, &fixture);
    let two = synthesize(&goal, &second, &fixture);
    assert_eq!(one["ok"], true, "{one}");
    assert_eq!(two["ok"], true, "{two}");
    assert_eq!(
        std::fs::read(&first).unwrap(),
        std::fs::read(&second).unwrap(),
        "two runs, one document"
    );
    assert_eq!(one["data"]["document"], two["data"]["document"]);
}

#[test]
fn a_goal_needing_a_program_outside_the_allowlist_is_refused_and_names_the_program() {
    let directory = tempfile::tempdir().unwrap();
    let out = directory.path().join("refused.json");
    let fixture = fixtures().join("sabotage/program-outside-catalog.json");
    let value = synthesize(&first_compile_goal(), &out, &fixture);
    assert_eq!(value["ok"], false, "{value}");
    assert_eq!(value["command"], "graph.synthesize");
    let diagnostics = value["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics.len(), 1, "{value}");
    assert_eq!(diagnostics[0]["code"], REFUSED_CODE);
    assert_eq!(diagnostics[0]["path"], "/goal");
    let message = diagnostics[0]["message"].as_str().unwrap();
    let refusal: Value = serde_json::from_str(message).unwrap_or_else(|error| {
        panic!("the message is the refusal as compact JSON: {error}: {message}")
    });
    assert_eq!(refusal["kind"], "capabilityMissing");
    assert_eq!(refusal["node"], "build_check");
    assert_eq!(refusal["program"], "python");
    assert!(!out.exists(), "a refusal writes nothing");
}

#[test]
fn an_existing_out_path_is_never_overwritten() {
    let directory = tempfile::tempdir().unwrap();
    let out = directory.path().join("taken.json");
    std::fs::write(&out, b"precious\n").unwrap();
    let fixture = fixtures().join("first-compile/replies.json");
    let value = synthesize(&first_compile_goal(), &out, &fixture);
    assert_eq!(value["ok"], false, "{value}");
    assert_eq!(value["diagnostics"][0]["code"], ARGUMENT_CODE);
    assert_eq!(value["diagnostics"][0]["path"], "/out");
    assert_eq!(std::fs::read(&out).unwrap(), b"precious\n");
}

#[test]
fn a_fixture_without_the_prompt_prints_the_hash_to_record() {
    let directory = tempfile::tempdir().unwrap();
    let out = directory.path().join("unrecorded.json");
    let empty = write_json(
        directory.path(),
        "empty.json",
        &serde_json::json!({"replies": {}}),
    );
    let value = synthesize(&first_compile_goal(), &out, &empty);
    assert_eq!(value["ok"], false, "{value}");
    assert_eq!(value["diagnostics"][0]["code"], REFUSED_CODE);
    let message = value["diagnostics"][0]["message"].as_str().unwrap();
    let refusal: Value = serde_json::from_str(message).unwrap();
    assert_eq!(refusal["kind"], "fixtureMissing");
    // camelCase on the wire, like every other key of the envelope (`rename_all_fields`).
    let hash = refusal["promptSha256"].as_str().unwrap();
    assert!(
        refusal.get("prompt_sha256").is_none(),
        "the snake_case spelling must not survive: {refusal}"
    );
    assert_eq!(hash.len(), 64, "{hash}");
    assert!(hash.bytes().all(|byte| byte.is_ascii_hexdigit()), "{hash}");
    assert!(!out.exists());
}
