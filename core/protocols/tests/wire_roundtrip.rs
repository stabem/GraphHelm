use chrono::{TimeZone, Utc};
use graphhelm_protocols::{
    Actor, ActorType, DraftOperation, DraftRejected, EventKind, ExecutionGraph, ExecutionMode,
    GraphDraft, GraphImported, GraphSourceKind, ManualOverride, NodeOutcome, NodeState, OpaqueId,
    PolicyWaiver, RawSha256, SafeCode, SemanticHash, SignalSeverity, SignalSourceKind,
    SimulationStarted, SimulationStatus, WaiverScope, WireHash,
};

#[test]
fn canonical_graph_wire_shape_round_trips() {
    let source = include_str!("../../../examples/graphs/manual-override-deploy.yaml");
    let value: serde_json::Value = serde_yaml_ng::from_str(source).unwrap();
    let graph: ExecutionGraph = serde_json::from_value(value).unwrap();

    assert_eq!(graph.metadata.version, 13);
    assert_eq!(graph.spec.nodes["deploy"].node_type.as_str(), "deploy");
    assert_eq!(
        serde_json::to_value(NodeState::WaitingCapacity).unwrap(),
        "waiting_capacity"
    );
}

#[test]
fn unknown_node_fields_survive_round_trip_but_unknown_types_fail() {
    let source = include_str!("../../../examples/graphs/manual-override-deploy.yaml");
    let value: serde_json::Value = serde_yaml_ng::from_str(source).unwrap();
    let graph: ExecutionGraph = serde_json::from_value(value.clone()).unwrap();
    let round_trip = serde_json::to_value(graph).unwrap();
    assert_eq!(
        round_trip["spec"]["nodes"]["deploy"]["targetRef"],
        "environment://staging"
    );

    let mut unknown = value;
    unknown["spec"]["nodes"]["deploy"]["type"] = "future_node".into();
    assert!(serde_json::from_value::<ExecutionGraph>(unknown).is_err());
}

#[test]
fn draft_override_and_waiver_use_stable_camel_case_fields() {
    let override_request = ManualOverride {
        actor: Actor::new(ActorType::Owner, "owner-local"),
        reason: "release is explicitly accepted".into(),
        waived_requirements: vec!["review".into()],
        acknowledged_risks: vec!["unreviewed change".into()],
        scope: WaiverScope::Execution,
    };
    let draft = GraphDraft {
        id: "draft-1".into(),
        expected_version: 13,
        expected_hash: SemanticHash::new("sha256:abc"),
        operations: vec![DraftOperation::RemoveNode {
            id: "review".into(),
        }],
        manual_override: Some(override_request.clone()),
    };
    let draft_value = serde_json::to_value(&draft).unwrap();
    assert_eq!(draft_value["expectedVersion"], 13);
    assert_eq!(draft_value["operations"][0]["op"], "removeNode");
    assert_eq!(draft_value["operations"][0]["path"], "/spec/nodes/review");
    assert!(draft_value["operations"][0].get("id").is_none());
    assert_eq!(
        draft_value["manualOverride"]["waivedRequirements"][0],
        "review"
    );

    let waiver = PolicyWaiver {
        id: "waiver-1".into(),
        requirement: "review".into(),
        execution_id: "exec-1".into(),
        graph_version: 14,
        actor: override_request.actor.id.clone(),
        reason: Some(override_request.reason),
        acknowledged_risks: override_request.acknowledged_risks,
        scope: WaiverScope::Execution,
        created_at: Utc.with_ymd_and_hms(2026, 8, 8, 12, 0, 0).unwrap(),
        expires_at: None,
    };
    let waiver_value = serde_json::to_value(&waiver).unwrap();
    assert_eq!(waiver_value["executionId"], "exec-1");
    assert_eq!(waiver_value["graphVersion"], 14);
    assert_eq!(waiver_value["acknowledgedRisks"][0], "unreviewed change");
    assert_eq!(
        serde_json::from_value::<PolicyWaiver>(waiver_value).unwrap(),
        waiver
    );
}

#[test]
fn normative_add_edge_uses_value_without_path() {
    let value = serde_json::json!({
        "op": "addEdge",
        "value": {
            "id": "a-to-b",
            "from": "a",
            "to": "b",
            "type": "control",
            "map": {}
        }
    });
    let operation: DraftOperation = serde_json::from_value(value).unwrap();
    let encoded = serde_json::to_value(operation).unwrap();
    assert_eq!(encoded["value"]["id"], "a-to-b");
    assert!(encoded.get("path").is_none());
}

/// The sixteen wire spellings, written out BY HAND from the enum declaration.
///
/// DELIBERATELY NOT GENERATED, and this is the same argument `core/governor/tests/memory.rs` makes
/// for `STATE_NAMES` since #362. It looks exactly like the duplication this repository keeps
/// removing, and it is the opposite: **serde's spelling and `wire_name()`'s spelling both expand
/// from the SAME `$wire` literal in `wire_vocabulary!`**, so any check that compares those two is
/// comparing a value with itself and is green by construction. `Ghost => "ghostx"` satisfies it.
///
/// These names are the only statement in this file that can CONTRADICT the generated one. A reader
/// who deletes them as redundant would be applying the house rule correctly and removing the sole
/// independent witness. Update them by reading the enum, never by copying from a failure message.
///
/// (Found by N reviewing #408: the cell below was written to justify moving `NodeState` onto
/// per-variant literals, and could not have caught a mistyped literal. The move is right; the
/// evidence offered for it was not.)
const NODE_STATE_WIRE_NAMES: [&str; 16] = [
    "draft",
    "ghost",
    "linting",
    "ready",
    "queued",
    "running",
    "waiting_input",
    "waiting_capacity",
    "paused",
    "blocked",
    "succeeded",
    "failed",
    "waived",
    "skipped",
    "cancelled",
    "invalidated",
];

/// The hand-read spellings against the generated ones, IN BOTH DIRECTIONS.
///
/// A one-way check passes when the generated list grows, and a message reporting only two counts
/// invites the next reader to "fix" the hand-written side in whichever direction turns it green --
/// so each side's difference is named separately.
#[test]
fn the_hand_read_wire_spellings_agree_with_the_generated_ones() {
    let generated: Vec<&str> = NodeState::every()
        .iter()
        .map(|state| state.wire_name())
        .collect();

    let missing_from_generated: Vec<&&str> = NODE_STATE_WIRE_NAMES
        .iter()
        .filter(|name| !generated.contains(*name))
        .collect();
    let missing_from_hand: Vec<&&str> = generated
        .iter()
        .filter(|name| !NODE_STATE_WIRE_NAMES.contains(name))
        .collect();

    assert!(
        missing_from_generated.is_empty() && missing_from_hand.is_empty(),
        "the wire spellings disagree with the hand-read declaration.\n  \
         hand-written ({}): {NODE_STATE_WIRE_NAMES:?}\n  \
         generated ({}): {generated:?}\n  \
         hand-written but NOT generated: {missing_from_generated:?}\n  \
         generated but NOT hand-written: {missing_from_hand:?}",
        NODE_STATE_WIRE_NAMES.len(),
        generated.len()
    );
}

/// Serde APPLIES the rename and reads it back — which is NOT a check on the spelling.
///
/// **Stated narrowly because the first version of this doc claimed more than the code does.** It
/// compares `to_value(state)` against `wire_name()`, and both expand from the same `$wire` literal
/// in the macro, so a mistyped literal moves both sides together and this stays green. What it
/// does prove is that the `#[serde(rename = ...)]` the macro emits is actually honoured — a derive
/// or attribute change that dropped it would show up here — and that whatever serde emits is
/// something serde will accept back.
///
/// The spelling itself is guarded by [`NODE_STATE_WIRE_NAMES`] above, which has an independent
/// origin. Both cells iterate `every()`, so they are exhaustive by construction: a variant added
/// tomorrow is covered without anyone remembering.
#[test]
fn every_node_state_serialises_as_its_wire_name_and_reads_back() {
    for state in NodeState::every() {
        let encoded = serde_json::to_value(state).unwrap();
        assert_eq!(
            encoded,
            serde_json::Value::String(state.wire_name().to_owned()),
            "{state:?} serialises as {encoded} but wire_name() reports {:?}. serde and wire_name \
             are two producers of one spelling, and this is where they are compared",
            state.wire_name()
        );
        let decoded: NodeState = serde_json::from_value(encoded.clone())
            .unwrap_or_else(|e| panic!("{state:?} emits {encoded} which serde will not read: {e}"));
        assert_eq!(
            decoded, *state,
            "{state:?} did not survive a round trip through {encoded}"
        );
    }
}

/// `NodeState` is no longer sampled here, and its five entries were REPLACED rather than dropped.
///
/// They were a hand-written witness against serde's output, which is the right shape — but a
/// sample of five out of sixteen. [`NODE_STATE_WIRE_NAMES`] is the same witness over all sixteen,
/// and the chain closes through the round-trip cell: hand agrees with `wire_name`, `wire_name`
/// agrees with serde, so hand agrees with serde. Keeping five of them here as well would leave two
/// cells covering one property with no stated division of labour, which is how a reader ends up
/// improving the weaker one. (#408, N's review.)
#[test]
fn normative_statuses_and_event_kinds_have_exact_wire_names() {
    // All thirteen, not a sample. The point of this loop is pinning snake_case conversion, and
    // leaving NeedsInput out while pinning its sibling NeedsCapacity would miss exactly the
    // multi-word case it exists to catch.
    let outcomes = [
        (NodeOutcome::Started, "started"),
        (NodeOutcome::Succeeded, "succeeded"),
        (NodeOutcome::RetryableFailure, "retryable_failure"),
        (NodeOutcome::TerminalFailure, "terminal_failure"),
        (NodeOutcome::NeedsInput, "needs_input"),
        (NodeOutcome::NeedsCapacity, "needs_capacity"),
        (NodeOutcome::Approved, "approved"),
        (NodeOutcome::Waived, "waived"),
        (NodeOutcome::Skipped, "skipped"),
        (NodeOutcome::Cancelled, "cancelled"),
        (NodeOutcome::Invalidated, "invalidated"),
        (NodeOutcome::Paused, "paused"),
        (NodeOutcome::Interrupted, "interrupted"),
    ];
    for (outcome, expected) in outcomes {
        assert_eq!(serde_json::to_value(outcome).unwrap(), expected);
        assert_eq!(
            serde_json::from_value::<NodeOutcome>(serde_json::json!(expected)).unwrap(),
            outcome
        );
    }

    for (status, expected) in [
        (SimulationStatus::Running, "running"),
        (SimulationStatus::Completed, "completed"),
        (SimulationStatus::Failed, "failed"),
        (SimulationStatus::Paused, "paused"),
        (SimulationStatus::Cancelled, "cancelled"),
    ] {
        assert_eq!(serde_json::to_value(status).unwrap(), expected);
    }

    let kinds = [
        (
            EventKind::GraphImported(GraphImported {
                source_sha256: RawSha256::parse("a".repeat(64)).unwrap(),
                source_kind: GraphSourceKind::GraphDocument,
            }),
            "graph_imported",
        ),
        (
            EventKind::DraftRejected(DraftRejected {
                draft_id: OpaqueId::parse("draft-1").unwrap(),
                reason_code: SafeCode::parse("blocked").unwrap(),
                diagnostics: vec![],
                detail_evidence_id: None,
            }),
            "draft_rejected",
        ),
        (
            EventKind::SimulationStarted(SimulationStarted {
                simulation_id: OpaqueId::parse("simulation-1").unwrap(),
                graph_version: 1,
                graph_hash: WireHash::parse(format!("sha256:{}", "b".repeat(64))).unwrap(),
            }),
            "simulation_started",
        ),
    ];
    for (kind, expected) in kinds {
        assert_eq!(serde_json::to_value(kind).unwrap()["type"], expected);
    }

    for (mode, expected) in [
        (ExecutionMode::Autopilot, "autopilot"),
        (ExecutionMode::Supervised, "supervised"),
        (ExecutionMode::Manual, "manual"),
    ] {
        assert_eq!(serde_json::to_value(mode).unwrap(), expected);
        assert_eq!(
            serde_json::from_value::<ExecutionMode>(serde_json::json!(expected)).unwrap(),
            mode
        );
    }
    assert!(serde_json::from_value::<ExecutionMode>(serde_json::json!("god_mode")).is_err());

    for (severity, expected) in [
        (SignalSeverity::Low, "low"),
        (SignalSeverity::Medium, "medium"),
        (SignalSeverity::High, "high"),
        (SignalSeverity::Critical, "critical"),
    ] {
        assert_eq!(serde_json::to_value(severity).unwrap(), expected);
    }
    for (source, expected) in [
        (SignalSourceKind::Node, "node"),
        (SignalSourceKind::Runtime, "runtime"),
        (SignalSourceKind::Tool, "tool"),
        (SignalSourceKind::Test, "test"),
        (SignalSourceKind::User, "user"),
        (SignalSourceKind::Dream, "dream"),
        (SignalSourceKind::System, "system"),
    ] {
        assert_eq!(serde_json::to_value(source).unwrap(), expected);
        assert_eq!(
            serde_json::from_value::<SignalSourceKind>(serde_json::json!(expected)).unwrap(),
            source
        );
    }
}

/// M07 F3, the guard that protects every stream ever written: an outcome event recorded
/// before causes existed must re-serialize BYTE-IDENTICALLY, because replay re-serializes
/// each deserialized envelope and recomputes its hash against the stored one
/// (`core/events/src/projection.rs`). If `reason` were emitted as an explicit `null`, every
/// pre-M07 event's hash would change and every existing stream — the committed acceptance
/// evidence included — would fail replay with an integrity error. `skip_serializing_if` is
/// therefore load-bearing, not cosmetic, and this test is why it can never be removed.
#[test]
fn an_outcome_written_before_causes_existed_round_trips_byte_identically() {
    let stored = serde_json::json!({
        "executionId": "exec-1",
        "nodeId": "implement",
        "outcome": "retryable_failure",
        "nextState": "queued",
    });
    let parsed: graphhelm_protocols::NodeOutcomeRecorded =
        serde_json::from_value(stored.clone()).expect("an old outcome still deserializes");
    assert_eq!(
        parsed.reason, None,
        "absence reads as 'written before causes'"
    );
    assert_eq!(
        serde_json::to_value(&parsed).unwrap(),
        stored,
        "re-serializing must not add a key: the event hash is computed over these bytes"
    );
}

/// The other half: a cause, when present, rides the wire under its snake_case name.
#[test]
fn a_recorded_cause_uses_its_stable_wire_name() {
    let value = serde_json::json!({
        "executionId": "exec-1",
        "nodeId": "implement",
        "outcome": "retryable_failure",
        "nextState": "queued",
        "reason": "provider_unavailable",
    });
    let parsed: graphhelm_protocols::NodeOutcomeRecorded =
        serde_json::from_value(value.clone()).expect("a caused outcome deserializes");
    assert_eq!(
        parsed.reason,
        Some(graphhelm_protocols::NodeOutcomeReason::ProviderUnavailable)
    );
    assert_eq!(serde_json::to_value(&parsed).unwrap(), value);
}

// -----------------------------------------------------------------------------------
// M08: the per-node timeout must SURVIVE persistence. The user declares it, our own
// linter demands it (GHG101_DEFAULT_TIMEOUT), and the store threw it away — so the
// silence budget could never be derived from what the user already told us. This is not
// a new policy; it is a declaration that did not survive the store.
// -----------------------------------------------------------------------------------

/// (1) A node WITH a declared timeout survives the round trip.
#[test]
fn a_declared_node_timeout_survives_persistence() {
    let value = serde_json::json!({
        "nodeType": "agent",
        "optionality": "required",
        "controls": [],
        "contentSlotIds": [],
        "timeoutSeconds": 300,
    });
    let node: graphhelm_protocols::PersistedNode =
        serde_json::from_value(value.clone()).expect("a node with a timeout deserializes");
    assert_eq!(
        node.timeout_seconds(),
        Some(300),
        "the declaration the user made must reach the store"
    );
    assert_eq!(serde_json::to_value(&node).unwrap(), value);
}

/// (2) Absence stays ABSENCE: never zero, never a default. Downstream it becomes
/// `unknown`, which is the only honest verdict for work nobody set a bound on.
#[test]
fn an_undeclared_node_timeout_stays_absent_and_never_becomes_zero() {
    let value = serde_json::json!({
        "nodeType": "tool",
        "optionality": "required",
        "controls": [],
        "contentSlotIds": [],
    });
    let node: graphhelm_protocols::PersistedNode =
        serde_json::from_value(value.clone()).expect("an old node still deserializes");
    assert_eq!(
        node.timeout_seconds(),
        None,
        "no declaration is not a budget of zero — silence must read as unknown, not calm"
    );
    assert_eq!(
        serde_json::to_value(&node).unwrap(),
        value,
        "and it re-serializes byte-identically: every graph version already published \
         must keep validating and keep its hash"
    );
}

/// M11 #160: customs budgets inherit `timeout_seconds`' absence rule EXACTLY, and this pair is
/// the guard that says so.
///
/// The trap is specific and it is silent. Every graph version published before customs existed
/// has a `PersistedNode` with no `customs` key. Once the sweep computes stage deadlines as
/// `occurred_at + budget`, a missing budget read as ZERO makes the deadline equal to the moment
/// the stage was entered — so every stage of every historical replay is instantly overdue, and
/// the system would raise exceptions against work that was never bounded by anyone. That reads
/// as a flood of real findings, not as a bug, which is what makes it worth a guard rather than a
/// comment.
///
/// Byte-identical re-serialization is the second half and is not decoration: replay re-serializes
/// every event and re-hashes it, so a `customs: null` emitted where the stored bytes had no key
/// would break the hash chain of every version already published. `skip_serializing_if` is what
/// prevents it, and this assertion is what would notice if it were ever removed.
#[test]
fn an_undeclared_customs_budget_stays_absent_and_never_becomes_zero() {
    let value = serde_json::json!({
        "nodeType": "agent",
        "optionality": "required",
        "controls": [],
        "contentSlotIds": [],
    });
    let node: graphhelm_protocols::PersistedNode =
        serde_json::from_value(value.clone()).expect("a pre-customs node still deserializes");
    assert!(
        node.customs().is_none(),
        "no declaration is not a budget of zero — a stage nobody bounded has NO deadline, and \
         reading absence as zero would make every historical stage instantly overdue"
    );
    assert_eq!(
        serde_json::to_value(&node).unwrap(),
        value,
        "and it re-serializes byte-identically: every graph version already published must keep \
         its hash, which an always-emitted null would break"
    );
}

/// The declared half of the same pair: a budget that IS written reaches the store intact, so the
/// absence assertion above cannot pass by the field being unreadable for everyone.
#[test]
fn a_declared_customs_budget_survives_persistence() {
    let value = serde_json::json!({
        "nodeType": "agent",
        "optionality": "required",
        "controls": [],
        "contentSlotIds": [],
        "customs": {
            "waitWithinSeconds": 3600,
            "clearanceWithinSeconds": 900,
        },
    });
    let node: graphhelm_protocols::PersistedNode =
        serde_json::from_value(value.clone()).expect("a node with customs budgets deserializes");
    let customs = node.customs().expect("the declaration reaches the store");
    assert_eq!(customs.wait_within_seconds(), 3600);
    assert_eq!(customs.clearance_within_seconds(), 900);
    assert_eq!(
        customs.dlq_within_seconds(),
        None,
        "an absent dead-letter budget is absent, not zero: the dead-letter state IS the exception"
    );
    assert_eq!(serde_json::to_value(&node).unwrap(), value);
}

/// (3) An unreadable value is NOT a budget. A negative or non-numeric timeout must arrive
/// downstream as absent, or the surface would publish calm over a number nobody can read.
#[test]
fn an_unreadable_node_timeout_is_refused_never_silently_accepted() {
    for hostile in [
        serde_json::json!("soon"),
        serde_json::json!(-1),
        serde_json::json!(1.5),
    ] {
        let value = serde_json::json!({
            "nodeType": "agent",
            "optionality": "required",
            "controls": [],
            "contentSlotIds": [],
            "timeoutSeconds": hostile,
        });
        let parsed: Result<graphhelm_protocols::PersistedNode, _> = serde_json::from_value(value);
        assert!(
            parsed.is_err(),
            "an unreadable timeout ({hostile}) must be REFUSED at the boundary, never \
             accepted as a bound the operator would be told to trust"
        );
    }
}

/// #162: the stage recorded on an `overdue_exception` must speak the SAME WIRE VOCABULARY as
/// the fold's customs timeline, even though the two are different types in different crates on
/// purpose (issue #162 amendment: wire layer vs projection layer).
///
/// This lane originally minted `waiting` for the parked stage while #160's projection enum
/// spells it `parked`. Two spellings for one stage is the defect that started the exchange —
/// `claimed` meaning two things on the wire — so the vocabularies are held equal by test.
///
/// The assertion is on the SET of emitted strings rather than on each variant individually: a
/// test that said `Parked` serialises as `parked` would only be restating `rename_all`.
///
/// TWO MUTATIONS, TWO DETECTORS, and they are not the same detector — measured, because the
/// first sabotage run did not fall where this comment originally claimed it would:
///   * renaming the VARIANT (`Parked` -> `Waiting`) breaks the `use` above: a COMPILE error,
///     never this assertion;
///   * changing the SPELLING (`#[serde(rename = "waiting")]`, variant untouched) leaves the
///     import intact and falls HERE, printing both sets.
///
/// Only the second is this assertion's own. Saying so beats implying one guard covers both.
#[test]
fn overdue_stage_emits_exactly_the_customs_wire_vocabulary() {
    use graphhelm_protocols::CustomsStage::{Claimed, DeadLettered, Parked};

    let emitted: std::collections::BTreeSet<String> = [Parked, Claimed, DeadLettered]
        .into_iter()
        .map(|stage| {
            serde_json::to_value(stage)
                .unwrap()
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();

    let expected: std::collections::BTreeSet<String> = ["parked", "claimed", "dead_lettered"]
        .into_iter()
        .map(str::to_owned)
        .collect();

    assert_eq!(
        emitted, expected,
        "the wire vocabulary must match #160's timeline spellings exactly"
    );
}
