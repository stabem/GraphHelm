//! The effect-free `NodeExecutor`.
//!
//! This is the only milestone-04 implementation of the seam: it consults a fixture table and
//! nothing else — no model, no tool, no sandbox, no network. Milestone 05 supplies one that does
//! real work behind the same contract, which is what keeps every property in this milestone
//! testable offline.

use graphhelm_execution::{ExecutionError, NodeExecutor};
use graphhelm_protocols::{FixtureOutcome, NodeOutcome};

use crate::fixtures::SimulationFixtures;

/// Answers from a fixture table. Deterministic and side-effect free.
#[derive(Clone, Debug, Default)]
pub struct FixtureExecutor {
    fixtures: SimulationFixtures,
}

impl FixtureExecutor {
    #[must_use]
    pub const fn new(fixtures: SimulationFixtures) -> Self {
        Self { fixtures }
    }
}

impl NodeExecutor for FixtureExecutor {
    fn execute(&self, node_id: &str, _attempt: u32) -> Result<NodeOutcome, ExecutionError> {
        // The attempt number is deliberately unused. An executor whose answer changed with the
        // attempt would make a replay diverge from the run it replays.
        Ok(match self.fixtures.node_outcomes.get(node_id) {
            Some(FixtureOutcome::Success) => NodeOutcome::Succeeded,
            Some(FixtureOutcome::Failure) => NodeOutcome::RetryableFailure,
            // An absent fixture and an explicitly unknown one mean the same thing: nobody said what
            // this node does. Waiting is honest; inventing a success is not.
            Some(FixtureOutcome::Unknown) | None => NodeOutcome::NeedsInput,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use graphhelm_execution::NodeExecutor;
    use graphhelm_protocols::{FixtureOutcome, NodeOutcome};

    fn executor(pairs: &[(&str, FixtureOutcome)]) -> FixtureExecutor {
        let mut fixtures = SimulationFixtures::default();
        for (node, outcome) in pairs {
            fixtures
                .node_outcomes
                .insert((*node).to_owned(), outcome.clone());
        }
        FixtureExecutor::new(fixtures)
    }

    #[test]
    fn a_fixture_outcome_maps_to_a_node_outcome() {
        let executor = executor(&[
            ("ok", FixtureOutcome::Success),
            ("bad", FixtureOutcome::Failure),
        ]);
        assert_eq!(executor.execute("ok", 0).unwrap(), NodeOutcome::Succeeded);
        assert_eq!(
            executor.execute("bad", 0).unwrap(),
            NodeOutcome::RetryableFailure
        );
    }

    /// A node with no fixture has not been told what to do. It waits for input rather than being
    /// invented as a success, because inventing one would make a simulation pass for a node nobody
    /// specified.
    #[test]
    fn an_unspecified_node_waits_for_input() {
        let empty = executor(&[]);
        assert_eq!(empty.execute("absent", 0).unwrap(), NodeOutcome::NeedsInput);
        assert_eq!(
            executor(&[("u", FixtureOutcome::Unknown)])
                .execute("u", 0)
                .unwrap(),
            NodeOutcome::NeedsInput
        );
    }

    /// The executor is effect-free and its answer cannot depend on the attempt number, or replaying
    /// an execution would diverge from the original run.
    #[test]
    fn the_outcome_does_not_depend_on_the_attempt() {
        let executor = executor(&[("bad", FixtureOutcome::Failure)]);
        let first = executor.execute("bad", 0).unwrap();
        for attempt in [1, 7, u32::MAX] {
            assert_eq!(executor.execute("bad", attempt).unwrap(), first);
        }
    }
}
