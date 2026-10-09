//! #534: owner-signed journey approvals. A flow's `approved: {revision, digest}` lives in its YAML,
//! and the digest is a plain sha256 over the canonical flow, so the YAML alone cannot say that the
//! OWNER approved it. Approving now also appends an owner-only `journey_flow_approved` record to
//! a reserved execution of the project's events store; replay and validate trust a YAML approval
//! only when that record exists for the exact digest.
//!
//! The record's signal id names the flow and the digest (`journey-approved-<flow>-<hex>`), and
//! signal ids are stored unsealed in the event, so CHECKING a record needs no key: verification
//! never hands the verifier the means to forge. Admission refuses the kind from anyone but the
//! owner (`execution::documents::validate_owner_signal`).
//!
//! Limit, said out loud: on one machine, as one OS user, a process that can read the owner's
//! token or write the events store directly can still record as the owner. This stops an agent
//! that holds only the agent session token and the YAML.
use std::path::{Path, PathBuf};

use graphhelm_protocols::{EventKind, OpaqueId, PersistedActorType};

use super::execution;

/// The reserved execution that holds the project's owner records.
pub(crate) const OWNER_EXECUTION: &str = "graphhelm-owner";
/// The signal kind of an owner's journey approval.
pub(crate) const APPROVED_KIND: &str = "journey_flow_approved";
/// The description protocol of that signal.
pub(crate) const APPROVED_PROTOCOL: &str = "graphhelm-journey-approval-v1";

const OWNER_GRAPH: &str = include_str!("../../../../conformance/schemas/valid/graph.json");

/// The project's owner record store: the standard `graphhelm init` layout.
pub(crate) fn store(project: &Path) -> PathBuf {
    project.join(".graphhelm/events")
}

/// `journey-approved-<32 hex>`: the first 128 bits of sha256 over the flow id and the digest. A
/// flow id may be 128 characters, so it cannot be spelled into an id (`OpaqueId` holds 128 bytes);
/// the description carries both in full and admission recomputes this id from them, so the id still
/// names exactly the flow and digest it vouches for.
pub(crate) fn signal_id(flow: &str, digest: &str) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(format!("{flow}\n{digest}").as_bytes());
    format!("journey-approved-{}", &hex::encode(hash)[..32])
}

/// #534 slice 2: the signal kind of an owner's safe mark on a draft's edge (#518).
pub(crate) const MARK_KIND: &str = "journey_edge_marked_safe";
/// The description protocol of that signal.
pub(crate) const MARK_PROTOCOL: &str = "graphhelm-journey-safe-mark-v1";

/// `journey-safe-<32 hex>`: sha256 over the flow id, the edge id and the mark's digest, like
/// `signal_id`, so the id names exactly the edge state the owner marked.
pub(crate) fn mark_signal_id(flow: &str, edge: &str, digest: &str) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(format!("{flow}\n{edge}\n{digest}").as_bytes());
    format!("journey-safe-{}", &hex::encode(hash)[..32])
}

/// Whether the project's owner store holds an owner's approval of `flow` at exactly `digest`.
/// `Err` when the store is absent or unreadable: the approval cannot be verified.
pub(crate) fn approved(project: &Path, flow: &str, digest: &str) -> Result<bool, String> {
    has(project, &signal_id(flow, digest), APPROVED_KIND)
}

/// Whether the owner marked `edge` of `flow` safe at exactly `digest`. `Err` as for `approved`.
pub(crate) fn marked(project: &Path, flow: &str, edge: &str, digest: &str) -> Result<bool, String> {
    has(project, &mark_signal_id(flow, edge, digest), MARK_KIND)
}

fn has(project: &Path, wanted: &str, kind: &str) -> Result<bool, String> {
    let events = store(project);
    if !events.exists() {
        return Err("the project's owner record store (.graphhelm/events) is missing".into());
    }
    let repository = super::event_store(&events)
        .map_err(|_| "the owner record store could not be opened".to_string())?;
    let history = match execution::resolve_stream(&repository, Some(OWNER_EXECUTION)) {
        Ok((_, _, history)) => history,
        // No reserved execution yet: nothing was ever recorded through an owner door.
        Err(_) => return Ok(false),
    };
    Ok(history.iter().any(|event| {
        event.actor.actor_type() == PersistedActorType::Owner
            && matches!(&event.kind, EventKind::SignalRecorded(signal)
                if signal.signal_id.as_str() == wanted && signal.kind.as_str() == kind)
    }))
}

/// Appends the owner's approval of `flow` at `digest` (approved at git `revision`) to the
/// project's owner store, starting the reserved execution on first use. The same approval again
/// adds nothing.
pub(crate) fn record(
    project: &Path,
    flow: &str,
    digest: &str,
    revision: &str,
) -> Result<(), String> {
    if store(project).exists() && approved(project, flow, digest)? {
        return Ok(());
    }
    let description = serde_json::json!({
        "protocol": APPROVED_PROTOCOL, "flowId": flow, "digest": digest, "revision": revision,
    });
    append(
        project,
        &signal_id(flow, digest),
        APPROVED_KIND,
        &description,
        flow,
    )
}

/// Appends the owner's safe mark of `edge` of `flow` at `digest` (#534 slice 2). The same mark
/// again adds nothing.
pub(crate) fn record_mark(
    project: &Path,
    flow: &str,
    edge: &str,
    digest: &str,
) -> Result<(), String> {
    if store(project).exists() && marked(project, flow, edge, digest)? {
        return Ok(());
    }
    let description = serde_json::json!({
        "protocol": MARK_PROTOCOL, "flowId": flow, "edgeId": edge, "digest": digest,
    });
    append(
        project,
        &mark_signal_id(flow, edge, digest),
        MARK_KIND,
        &description,
        flow,
    )
}

fn append(
    project: &Path,
    id: &str,
    kind: &str,
    description: &serde_json::Value,
    flow: &str,
) -> Result<(), String> {
    let events = store(project);
    if !events.exists() {
        return Err("the project's owner record store (.graphhelm/events) is missing; run `graphhelm init` first".into());
    }
    let records = project.join(".graphhelm/owner-records");
    std::fs::create_dir_all(&records)
        .map_err(|_| "the owner record directory could not be created".to_string())?;
    ensure_owner_execution(&events, &records)?;
    let envelope = serde_json::json!({
        "id": id, "type": kind, "severity": "low",
        "source": {"type": "user", "id": "owner"},
        "description": description.to_string(),
        "evidence": [format!("journey-flow:{flow}")],
        "emittedAt": chrono_now(),
    });
    // An unpredictable idempotency key (#569 review): an agent can choose any route's
    // `Idempotency-Key`, so a key it could compute (the signal id) could be squatted to block the
    // owner. A retry stays safe because the record is looked for first.
    let key = OpaqueId::parse(format!(
        "{id}-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    ))
    .map_err(|_| "the record id is not a valid identifier".to_string())?;
    execution::signal::execute(
        &events,
        Some(OWNER_EXECUTION),
        envelope.to_string().as_bytes(),
        Some(&records.join(format!("{id}.json"))),
        execution::owner_actor(),
        key,
        None,
        &[],
    )
    .map(|_| ())
    .map_err(|failure| format!("the owner record was not appended: {}", failure.message))
}

fn ensure_owner_execution(events: &Path, records: &Path) -> Result<(), String> {
    let repository = super::event_store(events)
        .map_err(|_| "the owner record store could not be opened".to_string())?;
    if execution::resolve_stream(&repository, Some(OWNER_EXECUTION))
        .is_ok_and(|(_, _, history)| !history.is_empty())
    {
        return Ok(());
    }
    let mut graph: serde_json::Value =
        serde_json::from_str(OWNER_GRAPH).map_err(|_| "the owner graph is invalid".to_string())?;
    graph["metadata"]["executionId"] = OWNER_EXECUTION.into();
    graph["metadata"]["id"] = "graphhelm-owner-records".into();
    graph["metadata"]["name"] = "Owner records".into();
    let file = records.join("owner-graph.json");
    std::fs::write(&file, graph.to_string())
        .map_err(|_| "the owner graph could not be written".to_string())?;
    let outcome = execution::start::run(
        &file,
        events,
        None,
        "supervised",
        Some(OWNER_EXECUTION),
        false,
        execution::start::GenesisKeyring {
            directory: None,
            key_id: None,
        },
    );
    if outcome.exit_code == 0 {
        Ok(())
    } else {
        Err("the owner record execution could not be started".into())
    }
}

fn chrono_now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}
