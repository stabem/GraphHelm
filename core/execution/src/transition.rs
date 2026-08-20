//! The node transition function.
//!
//! `apply_transition` is total over its inputs and consults no clock, no randomness and no I/O, so
//! replaying the same request always yields the same state. Every bound it applies is a counter.

use graphhelm_protocols::{NodeOutcome, NodeState};

use crate::bounds::{MAX_IDENTICAL_OUTCOMES, MAX_NODE_ATTEMPTS};

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

/// A node in one of these states will never be revisited: the execution is done with it either
/// way.
///
/// PUBLIC AS OF #101, and the widening is the point. Three identical copies of this predicate
/// existed — here, `apps/cli/.../driver.rs` and `apps/cli/.../mod.rs` — and `mod.rs`'s own comment
/// recorded the trade honestly: it "mirrors — rather than widens the visibility of — driver.rs's
/// private copy, keeping this task's diff inside the files it owns". That was a reasonable local
/// call and it left a defect nobody would notice: adding one terminal state to `NodeState` needed
/// THREE edits, and editing two of three fails silently — the copies agree today and would simply
/// stop agreeing, with no test anywhere comparing them.
///
/// This copy is the authority because it is the one `apply_transition` short-circuits on: if these
/// ever disagreed, this is the one that decides what the STATE MACHINE does, and the others would
/// merely be describing it wrongly.
#[must_use]
pub const fn is_terminal(state: NodeState) -> bool {
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
        // The owner's resume path out of Blocked, missing since 04a. Approval makes the node
        // dispatchable on the next scheduler pass; nothing auto-starts out of a manual
        // intervention — the scheduler's normal cycle does.
        (S::Blocked, O::Approved) => Ok(S::Ready),
        (S::Ready, O::Started) => Ok(S::Queued),
        (S::Queued, O::Started) => Ok(S::Running),

        // Graceful pause holds work that has not started. A running node completes instantly in
        // this milestone, so it is not pausable; interrupting real work is Milestone 05's.
        (S::Ready | S::Queued, O::Paused) => Ok(S::Paused),

        (S::Running, O::Succeeded) => Ok(S::Succeeded),
        (S::Running, O::TerminalFailure) => Ok(S::Failed),
        (S::Running, O::NeedsInput) => Ok(S::WaitingInput),
        (S::Running, O::NeedsCapacity) => Ok(S::WaitingCapacity),
        // The execution stopped while this node was running, so its effects are unknown. Blocked
        // is the only legal consequence: nothing may resume a node whose effects are unknown.
        (S::Running, O::Interrupted) => Ok(S::Blocked),

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

    /// The owner's resume path out of Blocked, missing since 04a. Approval makes the node
    /// dispatchable on the next scheduler pass; per D-020's spirit nothing auto-starts out of a
    /// manual intervention — the scheduler's normal cycle does.
    #[test]
    fn an_owner_approval_readies_a_blocked_node() {
        assert_eq!(
            apply_transition(&request(NodeState::Blocked, NodeOutcome::Approved, 3)).unwrap(),
            NodeState::Ready
        );
    }

    /// Graceful pause holds work that has not started. A running node is not pausable in this
    /// milestone, and a ghost is not pausable in any.
    #[test]
    fn pause_holds_ready_and_queued_work_only() {
        for from in [NodeState::Ready, NodeState::Queued] {
            assert_eq!(
                apply_transition(&request(from, NodeOutcome::Paused, 0)).unwrap(),
                NodeState::Paused
            );
        }
        for from in [NodeState::Running, NodeState::Ghost, NodeState::Succeeded] {
            assert!(
                apply_transition(&request(from, NodeOutcome::Paused, 0)).is_err(),
                "{from:?} must not pause"
            );
        }
    }

    /// A node running when the execution stopped has unknown effects. Blocked is the only legal
    /// consequence; anything else would resume work nobody judged safe.
    #[test]
    fn an_interrupted_running_node_blocks() {
        assert_eq!(
            apply_transition(&request(NodeState::Running, NodeOutcome::Interrupted, 1)).unwrap(),
            NodeState::Blocked
        );
        for from in [NodeState::Ready, NodeState::Queued, NodeState::Paused] {
            assert!(
                apply_transition(&request(from, NodeOutcome::Interrupted, 0)).is_err(),
                "{from:?} was not running; interruption does not apply"
            );
        }
    }
}
