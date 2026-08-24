//! G5 — an unreadable source does not become a partial contract.
//!
//! Step 1 of the design's resolution order is *validate every source before interpretation*, and
//! the failure mode it guards is not a crash: it is a contract that looks complete while one of
//! its inputs was silently dropped. A resolver written with `filter_map` does exactly that, and
//! nothing downstream can tell the difference between "these were all the rules" and "these were
//! the rules that happened to parse".

use graphhelm_policy::{ResolutionRefusal, resolve_code_contract};
use graphhelm_protocols::{
    ArtifactId, DevelopmentEnvelope, DevelopmentKind, DevelopmentMetadata, DevelopmentScope,
    OpaqueId, ProjectId, SemanticVersion, WireHash, WorkspaceId,
};

fn clock() -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339("2026-08-24T12:00:00Z")
        .expect("fixed clock")
        .with_timezone(&chrono::Utc)
}

fn envelope(id: &str, spec: serde_json::Value) -> DevelopmentEnvelope {
    DevelopmentEnvelope {
        api_version: "p50.dev/development/v1alpha1".to_owned(),
        kind: DevelopmentKind::CodeRule,
        metadata: DevelopmentMetadata {
            id: ArtifactId::parse(id).expect("artifact id"),
            artifact_version: SemanticVersion::parse("1.0.0").expect("artifact version"),
            scope: DevelopmentScope {
                workspace_id: WorkspaceId::parse("workspace-1").expect("workspace"),
                project_id: ProjectId::parse("project-1").expect("project"),
                subproject_id: None,
                execution_id: None,
            },
        },
        producer: OpaqueId::parse("graphhelm-policy").expect("producer"),
        producer_version: SemanticVersion::parse("0.1.0").expect("producer version"),
        bindings: Vec::new(),
        spec,
        digest: WireHash::parse(format!("sha256:{}", "a".repeat(64))).expect("digest"),
        additional: serde_json::Map::new(),
    }
}

fn a_readable_rule(id: &str, key: &str, minimum: u32) -> DevelopmentEnvelope {
    envelope(
        id,
        serde_json::json!({ "conflictKey": key, "minimum": minimum }),
    )
}

#[test]
fn one_unreadable_source_refuses_instead_of_yielding_the_rules_that_parsed() {
    let sources = vec![
        a_readable_rule("code-rule-1", "line-length", 100),
        // Not a rule: the shape is wrong, and the resolver has no basis to guess what it meant.
        envelope("code-rule-2", serde_json::json!({ "statement": "be nice" })),
    ];

    let outcome = resolve_code_contract(&sources, clock());

    // The refusal is the POINT. A resolver that returned Ok here would be reporting a contract
    // built from one rule while two were declared, and every consumer of that contract would be
    // trusting a completeness that was never checked.
    let refusal = outcome.expect_err(
        "the resolver accepted a source it could not read and returned a contract anyway — the \
         contract would claim to cover a rule set it silently narrowed",
    );

    assert_eq!(
        refusal,
        ResolutionRefusal::SourceUnavailable {
            artifact: "code-rule-2".to_owned()
        },
        "the refusal must name WHICH source could not be read; a bare refusal sends the caller \
         back to bisecting the input, which is the cost #247 records"
    );
}

#[test]
fn every_source_readable_resolves_without_refusing() {
    // POSITIVE CONTROL. Without it, a resolver that refuses EVERYTHING passes the cell above.
    let sources = vec![
        a_readable_rule("code-rule-1", "line-length", 100),
        a_readable_rule("code-rule-2", "line-length", 120),
    ];

    let contract = resolve_code_contract(&sources, clock()).expect("every source here is readable");

    assert_eq!(
        contract.requirements.get("line-length"),
        Some(&120),
        "the stronger of two minimums on one key must win"
    );
    // Both rules are on ONE key with the same (empty) selector, so the stronger contributes and
    // the other is SHADOWED. Asserting `included.len() == 2` was right when `included` meant
    // "parsed"; it now means "contributed a requirement", and the record has to account for both
    // rules across its fields rather than pile them into one.
    assert_eq!(contract.record.included, vec!["code-rule-2".to_owned()]);
    assert_eq!(contract.record.shadowed, vec!["code-rule-1".to_owned()]);
}
