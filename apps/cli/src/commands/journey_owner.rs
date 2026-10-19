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

/// #1130: the signal kind of a blind reviewer agent's journey approval.
pub(crate) const AGENT_APPROVED_KIND: &str = "journey_flow_agent_approved";
/// The description protocol of that signal.
pub(crate) const AGENT_APPROVED_PROTOCOL: &str = "graphhelm-journey-agent-approval-v1";
/// The longest `checked` note an agent approval carries.
pub(crate) const CHECKED_MAX: usize = 4096;

/// `journey-agent-approved-<32 hex>`, like `signal_id`, for a reviewer agent's approval.
pub(crate) fn agent_signal_id(flow: &str, digest: &str) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(format!("agent\n{flow}\n{digest}").as_bytes());
    format!("journey-agent-approved-{}", &hex::encode(hash)[..32])
}

/// #1130: the signal kind that names a lane that wrote a draft, bound to that draft's digest.
pub(crate) const DRAFTED_KIND: &str = "journey_flow_drafted";
/// The description protocol of that signal.
pub(crate) const DRAFTED_PROTOCOL: &str = "graphhelm-journey-drafted-v1";

/// `journey-drafted-<16 hex flow>-<16 hex digest and writer>`: one record per writer of each
/// digest, under a prefix that names the flow, so every lane that ever wrote a flow is found.
fn drafted_prefix(flow: &str) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(format!("drafted\n{flow}").as_bytes());
    format!("journey-drafted-{}-", &hex::encode(hash)[..16])
}

pub(crate) fn drafted_signal_id(flow: &str, digest: &str, writer: &str) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(format!("{digest}\n{writer}").as_bytes());
    format!("{}{}", drafted_prefix(flow), &hex::encode(hash)[..16])
}

/// #1309 card 1c: the signal kind that records a passing preview of a flow, written by the preview
/// runner itself (the process that drove the browser) and by no route a caller can ask.
pub(crate) const PREVIEWED_KIND: &str = "journey_flow_previewed";
/// The description protocol of that signal.
pub(crate) const PREVIEWED_PROTOCOL: &str = "graphhelm-journey-preview-v2";
/// The actor the runner appends as.
pub(crate) const PREVIEW_RUNNER: &str = "journey-preview-runner";

/// `journey-previewed-<32 hex>`: sha256 over the flow id, the draft digest and the scope commit,
/// so the id names exactly the flow state and code the preview passed on. #1633: salted `v2`,
/// because v1 records were written for runs that skipped or missed steps; none of them counts.
pub(crate) fn previewed_signal_id(flow: &str, digest: &str, scope_commit: &str) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(format!("previewed-v2\n{flow}\n{digest}\n{scope_commit}").as_bytes());
    format!("journey-previewed-{}", &hex::encode(hash)[..32])
}

/// Every `drafted` record of `flow`: (signal id, writer).
fn drafted_records(project: &Path, flow: &str) -> Result<Vec<(String, String)>, String> {
    let prefix = drafted_prefix(flow);
    Ok(owner_history(project)?
        .iter()
        .filter_map(|event| match &event.kind {
            EventKind::SignalRecorded(signal)
                if event.actor.actor_type() == PersistedActorType::Agent
                    && signal.kind.as_str() == DRAFTED_KIND
                    && signal.signal_id.as_str().starts_with(&prefix) =>
            {
                Some((
                    signal.signal_id.as_str().to_owned(),
                    event.actor.id().as_str().to_owned(),
                ))
            }
            _ => None,
        })
        .collect())
}

/// The lane that wrote `flow` at exactly `digest` through the Runtime's draft route, if any.
pub(crate) fn drafted_by(
    project: &Path,
    flow: &str,
    digest: &str,
) -> Result<Option<String>, String> {
    Ok(drafted_records(project, flow)?
        .into_iter()
        .find(|(id, writer)| *id == drafted_signal_id(flow, digest, writer))
        .map(|(_, writer)| writer))
}

/// Whether `lane` ever wrote any version of `flow` through the draft route.
pub(crate) fn ever_wrote(project: &Path, flow: &str, lane: &str) -> Result<bool, String> {
    Ok(drafted_records(project, flow)?
        .iter()
        .any(|(_, writer)| writer.eq_ignore_ascii_case(lane)))
}

/// Records that `writer` (an authenticated agent) wrote `flow` at `digest`.
pub(crate) fn record_drafted(
    project: &Path,
    flow: &str,
    digest: &str,
    writer: &str,
) -> Result<(), String> {
    let id = drafted_signal_id(flow, digest, writer);
    if drafted_records(project, flow)?
        .iter()
        .any(|(known, _)| *known == id)
    {
        return Ok(());
    }
    let actor = graphhelm_protocols::PersistedActor::new(
        PersistedActorType::Agent,
        graphhelm_protocols::ActorId::parse(writer)
            .map_err(|_| "the writer is not a valid actor id".to_string())?,
    );
    let description = serde_json::json!({
        "protocol": DRAFTED_PROTOCOL, "flowId": flow, "digest": digest,
    });
    append_as(
        project,
        &id,
        DRAFTED_KIND,
        &description,
        flow,
        actor,
        writer,
    )
}

/// #1588: the signal kind of an owner's rejection of a flow, with the reason.
pub(crate) const REJECTED_KIND: &str = "journey_flow_rejected";
/// The description protocol of that signal.
pub(crate) const REJECTED_PROTOCOL: &str = "graphhelm-journey-rejection-v1";

/// `journey-rejected-<16 hex flow>-`: every rejection of a flow shares this prefix, so the latest
/// one is found without knowing its note.
fn rejected_prefix(flow: &str) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(format!("rejected\n{flow}").as_bytes());
    format!("journey-rejected-{}-", &hex::encode(hash)[..16])
}

/// The owner's latest rejection of `flow` at exactly `digest`: `{note, digest, sequence}`. Only a
/// record in the reserved execution, by the owner, whose evidence file still hashes to the
/// recorded envelope counts. An edit to the flow changes its digest, so it clears the rejection.
pub(crate) fn rejection(project: &Path, flow: &str, digest: &str) -> Option<serde_json::Value> {
    use sha2::{Digest, Sha256};
    let prefix = rejected_prefix(flow);
    let records = project.join(".graphhelm/owner-records");
    owner_history(project).ok()?.iter().rev().find_map(|event| {
        let EventKind::SignalRecorded(signal) = &event.kind else {
            return None;
        };
        if event.actor.actor_type() != PersistedActorType::Owner
            || signal.kind.as_str() != REJECTED_KIND
            || !signal.signal_id.as_str().starts_with(&prefix)
        {
            return None;
        }
        let bytes =
            std::fs::read(records.join(format!("{}.json", signal.signal_id.as_str()))).ok()?;
        if hex::encode(Sha256::digest(&bytes)) != signal.envelope_sha256.as_str() {
            return None;
        }
        let envelope: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
        let description: serde_json::Value =
            serde_json::from_str(envelope["description"].as_str()?).ok()?;
        (description["protocol"] == REJECTED_PROTOCOL
            && description["flowId"] == flow
            && description["digest"] == digest)
            .then(|| {
                serde_json::json!({
                    "note": description["note"], "digest": digest, "sequence": event.sequence,
                })
            })
    })
}

/// Appends the owner's rejection of `flow` at `digest` with `note` (#1588). Every rejection is a
/// new record: the earlier ones and any approval stay in the store.
pub(crate) fn record_rejection(
    project: &Path,
    flow: &str,
    digest: &str,
    note: &str,
) -> Result<(), String> {
    let note = note.trim();
    if note.is_empty() || note.len() > CHECKED_MAX {
        return Err(format!(
            "--note must say why the flow is rejected (1 to {CHECKED_MAX} bytes)"
        ));
    }
    let id = format!(
        "{}{}",
        rejected_prefix(flow),
        &uuid::Uuid::new_v4().simple().to_string()[..16]
    );
    let description = serde_json::json!({
        "protocol": REJECTED_PROTOCOL, "flowId": flow, "digest": digest, "note": note,
    });
    append(project, &id, REJECTED_KIND, &description, flow)
}

/// The signal kind of the owner's confirm that a draft's commands may run (#1662).
pub(crate) const CLI_CONFIRMED_KIND: &str = "journey_flow_cli_confirmed";
/// The description protocol of that signal.
pub(crate) const CLI_CONFIRMED_PROTOCOL: &str = "graphhelm-journey-cli-confirm-v1";

/// `journey-cli-confirmed-<first 32 hex of sha256(flow, digest, argv...)>`: one id per exact
/// flow content and argv list, so an edit to either needs a new confirm.
pub(crate) fn cli_confirmed_signal_id(flow: &str, digest: &str, argv: &[String]) -> String {
    use sha2::{Digest, Sha256};
    let mut text = format!(
        "cli-confirmed
{flow}
{digest}"
    );
    for word in argv {
        text.push('\n');
        text.push_str(word);
    }
    format!(
        "journey-cli-confirmed-{}",
        &hex::encode(Sha256::digest(text.as_bytes()))[..32]
    )
}

/// Whether the owner confirmed that `flow` at exactly `digest` may run exactly `argv` (#1662).
/// `Err` when the store cannot be read: the confirm cannot be verified.
pub(crate) fn cli_confirmed(
    project: &Path,
    flow: &str,
    digest: &str,
    argv: &[String],
) -> Result<bool, String> {
    has(
        project,
        &cli_confirmed_signal_id(flow, digest, argv),
        CLI_CONFIRMED_KIND,
    )
}

/// Appends the owner's confirm that `flow` at `digest` may run `argv` (#1662), so the history
/// shows who allowed which commands. The same confirm again adds nothing.
pub(crate) fn record_cli_confirmed(
    project: &Path,
    flow: &str,
    digest: &str,
    argv: &[String],
) -> Result<(), String> {
    if cli_confirmed(project, flow, digest, argv).unwrap_or(false) {
        return Ok(());
    }
    let description = serde_json::json!({
        "protocol": CLI_CONFIRMED_PROTOCOL, "flowId": flow, "digest": digest, "argv": argv,
    });
    append(
        project,
        &cli_confirmed_signal_id(flow, digest, argv),
        CLI_CONFIRMED_KIND,
        &description,
        flow,
    )
}

fn owner_history(project: &Path) -> Result<Vec<graphhelm_protocols::EventEnvelope>, String> {
    let events = store(project);
    if !events.exists() {
        return Err("the project's owner record store (.graphhelm/events) is missing".into());
    }
    let repository = super::event_store(&events)
        .map_err(|_| "the owner record store could not be opened".to_string())?;
    Ok(
        execution::resolve_stream(&repository, Some(OWNER_EXECUTION))
            .map(|(_, _, history)| history)
            .unwrap_or_default(),
    )
}

/// Who approved a flow (#1130): the owner, or a reviewer agent that is not the flow's author.
#[derive(Debug, Clone)]
pub(crate) enum Approver {
    Owner,
    Agent {
        reviewer: String,
        author: String,
        checked: String,
    },
}

impl Approver {
    /// The agent approver, refused when it names itself as the author, an id that is not an
    /// actor id, or no note of what it checked.
    pub(crate) fn agent(reviewer: &str, author: &str, checked: &str) -> Result<Self, String> {
        let (reviewer, author, checked) = (reviewer.trim(), author.trim(), checked.trim());
        if graphhelm_protocols::ActorId::parse(reviewer).is_err()
            || graphhelm_protocols::ActorId::parse(author).is_err()
        {
            return Err(
                "--reviewer and --author must be session ids (letters, digits, `-`, `_`, `.`)"
                    .into(),
            );
        }
        if reviewer.eq_ignore_ascii_case(author) {
            return Err(
                "the reviewer is the flow's author: a journey is approved by someone else".into(),
            );
        }
        if checked.is_empty() || checked.len() > CHECKED_MAX {
            return Err(format!(
                "--checked must say what was checked (1 to {CHECKED_MAX} bytes)"
            ));
        }
        Ok(Self::Agent {
            reviewer: reviewer.into(),
            author: author.into(),
            checked: checked.into(),
        })
    }
}

/// Whether the project's owner store holds the owner's approval of `flow` at exactly `digest`.
/// #1309 ruling A: final approval is the owner's alone; a reviewer agent's record is a review
/// (`reviewed`) and never signs an approval. `Err` when the store is absent or unreadable: the
/// approval cannot be verified.
pub(crate) fn approved(project: &Path, flow: &str, digest: &str) -> Result<bool, String> {
    has(project, &signal_id(flow, digest), APPROVED_KIND)
}

/// Whether a reviewer agent recorded its blind review of `flow` at exactly `digest` (#1130).
pub(crate) fn reviewed(project: &Path, flow: &str, digest: &str) -> Result<bool, String> {
    has_agent(project, &agent_signal_id(flow, digest))
}

fn has_agent(project: &Path, wanted: &str) -> Result<bool, String> {
    Ok(owner_history(project)?.iter().any(|event| {
        event.actor.actor_type() == PersistedActorType::Agent
            && matches!(&event.kind, EventKind::SignalRecorded(signal)
                if signal.signal_id.as_str() == wanted
                    && signal.kind.as_str() == AGENT_APPROVED_KIND)
    }))
}

/// Whether the preview runner recorded a passing preview of `flow` at exactly `digest` on
/// `scope_commit` (#1309 card 1c). `Err` as for `approved`.
pub(crate) fn previewed(
    project: &Path,
    flow: &str,
    digest: &str,
    scope_commit: &str,
) -> Result<bool, String> {
    let wanted = previewed_signal_id(flow, digest, scope_commit);
    Ok(owner_history(project)?.iter().any(|event| {
        event.actor.actor_type() == PersistedActorType::System
            && event.actor.id().as_str() == PREVIEW_RUNNER
            && matches!(&event.kind, EventKind::SignalRecorded(signal)
                if signal.signal_id.as_str() == wanted && signal.kind.as_str() == PREVIEWED_KIND)
    }))
}

/// What a passing preview record carries besides its key: the evidence the runner read and the
/// runner that read it, so a forged or odd runner is visible in the history.
pub(crate) struct PreviewEvidence<'a> {
    pub(crate) evidence_sha256: &'a str,
    pub(crate) runner_sha256: &'a str,
    pub(crate) version: &'a str,
}

/// Appends the runner's record of a passing preview of `flow` at `digest` on `scope_commit`. Only
/// the preview runner calls this, after it played the flow; the same record again adds nothing.
pub(crate) fn record_previewed(
    project: &Path,
    flow: &str,
    digest: &str,
    scope_commit: &str,
    evidence: &PreviewEvidence<'_>,
) -> Result<(), String> {
    if store(project).exists() && previewed(project, flow, digest, scope_commit)? {
        return Ok(());
    }
    let description = serde_json::json!({
        "protocol": PREVIEWED_PROTOCOL, "flowId": flow, "digest": digest,
        "scopeCommit": scope_commit, "result": "pass",
        "evidenceSha256": evidence.evidence_sha256, "runnerSha256": evidence.runner_sha256,
        "version": evidence.version,
    });
    let actor = graphhelm_protocols::PersistedActor::new(
        PersistedActorType::System,
        graphhelm_protocols::ActorId::parse(PREVIEW_RUNNER).expect("constant actor id is valid"),
    );
    append_as(
        project,
        &previewed_signal_id(flow, digest, scope_commit),
        PREVIEWED_KIND,
        &description,
        flow,
        actor,
        PREVIEW_RUNNER,
    )
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
    record_as(project, flow, digest, revision, &Approver::Owner)
}

/// Appends `approver`'s approval of `flow` at `digest`. An agent's record names the reviewer, the
/// flow's author and what the reviewer checked, and is appended as that reviewer (#1130).
pub(crate) fn record_as(
    project: &Path,
    flow: &str,
    digest: &str,
    revision: &str,
    approver: &Approver,
) -> Result<(), String> {
    if let Approver::Agent {
        reviewer,
        author,
        checked,
    } = approver
    {
        let description = serde_json::json!({
            "protocol": AGENT_APPROVED_PROTOCOL, "flowId": flow, "digest": digest,
            "revision": revision, "reviewer": reviewer, "author": author, "checked": checked,
        });
        if store(project).exists() && reviewed(project, flow, digest)? {
            return Ok(());
        }
        let actor = graphhelm_protocols::PersistedActor::new(
            PersistedActorType::Agent,
            graphhelm_protocols::ActorId::parse(reviewer.as_str())
                .map_err(|_| "the reviewer is not a valid actor id".to_string())?,
        );
        return append_as(
            project,
            &agent_signal_id(flow, digest),
            AGENT_APPROVED_KIND,
            &description,
            flow,
            actor,
            reviewer,
        );
    }
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
    append_as(
        project,
        id,
        kind,
        description,
        flow,
        execution::owner_actor(),
        "owner",
    )
}

fn append_as(
    project: &Path,
    id: &str,
    kind: &str,
    description: &serde_json::Value,
    flow: &str,
    actor: graphhelm_protocols::PersistedActor,
    source: &str,
) -> Result<(), String> {
    let source_type = if source == "owner" { "user" } else { "node" };
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
        "source": {"type": source_type, "id": source},
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
    // #1130: the reviewer and draft records are reserved for these internal doors, whose actor
    // the Runtime authenticated; the generic signal route refuses both kinds.
    execution::signal::execute_reserved(
        &events,
        OWNER_EXECUTION,
        envelope.to_string().as_bytes(),
        Some(&records.join(format!("{id}.json"))),
        actor,
        key,
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
