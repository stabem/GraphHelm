use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// The authority class behind a mutation or event.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorType {
    Owner,
    Human,
    Agent,
    System,
}

/// A stable actor identity used in audit records.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Actor {
    #[serde(rename = "type")]
    pub actor_type: ActorType,
    pub id: String,
}

impl Actor {
    #[must_use]
    pub fn new(actor_type: ActorType, id: impl Into<String>) -> Self {
        Self {
            actor_type,
            id: id.into(),
        }
    }

    #[must_use]
    pub fn is_owner(&self) -> bool {
        self.actor_type == ActorType::Owner && !self.id.trim().is_empty()
    }
}

/// Injected wall clock, allowing deterministic tests and replayable decisions.
pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

/// Injected identifier source, avoiding hidden randomness in domain logic.
pub trait IdGenerator: Send + Sync {
    fn next_id(&self, prefix: &'static str) -> String;
}
