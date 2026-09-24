use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

fn command() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

fn write_resource(root: &Path, relative: &str, bytes: &[u8]) -> Value {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, bytes).unwrap();
    json!({
        "id": relative.replace(['/', '.'], "-"),
        "kind": "fixture",
        "path": relative,
        "sha256": digest(bytes),
        "effects": [],
        "permissions": [],
        "requires": {"capabilities": [], "observers": []}
    })
}

fn manifest(contributions: Vec<Value>) -> Value {
    json!({
        "apiVersion": "p50.dev/v1",
        "kind": "Extension",
        "metadata": {
            "id": "graphhelm-jpd-test",
            "version": "1.0.0",
            "publisher": "graphhelm"
        },
        "spec": {
            "type": "skill-package",
            "capabilities": ["journey_proof"],
            "permissions": {
                "filesystem": {
                    "package": "read",
                    "workspaceArtifacts": "proposal-write"
                },
                "network": {
                    "external": false,
                    "loopbackRuntimeApi": true
                },
                "runtime": {
                    "read": true,
                    "mutations": ["amend_budget"],
                    "ownerConfirmationRequired": ["amend_budget"]
                },
                "secrets": {
                    "artifactValues": false,
                    "tokenFile": "reference-only"
                }
            },
            "contracts": {"contributions": contributions},
            "runtime": {"kind": "data", "isolationMinimum": "tier_0"},
            "compatibility": {"framework": ">=0.1"}
        }
    })
}

struct PackageFixture {
    _directory: TempDir,
    root: PathBuf,
    manifest: Value,
}

impl PackageFixture {
    fn write_manifest(&self) {
        fs::write(
            self.root.join("extension.json"),
            serde_json::to_vec_pretty(&self.manifest).unwrap(),
        )
        .unwrap();
    }

    fn run(&self) -> std::process::Output {
        command()
            .arg("extension")
            .arg("validate")
            .arg(&self.root)
            .output()
            .unwrap()
    }
}

fn valid_package() -> PackageFixture {
    let directory = TempDir::new().unwrap();
    let root = directory.path().to_path_buf();

    let skill = br#"---
name: journey-contract
description: Compile one user journey into observable promises and failure contracts.
---

# Journey contract

Read with `tool:status`, then validate locally with `cli:graph validate`. An owner can use
`cli:execution amend-budget` when the evidence calls for a new silence budget.
"#;
    fs::create_dir_all(root.join("skills/journey-contract")).unwrap();
    fs::write(root.join("skills/journey-contract/SKILL.md"), skill).unwrap();
    let skill_contribution = json!({
        "id": "journey-contract",
        "kind": "skill",
        "path": "skills/journey-contract/SKILL.md",
        "sha256": digest(skill),
        "surfaces": ["tool:status", "cli:graph validate", "cli:execution amend-budget"],
        "effects": ["runtime.connect", "runtime.mutate", "runtime.read"],
        "permissions": ["network.loopback", "owner.decision.request", "runtime.read", "runtime.write"],
        "requires": {"capabilities": [], "observers": []},
        "family": "journey"
    });

    let schema = br#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "urn:graphhelm:test:journey-contract",
  "type": "object",
  "required": ["journeyId"],
  "properties": {"journeyId": {"type": "string"}}
}"#;
    let mut schema_contribution = write_resource(&root, "schemas/journey.schema.json", schema);
    schema_contribution["id"] = json!("journey-contract-schema");
    schema_contribution["kind"] = json!("schema");

    let agent = br#"purpose: Find the shortest reproducible journey defect.
capabilities:
  - defect_analysis
allowedTools:
  - tool:status
  - cli:graph validate
  - cli:events verify
inputSchema: schema://JourneyContract@1
outputSchema: schema://JourneyDefectClaim@1
instructions: Preserve the semantic action trace.
completionContract: Emit a claim or a typed no-finding result.
"#;
    let mut agent_contribution = write_resource(&root, "agents/defect-hunter.yaml", agent);
    agent_contribution["id"] = json!("defect-hunter");
    agent_contribution["kind"] = json!("agent");
    agent_contribution["surfaces"] =
        json!(["tool:status", "cli:graph validate", "cli:events verify"]);
    agent_contribution["effects"] = json!(["runtime.connect", "runtime.read"]);
    agent_contribution["permissions"] = json!(["network.loopback", "runtime.read"]);

    let graph_path = root.join("graphs/dogfood.yaml");
    fs::create_dir_all(graph_path.parent().unwrap()).unwrap();
    fs::copy(
        repository_root().join("examples/graphs/software-feature.yaml"),
        &graph_path,
    )
    .unwrap();
    let mut graph: Value = serde_yaml_ng::from_slice(&fs::read(&graph_path).unwrap()).unwrap();
    graph["spec"]["policies"] = json!([]);
    let graph_bytes = serde_yaml_ng::to_string(&graph).unwrap().into_bytes();
    fs::write(&graph_path, &graph_bytes).unwrap();
    let graph_contribution = json!({
        "id": "dogfood-graph",
        "kind": "graph",
        "path": "graphs/dogfood.yaml",
        "sha256": digest(&graph_bytes),
        "surfaces": ["cli:graph validate", "cli:graph lint"],
        "effects": [],
        "permissions": [],
        "requires": {"capabilities": [], "observers": []}
    });

    let claude = br#"{"name":"graphhelm-jpd-test","version":"1.0.0"}"#;
    let mut claude_contribution = write_resource(&root, ".claude-plugin/plugin.json", claude);
    claude_contribution["id"] = json!("claude-host");
    claude_contribution["kind"] = json!("host-adapter");

    let codex = br#"{"id":"graphhelm-jpd-test","name":"graphhelm-jpd-test","version":"1.0.0"}"#;
    let mut codex_contribution = write_resource(&root, ".codex-plugin/plugin.json", codex);
    codex_contribution["id"] = json!("codex-host");
    codex_contribution["kind"] = json!("host-adapter");

    let mcp = br#"{"mcpServers":{"graphhelm":{"command":"${GRAPHHELM_CLI}","args":["mcp","--url","http://127.0.0.1:8080","--token-file","${GRAPHHELM_TOKEN_FILE}","--actor","${GRAPHHELM_ACTOR}"]}}}"#;
    let mut mcp_contribution = write_resource(&root, ".mcp.json", mcp);
    mcp_contribution["id"] = json!("graphhelm-mcp-host");
    mcp_contribution["kind"] = json!("host-adapter");
    mcp_contribution["surfaces"] = json!(["cli:mcp"]);
    mcp_contribution["effects"] = json!(["runtime.connect"]);
    mcp_contribution["permissions"] = json!(["network.loopback", "token.reference.read"]);

    let manifest = manifest(vec![
        graph_contribution,
        agent_contribution,
        skill_contribution,
        schema_contribution,
        claude_contribution,
        codex_contribution,
        mcp_contribution,
    ]);
    let fixture = PackageFixture {
        _directory: directory,
        root,
        manifest,
    };
    fixture.write_manifest();
    fixture
}

fn add_typed_policy(fixture: &mut PackageFixture) -> (usize, usize) {
    let schema = br#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "urn:graphhelm:test:strict-policy",
  "type": "object",
  "required": ["mode"],
  "properties": {"mode": {"const": "strict"}},
  "additionalProperties": false
}"#;
    let mut schema_contribution =
        write_resource(&fixture.root, "schemas/strict-policy.schema.json", schema);
    schema_contribution["id"] = json!("strict-policy-schema");
    schema_contribution["kind"] = json!("schema");

    let policy = b"mode: strict\n";
    let mut policy_contribution =
        write_resource(&fixture.root, "policies/strict-policy.yaml", policy);
    policy_contribution["id"] = json!("strict-policy");
    policy_contribution["kind"] = json!("policy");
    policy_contribution["schema"] = json!("schemas/strict-policy.schema.json");

    let contributions = fixture.manifest["spec"]["contracts"]["contributions"]
        .as_array_mut()
        .unwrap();
    let schema_index = contributions.len();
    contributions.push(schema_contribution);
    let policy_index = contributions.len();
    contributions.push(policy_contribution);
    fixture.write_manifest();
    (schema_index, policy_index)
}

fn add_artifact_flow_contract(fixture: &mut PackageFixture) {
    fixture.manifest["spec"]["contracts"]["entryFamilies"] = json!(["journey-contract"]);
    fixture.manifest["spec"]["contracts"]["artifactFlowFormat"] =
        json!("p50.dev/jpd/artifact-flow/v1");
    fixture.manifest["spec"]["contracts"]["artifactFlows"] = json!([{
        "family": "journey-contract",
        "inputs": ["task:scope", "runtime:status"],
        "outputs": ["schema:journey-contract-schema"]
    }]);
    fixture.write_manifest();
}

fn set_graph_agent_reference(fixture: &mut PackageFixture, reference: &str) {
    let graph_path = fixture.root.join("graphs/dogfood.yaml");
    let mut graph: Value = serde_yaml_ng::from_slice(&fs::read(&graph_path).unwrap()).unwrap();
    *graph
        .pointer_mut("/spec/nodes/map_repository/agent")
        .unwrap() = json!({"ref": reference});
    let bytes = serde_yaml_ng::to_string(&graph).unwrap().into_bytes();
    fs::write(&graph_path, &bytes).unwrap();
    fixture.manifest["spec"]["contracts"]["contributions"][0]["sha256"] = json!(digest(&bytes));
    fixture.write_manifest();
}

fn set_graph_agent_crew(fixture: &mut PackageFixture, crew: Value) {
    let graph_path = fixture.root.join("graphs/dogfood.yaml");
    let mut graph: Value = serde_yaml_ng::from_slice(&fs::read(&graph_path).unwrap()).unwrap();
    graph
        .pointer_mut("/spec/nodes/map_repository")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .insert("agents".into(), crew);
    let bytes = serde_yaml_ng::to_string(&graph).unwrap().into_bytes();
    fs::write(&graph_path, &bytes).unwrap();
    fixture.manifest["spec"]["contracts"]["contributions"][0]["sha256"] = json!(digest(&bytes));
    fixture.write_manifest();
}

fn set_graph_policies(fixture: &mut PackageFixture, policies: Value) {
    let graph_path = fixture.root.join("graphs/dogfood.yaml");
    let mut graph: Value = serde_yaml_ng::from_slice(&fs::read(&graph_path).unwrap()).unwrap();
    graph["spec"]["policies"] = policies;
    let bytes = serde_yaml_ng::to_string(&graph).unwrap().into_bytes();
    fs::write(&graph_path, &bytes).unwrap();
    fixture.manifest["spec"]["contracts"]["contributions"][0]["sha256"] = json!(digest(&bytes));
    fixture.write_manifest();
}

fn output_json(output: &std::process::Output) -> Value {
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn assert_domain_code(output: &std::process::Output, code: &str) -> Value {
    assert_eq!(output.status.code(), Some(2));
    let value = output_json(output);
    assert_eq!(value["ok"], false);
    assert_eq!(value["command"], "extension.validate");
    assert!(
        value["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|diagnostic| diagnostic["code"] == code),
        "expected {code}: {value}"
    );
    value
}

#[test]
fn validates_a_bounded_offline_package_and_returns_a_deterministic_digest() {
    let mut fixture = valid_package();
    let first = fixture.run();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stdout)
    );
    let first_json = output_json(&first);
    assert_eq!(first_json["ok"], true);
    assert_eq!(first_json["command"], "extension.validate");
    assert_eq!(first_json["data"]["id"], "graphhelm-jpd-test");
    assert_eq!(first_json["data"]["version"], "1.0.0");
    assert_eq!(first_json["data"]["contributionCount"], 7);
    assert!(
        first_json["data"]["packageDigest"]
            .as_str()
            .unwrap()
            .starts_with("sha256:")
    );

    fixture.manifest["spec"]["contracts"]["contributions"]
        .as_array_mut()
        .unwrap()
        .reverse();
    fixture.write_manifest();
    let second_json = output_json(&fixture.run());
    assert_eq!(
        first_json["data"]["packageDigest"],
        second_json["data"]["packageDigest"]
    );
}

#[test]
fn package_digest_binds_canonical_semantic_manifest_permissions() {
    let mut fixture = valid_package();
    let first = fixture.run();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stdout)
    );
    let first_digest = output_json(&first)["data"]["packageDigest"]
        .as_str()
        .unwrap()
        .to_owned();

    fixture.manifest["spec"]["permissions"]["network"]["external"] = json!(true);
    fixture.write_manifest();
    let second = fixture.run();
    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stdout)
    );
    let second_digest = output_json(&second)["data"]["packageDigest"]
        .as_str()
        .unwrap()
        .to_owned();

    assert_ne!(first_digest, second_digest);
}

#[test]
fn rejects_manifest_schema_errors_with_a_redacted_diagnostic() {
    let mut fixture = valid_package();
    fixture.manifest["metadata"]
        .as_object_mut()
        .unwrap()
        .remove("publisher");
    fixture.write_manifest();

    let value = assert_domain_code(&fixture.run(), "GHEX002_MANIFEST");
    assert!(
        !value
            .to_string()
            .contains(&fixture.root.to_string_lossy().to_string())
    );
}

#[test]
fn rejects_digest_tampering() {
    let fixture = valid_package();
    fs::write(
        fixture.root.join("skills/journey-contract/SKILL.md"),
        "tampered",
    )
    .unwrap();
    assert_domain_code(&fixture.run(), "GHEX005_DIGEST");
}

#[test]
fn rejects_traversal_and_duplicate_ids_or_paths() {
    let directory = TempDir::new().unwrap();
    let outside = directory.path().join("outside.txt");
    fs::write(&outside, b"outside").unwrap();
    let root = directory.path().join("package");
    fs::create_dir(&root).unwrap();
    fs::write(
        root.join("extension.json"),
        serde_json::to_vec(&manifest(vec![json!({
            "id": "escape",
            "kind": "fixture",
            "path": "../outside.txt",
            "sha256": digest(b"outside"),
            "effects": [],
            "permissions": [],
            "requires": {"capabilities": [], "observers": []}
        })]))
        .unwrap(),
    )
    .unwrap();
    let output = command()
        .args(["extension", "validate"])
        .arg(&root)
        .output()
        .unwrap();
    assert_domain_code(&output, "GHEX004_PATH");

    let mut fixture = valid_package();
    let duplicate = fixture.manifest["spec"]["contracts"]["contributions"][0].clone();
    fixture.manifest["spec"]["contracts"]["contributions"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);
    fixture.write_manifest();
    assert_domain_code(&fixture.run(), "GHEX003_CONTRIBUTION");
}

#[test]
fn rejects_unknown_or_undeclared_public_surface_markers() {
    let mut fixture = valid_package();
    fixture.manifest["spec"]["contracts"]["contributions"][2]["surfaces"] =
        json!(["tool:not-real", "cli:graph validate"]);
    fixture.write_manifest();
    assert_domain_code(&fixture.run(), "GHEX006_SURFACE");

    let mut fixture = valid_package();
    fixture.manifest["spec"]["contracts"]["contributions"][2]["surfaces"] =
        json!(["cli:graph validate"]);
    fixture.write_manifest();
    assert_domain_code(&fixture.run(), "GHEX006_SURFACE");

    let mut fixture = valid_package();
    let skill_path = fixture.root.join("skills/journey-contract/SKILL.md");
    let mut skill = fs::read(&skill_path).unwrap();
    skill.extend_from_slice(b"\nCall tool:not-real without Markdown code delimiters.\n");
    fs::write(&skill_path, &skill).unwrap();
    fixture.manifest["spec"]["contracts"]["contributions"][2]["sha256"] = json!(digest(&skill));
    fixture.write_manifest();
    assert_domain_code(&fixture.run(), "GHEX006_SURFACE");

    let mut fixture = valid_package();
    let skill_path = fixture.root.join("skills/journey-contract/SKILL.md");
    let mut skill = fs::read(&skill_path).unwrap();
    skill.extend_from_slice(b"\nPlease call `the tool:cancel now.\n");
    fs::write(&skill_path, &skill).unwrap();
    fixture.manifest["spec"]["contracts"]["contributions"][2]["sha256"] = json!(digest(&skill));
    fixture.write_manifest();
    assert_domain_code(&fixture.run(), "GHEX006_SURFACE");

    for suffix in [
        "\nEscaped delimiters are plain text: \\`tool:cancel\\`.\n",
        "\n```text\ntool:cancel\n```\n",
    ] {
        let mut fixture = valid_package();
        let skill_path = fixture.root.join("skills/journey-contract/SKILL.md");
        let mut skill = fs::read(&skill_path).unwrap();
        skill.extend_from_slice(suffix.as_bytes());
        fs::write(&skill_path, &skill).unwrap();
        fixture.manifest["spec"]["contracts"]["contributions"][2]["sha256"] = json!(digest(&skill));
        fixture.write_manifest();
        assert_domain_code(&fixture.run(), "GHEX006_SURFACE");
    }

    let mut fixture = valid_package();
    let skill_path = fixture.root.join("skills/journey-contract/SKILL.md");
    let mut skill = fs::read(&skill_path).unwrap();
    skill.extend_from_slice(b"\nA longer code delimiter is valid: ``tool:status``.\n");
    fs::write(&skill_path, &skill).unwrap();
    fixture.manifest["spec"]["contracts"]["contributions"][2]["sha256"] = json!(digest(&skill));
    fixture.write_manifest();
    let output = fixture.run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn requires_explicit_unique_and_sorted_contribution_authority() {
    let mut fixture = valid_package();
    fixture.manifest["spec"]["contracts"]["contributions"][2]
        .as_object_mut()
        .unwrap()
        .remove("effects");
    fixture.write_manifest();
    assert_domain_code(&fixture.run(), "GHEX003_CONTRIBUTION");

    let mut fixture = valid_package();
    fixture.manifest["spec"]["contracts"]["contributions"][2]["effects"] = json!(["read", "read"]);
    fixture.write_manifest();
    assert_domain_code(&fixture.run(), "GHEX017_AUTHORITY");

    let mut fixture = valid_package();
    fixture.manifest["spec"]["contracts"]["contributions"][2]["permissions"] =
        json!(["network.teleport", "runtime.write"]);
    fixture.write_manifest();
    assert_domain_code(&fixture.run(), "GHEX017_AUTHORITY");

    let mut fixture = valid_package();
    fixture.manifest["spec"]["contracts"]["contributions"][2]["requires"]["capabilities"] =
        json!(["zeta", "alpha"]);
    fixture.write_manifest();
    assert_domain_code(&fixture.run(), "GHEX017_AUTHORITY");
}

#[test]
fn rejects_unknown_authority_and_package_grant_escalation() {
    let mut fixture = valid_package();
    fixture.manifest["spec"]["contracts"]["contributions"][2]["effects"] =
        json!(["database.drop", "runtime.mutate"]);
    fixture.write_manifest();
    assert_domain_code(&fixture.run(), "GHEX017_AUTHORITY");

    let mut fixture = valid_package();
    fixture.manifest["spec"]["contracts"]["contributions"][2]["permissions"] =
        json!(["network.external", "runtime.write"]);
    fixture.write_manifest();
    assert_domain_code(&fixture.run(), "GHEX018_AUTHORITY_ESCALATION");

    let mut fixture = valid_package();
    fixture.manifest["spec"]["contracts"]["contributions"][2]["effects"] =
        json!(["external.read", "runtime.mutate"]);
    fixture.write_manifest();
    assert_domain_code(&fixture.run(), "GHEX018_AUTHORITY_ESCALATION");

    let mut fixture = valid_package();
    fixture.manifest["spec"]["contracts"]["contributions"][2]["surfaces"]
        .as_array_mut()
        .unwrap()
        .push(json!("tool:start"));
    fixture.write_manifest();
    assert_domain_code(&fixture.run(), "GHEX018_AUTHORITY_ESCALATION");
}

#[test]
fn rejects_effect_without_matching_contribution_permission() {
    let mut fixture = valid_package();
    fixture.manifest["spec"]["contracts"]["contributions"][0]["effects"] =
        json!(["artifact.local.write"]);
    fixture.write_manifest();

    let value = assert_domain_code(&fixture.run(), "GHEX018_AUTHORITY_ESCALATION");
    assert!(
        value["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|diagnostic| {
                diagnostic["path"] == "/spec/contracts/contributions/0/effects/0"
            })
    );
}

#[test]
fn rejects_runtime_surfaces_without_explicit_local_authority() {
    let mut fixture = valid_package();
    fixture.manifest["spec"]["contracts"]["contributions"][0]["surfaces"]
        .as_array_mut()
        .unwrap()
        .push(json!("tool:status"));
    fixture.write_manifest();
    assert_domain_code(&fixture.run(), "GHEX018_AUTHORITY_ESCALATION");

    let mut fixture = valid_package();
    fixture.manifest["spec"]["contracts"]["contributions"][0]["surfaces"]
        .as_array_mut()
        .unwrap()
        .push(json!("cli:execution status"));
    fixture.write_manifest();
    assert_domain_code(&fixture.run(), "GHEX018_AUTHORITY_ESCALATION");

    let mut fixture = valid_package();
    fixture.manifest["spec"]["contracts"]["contributions"][2]["permissions"]
        .as_array_mut()
        .unwrap()
        .retain(|permission| permission != "owner.decision.request");
    fixture.write_manifest();
    assert_domain_code(&fixture.run(), "GHEX018_AUTHORITY_ESCALATION");
}

#[test]
fn allows_data_only_artifact_proposals_without_write_permission() {
    let mut fixture = valid_package();
    fixture.manifest["spec"]["permissions"]["filesystem"]
        .as_object_mut()
        .unwrap()
        .remove("workspaceArtifacts");
    fixture.manifest["spec"]["contracts"]["contributions"][0]["effects"] =
        json!(["artifact.propose"]);
    fixture.write_manifest();

    let output = fixture.run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn validates_a_closed_artifact_flow_contract() {
    let mut fixture = valid_package();
    add_artifact_flow_contract(&mut fixture);

    let output = fixture.run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn rejects_artifact_flow_format_shape_and_reference_errors() {
    let mut fixture = valid_package();
    add_artifact_flow_contract(&mut fixture);
    fixture.manifest["spec"]["contracts"]["artifactFlowFormat"] = json!("unknown/v2");
    fixture.write_manifest();
    assert_domain_code(&fixture.run(), "GHEX019_ARTIFACT_FLOW");

    let mut fixture = valid_package();
    add_artifact_flow_contract(&mut fixture);
    fixture.manifest["spec"]["contracts"]["artifactFlows"][0]["unexpected"] = json!(true);
    fixture.write_manifest();
    assert_domain_code(&fixture.run(), "GHEX019_ARTIFACT_FLOW");

    let mut fixture = valid_package();
    add_artifact_flow_contract(&mut fixture);
    fixture.manifest["spec"]["contracts"]["artifactFlows"][0]["inputs"] =
        json!(["secret:value", "task:scope", "task:scope"]);
    fixture.write_manifest();
    assert_domain_code(&fixture.run(), "GHEX019_ARTIFACT_FLOW");

    let mut fixture = valid_package();
    add_artifact_flow_contract(&mut fixture);
    fixture.manifest["spec"]["contracts"]["artifactFlows"][0]["outputs"] =
        json!(["schema:not-declared"]);
    fixture.write_manifest();
    assert_domain_code(&fixture.run(), "GHEX019_ARTIFACT_FLOW");
}

#[test]
fn requires_exactly_one_artifact_flow_per_entry_family() {
    let mut fixture = valid_package();
    add_artifact_flow_contract(&mut fixture);
    let duplicate = fixture.manifest["spec"]["contracts"]["artifactFlows"][0].clone();
    fixture.manifest["spec"]["contracts"]["artifactFlows"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);
    fixture.write_manifest();
    assert_domain_code(&fixture.run(), "GHEX019_ARTIFACT_FLOW");

    let mut fixture = valid_package();
    add_artifact_flow_contract(&mut fixture);
    fixture.manifest["spec"]["contracts"]["entryFamilies"] =
        json!(["journey-contract", "plan-council"]);
    fixture.manifest["spec"]["contracts"]["artifactFlows"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "family": "not-an-entry-family",
            "inputs": ["artifact:proposal"],
            "outputs": ["artifact:review"]
        }));
    fixture.write_manifest();

    assert_domain_code(&fixture.run(), "GHEX019_ARTIFACT_FLOW");
}

#[test]
fn validates_typed_data_against_a_declared_offline_package_schema() {
    let mut fixture = valid_package();
    add_typed_policy(&mut fixture);
    let output = fixture.run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(output_json(&output)["data"]["contributionCount"], 9);
}

#[test]
fn requires_policy_evaluator_and_observer_schema_references() {
    for (kind, directory) in [
        ("policy", "policies"),
        ("evaluator", "evaluators"),
        ("observer", "observers"),
    ] {
        let mut fixture = valid_package();
        let relative = format!("{directory}/missing-schema.yaml");
        let mut contribution = write_resource(&fixture.root, &relative, b"mode: strict\n");
        contribution["id"] = json!(format!("missing-{kind}-schema"));
        contribution["kind"] = json!(kind);
        fixture.manifest["spec"]["contracts"]["contributions"]
            .as_array_mut()
            .unwrap()
            .push(contribution);
        fixture.write_manifest();
        assert_domain_code(&fixture.run(), "GHEX014_SCHEMA_REF");
    }
}

#[test]
fn resolves_declared_cross_resource_schema_references_offline() {
    let mut fixture = valid_package();
    let capability_schema = br#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "urn:graphhelm:test:observer-capability",
  "type": "object",
  "required": ["observerId"],
  "properties": {"observerId": {"type": "string"}},
  "additionalProperties": false
}"#;
    let mut capability = write_resource(
        &fixture.root,
        "schemas/observer-capability.schema.json",
        capability_schema,
    );
    capability["id"] = json!("observer-capability-schema");
    capability["kind"] = json!("schema");

    let catalog_schema = br#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "urn:graphhelm:test:observer-catalog",
  "type": "object",
  "required": ["observers"],
  "properties": {
    "observers": {
      "type": "array",
      "items": {"$ref": "urn:graphhelm:test:observer-capability"}
    }
  },
  "additionalProperties": false
}"#;
    let mut catalog = write_resource(
        &fixture.root,
        "schemas/observer-catalog.schema.json",
        catalog_schema,
    );
    catalog["id"] = json!("observer-catalog-schema");
    catalog["kind"] = json!("schema");

    let observer = b"observers:\n  - observerId: graphhelm-cli\n";
    let mut observer_contribution =
        write_resource(&fixture.root, "observers/catalog.yaml", observer);
    observer_contribution["id"] = json!("observer-catalog");
    observer_contribution["kind"] = json!("observer");
    observer_contribution["schema"] = json!("schemas/observer-catalog.schema.json");

    fixture.manifest["spec"]["contracts"]["contributions"]
        .as_array_mut()
        .unwrap()
        .extend([capability, catalog, observer_contribution]);
    fixture.write_manifest();
    let output = fixture.run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn rejects_typed_data_parse_schema_and_reference_failures() {
    let mut fixture = valid_package();
    let (_, policy_index) = add_typed_policy(&mut fixture);
    fixture.manifest["spec"]["contracts"]["contributions"][policy_index]["schema"] =
        json!("../outside.schema.json");
    fixture.write_manifest();
    assert_domain_code(&fixture.run(), "GHEX014_SCHEMA_REF");

    let mut fixture = valid_package();
    let (_, policy_index) = add_typed_policy(&mut fixture);
    let bytes = b"mode: [unterminated\n";
    fs::write(fixture.root.join("policies/strict-policy.yaml"), bytes).unwrap();
    fixture.manifest["spec"]["contracts"]["contributions"][policy_index]["sha256"] =
        json!(digest(bytes));
    fixture.write_manifest();
    assert_domain_code(&fixture.run(), "GHEX015_DATA_PARSE");

    let mut fixture = valid_package();
    let (_, policy_index) = add_typed_policy(&mut fixture);
    let bytes = b"mode: weak\n";
    fs::write(fixture.root.join("policies/strict-policy.yaml"), bytes).unwrap();
    fixture.manifest["spec"]["contracts"]["contributions"][policy_index]["sha256"] =
        json!(digest(bytes));
    fixture.write_manifest();
    assert_domain_code(&fixture.run(), "GHEX016_DATA_SCHEMA");
}

#[test]
fn rejects_invalid_skill_frontmatter() {
    let fixture = valid_package();
    let bytes = b"---\nname: journey-contract\n---\n\nNo description.\n";
    fs::write(fixture.root.join("skills/journey-contract/SKILL.md"), bytes).unwrap();
    let mut manifest: Value =
        serde_json::from_slice(&fs::read(fixture.root.join("extension.json")).unwrap()).unwrap();
    manifest["spec"]["contracts"]["contributions"][2]["sha256"] = json!(digest(bytes));
    fs::write(
        fixture.root.join("extension.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    assert_domain_code(&fixture.run(), "GHEX007_SKILL");
}

#[test]
fn rejects_invalid_inline_schema_agent_and_graph_contributions() {
    for (index, bytes, code) in [
        (
            3usize,
            br#"{"$schema":"https://json-schema.org/draft/2020-12/schema","$ref":"https://example.invalid/schema"}"#.as_slice(),
            "GHEX008_SCHEMA",
        ),
        (1usize, b"purpose: incomplete\n".as_slice(), "GHEX009_AGENT"),
        (0usize, b"not: a graph\n".as_slice(), "GHEX010_GRAPH"),
    ] {
        let fixture = valid_package();
        let relative = fixture.manifest["spec"]["contracts"]["contributions"][index]["path"]
            .as_str()
            .unwrap();
        fs::write(fixture.root.join(relative), bytes).unwrap();
        let mut manifest: Value =
            serde_json::from_slice(&fs::read(fixture.root.join("extension.json")).unwrap()).unwrap();
        manifest["spec"]["contracts"]["contributions"][index]["sha256"] =
            json!(digest(bytes));
        fs::write(
            fixture.root.join("extension.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        assert_domain_code(&fixture.run(), code);
    }
}

#[test]
fn requires_canonical_package_local_graph_extension_references() {
    let mut fixture = valid_package();
    set_graph_agent_reference(&mut fixture, "extension://graphhelm-jpd-test/defect-hunter");
    let output = fixture.run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );

    for reference in [
        "defect-hunter",
        "extension://another-package/defect-hunter",
        "extension://graphhelm-jpd-test/agents/defect-hunter",
        "extension://graphhelm-jpd-test/defect-hunter@1.0.0",
    ] {
        let mut fixture = valid_package();
        set_graph_agent_reference(&mut fixture, reference);
        assert_domain_code(&fixture.run(), "GHEX020_EXTENSION_REF");
    }
}

// #1049: a crew member's reference is the same reference the singular form carries, and an
// extension package must not be able to smuggle a dangling one in by writing it in the plural.
#[test]
fn requires_canonical_package_local_references_for_every_crew_member() {
    let mut fixture = valid_package();
    set_graph_agent_reference(&mut fixture, "extension://graphhelm-jpd-test/defect-hunter");
    set_graph_agent_crew(
        &mut fixture,
        json!([{"ref": "extension://graphhelm-jpd-test/defect-hunter"}]),
    );
    let output = fixture.run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );

    for reference in [
        "defect-hunter",
        "extension://another-package/defect-hunter",
        "extension://graphhelm-jpd-test/agents/defect-hunter",
        "extension://graphhelm-jpd-test/defect-hunter@1.0.0",
    ] {
        let mut fixture = valid_package();
        set_graph_agent_reference(&mut fixture, "extension://graphhelm-jpd-test/defect-hunter");
        set_graph_agent_crew(
            &mut fixture,
            json!([
                {"ref": "extension://graphhelm-jpd-test/defect-hunter"},
                {"ref": reference}
            ]),
        );
        let value = assert_domain_code(&fixture.run(), "GHEX020_EXTENSION_REF");
        assert!(
            value["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|diagnostic| {
                    diagnostic["code"] == "GHEX020_EXTENSION_REF"
                        && diagnostic["path"]
                            .as_str()
                            .unwrap()
                            .ends_with("/spec/nodes/map_repository/agents/1/ref")
                }),
            "the refused crew member must be named by its own index: {value}"
        );
    }
}

#[test]
fn requires_canonical_typed_policy_references_in_extension_graphs() {
    let mut fixture = valid_package();
    add_typed_policy(&mut fixture);
    set_graph_policies(
        &mut fixture,
        json!(["extension://graphhelm-jpd-test/strict-policy"]),
    );
    let output = fixture.run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );

    for reference in [
        "strict-policy",
        "policy://workspace/strict-policy@1",
        "extension://graphhelm-jpd-test/defect-hunter",
    ] {
        let mut fixture = valid_package();
        add_typed_policy(&mut fixture);
        set_graph_policies(&mut fixture, json!([reference]));
        assert_domain_code(&fixture.run(), "GHEX020_EXTENSION_REF");
    }
}

#[test]
fn rejects_unknown_or_undeclared_graphhelm_agent_tools() {
    for (allowed_tools, surfaces) in [
        (
            json!(["graphhelm.mcp.typo"]),
            json!(["tool:status", "cli:graph validate"]),
        ),
        (
            json!(["graphhelm.mcp.status"]),
            json!(["cli:graph validate"]),
        ),
        (
            json!(["graphhelm.cli.graph.typo"]),
            json!(["tool:status", "cli:graph validate"]),
        ),
        (
            json!(["tool:typo"]),
            json!(["tool:status", "cli:graph validate"]),
        ),
    ] {
        let fixture = valid_package();
        let agent_path = fixture.root.join("agents/defect-hunter.yaml");
        let mut agent: Value = serde_yaml_ng::from_slice(&fs::read(&agent_path).unwrap()).unwrap();
        agent["allowedTools"] = allowed_tools;
        let bytes = serde_yaml_ng::to_string(&agent).unwrap().into_bytes();
        fs::write(&agent_path, &bytes).unwrap();

        let mut manifest: Value =
            serde_json::from_slice(&fs::read(fixture.root.join("extension.json")).unwrap())
                .unwrap();
        manifest["spec"]["contracts"]["contributions"][1]["sha256"] = json!(digest(&bytes));
        manifest["spec"]["contracts"]["contributions"][1]["surfaces"] = surfaces;
        fs::write(
            fixture.root.join("extension.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        assert_domain_code(&fixture.run(), "GHEX006_SURFACE");
    }
}

#[test]
fn rejects_agent_instruction_surfaces_that_bypass_allowed_tools() {
    let fixture = valid_package();
    let agent_path = fixture.root.join("agents/defect-hunter.yaml");
    let mut agent: Value = serde_yaml_ng::from_slice(&fs::read(&agent_path).unwrap()).unwrap();
    agent.as_object_mut().unwrap().remove("allowedTools");
    agent["instructions"] = json!("Call tool:cancel without declaring the public surface.");
    let bytes = serde_yaml_ng::to_string(&agent).unwrap().into_bytes();
    fs::write(&agent_path, &bytes).unwrap();

    let mut manifest: Value =
        serde_json::from_slice(&fs::read(fixture.root.join("extension.json")).unwrap()).unwrap();
    manifest["spec"]["contracts"]["contributions"][1]["sha256"] = json!(digest(&bytes));
    fs::write(
        fixture.root.join("extension.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    assert_domain_code(&fixture.run(), "GHEX006_SURFACE");
}

#[test]
fn rejects_discoverable_files_that_are_not_in_the_manifest_inventory() {
    let fixture = valid_package();
    let hidden = fixture.root.join("skills/hidden/SKILL.md");
    fs::create_dir_all(hidden.parent().unwrap()).unwrap();
    fs::write(&hidden, b"undeclared").unwrap();
    assert_domain_code(&fixture.run(), "GHEX012_INVENTORY");

    let fixture = valid_package();
    fs::write(fixture.root.join("surprise.txt"), b"unknown").unwrap();
    assert_domain_code(&fixture.run(), "GHEX012_INVENTORY");
}

#[test]
fn rejects_host_adapters_that_do_not_match_the_extension_envelope() {
    for (index, relative, bytes) in [
        (
            4usize,
            ".claude-plugin/plugin.json",
            br#"{"name":"another-plugin","version":"1.0.0"}"#.as_slice(),
        ),
        (
            5usize,
            ".codex-plugin/plugin.json",
            br#"{"id":"graphhelm-jpd-test","name":"graphhelm-jpd-test","version":"9.9.9"}"#
                .as_slice(),
        ),
        (
            6usize,
            ".mcp.json",
            br#"{"mcpServers":{"graphhelm":{"command":"${GRAPHHELM_CLI}","args":["mcp","--url","http://127.0.0.1:8080","--token","inline-secret","--actor","${GRAPHHELM_ACTOR}"]}}}"#.as_slice(),
        ),
    ] {
        let fixture = valid_package();
        fs::write(fixture.root.join(relative), bytes).unwrap();
        let mut manifest: Value =
            serde_json::from_slice(&fs::read(fixture.root.join("extension.json")).unwrap()).unwrap();
        manifest["spec"]["contracts"]["contributions"][index]["sha256"] =
            json!(digest(bytes));
        fs::write(
            fixture.root.join("extension.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        let value = assert_domain_code(&fixture.run(), "GHEX013_HOST");
        assert!(!value.to_string().contains("inline-secret"));
    }
}

#[test]
fn rejects_mcp_adapter_that_resolves_graphhelm_from_host_search_path() {
    let fixture = valid_package();
    let bytes = br#"{"mcpServers":{"graphhelm":{"command":"graphhelm","args":["mcp","--url","http://127.0.0.1:8080","--token-file","${GRAPHHELM_TOKEN_FILE}","--actor","${GRAPHHELM_ACTOR}"]}}}"#;
    fs::write(fixture.root.join(".mcp.json"), bytes).unwrap();
    let mut manifest: Value =
        serde_json::from_slice(&fs::read(fixture.root.join("extension.json")).unwrap()).unwrap();
    manifest["spec"]["contracts"]["contributions"][6]["sha256"] = json!(digest(bytes));
    fs::write(
        fixture.root.join("extension.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    assert_domain_code(&fixture.run(), "GHEX013_HOST");
}

#[test]
fn malformed_extension_invocations_return_one_json_envelope() {
    for arguments in [
        vec!["extension", "validate"],
        vec!["extension", "unknown"],
        vec!["--pretty", "extension", "validate"],
        vec!["--json", "extension", "validate"],
        vec!["extension", "--json", "validate"],
    ] {
        let output = command().args(arguments).output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stderr.is_empty());
        let value = output_json(&output);
        assert_eq!(value["ok"], false);
        assert_eq!(value["command"], "extension");
        assert_eq!(value["diagnostics"][0]["code"], "GHCLI001_ARGUMENT_INVALID");
    }
}

#[test]
fn bounds_package_diagnostics_with_one_deterministic_omission_sentinel() {
    let fixture = valid_package();
    let skill_path = fixture.root.join("skills/journey-contract/SKILL.md");
    let mut skill = String::from(
        "---\nname: journey-contract\ndescription: Exercise package-wide diagnostic bounds.\n---\n\n",
    );
    for index in 0..300 {
        skill.push_str(&format!("`tool:invalid-{index:03}`\n"));
    }
    fs::write(&skill_path, skill.as_bytes()).unwrap();

    let mut manifest: Value =
        serde_json::from_slice(&fs::read(fixture.root.join("extension.json")).unwrap()).unwrap();
    manifest["spec"]["contracts"]["contributions"][2]["sha256"] = json!(digest(skill.as_bytes()));
    fs::write(
        fixture.root.join("extension.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let first = fixture.run();
    assert_eq!(first.status.code(), Some(2));
    assert!(first.stderr.is_empty());
    let first = output_json(&first);
    let diagnostics = first["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics.len(), 257);
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| {
                diagnostic["code"] == "GHEX011_LIMIT"
                    && diagnostic["path"] == "/diagnostics"
                    && diagnostic["message"]
                        == "extension package diagnostic limit reached; remaining validation omitted"
            })
            .count(),
        1
    );
    assert!(diagnostics.windows(2).all(|pair| {
        (
            pair[0]["path"].as_str().unwrap(),
            pair[0]["code"].as_str().unwrap(),
            pair[0]["message"].as_str().unwrap(),
        ) <= (
            pair[1]["path"].as_str().unwrap(),
            pair[1]["code"].as_str().unwrap(),
            pair[1]["message"].as_str().unwrap(),
        )
    }));

    let second = output_json(&fixture.run());
    assert_eq!(first["diagnostics"], second["diagnostics"]);
}

#[test]
fn rejects_many_increasing_unmatched_markdown_delimiters() {
    let fixture = valid_package();
    let skill_path = fixture.root.join("skills/journey-contract/SKILL.md");
    let mut skill = String::from(
        "---\nname: journey-contract\ndescription: Reject unmatched Markdown delimiters.\n---\n\n",
    );
    for delimiter_length in 1..=128 {
        skill.push_str(&"`".repeat(delimiter_length));
        skill.push('x');
    }
    fs::write(&skill_path, skill.as_bytes()).unwrap();

    let mut manifest: Value =
        serde_json::from_slice(&fs::read(fixture.root.join("extension.json")).unwrap()).unwrap();
    manifest["spec"]["contracts"]["contributions"][2]["sha256"] = json!(digest(skill.as_bytes()));
    fs::write(
        fixture.root.join("extension.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    assert_domain_code(&fixture.run(), "GHEX006_SURFACE");
}

/// Parses one `--help` invocation's `Commands:` section into subcommand names, in the exact form
/// clap prints them (kebab-case). Anything outside that section (Usage/Options/Arguments/a
/// trailing blank line) ends the scan - defensively, since a name accidentally picked up from
/// the wrong section would silently widen the derived surface rather than narrow it.
fn parse_help_subcommand_names(help_text: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_commands_section = false;
    for line in help_text.lines() {
        if line.trim() == "Commands:" {
            in_commands_section = true;
            continue;
        }
        if !in_commands_section {
            continue;
        }
        if line.trim().is_empty() || !line.starts_with(char::is_whitespace) {
            break;
        }
        if let Some(name) = line.split_whitespace().next()
            && name != "help"
        {
            names.push(name.to_owned());
        }
    }
    names
}

/// Walks the REAL binary's own `--help` tree (never a hand-list) to derive every invocable
/// command path, space-joined to match the `cli:<command>` surface convention
/// `core/schema/src/extension.rs`'s `CLI_COMMANDS` uses. A rename or removal changes what
/// `--help` prints, so this set changes with it automatically - the guard below is only as good
/// as this derivation staying faithful to the real CLI, which is why it walks the built binary
/// instead of re-parsing `args.rs`'s source text (one less thing this test could get out of sync
/// with: clap's own `--help` IS the CLI's public contract).
fn derive_real_cli_command_paths() -> std::collections::BTreeSet<String> {
    let mut paths = std::collections::BTreeSet::new();
    let mut frontier: Vec<Vec<String>> = vec![Vec::new()];
    while let Some(prefix) = frontier.pop() {
        let mut invocation = command();
        for part in &prefix {
            invocation.arg(part);
        }
        let output = invocation.arg("--help").output().unwrap();
        let help_text = String::from_utf8_lossy(&output.stdout);
        let subcommands = parse_help_subcommand_names(&help_text);
        if subcommands.is_empty() {
            if !prefix.is_empty() {
                paths.insert(prefix.join(" "));
            }
            continue;
        }
        for name in subcommands {
            let mut next = prefix.clone();
            next.push(name);
            frontier.push(next);
        }
    }
    paths
}

#[test]
fn cli_and_mcp_surface_allowlists_stay_a_subset_of_the_real_derived_surface() {
    let (cli_commands, mcp_tools) = graphhelm_schema::__surface_allowlists_for_testing();
    let real_commands = derive_real_cli_command_paths();

    // Two assertions, two DIFFERENT reasons -- neither is the other's spare copy (#249).
    //
    // FIRST, and it is DIAGNOSTIC rather than a vacuity guard. An empty derivation does not make
    // the subset check below pass: `A ⊆ ∅` is vacuous only when A is empty, and A here is
    // CLI_COMMANDS. Measured, with the derivation returning a legal empty set: the loop below
    // marks every one of the 21 declared commands unknown and the test goes RED -- but it goes red
    // saying "CLI_COMMANDS names a surface the real CLI does not have (renamed or removed?)" and
    // attaches all 21 as if they were the defect. A `--help` regression is then diagnosed as an
    // allowlist problem, with twenty-one innocent names in the report. This assertion exists so
    // the failure names its own cause; without it the guard is not silent, it is CONFIDENTLY WRONG.
    assert!(
        !real_commands.is_empty(),
        "the CLI surface derivation returned nothing, so nothing below has been compared. This is \
         a build or `--help` regression -- clap stopped emitting a `Commands:` section, or the \
         binary failed to run -- NOT a CLI_COMMANDS problem. Do not edit the allowlist to make \
         this pass."
    );

    // SECOND, and this one IS the vacuity guard, on the side where vacuity actually lives. An
    // empty CLI_COMMANDS never enters the loop, `unknown_cli` stays empty, and the test passes
    // having compared nothing at all. Measured: with the allowlist emptied and the derivation
    // healthy, this test was GREEN before this line existed.
    //
    // The MCP half of this same test has carried its twin (`!mcp_tools.is_empty()`) since it was
    // written; the CLI half never did. The symmetry was half-built inside one function.
    assert!(
        !cli_commands.is_empty(),
        "CLI_COMMANDS is empty, so the subset check below compares nothing and passes for free. \
         The allowlist is the surface this repository claims to expose -- an empty one is not a \
         clean bill of health"
    );

    let mut unknown_cli: Vec<String> = Vec::new();
    for declared in cli_commands {
        let declared_owned = declared.to_string();
        if !real_commands.contains(&declared_owned) {
            unknown_cli.push(declared_owned);
        }
    }
    assert!(
        unknown_cli.is_empty(),
        "CLI_COMMANDS names a surface the real CLI does not have (renamed or removed?): {unknown_cli:?}\nreal surface: {real_commands:?}"
    );

    // MCP_TOOLS names the Public Runtime API's tool surface, not a clap subcommand tree - `mcp`
    // itself is a real, leaf CLI command (no further subcommands), so the same --help walk that
    // derives CLI_COMMANDS's world cannot also derive MCP_TOOLS's. What this cell checks is that
    // `mcp` itself is still a real command, since MCP_TOOLS's whole premise is that requests
    // arrive through it.
    //
    // This used to read "what IS checkable from here, cheaply", which was true about --help and
    // false about the environment, and the difference held the existence check open (#231): the
    // tool surface is a static table in the same binary, and the binary lists it on request. The
    // subset property now lives in
    // `every_mcp_tools_entry_names_a_tool_the_binary_actually_serves` below. Measured while
    // writing it: rename a tool and THIS cell stays green.
    assert!(
        real_commands.contains("mcp"),
        "MCP_TOOLS assumes the `mcp` CLI command exists to carry these tool calls, but the real \
         CLI no longer has it"
    );
    assert!(
        !mcp_tools.is_empty(),
        "MCP_TOOLS should not be empty while `mcp` names it as the transport"
    );
}

/// The real tool surface, read from the BINARY rather than derived from `--help`.
///
/// The server answers `tools/list` from its own static table before any request reaches the API,
/// so the URL points at a port nothing listens on: the names are a property of the build, and
/// involving a live server would make this test depend on something it is not measuring.
fn real_mcp_tool_names() -> Vec<String> {
    let mut input = String::new();
    for line in [
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {"protocolVersion": "2025-06-18", "capabilities": {},
                       "clientInfo": {"name": "surface-existence", "version": "0"}}}),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
    ] {
        input.push_str(&line.to_string());
        input.push('\n');
    }
    let output = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args(["mcp", "--url", "http://127.0.0.1:9", "--actor", "agent-x"])
        .env("GRAPHHELM_API_TOKEN", "test-token")
        .write_stdin(input)
        .timeout(std::time::Duration::from_secs(30))
        .output()
        .expect("the mcp server runs to EOF");

    let reply = String::from_utf8(output.stdout)
        .expect("stdout is UTF-8")
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|value| value["id"] == json!(2))
        .expect("the session answers tools/list with id 2");

    reply["result"]["tools"]
        .as_array()
        .expect("tools/list returns an array of tools")
        .iter()
        .map(|tool| {
            tool["name"]
                .as_str()
                .expect("every tool carries a name")
                .to_owned()
        })
        .collect()
}

/// Every name in `MCP_TOOLS` resolves to a tool the binary actually serves.
///
/// **This is the half the guard above states it cannot reach, and the statement is about `--help`
/// rather than about the environment (#231).** `MCP_TOOLS` is not a clap subcommand tree, so the
/// `--help` walk genuinely cannot derive it — but the tool surface is a static table in the same
/// binary, and the binary will list it on request. The subset property the CLI half enjoys is
/// therefore available here too, by asking the server instead of the parser.
///
/// What the gap cost while it was open: rename a tool in `commands/mcp/tools.rs` and `MCP_TOOLS`
/// keeps naming the old one. The membership test passes, a skill declaring the dead tool
/// validates, and the check reports success while proving the opposite of its stated purpose.
///
/// **SUBSET, never equality, and that is a decision somebody already recorded.**
/// `apps/cli/tests/development_surface_parity.rs` documents why the two lists must not be bound
/// as equal: `MCP_TOOLS` is a narrow allowlist of the domain surface a skill journey may claim to
/// drive, with mutating and destructive operations excluded ON PURPOSE. The day a destructive tool
/// is added, the two diverge CORRECTLY, and an equality guard would fire on that correct
/// divergence and pressure the next reader to widen the allowlist to silence it -- turning a
/// safety boundary into bookkeeping.
///
/// One direction has neither problem. A name in `MCP_TOOLS` that no tool answers to is wrong under
/// every reading of that boundary: the allowlist may be narrower than the surface, never other
/// than it.
#[test]
fn every_mcp_tools_entry_names_a_tool_the_binary_actually_serves() {
    let (_, mcp_tools) = graphhelm_schema::__surface_allowlists_for_testing();
    let real = real_mcp_tool_names();

    assert!(
        real.len() > 1,
        "HARNESS-BROKE: the binary listed {} tools, so the subset check below would be nearly \
         vacuous. tools/list did not answer with the real table: {real:?}",
        real.len()
    );

    let dead: Vec<&str> = mcp_tools
        .iter()
        .copied()
        .filter(|declared| !real.iter().any(|name| name == declared))
        .collect();

    assert!(
        dead.is_empty(),
        "MCP_TOOLS names {dead:?}, which the binary does not serve -- renamed or removed in \
         commands/mcp/tools.rs without updating the allowlist. A skill journey declaring one of \
         these validates against a tool that cannot be called.\nserved: {real:?}"
    );
}

// -------------------------------------------------------------------------------------------
// #285: publication, hostViews, composition were declared in every manifest's spec.contracts
// and read by nothing. formatVersion, missingCapabilityResult, activation stay advisory
// (design note #285) -- pinned below so a future accidental
// enforcement attempt is a visible test change, not a silent behavior shift.
// -------------------------------------------------------------------------------------------

#[test]
fn a_non_governor_only_publication_is_refused() {
    let mut fixture = valid_package();
    fixture.manifest["spec"]["contracts"]["publication"] = json!("self");
    fixture.write_manifest();
    assert_domain_code(&fixture.run(), "GHEX021_PUBLICATION");
}

#[test]
fn a_non_array_host_views_is_refused() {
    // The real, already-shipped defect: extensions/builtin/graphhelm-jpd/extension.json declared
    // "hostViews": "derived-and-deletable" -- a bare string, not the array shape the field's other
    // shipped instance uses. Reproduced here rather than invented.
    let mut fixture = valid_package();
    fixture.manifest["spec"]["contracts"]["hostViews"] = json!("derived-and-deletable");
    fixture.write_manifest();
    assert_domain_code(&fixture.run(), "GHEX022_HOST_VIEWS");
}

#[test]
fn an_array_host_views_still_validates() {
    let mut fixture = valid_package();
    fixture.manifest["spec"]["contracts"]["hostViews"] = json!(["derived-and-deletable"]);
    fixture.write_manifest();
    let output = fixture.run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

/// Pins the advisory decisions themselves (blueprint §2): any value for a field the sidecar
/// tags advisory currently validates, so a future accidental enforcement attempt is a visible
/// test change, never a silent behavior shift landing without anyone noticing the field started
/// mattering. The population comes from schemas/extension.status.json, not a fourth hand-written
/// copy of it (#318; moved from an inline schema tag to this sidecar by the #339 incident fix --
/// see the comment above extension_status_sidecar()) -- only the hostile VALUE below is chosen
/// per test, since "any value" is the claim being made and the exact string carries no meaning.
#[test]
fn every_schema_advisory_field_is_read_and_ignored() {
    let advisory = contracts_fields_by_status(&extension_status_sidecar(), "advisory");
    assert!(
        !advisory.is_empty(),
        "non-empty-first: see the sentinel test above"
    );
    for field in &advisory {
        let mut fixture = valid_package();
        fixture.manifest["spec"]["contracts"][field] = json!("clearly-not-a-real-value");
        fixture.write_manifest();
        let output = fixture.run();
        assert!(
            output.status.success(),
            "{field} is schema-tagged advisory and must not be enforced yet, got: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
}

/// L's finding on #315: the advisory decisions above are pinned as BEHAVIOR (nothing is
/// enforced), but the REASONING for each -- the wake condition that would flip it to
/// enforcement -- lived only in a doc comment with no reader. Deleting any one field's
/// paragraph left every test green: #285's own defect (declared-and-unexplained reads
/// identical to explained) reproduced one file over. Mirrors
/// `development_sabotage.rs::the_doc_declares_every_criterion_it_does_not_prove`'s pattern:
/// read the real source text, assert each expected fragment is actually there.
///
/// #318: the FIELD NAMES this loop checks come from schemas/extension.status.json (moved there
/// from an inline schema tag by the #339 incident fix), not a third hand-written copy. The
/// wake-condition PROSE cannot come from the sidecar either (it is English reasoning, not data)
/// -- it stays a hand-written map here, one entry per tagged field. A fifth advisory field added
/// to the sidecar without a matching entry in this map is caught by the bidirectional check
/// below, rather than being silently skipped the way a `for` loop over a separate hand array
/// would be.
#[test]
fn the_advisory_field_doc_names_each_field_and_its_wake_condition() {
    let wake_conditions = std::collections::BTreeMap::from([
        (
            "formatVersion",
            "a real design need for a contracts-format version appears",
        ),
        ("missingCapabilityResult", "that registry is built"),
        ("composition", "a documented behavioral distinction"),
        ("activation", "#212 lands its state machine"),
    ]);

    let source = fs::read_to_string(repository_root().join("core/schema/src/extension.rs"))
        .expect("core/schema/src/extension.rs is readable from apps/cli's tests");
    let doc = source
        .split_once("/// #285: six `spec.contracts` fields")
        .and_then(|(_, rest)| rest.split_once("fn validate_contracts_fields"))
        .map(|(doc, _)| doc)
        .expect(
            "the #285 doc comment must still precede validate_contracts_fields, unmoved and \
             unrenamed",
        );

    let advisory = contracts_fields_by_status(&extension_status_sidecar(), "advisory");
    assert!(
        !advisory.is_empty(),
        "non-empty-first: see the sentinel test above"
    );

    // L's finding on #339: the loop below only ever checked advisory ⊆ wake_conditions.keys()
    // (a missing entry panics) -- never the reverse. The live case for the reverse is
    // PROMOTION, not rename: the day an advisory field flips to enforced/structural, its
    // wake_conditions entry becomes orphaned -- a wake condition that ALREADY FIRED, describing
    // a state the system has left -- and this test would keep passing forever on the stale
    // paragraph. Set equality in both directions, both remainders printed
    // (closed-vocabulary-guard-pattern), catches both a new field with no entry and a stale
    // entry for a field no longer advisory.
    let wake_condition_keys: std::collections::BTreeSet<String> =
        wake_conditions.keys().map(|&key| key.to_owned()).collect();
    let missing: Vec<&String> = advisory.difference(&wake_condition_keys).collect();
    let orphaned: Vec<&String> = wake_condition_keys.difference(&advisory).collect();
    assert!(
        missing.is_empty() && orphaned.is_empty(),
        "wake_conditions and the schema's advisory set must match exactly -- schema fields with \
         no registered wake condition (a new advisory field, #318's whole point): {missing:?}; \
         wake_conditions entries for fields no longer advisory (promoted to enforced/structural, \
         or removed -- the wake condition already fired, delete the stale entry): {orphaned:?}"
    );

    // #347: the third copy. #318 tagged the population in the schema, #339 made the
    // wake_conditions map answer back to it -- and the doc PROSE, the artifact a human actually
    // reads to learn a field's status, still answered to nothing. On promotion the map's check goes
    // red and the bullet above it keeps saying ADVISORY with a wake condition that already fired.
    //
    // Set equality in both directions, both remainders printed: the same shape as the two checks
    // this file already carries, applied to the copy that had no guard.
    let doc_labels = advisory_fields_the_doc_labels(doc);
    assert!(
        !doc_labels.is_empty(),
        "HARNESS-BROKE: the extractor found no ADVISORY bullets at all, so the comparison below would pass against an empty set -- the bullet shape changed, or the doc slice is wrong"
    );
    let undocumented: Vec<&String> = advisory.difference(&doc_labels).collect();
    let stale: Vec<&String> = doc_labels.difference(&advisory).collect();
    assert!(
        undocumented.is_empty() && stale.is_empty(),
        "the doc's ADVISORY bullets and the schema's advisory set must match exactly.
  advisory in the schema with no ADVISORY bullet (a reader cannot learn why it is advisory): {undocumented:?}
  bullets still calling a field ADVISORY after promotion (the wake condition already fired -- rewrite the bullet to its new verdict): {stale:?}"
    );

    for field in &advisory {
        let wake_condition = wake_conditions[field.as_str()];
        assert!(
            doc.contains(&format!("`{field}`")),
            "the doc never names `{field}`, so a reader cannot find why it is advisory"
        );
        assert!(
            doc.contains(wake_condition),
            "the doc never states {field}'s wake condition ({wake_condition:?}), so deleting \
             the reasoning leaves every test green -- #285's own defect, reproduced here"
        );
    }
}

// -------------------------------------------------------------------------------------------
// #318: the advisory field list above was hand-written in three places (this pin test, the
// doc-guard test, and the doc comment) with nothing tying them together -- a fifth advisory
// field added later without being added to all three would be invisible to both guards, each
// measuring a population its own author wrote (L's finding on #315). The only non-circular
// authority for "which /spec/contracts fields exist" was the manifest schema itself, read here
// as the source, never re-typed (closed-vocabulary-guard-pattern).
//
// #339 INCIDENT: that authority lived as inline `x-graphhelm-status` tags on
// schemas/extension.schema.json -- which broke `checked_in_1_0_0_release_is_complete_and_raw_
// byte_identical` (core/schema-evolution/tests/catalog_integrity.rs), because that file has been
// frozen byte-for-byte since the 1.0.0 release shipped (8ee8f49) and #339 was the first edit to
// it since. The schema is reverted to its exact 1.0.0 bytes; the status annotation moved to
// schemas/extension.status.json, a sidecar that is explicitly allowed to change (its own name
// does not end in .schema.json, so the catalog's inventory scan never sees it). Same discipline,
// different, mutable source.
// -------------------------------------------------------------------------------------------

fn extension_status_sidecar() -> Value {
    serde_json::from_str(
        &fs::read_to_string(repository_root().join("schemas/extension.status.json"))
            .expect("schemas/extension.status.json is readable"),
    )
    .expect("schemas/extension.status.json is valid JSON")
}

/// Every field name the sidecar declares a status for -- the full set `contracts_fields_by_status`
/// partitions.
fn contracts_field_names(sidecar: &Value) -> std::collections::BTreeSet<String> {
    sidecar["fields"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, _)| name.clone())
        .collect()
}

/// Reads the sidecar's `fields` map and returns the names carrying the given status.
fn contracts_fields_by_status(sidecar: &Value, status: &str) -> std::collections::BTreeSet<String> {
    sidecar["fields"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(_, value)| *value == status)
        .map(|(name, _)| name.clone())
        .collect()
}

/// L's finding on #339: `contracts_fields_by_status` only sees a field if it carries a
/// RECOGNIZED status. A field added to the sidecar with no status (or a typo'd one) matches
/// none of "advisory"/"enforced"/"structural", and silently drops out of every population this
/// file derives from it. This is #318's own defect ("population is the instrument") one layer
/// up: the sidecar is meant to be the one non-circular authority, but only for the fields it
/// remembers to tag. This does NOT close the schema's own residual gap: `core/schema/src/
/// extension.rs` reads `contracts.get(...)` for specific field names directly (`publication`,
/// `hostViews`, `artifactFlows`) -- a field a future validator reads this way without ever
/// adding it to the sidecar stays invisible to this whole scheme, tagged or not. That is a
/// second authority, untouched by this test.
#[test]
fn every_contracts_property_carries_a_recognized_status() {
    let sidecar = extension_status_sidecar();
    let all = contracts_field_names(&sidecar);
    assert!(
        !all.is_empty(),
        "non-empty-first: schemas/extension.status.json declared zero fields -- extraction is \
         broken"
    );
    let tagged: std::collections::BTreeSet<String> = ["advisory", "enforced", "structural"]
        .into_iter()
        .flat_map(|status| contracts_fields_by_status(&sidecar, status))
        .collect();
    let untagged: Vec<&String> = all.difference(&tagged).collect();
    let unexpected: Vec<&String> = tagged.difference(&all).collect();
    assert!(
        untagged.is_empty() && unexpected.is_empty(),
        "every field in schemas/extension.status.json must carry a status in \
         {{advisory, enforced, structural}} -- untagged or mistagged: {untagged:?}; tagged but \
         not a real field (should be impossible by construction, name the bug if non-empty): \
         {unexpected:?}"
    );
}

#[test]
fn the_sidecar_names_a_non_empty_advisory_field_set() {
    let advisory = contracts_fields_by_status(&extension_status_sidecar(), "advisory");
    assert!(
        !advisory.is_empty(),
        "no field in schemas/extension.status.json is tagged advisory -- either the tagging has \
         not been added yet, or the extraction path itself is broken (non-empty-first: an empty \
         set passes every equality check vacuously)"
    );
}

/// The field names the #285 doc block itself LABELS advisory, extracted from the bullet shape the
/// block already uses uniformly: `- ` + a backticked field + exactly `: ADVISORY.`.
///
/// Anchored at the bullet, not searched free-text, and that is the whole difference between this
/// and a brittle scan: a continuation line, a wrapped sentence, or the word ADVISORY appearing in
/// prose cannot produce a name, because none of them begin a bullet. The cost of the anchoring is
/// stated where it lives -- if the block ever adopts a second bullet shape, this returns fewer
/// names and the set comparison below fails LOUDLY rather than silently under-reporting.
fn advisory_fields_the_doc_labels(doc: &str) -> std::collections::BTreeSet<String> {
    doc.lines()
        .filter_map(|line| {
            let bullet = line.trim_start().strip_prefix("/// - `")?;
            let (field, rest) = bullet.split_once('`')?;
            rest.starts_with(": ADVISORY.").then(|| field.to_owned())
        })
        .collect()
}
