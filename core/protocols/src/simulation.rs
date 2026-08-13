use serde::{Deserialize, Serialize};

/// Runtime node states with exact, stable snake-case wire names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeState {
    Draft,
    /// A Governor-proposed expansion, visible before approval and never scheduled.
    ///
    /// Approval to [`NodeState::Ready`] is its only legal exit. That single exit is what
    /// guarantees a ghost consumes no execution budget while it is still a proposal.
    Ghost,
    Linting,
    Ready,
    Queued,
    Running,
    WaitingInput,
    WaitingCapacity,
    Paused,
    Blocked,
    Succeeded,
    Failed,
    Waived,
    Skipped,
    Cancelled,
    Invalidated,
}

/// Aggregate simulation state exposed by events and CLI projections.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SimulationStatus {
    Running,
    Completed,
    Failed,
    Paused,
    Blocked,
}

/// A deterministic node outcome supplied to the effect-free simulator.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FixtureOutcome {
    Success,
    Failure,
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::NodeState;

    /// A proposed expansion must be representable in the one shared vocabulary. A second
    /// execution-only enum would let simulation and execution drift apart permanently.
    #[test]
    fn ghost_is_part_of_the_shared_node_state_vocabulary() {
        let ghost = NodeState::Ghost;
        let encoded = serde_json::to_string(&ghost).unwrap();
        assert_eq!(encoded, "\"ghost\"");
        assert_eq!(
            serde_json::from_str::<NodeState>("\"ghost\"").unwrap(),
            ghost
        );
    }
}
