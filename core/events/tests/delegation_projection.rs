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
    ActorId, Clock, DelegationChosen, DelegationEffort, DelegationPolicyId, DelegationTier,
    EventKind, ExecutionId, GateFinding, GateVerdict, IdGenerator, NewEvent, OpaqueId,
    PersistedActor, PersistedActorType, ProjectId, RepositoryScope, Sensitivity, SignalSeverity,
    SubagentBasis, SubagentKind, SubagentReused, WorkspaceId,
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
        policy: DelegationPolicyId::Routed,
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
    // `redChecks` is never on the wire (it is re-folded by every replay), so the restored
    // projection equals the replayed one minus that fold-time derivation.
    assert!(wire.get("redChecks").is_none());
    let restored: graphhelm_events::ExecutionProjection = serde_json::from_value(wire).unwrap();
    assert!(restored.red_checks.is_empty());
    let mut without_red_checks = projection.clone();
    without_red_checks.red_checks.clear();
    assert_eq!(restored, without_red_checks);
}

#[test]
fn a_history_without_delegation_projects_no_delegation_fields() {
    let projection = graphhelm_events::ExecutionProjection::default();
    let wire = serde_json::to_value(&projection).unwrap();
    // `delegationChoices` is skipped when empty; `redChecks` is never serialized (see the next
    // test for why that is what keeps pre-ADR-040 digests unchanged).
    assert!(wire.get("delegationChoices").is_none());
    assert!(wire.get("redChecks").is_none());
}

/// Defect guarded: `red_checks` is folded from `gate_verdict`, which predates ADR-040. If it were
/// serialized, every older history with a failing verdict would gain a `redChecks` key and its
/// projection digest would move. The fold still counts it (the dispatch path needs it); the wire
/// form is exactly what it was before the field existed.
#[test]
fn a_failing_gate_verdict_without_delegation_serializes_as_before_adr_040() {
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
                vec![event("event-1", verdict("implement", false))],
                vec![],
                vec![],
            )
            .unwrap(),
        )
        .unwrap();
    let history = store.read_replay_stream(&scope(), stream.as_str()).unwrap();
    let projection = replay(&scope(), stream.as_str(), &history).unwrap();
    assert_eq!(projection.red_checks.get("implement"), Some(&1));
    assert!(projection.delegation_choices.is_empty());

    let wire = serde_json::to_value(&projection).unwrap();
    let mut before = wire.clone();
    before.as_object_mut().unwrap().remove("redChecks");
    before.as_object_mut().unwrap().remove("delegationChoices");
    assert_eq!(wire, before, "no ADR-040 key reaches the wire");
}

fn subagent(node: &str, id: &str, basis: SubagentBasis) -> EventKind {
    EventKind::SubagentReused(SubagentReused {
        node_id: OpaqueId::parse(node).unwrap(),
        subagent_id: OpaqueId::parse(id).unwrap(),
        kind: SubagentKind::Implementer,
        graph_version: 1,
        basis,
        from_node_id: None,
        from_kind: None,
        tokens_used: None,
        tokens_allocated: None,
    })
}

/// #298 (ADR-041): the per-node subagent record -- the only authorship evidence reuse reads --
/// rebuilds from the journal, newest per node, and survives the persisted round trip. A history
/// with no such record serializes no `subagents` key, so older projection digests do not move.
#[test]
fn the_newest_subagent_record_per_node_rebuilds_from_events() {
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
                    event(
                        "event-2",
                        subagent("implement", "subagent-1", SubagentBasis::NoEligibleSubagent),
                    ),
                    event("event-3", chosen("implement", DelegationTier::Large, 1)),
                    event(
                        "event-4",
                        subagent("implement", "subagent-2", SubagentBasis::BoundUnavailable),
                    ),
                ],
                vec![],
                vec![],
            )
            .unwrap(),
        )
        .unwrap();
    let history = store.read_replay_stream(&scope(), stream.as_str()).unwrap();
    let projection = replay(&scope(), stream.as_str(), &history).unwrap();
    let record = &projection.subagents["implement"];
    assert_eq!(record.at_sequence, 4);
    assert_eq!(record.record.subagent_id.as_str(), "subagent-2");
    assert_eq!(projection.subagents.len(), 1);
    assert_eq!(
        projection,
        replay(&scope(), stream.as_str(), &history).unwrap()
    );
    let wire = serde_json::to_value(&projection).unwrap();
    assert!(wire.get("subagents").is_some());
    assert!(wire.get("startedGraphVersion").is_none());
    let restored: graphhelm_events::ExecutionProjection = serde_json::from_value(wire).unwrap();
    assert_eq!(restored.subagents, projection.subagents);

    let empty = serde_json::to_value(graphhelm_events::ExecutionProjection::default()).unwrap();
    assert!(empty.get("subagents").is_none());
}
