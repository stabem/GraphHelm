use std::path::{Path, PathBuf};

use assert_cmd::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn command() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
}

#[test]
fn legacy_graph_parse_errors_help_and_version_keep_clap_behavior() {
    let invalid = command().args(["graph", "validate"]).output().unwrap();
    assert_eq!(invalid.status.code(), Some(2));
    assert!(invalid.stdout.is_empty());
    assert!(!invalid.stderr.is_empty());

    for arguments in [vec!["--help"], vec!["--version"]] {
        let output = command().args(arguments).output().unwrap();
        assert!(output.status.success());
        assert!(!output.stdout.is_empty());
        assert!(output.stderr.is_empty());
    }
}

fn json(output: &[u8]) -> serde_json::Value {
    serde_json::from_slice(output).unwrap()
}

#[test]
fn validate_returns_one_json_document_and_zero_for_canonical_yaml() {
    let output = command()
        .args([
            "graph",
            "validate",
            root()
                .join("examples/graphs/software-feature.yaml")
                .to_str()
                .unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value = json(&output.stdout);
    assert_eq!(value["ok"], true);
    assert_eq!(value["command"], "graph.validate");
}

#[test]
fn lint_failure_is_json_and_exit_two() {
    let output = command()
        .args([
            "graph",
            "lint",
            root()
                .join("tests/fixtures/invalid/entrypoint.yaml")
                .to_str()
                .unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let value = json(&output.stdout);
    assert_eq!(value["diagnostics"][0]["code"], "GHG001_ENTRYPOINT_UNKNOWN");
    let source = value["diagnostics"][0]["source"].as_str().unwrap();
    assert!(
        !source.contains(root().to_string_lossy().as_ref()),
        "#592: error-path diagnostic must not carry the local repo prefix: {source}"
    );
}

#[test]
fn json_graph_hash_matches_yaml_hash() {
    let directory = tempfile::tempdir().unwrap();
    let yaml_path = root().join("examples/graphs/software-feature.yaml");
    let yaml: serde_json::Value =
        serde_yaml_ng::from_str(&std::fs::read_to_string(&yaml_path).unwrap()).unwrap();
    let json_path = directory.path().join("graph.json");
    std::fs::write(&json_path, serde_json::to_vec_pretty(&yaml).unwrap()).unwrap();
    let yaml_output = command()
        .args(["graph", "hash", yaml_path.to_str().unwrap()])
        .output()
        .unwrap();
    let json_output = command()
        .args(["graph", "hash", json_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(yaml_output.status.success());
    assert!(json_output.status.success());
    assert_eq!(
        json(&yaml_output.stdout)["data"]["hash"],
        json(&json_output.stdout)["data"]["hash"]
    );
}

#[test]
fn secret_values_are_never_echoed_or_persisted_by_cli_preflight() {
    let directory = tempfile::tempdir().unwrap();
    let mut graph =
        graphhelm_schema::load_graph(&root().join("examples/graphs/software-feature.yaml"))
            .unwrap()
            .graph;
    graph
        .spec
        .nodes
        .get_mut("implement")
        .unwrap()
        .properties
        .insert("apiKey".into(), serde_json::json!("TOP-SECRET-DO-NOT-ECHO"));
    let graph_path = directory.path().join("secret.json");
    std::fs::write(&graph_path, serde_json::to_vec(&graph).unwrap()).unwrap();

    for subcommand in ["validate", "hash"] {
        let output = command()
            .args(["graph", subcommand, graph_path.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(!String::from_utf8_lossy(&output.stdout).contains("TOP-SECRET"));
    }

    let events = directory.path().join("events.jsonl");
    let output = command()
        .args([
            "graph",
            "simulate",
            graph_path.to_str().unwrap(),
            "--events",
            events.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        json(&output.stdout)["diagnostics"][0]["code"],
        "GHG008_INLINE_SECRET"
    );
    assert!(!events.exists());
}

#[test]
fn invalid_schema_payload_is_redacted_from_stdout_and_stderr() {
    let directory = tempfile::tempdir().unwrap();
    let graph = graphhelm_schema::load_graph(&root().join("examples/graphs/software-feature.yaml"))
        .unwrap()
        .graph;
    let mut document = serde_json::to_value(graph).unwrap();
    document["metadata"]["version"] = serde_json::json!("TOP-SECRET-DO-NOT-ECHO");
    let graph_path = directory.path().join("invalid-secret.json");
    std::fs::write(&graph_path, serde_json::to_vec(&document).unwrap()).unwrap();

    let output = command()
        .args(["graph", "validate", graph_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("TOP-SECRET"));
    assert_eq!(
        json(&output.stdout)["diagnostics"][0]["code"],
        "GHS002_SCHEMA"
    );
}

#[test]
fn simulate_then_fresh_process_replay_reconstructs_terminal_state() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events.jsonl");
    let graph = root().join("examples/graphs/software-feature.yaml");
    let simulated = command()
        .args([
            "graph",
            "simulate",
            graph.to_str().unwrap(),
            "--events",
            events.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        simulated.status.success(),
        "{}",
        String::from_utf8_lossy(&simulated.stderr)
    );
    assert_eq!(json(&simulated.stdout)["data"]["status"], "completed");

    let replayed = command()
        .args(["graph", "replay", "--events", events.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        replayed.status.success(),
        "{}",
        String::from_utf8_lossy(&replayed.stderr)
    );
    let projection = json(&replayed.stdout);
    assert_eq!(projection["data"]["simulationStatus"], "completed");
    assert!(projection["data"]["currentGraph"].is_null());
}

/// #192: a warning-only lint pass (no errors) was computed and then silently dropped on the
/// success path — `diagnostics.extend(report.warnings)` lived only inside the `!errors.is_empty()`
/// branch, so a graph that lints clean except for warnings reached `ok: true` with an empty
/// `diagnostics` array, even though the lint pass had genuinely found something.
///
/// `manual-override-deploy.yaml` is the fixture, not one invented for this test: `deploy` and
/// `implementation` both lack `timeoutSeconds` (GHG101) and declare no customs budget while able
/// to park (GHG102) — `graph lint` against it reports zero errors and four warnings, confirmed by
/// running it directly before writing this assertion.
#[test]
fn a_warning_only_lint_pass_reaches_the_caller_on_a_successful_simulate() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events.jsonl");
    let graph = root().join("examples/graphs/manual-override-deploy.yaml");
    let simulated = command()
        .args([
            "graph",
            "simulate",
            graph.to_str().unwrap(),
            "--events",
            events.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        simulated.status.success(),
        "{}",
        String::from_utf8_lossy(&simulated.stderr)
    );
    let reply = json(&simulated.stdout);
    assert_eq!(reply["data"]["status"], "completed");
    let diagnostics = reply["diagnostics"].as_array().expect("diagnostics array");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic["code"] == "GHG101_DEFAULT_TIMEOUT"),
        "a clean-but-warned lint pass must still reach the caller on success: {reply}"
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic["severity"] == "warning"),
        "no error exists in this fixture's lint pass, so nothing here should be one: {reply}"
    );
    let root_prefix = root().to_string_lossy().into_owned();
    assert!(
        diagnostics.iter().all(|diagnostic| {
            !diagnostic["source"]
                .as_str()
                .unwrap()
                .contains(&root_prefix)
        }),
        "#592: a warning that now reaches the caller (per #575) must not carry the local repo prefix: {reply}"
    );
}

#[test]
fn replay_requires_explicit_scope_and_stream_when_repository_has_multiple_streams() {
    let directory = tempfile::tempdir().unwrap();
    let repository = directory.path().join("repository-v1");
    for graph in ["software-feature.yaml", "research-to-publish.yaml"] {
        let output = command()
            .args([
                "graph",
                "simulate",
                root().join("examples/graphs").join(graph).to_str().unwrap(),
                "--events",
                repository.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{graph}: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    }

    let ambiguous = command()
        .args(["graph", "replay", "--events", repository.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(ambiguous.status.code(), Some(3));
    assert_eq!(
        json(&ambiguous.stdout)["diagnostics"][0]["code"],
        "GHE010_STREAM_SELECTION_REQUIRED"
    );

    let selected = command()
        .args([
            "graph",
            "replay",
            "--events",
            repository.to_str().unwrap(),
            "--workspace",
            "workspace-local",
            "--project",
            "project-local",
            "--execution",
            "exec_feature",
            "--stream",
            "exec_feature",
        ])
        .output()
        .unwrap();
    assert!(
        selected.status.success(),
        "{}",
        String::from_utf8_lossy(&selected.stdout)
    );
    assert_eq!(json(&selected.stdout)["data"]["streamId"], "exec_feature");
}

#[test]
fn draft_apply_fails_closed_without_external_key_provider_and_does_not_mutate() {
    let directory = tempfile::tempdir().unwrap();
    let base = root().join("examples/graphs/software-feature.yaml");
    let loaded = graphhelm_schema::load_graph(&base).unwrap();
    let draft = graphhelm_protocols::GraphDraft {
        id: "draft-1".into(),
        expected_version: loaded.graph.metadata.version,
        expected_hash: graphhelm_graph::semantic_hash(&loaded.graph).unwrap(),
        operations: vec![graphhelm_protocols::DraftOperation::RemoveNode { id: "docs".into() }],
        manual_override: None,
    };
    let draft_path = directory.path().join("draft.json");
    std::fs::write(&draft_path, serde_json::to_vec(&draft).unwrap()).unwrap();
    let repository = directory.path().join("repository-v1");

    let output = command()
        .args([
            "graph",
            "draft",
            "apply",
            base.to_str().unwrap(),
            draft_path.to_str().unwrap(),
            "--actor",
            "owner-local",
            "--events",
            repository.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
    let diagnostics = json(&output.stdout)["diagnostics"]
        .as_array()
        .expect("diagnostics array")
        .clone();
    assert_eq!(diagnostics[0]["code"], "GHK001_KEY_UNAVAILABLE");
    // #192: software-feature.yaml lints clean (zero errors) but with warnings (GHG101/GHG102 on
    // several nodes) — this command is fail-closed by design and never reaches a real success,
    // but those warnings were genuinely computed and must still reach the caller, appended after
    // the key-unavailable diagnostic rather than lost because the eventual failure is unrelated.
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic["code"] == "GHG101_DEFAULT_TIMEOUT"),
        "the lint warnings this fixture produces must still reach the caller: {diagnostics:?}"
    );
    assert!(!repository.exists());
}

#[test]
fn draft_apply_rejects_legacy_event_file_before_key_provider_diagnostic() {
    let directory = tempfile::tempdir().unwrap();
    let base = root().join("examples/graphs/software-feature.yaml");
    let loaded = graphhelm_schema::load_graph(&base).unwrap();
    let draft = graphhelm_protocols::GraphDraft {
        id: "draft-1".into(),
        expected_version: loaded.graph.metadata.version,
        expected_hash: graphhelm_graph::semantic_hash(&loaded.graph).unwrap(),
        operations: vec![graphhelm_protocols::DraftOperation::RemoveNode { id: "docs".into() }],
        manual_override: None,
    };
    let draft_path = directory.path().join("draft.json");
    std::fs::write(&draft_path, serde_json::to_vec(&draft).unwrap()).unwrap();
    let legacy = directory.path().join("events.jsonl");
    std::fs::write(&legacy, b"{\"legacy\":true}\n").unwrap();

    let output = command()
        .args([
            "graph",
            "draft",
            "apply",
            base.to_str().unwrap(),
            draft_path.to_str().unwrap(),
            "--actor",
            "owner-local",
            "--events",
            legacy.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(
        json(&output.stdout)["diagnostics"][0]["code"],
        "GHE007_UNSUPPORTED_FORMAT"
    );
    assert_eq!(std::fs::read(&legacy).unwrap(), b"{\"legacy\":true}\n");
}

/// #1049: a node is the TASK, so more than one agent may work it. The crew has to survive the
/// whole authored journey and not merely the schema: `graph validate` -> `graph lint` ->
/// `graph hash` -> `graph simulate`. The example is derived from the canonical one rather than
/// checked in beside it, so the crew is proven on a real graph without moving any committed
/// semantic hash.
#[test]
fn a_node_worked_by_a_crew_crosses_the_authored_journey() {
    let directory = tempfile::tempdir().unwrap();
    let graph = graphhelm_schema::load_graph(&root().join("examples/graphs/software-feature.yaml"))
        .unwrap()
        .graph;
    let mut document = serde_json::to_value(graph).unwrap();
    document["spec"]["nodes"]["map_repository"]["agents"] = serde_json::json!([
        {"ref": "project/security-reviewer@3"},
        {"ref": "project/scribe@2"}
    ]);
    let graph_path = directory.path().join("crew.json");
    std::fs::write(&graph_path, serde_json::to_vec(&document).unwrap()).unwrap();
    let path = graph_path.to_str().unwrap();

    for subcommand in ["validate", "lint", "hash"] {
        let output = command()
            .args(["graph", subcommand, path])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "graph {subcommand}: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert_eq!(json(&output.stdout)["ok"], true);
    }

    let events = directory.path().join("events.jsonl");
    let simulated = command()
        .args([
            "graph",
            "simulate",
            path,
            "--events",
            events.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        simulated.status.success(),
        "{}",
        String::from_utf8_lossy(&simulated.stdout)
    );
    assert_eq!(json(&simulated.stdout)["data"]["status"], "completed");
}
