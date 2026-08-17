//! M07 Task 1 (F1): the operator's sleep question, decided ONCE.
//!
//! Two contested corrections to the declared contract are pinned here as tests (reported
//! in the handoff, per Task 0's "the contract is declared, not sacred"):
//! - `WaitingCapacity` is NOT wedged. It is the §12 park-and-wait rule working (the M05
//!   acceptance clause `exhausted-route-parks`): capacity returns and the node advances
//!   with no operator action. Calling it wedged would page the owner for a healthy state.
//! - `WaitingInput` IS attention, but never as `WedgedQuiescence`: nothing advances until
//!   the OWNER answers, and the reason must say so — a mislabelled reason is the same
//!   defect F1 names (a surface that does not answer the question honestly).

use std::collections::BTreeMap;

use graphhelm_events::ExecutionProjection;
use graphhelm_execution::attention::{Attention, AttentionReason, attention};
use graphhelm_protocols::{NodeOutcome, NodeState, SimulationStatus};

fn projection(
    states: &[(&str, NodeState)],
    status: Option<SimulationStatus>,
) -> ExecutionProjection {
    let mut projection = ExecutionProjection {
        simulation_status: status,
        ..ExecutionProjection::default()
    };
    projection.node_states = states
        .iter()
        .map(|(node, state)| ((*node).to_owned(), *state))
        .collect::<BTreeMap<_, _>>();
    projection
}

#[test]
fn a_clean_completed_story_lets_the_operator_sleep() {
    let projection = projection(
        &[
            ("build", NodeState::Succeeded),
            ("ship", NodeState::Succeeded),
        ],
        Some(SimulationStatus::Completed),
    );
    assert_eq!(
        attention(&projection),
        Attention {
            required: false,
            reasons: Vec::new()
        }
    );
}

#[test]
fn an_untriaged_interruption_names_the_node() {
    let mut projection = projection(
        &[("build", NodeState::Blocked)],
        Some(SimulationStatus::Blocked),
    );
    projection
        .last_outcome
        .insert("build".to_owned(), NodeOutcome::Interrupted);
    let answer = attention(&projection);
    assert!(answer.required);
    assert_eq!(
        answer.reasons,
        vec![AttentionReason::UntriagedInterruption {
            node: "build".to_owned()
        }],
        "an interrupted-and-never-looked-at node is the 04f triage rule, not a plain block"
    );
}

#[test]
fn a_blocked_node_without_an_interruption_is_a_plain_block() {
    let projection = projection(
        &[("build", NodeState::Blocked)],
        Some(SimulationStatus::Blocked),
    );
    assert_eq!(
        attention(&projection).reasons,
        vec![AttentionReason::BlockedNode {
            node: "build".to_owned()
        }]
    );
}

#[test]
fn a_failed_node_requires_attention() {
    let projection = projection(
        &[("build", NodeState::Failed)],
        Some(SimulationStatus::Failed),
    );
    assert_eq!(
        attention(&projection).reasons,
        vec![AttentionReason::FailedNode {
            node: "build".to_owned()
        }]
    );
}

#[test]
fn the_judges_shape_running_while_nothing_can_advance() {
    // The exact surface the blind judge saw reported green, stated precisely after FIX-1:
    // the graph is published, EVERY node in it reached a state no dispatch can pick up,
    // and the aggregate still claims to be running.
    let mut projection = projection(
        &[
            ("start", NodeState::Skipped),
            ("build", NodeState::Succeeded),
        ],
        Some(SimulationStatus::Running),
    );
    projection.current_graph = Some(published_graph());
    assert_eq!(
        attention(&projection).reasons,
        vec![AttentionReason::WedgedQuiescence],
        "running with nothing runnable and nothing untouched is the wedge F1 names"
    );
}

#[test]
fn a_running_story_with_work_in_flight_is_not_wedged() {
    for advanceable in [NodeState::Running, NodeState::Queued, NodeState::Ready] {
        let projection = projection(
            &[("build", advanceable), ("ship", NodeState::Draft)],
            Some(SimulationStatus::Running),
        );
        assert_eq!(
            attention(&projection),
            Attention {
                required: false,
                reasons: Vec::new()
            },
            "{advanceable:?} can advance — the operator may sleep"
        );
    }
}

#[test]
fn a_capacity_park_is_healthy_waiting_never_a_wedge() {
    // CONTESTED CORRECTION 1: the §12 wait rule (M05 clause `exhausted-route-parks`).
    // Quota returns and the node advances with no operator action, so paging here would
    // be a false alarm on a designed state.
    let projection = projection(
        &[("build", NodeState::WaitingCapacity)],
        Some(SimulationStatus::Running),
    );
    assert_eq!(
        attention(&projection),
        Attention {
            required: false,
            reasons: Vec::new()
        },
        "an exhausted route parks and resumes itself — sleep is the correct answer"
    );
}

#[test]
fn waiting_for_the_owners_input_says_so_and_never_calls_itself_wedged() {
    // CONTESTED CORRECTION 2: nothing advances until the owner answers — attention is
    // required, but `WedgedQuiescence` would misname a system that is working correctly
    // and waiting on a human.
    let projection = projection(
        &[("review", NodeState::WaitingInput)],
        Some(SimulationStatus::Running),
    );
    let answer = attention(&projection);
    assert!(answer.required);
    assert_eq!(
        answer.reasons,
        vec![AttentionReason::WaitingInputNode {
            node: "review".to_owned()
        }],
        "the reason must name the human as the blocker, not a wedge"
    );
}

#[test]
fn required_is_derived_never_declared() {
    // The first sabotage's target: `required` is a function of `reasons`, so no state can
    // ever produce a disagreement between them.
    let cases = [
        (NodeState::Succeeded, SimulationStatus::Completed),
        (NodeState::Failed, SimulationStatus::Failed),
        (NodeState::Blocked, SimulationStatus::Blocked),
        (NodeState::WaitingCapacity, SimulationStatus::Running),
        (NodeState::WaitingInput, SimulationStatus::Running),
        (NodeState::Running, SimulationStatus::Running),
    ];
    for (state, status) in cases {
        let status_label = format!("{status:?}");
        let answer = attention(&projection(&[("node", state)], Some(status)));
        assert_eq!(
            answer.required,
            !answer.reasons.is_empty(),
            "{state:?}/{status_label}: required must equal !reasons.is_empty()"
        );
    }
}

#[test]
fn reasons_are_deterministic_in_variant_then_node_order() {
    let mut projection = projection(
        &[
            ("zulu", NodeState::Failed),
            ("alpha", NodeState::Failed),
            ("mike", NodeState::Blocked),
            ("bravo", NodeState::Blocked),
        ],
        Some(SimulationStatus::Failed),
    );
    projection
        .last_outcome
        .insert("bravo".to_owned(), NodeOutcome::Interrupted);
    let first = attention(&projection);
    let second = attention(&projection);
    assert_eq!(first, second, "the same projection answers identically");
    assert_eq!(
        first.reasons,
        vec![
            AttentionReason::UntriagedInterruption {
                node: "bravo".to_owned()
            },
            AttentionReason::BlockedNode {
                node: "mike".to_owned()
            },
            AttentionReason::FailedNode {
                node: "alpha".to_owned()
            },
            AttentionReason::FailedNode {
                node: "zulu".to_owned()
            },
        ],
        "variant order first, node id within a variant"
    );
}

#[test]
fn an_empty_projection_is_not_an_alarm() {
    assert_eq!(
        attention(&ExecutionProjection::default()),
        Attention {
            required: false,
            reasons: Vec::new()
        },
        "nothing started is nothing to answer for"
    );
}

// ---------------------------------------------------------------------------------------------
// FIX-1 (A's Task 1 review, verified): `node_states` holds only nodes that ALREADY changed
// state. A node the driver has not touched yet is absent, and absence means "will be
// dispatched" — the driver itself reads it that way (`.get(node).unwrap_or(Draft)`,
// `driver.rs:72/447/511`). Deciding the wedge from that map alone therefore infers a
// verdict from silence, which is the exact defect F2 kills in the same delivery.
//
// The projection carries the published topology; a topology node with no recorded state is
// untouched and advances without the operator. With no graph published there is no basis to
// claim a wedge at all, so it is not claimed.
// ---------------------------------------------------------------------------------------------

fn published_graph() -> graphhelm_protocols::PersistedGraphVersion {
    serde_json::from_str(include_str!(
        "../../../conformance/schemas/valid/persisted-graph-version.json"
    ))
    .expect("the conformance fixture is a valid persisted graph version")
}

#[test]
fn a_just_started_execution_is_not_a_wedge() {
    // `SimulationStarted` sets `Running` (projection.rs:693) BEFORE any node state exists.
    // A status read at that instant must not scream at a perfectly healthy execution.
    let mut projection = projection(&[], Some(SimulationStatus::Running));
    projection.current_graph = Some(published_graph());
    assert_eq!(
        attention(&projection),
        Attention {
            required: false,
            reasons: Vec::new()
        },
        "every topology node is untouched — the driver will dispatch them"
    );
}

#[test]
fn an_untouched_topology_node_still_advances() {
    // The window between one node finishing and the next being dispatched — exactly when an
    // operator (or a blind judge) looks at a LIVE execution.
    let mut projection = projection(
        &[("finished-one", NodeState::Succeeded)],
        Some(SimulationStatus::Running),
    );
    projection.current_graph = Some(published_graph());
    assert_eq!(
        attention(&projection),
        Attention {
            required: false,
            reasons: Vec::new()
        },
        "`start` is in the topology and has no state yet: it is queued work, not a wedge"
    );
}

#[test]
fn without_a_published_graph_no_wedge_is_claimed() {
    // No topology means no way to know whether work remains. Claiming a wedge here would be
    // the same inference-from-silence, so the honest answer is to say nothing.
    let projection = projection(
        &[("finished-one", NodeState::Succeeded)],
        Some(SimulationStatus::Running),
    );
    assert_eq!(
        attention(&projection),
        Attention {
            required: false,
            reasons: Vec::new()
        },
        "no basis to assert a wedge without the graph that defines completeness"
    );
}

/// M07 Task 6, found by the CLOSING RULE — the blind judge's re-judgement caught this and
/// both agents had missed it: a REAL execution never emits `simulation_started`, so
/// `simulation_status` stays `None` for the whole run and only becomes `Some(..)` when
/// `execution_completed` folds (`core/events/src/projection.rs:782`). Proven on the M07
/// run's own journal: 22 events, zero `simulation_started`. A wedge rule that demanded
/// `Some(Running)` was therefore DEAD CODE in production — it could only ever fire for
/// simulation-driven stories, which is not what an operator watches at 3am.
#[test]
fn a_real_execution_reports_its_wedge_even_though_its_status_is_null() {
    let mut projection = projection(
        &[
            ("start", NodeState::Skipped),
            ("build", NodeState::Succeeded),
        ],
        // The production shape: started, not finished, no simulation status at all.
        None,
    );
    projection.execution_id = Some("exec-m07".to_owned());
    projection.current_graph = Some(published_graph());
    assert_eq!(
        attention(&projection).reasons,
        vec![AttentionReason::WedgedQuiescence],
        "a null status during a live execution means RUNNING, not 'no opinion'"
    );
}

/// The other side of the same rule, so the fix cannot become a false alarm: once the
/// execution completes, its terminal status is recorded and nothing is wedged.
#[test]
fn a_completed_execution_is_never_wedged() {
    for terminal in [
        SimulationStatus::Completed,
        SimulationStatus::Cancelled,
        SimulationStatus::Paused,
    ] {
        let mut projection = projection(
            &[
                ("start", NodeState::Skipped),
                ("build", NodeState::Succeeded),
            ],
            Some(terminal.clone()),
        );
        projection.execution_id = Some("exec-m07".to_owned());
        projection.current_graph = Some(published_graph());
        assert_eq!(
            attention(&projection),
            Attention {
                required: false,
                reasons: Vec::new()
            },
            "{terminal:?} is a finished story, not a wedged one"
        );
    }
}

/// A projection that never started is not a wedge either: no execution, no claim.
#[test]
fn an_unstarted_projection_claims_nothing() {
    let mut projection = projection(&[], None);
    projection.current_graph = Some(published_graph());
    assert_eq!(
        attention(&projection),
        Attention {
            required: false,
            reasons: Vec::new()
        },
        "nothing started is nothing to answer for"
    );
}
