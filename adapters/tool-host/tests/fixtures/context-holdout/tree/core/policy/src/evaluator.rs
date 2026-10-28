use std::collections::{BTreeMap, BTreeSet};

use graphhelm_graph::{GraphVersion, lint};
use graphhelm_protocols::{
    Diagnostic, ExecutionGraph, ManualOverride, NodeType, ObligationStatus, PolicyObligation,
    PolicyReport,
};

/// Checks borrowed override sizes before policy evidence or draft candidates are allocated.
/// Uses the durable draft limits; character scans stop at the first character beyond each limit.
/// This checks size only, not owner authority, completeness, or whether a requirement is waivable.
pub fn validate_manual_override_limits(request: &ManualOverride) -> Result<(), Diagnostic> {
    let path = if request.waived_requirements.len() > 64 {
        Some("/manualOverride/waivedRequirements")
    } else if request.acknowledged_risks.len() > 64 {
        Some("/manualOverride/acknowledgedRisks")
    } else if request.actor.id.chars().nth(256).is_some() {
        Some("/manualOverride/actor/id")
    } else if request.reason.chars().nth(2048).is_some() {
        Some("/manualOverride/reason")
    } else if request
        .waived_requirements
        .iter()
        .any(|value| value.chars().nth(128).is_some())
    {
        Some("/manualOverride/waivedRequirements")
    } else if request
        .acknowledged_risks
        .iter()
        .any(|value| value.chars().nth(512).is_some())
    {
        Some("/manualOverride/acknowledgedRisks")
    } else {
        None
    };
    match path {
        Some(path) => Err(Diagnostic::error(
            "GHP001_OVERRIDE_LIMIT_EXCEEDED",
            "manual override exceeds the supported collection or text limit",
            path,
            "manualOverride",
        )),
        None => Ok(()),
    }
}

/// Evaluates a candidate transition without side effects or ambient authority.
#[must_use]
pub fn evaluate_transition(
    base: &GraphVersion,
    candidate: &ExecutionGraph,
    manual_override: Option<&ManualOverride>,
) -> PolicyReport {
    if let Some(request) = manual_override
        && let Err(diagnostic) = validate_manual_override_limits(request)
    {
        return PolicyReport {
            // Keep the public decision predicate fail-closed without allocating
            // evidence from the oversized request or changing empty-report semantics.
            obligations: vec![PolicyObligation {
                requirement: "manual_override_limits".into(),
                status: ObligationStatus::Impossible,
                evidence: vec![format!(
                    "diagnostic:{}:{}",
                    diagnostic.code, diagnostic.path
                )],
                reason: diagnostic.message.clone(),
                overrideable: false,
            }],
            diagnostics: vec![diagnostic],
            result_status: "blocked".into(),
        };
    }
    let mut obligations = BTreeMap::<String, PolicyObligation>::new();

    for (id, node) in &base.graph().spec.nodes {
        if node.node_type == NodeType::Gate
            || node.optionality == graphhelm_protocols::Optionality::Required
        {
            let present = candidate.spec.nodes.contains_key(id);
            obligations.insert(
                id.clone(),
                PolicyObligation {
                    requirement: id.clone(),
                    status: if present {
                        ObligationStatus::Satisfied
                    } else {
                        ObligationStatus::Unsatisfied
                    },
                    evidence: vec![format!("graph-node:{id}")],
                    reason: if present {
                        "required predecessor obligation is preserved".into()
                    } else {
                        "required predecessor obligation was removed".into()
                    },
                    overrideable: true,
                },
            );
        }
    }

    for requirement in bypassed_requirements(candidate) {
        obligations
            .entry(requirement.clone())
            .or_insert(PolicyObligation {
                requirement,
                status: ObligationStatus::Unsatisfied,
                evidence: vec!["candidate-policy:bypassedRequirements".into()],
                reason: "candidate declares this quality requirement bypassed".into(),
                overrideable: true,
            });
    }

    let lint_report = lint(candidate, "candidate");
    for diagnostic in &lint_report.errors {
        let requirement = structural_requirement(&diagnostic.code);
        obligations.insert(
            requirement.clone(),
            PolicyObligation {
                requirement,
                status: ObligationStatus::Impossible,
                evidence: vec![format!(
                    "diagnostic:{}:{}",
                    diagnostic.code, diagnostic.path
                )],
                reason: diagnostic.message.clone(),
                overrideable: false,
            },
        );
    }

    if let Some(request) = manual_override.filter(|request| complete_owner_override(request)) {
        let requested: BTreeSet<_> = request
            .waived_requirements
            .iter()
            .map(String::as_str)
            .collect();
        for requirement in &request.waived_requirements {
            obligations
                .entry(requirement.clone())
                .or_insert(PolicyObligation {
                    requirement: requirement.clone(),
                    status: ObligationStatus::Unsatisfied,
                    evidence: vec!["owner-override:unknown-requirement".into()],
                    reason: "override names no discovered policy obligation".into(),
                    overrideable: false,
                });
        }
        for obligation in obligations.values_mut() {
            if obligation.overrideable
                && obligation.status == ObligationStatus::Unsatisfied
                && requested.contains(obligation.requirement.as_str())
            {
                obligation.status = ObligationStatus::Waived;
                obligation.reason = request.reason.clone();
                obligation
                    .evidence
                    .push(format!("owner-override:{}", request.actor.id));
                // `complete_owner_override` requires a non-empty `acknowledged_risks` before an
                // override is honored at all, but until now nothing carried the list itself past
                // that check: a waived obligation named who overrode it and why, never what they
                // said they were accepting. An override that happened left no mark of what was
                // acknowledged (#129).
                for risk in &request.acknowledged_risks {
                    obligation
                        .evidence
                        .push(format!("owner-override-acknowledged-risk:{risk}"));
                }
            }
        }
    }

    let obligations: Vec<_> = obligations.into_values().collect();
    let blocked = obligations.iter().any(|item| {
        matches!(
            item.status,
            ObligationStatus::Unsatisfied | ObligationStatus::Impossible
        )
    });
    let waived = obligations
        .iter()
        .any(|item| item.status == ObligationStatus::Waived);
    let result_status = if blocked {
        "blocked"
    } else if waived {
        result_label(candidate, &obligations).unwrap_or("completed_with_waivers")
    } else {
        "ready"
    };
    PolicyReport {
        obligations,
        diagnostics: lint_report.errors,
        result_status: result_status.into(),
    }
}

fn result_label<'a>(
    graph: &'a ExecutionGraph,
    obligations: &[PolicyObligation],
) -> Option<&'a str> {
    fn find<'a>(value: &'a serde_json::Value, obligations: &[PolicyObligation]) -> Option<&'a str> {
        match value {
            serde_json::Value::Object(object) => {
                if let Some(override_policy) = object
                    .get("manualOverride")
                    .and_then(serde_json::Value::as_object)
                {
                    let requirements = override_policy
                        .get("bypassedRequirements")
                        .and_then(serde_json::Value::as_array)?;
                    let names: Option<Vec<_>> = requirements
                        .iter()
                        .map(|value| value.as_str().filter(|name| !name.is_empty()))
                        .collect();
                    let applicable = names.as_ref().is_some_and(|names| {
                        !names.is_empty()
                            && names.iter().all(|requirement| {
                                obligations.iter().any(|obligation| {
                                    obligation.requirement == *requirement
                                        && obligation.status == ObligationStatus::Waived
                                })
                            })
                    });
                    if applicable {
                        return override_policy
                            .get("resultLabel")
                            .and_then(serde_json::Value::as_str);
                    }
                }
                object.values().find_map(|child| find(child, obligations))
            }
            serde_json::Value::Array(items) => {
                items.iter().find_map(|child| find(child, obligations))
            }
            _ => None,
        }
    }
    graph
        .spec
        .policies
        .iter()
        .find_map(|policy| find(policy, obligations))
}

fn complete_owner_override(request: &ManualOverride) -> bool {
    request.actor.is_owner()
        && !request.reason.trim().is_empty()
        && !request.waived_requirements.is_empty()
        && !request.acknowledged_risks.is_empty()
}

fn structural_requirement(code: &str) -> String {
    match code {
        "GHG009_DEPLOY_TARGET_MISSING" => "deploy_target".into(),
        "GHG006_UNCONTROLLED_CYCLE" => "controlled_cycle".into(),
        "GHG010_COMPENSATION_MISSING" => "compensation".into(),
        "GHG008_INLINE_SECRET" => "inline_secret".into(),
        "GHG014_HARD_POLICY_DENIED" => "hard_policy".into(),
        other => other.to_ascii_lowercase(),
    }
}

fn bypassed_requirements(graph: &ExecutionGraph) -> BTreeSet<String> {
    let mut requirements = BTreeSet::new();
    for policy in &graph.spec.policies {
        collect_bypassed(policy, &mut requirements);
    }
    requirements
}

fn collect_bypassed(value: &serde_json::Value, output: &mut BTreeSet<String>) {
    match value {
        serde_json::Value::Object(object) => {
            if let Some(values) = object
                .get("bypassedRequirements")
                .and_then(serde_json::Value::as_array)
            {
                output.extend(
                    values
                        .iter()
                        .filter_map(serde_json::Value::as_str)
                        .map(str::to_owned),
                );
            }
            for child in object.values() {
                collect_bypassed(child, output);
            }
        }
        serde_json::Value::Array(items) => {
            for child in items {
                collect_bypassed(child, output);
            }
        }
        _ => {}
    }
}
