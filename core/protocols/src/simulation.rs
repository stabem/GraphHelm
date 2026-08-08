use serde::{Deserialize, Serialize};

/// Runtime node states with exact, stable snake-case wire names.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeState {
    Draft,
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
