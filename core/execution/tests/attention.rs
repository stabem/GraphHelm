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

/// A projection carrying one recorded mis-burn for `session`, and nothing else wrong.
///
/// `WakeMisBurn` keeps both armings on purpose (`projection.rs`): "something was wrong here" is
/// not actionable and the PAIR is the diagnosis. These cells never read the pair -- attention's
/// question is existence -- but they build a faithful one so the fixture cannot pass by being
/// shaped differently from what the fold writes.
fn with_mis_burn(session: &str, at_sequence: u64) -> ExecutionProjection {
    let mut projection = projection(
        &[
            ("build", NodeState::Succeeded),
            ("ship", NodeState::Succeeded),
        ],
        Some(SimulationStatus::Completed),
    );
    projection.wake_mis_burns.insert(
        session.to_owned(),
        graphhelm_events::WakeMisBurn {
            at_sequence,
            captured_arming: 41,
            live_arming: 42,
        },
    );
    projection
}

/// #119: the fold records a consumption that burned an arming other than the one it captured,
/// and no operator surface reads it.
///
/// This is NOT a wake-timing defect and the reason must not read as one. Post-#74 the serve
/// path's own filter drops a stale capture, so the fold's mis-burn arm fires only for a
/// consumption written by something else: a direct append, an older binary, or a bug. The
/// remedy is provenance -- *check who is writing to this store* -- which is why it belongs in
/// attention rather than in the wake surfaces alone (design note #119).
///
/// The PRESENCE member of the absence-guard pair: without this cell, the "raises nothing"
/// assertion below would pass just as well if the predicate were never reached at all.
#[test]
fn a_recorded_mis_burn_reaches_the_operator() {
    let projection = with_mis_burn("session-a", 77);
    let answer = attention(&projection, &AttentionInputs::default());
    assert_eq!(
        answer.reasons(),
        &[AttentionReason::ForeignWakeConsumption {
            session: "session-a".to_owned(),
        }],
        "a mis-burn the fold recorded must reach the operator: {answer:?}"
    );
}

/// The mirror, and it is what makes the cell above a claim about mis-burns rather than about
/// wake activity: the SAME graph with no recorded mis-burn says nothing.
#[test]
fn a_projection_with_no_mis_burn_raises_nothing() {
    let projection = projection(
        &[
            ("build", NodeState::Succeeded),
            ("ship", NodeState::Succeeded),
        ],
        Some(SimulationStatus::Completed),
    );
    assert!(
        projection.wake_mis_burns.is_empty(),
        "ARRANGEMENT: the mirror must differ from the cell above ONLY by the mis-burn"
    );
    let answer = attention(&projection, &AttentionInputs::default());
    assert_eq!(
        answer.verdict,
        graphhelm_execution::Verdict::CanSleep,
        "no mis-burn, nothing else wrong: {answer:?}"
    );
}

/// THE GRAIN, pinned so it cannot be "improved" into something the map cannot support.
///
/// `wake_mis_burns` is `BTreeMap<session, WakeMisBurn>` and inserts OVERWRITE, so the map can
/// answer *has this session ever mis-burned* and never *how many times*. Existence survives the
/// overwrite; count and history do not. A later reader who turns this into a count would find
/// this cell red rather than shipping an answer the record cannot back.
#[test]
fn a_second_mis_burn_on_one_session_is_still_one_reason() {
    let mut projection = with_mis_burn("session-a", 77);
    projection.wake_mis_burns.insert(
        "session-a".to_owned(),
        graphhelm_events::WakeMisBurn {
            at_sequence: 91,
            captured_arming: 43,
            live_arming: 44,
        },
    );
    assert_eq!(
        projection.wake_mis_burns.len(),
        1,
        "ARRANGEMENT: the map overwrote rather than accumulated, which is the shape this pins"
    );
    let answer = attention(&projection, &AttentionInputs::default());
    assert_eq!(
        answer.reasons(),
        &[AttentionReason::ForeignWakeConsumption {
            session: "session-a".to_owned(),
        }],
        "existence per session, not a count: {answer:?}"
    );
}

/// #119, SECOND PASS (ISSUES 3, BLOCK at `798b7dcb`): a wedge and a foreign writer are TWO
/// findings, not one, and the first version of this change let the second erase the first.
///
/// It pushed `ForeignWakeConsumption` into `reasons` above a wedge test written as
/// `reasons.is_empty()`, so any recorded mis-burn suppressed `WedgedQuiescence`. And permanently,
/// not for a run: `wake_mis_burns` has exactly one write site (`projection.rs:1926`, an insert) and
/// no `remove`, `clear`, `retain` or `drain` anywhere in the workspace -- so the first mis-burn a
/// store ever recorded would have switched off its wedge detector for the life of that store.
///
/// The two answer different questions. The five reasons the wedge defers to all explain WHY NOTHING
/// ADVANCES, and deferring to them is coherent -- the wedge is the last-resort explanation. A
/// mis-burn explains WHO IS WRITING TO THIS STORE, which is orthogonal: a store can be wedged and
/// foreign-written at once, and the operator needs both.
///
/// THE INTERACTION HAD NO CELL, which is the other half of why the defect shipped green: every
/// wedge cell built an empty `wake_mis_burns` and every mis-burn cell built a projection that was
/// not wedged. Neither assertion below can pass vacuously -- both are positive, so a fixture that
/// stopped being wedged, or an insert that failed to take, reddens rather than quietly agreeing.
#[test]
fn a_wedged_run_still_reports_its_wedge_when_a_mis_burn_is_recorded() {
    // The wedge fixture, verbatim from `the_judges_shape_running_while_nothing_can_advance`: every
    // node in a published graph is in a state no dispatch can pick up, and the aggregate still
    // claims to be running.
    let mut projection = projection(
        &[
            ("start", NodeState::Skipped),
            ("build", NodeState::Succeeded),
        ],
        Some(SimulationStatus::Running),
    );
    projection.current_graph = Some(published_graph());
    projection.wake_mis_burns.insert(
        "session-a".to_owned(),
        graphhelm_events::WakeMisBurn {
            at_sequence: 77,
            captured_arming: 41,
            live_arming: 42,
        },
    );

    let answer = attention(&projection, &AttentionInputs::default());
    let reasons = answer.reasons();
    assert!(
        reasons.contains(&AttentionReason::WedgedQuiescence),
        "a foreign writer does not explain why nothing advances, so it must not suppress the wedge: {answer:?}"
    );
    assert!(
        reasons.contains(&AttentionReason::ForeignWakeConsumption {
            session: "session-a".to_owned(),
        }),
        "and the mis-burn is reported beside the wedge, not instead of it: {answer:?}"
    );
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
        attention(&projection, &AttentionInputs::default()),
        Attention {
            verdict: graphhelm_execution::Verdict::CanSleep,
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
    let answer = attention(&projection, &AttentionInputs::default());
    assert!(matches!(
        answer.verdict,
        graphhelm_execution::Verdict::NeedsYou { .. }
    ));
    assert_eq!(
        answer.reasons(),
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
        attention(&projection, &AttentionInputs::default()).reasons(),
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
        attention(&projection, &AttentionInputs::default()).reasons(),
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
        attention(&projection, &AttentionInputs::default()).reasons(),
        vec![AttentionReason::WedgedQuiescence],
        "running with nothing runnable and nothing untouched is the wedge F1 names"
    );
}

#[test]
fn a_running_story_with_work_in_flight_is_not_wedged() {
    // #92: `Ready` rode in this list until #920 took it out of `advances_without_the_operator`,
    // and the cell kept passing for it -- NOT because a `Ready` node advances, but because this
    // fixture declares no published graph, so `graph_defines_completeness` is false and the wedge
    // branch is unreachable whatever the state says. The message below ("can advance") was then
    // true of `Running` and `Queued` and false of `Ready`, and nothing could tell.
    //
    // Measured on this tree: with a published graph, `Ready` answers `WedgedQuiescence` and
    // `Queued` answers `CanSleep`. The two states diverge, so `Ready` is pinned by its own cell
    // below rather than riding here where the fixture, not the behaviour, decides the answer.
    for advanceable in [NodeState::Running, NodeState::Queued] {
        let projection = projection(
            &[("build", advanceable), ("ship", NodeState::Draft)],
            Some(SimulationStatus::Running),
        );
        let answer = attention(&projection, &AttentionInputs::default());
        assert!(
            !matches!(
                answer.verdict,
                graphhelm_execution::Verdict::NeedsYou { .. }
            ) && answer.reasons().is_empty(),
            "{advanceable:?} can advance — the operator may sleep: {answer:?}"
        );
        // DELIBERATE CHANGE (M08 Task 1): with no budget and no measurement declared, a
        // node IN FLIGHT is an UNKNOWN, not calm. The M07 version of this test asserted an
        // entirely empty answer, which today would assert that silence was judged when
        // nothing was ever supplied to judge it with.
        assert_eq!(
            answer.silence_unevaluated(),
            if advanceable == NodeState::Running {
                vec![graphhelm_execution::Unevaluated::Node {
                    node: "build".to_owned(),
                    reason: graphhelm_execution::NodeUnevaluated::NoDeclaredBudget,
                    remedy: graphhelm_execution::Remedy::DeclareNodeBudget {
                        node: "build".to_owned(),
                        observed_silence_seconds: 0,
                        computed_at_sequence: None,
                    },
                }]
            } else {
                Vec::new()
            },
            "only work in flight can be unevaluated: {answer:?}"
        );
    }
}

#[test]
fn a_ready_node_under_a_published_graph_is_the_wedge_920_chose() {
    // #92 / #920. `advances_without_the_operator` excludes `Ready` because no dispatcher runs on
    // its own: a node left `Ready` waits for an operator to run `resume`. This cell pins the
    // CONSEQUENCE of that choice, which no cell held before: under a PUBLISHED graph -- the only
    // shape where the wedge branch is reachable -- a merely `Ready` node reads as wedged.
    //
    // It is here so the choice cannot be reverted in silence. Putting `Ready` back into that
    // predicate turns this red at the assertion, which the sibling loop above cannot do.
    // ON A NODE THE PUBLISHED TOPOLOGY ACTUALLY DECLARES (Codex P2 on this PR, accepted).
    // `published_graph()` loads conformance/schemas/valid/persisted-graph-version.json, whose
    // `topology.nodes` is exactly {"start"}. The first draft of this cell put `Ready` on a
    // `build` node that the graph does not contain: the wedge still fired, but for the
    // completeness of an out-of-topology state map rather than for a Ready node under the
    // graph -- the same "satisfied by the fixture, not the behaviour" defect this cell exists
    // to correct in its sibling. One node, and it is the graph's own.
    let mut projection = projection(
        &[("start", NodeState::Ready)],
        Some(SimulationStatus::Running),
    );
    projection.current_graph = Some(published_graph());
    assert_eq!(
        attention(&projection, &AttentionInputs::default()).reasons(),
        vec![AttentionReason::WedgedQuiescence],
        "a node nothing will dispatch is the wedge #920 chose to make audible"
    );

    // CONTROL, and it is what makes the assertion above about the STATE rather than the fixture:
    // the identical projection with `Queued` -- a state that does advance -- stays calm.
    let mut queued = projection_with_queued();
    queued.current_graph = Some(published_graph());
    let answer = attention(&queued, &AttentionInputs::default());
    assert!(
        answer.reasons().is_empty(),
        "the same shape with a dispatchable state is not a wedge: {answer:?}"
    );
}

/// The control's projection, kept out of the cell so the two differ in exactly one state.
fn projection_with_queued() -> ExecutionProjection {
    projection(
        &[("start", NodeState::Queued)],
        Some(SimulationStatus::Running),
    )
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
        attention(&projection, &AttentionInputs::default()),
        Attention {
            verdict: graphhelm_execution::Verdict::CanSleep,
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
    let answer = attention(&projection, &AttentionInputs::default());
    assert!(matches!(
        answer.verdict,
        graphhelm_execution::Verdict::NeedsYou { .. }
    ));
    assert_eq!(
        answer.reasons(),
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
        let answer = attention(
            &projection(&[("node", state)], Some(status)),
            &AttentionInputs::default(),
        );
        assert_eq!(
            matches!(
                answer.verdict,
                graphhelm_execution::Verdict::NeedsYou { .. }
            ),
            !answer.reasons().is_empty(),
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
    let first = attention(&projection, &AttentionInputs::default());
    let second = attention(&projection, &AttentionInputs::default());
    assert_eq!(first, second, "the same projection answers identically");
    assert_eq!(
        first.reasons(),
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
        attention(&ExecutionProjection::default(), &AttentionInputs::default()),
        Attention {
            verdict: graphhelm_execution::Verdict::CanSleep,
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
        attention(&projection, &AttentionInputs::default()),
        Attention {
            verdict: graphhelm_execution::Verdict::CanSleep,
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
        attention(&projection, &AttentionInputs::default()),
        Attention {
            verdict: graphhelm_execution::Verdict::CanSleep,
        },
        "`start` is in the topology and has no state yet: it is queued work, not a wedge"
    );
}

#[test]
fn without_a_recorded_shape_no_wedge_is_claimed_and_no_calm_either() {
    // No record of the node set means no way to know whether work remains. Claiming a wedge
    // here would be inference-from-silence -- and so was the OLD expectation of this test,
    // which demanded `can_sleep`. Saying nothing and saying "fine" are the same sentence to
    // an operator, and this milestone deleted that equivalence everywhere else before
    // finding it still standing here, in a test written to defend honesty.
    let projection = projection(
        &[("finished-one", NodeState::Succeeded)],
        Some(SimulationStatus::Running),
    );
    let answer = attention(&projection, &AttentionInputs::default());
    assert!(
        !answer
            .reasons()
            .contains(&AttentionReason::WedgedQuiescence),
        "no basis to ASSERT a wedge without a record of what completeness means: {answer:?}"
    );
    assert_eq!(
        answer,
        Attention {
            verdict: graphhelm_execution::Verdict::Unknown {
                unevaluated: graphhelm_execution::NonEmpty::new(vec![
                    graphhelm_execution::Unevaluated::Execution {
                        reason: graphhelm_execution::ExecutionUnevaluated::NoRecordedNodeSet,
                        remedy: graphhelm_execution::Remedy::Unavailable {
                            because: graphhelm_execution::RemedyUnavailable::AmendmentDeclaresBoundsNotShape,
                        },
                    }
                ])
                .expect("one reason"),
            },
        },
        "and no basis to assert CALM either -- the question is unanswerable, which is its own \
         answer"
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
        attention(&projection, &AttentionInputs::default()).reasons(),
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
            attention(&projection, &AttentionInputs::default()),
            Attention {
                verdict: graphhelm_execution::Verdict::CanSleep,
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
        attention(&projection, &AttentionInputs::default()),
        Attention {
            verdict: graphhelm_execution::Verdict::CanSleep,
        },
        "nothing started is nothing to answer for"
    );
}

// ----------------------------------------------------------------------------------------
// M08 Task 1 (F1's twin): silence is a reason — and "I was not told the budget" is NOT
// silence. The contract closed on issue #63: the seam sees neither a clock nor a route
// timeout, so both are INJECTED; and an absent budget must be sayable, because "no silence
// reason" and "healthy" are otherwise the same answer — the defect this milestone exists
// to kill, one floor down.
// ----------------------------------------------------------------------------------------

use graphhelm_execution::attention::AttentionInputs;

/// The surface does the subtraction, not the seam: `core/execution` forbids `chrono` in
/// production dependencies (`the_execution_crate_has_no_impure_dependency`) AND the
/// projection folds no per-node timestamps at all — only `last_event_hash`. So no time type
/// may cross this boundary; ages arrive already computed, in seconds.
/// Budgets are keyed by NODE, not by node type: the `timeoutSeconds` the linter has always
/// demanded lives on the node, and a per-type map could only ever have been filled by a
/// number somebody invented.
fn inputs(ages: &[(&str, u64)], budgets: &[(&str, u64)]) -> AttentionInputs {
    AttentionInputs {
        node_silence_seconds: ages
            .iter()
            .map(|(node, age)| ((*node).to_owned(), *age))
            .collect(),
        silence_budget_seconds: budgets
            .iter()
            .map(|(node, budget)| ((*node).to_owned(), *budget))
            .collect(),
        at_sequence: Some(7),
    }
}

/// The guard B demanded, and the one the whole contract turns on: an empty budget map with
/// a node running long must NOT read like a healthy system.
#[test]
fn an_unbudgeted_running_node_says_it_was_not_evaluated() {
    let mut projection = projection(&[("start", NodeState::Running)], None);
    projection.execution_id = Some("exec-m08".to_owned());
    projection.current_graph = Some(published_graph());

    let answer = attention(&projection, &inputs(&[("start", 9_999)], &[]));

    assert_eq!(
        answer.silence_unevaluated(),
        vec![graphhelm_execution::Unevaluated::Node {
            node: "start".to_owned(),
            reason: graphhelm_execution::NodeUnevaluated::NoDeclaredBudget,
            remedy: graphhelm_execution::Remedy::DeclareNodeBudget {
                node: "start".to_owned(),
                observed_silence_seconds: 9999,
                computed_at_sequence: Some(7),
            },
        }],
        "an absent budget must be SAID, never rendered as calm: {answer:?}"
    );
    assert!(
        !matches!(
            answer.verdict,
            graphhelm_execution::Verdict::NeedsYou { .. }
        ),
        "missing configuration is not an emergency — it is an unknown"
    );
}

/// A node terminated long ago has no silence to evaluate: listing it would be the noise
/// that kills an honest field (the FIX-2 argument, pointed at this field).
#[test]
fn a_terminated_node_is_never_listed_as_unevaluated() {
    let mut projection = projection(&[("start", NodeState::Succeeded)], None);
    projection.execution_id = Some("exec-m08".to_owned());
    projection.current_graph = Some(published_graph());

    let answer = attention(&projection, &inputs(&[("start", 9_999)], &[]));

    assert!(
        answer.silence_unevaluated().is_empty(),
        "only work IN FLIGHT can be silent: {answer:?}"
    );
}

/// With a budget declared, a node in flight that has said nothing past its own deadline is
/// silent for real — not patient.
#[test]
fn a_running_node_past_its_declared_budget_reports_silence() {
    let mut projection = projection(&[("start", NodeState::Running)], None);
    projection.execution_id = Some("exec-m08".to_owned());
    projection.current_graph = Some(published_graph());

    let answer = attention(&projection, &inputs(&[("start", 9_999)], &[("start", 30)]));

    assert!(
        matches!(
            answer.verdict,
            graphhelm_execution::Verdict::NeedsYou { .. }
        ),
        "past its own deadline, silence is real: {answer:?}"
    );
    assert!(
        answer.silence_unevaluated().is_empty(),
        "it WAS evaluated — the budget was declared: {answer:?}"
    );
}

/// B's twin condition: v3 MOVED the likely omission. Before, the risk was an empty budget
/// map; now it is an empty MEASUREMENT map. A surface that declares budgets and forgets to
/// measure would produce zero silence reasons — the same lie one door further along.
#[test]
fn an_unmeasured_running_node_says_it_was_not_evaluated() {
    let mut projection = projection(&[("start", NodeState::Running)], None);
    projection.execution_id = Some("exec-m08".to_owned());
    projection.current_graph = Some(published_graph());

    let answer = attention(&projection, &inputs(&[], &[("start", 30)]));

    assert_eq!(
        answer.silence_unevaluated(),
        vec![graphhelm_execution::Unevaluated::Node {
            node: "start".to_owned(),
            reason: graphhelm_execution::NodeUnevaluated::NotMeasured,
            remedy: graphhelm_execution::Remedy::Unavailable {
                because: graphhelm_execution::RemedyUnavailable::SurfaceMeasuredNoAge,
            },
        }],
        "budget declared but nothing measured is still an UNKNOWN: {answer:?}"
    );
    assert!(
        !matches!(
            answer.verdict,
            graphhelm_execution::Verdict::NeedsYou { .. }
        ),
        "an unknown is not an alarm"
    );
}

/// M08 re-judge, the critical finding: the all-clear was published beside an admission that
/// the check justifying it had not run.
///
/// The judge caught `attentionRequired:false` and `attentionReasons:[]` in the SAME payload
/// as `silenceUnevaluated:['judge']`, on a node that had been running for minutes without an
/// append. That is not a false alarm traded for safety — it is FALSE CALM, which is worse:
/// an operator who reads "you can sleep" never reaches the third field.
///
/// The cause was the type. A boolean has two seats and we needed three, so "I don't know"
/// was seated in a side field while the headline kept answering "no". The milestone's own
/// principle says an unknown must be REPRESENTABLE — and representable BESIDE the claim is
/// not representable INSIDE it.
#[test]
fn unevaluated_silence_on_live_work_answers_unknown_not_calm() {
    let mut projection = graphhelm_events::ExecutionProjection {
        execution_id: Some("exec-unknown".to_owned()),
        ..graphhelm_events::ExecutionProjection::default()
    };
    projection
        .node_states
        .insert("judge".to_owned(), graphhelm_protocols::NodeState::Running);

    let answer = graphhelm_execution::attention(
        &projection,
        &graphhelm_execution::AttentionInputs::default(),
    );

    assert!(
        matches!(answer.verdict, graphhelm_execution::Verdict::Unknown { .. }),
        "a running node whose silence could not be judged is an UNKNOWN, never an all-clear: \
         {answer:?}"
    );
    assert!(
        answer.reasons().is_empty(),
        "unknown is not an alarm: nothing is claimed to be wrong, only unmeasured: {answer:?}"
    );
    assert_eq!(
        answer.silence_unevaluated(),
        vec![graphhelm_execution::Unevaluated::Node {
            node: "judge".to_owned(),
            reason: graphhelm_execution::NodeUnevaluated::NoDeclaredBudget,
            remedy: graphhelm_execution::Remedy::DeclareNodeBudget {
                node: "judge".to_owned(),
                observed_silence_seconds: 0,
                computed_at_sequence: None,
            },
        }],
        "and the unknown names the input it never received: {answer:?}"
    );
}

/// The verdict must be able to LEAVE unknown, or the tri-state is decoration.
///
/// The judge's third refusal was `attention='unknown'` on every read across six minutes: the
/// milestone had made the answer honest and left it permanently indeterminate, because no
/// budget ever reached the seam. A verdict that never leaves unknown is exactly as useless as
/// one that never enters it — the first lies by omission, the second by silence.
///
/// So this pins BOTH directions from one declared budget: under it, the operator may sleep
/// BECAUSE something was checked; over it, the node is named as silent.
#[test]
fn a_declared_budget_lets_the_verdict_leave_unknown_in_both_directions() {
    let mut projection = graphhelm_events::ExecutionProjection {
        execution_id: Some("exec-budgeted".to_owned()),
        ..graphhelm_events::ExecutionProjection::default()
    };
    projection
        .node_states
        .insert("build".to_owned(), graphhelm_protocols::NodeState::Running);

    let budgeted = |age: u64| graphhelm_execution::AttentionInputs {
        node_silence_seconds: [("build".to_owned(), age)].into_iter().collect(),
        silence_budget_seconds: [("build".to_owned(), 60)].into_iter().collect(),
        at_sequence: Some(7),
    };

    let calm = graphhelm_execution::attention(&projection, &budgeted(10));
    assert_eq!(
        calm.verdict,
        graphhelm_execution::Verdict::CanSleep,
        "inside its own declared bound, the node was CHECKED — that is what earns sleep: \
         {calm:?}"
    );
    assert!(
        calm.silence_unevaluated().is_empty(),
        "nothing is unevaluated once the operator declared the bound: {calm:?}"
    );

    let loud = graphhelm_execution::attention(&projection, &budgeted(600));
    assert!(
        matches!(loud.verdict, graphhelm_execution::Verdict::NeedsYou { .. }),
        "past its own declared bound the node is silent, and silence is a REASON: {loud:?}"
    );
    assert_eq!(
        loud.reasons(),
        vec![graphhelm_execution::AttentionReason::SilentNode {
            node: "build".to_owned()
        }],
        "and the reason names the node, so the answer is actionable: {loud:?}"
    );
}

/// The judge's fourth refusal, and the honest cure for it.
///
/// He read `silenceUnevaluated: ["judge"]` and called it bare jargon with no severity and no
/// remedy — and he was right. The verdict said "I don't know" and left the operator with
/// nowhere to go, on a story where the cure was one line of YAML they had never been told to
/// write.
///
/// The tempting answer was to argue that the unknown is CORRECT because nobody declared a
/// bound. It is correct — and correctness that strands the reader is still a dead end. An
/// unknown must carry WHY it could not be judged, because the why IS the remedy.
///
/// This is not a threshold invented to make the unknown disappear. The seam still refuses to
/// guess a bound; it just stops being mute about which input it never received.
#[test]
fn an_unknown_names_the_input_it_never_received() {
    let mut projection = graphhelm_events::ExecutionProjection {
        execution_id: Some("exec-cure".to_owned()),
        ..graphhelm_events::ExecutionProjection::default()
    };
    projection
        .node_states
        .insert("judge".to_owned(), graphhelm_protocols::NodeState::Running);

    // No budget declared for `judge`, but its age WAS measured: the missing half is the
    // declaration, and the answer must say so rather than shrugging.
    let answer = graphhelm_execution::attention(
        &projection,
        &graphhelm_execution::AttentionInputs {
            node_silence_seconds: [("judge".to_owned(), 400)].into_iter().collect(),
            silence_budget_seconds: std::collections::BTreeMap::new(),
            at_sequence: Some(7),
        },
    );

    assert!(matches!(
        answer.verdict,
        graphhelm_execution::Verdict::Unknown { .. }
    ));
    assert_eq!(
        answer.silence_unevaluated(),
        vec![graphhelm_execution::Unevaluated::Node {
            node: "judge".to_owned(),
            reason: graphhelm_execution::NodeUnevaluated::NoDeclaredBudget,
            remedy: graphhelm_execution::Remedy::DeclareNodeBudget {
                node: "judge".to_owned(),
                observed_silence_seconds: 400,
                computed_at_sequence: Some(7),
            },
        }],
        "the unknown must name the MISSING INPUT, because that is the operator's remedy: \
         {answer:?}"
    );

    // The mirror: a declared bound with no measurement is a DIFFERENT ignorance, and telling
    // the operator to declare a timeout they already declared would be worse than silence.
    let unmeasured = graphhelm_execution::attention(
        &projection,
        &graphhelm_execution::AttentionInputs {
            node_silence_seconds: std::collections::BTreeMap::new(),
            silence_budget_seconds: [("judge".to_owned(), 60)].into_iter().collect(),
            at_sequence: Some(7),
        },
    );
    assert_eq!(
        unmeasured.silence_unevaluated(),
        vec![graphhelm_execution::Unevaluated::Node {
            node: "judge".to_owned(),
            reason: graphhelm_execution::NodeUnevaluated::NotMeasured,
            remedy: graphhelm_execution::Remedy::Unavailable {
                because: graphhelm_execution::RemedyUnavailable::SurfaceMeasuredNoAge,
            },
        }],
        "two ignorances with different cures must not share one word: {unmeasured:?}"
    );
}

/// The wedge rule was structurally DEAD in production, and the cause was the same missing
/// record that starved the silence budget: it demanded `current_graph`, which is `None` on
/// every path anyone uses. It now reads the strongest record available.
///
/// Written against the reviewer's sabotage list, published BEFORE this code existed:
///
/// * **S1 (dual source)** — precedence is declared: a SEALED graph outranks a DECLARED form,
///   because it is stronger evidence about the same fact. Pinned below with both present and
///   disagreeing.
/// * **S2 (empty guard)** — the fixture leaves a node GENUINELY without state, so
///   `untouched_topology_work` is exercised rather than trivially false.
/// * **S3 (old journals)** — neither record present must stay silent, never accuse.
#[test]
fn the_wedge_reads_the_declared_form_and_prefers_the_sealed_one() {
    let mut projection = graphhelm_events::ExecutionProjection {
        execution_id: Some("exec-wedge".to_owned()),
        ..graphhelm_events::ExecutionProjection::default()
    };
    // One node finished, and a SECOND node the run never touched at all (S2): work the
    // topology promised and the execution never reached.
    projection.node_states.insert(
        "build".to_owned(),
        graphhelm_protocols::NodeState::Succeeded,
    );

    // S3: with NO record of the complete set, the rule must stay silent. It cannot know
    // whether anything is missing, and an accusation it cannot support is worse than none.
    let blind = graphhelm_execution::attention(&projection, &AttentionInputs::default());
    assert!(
        !blind.reasons().contains(&AttentionReason::WedgedQuiescence),
        "with no record of the node set, the rule must not accuse: {blind:?}"
    );

    // Now the DECLARED form supplies the set, and `ship` has no state at all.
    projection.declared_form = Some(graphhelm_protocols::ExecutionFormDeclared {
        node_descriptors: Default::default(),
        topology: None,
        execution_id: graphhelm_protocols::OpaqueId::parse("exec-wedge").unwrap(),
        node_ids: vec![
            graphhelm_protocols::OpaqueId::parse("build").unwrap(),
            graphhelm_protocols::OpaqueId::parse("ship").unwrap(),
        ],
        node_timeout_seconds: std::collections::BTreeMap::new(),
        name: None,
        objective: None,
        executor: None,
        // These fixtures are about the declared form's OTHER fields; a graph that declares no
        // customs produces an empty map, which is what keeps each cell asking its own question.
        node_customs_budgets: std::collections::BTreeMap::new(),
    });
    let seen = graphhelm_execution::attention(&projection, &AttentionInputs::default());
    assert!(
        !seen.reasons().contains(&AttentionReason::WedgedQuiescence),
        "untouched topology work is NOT a wedge -- there is still something to do, which is \
         the M07 distinction this must not lose: {seen:?}"
    );

    // S1, PINNED rather than merely asserted in prose. Both records present and DISAGREEING:
    // the sealed graph knows only `start`; the declared form claims `build` and `ship`. The
    // sealed record wins. Precedence is proven OBSERVABLE below, not just documented -- a
    // precedence nobody can see is a comment, and comments were the defect this milestone
    // kept finding.
    projection.current_graph = Some(published_graph());
    projection.node_states.insert(
        "start".to_owned(),
        graphhelm_protocols::NodeState::Succeeded,
    );
    let sealed = graphhelm_execution::attention(&projection, &AttentionInputs::default());
    projection.current_graph = None;
    let declared = graphhelm_execution::attention(&projection, &AttentionInputs::default());
    assert_ne!(
        sealed.verdict, declared.verdict,
        "with every SEALED node finished the run is wedged, while the DECLARED form still \
         lists `ship` as untouched -- if these agreed, precedence would be decorative: \
         sealed={sealed:?} declared={declared:?}"
    );
}

/// S4 from the reviewer's sabotage list, and the last honest gap I knew of.
///
/// With NO record of the node set, a quiescent execution still answered `can_sleep` — and
/// "I cannot tell whether this is wedged" is not calm. It is the same defect this milestone
/// buried twice at other levels: not fabricating an alarm quietly became asserting calm.
///
/// The reviewer's attack on my first excuse was right: I said the payload was keyed by node
/// so an execution-scoped unknown had nowhere to live, and inventing a node id would be a lie
/// in the data. The defect was the mandatory `node: String` — so the TYPE widens, and no id
/// gets invented.
#[test]
fn a_run_whose_shape_was_never_recorded_is_not_calm() {
    let mut projection = graphhelm_events::ExecutionProjection {
        execution_id: Some("exec-shapeless".to_owned()),
        ..graphhelm_events::ExecutionProjection::default()
    };
    // Everything the fold knows about is finished, and NOTHING says what the full set was.
    projection.node_states.insert(
        "build".to_owned(),
        graphhelm_protocols::NodeState::Succeeded,
    );

    let answer = graphhelm_execution::attention(&projection, &AttentionInputs::default());

    assert!(
        matches!(answer.verdict, graphhelm_execution::Verdict::Unknown { .. }),
        "with no record of the node set, nothing can say the run is done -- and an \
         unanswerable question is never an all-clear: {answer:?}"
    );
    assert_eq!(
        answer.silence_unevaluated(),
        vec![graphhelm_execution::Unevaluated::Execution {
            reason: graphhelm_execution::ExecutionUnevaluated::NoRecordedNodeSet,
            remedy: graphhelm_execution::Remedy::Unavailable {
                because: graphhelm_execution::RemedyUnavailable::AmendmentDeclaresBoundsNotShape,
            },
        }],
        "and the unknown is EXECUTION-scoped, carrying no invented node id: {answer:?}"
    );
}

/// The remedy travels as DATA inside the answer, so no surface can explain a problem it
/// cannot offer to fix — and none can stay silent about a fix another one shows.
///
/// The judge's fifth finding: the reasons now give a CAUSE and no ACTION. Written against
/// the reviewer's list, published before this code existed:
///
/// * **R1 (default smuggling)** — the value the operator must DECIDE has no field at all.
///   Not an `Option` the seam could fill: there is nothing to fill. A suggested default
///   would turn "absent means unknown" into "absent means 300s" through the back door, which
///   is the defect this milestone killed three times.
/// * **R8 (execution-scoped unknown)** — it carries a remedy too, and that remedy says
///   explicitly that none exists. "No remedy" and "field forgotten" must not be the same
///   bytes; that is the day's rule applied to the field the day created.
#[test]
fn every_unknown_carries_a_remedy_and_never_a_suggested_value() {
    let mut projection = graphhelm_events::ExecutionProjection {
        execution_id: Some("exec-remedy".to_owned()),
        ..graphhelm_events::ExecutionProjection::default()
    };
    projection
        .node_states
        .insert("judge".to_owned(), graphhelm_protocols::NodeState::Running);

    let answer = graphhelm_execution::attention(
        &projection,
        &graphhelm_execution::AttentionInputs {
            node_silence_seconds: [("judge".to_owned(), 412)].into_iter().collect(),
            silence_budget_seconds: std::collections::BTreeMap::new(),
            at_sequence: Some(12),
        },
    );

    let unevaluated = answer.silence_unevaluated();
    assert_eq!(unevaluated.len(), 1, "{answer:?}");
    assert_eq!(
        unevaluated[0].remedy(),
        &graphhelm_execution::Remedy::DeclareNodeBudget {
            node: "judge".to_owned(),
            observed_silence_seconds: 412,
            computed_at_sequence: Some(12),
        },
        "the remedy names the declaration that is missing and the moment it was computed -- \
         and carries NO value for the operator to accept blindly: {answer:?}"
    );

    // R8: the execution-scoped unknown answers too, by saying there is nothing to offer.
    let shapeless = graphhelm_execution::attention(
        &graphhelm_events::ExecutionProjection {
            execution_id: Some("exec-shapeless".to_owned()),
            ..graphhelm_events::ExecutionProjection::default()
        },
        &AttentionInputs::default(),
    );
    assert_eq!(
        shapeless.silence_unevaluated()[0].remedy(),
        &graphhelm_execution::Remedy::Unavailable {
            because: graphhelm_execution::RemedyUnavailable::AmendmentDeclaresBoundsNotShape,
        },
        "a run already under way cannot be given the shape nobody recorded, and saying so is \
         not the same as forgetting the field: {shapeless:?}"
    );
}

// ---------------------------------------------------------------------------------------------
// M09 A: which states have judgeable silence.
//
// Today the seam asks `state == Running` and nothing else. A node that failed, was requeued and
// then sat in `queued` for minutes reads as healthy forever — the judge measured exactly that on
// `flaky_check`, and declaring a budget for it did not help, because the node never entered the
// question at all.
//
// The property is not "did it fail". It is "has anyone got to this node yet": `Invalidated`
// returns a COMPLETED node to the queue without any retryable failure, and a rule keyed on
// failure would leave it mute forever. `node_attempts` already folds exactly that — it counts
// entries into `Running`, never reports about a node — so the question is asked of the record
// that already answers it.
// ---------------------------------------------------------------------------------------------

fn silent_queued_node(attempts: u32) -> graphhelm_events::ExecutionProjection {
    let mut projection = graphhelm_events::ExecutionProjection {
        execution_id: Some("exec-m09-a".to_owned()),
        ..graphhelm_events::ExecutionProjection::default()
    };
    projection.node_states.insert(
        "flaky_check".to_owned(),
        graphhelm_protocols::NodeState::Queued,
    );
    if attempts > 0 {
        projection
            .node_attempts
            .insert("flaky_check".to_owned(), attempts);
    }
    projection
}

fn judged(projection: &graphhelm_events::ExecutionProjection) -> graphhelm_execution::Attention {
    graphhelm_execution::attention(
        projection,
        &graphhelm_execution::AttentionInputs {
            node_silence_seconds: [("flaky_check".to_owned(), 900)].into_iter().collect(),
            // Declared ON PURPOSE in both directions of the scissor: without a budget the
            // absence rule already keeps the node unevaluated, so a guard that omitted it
            // would pass by coincidence rather than by measuring the policy.
            silence_budget_seconds: [("flaky_check".to_owned(), 30)].into_iter().collect(),
            at_sequence: Some(11),
        },
    )
}

/// The judge's own case: requeued after it had already run, silent far past its declared bound.
#[test]
fn a_requeued_node_that_already_ran_has_judgeable_silence() {
    let answer = judged(&silent_queued_node(1));
    // At the grain the question has. The first version of this assertion searched the whole
    // debug rendering for the node id, and Agent B proved it vacuous by breaking the budget
    // lookup: the answer became `unknown`/`NoDeclaredBudget`, the id still appeared in the
    // string, and the guard stayed green while the product regressed into the exact defect
    // M08 spent nine judge runs removing. An assertion one level above what it measures is
    // the family this milestone is named after, and it bit inside the test written to enforce
    // the rule.
    assert!(
        answer
            .reasons()
            .contains(&graphhelm_execution::AttentionReason::SilentNode {
                node: "flaky_check".to_owned(),
            }),
        "a node that already ran and is sitting requeued past its bound is SILENT, and must be \
         named as such rather than merely appearing somewhere in the answer: {answer:?}"
    );
}

/// The other blade. A rule that simply included every `Queued` node would satisfy the test
/// above and turn every freshly started graph into a false-alarm factory, naming nodes nobody
/// has dispatched yet. A confident false alarm is how a rule dies.
#[test]
fn a_node_that_never_ran_is_not_judged_for_silence() {
    let answer = judged(&silent_queued_node(0));
    assert!(
        !format!("{answer:?}").contains("flaky_check"),
        "nobody has dispatched this node yet, so its quiet is not silence: {answer:?}"
    );
}

/// L7: including `Queued` must not turn "declared nothing" into calm. Absence stays absence.
#[test]
fn a_requeued_node_with_no_declared_bound_is_unevaluated_not_calm() {
    let answer = graphhelm_execution::attention(
        &silent_queued_node(1),
        &graphhelm_execution::AttentionInputs {
            node_silence_seconds: [("flaky_check".to_owned(), 900)].into_iter().collect(),
            silence_budget_seconds: std::collections::BTreeMap::new(),
            at_sequence: Some(11),
        },
    );
    let rendered = format!("{answer:?}");
    assert!(
        rendered.contains("NoDeclaredBudget") && rendered.contains("flaky_check"),
        "a requeued node nobody bounded is UNEVALUATED, never quiet: {rendered}"
    );
}

/// L2: the reason arm and `purchased_calm` ask ONE question. Two predicates for one question
/// is the defect this milestone paid for twice — two budget functions, then two "where were
/// you looking" counters. Sabotage: return `purchased_calm` to `== Running` and this falls,
/// because a calm BOUGHT over a requeued node becomes invisible while the reason arm sees it.
#[test]
fn a_requeued_node_whose_calm_was_bought_is_still_named() {
    let mut projection = silent_queued_node(1);
    projection.declared_form = Some(graphhelm_protocols::ExecutionFormDeclared {
        node_descriptors: Default::default(),
        topology: None,
        execution_id: graphhelm_protocols::OpaqueId::parse("exec-m09-a").unwrap(),
        node_ids: vec![graphhelm_protocols::OpaqueId::parse("flaky_check").unwrap()],
        node_timeout_seconds: [(
            graphhelm_protocols::OpaqueId::parse("flaky_check").unwrap(),
            30,
        )]
        .into_iter()
        .collect(),
        name: None,
        objective: None,
        executor: None,
        // These fixtures are about the declared form's OTHER fields; a graph that declares no
        // customs produces an empty map, which is what keeps each cell asking its own question.
        node_customs_budgets: std::collections::BTreeMap::new(),
    });
    projection.apply_amendment(graphhelm_protocols::ExecutionFormAmended {
        execution_id: graphhelm_protocols::OpaqueId::parse("exec-m09-a").unwrap(),
        node_timeout_seconds: [(
            graphhelm_protocols::OpaqueId::parse("flaky_check").unwrap(),
            100_000,
        )]
        .into_iter()
        .collect(),
        observed_silence_seconds: [(
            graphhelm_protocols::OpaqueId::parse("flaky_check").unwrap(),
            900,
        )]
        .into_iter()
        .collect(),
        computed_at_sequence: 11,
    });

    let answer = graphhelm_execution::attention(
        &projection,
        &graphhelm_execution::AttentionInputs {
            node_silence_seconds: [("flaky_check".to_owned(), 900)].into_iter().collect(),
            silence_budget_seconds: graphhelm_execution::effective_budgets(&projection),
            at_sequence: Some(12),
        },
    );
    assert!(
        format!("{answer:?}").contains("CalmedByAmendment"),
        "the ceiling was raised over a requeued node that had already blown a tighter one, and \
         that calm must not read as untroubled: {answer:?}"
    );
}

/// A DEFAULT FEED IS NOT AN ABSTENTION -- it is a false claim about the GRAPH (#1013).
///
/// Five mutation replies and one serve route passed `AttentionInputs::default()` and explained
/// it as "nothing measured here on purpose". That reasoning is right about the AGE and wrong
/// about the BUDGET: the budget is not measured by a surface, it is declared in the graph the
/// projection already holds. An empty `silence_budget_seconds` sends `attention` down the
/// `(None, measured)` arm, which answers `NoDeclaredBudget` and hands the operator the remedy
/// "declare a budget for this node" -- for a node whose budget they declared.
///
/// Found by G, on #1013, by judging ONE projection twice. That is why this cell judges one
/// projection twice too: two fixtures could differ for a reason that is not the feed.
///
/// The `default()` half is not certifying a shipped defect -- no production site passes a
/// default any more. It is the CONTRAST that makes the other half mean something, and it is
/// what falls if someone reverts a surface to `default()` believing it abstains.
#[test]
fn a_default_feed_claims_no_budget_where_the_graph_declares_one() {
    let mut projection = silent_queued_node(1);
    projection.declared_form = Some(graphhelm_protocols::ExecutionFormDeclared {
        node_descriptors: Default::default(),
        topology: None,
        execution_id: graphhelm_protocols::OpaqueId::parse("exec-m09-a").unwrap(),
        node_ids: vec![graphhelm_protocols::OpaqueId::parse("flaky_check").unwrap()],
        node_timeout_seconds: [(
            graphhelm_protocols::OpaqueId::parse("flaky_check").unwrap(),
            30,
        )]
        .into_iter()
        .collect(),
        // #134 added these three to `ExecutionFormDeclared` after this branch was cut. `None` for
        // all three on purpose: this cell is about a DECLARED BUDGET being visible to a surface
        // that passed `default()`, and a name, an objective or an executor would be three more
        // things the reader has to rule out before believing the verdict. The absent case is the
        // one that keeps the contrast clean.
        name: None,
        objective: None,
        executor: None,
        // These fixtures are about the declared form's OTHER fields; a graph that declares no
        // customs produces an empty map, which is what keeps each cell asking its own question.
        node_customs_budgets: std::collections::BTreeMap::new(),
    });

    // CONTROL: the budget really is declared. Without this the two verdicts below could differ
    // because the fixture declares nothing, which would measure the fixture and not the feed.
    assert_eq!(
        graphhelm_execution::effective_budgets(&projection).get("flaky_check"),
        Some(&30),
        "the fixture must declare a budget or neither assertion below means anything"
    );

    let defaulted = graphhelm_execution::attention(
        &projection,
        &graphhelm_execution::AttentionInputs::default(),
    );
    assert_eq!(
        defaulted.silence_unevaluated(),
        vec![graphhelm_execution::Unevaluated::Node {
            node: "flaky_check".to_owned(),
            reason: graphhelm_execution::NodeUnevaluated::NoDeclaredBudget,
            remedy: graphhelm_execution::Remedy::DeclareNodeBudget {
                node: "flaky_check".to_owned(),
                observed_silence_seconds: 0,
                computed_at_sequence: None,
            },
        }],
        "a default feed must be shown SAYING there is no declared budget: {defaulted:?}"
    );

    let fed = graphhelm_execution::attention(
        &projection,
        &graphhelm_execution::AttentionInputs::for_surface(
            &projection,
            std::collections::BTreeMap::new(),
            None,
        ),
    );
    assert_eq!(
        fed.silence_unevaluated(),
        vec![graphhelm_execution::Unevaluated::Node {
            node: "flaky_check".to_owned(),
            reason: graphhelm_execution::NodeUnevaluated::NotMeasured,
            remedy: graphhelm_execution::Remedy::Unavailable {
                because: graphhelm_execution::RemedyUnavailable::SurfaceMeasuredNoAge,
            },
        }],
        "the shared feed must report the budget present and the age unmeasured: {fed:?}"
    );
}
