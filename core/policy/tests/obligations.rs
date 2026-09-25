use std::path::Path;

use chrono::{TimeZone, Utc};
use graphhelm_graph::GraphVersion;
use graphhelm_policy::evaluate_transition;
use graphhelm_protocols::{
    Actor, ActorType, ManualOverride, NodeType, ObligationStatus, WaiverScope,
};

fn version() -> GraphVersion {
    let mut graph = graphhelm_schema::load_graph(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/graphs/software-feature.yaml"),
    )
    .unwrap()
    .graph;
    graph.spec.nodes.get_mut("review").unwrap().node_type = NodeType::Gate;
    GraphVersion::publish(
        graph,
        None,
        Actor::new(ActorType::Owner, "owner-local"),
        Utc.with_ymd_and_hms(2026, 8, 8, 12, 0, 0).unwrap(),
    )
    .unwrap()
}

fn owner_override(requirement: &str) -> ManualOverride {
    ManualOverride {
        actor: Actor::new(ActorType::Owner, "owner-local"),
        reason: "risk explicitly accepted".into(),
        waived_requirements: vec![requirement.into()],
        acknowledged_risks: vec!["quality gate bypassed".into()],
        scope: WaiverScope::Execution,
    }
}

#[test]
fn removed_review_is_unsatisfied_then_waived_only_by_complete_owner_override() {
    let base = version();
    let mut candidate = base.graph().clone();
    candidate.spec.nodes.remove("review");
    candidate
        .spec
        .edges
        .retain(|edge| edge.from != "review" && edge.to != "review");

    let absent = evaluate_transition(&base, &candidate, None);
    assert_eq!(
        absent.requirement("review").unwrap().status,
        ObligationStatus::Unsatisfied
    );

    let report = evaluate_transition(&base, &candidate, Some(&owner_override("review")));
    assert_eq!(
        report.requirement("review").unwrap().status,
        ObligationStatus::Waived
    );
    assert_eq!(report.result_status, "completed_with_waivers");
}

/// #129: `complete_owner_override` refuses a request whose `acknowledged_risks` is empty, but
/// nothing checked that the risks it demanded were then recorded anywhere -- an override that
/// happened left no mark of what was acknowledged, only who did it and why. Two risks, not one,
/// so a fix that only echoes the first (or a constant string) still fails this.
#[test]
fn a_waived_obligation_names_every_acknowledged_risk_not_just_the_actor_and_reason() {
    let base = version();
    let mut candidate = base.graph().clone();
    candidate.spec.nodes.remove("review");
    candidate
        .spec
        .edges
        .retain(|edge| edge.from != "review" && edge.to != "review");
    let mut request = owner_override("review");
    request.acknowledged_risks = vec![
        "quality gate bypassed".into(),
        "no independent review before merge".into(),
    ];

    let report = evaluate_transition(&base, &candidate, Some(&request));
    assert!(report.allows_transition());
    let evidence = &report.requirement("review").unwrap().evidence;
    for risk in &request.acknowledged_risks {
        let marker = format!("owner-override-acknowledged-risk:{risk}");
        assert!(
            evidence.contains(&marker),
            "waived obligation's evidence {evidence:?} does not name acknowledged risk {risk:?}"
        );
    }
}

#[test]
fn non_owner_or_incomplete_override_cannot_waive_quality_gate() {
    let base = version();
    let mut candidate = base.graph().clone();
    candidate.spec.nodes.remove("review");
    candidate
        .spec
        .edges
        .retain(|edge| edge.from != "review" && edge.to != "review");
    let mut request = owner_override("review");
    request.actor.actor_type = ActorType::Agent;
    request.acknowledged_risks.clear();

    let report = evaluate_transition(&base, &candidate, Some(&request));
    assert_eq!(
        report.requirement("review").unwrap().status,
        ObligationStatus::Unsatisfied
    );
}

#[test]
fn oversized_manual_overrides_are_rejected_before_obligation_evidence_is_built() {
    let base = version();
    for (field, size) in [
        ("waivedRequirements", 65),
        ("acknowledgedRisks", 65),
        ("actor/id", 257),
        ("reason", 2049),
        ("requirementText", 129),
        ("riskText", 513),
    ] {
        let mut request = owner_override("review");
        let path = match field {
            "waivedRequirements" => {
                request.waived_requirements = vec!["review".into(); size];
                "/manualOverride/waivedRequirements"
            }
            "acknowledgedRisks" => {
                request.acknowledged_risks = vec!["risk".into(); size];
                "/manualOverride/acknowledgedRisks"
            }
            "actor/id" => {
                request.actor.id = "a".repeat(size);
                "/manualOverride/actor/id"
            }
            "reason" => {
                request.reason = "r".repeat(size);
                "/manualOverride/reason"
            }
            "requirementText" => {
                request.waived_requirements = vec!["r".repeat(size)];
                "/manualOverride/waivedRequirements"
            }
            "riskText" => {
                request.acknowledged_risks = vec!["r".repeat(size)];
                "/manualOverride/acknowledgedRisks"
            }
            _ => unreachable!(),
        };
        let report = evaluate_transition(&base, base.graph(), Some(&request));
        assert_eq!(report.result_status, "blocked", "{field}");
        assert!(!report.allows_transition(), "{field}");
        assert_eq!(report.obligations.len(), 1, "{field}");
        assert_eq!(report.obligations[0].requirement, "manual_override_limits");
        assert_eq!(report.obligations[0].status, ObligationStatus::Impossible);
        assert!(!report.obligations[0].overrideable);
        assert_eq!(report.diagnostics.len(), 1, "{field}");
        assert_eq!(report.diagnostics[0].code, "GHP001_OVERRIDE_LIMIT_EXCEEDED");
        assert_eq!(report.diagnostics[0].path, path);
    }
}

#[test]
fn maximum_override_bounds_preserve_unicode_risks() {
    let base = version();
    let mut candidate = base.graph().clone();
    candidate.spec.nodes.remove("review");
    candidate
        .spec
        .edges
        .retain(|edge| edge.from != "review" && edge.to != "review");
    let mut request = owner_override("review");
    request.actor.id = "a".repeat(256);
    request.reason = "r".repeat(2048);
    request.waived_requirements = vec!["review".into(); 64];
    request.acknowledged_risks = vec!["é".repeat(512); 64];
    let report = evaluate_transition(&base, &candidate, Some(&request));
    let obligation = report.requirement("review").unwrap();
    assert_eq!(obligation.status, ObligationStatus::Waived);
    assert_eq!(
        obligation
            .evidence
            .iter()
            .filter(|item| item.starts_with("owner-override-acknowledged-risk:"))
            .count(),
        64
    );
    assert!(obligation.evidence.contains(&format!(
        "owner-override-acknowledged-risk:{}",
        "é".repeat(512)
    )));
}

#[test]
fn missing_deploy_target_is_impossible_even_with_override() {
    let base = version();
    let mut candidate = base.graph().clone();
    let deploy = candidate.spec.nodes.get_mut("implement").unwrap();
    deploy.node_type = NodeType::Deploy;
    deploy.properties.remove("targetRef");

    let report = evaluate_transition(&base, &candidate, Some(&owner_override("deploy_target")));
    let obligation = report.requirement("deploy_target").unwrap();
    assert_eq!(obligation.status, ObligationStatus::Impossible);
    assert!(!obligation.overrideable);
    assert_eq!(report.result_status, "blocked");
}

#[test]
fn supplied_manual_override_requirements_are_discovered_in_stable_order() {
    let base = version();
    let candidate = graphhelm_schema::load_graph(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/graphs/manual-override-deploy.yaml"),
    )
    .unwrap()
    .graph;
    let report = evaluate_transition(&base, &candidate, None);
    let logical: Vec<_> = report
        .obligations
        .iter()
        .filter(|item| {
            matches!(
                item.requirement.as_str(),
                "integration_tests" | "independent_security_review"
            )
        })
        .map(|item| item.requirement.as_str())
        .collect();
    assert_eq!(
        logical,
        vec!["independent_security_review", "integration_tests"]
    );
}

#[test]
fn unknown_requested_waiver_is_rejected_instead_of_ignored() {
    let base = version();
    let mut candidate = base.graph().clone();
    candidate.spec.nodes.remove("review");
    candidate
        .spec
        .edges
        .retain(|edge| edge.from != "review" && edge.to != "review");
    let mut request = owner_override("review");
    request
        .waived_requirements
        .push("invented_requirement".into());

    let report = evaluate_transition(&base, &candidate, Some(&request));
    assert_eq!(
        report.requirement("invented_requirement").unwrap().status,
        ObligationStatus::Unsatisfied
    );
    assert_eq!(report.result_status, "blocked");
}

#[test]
fn policy_result_label_is_preserved_after_complete_waiver() {
    let base = version();
    let mut candidate = base.graph().clone();
    candidate.spec.policies.push(serde_json::json!({
        "manualOverride": {
            "bypassedRequirements": ["integration_tests"],
            "resultLabel": "deployed_without_full_validation"
        }
    }));

    let report = evaluate_transition(
        &base,
        &candidate,
        Some(&owner_override("integration_tests")),
    );
    assert_eq!(report.result_status, "deployed_without_full_validation");
}

#[test]
fn unrelated_result_label_cannot_relabel_a_waiver() {
    let base = version();
    let mut candidate = base.graph().clone();
    candidate.spec.policies.push(serde_json::json!({
        "resultLabel": "unrelated_status",
        "bypassedRequirements": ["integration_tests"]
    }));
    let report = evaluate_transition(
        &base,
        &candidate,
        Some(&owner_override("integration_tests")),
    );
    assert_eq!(report.result_status, "completed_with_waivers");
}

#[test]
fn malformed_bypass_array_cannot_supply_result_label() {
    let base = version();
    let mut candidate = base.graph().clone();
    candidate.spec.policies.extend([
        serde_json::json!({"bypassedRequirements": ["integration_tests"]}),
        serde_json::json!({"manualOverride": {
            "bypassedRequirements": [123],
            "resultLabel": "injected_status"
        }}),
    ]);
    let report = evaluate_transition(
        &base,
        &candidate,
        Some(&owner_override("integration_tests")),
    );
    assert_eq!(report.result_status, "completed_with_waivers");
}
