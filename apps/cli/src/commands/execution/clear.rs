//! `execution clear` (#159): countersign an open claim by machine replay.
//!
//! The VERDICT is the fold's: `graphhelm_events::clear` appends the clearance and reads the
//! outcome back from the replayed journal, so this door never compares a digest itself. What the
//! door adds is the DRIVE (D3): after `completion_cleared` folds to `Cleared` the node is
//! `Succeeded` and its dependents are dispatchable, but nothing in the tree dispatches them —
//! `resume_preconditions` refuses a non-paused execution and `start` refuses a started stream.
//! So a clearance that clears runs the same sync drive `resume` runs, with an EMPTY release set;
//! a rejected clearance drives nothing.
//!
//! `countersign` is refused AT THE DOOR (D1) — before the store is opened, appending nothing —
//! because the wire carries no signature to verify until D-047 / #529, and the `#527` trap goes
//! red the moment a production surface appends one.

use std::collections::BTreeSet;
use std::path::Path;

use graphhelm_events::{ClearError, ClearanceOutcome};
use graphhelm_graph::GraphVersion;
use graphhelm_protocols::{ClaimEvidence, OpaqueId, PersistedActor, WireHash};
use graphhelm_simulation::FixtureExecutor;

use super::driver::{Release, drive_to_quiescence};
use super::{
    Failure, PreparedDrive, argument, execution_state, finish, idempotency_key,
    load_claim_evidence, load_fixtures, owner_actor, render, replay_failure, replay_projection,
    repository_failure, resolve_stream, system_actor, verify_graph_matches_execution,
};
use crate::commands::{event_store, owner, publish_loaded};
use crate::output::Outcome;

const COMMAND: &str = "execution.clear";

/// The CLI's arguments, as one value (see `claim::Arguments`).
pub(crate) struct Arguments<'a> {
    pub file: &'a Path,
    pub events: &'a Path,
    pub fixtures: Option<&'a Path>,
    pub execution: Option<&'a str>,
    pub claim_seq: u64,
    pub manifest_hash: Option<&'a str>,
    pub evidence: Option<&'a Path>,
    pub verifier: &'a str,
}

/// The clearance this surface can journal. There is exactly one variant on purpose: a
/// `Countersign` arm would be a place for the door refusal to be bypassed, and the type is built
/// only AFTER that refusal has run.
pub(crate) enum Verifier {
    MachineReplay(WireHash),
}

/// Decide the verifier from what the operator presented, refusing `countersign` before any store
/// access. `--evidence` means "I hold this bundle; compute its digest here", which is what a
/// machine replay IS; `--manifest-hash` presents a digest computed elsewhere.
pub(crate) fn verifier(
    kind: &str,
    manifest_hash: Option<&str>,
    evidence: Option<&[ClaimEvidence]>,
) -> Result<Verifier, Failure> {
    match kind {
        "machine_replay" => {}
        "countersign" => {
            return Err(execution_state(
                "countersign clearance is not available: the wire carries no signature to verify (#529, D-047); use --verifier machine_replay",
                "/verifier",
            ));
        }
        _ => return Err(argument("--verifier must be machine_replay", "/verifier")),
    }
    match (manifest_hash, evidence) {
        (Some(hash), None) => WireHash::parse(hash)
            .map(Verifier::MachineReplay)
            .map_err(|_| argument("--manifest-hash must be sha256:<64 hex>", "/manifestHash")),
        (None, Some(bundle)) => Ok(Verifier::MachineReplay(
            graphhelm_events::claim_evidence_digest(bundle),
        )),
        _ => Err(argument(
            "give exactly one of --manifest-hash and --evidence",
            "/manifestHash",
        )),
    }
}

/// Loads, lints and publishes the graph exactly as `resume` does, builds the verifier — the
/// countersign refusal runs HERE, before `execute` opens the store — and runs the verb with the
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
    let warnings = report.warnings;
    let version = match publish_loaded(&loaded, owner("owner-local")) {
        Ok(version) => version,
        Err(error) => return Outcome::internal(COMMAND, error).with_warnings(warnings),
    };
    let result = arguments
        .evidence
        .map(load_claim_evidence)
        .transpose()
        .and_then(|evidence| {
            let verifier = verifier(
                arguments.verifier,
                arguments.manifest_hash,
                evidence.as_deref(),
            )?;
            execute(
                &version,
                arguments.events,
                arguments.fixtures,
                arguments.execution,
                arguments.claim_seq,
                &verifier,
                owner_actor(),
                idempotency_key("completion-clear"),
            )
        });
    finish(COMMAND, result, |value| value).with_warnings(warnings)
}

/// The decision half: the seam, the journaled clearance, the fold's verdict, and the
/// `PreparedDrive` the drive half needs — the same split `resume` and `start` use so the HTTP
/// route can drive asynchronously.
#[allow(clippy::too_many_arguments)]
pub(crate) fn decide(
    version: &GraphVersion,
    events: &Path,
    fixtures: Option<&Path>,
    execution: Option<&str>,
    claim_seq: u64,
    verifier: &Verifier,
    actor: PersistedActor,
    key: OpaqueId,
) -> Result<(ClearanceOutcome, PreparedDrive), Failure> {
    let store = event_store(events).map_err(|error| repository_failure(&error))?;
    let fixtures = load_fixtures(fixtures)?;
    let (scope, stream, history) = resolve_stream(&store, execution)?;
    let initial = graphhelm_events::replay(&scope, &stream, &history)
        .map_err(|error| replay_failure(&error))?;
    verify_graph_matches_execution(version, &initial, &history, "clear")?;

    let Verifier::MachineReplay(manifest_hash) = verifier;
    let (outcome, _appended) = graphhelm_events::clear(
        &store,
        &scope,
        &stream,
        &actor,
        &key,
        claim_seq,
        manifest_hash,
    )
    .map_err(|error| match error {
        ClearError::NotStarted => {
            execution_state("no execution has started on this stream", "/execution")
        }
        // Refused WITHOUT appending (D5): the fold reads a clearance with no claim under it as
        // Corrupt, and a verb that journaled one would poison every later replay.
        ClearError::NotAnOpenClaim { claim_seq } => execution_state(
            &format!("sequence {claim_seq} is not an open claim on this execution"),
            "/claimSeq",
        ),
        ClearError::Repository(error) => repository_failure(&error),
    })?;

    let stream_id = OpaqueId::parse(&stream)
        .map_err(|_| execution_state("the stream identifier is not wire-safe", "/execution"))?;
    let execution_id = OpaqueId::parse(initial.execution_id.as_deref().unwrap_or_default())
        .map_err(|_| execution_state("the execution identifier is not wire-safe", "/execution"))?;
    Ok((
        outcome,
        PreparedDrive {
            scope,
            stream: stream_id,
            execution_id,
            spec: version.graph().spec.clone(),
            fixtures,
            // EMPTY on purpose: the release is the FOLD's (`Cleared` makes the node `Succeeded`),
            // and the drive only lets the now-satisfied dependents run. Nothing was paused.
            release: BTreeSet::new(),
        },
    ))
}

/// The `clearance` key every door adds to the shared `render()` reply.
pub(crate) fn annotate(value: &mut serde_json::Value, outcome: &ClearanceOutcome, claim_seq: u64) {
    value["clearance"] = match outcome {
        ClearanceOutcome::Cleared => serde_json::json!({
            "outcome": "cleared",
            "claimSeq": claim_seq,
            "reasonCode": null,
        }),
        ClearanceOutcome::Refused { reason_code } => serde_json::json!({
            "outcome": "rejected",
            "claimSeq": claim_seq,
            "reasonCode": reason_code.as_str(),
        }),
    };
}

/// `decide` plus the sync drive when the clearance cleared, plus the shared reply.
#[allow(clippy::too_many_arguments)]
pub(crate) fn execute(
    version: &GraphVersion,
    events: &Path,
    fixtures: Option<&Path>,
    execution: Option<&str>,
    claim_seq: u64,
    verifier: &Verifier,
    actor: PersistedActor,
    key: OpaqueId,
) -> Result<serde_json::Value, Failure> {
    let (outcome, prepared) = decide(
        version, events, fixtures, execution, claim_seq, verifier, actor, key,
    )?;
    let store = event_store(events).map_err(|error| repository_failure(&error))?;
    let projection = if outcome == ClearanceOutcome::Cleared {
        // THE RELEASE IS THE FOLD'S; the drive only lets the now-satisfied dependents run, exactly
        // as `resume` does — under the system actor, so the log tells sovereignty from machinery.
        drive_to_quiescence(
            &store,
            &prepared.scope,
            prepared.stream.as_str(),
            &prepared.spec,
            &FixtureExecutor::new(prepared.fixtures.clone()),
            &system_actor(),
            &Release {
                nodes: &prepared.release,
                actor: &owner_actor(),
            },
        )?
    } else {
        replay_projection(&store, &prepared.scope, prepared.stream.as_str())?
    };
    let mut value = render(
        &projection,
        // THE EIGHTH SURFACE, arriving with `claim` at a31265d9 (#1036) after this branch converted
        // the six it knew about. Same correction, same reason: `default()` asserts there is no
        // declared budget, and the budget is declared in the graph this projection already holds --
        // so it answered `NoDeclaredBudget` to an operator who had declared one. `for_surface`
        // derives it and leaves the age empty, which is the half that genuinely is unmeasured here.
        &graphhelm_execution::AttentionInputs::for_surface(
            &projection,
            std::collections::BTreeMap::new(),
            None,
        ),
        &super::Liveness::from_store(&store, &prepared.scope, prepared.stream.as_str()),
        // #134: this door holds no graph, so no dispatch gate is published from it.
        None,
    );
    annotate(&mut value, &outcome, claim_seq);
    Ok(value)
}
