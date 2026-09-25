//! Deterministic limits on adoption-plan decisions.

use serde::{Deserialize, Serialize};

/// Every required observation must be present; an empty evidence set proves nothing.
#[must_use]
pub fn adoption_verified(required: &[bool]) -> bool {
    !required.is_empty() && required.iter().all(|observed| *observed)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Keep,
    Disable,
    Replace,
    Unresolved,
}

/// A classifier may only preserve or defer a protected rule.
#[must_use]
pub const fn decision_allowed(protected: bool, decision: Decision) -> bool {
    !protected || matches!(decision, Decision::Keep | Decision::Unresolved)
}

/// Consent is valid only for the exact, nonempty reviewed plan digest.
#[must_use]
pub fn approval_matches(actual: &str, accepted: &str) -> bool {
    !actual.is_empty() && actual == accepted
}

/// Chooses a single owned value during restore without replacing a later user edit.
pub fn restore_value(
    base: Option<&serde_json::Value>,
    installed: Option<&serde_json::Value>,
    current: Option<&serde_json::Value>,
) -> Result<Option<serde_json::Value>, &'static str> {
    if current == installed || current == base {
        return Ok(base.cloned());
    }
    if base == installed {
        return Ok(current.cloned());
    }
    Err("restore_conflict")
}
