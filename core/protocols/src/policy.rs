use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{Actor, Diagnostic};

/// Scope accepted by the checked-in waiver schema.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaiverScope {
    Node,
    Branch,
    Execution,
}

/// An explicit owner request. It cannot waive structural or hard-policy failures.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualOverride {
    pub actor: Actor,
    pub reason: String,
    pub waived_requirements: Vec<String>,
    pub acknowledged_risks: Vec<String>,
    pub scope: WaiverScope,
}

/// Deterministic policy resolution state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObligationStatus {
    Satisfied,
    Unsatisfied,
    Waived,
    Impossible,
}

/// One policy requirement and the evidence supporting its status.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyObligation {
    pub requirement: String,
    pub status: ObligationStatus,
    #[serde(default)]
    pub evidence: Vec<String>,
    pub reason: String,
    pub overrideable: bool,
}

/// Complete transition decision, kept in stable requirement order.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyReport {
    pub obligations: Vec<PolicyObligation>,
    pub diagnostics: Vec<Diagnostic>,
    pub result_status: String,
}

impl PolicyReport {
    #[must_use]
    pub fn requirement(&self, name: &str) -> Option<&PolicyObligation> {
        self.obligations
            .iter()
            .find(|item| item.requirement == name)
    }

    #[must_use]
    pub fn allows_transition(&self) -> bool {
        self.obligations.iter().all(|item| {
            matches!(
                item.status,
                ObligationStatus::Satisfied | ObligationStatus::Waived
            )
        })
    }
}

/// Persisted evidence that an owner accepted a logical quality risk.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyWaiver {
    pub id: String,
    pub requirement: String,
    pub execution_id: String,
    pub graph_version: u64,
    pub actor: String,
    #[serde(default)]
    pub reason: Option<String>,
    pub acknowledged_risks: Vec<String>,
    pub scope: WaiverScope,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
}
