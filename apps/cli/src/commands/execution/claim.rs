//! `execution claim` (#159): testimony that a `waiting_input` node's external work is done.
//!
//! This file is the operator's door to `graphhelm_events::claim` and carries no decision of its
//! own — the decision table (not_waiting, stale_rendezvous, unknown_wait, duplicate_completion,
//! evidence_budget_unmet) lives in `core/events` beside the fold that judges the result. What
//! the door adds is what the verb cannot supply for itself: the graph the execution started
//! from, checked through the same file-trust seam `resume` uses (D2), and the node's declared
//! `proofKinds` read from that graph. A refusal is a JOURNAL EVENT and an `ok: true` reply with
//! `claim.outcome == "refused"`; only a malformed argument or a wrong graph is a failure.

use std::path::Path;

use graphhelm_events::{ClaimError, ClaimOutcome, ClaimRequest};
use graphhelm_graph::GraphVersion;
use graphhelm_protocols::{
    ClaimAttestation, ClaimAttestationMode, ClaimEvidence, OpaqueId, PersistedActor,
};

use super::{
    Failure, argument, execution_state, finish, idempotency_key, load_claim_evidence, owner_actor,
    render, replay_failure, replay_projection, repository_failure, resolve_stream,
    verify_graph_matches_execution,
};
use crate::commands::{event_store, owner, publish_loaded};
use crate::output::Outcome;

const COMMAND: &str = "execution.claim";

/// The CLI's arguments, as one value: eight loose parameters would have earned the
/// argument-count lint the same way `start`'s `Attribution` did, and they travel together.
pub(crate) struct Arguments<'a> {
    pub file: &'a Path,
    pub events: &'a Path,
    pub execution: Option<&'a str>,
    pub node: &'a str,
    pub wait_seq: Option<u64>,
    pub evidence: &'a Path,
    pub asserter: Option<&'a str>,
    pub mode: &'a str,
}

/// Loads, lints and publishes the graph exactly as `resume` does, then runs `execute` with the
/// owner actor and a fresh per-invocation idempotency key.
pub fn run(arguments: &Arguments<'_>) -> Outcome {
    let loaded = match graphhelm_schema::load_graph(arguments.file) {
        Ok(loaded) => loaded,
        Err(diagnostics) => return Outcome::domain(COMMAND, diagnostics),
    };
    let report = graphhelm_graph::lint(&loaded.graph, &loaded.source);
    if !report.errors.is_empty() {
        let mut diagnostics = report.errors;
        diagnostics.extend(report.warnings);
        return Outcome::domain(COMMAND, diagnostics);
    }
    // #192: a warning-only lint pass reaches every exit past this point, not only the success
    // one.
    let warnings = report.warnings;
    let version = match publish_loaded(&loaded, owner("owner-local")) {
        Ok(version) => version,
        Err(error) => return Outcome::internal(COMMAND, error).with_warnings(warnings),
    };
    let actor = owner_actor();
    let result = load_claim_evidence(arguments.evidence).and_then(|evidence| {
        let attestation = attestation(arguments.asserter, arguments.mode, &actor)?;
        execute(
            &version,
            arguments.events,
            arguments.execution,
            arguments.node,
            arguments.wait_seq,
            evidence,
            attestation,
            actor,
            idempotency_key("completion-claim"),
        )
    });
    finish(COMMAND, result, |value| value).with_warnings(warnings)
}

/// Who asserts the completion and on what authority. An absent asserter is the actor this
/// command runs as — on the CLI the owner, over HTTP the `X-GraphHelm-Actor` header.
pub(crate) fn attestation(
    asserter: Option<&str>,
    mode: &str,
    actor: &PersistedActor,
) -> Result<ClaimAttestation, Failure> {
    let mode = match mode {
        "operator_attested" => ClaimAttestationMode::OperatorAttested,
        "machine_verified" => ClaimAttestationMode::MachineVerified,
        _ => {
            return Err(argument(
                "--mode must be operator_attested or machine_verified",
                "/mode",
            ));
        }
    };
    let asserter = OpaqueId::parse(asserter.map_or_else(|| actor.id().to_string(), str::to_owned))
        .map_err(|_| argument("--asserter is not a valid identifier", "/asserter"))?;
    Ok(ClaimAttestation { asserter, mode })
}

/// The verb behind both doors (CLI here, HTTP in `serve::routes`): the seam, the node's declared
/// budget, the journaled decision, and the shared `render()` reply plus a `claim` key.
#[allow(clippy::too_many_arguments)]
pub(crate) fn execute(
    version: &GraphVersion,
    events: &Path,
    execution: Option<&str>,
    node: &str,
    wait_seq: Option<u64>,
    evidence: Vec<ClaimEvidence>,
    attestation: ClaimAttestation,
    actor: PersistedActor,
    key: OpaqueId,
) -> Result<serde_json::Value, Failure> {
    let store = event_store(events).map_err(|error| repository_failure(&error))?;
    let (scope, stream, history) = resolve_stream(&store, execution)?;
    let initial = graphhelm_events::replay(&scope, &stream, &history)
        .map_err(|error| replay_failure(&error))?;
    verify_graph_matches_execution(version, &initial, &history, "claim")?;

    // The declared budget comes from the GRAPH, not the projection: on the start path nothing
    // publishes a `PersistedGraphVersion`, so the fold holds no node spec to read it from (D2).
    let required: Vec<String> = match version.graph().spec.nodes.get(node) {
        None => {
            return Err(execution_state(
                "the graph has no node with that id",
                "/node",
            ));
        }
        Some(graph_node) => graph_node
            .customs()
            .map_err(|_| {
                execution_state("the node's completion.customs block is malformed", "/node")
            })?
            .map(|customs| customs.proof_kinds)
            .unwrap_or_default(),
    };

    let (outcome, _appended) = graphhelm_events::claim(
        &store,
        &scope,
        &stream,
        &actor,
        &key,
        ClaimRequest {
            node,
            completes_wait_seq: wait_seq,
            evidence,
            attestation,
            required_proof_kinds: &required,
        },
    )
    .map_err(|error| match error {
        ClaimError::NotStarted => {
            execution_state("no execution has started on this stream", "/execution")
        }
        ClaimError::Repository(error) => repository_failure(&error),
    })?;

    let projection = replay_projection(&store, &scope, &stream)?;
    let mut value = render(
        &projection,
        // THE SEVENTH SURFACE, and it arrived on main at a31265d9 (#1036) AFTER this branch had
        // converted the six it knew about -- caught by this PR's own census on a tree nobody aimed
        // it at, which is the drift the census exists for.
        //
        // The comment that stood here said "nothing measured here on purpose", and the liveness
        // half of that is still true: this command reports the mutation it just made, not a
        // liveness reading. But `default()` does not say "unmeasured" -- it asserts there is NO
        // DECLARED BUDGET, and the budget is not a measurement. It is declared in the graph this
        // projection already holds. So an operator who HAD declared one was answered
        // `NoDeclaredBudget` with the remedy "declare a budget for this node".
        //
        // `for_surface` derives the budget from the projection and leaves the AGE empty, which is
        // the part that really is unmeasured here, landing on the honest `(Some, None)` arm:
        // `NotMeasured`, remedy `Unavailable { SurfaceMeasuredNoAge }`.
        &graphhelm_execution::AttentionInputs::for_surface(
            &projection,
            std::collections::BTreeMap::new(),
            None,
        ),
        &super::Liveness::from_store(&store, &scope, &stream),
        // #134: this door holds no graph, so no dispatch gate is published from it.
        None,
    );
    value["claim"] = match outcome {
        ClaimOutcome::Claimed {
            claim_seq,
            wait_seq,
        } => serde_json::json!({
            "outcome": "claimed",
            "claimSeq": claim_seq,
            "reasonCode": null,
            "waitSeq": wait_seq,
        }),
        // `waitSeq` is what the verb journaled as `claimedWaitSeq`: the wait the refused claim
        // named or resolved to, or — when it named none and the node had none — the refusal's
        // own sequence (see `graphhelm_events::claim` for why that is the honest value).
        ClaimOutcome::Refused {
            reason_code,
            wait_seq,
        } => serde_json::json!({
            "outcome": "refused",
            "claimSeq": null,
            "reasonCode": reason_code,
            "waitSeq": wait_seq,
        }),
    };
    Ok(value)
}
