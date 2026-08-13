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

/// What happened to a node, as reported by the executor or the owner.
///
/// This is a wire vocabulary because `node_outcome_recorded` carries it. `core/execution` holds the
/// rules that interpret it and re-exports this type rather than defining a second one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeOutcome {
    /// The scheduler dispatched the node.
    Started,
    Succeeded,
    /// Failed in a way a further attempt could resolve.
    RetryableFailure,
    /// Failed in a way no further attempt can resolve.
    TerminalFailure,
    /// The node is waiting on input that has not arrived.
    NeedsInput,
    /// The node is waiting on capacity, such as an exhausted subscription quota.
    NeedsCapacity,
    /// The owner approved a proposed expansion.
    Approved,
    /// The owner waived the obligation blocking this node.
    Waived,
    /// The owner or a dependency failure removed this node from the run.
    Skipped,
    Cancelled,
    /// An upstream change invalidated a completed node's output.
    Invalidated,
    /// The owner paused work that had not started. Only `Ready` and `Queued` nodes pause; a
    /// running node in this milestone completes instantly, and interrupting real work is
    /// Milestone 05's problem.
    Paused,
    /// The execution stopped while this node was running, so its effects are unknown. The only
    /// legal consequence is `Blocked`: nothing may resume a node whose effects are unknown.
    Interrupted,
}

/// How much autonomy the owner has granted this execution, per D-022.
///
/// The vocabulary is closed: an unrecognized mode must fail deserialization rather than default to
/// the permissive one, because defaulting would silently grant autonomy nobody approved. For the
/// same reason this type deliberately implements neither `Default` nor `#[serde(other)]`, and every
/// field carrying it is required — an absent mode must be an error, not `Autopilot`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    /// The Governor may accept its own mutations.
    Autopilot,
    /// The Governor proposes; expansions await owner confirmation according to policy.
    Supervised,
    /// Only the owner changes the graph.
    Manual,
}

/// Signal severity, from `schemas/graph-signal.schema.json`. A wire vocabulary because
/// `signal_recorded` carries it; `core/execution` re-exports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SignalSeverity {
    Low,
    Medium,
    High,
    Critical,
}

/// The closed signal source vocabulary, from `schemas/graph-signal.schema.json`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SignalSourceKind {
    Node,
    Runtime,
    Tool,
    Test,
    User,
    Dream,
    System,
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
    /// Cancelled by the owner. History is not erased and partial effects are recorded, per
    /// `OBSERVABILITY_AND_RECOVERY.md` §13.
    Cancelled,
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
