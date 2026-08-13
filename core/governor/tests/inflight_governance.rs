//! Properties of the in-flight governance decisions in `core/governor/src/inflight.rs`.
//!
//! `decide_mutation` is a pure function over `(ExecutionProjection, AdmittedSignal)`. These tests
//! sweep the input space each property claims to hold across, rather than asserting single
//! examples, because the whole point of a pure decision function is that it owes the same answer
//! to every caller who hands it the same inputs.

use graphhelm_events::ExecutionProjection;
use graphhelm_execution::{MAX_ACCEPTED_MUTATIONS, MAX_SIGNALS_PER_EXECUTION};
use graphhelm_governor::{MutationDecision, RejectionReason, admit_signal, decide_mutation};
use graphhelm_protocols::ExecutionMode;

fn projection(mode: ExecutionMode, accepted: u32, signals: u32) -> ExecutionProjection {
    ExecutionProjection {
        execution_id: Some("execution-1".to_owned()),
        mode: Some(mode),
        accepted_mutations: accepted,
        signals_recorded: signals,
        ..ExecutionProjection::default()
    }
}

/// A recognized kind, so `may_propose_mutation` is true and the mode/budget checks are what
/// actually decide the outcome.
fn actionable_signal() -> serde_json::Value {
    serde_json::json!({
        "id": "signal-1",
        "source": {"type": "node", "id": "node-a"},
        "type": "no_progress",
        "severity": "high",
        "description": "a dependency was discovered",
        "evidence": ["exec-1"],
        "emittedAt": "2026-08-13T00:00:00Z"
    })
}

/// An unrecognized kind, so `may_propose_mutation` is false regardless of mode or counters.
fn unactionable_signal() -> serde_json::Value {
    serde_json::json!({
        "id": "signal-1",
        "source": {"type": "node", "id": "node-a"},
        "type": "invented_by_an_agent",
        "severity": "high",
        "description": "a dependency was discovered",
        "evidence": ["exec-1"],
        "emittedAt": "2026-08-13T00:00:00Z"
    })
}

/// Property 1 (decision 5.5): the mode in force *at the decision* governs it, never the mode in
/// force when the proposal was admitted.
///
/// The same admitted signal is decided against two projections that differ only in `mode` — one
/// built as if the execution's mode changed after admission. If `decide_mutation` remembered the
/// mode at admission time instead of consulting the projection it is handed, this would not catch
/// it; Step 2 below sabotages exactly that and confirms it does.
#[test]
fn mode_binds_at_the_decision_not_at_admission() {
    let started_in_autopilot = projection(ExecutionMode::Autopilot, 0, 0);
    let admitted = admit_signal(&started_in_autopilot, &actionable_signal()).unwrap();
    assert_eq!(
        decide_mutation(&started_in_autopilot, &admitted),
        MutationDecision::Accept
    );

    // The execution's mode switches to Manual after the signal was admitted. The proposal's age
    // never matters; only the mode in force right now.
    let switched_to_manual = projection(ExecutionMode::Manual, 0, 0);
    assert_eq!(
        decide_mutation(&switched_to_manual, &admitted),
        MutationDecision::Rejected(RejectionReason::ManualMode)
    );

    // Reverse direction: proposed under Manual, decided after switching to Autopilot.
    let started_in_manual = projection(ExecutionMode::Manual, 0, 0);
    let admitted_under_manual = admit_signal(&started_in_manual, &actionable_signal()).unwrap();
    let switched_to_autopilot = projection(ExecutionMode::Autopilot, 0, 0);
    assert_eq!(
        decide_mutation(&switched_to_autopilot, &admitted_under_manual),
        MutationDecision::Accept
    );
}

/// Property 2 (decision 5.4): an unrecognized kind can never mutate, in every mode and at every
/// counter extreme — the rejection is unconditional on everything else in the projection.
#[test]
fn the_unrecognized_kind_can_never_mutate() {
    let neutral = projection(ExecutionMode::Autopilot, 0, 0);
    let admitted = admit_signal(&neutral, &unactionable_signal()).unwrap();
    assert!(!admitted.may_propose_mutation);

    let counter_values = [0, 1, MAX_ACCEPTED_MUTATIONS - 1, MAX_ACCEPTED_MUTATIONS];
    let signal_values = [
        0,
        1,
        MAX_SIGNALS_PER_EXECUTION - 1,
        MAX_SIGNALS_PER_EXECUTION,
    ];
    for mode in [
        ExecutionMode::Autopilot,
        ExecutionMode::Supervised,
        ExecutionMode::Manual,
    ] {
        for &accepted in &counter_values {
            for &signals in &signal_values {
                let swept = projection(mode, accepted, signals);
                assert_eq!(
                    decide_mutation(&swept, &admitted),
                    MutationDecision::Rejected(RejectionReason::SignalNotActionable),
                    "mode={mode:?} accepted={accepted} signals={signals}"
                );
            }
        }
    }
}

/// Property 3 (decisions 5.1/5.7): `MAX_ACCEPTED_MUTATIONS` is a hard ceiling, in every mode; one
/// short of it, Autopilot still accepts.
///
/// `SignalNotActionable` is checked before `Blocked` in `decide_mutation`, so this uses an
/// actionable signal — an unactionable one would be rejected for that reason first and never
/// exercise the budget check at all.
#[test]
fn the_mutation_budget_is_a_hard_ceiling_in_every_mode() {
    for mode in [
        ExecutionMode::Autopilot,
        ExecutionMode::Supervised,
        ExecutionMode::Manual,
    ] {
        let at_ceiling = projection(mode, MAX_ACCEPTED_MUTATIONS, 0);
        let admitted = admit_signal(&at_ceiling, &actionable_signal()).unwrap();
        assert_eq!(
            decide_mutation(&at_ceiling, &admitted),
            MutationDecision::Blocked,
            "mode={mode:?}"
        );
    }

    let one_short = projection(ExecutionMode::Autopilot, MAX_ACCEPTED_MUTATIONS - 1, 0);
    let admitted = admit_signal(&one_short, &actionable_signal()).unwrap();
    assert_eq!(
        decide_mutation(&one_short, &admitted),
        MutationDecision::Accept
    );
}

/// Property 4: the same projection and signal always produce the same decision. Nothing in
/// `decide_mutation` reads a clock or any other hidden state.
#[test]
fn the_decision_is_deterministic() {
    for mode in [
        ExecutionMode::Autopilot,
        ExecutionMode::Supervised,
        ExecutionMode::Manual,
    ] {
        let projection = projection(mode, 3, 5);
        let admitted = admit_signal(&projection, &actionable_signal()).unwrap();
        let first = decide_mutation(&projection, &admitted);
        let second = decide_mutation(&projection, &admitted);
        assert_eq!(first, second, "mode={mode:?}");
    }
}
