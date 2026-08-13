//! The node transition function.
//!
//! `apply_transition` is total over its inputs and consults no clock, no randomness and no I/O, so
//! replaying the same request always yields the same state. Every bound it applies is a counter.

use graphhelm_protocols::NodeState;

use crate::bounds::{MAX_IDENTICAL_OUTCOMES, MAX_NODE_ATTEMPTS};

/// What happened to a node, as reported by the executor or the owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionError {
    /// The outcome is not legal from the current state.
    IllegalTransition,
}

/// One transition request. Counters are supplied by the caller so this function stays pure.
#[derive(Clone, Copy, Debug)]
pub struct TransitionRequest {
    pub current: NodeState,
    pub outcome: NodeOutcome,
    /// Attempts already made, not counting the one being reported.
    pub attempts: u32,
    /// Consecutive semantically identical outcomes already observed.
    pub identical_outcomes: u32,
}

/// Performs node work. The only milestone-04 implementation is effect-free; Milestone 05 supplies
/// one that calls real models and tools behind this same contract.
pub trait NodeExecutor {
    /// Executes one attempt of a node and reports its outcome.
    ///
    /// # Errors
    /// Returns `ExecutionError::IllegalTransition` when asked to execute a node that cannot run.
    fn execute(&self, node_id: &str, attempt: u32) -> Result<NodeOutcome, ExecutionError>;
}

const fn is_terminal(state: NodeState) -> bool {
    matches!(
        state,
        NodeState::Succeeded
            | NodeState::Failed
            | NodeState::Waived
            | NodeState::Skipped
            | NodeState::Cancelled
    )
}

/// Applies one outcome to one node.
///
/// # Errors
/// Returns `ExecutionError::IllegalTransition` when the outcome is not reachable from `current`.
pub fn apply_transition(request: &TransitionRequest) -> Result<NodeState, ExecutionError> {
    use NodeOutcome as O;
    use NodeState as S;

    // A terminal node is immutable. Only invalidation, which is an upstream event rather than an
    // outcome of this node, may reopen a success.
    if is_terminal(request.current) {
        return match (request.current, request.outcome) {
            (S::Succeeded, O::Invalidated) => Ok(S::Invalidated),
            _ => Err(ExecutionError::IllegalTransition),
        };
    }

    // Cancellation is owner sovereignty and applies from any non-terminal state.
    if request.outcome == O::Cancelled {
        return Ok(S::Cancelled);
    }

    match (request.current, request.outcome) {
        // A ghost is a proposal: approval readies it, nothing else may touch it.
        (S::Ghost, O::Approved) => Ok(S::Ready),
        (S::Ghost, _) => Err(ExecutionError::IllegalTransition),

        (S::Draft | S::Linting, O::Approved) => Ok(S::Ready),
        (S::Ready, O::Started) => Ok(S::Queued),
        (S::Queued, O::Started) => Ok(S::Running),

        (S::Running, O::Succeeded) => Ok(S::Succeeded),
        (S::Running, O::TerminalFailure) => Ok(S::Failed),
        (S::Running, O::NeedsInput) => Ok(S::WaitingInput),
        (S::Running, O::NeedsCapacity) => Ok(S::WaitingCapacity),

        // Retry is bounded by counters only. Exhaustion blocks for an owner decision rather than
        // failing silently, so no work is discarded without a record.
        (S::Running, O::RetryableFailure) => {
            if request.attempts >= MAX_NODE_ATTEMPTS
                || request.identical_outcomes >= MAX_IDENTICAL_OUTCOMES
            {
                Ok(S::Blocked)
            } else {
                Ok(S::Queued)
            }
        }

        (S::WaitingInput, O::NeedsInput) => Ok(S::WaitingInput),
        (S::WaitingCapacity, O::NeedsCapacity) => Ok(S::WaitingCapacity),
        (S::WaitingInput | S::WaitingCapacity | S::Paused, O::Started) => Ok(S::Queued),

        (_, O::Waived) => Ok(S::Waived),
        (_, O::Skipped) => Ok(S::Skipped),
        (S::Invalidated, O::Approved) => Ok(S::Ready),

        _ => Err(ExecutionError::IllegalTransition),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use graphhelm_protocols::NodeState;

    fn request(from: NodeState, outcome: NodeOutcome, attempts: u32) -> TransitionRequest {
        TransitionRequest {
            current: from,
            outcome,
            attempts,
            identical_outcomes: 0,
        }
    }

    #[test]
    fn a_queued_node_starts_and_then_succeeds() {
        let started =
            apply_transition(&request(NodeState::Queued, NodeOutcome::Started, 0)).unwrap();
        assert_eq!(started, NodeState::Running);
        let done =
            apply_transition(&request(NodeState::Running, NodeOutcome::Succeeded, 1)).unwrap();
        assert_eq!(done, NodeState::Succeeded);
    }

    /// A ghost is a proposal. It never runs, which is how "consumes no tokens" is enforced
    /// structurally rather than by convention.
    #[test]
    fn a_ghost_node_can_never_start() {
        assert_eq!(
            apply_transition(&request(NodeState::Ghost, NodeOutcome::Started, 0)).unwrap_err(),
            ExecutionError::IllegalTransition
        );
        assert_eq!(
            apply_transition(&request(NodeState::Ghost, NodeOutcome::Approved, 0)).unwrap(),
            NodeState::Ready
        );
    }

    #[test]
    fn a_terminal_node_never_transitions_again() {
        for terminal in [
            NodeState::Succeeded,
            NodeState::Failed,
            NodeState::Cancelled,
            NodeState::Waived,
            NodeState::Skipped,
        ] {
            assert_eq!(
                apply_transition(&request(terminal, NodeOutcome::Started, 1)).unwrap_err(),
                ExecutionError::IllegalTransition,
                "{terminal:?} must be terminal"
            );
        }
    }

    /// Exhausting attempts blocks for an owner decision. It never silently gives up, and it never
    /// consults a clock.
    #[test]
    fn exhausting_attempts_blocks_rather_than_failing_silently() {
        let retryable = request(
            NodeState::Running,
            NodeOutcome::RetryableFailure,
            MAX_NODE_ATTEMPTS - 1,
        );
        assert_eq!(apply_transition(&retryable).unwrap(), NodeState::Queued);

        let exhausted = request(
            NodeState::Running,
            NodeOutcome::RetryableFailure,
            MAX_NODE_ATTEMPTS,
        );
        assert_eq!(apply_transition(&exhausted).unwrap(), NodeState::Blocked);
    }

    #[test]
    fn repeated_identical_outcomes_block_as_no_progress() {
        let stalled = TransitionRequest {
            current: NodeState::Running,
            outcome: NodeOutcome::RetryableFailure,
            attempts: 1,
            identical_outcomes: MAX_IDENTICAL_OUTCOMES,
        };
        assert_eq!(apply_transition(&stalled).unwrap(), NodeState::Blocked);
    }

    #[test]
    fn an_owner_waiver_clears_a_blocked_node() {
        assert_eq!(
            apply_transition(&request(NodeState::Blocked, NodeOutcome::Waived, 3)).unwrap(),
            NodeState::Waived
        );
    }
}
