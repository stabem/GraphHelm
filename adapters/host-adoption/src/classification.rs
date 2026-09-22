//! Conservative local proposal generation; no model is consulted in this slice.

use graphhelm_policy::adoption::{Decision, decision_allowed};
use graphhelm_protocols::adoption::{AdoptionError, AdoptionReason};
use serde_json::{Value, json};

/// Creates a review-only plan. Unknown instruction text remains unresolved.
pub fn propose(inventory: &Value) -> Result<Value, AdoptionError> {
    let hosts = inventory
        .pointer("/spec/hosts")
        .and_then(Value::as_array)
        .ok_or(AdoptionError {
            reason: AdoptionReason::InvalidConfiguration,
        })?;
    let mut decisions = Vec::new();
    for host in hosts {
        let name = host
            .get("host")
            .and_then(Value::as_str)
            .ok_or(AdoptionError {
                reason: AdoptionReason::InvalidConfiguration,
            })?;
        let items = host
            .get("items")
            .and_then(Value::as_array)
            .ok_or(AdoptionError {
                reason: AdoptionReason::InvalidConfiguration,
            })?;
        for item in items {
            let kind = item
                .get("kind")
                .and_then(Value::as_str)
                .ok_or(AdoptionError {
                    reason: AdoptionReason::InvalidConfiguration,
                })?;
            let protected =
                kind.ends_with("settings.json") || kind.ends_with("settings.local.json");
            let decision = Decision::Unresolved;
            if !decision_allowed(protected, decision) {
                return Err(AdoptionError {
                    reason: AdoptionReason::InvalidConfiguration,
                });
            }
            decisions.push(json!({"host": name, "item": kind, "decision": "unresolved", "reason": "owner_review_required"}));
        }
    }
    Ok(json!({
        "apiVersion": "p50.dev/adoption/v1",
        "kind": "AdoptionPlan",
        "id": "plan/local-preview",
        "spec": {"decisions": decisions, "applyAllowed": false}
    }))
}
