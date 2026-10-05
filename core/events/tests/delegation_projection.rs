//! #290 (ADR-040): the `delegation_chosen` projection is a replay of the journal and nothing else.
//!
//! The newest choice per node wins, a failing `gate_verdict` counts as one red check against the
//! node it names, and replaying the same history twice yields the same projection.

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use chrono::{TimeZone, Utc};
use graphhelm_events::{LocalEventRepository, PreparedAppend, replay};
use graphhelm_protocols::{
    ActorId, Clock, DelegationChosen, DelegationEffort, DelegationTier, EventKind, ExecutionId,
    GateFinding, GateVerdict, IdGenerator, NewEvent, OpaqueId, PersistedActor, PersistedActorType,
    ProjectId, RepositoryScope, Sensitivity, SignalSeverity, SubagentKind, WorkspaceId,
};

fn scope() -> RepositoryScope {
    RepositoryScope::new(
        WorkspaceId::parse("workspace-1").unwrap(),
        ProjectId::parse("project-1").unwrap(),
        Some(ExecutionId::parse("execution-1").unwrap()),
    )
}

struct FixedClock;
impl Clock for FixedClock {
    fn now(&self) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 5, 12, 0, 0).unwrap()
    }
}

#[derive(Default)]
struct Ids(AtomicU64);
impl IdGenerator for Ids {
    fn next_id(&self, prefix: &'static str) -> String {
        format!("{prefix}-{}", self.0.fetch_add(1, Ordering::SeqCst) + 1)
    }
}

fn event(key: &str, kind: EventKind) -> NewEvent {
    NewEvent::new(
        OpaqueId::parse(key).unwrap(),
        PersistedActor::new(
            PersistedActorType::System,
            ActorId::parse("system-1").unwrap(),
        ),
        Sensitivity::Internal,
        kind,
        vec![],
        vec![],
    )
}

fn chosen(node: &str, tier: DelegationTier, red_checks: u32) -> EventKind {
    EventKind::DelegationChosen(DelegationChosen {
        node_id: OpaqueId::parse(node).unwrap(),
        kind: SubagentKind::Implementer,
        tier,
        effort: if red_checks == 0 {
            DelegationEffort::Medium
        } else {
            DelegationEffort::High
        },
        escalated: red_checks > 0,
        red_checks,
    })
}

fn verdict(node: &str, passed: bool) -> EventKind {
    EventKind::GateVerdict(GateVerdict {
        execution_id: OpaqueId::parse("execution-1").unwrap(),
        node_id: OpaqueId::parse(node).unwrap(),
        gate_id: OpaqueId::parse("gate-1").unwrap(),
        passed,
        // The schema refuses a failing verdict with no findings: never pass/fail alone.
        findings: if passed {
            vec![]
        } else {
            vec![GateFinding {
                severity: SignalSeverity::High,
                claim: "the test suite is red".into(),
                evidence: vec![],
                remediation: "fix the failing test".into(),
            }]
        },
    })
}

#[test]
fn the_newest_delegation_choice_per_node_and_the_red_checks_rebuild_from_events() {
    let directory = tempfile::tempdir().unwrap();
    let store = LocalEventRepository::open(
        directory.path(),
        Arc::new(FixedClock),
        Arc::new(Ids::default()),
    )
    .unwrap();
    let stream = OpaqueId::parse("stream-1").unwrap();
    store
        .append_atomic(
            &PreparedAppend::new(
                scope(),
                stream.clone(),
                1,
                vec![
                    event("event-1", chosen("implement", DelegationTier::Standard, 0)),
                    event("event-2", verdict("implement", true)),
                    event("event-3", verdict("implement", false)),
                    event("event-4", chosen("implement", DelegationTier::Large, 1)),
                    event("event-5", chosen("explore", DelegationTier::Small, 0)),
                ],
                vec![],
                vec![],
            )
            .unwrap(),
        )
        .unwrap();
    let history = store.read_replay_stream(&scope(), stream.as_str()).unwrap();

    let projection = replay(&scope(), stream.as_str(), &history).unwrap();
    // A passing verdict is not a red check; the failing one is.
    assert_eq!(projection.red_checks.get("implement"), Some(&1));
    assert_eq!(projection.red_checks.get("explore"), None);
    // Last wins per node, and the record cites the sequence that carried it.
    let implement = &projection.delegation_choices["implement"];
    assert_eq!(implement.at_sequence, 4);
    assert_eq!(implement.chosen.tier, DelegationTier::Large);
    assert!(implement.chosen.escalated);
    assert_eq!(implement.chosen.red_checks, 1);
    assert_eq!(projection.delegation_choices["explore"].at_sequence, 5);
    assert_eq!(projection.delegation_choices.len(), 2);

    // Rebuildable: a second replay of the same journal is the same projection, and it survives a
    // serialize/deserialize round trip (the projection repository's persisted form).
    let again = replay(&scope(), stream.as_str(), &history).unwrap();
    assert_eq!(projection, again);
    let wire = serde_json::to_value(&projection).unwrap();
    assert!(wire.get("delegationChoices").is_some());
    let restored: graphhelm_events::ExecutionProjection = serde_json::from_value(wire).unwrap();
    assert_eq!(restored, projection);
}

#[test]
fn a_history_without_delegation_projects_no_delegation_fields() {
    let projection = graphhelm_events::ExecutionProjection::default();
    let wire = serde_json::to_value(&projection).unwrap();
    // Skipped when empty, so every projection digest written before ADR-040 is unchanged.
    assert!(wire.get("delegationChoices").is_none());
    assert!(wire.get("redChecks").is_none());
}
