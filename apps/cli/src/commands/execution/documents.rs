//! Registered project documents. Free-form edit reasons stay in sealed signal evidence.
use std::collections::BTreeSet;
use std::io::Read;
use std::path::Path;

use graphhelm_events::{EvidenceOpener, EvidenceRead};
use graphhelm_protocols::{EventKind, EvidenceId, OpaqueId, PersistedActor};
use graphhelm_tool_host::documents::{DocumentError, ProjectDocuments};
use serde::{Deserialize, Serialize};

use super::{Failure, argument, delivery, execution_state, repository_failure, signal};
use crate::commands::event_store;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DocumentReference {
    pub(crate) evidence_id: String,
    pub(crate) index: usize,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SaveDocument {
    pub(crate) document: DocumentReference,
    pub(crate) content: String,
    pub(crate) expected_sha256: String,
    pub(crate) reason: String,
    pub(crate) idempotency_key: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OwnerDocumentEdit {
    actor: PersistedActor,
    project_id: String,
    path: String,
    before_sha256: String,
    after_sha256: String,
    reason: String,
    document: DocumentReference,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OwnerDocumentIntent {
    edit: OwnerDocumentEdit,
    runs: Vec<String>,
}

pub(crate) fn run_read(
    events: &Path,
    execution: &str,
    project: &Path,
    evidence: &str,
    index: usize,
    keyring: &Path,
    key_id: &str,
) -> crate::output::Outcome {
    super::finish(
        "execution.document_read",
        read(
            events,
            execution,
            project,
            &DocumentReference {
                evidence_id: evidence.to_owned(),
                index,
            },
            &signal::SignalKeyring {
                directory: keyring.to_owned(),
                key_id: key_id.to_owned(),
            },
        ),
        |v| v,
    )
}

pub(crate) fn run_save(
    events: &Path,
    execution: &str,
    project: &Path,
    edit: &Path,
    keyring: &Path,
    key_id: &str,
) -> crate::output::Outcome {
    let result = (|| {
        let mut bytes = Vec::new();
        std::fs::File::open(edit)
            .and_then(|file| file.take(800_001).read_to_end(&mut bytes))
            .map_err(|_| argument("the edit request could not be read", "/edit"))?;
        if bytes.len() > 800_000 {
            return Err(argument("the edit request is too large", "/edit"));
        }
        let request = serde_json::from_slice(&bytes)
            .map_err(|_| argument("invalid edit request", "/edit"))?;
        save(
            events,
            execution,
            project,
            &request,
            super::owner_actor(),
            &signal::SignalKeyring {
                directory: keyring.to_owned(),
                key_id: key_id.to_owned(),
            },
        )
    })();
    super::finish("execution.document_save", result, |v| v)
}

fn document_failure(error: DocumentError) -> Failure {
    execution_state(
        &error.to_string(),
        if error == DocumentError::Conflict {
            "/expectedSha256"
        } else {
            "/document"
        },
    )
}

/// Reserved notice contracts cannot accept arbitrary prose through the generic signal command.
pub(crate) fn validate_owner_signal(
    value: &serde_json::Value,
    actor: &PersistedActor,
    sealed: bool,
) -> Result<(), Failure> {
    let kind = value["type"].as_str().unwrap_or("");
    if !matches!(
        kind,
        "owner_document_changed" | "owner_document_edit_intent" | "owner_document_edit_saved"
    ) {
        return Ok(());
    }
    let invalid = || {
        super::signal_invalid(
            "owner document notices require a sealed, bounded owner edit record",
            "/signal",
        )
    };
    if !sealed
        || actor.actor_type() != graphhelm_protocols::PersistedActorType::Owner
        || value
            .pointer("/source/type")
            .and_then(serde_json::Value::as_str)
            != Some("user")
    {
        return Err(invalid());
    }
    if kind == "owner_document_changed" && owner_notice_order(value).is_none() {
        return Err(invalid());
    }
    let text = value["description"].as_str().ok_or_else(invalid)?;
    if text.len()
        > if kind == "owner_document_edit_intent" {
            65_536
        } else {
            8192
        }
    {
        return Err(invalid());
    }
    let edit = if kind == "owner_document_edit_intent" {
        let description: OwnerDocumentIntent = serde_json::from_str(text).map_err(|_| invalid())?;
        let runs = &description.runs;
        if runs.len() > 256
            || runs.iter().any(|run| {
                OpaqueId::parse(run).is_err() || graphhelm_runtime::context::secret_shaped(run)
            })
        {
            return Err(invalid());
        }
        description.edit
    } else {
        serde_json::from_str::<OwnerDocumentEdit>(text).map_err(|_| invalid())?
    };
    for hash in [&edit.project_id, &edit.before_sha256, &edit.after_sha256] {
        if hash.len() != 64
            || !hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(invalid());
        }
    }
    if edit.actor != *actor
        || graphhelm_runtime::context::secret_shaped(edit.actor.id().as_str())
        || !delivery::validate_path(&edit.path)
        || graphhelm_runtime::context::secret_shaped(&edit.path)
        || edit.reason.trim().is_empty()
        || edit.reason.len() > 2048
        || graphhelm_runtime::context::secret_shaped(&edit.reason)
        || edit.document.evidence_id.is_empty()
        || graphhelm_runtime::context::secret_shaped(&edit.document.evidence_id)
    {
        return Err(invalid());
    }
    Ok(())
}

/// The stream reference is checked BEFORE decrypting; another run cannot lend its authority.
fn envelope(
    events: &Path,
    execution: &str,
    evidence: &str,
    keyring: &signal::SignalKeyring,
    expected_kind: &str,
) -> Result<serde_json::Value, Failure> {
    let id = EvidenceId::parse(evidence)
        .map_err(|_| argument("invalid evidence reference", "/document/evidenceId"))?;
    let (scope, sealed) = {
        let store = event_store(events).map_err(|e| repository_failure(&e))?;
        let (scope, _, history) = super::resolve_stream(&store, Some(execution))?;
        let belongs = history.iter().any(|event| {
            matches!(&event.kind, EventKind::SignalRecorded(record) if record.kind == expected_kind)
                && event.evidence_refs.iter().any(|r| r.evidence_id() == &id)
        });
        if !belongs {
            return Err(argument(
                "this run does not reference that delivery",
                "/document/evidenceId",
            ));
        }
        let EvidenceRead::Available(sealed) = store
            .sealed_evidence(&scope, &id)
            .map_err(|e| repository_failure(&e))?
        else {
            return Err(execution_state(
                "the delivery evidence is unavailable",
                "/document",
            ));
        };
        (scope, sealed)
    };
    let opener = signal::open_sealer(keyring)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .map_err(|_| execution_state("the evidence reader could not start", "/document"))?;
    let plaintext = runtime
        .block_on(opener.open(scope, &sealed))
        .map_err(|_| execution_state("the delivery evidence could not be opened", "/document"))?;
    plaintext
        .expose(|bytes| serde_json::from_slice(bytes))
        .map_err(|_| argument("the delivery evidence is not valid JSON", "/document"))
}

fn binding(
    events: &Path,
    execution: &str,
    project: &Path,
    document: &DocumentReference,
    keyring: &signal::SignalKeyring,
) -> Result<(delivery::DeliveryRecord, String), Failure> {
    let value = envelope(
        events,
        execution,
        &document.evidence_id,
        keyring,
        "node_delivery",
    )?;
    let description = value
        .get("description")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| argument("the delivery has no structured description", "/document"))?;
    let record = delivery::parse_record(description.as_bytes())?;
    if record.project_id != delivery::project_id(project)? {
        return Err(execution_state(
            "the delivery belongs to a different configured project",
            "/project",
        ));
    }
    let path = record
        .documents
        .get(document.index)
        .ok_or_else(|| {
            argument(
                "the delivery does not reference that document",
                "/document/index",
            )
        })?
        .path
        .clone();
    Ok((record, path))
}

fn check_protected_path(
    project: &Path,
    path: &str,
    protected: &[std::path::PathBuf],
) -> Result<(), Failure> {
    let target = project
        .join(path)
        .canonicalize()
        .map_err(|_| execution_state("the project document is unavailable", "/document"))?;
    for directory in protected {
        let canonical = directory.canonicalize().map_err(|_| {
            execution_state("a protected directory could not be verified", "/project")
        })?;
        if target.starts_with(canonical) {
            return Err(execution_state(
                "the document is inside a protected directory",
                "/document",
            ));
        }
    }
    Ok(())
}

/// Caller-specific protected directories only extend these mandatory Runtime storage roots.
fn protected_directories(
    events: &Path,
    keyring: &signal::SignalKeyring,
    additional: &[std::path::PathBuf],
) -> Vec<std::path::PathBuf> {
    let mut directories = vec![events.to_path_buf(), keyring.directory.clone()];
    directories.extend_from_slice(additional);
    directories
}

pub(crate) fn check_protected_reference(
    events: &Path,
    execution: &str,
    project: &Path,
    document: &DocumentReference,
    keyring: &signal::SignalKeyring,
    protected: &[std::path::PathBuf],
) -> Result<(), Failure> {
    let (_, path) = binding(events, execution, project, document, keyring)?;
    check_protected_path(
        project,
        &path,
        &protected_directories(events, keyring, protected),
    )
}

pub(crate) fn read(
    events: &Path,
    execution: &str,
    project: &Path,
    document: &DocumentReference,
    keyring: &signal::SignalKeyring,
) -> Result<serde_json::Value, Failure> {
    let (_, path) = binding(events, execution, project, document, keyring)?;
    check_protected_path(project, &path, &protected_directories(events, keyring, &[]))?;
    let snapshot = ProjectDocuments::open(project)
        .and_then(|p| p.read(&path))
        .map_err(document_failure)?;
    Ok(
        serde_json::json!({"content":snapshot.content,"contentSha256":snapshot.content_sha256,"target":"main_project"}),
    )
}

/// Bound the association census before making a filesystem change. Refuse an oversized census
/// rather than saving and silently omitting some runs. Only a matching sealed delivery binds a run.
fn associated_runs(
    events: &Path,
    project_id: &str,
    keyring: &signal::SignalKeyring,
) -> Result<Vec<String>, Failure> {
    let candidates = {
        let store = event_store(events).map_err(|e| repository_failure(&e))?;
        let streams = store.list_streams().map_err(|e| repository_failure(&e))?;
        if streams.len() > 256 {
            return Err(execution_state(
                "the project association census exceeds 256 runs; no file was changed",
                "/project",
            ));
        }
        let mut candidates = Vec::new();
        for stream in streams {
            if !super::addressable_scope(&stream.stream_id).is_ok_and(|scope| scope == stream.scope)
            {
                continue;
            }
            let history = store
                .read_replay_stream(&stream.scope, &stream.stream_id)
                .map_err(|e| repository_failure(&e))?;
            let refs: Vec<String> = history
                .iter()
                .filter(|event| {
                    matches!(&event.kind,
                EventKind::SignalRecorded(record) if record.kind == "node_delivery")
                })
                .flat_map(|event| {
                    event
                        .evidence_refs
                        .iter()
                        .map(|r| r.evidence_id().as_str().to_owned())
                })
                .collect();
            if refs.len() > 128 {
                return Err(execution_state(
                    "the project association census exceeds 128 deliveries per run; no file was changed",
                    "/project",
                ));
            }
            candidates.push((stream.stream_id.as_str().to_owned(), refs));
        }
        candidates
    };
    let mut result = BTreeSet::new();
    for (run, refs) in candidates {
        for reference in refs {
            let value = envelope(events, &run, &reference, keyring, "node_delivery")?;
            let description = value
                .get("description")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| argument("a delivery has no structured description", "/document"))?;
            if delivery::parse_record(description.as_bytes())?.project_id == project_id {
                result.insert(run.clone());
                break;
            }
        }
    }
    Ok(result.into_iter().collect())
}

fn recorded_envelope(
    events: &Path,
    execution: &str,
    id: &str,
    kind: &str,
    keyring: &signal::SignalKeyring,
) -> Result<Option<serde_json::Value>, Failure> {
    let evidence = {
        let store = event_store(events).map_err(|e| repository_failure(&e))?;
        let (_, _, history) = super::resolve_stream(&store, Some(execution))?;
        history
            .iter()
            .find(|event| {
                matches!(&event.kind, EventKind::SignalRecorded(record)
            if record.signal_id.as_str() == id && record.kind == kind)
            })
            .and_then(|event| event.evidence_refs.first())
            .map(|r| r.evidence_id().as_str().to_owned())
    };
    evidence
        .map(|id| envelope(events, execution, &id, keyring, kind))
        .transpose()
}

/// Included in the next ordinary cognitive node's actual task input, never in a blind judge's
/// diet. Bounded recent notices include their identities; an older-count makes truncation explicit.
pub(crate) fn notice_context(
    events: &Path,
    execution: &str,
    keyring: &signal::SignalKeyring,
) -> Result<String, Failure> {
    let store = event_store(events).map_err(|e| repository_failure(&e))?;
    let (scope, _, history) = super::resolve_stream(&store, Some(execution))?;
    let notices: Vec<_> = history
        .iter()
        .filter(|event| {
            matches!(&event.kind,
        EventKind::SignalRecorded(record) if record.kind == "owner_document_changed")
        })
        .collect();
    let count = notices.len();
    if count == 0 {
        return Ok(String::new());
    }
    let mut candidates = Vec::new();
    for event in notices {
        let EventKind::SignalRecorded(record) = &event.kind else {
            return Err(argument("invalid owner change notice", "/notice"));
        };
        let reference = event
            .evidence_refs
            .first()
            .filter(|_| event.evidence_refs.len() == 1)
            .ok_or_else(|| argument("invalid owner change notice", "/notice"))?;
        let Some(order) = owner_notice_order_from_parts(record.signal_id.as_str(), None) else {
            // Pre-release notices did not carry a sortable identity. Keep them explicit in
            // olderNoticeCount, but never let one ancient record force unbounded evidence reads
            // or displace a newer notice whose order can be proved before decryption.
            continue;
        };
        candidates.push((
            order,
            record.signal_id.as_str().to_owned(),
            reference.evidence_id().clone(),
        ));
    }
    candidates.sort();
    let candidates = candidates.into_iter().rev().take(16);
    let opener = signal::open_sealer(keyring)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .map_err(|_| execution_state("the evidence reader could not start", "/notice"))?;
    let mut ordered = Vec::new();
    for (order, recorded_id, reference) in candidates {
        let EvidenceRead::Available(sealed) = store
            .sealed_evidence(&scope, &reference)
            .map_err(|error| repository_failure(&error))?
        else {
            return Err(execution_state(
                "the owner change evidence is unavailable",
                "/notice",
            ));
        };
        let plaintext = runtime
            .block_on(opener.open(scope.clone(), &sealed))
            .map_err(|_| {
                execution_state("the owner change evidence could not be opened", "/notice")
            })?;
        let value: serde_json::Value = plaintext
            .expose(|bytes| serde_json::from_slice(bytes))
            .map_err(|_| argument("invalid owner change notice", "/notice"))?;
        let description = value["description"]
            .as_str()
            .ok_or_else(|| argument("invalid owner change notice", "/notice"))?;
        if description.len() > 8192 {
            return Err(argument(
                "owner change notice exceeds its input bound",
                "/notice",
            ));
        }
        let emitted_at = value["emittedAt"]
            .as_str()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| argument("invalid owner change notice", "/notice"))?;
        let emitted_at = chrono::DateTime::parse_from_rfc3339(emitted_at)
            .map_err(|_| argument("invalid owner change notice", "/notice"))?
            .with_timezone(&chrono::Utc);
        let signal_id = value["id"]
            .as_str()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| argument("invalid owner change notice", "/notice"))?;
        if signal_id != recorded_id || emitted_at.timestamp_nanos_opt() != Some(order) {
            return Err(argument("invalid owner change notice", "/notice"));
        }
        let change = serde_json::from_str::<serde_json::Value>(description)
            .map_err(|_| argument("invalid owner change notice", "/notice"))?;
        ordered.push((
            order,
            signal_id.to_owned(),
            reference.as_str().to_owned(),
            serde_json::json!({"signalId":signal_id,"change":change}),
        ));
    }
    ordered = newest_notice_records(ordered);
    let newest_first = ordered
        .into_iter()
        .map(|(_, _, _, notice)| notice)
        .collect();
    Ok(pack_notices(newest_first, count))
}

fn newest_notice_records(
    mut ordered: Vec<(i64, String, String, serde_json::Value)>,
) -> Vec<(i64, String, String, serde_json::Value)> {
    ordered.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.cmp(&right.1))
            .then_with(|| left.2.cmp(&right.2))
    });
    ordered.into_iter().rev().take(16).collect()
}

fn owner_notice_order(value: &serde_json::Value) -> Option<i64> {
    owner_notice_order_from_parts(value["id"].as_str()?, value["emittedAt"].as_str())
}

fn owner_notice_order_from_parts(signal_id: &str, emitted_at: Option<&str>) -> Option<i64> {
    if graphhelm_runtime::context::secret_shaped(signal_id) {
        return None;
    }
    let suffix = signal_id.strip_prefix("owner-change-")?;
    let (stamp, identity) = suffix.split_once('-')?;
    if stamp.len() != 20 || identity.is_empty() || !stamp.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let stamp = stamp.parse().ok()?;
    if let Some(emitted_at) = emitted_at {
        let emitted = chrono::DateTime::parse_from_rfc3339(emitted_at)
            .ok()?
            .timestamp_nanos_opt()?;
        if emitted != stamp {
            return None;
        }
    }
    Some(stamp)
}

fn owner_notice_id(change_id: &str, run_digest: &str, emitted_at: &str) -> Result<String, Failure> {
    let stamp = chrono::DateTime::parse_from_rfc3339(emitted_at)
        .map_err(|_| execution_state("the edit intent has an invalid timestamp", "/document"))?
        .timestamp_nanos_opt()
        .ok_or_else(|| execution_state("the edit intent has an invalid timestamp", "/document"))?;
    let identity = change_id.strip_prefix("owner-edit-").unwrap_or(change_id);
    Ok(format!(
        "owner-change-{stamp:020}-{identity}-{}",
        &run_digest[..16]
    ))
}

fn pack_notices(mut newest_first: Vec<serde_json::Value>, total: usize) -> String {
    loop {
        let chronological: Vec<_> = newest_first.iter().rev().collect();
        let text = serde_json::json!({"scope":"main_project","olderNoticeCount":total.saturating_sub(newest_first.len()),"notices":chronological}).to_string();
        if text.len() <= 65_536 {
            return text;
        }
        newest_first.pop();
    }
}

fn append_notice(
    events: &Path,
    execution: &str,
    value: &serde_json::Value,
    actor: PersistedActor,
    keyring: &signal::SignalKeyring,
) -> Result<(), Failure> {
    let id = value["id"]
        .as_str()
        .ok_or_else(|| argument("missing change identity", "/document"))?;
    let kind = value["type"]
        .as_str()
        .ok_or_else(|| argument("missing change type", "/document"))?;
    if let Some(previous) = recorded_envelope(events, execution, id, kind, keyring)? {
        if previous == *value {
            return Ok(());
        }
        return Err(execution_state(
            "the change identity was already used differently",
            "/idempotencyKey",
        ));
    }
    let bytes =
        serde_json::to_vec(value).map_err(|_| argument("invalid change notice", "/document"))?;
    let key = OpaqueId::parse(format!("{id}-record"))
        .map_err(|_| argument("invalid change identity", "/idempotencyKey"))?;
    signal::execute(
        events,
        Some(execution),
        &bytes,
        None,
        actor,
        key,
        Some(keyring),
    )?;
    Ok(())
}

fn append_notice_compatible(
    events: &Path,
    execution: &str,
    value: &serde_json::Value,
    legacy_id: &str,
    actor: PersistedActor,
    keyring: &signal::SignalKeyring,
) -> Result<(), Failure> {
    let id = value["id"]
        .as_str()
        .ok_or_else(|| argument("missing change identity", "/document"))?;
    let kind = value["type"]
        .as_str()
        .ok_or_else(|| argument("missing change type", "/document"))?;
    if recorded_envelope(events, execution, id, kind, keyring)?.is_some() {
        return append_notice(events, execution, value, actor, keyring);
    }
    if let Some(previous) = recorded_envelope(events, execution, legacy_id, kind, keyring)? {
        let mut expected = value.clone();
        expected["id"] = legacy_id.into();
        if previous == expected {
            return Ok(());
        }
        return Err(execution_state(
            "the change identity was already used differently",
            "/idempotencyKey",
        ));
    }
    append_notice(events, execution, value, actor, keyring)
}

pub(crate) fn save(
    events: &Path,
    execution: &str,
    project: &Path,
    request: &SaveDocument,
    actor: PersistedActor,
    keyring: &signal::SignalKeyring,
) -> Result<serde_json::Value, Failure> {
    save_with_protected(
        events,
        execution,
        project,
        request,
        actor,
        keyring,
        std::slice::from_ref(&keyring.directory),
    )
}

pub(crate) fn save_with_protected(
    events: &Path,
    execution: &str,
    project: &Path,
    request: &SaveDocument,
    actor: PersistedActor,
    keyring: &signal::SignalKeyring,
    protected: &[std::path::PathBuf],
) -> Result<serde_json::Value, Failure> {
    if request.reason.trim().is_empty()
        || request.reason.len() > 2048
        || request.idempotency_key.is_empty()
        || request.idempotency_key.len() > 100
        || request.content.len() > graphhelm_tool_host::documents::MAX_DOCUMENT_BYTES
        || graphhelm_runtime::context::secret_shaped(&request.reason)
    {
        return Err(argument(
            "a bounded edit and change reason are required",
            "/document",
        ));
    }
    graphhelm_tool_host::documents::validate_content(&request.content).map_err(document_failure)?;
    let documents = ProjectDocuments::open(project).map_err(document_failure)?;
    let project_lock = documents.lock().map_err(document_failure)?;
    let (record, path) = binding(events, execution, project, &request.document, keyring)?;
    let after = graphhelm_graph::raw_content_sha256(request.content.as_bytes())
        .map_err(|_| argument("the document digest could not be derived", "/document"))?
        .as_str()
        .to_owned();
    let identity_bytes =
        serde_json::to_vec(&(execution, &record.project_id, &request.idempotency_key))
            .map_err(|_| argument("invalid edit identity", "/idempotencyKey"))?;
    let identity = graphhelm_graph::raw_content_sha256(&identity_bytes)
        .map_err(|_| argument("invalid edit identity", "/idempotencyKey"))?;
    let change_id = format!("owner-edit-{}", &identity.as_str()[..32]);
    let data = serde_json::json!({"actor":actor,"projectId":record.project_id,"path":path,"beforeSha256":request.expected_sha256,
        "afterSha256":after,"reason":request.reason,"document":request.document});
    if data.to_string().len() > 8192 {
        return Err(argument(
            "the encoded change notice exceeds its size limit",
            "/reason",
        ));
    }
    let receipt_id = format!("{change_id}-saved");
    let saved_receipt = recorded_envelope(
        events,
        execution,
        &receipt_id,
        "owner_document_edit_saved",
        keyring,
    )?;
    // Historical notification retries require the immutable receipt, not today's file contents.
    let current = if saved_receipt.is_none() {
        check_protected_path(
            project,
            &path,
            &protected_directories(events, keyring, protected),
        )?;
        Some(documents.read(&path).map_err(document_failure)?)
    } else {
        None
    };
    let intent = match recorded_envelope(
        events,
        execution,
        &change_id,
        "owner_document_edit_intent",
        keyring,
    )? {
        Some(previous) => {
            let payload: serde_json::Value = serde_json::from_str(
                previous["description"].as_str().unwrap_or(""),
            )
            .map_err(|_| execution_state("the saved edit intent is unreadable", "/document"))?;
            if payload["edit"] != data {
                return Err(execution_state(
                    "the edit identity was already used differently",
                    "/idempotencyKey",
                ));
            }
            previous
        }
        None => {
            if current
                .as_ref()
                .is_none_or(|snapshot| snapshot.content_sha256 != request.expected_sha256)
            {
                return Err(document_failure(DocumentError::Conflict));
            }
            let runs = associated_runs(events, &record.project_id, keyring)?;
            let value = serde_json::json!({"id":change_id,"source":{"type":"user","id":"owner"},
                "type":"owner_document_edit_intent","severity":"low",
                "description":serde_json::json!({"edit":data,"runs":runs}).to_string(),
                "evidence":[format!("project-document:{path}")],"emittedAt":chrono::Utc::now().to_rfc3339()});
            append_notice(events, execution, &value, actor.clone(), keyring)?;
            value
        }
    };
    // Without a saved receipt, matching visible bytes do not prove durable publication (a
    // previous directory sync may have failed). Re-publish the same bytes to confirm durability.
    // Once a receipt exists, historical retries skip the filesystem entirely.
    if let Some(current) = current {
        let expected = if current.content_sha256 == after {
            &after
        } else {
            &request.expected_sha256
        };
        documents
            .save_locked(&project_lock, &path, expected, &request.content)
            .map_err(document_failure)?;
    }
    let payload: serde_json::Value =
        serde_json::from_str(intent["description"].as_str().unwrap_or(""))
            .map_err(|_| execution_state("the edit intent is unreadable", "/document"))?;
    let runs = payload["runs"]
        .as_array()
        .ok_or_else(|| execution_state("the edit intent has no run census", "/document"))?;
    let receipt = serde_json::json!({"id":receipt_id,"source":{"type":"user","id":"owner"},
        "type":"owner_document_edit_saved","severity":"low","description":data.to_string(),
        "evidence":[format!("project-document:{path}")],"emittedAt":intent["emittedAt"]});
    // Record that the file was published before fanout. Later retries can notify about this
    // historical edit even after another edit moved the file again, without replaying old bytes.
    if append_notice(events, execution, &receipt, actor.clone(), keyring).is_err() {
        return Ok(
            serde_json::json!({"contentSha256":after,"changeId":change_id,"target":"main_project",
            "notification":{"status":"pending","notifiedRuns":[],"pendingRuns":runs}}),
        );
    }
    let mut notified = Vec::new();
    let mut pending = Vec::new();
    for run in runs.iter().filter_map(serde_json::Value::as_str) {
        let run_digest = graphhelm_graph::raw_content_sha256(run.as_bytes())
            .map_err(|_| argument("invalid run identity", "/document"))?;
        let emitted_at = intent["emittedAt"]
            .as_str()
            .ok_or_else(|| execution_state("the edit intent has no timestamp", "/document"))?;
        let notice_id = owner_notice_id(&change_id, run_digest.as_str(), emitted_at)?;
        let legacy_id = format!("{change_id}-{}", &run_digest.as_str()[..16]);
        let notice = serde_json::json!({"id":notice_id,"source":{"type":"user","id":"owner"},
            "type":"owner_document_changed","severity":"medium","description":data.to_string(),
            "evidence":[format!("project-document:{path}")],"emittedAt":intent["emittedAt"]});
        if append_notice_compatible(events, run, &notice, &legacy_id, actor.clone(), keyring)
            .is_ok()
        {
            notified.push(run);
        } else {
            pending.push(run);
        }
    }
    Ok(
        serde_json::json!({"contentSha256":after,"changeId":change_id,"target":"main_project",
        "notification":{"status":if pending.is_empty(){"recorded"}else{"pending"},"notifiedRuns":notified,"pendingRuns":pending}}),
    )
}

#[cfg(test)]
mod notice_tests {
    #[test]
    fn save_entrypoint_uses_the_project_transaction_lock() {
        let scratch = tempfile::tempdir().unwrap();
        let project = scratch.path().join("project");
        std::fs::create_dir(&project).unwrap();
        let documents = super::ProjectDocuments::open(&project).unwrap();
        let lock = documents.lock().unwrap();
        let request = super::SaveDocument {
            document: super::DocumentReference {
                evidence_id: "missing".into(),
                index: 0,
            },
            content: "content".into(),
            expected_sha256: "0".repeat(64),
            reason: "Valid contention probe".into(),
            idempotency_key: "save".into(),
        };
        let actor: super::PersistedActor =
            serde_json::from_value(serde_json::json!({"type":"owner","id":"owner"})).unwrap();
        let keyring = super::signal::SignalKeyring {
            directory: project.join("vaultdata"),
            key_id: "test".into(),
        };
        let busy = super::save_with_protected(
            &project.join("events"),
            "run",
            &project,
            &request,
            actor.clone(),
            &keyring,
            &[],
        )
        .unwrap_err();
        assert_eq!(busy.code, crate::error_codes::GHCLI005_EXECUTION_STATE);
        assert_eq!(busy.message, super::DocumentError::Busy.to_string());
        drop(lock);
        let validation = super::save_with_protected(
            &project.join("events"),
            "run",
            &project,
            &request,
            actor,
            &keyring,
            &[],
        )
        .unwrap_err();
        assert_eq!(
            validation.code,
            crate::error_codes::GHCLI001_ARGUMENT_INVALID
        );
    }

    #[test]
    fn input_budget_keeps_newest_notices_and_reports_omissions() {
        let entries: Vec<_> = (0..16)
            .map(|id| serde_json::json!({"signalId":id,"reason":"\\".repeat(2048)}))
            .collect();
        let packed = super::pack_notices(entries, 20);
        assert!(packed.len() <= 65_536);
        let value: serde_json::Value = serde_json::from_str(&packed).unwrap();
        let notices = value["notices"].as_array().unwrap();
        assert!(notices.len() < 16);
        assert_eq!(notices.last().unwrap()["signalId"], 0);
        assert_eq!(value["olderNoticeCount"], 20 - notices.len());
    }

    #[test]
    fn historical_retry_stays_before_newer_owner_notice() {
        let timestamp = |value: &str| {
            chrono::DateTime::parse_from_rfc3339(value)
                .unwrap()
                .timestamp_nanos_opt()
                .unwrap()
        };
        let newest_first = super::newest_notice_records(vec![
            (
                timestamp("2026-09-14T12:00:00Z"),
                "new".into(),
                "new-ref".into(),
                serde_json::json!({"signalId":"new"}),
            ),
            (
                timestamp("2026-09-14T11:30:00Z"),
                "old".into(),
                "old-ref".into(),
                serde_json::json!({"signalId":"old"}),
            ),
            (
                timestamp("2026-09-14T13:00:00+02:00"),
                "retry".into(),
                "retry-ref".into(),
                serde_json::json!({"signalId":"retry"}),
            ),
        ]);
        assert_eq!(
            newest_first
                .iter()
                .map(|notice| notice.3["signalId"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["new", "old", "retry"]
        );
        let id = super::owner_notice_id(
            "owner-edit-0123456789abcdef0123456789abcdef",
            "fedcba98765432100123456789abcdef0123456789abcdef0123456789abcdef",
            "2026-09-14T13:00:00+02:00",
        )
        .ok()
        .unwrap();
        assert_eq!(
            super::owner_notice_order_from_parts(&id, None).unwrap(),
            timestamp("2026-09-14T11:00:00Z")
        );
        assert!(
            super::owner_notice_order_from_parts(
                "owner-edit-0123456789abcdef0123456789abcdef-fedcba9876543210",
                None,
            )
            .is_none()
        );
    }
}
