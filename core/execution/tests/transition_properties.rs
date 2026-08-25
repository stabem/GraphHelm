use graphhelm_execution::{ExecutionError, NodeOutcome, TransitionRequest, apply_transition};
use graphhelm_protocols::NodeState;
use proptest::prelude::*;

const STATES: &[NodeState] = NodeState::every();

const OUTCOMES: [NodeOutcome; 13] = [
    NodeOutcome::Started,
    NodeOutcome::Succeeded,
    NodeOutcome::RetryableFailure,
    NodeOutcome::TerminalFailure,
    NodeOutcome::NeedsInput,
    NodeOutcome::NeedsCapacity,
    NodeOutcome::Approved,
    NodeOutcome::Waived,
    NodeOutcome::Skipped,
    NodeOutcome::Cancelled,
    NodeOutcome::Invalidated,
    NodeOutcome::Paused,
    NodeOutcome::Interrupted,
];

fn request(state: usize, outcome: usize, attempts: u32, identical: u32) -> TransitionRequest {
    TransitionRequest {
        current: STATES[state % STATES.len()],
        outcome: OUTCOMES[outcome % OUTCOMES.len()],
        attempts,
        identical_outcomes: identical,
    }
}

proptest! {
    /// The function is total: every input either yields a state or a typed error, never a panic.
    #[test]
    fn every_input_is_total(state in 0usize..64, outcome in 0usize..64, attempts in 0u32..64, identical in 0u32..64) {
        let _ = apply_transition(&request(state, outcome, attempts, identical));
    }

    /// The same request always yields the same answer. Any clock or ordering dependence here would
    /// break replay, which is the defect class this milestone must not repeat.
    #[test]
    fn transitions_are_deterministic(state in 0usize..64, outcome in 0usize..64, attempts in 0u32..64, identical in 0u32..64) {
        let request = request(state, outcome, attempts, identical);
        prop_assert_eq!(apply_transition(&request), apply_transition(&request));
    }

    /// A ghost never reaches a running state by any path, at any counter value. A ghost cannot be
    /// paused either: pause holds work that has started toward running, and a ghost is a proposal
    /// that never runs.
    #[test]
    fn a_ghost_never_becomes_runnable(outcome in 0usize..64, attempts in 0u32..64, identical in 0u32..64) {
        let mut request = request(0, outcome, attempts, identical);
        request.current = NodeState::Ghost;
        match apply_transition(&request) {
            Ok(next) => prop_assert!(
                !matches!(next, NodeState::Queued | NodeState::Running | NodeState::Paused),
                "a ghost reached {next:?}"
            ),
            Err(ExecutionError::IllegalTransition) => {}
        }
    }
}
