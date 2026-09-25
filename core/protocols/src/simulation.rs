use serde::{Deserialize, Serialize};

wire_vocabulary! {
    /// Runtime node states with exact, stable snake-case wire names.
    ///
    /// **Declared through the vocabulary macro so `every()` comes from the same list as the enum
    /// (#408).** Four consumers used to carry `const [NodeState; 16]` arrays, hand-typed and
    /// independently maintained. The size annotation only forces the LITERAL to hold sixteen
    /// entries; it says nothing about those sixteen being the current set.
    ///
    /// Measured, by adding a seventeenth variant to this enum and building the workspace: the
    /// compiler reported ONE error, a non-exhaustive match in `core/execution/src/attention.rs`.
    /// **None of the four arrays failed.** Each kept sixteen entries, matched its own annotation,
    /// compiled, and silently stopped being exhaustive. The drift was never invisible -- it was
    /// incompletely signalled, which is worse, because the one loud site makes the author believe
    /// they have been told everything.
    ///
    /// The wire spellings were `#[serde(rename_all = "snake_case")]` before and are per-variant
    /// literals now, which is what the macro's own doc argues for: serde is a third producer of
    /// these strings, and left to a naming convention it can drift from `wire_name` without any
    /// equality cell noticing. `every_node_state_serialises_as_its_wire_name` in
    /// `tests/wire_roundtrip.rs` checks all sixteen against each other, closing the sampling gap
    /// that covered four of them.
    NodeState {
        Draft => "draft",
        /// A Governor-proposed expansion, visible before approval and never scheduled.
        ///
        /// Approval to [`NodeState::Ready`] is its only legal exit. That single exit is what
        /// guarantees a ghost consumes no execution budget while it is still a proposal.
        Ghost => "ghost",
        Linting => "linting",
        Ready => "ready",
        Queued => "queued",
        Running => "running",
        WaitingInput => "waiting_input",
        WaitingCapacity => "waiting_capacity",
        Paused => "paused",
        Blocked => "blocked",
        Succeeded => "succeeded",
        Failed => "failed",
        Waived => "waived",
        Skipped => "skipped",
        Cancelled => "cancelled",
        Invalidated => "invalidated",
    }
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
    ///
    /// NO PRODUCTION EMITTER AS OF 0f4e7fe — every occurrence in the tree is a test. #80 relies on
    /// that: it gates dispatch on unsatisfied edges but leaves attention state-only, which is safe
    /// only while no reachable predecessor can gate a dependent without raising a reason of its
    /// own. This outcome is exactly such a predecessor. THE FIRST PRODUCTION EMITTER OF THIS
    /// VARIANT MUST MAKE ATTENTION EDGE-AWARE — see #95, which carries the
    /// clause design ready to apply.
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

/// How a read-only tool call's result may be reused, per `AGENTS_SKILLS_PLUGINS.md` §11.3 as
/// amended by the context-economy wave: `immutable_by_input` is a pure function of the input,
/// `snapshot_closed` is exact within one source snapshot, `drifting` may change between
/// identical calls and is not cached in this milestone. A wire vocabulary lives here in
/// protocols (the `SignalSeverity` precedent); `graphhelm-tool-broker` re-exports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FreshnessClass {
    ImmutableByInput,
    SnapshotClosed,
    Drifting,
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
