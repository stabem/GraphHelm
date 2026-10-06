//! #339: real offline CLI observer of flow schema, canonicalization, generated contracts and
//! approval. Credible regressions: silently ignored YAML, stale approvals and unsafe overwrites.
//! Existing journey tests accept JSON contracts only. Cost: subprocesses/tempdirs, no network,
//! browser, model, credentials or production seams; seconds after the build.
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

const EXAMPLE: &str = include_str!("fixtures/journey_flow/checkout.journey.yaml");

fn graphhelm() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
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
        EXAMPLE.replace("    url: /cart", "    extra: 1\n    url: /cart"),
        EXAMPLE.replace("kind: activate,", "extra: 1, kind: activate,"),
        EXAMPLE.replace("scope: unknown", "scope: []"),
    ] {
        write_flow(dir.path(), &text);
        let (out, reply) = run(dir.path(), &["validate", "--all"]);
        assert_eq!(out.status.code(), Some(2), "{reply}");
        assert!(finding(&reply, "flow.schema_invalid"), "{reply}");
    }
}
