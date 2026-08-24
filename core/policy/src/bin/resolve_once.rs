//! Resolves one fixed rule set and writes the contract's canonical bytes to stdout.
//!
//! A separate BINARY rather than a re-entrant test process: G7's whole point is a fresh process
//! with a fresh hash seed, and a test binary spawned with foreign argv fights libtest's own
//! argument parser.

use graphhelm_protocols::{
    ArtifactId, DevelopmentEnvelope, DevelopmentKind, DevelopmentMetadata, DevelopmentScope,
    OpaqueId, ProjectId, SemanticVersion, WireHash,
};

fn rule(index: usize) -> DevelopmentEnvelope {
    DevelopmentEnvelope {
        api_version: "p50.dev/development/v1alpha1".to_owned(),
        kind: DevelopmentKind::CodeRule,
        metadata: DevelopmentMetadata {
            id: ArtifactId::parse(format!("rule-{index:02}")).expect("artifact id"),
            artifact_version: SemanticVersion::parse("1.0.0").expect("artifact version"),
            scope: DevelopmentScope {
                workspace_id: graphhelm_protocols::WorkspaceId::parse("workspace-1")
                    .expect("workspace"),
                project_id: ProjectId::parse("project-1").expect("project"),
                subproject_id: None,
                execution_id: None,
            },
        },
        producer: OpaqueId::parse("graphhelm-policy").expect("producer"),
        producer_version: SemanticVersion::parse("0.1.0").expect("producer version"),
        bindings: Vec::new(),
        spec: serde_json::json!({
            "conflictKey": format!("key-{index:02}"),
            "minimum": index,
        }),
        digest: WireHash::parse(format!("sha256:{}", "a".repeat(64))).expect("digest"),
        additional: serde_json::Map::new(),
    }
}

fn main() {
    let count: usize = std::env::args()
        .nth(1)
        .expect("resolve_once needs a rule count")
        .parse()
        .expect("the rule count must be a number");

    let sources: Vec<DevelopmentEnvelope> = (0..count).map(rule).collect();
    // The clock is an INPUT, and the fixture pins it: a resolver that read the system clock
    // would make two processes disagree for a reason that is not in the rule set.
    let clock = chrono::DateTime::parse_from_rfc3339("2026-08-24T12:00:00Z")
        .expect("fixed clock")
        .with_timezone(&chrono::Utc);
    let contract = graphhelm_policy::resolve_code_contract(&sources, clock)
        .expect("the fixture's sources are all readable");
    std::io::Write::write_all(&mut std::io::stdout(), &contract.canonical_bytes())
        .expect("writing the contract to stdout");
}
