use std::collections::BTreeMap;

use graphhelm_protocols::FixtureOutcome;
use serde::{Deserialize, Serialize};

/// Explicit deterministic inputs for effect-free graph simulation.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimulationFixtures {
    #[serde(default)]
    pub node_outcomes: BTreeMap<String, FixtureOutcome>,
    #[serde(default)]
    pub conditions: BTreeMap<String, bool>,
}
