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
        if let Some(name) = line.trim_start().split_whitespace().next() {
            if name != "help" {
                names.push(name.to_owned());
            }
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
    // derives CLI_COMMANDS's world cannot also derive MCP_TOOLS's. What IS checkable from here,
    // cheaply: `mcp` itself must still be a real command, since MCP_TOOLS's whole premise is that
    // requests arrive through it.
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
