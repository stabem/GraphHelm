//! In-flight governance decisions.
//!
//! Pure functions over the projection and 04a's bounds. Nothing here performs I/O, appends an
//! event, or externalizes Evidence: each function returns what should happen, and the driver (04f)
//! makes it happen. That split is what keeps every rule here property-testable offline.

use graphhelm_events::ExecutionProjection;
use graphhelm_execution::{MAX_ACCEPTED_MUTATIONS, MAX_SIGNALS_PER_EXECUTION, TypedSignal};
use graphhelm_protocols::{ExecutionMode, PolicyWaiver, SignalRecorded, WaiverScope};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GovernanceError {
    /// The signal failed validation against the contract the schema fixes.
    InvalidSignal,
    /// The execution has recorded `MAX_SIGNALS_PER_EXECUTION` signals. Per decision 5.7 this
    /// blocks the execution for an owner decision; the signal is refused, never dropped silently.
    SignalBudgetExhausted,
    /// No execution has started, so there is nothing to govern.
    NotStarted,
    /// An override must acknowledge at least one risk, or it is not an informed decision.
    UnacknowledgedRisk,
    /// The signal is schema-valid — evidence that must not be dropped — but its `id` or
    /// `source.id` is not an `OpaqueId`, so it cannot be recorded on the wire. The driver must
    /// still externalize the envelope and alert the owner; this is not the emitter's envelope
    /// being garbage, which is what `InvalidSignal` means.
    UnrecordableIdentity,
    /// The constructed waiver failed serialization or schema validation. Kept distinct from
    /// `InvalidSignal` because the two failures have unrelated causes and callers should not
    /// have to disambiguate a waiver failure from a malformed signal envelope.
    InvalidWaiver,
}

/// A signal the governor has admitted: what to record, what to externalize, and whether it may
/// ever become a mutation.
#[derive(Clone, Debug)]
pub struct AdmittedSignal {
    /// The event payload to append. Carries no free-form content, per D-036.
    pub record: SignalRecorded,
    /// The raw envelope bytes to externalize as encrypted Evidence, referenced by the event.
    pub externalize: Vec<u8>,
    /// Decision 5.4: false for an unrecognized kind, and nothing downstream may override it.
    pub may_propose_mutation: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RejectionReason {
    /// Manual mode: only the owner changes the graph (D-022).
    ManualMode,
    /// The signal's kind is unrecognized and can never propose a mutation (decision 5.4).
    SignalNotActionable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MutationDecision {
    /// Autopilot: the Governor may accept its own mutation.
    Accept,
    /// Supervised: the proposal stands until the owner approves it.
    RequiresApproval,
    /// The proposal is refused outright.
    Rejected(RejectionReason),
    /// `MAX_ACCEPTED_MUTATIONS` is spent. The execution blocks for an owner decision, in every
    /// mode, per decisions 5.1 and 5.7.
    Blocked,
}

/// Validates and admits one signal, or refuses it.
///
/// # Errors
/// `InvalidSignal` when the envelope violates the schema contract, or when its `id` or
/// `source.id` is not a valid `OpaqueId` even though the signal schema accepts it — the event
/// contract that carries the record on the wire is stricter than the signal contract, and that
/// stricter boundary wins; `SignalBudgetExhausted` when the execution has recorded its full
/// signal budget; `NotStarted` when no execution exists.
pub fn admit_signal(
    projection: &ExecutionProjection,
    envelope: &serde_json::Value,
) -> Result<AdmittedSignal, GovernanceError> {
    let execution_id = projection
        .execution_id
        .as_deref()
        .ok_or(GovernanceError::NotStarted)?;
    if projection.signals_recorded >= MAX_SIGNALS_PER_EXECUTION {
        return Err(GovernanceError::SignalBudgetExhausted);
    }
    let signal = TypedSignal::parse(envelope).map_err(|_| GovernanceError::InvalidSignal)?;
    let externalize = serde_json::to_vec(envelope).map_err(|_| GovernanceError::InvalidSignal)?;
    let record = build_signal_record(execution_id, &signal, &externalize)?;
    Ok(AdmittedSignal {
        record,
        externalize,
        may_propose_mutation: signal.can_propose_mutation(),
    })
}

/// Decides one admitted proposal under the mode in force right now (decision 5.5).
///
/// "Right now" means the projection folded to the append point. A driver that decides, lets a
/// `execution_mode_changed` land, and then appends the stale acceptance poisons the stream: the
/// fold rejects a mode-mismatched acceptance as corrupt on every subsequent replay. Re-derive the
/// decision against the projection as of the append, not as of the proposal.
#[must_use]
pub fn decide_mutation(
    projection: &ExecutionProjection,
    signal: &AdmittedSignal,
) -> MutationDecision {
    if !signal.may_propose_mutation {
        return MutationDecision::Rejected(RejectionReason::SignalNotActionable);
    }
    if projection.accepted_mutations >= MAX_ACCEPTED_MUTATIONS {
        return MutationDecision::Blocked;
    }
    match projection.mode {
        Some(ExecutionMode::Autopilot) => MutationDecision::Accept,
        Some(ExecutionMode::Supervised) => MutationDecision::RequiresApproval,
        // No mode and Manual read the same: the Governor does not act on its own.
        Some(ExecutionMode::Manual) | None => {
            MutationDecision::Rejected(RejectionReason::ManualMode)
        }
    }
}

fn build_signal_record(
    execution_id: &str,
    signal: &TypedSignal,
    externalize: &[u8],
) -> Result<SignalRecorded, GovernanceError> {
    // Reuses the graph crate's digest helper — the same `sha2`/`hex` combination `apply.rs` and
    // `externalize.rs` already use through it — rather than adding `sha2`/`hex` as direct
    // dependencies of this crate. `graphhelm-graph` is already a dependency.
    let envelope_sha256 = graphhelm_graph::raw_content_sha256(externalize)
        .map_err(|_| GovernanceError::InvalidSignal)?;
    Ok(SignalRecorded {
        execution_id: graphhelm_protocols::OpaqueId::parse(execution_id)
            .map_err(|_| GovernanceError::InvalidSignal)?,
        signal_id: graphhelm_protocols::OpaqueId::parse(signal.id())
            .map_err(|_| GovernanceError::UnrecordableIdentity)?,
        source_kind: signal.source().kind(),
        source_id: graphhelm_protocols::OpaqueId::parse(signal.source().id())
            .map_err(|_| GovernanceError::UnrecordableIdentity)?,
        kind: signal.raw_kind().to_owned(),
        severity: signal.severity(),
        envelope_sha256,
    })
}

/// Builds the M03 waiver for an owner override during execution, per decision 5.8: the same
/// contract, additionally bound to the node and the obligation it clears.
///
/// # Errors
/// `NotStarted` with no execution; `UnacknowledgedRisk` when no risk is acknowledged;
/// A projection with no published graph cannot produce a valid waiver: the waiver schema requires
/// `graphVersion >= 1` and there is no version to bind. That surfaces as `InvalidWaiver`.
///
/// # Errors
/// `InvalidWaiver` when the constructed waiver fails serialization or the schema's own
/// validation — which per decision 5.8 means the construction here is wrong, not the validator.
/// One guaranteed case: a projection with no published graph, because the waiver schema requires
/// `graphVersion >= 1` and there is no version to bind.
pub fn override_with_waiver(
    projection: &ExecutionProjection,
    node_id: &str,
    obligation: &str,
    actor: &str,
    acknowledged_risks: &[&str],
    waiver_id: String,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<PolicyWaiver, GovernanceError> {
    let execution_id = projection
        .execution_id
        .as_deref()
        .ok_or(GovernanceError::NotStarted)?;
    if acknowledged_risks.is_empty() {
        return Err(GovernanceError::UnacknowledgedRisk);
    }
    let waiver = PolicyWaiver {
        id: waiver_id,
        requirement: obligation.to_owned(),
        execution_id: execution_id.to_owned(),
        graph_version: projection
            .current_graph
            .as_ref()
            .map_or(0, |graph| graph.number()),
        actor: actor.to_owned(),
        reason: Some(format!("owner override on node {node_id}")),
        acknowledged_risks: acknowledged_risks
            .iter()
            .map(|risk| (*risk).to_owned())
            .collect(),
        scope: WaiverScope::Node,
        created_at: now,
        expires_at: None,
    };
    let raw = serde_json::to_value(&waiver).map_err(|_| GovernanceError::InvalidWaiver)?;
    if !graphhelm_schema::validate_waiver(&raw, "override-waiver").is_empty() {
        return Err(GovernanceError::InvalidWaiver);
    }
    Ok(waiver)
}

#[cfg(test)]
mod tests {
    use super::*;
    use graphhelm_events::ExecutionProjection;
    use graphhelm_execution::{MAX_ACCEPTED_MUTATIONS, MAX_SIGNALS_PER_EXECUTION};
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

    fn signal(kind: &str) -> serde_json::Value {
        serde_json::json!({
            "id": "signal-1",
            "source": {"type": "node", "id": "node-a"},
            "type": kind,
            "severity": "high",
            "description": "a dependency was discovered",
            "evidence": ["exec-1"],
            "emittedAt": "2026-08-13T00:00:00Z"
        })
    }

    #[test]
    fn a_valid_signal_is_admitted_with_its_externalization_plan() {
        let admitted = admit_signal(
            &projection(ExecutionMode::Autopilot, 0, 0),
            &signal("no_progress"),
        )
        .unwrap();
        assert_eq!(admitted.record.kind, "no_progress");
        assert!(!admitted.externalize.is_empty());
    }

    /// A schema-valid signal whose id is not an OpaqueId is evidence that cannot be
    /// wire-recorded. That is a different operator response from a garbage envelope, and the
    /// error must say which one happened.
    #[test]
    fn an_unrecordable_identity_is_distinguished_from_a_garbage_envelope() {
        let mut sparse = signal("no_progress");
        sparse["id"] = serde_json::json!("");
        assert_eq!(
            admit_signal(&projection(ExecutionMode::Autopilot, 0, 0), &sparse).unwrap_err(),
            GovernanceError::UnrecordableIdentity
        );
        assert_eq!(
            admit_signal(
                &projection(ExecutionMode::Autopilot, 0, 0),
                &serde_json::json!({"not": "a signal"})
            )
            .unwrap_err(),
            GovernanceError::InvalidSignal
        );
    }

    /// Decision 5.7: the signal budget blocks, it never drops evidence.
    #[test]
    fn the_signal_budget_blocks_rather_than_dropping() {
        let full = projection(ExecutionMode::Autopilot, 0, MAX_SIGNALS_PER_EXECUTION);
        assert_eq!(
            admit_signal(&full, &signal("no_progress")).unwrap_err(),
            GovernanceError::SignalBudgetExhausted
        );
    }

    /// Decision 5.4: an unrecognized kind is recorded but can never propose a mutation.
    #[test]
    fn an_unrecognized_signal_is_recorded_but_never_actionable() {
        let projection = projection(ExecutionMode::Autopilot, 0, 0);
        let admitted = admit_signal(&projection, &signal("invented_by_an_agent")).unwrap();
        assert!(!admitted.may_propose_mutation);
        assert_eq!(
            decide_mutation(&projection, &admitted),
            MutationDecision::Rejected(RejectionReason::SignalNotActionable)
        );
    }

    /// Decision 5.5 and D-022: the mode in force at the decision governs it.
    #[test]
    fn the_mode_in_force_governs_the_decision() {
        let admitted = |mode| {
            let projection = projection(mode, 0, 0);
            let admitted = admit_signal(&projection, &signal("no_progress")).unwrap();
            decide_mutation(&projection, &admitted)
        };
        assert_eq!(admitted(ExecutionMode::Autopilot), MutationDecision::Accept);
        assert_eq!(
            admitted(ExecutionMode::Supervised),
            MutationDecision::RequiresApproval
        );
        assert_eq!(
            admitted(ExecutionMode::Manual),
            MutationDecision::Rejected(RejectionReason::ManualMode)
        );
    }

    /// Decision 5.1/5.7: the 65th acceptance blocks for an owner decision, in every mode.
    #[test]
    fn the_mutation_budget_blocks_in_every_mode() {
        for mode in [ExecutionMode::Autopilot, ExecutionMode::Supervised] {
            let projection = projection(mode, MAX_ACCEPTED_MUTATIONS, 0);
            let admitted = admit_signal(&projection, &signal("no_progress")).unwrap();
            assert_eq!(
                decide_mutation(&projection, &admitted),
                MutationDecision::Blocked
            );
        }
    }

    /// A real published graph version, so `graph_version` in the constructed waiver is the
    /// schema's required positive integer rather than the `0` fallback with no graph published
    /// yet. Deviation from the plan's sketch: the plan's own waiver test left `current_graph`
    /// unset, which drives `override_with_waiver`'s `graph_version` fallback to `0` — a value the
    /// waiver schema's `graphVersion` (`minimum: 1`) always rejects. Loaded from the same
    /// checked-in fixture `core/events/src/projection.rs`'s own tests use for a valid persisted
    /// graph version.
    fn fixture_graph() -> graphhelm_protocols::PersistedGraphVersion {
        serde_json::from_str(include_str!(
            "../../../conformance/schemas/valid/persisted-graph-version.json"
        ))
        .unwrap()
    }

    /// Decision 5.8: an override is the M03 waiver, node-scoped, naming its obligation.
    #[test]
    fn an_override_is_the_existing_waiver_bound_to_node_and_obligation() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-08-13T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let waiver = override_with_waiver(
            &ExecutionProjection {
                current_graph: Some(fixture_graph()),
                ..projection(ExecutionMode::Manual, 0, 0)
            },
            "node-a",
            "quality.gate.tests",
            // Deviation from the plan's sketch: the plan's fixture used "owner@example", which
            // the waiver schema's `actorId` pattern (`^[A-Za-z0-9][A-Za-z0-9._-]*$`, no `@`)
            // rejects, and which `graphhelm_protocols::ActorId` would reject identically
            // elsewhere in the codebase. Substituted a schema-valid actor id.
            "owner.example",
            &["tests were reviewed manually"],
            "waiver-1".to_owned(),
            now,
        )
        .unwrap();
        assert_eq!(waiver.scope, graphhelm_protocols::WaiverScope::Node);
        assert_eq!(waiver.requirement, "quality.gate.tests");
        assert_eq!(waiver.execution_id, "execution-1");
        assert!(!waiver.acknowledged_risks.is_empty());
    }

    /// The waiver schema requires a bound graph version, so an execution with no published graph
    /// cannot be overridden into one. Pinning it keeps the failure from surprising the driver.
    #[test]
    fn an_override_with_no_published_graph_is_an_invalid_waiver() {
        assert_eq!(
            override_with_waiver(
                &projection(ExecutionMode::Manual, 0, 0),
                "node-a",
                "quality.gate.tests",
                "owner.example",
                &["risk acknowledged"],
                "waiver-1".to_owned(),
                chrono::DateTime::parse_from_rfc3339("2026-08-13T00:00:00Z")
                    .unwrap()
                    .with_timezone(&chrono::Utc),
            )
            .unwrap_err(),
            GovernanceError::InvalidWaiver
        );
    }

    /// An override with no acknowledged risk is not an informed decision and must be refused.
    #[test]
    fn an_override_without_acknowledged_risks_is_refused() {
        assert_eq!(
            override_with_waiver(
                &projection(ExecutionMode::Manual, 0, 0),
                "node-a",
                "quality.gate.tests",
                "owner.example",
                &[],
                "waiver-1".to_owned(),
                chrono::DateTime::parse_from_rfc3339("2026-08-13T00:00:00Z")
                    .unwrap()
                    .with_timezone(&chrono::Utc),
            )
            .unwrap_err(),
            GovernanceError::UnacknowledgedRisk
        );
    }
}
