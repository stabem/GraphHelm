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

/// #1357: the one test fake for [`Clock`]: always answers the instant it holds. Tests that need a
/// moving clock keep their own (a lapse, a counter); a fixed instant is this.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FixedClock(pub DateTime<Utc>);

impl Clock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        self.0
    }
}

impl From<crate::PersistedTimestamp> for FixedClock {
    fn from(timestamp: crate::PersistedTimestamp) -> Self {
        Self(*timestamp.as_datetime())
    }
}

/// Injected identifier source, avoiding hidden randomness in domain logic.
pub trait IdGenerator: Send + Sync {
    fn next_id(&self, prefix: &'static str) -> String;
}

#[cfg(test)]
mod clock_tests {
    use super::{Clock, FixedClock};
    use crate::PersistedTimestamp;
    use chrono::{TimeZone, Utc};

    /// #1357: the fake answers the instant it was built from, and the one conversion
    /// `RetentionService` makes refuses only a time outside the schema format.
    #[test]
    fn fixed_clock_answers_its_instant_and_the_persisted_form_bounds_the_year() {
        let at = PersistedTimestamp::parse("2026-10-15T12:00:00Z").unwrap();
        assert_eq!(FixedClock::from(at.clone()).now(), *at.as_datetime());
        let far = Utc.with_ymd_and_hms(10_000, 1, 1, 0, 0, 0).unwrap();
        assert!(PersistedTimestamp::from_datetime(FixedClock(far).now()).is_err());
        let near = Utc.with_ymd_and_hms(9_999, 12, 31, 23, 59, 59).unwrap();
        assert!(PersistedTimestamp::from_datetime(FixedClock(near).now()).is_ok());
    }
}
