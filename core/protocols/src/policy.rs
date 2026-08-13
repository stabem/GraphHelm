use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{
    Actor, ActorId, Diagnostic, OpaqueId, PersistedTimestamp, PersistenceError,
    deserialize_required_nullable, parse_timestamp, valid_bounded_text,
    valid_positive_safe_integer,
};

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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicyWaiver {
    pub id: String,
    pub requirement: String,
    pub execution_id: String,
    pub graph_version: u64,
    pub actor: String,
    pub reason: Option<String>,
    pub acknowledged_risks: Vec<String>,
    pub scope: WaiverScope,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SerializablePolicyWaiver<'a> {
    id: &'a str,
    requirement: &'a str,
    execution_id: &'a str,
    graph_version: u64,
    actor: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'a str>,
    acknowledged_risks: &'a [String],
    scope: &'a WaiverScope,
    created_at: &'a DateTime<Utc>,
    expires_at: &'a Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawPolicyWaiver {
    id: String,
    requirement: String,
    execution_id: String,
    graph_version: u64,
    actor: String,
    #[serde(default, deserialize_with = "optional_non_null_string")]
    reason: Option<String>,
    acknowledged_risks: Vec<String>,
    scope: WaiverScope,
    created_at: String,
    #[serde(deserialize_with = "deserialize_required_nullable")]
    expires_at: Option<String>,
}

fn optional_non_null_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    String::deserialize(deserializer).map(Some)
}

fn validate_policy_waiver(waiver: &PolicyWaiver) -> Result<(), PersistenceError> {
    if OpaqueId::parse(&waiver.id).is_err()
        || OpaqueId::parse(&waiver.requirement).is_err()
        || OpaqueId::parse(&waiver.execution_id).is_err()
        || ActorId::parse(&waiver.actor).is_err()
        || !valid_positive_safe_integer(waiver.graph_version)
        || waiver
            .reason
            .as_deref()
            .is_some_and(|reason| !valid_bounded_text(reason, 1, 2048))
        || !(1..=64).contains(&waiver.acknowledged_risks.len())
        || waiver
            .acknowledged_risks
            .iter()
            .any(|risk| !valid_bounded_text(risk, 1, 512))
        || PersistedTimestamp::from_datetime(waiver.created_at).is_err()
        || waiver
            .expires_at
            .is_some_and(|expires_at| PersistedTimestamp::from_datetime(expires_at).is_err())
    {
        return Err(PersistenceError::new("policy waiver"));
    }
    Ok(())
}

impl Serialize for PolicyWaiver {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        validate_policy_waiver(self).map_err(serde::ser::Error::custom)?;
        SerializablePolicyWaiver {
            id: &self.id,
            requirement: &self.requirement,
            execution_id: &self.execution_id,
            graph_version: self.graph_version,
            actor: &self.actor,
            reason: self.reason.as_deref(),
            acknowledged_risks: &self.acknowledged_risks,
            scope: &self.scope,
            created_at: &self.created_at,
            expires_at: &self.expires_at,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for PolicyWaiver {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawPolicyWaiver::deserialize(deserializer)?;
        let created_at = parse_timestamp(&raw.created_at).map_err(serde::de::Error::custom)?;
        let expires_at = raw
            .expires_at
            .as_deref()
            .map(parse_timestamp)
            .transpose()
            .map_err(serde::de::Error::custom)?;
        let waiver = Self {
            id: raw.id,
            requirement: raw.requirement,
            execution_id: raw.execution_id,
            graph_version: raw.graph_version,
            actor: raw.actor,
            reason: raw.reason,
            acknowledged_risks: raw.acknowledged_risks,
            scope: raw.scope,
            created_at,
            expires_at,
        };
        validate_policy_waiver(&waiver).map_err(serde::de::Error::custom)?;
        Ok(waiver)
    }
}
