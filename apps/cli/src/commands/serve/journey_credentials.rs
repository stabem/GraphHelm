//! #1309 slice 1: owner-issued journey credential grants in one private, durable journal.
//!
//! RFC-1309 ("Registry and commit point") makes this journal the only authority for grants: it
//! lives beside, not inside, the events directory (`<parent-of-events>/credential-broker/`), it is
//! never copied into the execution Event Store, and the active-grant view is rebuilt by replaying
//! it. This slice records grants only. It issues no token and changes no journey route: secret
//! delivery and approval authority are still open decisions on #1309.
//!
//! Rules:
//!
//! - **Commit point is the durable sync.** A frame is appended, flushed and `sync_data`-ed before
//!   the in-memory view changes and before the caller is answered. A failed write or sync leaves
//!   the outcome on disk uncertain, so the journal stops serving until a restart replays it.
//! - **Replay accepts only complete, checksummed, consecutively sequenced frames.** A torn or
//!   corrupt tail makes the routes refuse; it is never truncated or guessed past.
//! - **One holder.** The broker takes an exclusive OS lock on `journey-credentials.v1.lock` for
//!   its lifetime. Without that lock the routes refuse; there is no unlocked fallback.
//! - **Bounded** before any write: a frame over `MAX_FRAME_BYTES` or a journal at `MAX_RECORDS`
//!   refuses with a stable code, so idempotency keys cannot grow the journal without limit.
//! - **Idempotent.** The same `Idempotency-Key` with the same request returns the first receipt;
//!   with a different request it is refused as a conflict.
//! - **Owner only.** The routes are outside every agent allow-list in the auth layer, and the
//!   handlers also refuse any request whose actor type is not `owner`.

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use axum::body::Bytes;
use axum::extract::{Path as UrlPath, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use graphhelm_protocols::{ActorId, Diagnostic, OpaqueId};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::{ServeState, respond};
use crate::output::Outcome;

const CREATE_COMMAND: &str = "journey.grant.create";
const REVOKE_COMMAND: &str = "journey.grant.revoke";
const SOURCE: &str = "graphhelm serve";
const DIR_NAME: &str = "credential-broker";
const LOG_NAME: &str = "journey-credentials.v1.log";
const LOCK_NAME: &str = "journey-credentials.v1.lock";
/// One frame's payload, in bytes. A grant record is a few hundred bytes.
const MAX_FRAME_BYTES: usize = 4096;
/// Records the journal may hold before it refuses further writes.
const MAX_RECORDS: u64 = 10_000;
const HEADER_BYTES: usize = 4 + 32;
const MAX_ID_LEN: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Grant {
    grant_id: String,
    project_id: String,
    execution_id: String,
    lane_id: String,
    flow_id: String,
    purpose: String,
    expires_at: String,
    revoked: bool,
}

impl Grant {
    fn view(&self) -> Value {
        json!({
            "grantId": self.grant_id,
            "projectId": self.project_id,
            "executionId": self.execution_id,
            "laneId": self.lane_id,
            "flowId": self.flow_id,
            "purpose": self.purpose,
            "expiresAt": self.expires_at,
            "issuer": "owner",
            "state": if self.revoked { "revoked" } else { "active" },
        })
    }
}

#[derive(Debug, Default)]
struct View {
    sequence: u64,
    grants: BTreeMap<String, Grant>,
    /// Idempotency key -> (canonical request digest, receipt).
    receipts: BTreeMap<String, (String, Value)>,
}

/// Why the journal cannot serve. Each maps to one stable refusal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Unavailable {
    /// Another process holds the lifetime lock.
    Locked,
    /// The directory, a file or a frame could not be trusted (symlink, torn tail, bad checksum,
    /// wrong sequence, unreadable record).
    Corrupt,
    /// A write or sync failed after the journal opened; restart replays what committed.
    Uncertain,
}

impl Unavailable {
    fn message(self) -> &'static str {
        match self {
            Self::Locked => "the journey credential journal is held by another process",
            Self::Corrupt => {
                "the journey credential journal cannot be trusted; it needs explicit recovery"
            }
            Self::Uncertain => {
                "a journey credential write did not complete; restart the Runtime to replay the journal"
            }
        }
    }
}

struct Open {
    log: File,
    /// Held for the broker's lifetime; dropping it releases the OS lock.
    _lock: File,
    view: View,
}

/// The journal as `serve` holds it: open, or the reason it cannot serve.
pub(super) struct Journal {
    inner: Mutex<Result<Open, Unavailable>>,
}

impl Journal {
    /// Opens (creating if absent) the journal beside `events` and replays it. Never fails `serve`
    /// start: an unusable journal answers its routes with a refusal instead.
    pub(super) fn open(events: &Path) -> Self {
        Self {
            inner: Mutex::new(open_at(&broker_dir(events))),
        }
    }

    #[cfg(test)]
    fn open_dir(dir: &Path) -> Self {
        Self {
            inner: Mutex::new(open_at(dir)),
        }
    }
}

fn broker_dir(events: &Path) -> PathBuf {
    let parent = events
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    parent.join(DIR_NAME)
}

fn is_link_or_odd(path: &Path, dir: bool) -> bool {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            metadata.file_type().is_symlink()
                || if dir {
                    !metadata.is_dir()
                } else {
                    !metadata.is_file()
                }
        }
        Err(_) => false,
    }
}

fn open_at(dir: &Path) -> Result<Open, Unavailable> {
    if is_link_or_odd(dir, true) {
        return Err(Unavailable::Corrupt);
    }
    std::fs::create_dir_all(dir).map_err(|_| Unavailable::Corrupt)?;
    let lock_path = dir.join(LOCK_NAME);
    let log_path = dir.join(LOG_NAME);
    if is_link_or_odd(&lock_path, false) || is_link_or_odd(&log_path, false) {
        return Err(Unavailable::Corrupt);
    }
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|_| Unavailable::Corrupt)?;
    fs2::FileExt::try_lock_exclusive(&lock).map_err(|_| Unavailable::Locked)?;
    let mut log = OpenOptions::new()
        .read(true)
        .append(true)
        .create(true)
        .open(&log_path)
        .map_err(|_| Unavailable::Corrupt)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let owner_only = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(&log_path, owner_only.clone())
            .map_err(|_| Unavailable::Corrupt)?;
        std::fs::set_permissions(&lock_path, owner_only).map_err(|_| Unavailable::Corrupt)?;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| Unavailable::Corrupt)?;
        File::open(dir)
            .and_then(|handle| handle.sync_all())
            .map_err(|_| Unavailable::Corrupt)?;
    }
    let mut bytes = Vec::new();
    log.read_to_end(&mut bytes)
        .map_err(|_| Unavailable::Corrupt)?;
    let view = replay(&bytes).ok_or(Unavailable::Corrupt)?;
    Ok(Open {
        log,
        _lock: lock,
        view,
    })
}

fn checksum(sequence: u64, payload: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(sequence.to_le_bytes());
    hasher.update(payload);
    hasher.finalize().into()
}

fn encode(sequence: u64, payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(HEADER_BYTES + payload.len());
    frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    frame.extend_from_slice(&checksum(sequence, payload));
    frame.extend_from_slice(payload);
    frame
}

/// Rebuilds the view from the whole journal, or `None` when any byte of it cannot be trusted.
fn replay(mut bytes: &[u8]) -> Option<View> {
    let mut view = View::default();
    while !bytes.is_empty() {
        if bytes.len() < HEADER_BYTES {
            return None;
        }
        let length = u32::from_le_bytes(bytes[..4].try_into().ok()?) as usize;
        if length == 0 || length > MAX_FRAME_BYTES || bytes.len() < HEADER_BYTES + length {
            return None;
        }
        let payload = &bytes[HEADER_BYTES..HEADER_BYTES + length];
        let sequence = view.sequence + 1;
        if bytes[4..HEADER_BYTES] != checksum(sequence, payload) {
            return None;
        }
        let record: Value = serde_json::from_slice(payload).ok()?;
        if record["sequence"].as_u64() != Some(sequence) {
            return None;
        }
        apply(&mut view, &record)?;
        view.sequence = sequence;
        bytes = &bytes[HEADER_BYTES + length..];
    }
    Some(view)
}

/// Applies one committed record. `None` is a record replay must refuse: unknown kind, a duplicate
/// grant, a revoke of a grant that is absent or already revoked, or a malformed field.
fn apply(view: &mut View, record: &Value) -> Option<()> {
    let key = record["idempotencyKey"].as_str()?.to_owned();
    let digest = record["requestDigest"].as_str()?.to_owned();
    if view.receipts.contains_key(&key) {
        return None;
    }
    let receipt = match record["kind"].as_str()? {
        "GrantCreated" => {
            let fields = &record["grant"];
            let field = |name: &str| fields[name].as_str().map(str::to_owned);
            let grant = Grant {
                grant_id: field("grantId")?,
                project_id: field("projectId")?,
                execution_id: field("executionId")?,
                lane_id: field("laneId")?,
                flow_id: field("flowId")?,
                purpose: field("purpose")?,
                expires_at: field("expiresAt")?,
                revoked: false,
            };
            if fields["issuer"] != "owner" || view.grants.contains_key(&grant.grant_id) {
                return None;
            }
            let receipt = grant.view();
            view.grants.insert(grant.grant_id.clone(), grant);
            receipt
        }
        "GrantRevoked" => {
            let grant = view.grants.get_mut(record["grantId"].as_str()?)?;
            if grant.revoked {
                return None;
            }
            grant.revoked = true;
            grant.view()
        }
        _ => return None,
    };
    view.receipts.insert(key, (digest, receipt));
    Some(())
}

/// The outcome of one mutation, before it becomes an HTTP response.
#[derive(Debug, PartialEq)]
enum Answer {
    Ok(Value),
    Refused(StatusCode, &'static str, &'static str, String),
}

fn refused(status: StatusCode, code: &'static str, pointer: &'static str, message: &str) -> Answer {
    Answer::Refused(status, code, pointer, message.to_owned())
}

impl Journal {
    /// Appends `record` (without its `sequence`) under the idempotency key, or answers the
    /// receipt already committed under it. `check` validates against the current view first.
    fn append(
        &self,
        key: &str,
        request: &Value,
        check: impl FnOnce(&View) -> Result<Value, Answer>,
    ) -> Answer {
        let mut guard = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let open = match guard.as_mut() {
            Ok(open) => open,
            Err(reason) => {
                return refused(
                    StatusCode::SERVICE_UNAVAILABLE,
                    crate::error_codes::GHCLI005_EXECUTION_STATE,
                    "/journal",
                    reason.message(),
                );
            }
        };
        let digest = hex::encode(Sha256::digest(request.to_string().as_bytes()));
        if let Some((committed, receipt)) = open.view.receipts.get(key) {
            return if *committed == digest {
                Answer::Ok(receipt.clone())
            } else {
                refused(
                    StatusCode::CONFLICT,
                    crate::error_codes::GHCLI005_EXECUTION_STATE,
                    "/idempotencyKey",
                    "this Idempotency-Key was already used for a different request",
                )
            };
        }
        let mut record = match check(&open.view) {
            Ok(record) => record,
            Err(answer) => return answer,
        };
        if open.view.sequence >= MAX_RECORDS {
            return refused(
                StatusCode::CONFLICT,
                crate::error_codes::GHCLI005_EXECUTION_STATE,
                "/journal",
                "the journey credential journal is full",
            );
        }
        let sequence = open.view.sequence + 1;
        record["sequence"] = json!(sequence);
        record["idempotencyKey"] = json!(key);
        record["requestDigest"] = json!(digest);
        let payload = record.to_string().into_bytes();
        if payload.len() > MAX_FRAME_BYTES {
            return refused(
                StatusCode::BAD_REQUEST,
                crate::error_codes::GHCLI001_ARGUMENT_INVALID,
                "/",
                "the grant record is too large",
            );
        }
        let mut next = View {
            sequence: open.view.sequence,
            grants: open.view.grants.clone(),
            receipts: open.view.receipts.clone(),
        };
        if apply(&mut next, &record).is_none() {
            return refused(
                StatusCode::CONFLICT,
                crate::error_codes::GHCLI005_EXECUTION_STATE,
                "/grantId",
                "the grant cannot change to that state",
            );
        }
        next.sequence = sequence;
        let frame = encode(sequence, &payload);
        let written = open
            .log
            .write_all(&frame)
            .and_then(|()| open.log.flush())
            .and_then(|()| open.log.sync_data());
        if written.is_err() {
            *guard = Err(Unavailable::Uncertain);
            return refused(
                StatusCode::SERVICE_UNAVAILABLE,
                crate::error_codes::GHCLI005_EXECUTION_STATE,
                "/journal",
                Unavailable::Uncertain.message(),
            );
        }
        let receipt = next.receipts[key].1.clone();
        open.view = next;
        Answer::Ok(receipt)
    }
}

fn valid_id(value: &str) -> bool {
    value.len() <= MAX_ID_LEN && OpaqueId::parse(value).is_ok()
}

fn owner_and_key<'a>(headers: &'a HeaderMap) -> Result<&'a str, Answer> {
    if super::header_value(headers, "x-graphhelm-actor-type").is_some_and(|kind| kind != "owner") {
        return Err(refused(
            StatusCode::FORBIDDEN,
            crate::error_codes::GHCLI001_ARGUMENT_INVALID,
            "/actor",
            "only the owner may change a journey credential grant",
        ));
    }
    match super::header_value(headers, "idempotency-key") {
        Some(key) if !key.is_empty() && key.len() <= super::IDEMPOTENCY_HEADER_MAX_LEN => Ok(key),
        _ => Err(refused(
            StatusCode::BAD_REQUEST,
            crate::error_codes::GHCLI001_ARGUMENT_INVALID,
            "/idempotencyKey",
            "Idempotency-Key is required (1 to 64 characters)",
        )),
    }
}

fn to_response(command: &'static str, answer: Answer) -> Response {
    match answer {
        Answer::Ok(value) => respond(StatusCode::OK, Outcome::success(command, value).output),
        Answer::Refused(status, code, pointer, message) => respond(
            status,
            Outcome::domain(
                command,
                vec![Diagnostic::error(code, &message, pointer, SOURCE)],
            )
            .output,
        ),
    }
}

/// The checks a create request must pass before the journal is touched, given what the server
/// knows: its project identity and directory, and whether the execution exists.
fn create_record(
    payload: &Value,
    execution_id: &str,
    project_id: Option<&str>,
    project: Option<&Path>,
    key: &str,
) -> Result<Value, Answer> {
    let bad = |pointer: &'static str, message: &str| {
        refused(
            StatusCode::BAD_REQUEST,
            crate::error_codes::GHCLI001_ARGUMENT_INVALID,
            pointer,
            message,
        )
    };
    let (Some(project_id), Some(project)) = (project_id, project) else {
        return Err(bad(
            "/project",
            "a journey credential grant needs a Runtime started with --project",
        ));
    };
    let text = |name: &str| payload.get(name).and_then(Value::as_str);
    if text("projectId") != Some(project_id) {
        return Err(bad(
            "/projectId",
            "projectId must name this Runtime's project",
        ));
    }
    let lane = text("laneId").filter(|lane| ActorId::parse(*lane).is_ok());
    let Some(lane) = lane else {
        return Err(bad("/laneId", "laneId must be a valid actor id"));
    };
    let Some(flow) = text("flowId").filter(|flow| valid_id(flow)) else {
        return Err(bad("/flowId", "flowId must be a valid journey flow id"));
    };
    if !project
        .join(".graphhelm/journeys")
        .join(format!("{flow}.journey.yaml"))
        .is_file()
    {
        return Err(bad("/flowId", "no journey flow with this id exists"));
    }
    let Some(purpose) = text("purpose").filter(|purpose| matches!(*purpose, "author" | "reviewer"))
    else {
        return Err(bad(
            "/purpose",
            "purpose must be \"author\" or \"reviewer\"",
        ));
    };
    let Some(expires) = text("expiresAt")
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&chrono::Utc))
    else {
        return Err(bad("/expiresAt", "expiresAt must be an RFC 3339 timestamp"));
    };
    let grant_id = format!(
        "grant-{}",
        &hex::encode(Sha256::digest(format!("{execution_id}\n{key}").as_bytes()))[..24]
    );
    Ok(json!({
        "kind": "GrantCreated",
        "grant": {
            "grantId": grant_id,
            "projectId": project_id,
            "executionId": execution_id,
            "laneId": lane,
            "flowId": flow,
            "purpose": purpose,
            "expiresAt": expires.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            "issuer": "owner",
        },
    }))
}

fn execution_exists(events: &Path, execution: &str) -> bool {
    crate::commands::execution::status::read_known_within(
        events,
        Some(execution),
        crate::commands::status_read_budget(),
    )
    .is_ok()
}

/// `POST /v1/executions/{id}/journey-credential-grants`: the owner grants one lane one purpose on
/// one journey flow. Body: `{projectId, laneId, flowId, purpose, expiresAt}`.
pub(super) async fn create(
    State(state): State<ServeState>,
    UrlPath(execution_id): UrlPath<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let key = match owner_and_key(&headers) {
        Ok(key) => key.to_owned(),
        Err(answer) => return to_response(CREATE_COMMAND, answer),
    };
    let Ok(payload) = serde_json::from_slice::<Value>(&body) else {
        return to_response(
            CREATE_COMMAND,
            refused(
                StatusCode::BAD_REQUEST,
                crate::error_codes::GHCLI001_ARGUMENT_INVALID,
                "/",
                "the request body is not valid JSON",
            ),
        );
    };
    if !valid_id(&execution_id) {
        return to_response(
            CREATE_COMMAND,
            refused(
                StatusCode::BAD_REQUEST,
                crate::error_codes::GHCLI001_ARGUMENT_INVALID,
                "/executionId",
                "the execution id is not valid",
            ),
        );
    }
    let answer = tokio::task::spawn_blocking(move || {
        if !execution_exists(&state.events, &execution_id) {
            return refused(
                StatusCode::NOT_FOUND,
                crate::error_codes::GHCLI028_EXECUTION_NOT_FOUND,
                "/execution",
                "no execution with this id exists in this store",
            );
        }
        let request = json!({"executionId": execution_id, "body": payload.clone()});
        state.grants.append(&key, &request, |_| {
            create_record(
                &payload,
                &execution_id,
                state.project_id.as_deref(),
                state.project.as_deref(),
                &key,
            )
        })
    })
    .await
    .ok();
    to_response(
        CREATE_COMMAND,
        answer.unwrap_or_else(|| {
            refused(
                StatusCode::INTERNAL_SERVER_ERROR,
                crate::error_codes::GHCLI005_EXECUTION_STATE,
                "/",
                "the grant task failed",
            )
        }),
    )
}

/// `POST /v1/executions/{id}/journey-credential-grants/{grantId}/revoke`: the owner revokes one
/// active grant of this execution.
pub(super) async fn revoke(
    State(state): State<ServeState>,
    UrlPath((execution_id, grant_id)): UrlPath<(String, String)>,
    headers: HeaderMap,
) -> Response {
    let key = match owner_and_key(&headers) {
        Ok(key) => key.to_owned(),
        Err(answer) => return to_response(REVOKE_COMMAND, answer),
    };
    let answer = tokio::task::spawn_blocking(move || {
        let request = json!({"executionId": execution_id, "grantId": grant_id});
        state
            .grants
            .append(&key, &request, |view| match view.grants.get(&grant_id) {
                Some(grant) if grant.execution_id == execution_id && !grant.revoked => {
                    Ok(json!({"kind": "GrantRevoked", "grantId": grant_id}))
                }
                Some(grant) if grant.execution_id == execution_id => Err(refused(
                    StatusCode::CONFLICT,
                    crate::error_codes::GHCLI005_EXECUTION_STATE,
                    "/grantId",
                    "the grant is already revoked",
                )),
                _ => Err(refused(
                    StatusCode::NOT_FOUND,
                    crate::error_codes::GHCLI001_ARGUMENT_INVALID,
                    "/grantId",
                    "no grant with this id exists for this execution",
                )),
            })
    })
    .await
    .ok();
    to_response(
        REVOKE_COMMAND,
        answer.unwrap_or_else(|| {
            refused(
                StatusCode::INTERNAL_SERVER_ERROR,
                crate::error_codes::GHCLI005_EXECUTION_STATE,
                "/",
                "the grant task failed",
            )
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn created(id: &str) -> Value {
        json!({"kind": "GrantCreated", "grant": {
            "grantId": id, "projectId": "p", "executionId": "e", "laneId": "agent-a",
            "flowId": "f", "purpose": "author", "expiresAt": "2030-01-01T00:00:00Z",
            "issuer": "owner"}})
    }

    #[test]
    fn a_committed_grant_replays_and_a_reused_key_answers_its_receipt_or_conflicts() {
        let dir = tempfile::tempdir().unwrap();
        let journal = Journal::open_dir(dir.path());
        let first = journal.append("k1", &json!({"a": 1}), |_| Ok(created("g1")));
        let Answer::Ok(receipt) = &first else {
            panic!("{first:?}")
        };
        assert_eq!(receipt["state"], "active");
        assert_eq!(
            journal.append("k1", &json!({"a": 1}), |_| Ok(created("g2"))),
            first
        );
        assert!(matches!(
            journal.append("k1", &json!({"a": 2}), |_| Ok(created("g2"))),
            Answer::Refused(StatusCode::CONFLICT, _, "/idempotencyKey", _)
        ));
        let revoked = journal.append("k2", &json!({}), |_| {
            Ok(json!({"kind": "GrantRevoked", "grantId": "g1"}))
        });
        assert!(matches!(&revoked, Answer::Ok(value) if value["state"] == "revoked"));
        drop(journal);
        let bytes = std::fs::read(dir.path().join(LOG_NAME)).unwrap();
        let view = replay(&bytes).expect("the journal replays");
        assert_eq!(view.sequence, 2);
        assert!(view.grants["g1"].revoked);
    }

    #[test]
    fn a_torn_or_altered_journal_does_not_replay() {
        let one = encode(1, br#"{"sequence":1}"#);
        assert!(replay(&one[..one.len() - 1]).is_none(), "torn tail");
        let mut altered = one.clone();
        let last = altered.len() - 2;
        altered[last] ^= 1;
        assert!(replay(&altered).is_none(), "bad checksum");
        let skipped = encode(2, br#"{"sequence":2}"#);
        assert!(replay(&skipped).is_none(), "sequence gap");
        let mut huge = (MAX_FRAME_BYTES as u32 + 1).to_le_bytes().to_vec();
        huge.extend_from_slice(&[0; 32]);
        assert!(replay(&huge).is_none(), "frame over the bound");
    }

    #[test]
    fn a_second_holder_and_a_corrupt_journal_refuse_every_write() {
        let dir = tempfile::tempdir().unwrap();
        let first = Journal::open_dir(dir.path());
        let second = Journal::open_dir(dir.path());
        assert!(matches!(
            second.append("k", &json!({}), |_| Ok(created("g"))),
            Answer::Refused(StatusCode::SERVICE_UNAVAILABLE, _, "/journal", _)
        ));
        drop(first);
        drop(second);
        std::fs::write(dir.path().join(LOG_NAME), b"\x05\x00").unwrap();
        let corrupt = Journal::open_dir(dir.path());
        assert!(matches!(
            corrupt.append("k", &json!({}), |_| Ok(created("g"))),
            Answer::Refused(StatusCode::SERVICE_UNAVAILABLE, _, "/journal", _)
        ));
        assert_eq!(
            std::fs::read(dir.path().join(LOG_NAME)).unwrap(),
            b"\x05\x00",
            "a refused write leaves the journal as it was"
        );
    }
}
