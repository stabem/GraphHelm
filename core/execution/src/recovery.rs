//! Crash recovery and resume preconditions, as pure decisions.
//!
//! `OBSERVABILITY_AND_RECOVERY.md` §11.4 lists eight resume steps. The decidable ones — a paused
//! execution, a matching graph version, no node still marked running — are validated here. Lease
//! renewal, route health, sandbox recreation and session invalidation need a runtime and are
//! Milestone 05's; naming them here is deliberate, so nothing pretends to validate them.

use graphhelm_events::ExecutionProjection;
use graphhelm_protocols::{NodeOutcome, NodeState, SimulationStatus};

/// The nodes whose effects are unknown, in deterministic order.
///
/// The driver must record `NodeOutcome::Interrupted` for each before anything else happens to the
/// execution; `(Running, Interrupted) -> Blocked` is the only legal consequence, and the design's
/// acceptance criterion — never resume a node whose effects are unknown — rests on it.
#[must_use]
pub fn recovery_plan(projection: &ExecutionProjection) -> Vec<String> {
    projection
        .node_states
        .iter()
        .filter(|(_, state)| **state == NodeState::Running)
        .map(|(node, _)| node.clone())
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResumeError {
    /// No execution has started; there is nothing to resume.
    NotStarted,
    /// Only a paused execution resumes. A completed, failed or cancelled one is history.
    NotPaused,
    /// A node is still marked running: the interruption has not been recovered, and resuming
    /// would run work whose predecessor effects are unknown.
    UnrecoveredInterruption,
    /// A node is `Blocked` with its last recorded outcome `Interrupted`: the crash was recovered
    /// (`Running -> Blocked`) but nobody has looked at it since. Resume refuses until the owner
    /// approves, waives, skips or cancels each such node — the triage act the projection can
    /// decide on its own, without a runtime. A node `Blocked` for any other reason (attempts
    /// exhausted, no progress) does not hold resume: the owner may legitimately resume the rest of
    /// the graph and deal with it later (04e finding 1, resolved).
    UntriagedInterruption,
    /// The graph version to resume against is not the one the projection recorded.
    VersionMismatch,
}

/// §11.4's decidable subset, as a pure gate the driver must pass before appending
/// `execution_resumed`.
///
/// # Errors
/// Each variant names the operator-facing reason; see `ResumeError`.
pub fn resume_preconditions(
    projection: &ExecutionProjection,
    resume_against_version: Option<u64>,
) -> Result<(), ResumeError> {
    if projection.execution_id.is_none() {
        return Err(ResumeError::NotStarted);
    }
    if projection.simulation_status != Some(SimulationStatus::Paused) {
        return Err(ResumeError::NotPaused);
    }
    if projection
        .node_states
        .values()
        .any(|state| *state == NodeState::Running)
    {
        return Err(ResumeError::UnrecoveredInterruption);
    }
    if projection.node_states.iter().any(|(node, state)| {
        *state == NodeState::Blocked
            && projection.last_outcome.get(node) == Some(&NodeOutcome::Interrupted)
    }) {
        return Err(ResumeError::UntriagedInterruption);
    }
    match (resume_against_version, &projection.current_graph) {
        (Some(requested), Some(current)) if requested != current.number() => {
            Err(ResumeError::VersionMismatch)
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use graphhelm_events::ExecutionProjection;
    use graphhelm_protocols::{NodeOutcome, NodeState, SimulationStatus};

    fn projection(
        nodes: &[(&str, NodeState)],
        status: Option<SimulationStatus>,
    ) -> ExecutionProjection {
        ExecutionProjection {
            execution_id: Some("execution-1".to_owned()),
            node_states: nodes
                .iter()
                .map(|(node, state)| ((*node).to_owned(), *state))
                .collect(),
            simulation_status: status,
            ..ExecutionProjection::default()
        }
    }

    /// A real published graph version, loaded the same way `core/governor/src/inflight.rs`'s 04d
    /// waiver test does. The fixture's `number` (2) is not settable through
    /// `PersistedGraphVersion`'s public API without also supplying a matching predecessor, so per
    /// the plan's fallback this test reads the fixture's own `number()` and tests the mismatch
    /// case against `number() + 1` rather than a hardcoded literal.
    fn fixture_graph() -> graphhelm_protocols::PersistedGraphVersion {
        serde_json::from_str(include_str!(
            "../../../conformance/schemas/valid/persisted-graph-version.json"
        ))
        .unwrap()
    }

    fn attach_current_graph(
        projection: &mut ExecutionProjection,
        graph: graphhelm_protocols::PersistedGraphVersion,
    ) {
        projection.current_graph = Some(graph);
    }

    /// Every running node was interrupted; nothing else was. The plan is deterministic because the
    /// map is a BTreeMap.
    #[test]
    fn recovery_interrupts_exactly_the_running_nodes() {
        let projection = projection(
            &[
                ("a", NodeState::Running),
                ("b", NodeState::Queued),
                ("c", NodeState::Succeeded),
                ("d", NodeState::Running),
            ],
            Some(SimulationStatus::Running),
        );
        let plan = recovery_plan(&projection);
        assert_eq!(plan, ["a".to_owned(), "d".to_owned()]);
    }

    #[test]
    fn a_clean_projection_needs_no_recovery() {
        let projection = projection(
            &[("a", NodeState::Succeeded)],
            Some(SimulationStatus::Running),
        );
        assert!(recovery_plan(&projection).is_empty());
    }

    /// §11.4's decidable subset. Each rejection names its reason, because the driver turns these
    /// into operator messages.
    #[test]
    fn resume_requires_a_paused_execution_with_matching_version_and_no_running_node() {
        let good = projection(&[("a", NodeState::Paused)], Some(SimulationStatus::Paused));
        assert_eq!(resume_preconditions(&good, None), Ok(()));

        let not_paused = projection(&[], Some(SimulationStatus::Running));
        assert_eq!(
            resume_preconditions(&not_paused, None),
            Err(ResumeError::NotPaused)
        );

        let still_running =
            projection(&[("a", NodeState::Running)], Some(SimulationStatus::Paused));
        assert_eq!(
            resume_preconditions(&still_running, None),
            Err(ResumeError::UnrecoveredInterruption)
        );

        let mut unstarted = projection(&[], Some(SimulationStatus::Paused));
        unstarted.execution_id = None;
        assert_eq!(
            resume_preconditions(&unstarted, None),
            Err(ResumeError::NotStarted)
        );
    }

    /// An interruption that was recorded but never looked at must hold resume. A node blocked for
    /// any other reason does not: the owner may resume the rest of the graph and deal with it
    /// later (04e finding 1, resolved).
    #[test]
    fn resume_refuses_an_untriaged_interruption_but_not_other_blocks() {
        let mut interrupted =
            projection(&[("a", NodeState::Blocked)], Some(SimulationStatus::Paused));
        interrupted
            .last_outcome
            .insert("a".to_owned(), NodeOutcome::Interrupted);
        assert_eq!(
            resume_preconditions(&interrupted, None),
            Err(ResumeError::UntriagedInterruption)
        );

        let mut exhausted =
            projection(&[("a", NodeState::Blocked)], Some(SimulationStatus::Paused));
        exhausted
            .last_outcome
            .insert("a".to_owned(), NodeOutcome::RetryableFailure);
        assert_eq!(resume_preconditions(&exhausted, None), Ok(()));
    }

    /// Resuming against a different graph version than the projection recorded is refused: the
    /// plan the nodes were scheduled under no longer describes the graph.
    #[test]
    fn resume_refuses_a_version_mismatch() {
        let mut projection = projection(&[], Some(SimulationStatus::Paused));
        let graph = fixture_graph();
        let version = graph.number();
        attach_current_graph(&mut projection, graph);
        assert_eq!(
            resume_preconditions(&projection, Some(version + 1)),
            Err(ResumeError::VersionMismatch)
        );
        assert_eq!(resume_preconditions(&projection, Some(version)), Ok(()));
    }
}
