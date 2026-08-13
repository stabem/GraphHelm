//! Whether a node may be attempted again.
//!
//! Every threshold here is a count, never a duration, so replaying the same history reaches the same
//! verdict on a slower machine. The counters themselves are derived by the projection in 04b; this
//! module only judges them.
//!
//! `OBSERVABILITY_AND_RECOVERY.md` §15 names seven no-progress conditions. Two are decidable from
//! recorded history and are implemented here: retries with no change, and semantically identical
//! outcomes. The remaining five — recurring remediation loops, alternating graph mutations, agent
//! delegation chains, repeated tool failure, and budget consumed without evidence gain — need signal
//! intake (04d) or real tool calls (Milestone 05). None of them is silently approximated.

use graphhelm_events::ExecutionProjection;
use graphhelm_protocols::NodeOutcome;

use crate::bounds::{MAX_IDENTICAL_OUTCOMES, MAX_NODE_ATTEMPTS};

/// The scheduler's verdict for one node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progress {
    /// The node may be dispatched again.
    Continue,
    /// Attempts are spent. Block for an owner decision rather than failing the node, so no work is
    /// discarded without a record.
    AttemptsExhausted,
    /// The same outcome keeps recurring. Block for an owner decision.
    NoProgress,
}

/// Classifies whether `node` may be attempted again, given the outcome about to be reported.
#[must_use]
pub fn classify_progress(
    projection: &ExecutionProjection,
    node: &str,
    outcome: NodeOutcome,
) -> Progress {
    if projection.node_attempts.get(node).copied().unwrap_or(0) >= MAX_NODE_ATTEMPTS {
        return Progress::AttemptsExhausted;
    }
    // `identical_outcomes_for` returns 0 when the last recorded outcome differs, which is why this
    // reads through it rather than the raw map: a run belonging to some other outcome must not
    // block this one on its first occurrence.
    if projection.identical_outcomes_for(node, outcome) >= MAX_IDENTICAL_OUTCOMES {
        return Progress::NoProgress;
    }
    Progress::Continue
}

#[cfg(test)]
mod tests {
    use super::*;
    use graphhelm_events::ExecutionProjection;
    use graphhelm_protocols::NodeOutcome;

    fn projection(attempts: u32, run: u32, last: NodeOutcome) -> ExecutionProjection {
        let mut projection = ExecutionProjection::default();
        projection
            .node_attempts
            .insert("start".to_owned(), attempts);
        projection.last_outcome.insert("start".to_owned(), last);
        projection
            .identical_outcomes
            .insert("start".to_owned(), run);
        projection
    }

    #[test]
    fn a_node_with_attempts_left_may_continue() {
        let projection = projection(1, 1, NodeOutcome::RetryableFailure);
        assert_eq!(
            classify_progress(&projection, "start", NodeOutcome::RetryableFailure),
            Progress::Continue
        );
    }

    /// Exhausting attempts blocks for an owner decision. It never silently gives up, and it never
    /// consults a clock.
    #[test]
    fn exhausted_attempts_block() {
        let projection = projection(MAX_NODE_ATTEMPTS, 1, NodeOutcome::RetryableFailure);
        assert_eq!(
            classify_progress(&projection, "start", NodeOutcome::RetryableFailure),
            Progress::AttemptsExhausted
        );
    }

    #[test]
    fn a_repeating_outcome_blocks_as_no_progress() {
        let projection = projection(1, MAX_IDENTICAL_OUTCOMES, NodeOutcome::RetryableFailure);
        assert_eq!(
            classify_progress(&projection, "start", NodeOutcome::RetryableFailure),
            Progress::NoProgress
        );
    }

    /// The run length belongs to the outcome that produced it. A node whose *previous* run reached
    /// the bound with some other outcome has made no repeated failure of this kind, and blocking it
    /// on its first would be wrong.
    #[test]
    fn a_run_of_a_different_outcome_does_not_block_this_one() {
        let projection = projection(1, MAX_IDENTICAL_OUTCOMES, NodeOutcome::NeedsCapacity);
        assert_eq!(
            classify_progress(&projection, "start", NodeOutcome::RetryableFailure),
            Progress::Continue
        );
    }

    #[test]
    fn an_unknown_node_may_continue() {
        assert_eq!(
            classify_progress(
                &ExecutionProjection::default(),
                "absent",
                NodeOutcome::Started
            ),
            Progress::Continue
        );
    }
}
