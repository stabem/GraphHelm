//! ADR-042 (#298): a reused subagent is a session held by the model executor for one drive.
//!
//! A session is the ordered transcript of one subagent within one execution drive: for each node
//! the subagent took, the prompt it was sent and the reply text it returned. It lives only in the
//! memory of the model executor that served the drive ([`SubagentSessions`]), keyed by the
//! `subagentId` recorded on `subagent_reused`, and is dropped when the drive ends, when a node
//! parks, on cancel, and with the process. It is never written to disk, cache or any side table:
//! what survives is what the journal already holds, plus the content-free
//! [`SessionProvenanceRecord`] each session-holding attempt seals beside its reply.
//!
//! The bound the reuse decision compares is read back here too ([`measured_bound`]): the serving
//! route's declared window against the provider counts in the subagent's SEALED accounting
//! receipts, never an estimate (D-043).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, PoisonError};

use graphhelm_events::{
    EventRepositoryError, EvidenceOpener, EvidenceRead, ExecutionProjection, LocalEventRepository,
    SealedEvidence,
};
use graphhelm_protocols::{EventEnvelope, EventKind, OpaqueId, RepositoryScope};
use serde::{Deserialize, Serialize};

use crate::context_accounting::ExecutionAccountingReceipt;
use crate::delegation::ReuseBound;

/// Media type of the sealed `session-provenance@1` record.
pub const SESSION_PROVENANCE_MEDIA_TYPE: &str = "application/vnd.graphhelm.session-provenance+json";

/// The local-ref suffix the record seals under (`exec-{id}-{node}-a{attempt}-session-provenance`).
pub const SESSION_PROVENANCE_SUFFIX: &str = "session-provenance";

/// The record's schema tag.
pub const SESSION_PROVENANCE_SCHEMA: &str = "session-provenance@1";

/// The accounting receipt's local-ref suffix, as the driver seals it.
const ACCOUNTING_RECEIPT_SUFFIX: &str = "-accounting-receipt";

/// The plain reply's local-ref suffix, as the executor seals it.
const REPLY_SUFFIX: &str = "-reply";

/// One exchange a subagent's session holds: what the node was sent on the wire and the reply text
/// it returned. In memory only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionTurn {
    pub node_id: String,
    pub attempt: u32,
    pub prompt: String,
    pub reply: String,
}

/// The in-memory session store of one model executor, for one drive (ADR-042 points 1 to 4).
///
/// `supported` is false for an executor whose route cannot accept a replayed message list (a
/// native runtime, a typed provider) and for every executor that is not a model executor: such a
/// store never holds a session, so every candidate on it is recorded `session_unavailable`.
#[derive(Debug, Default)]
pub struct SubagentSessions {
    supported: bool,
    allocated: Option<u64>,
    held: Mutex<BTreeMap<String, Vec<SessionTurn>>>,
    /// #298: subagent ids whose replayed history the provider refused as too large for its
    /// context window. Their session is dropped and never resumed again in this drive: the next
    /// dispatch records a fresh subagent with basis `bound_exceeded`.
    overflowed: Mutex<BTreeSet<String>>,
}

impl SubagentSessions {
    /// A store that holds no session, ever.
    #[must_use]
    pub fn unsupported() -> Self {
        Self::default()
    }

    /// A store that holds sessions, bounded by `allocated` tokens (`None`: no declared window, so
    /// the bound is unmeasurable and reuse is refused with `bound_unavailable`).
    #[must_use]
    pub fn new(allocated: Option<u64>) -> Self {
        Self {
            supported: true,
            allocated,
            held: Mutex::new(BTreeMap::new()),
            overflowed: Mutex::new(BTreeSet::new()),
        }
    }

    /// The store for a drive served by `route`: sessions only when its adapter replays a message
    /// list, bounded by its declared window less its output ceiling.
    #[must_use]
    pub fn for_route(route: &graphhelm_gateway::manifest::ModelRoute) -> Self {
        if route.supports_replayed_history() {
            Self::new(route.session_allocated_tokens())
        } else {
            Self::unsupported()
        }
    }

    #[must_use]
    pub const fn supported(&self) -> bool {
        self.supported
    }

    /// The tokens a session may occupy on the serving route, when declared.
    #[must_use]
    pub const fn allocated(&self) -> Option<u64> {
        self.allocated
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, Vec<SessionTurn>>> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The subagent ids whose session is held right now.
    #[must_use]
    pub fn held_ids(&self) -> BTreeSet<String> {
        self.lock().keys().cloned().collect()
    }

    /// A copy of the turns `subagent_id`'s session holds, oldest first, or `None` when it is not
    /// held.
    #[must_use]
    pub fn history(&self, subagent_id: &str) -> Option<Vec<SessionTurn>> {
        self.lock().get(subagent_id).cloned()
    }

    /// Appends one completed turn to `subagent_id`'s session. A store that does not support
    /// sessions keeps nothing.
    pub fn record(&self, subagent_id: &str, turn: SessionTurn) {
        if self.supported {
            self.lock()
                .entry(subagent_id.to_owned())
                .or_default()
                .push(turn);
        }
    }

    /// #298: the provider refused `subagent_id`'s replayed history as larger than its context
    /// window. Drops that session and remembers the refusal for the rest of the drive, so the
    /// reuse decision records the next dispatch as a fresh subagent (`bound_exceeded`).
    pub fn overflow(&self, subagent_id: &str) {
        self.lock().remove(subagent_id);
        self.overflowed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(subagent_id.to_owned());
    }

    /// The subagent ids whose replayed history the provider refused as too large.
    #[must_use]
    pub fn overflowed_ids(&self) -> BTreeSet<String> {
        self.overflowed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Drops every held session (drive end, park, cancel). Dropped, never logged.
    pub fn drop_all(&self) {
        self.lock().clear();
    }
}

/// The `sha256:<hex>` digest of `text`'s UTF-8 bytes.
#[must_use]
pub fn text_digest(text: &str) -> String {
    use sha2::Digest as _;
    format!(
        "sha256:{}",
        hex::encode(sha2::Sha256::digest(text.as_bytes()))
    )
}

/// One replayed turn, content-free.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TurnDigest {
    pub node_id: String,
    pub attempt: u32,
    pub prompt_digest: String,
    pub reply_digest: String,
}

/// The content-free `session-provenance@1` record a session-holding attempt seals (ADR-042
/// point 5): the attempt's own prompt digest (and reply digest, when it replied), and the ordered
/// digests of the turns replayed before its briefing. Digests and ids only: no prompt or reply
/// byte enters it.
///
/// Every attempt of a delegated node on a session-holding executor seals one, fresh subagents
/// included. That is how a later replay can check a replayed PROMPT: the prompt bytes are not
/// sealed anywhere, so the earlier attempt's own record is the sealed evidence its prompt digest
/// is checked against, while a replayed REPLY is checked against the earlier attempt's sealed
/// reply bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionProvenanceRecord {
    pub schema: String,
    pub subagent_id: String,
    pub node_id: String,
    pub attempt: u32,
    pub prompt_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_digest: Option<String>,
    pub replayed: Vec<TurnDigest>,
}

impl SessionProvenanceRecord {
    /// Stable bytes: serde field order, no clock, no generated id.
    #[must_use]
    pub fn stable_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("a session provenance record serializes")
    }
}

/// Why a sealed session record does not replay.
#[derive(Debug, thiserror::Error)]
pub enum SessionReplayError {
    #[error("the event repository refused the read")]
    Repository(#[from] EventRepositoryError),
    #[error("a referenced evidence item is not available")]
    EvidenceUnavailable,
    #[error("a sealed evidence item could not be opened or read")]
    Unreadable,
    #[error("a replayed turn names an attempt with no sealed session record")]
    MissingTurn { node_id: String, attempt: u32 },
    #[error("a replayed turn's prompt digest does not match the earlier attempt's sealed record")]
    PromptMismatch { node_id: String, attempt: u32 },
    #[error("a replayed turn's reply digest does not match the earlier attempt's sealed reply")]
    ReplyMismatch { node_id: String, attempt: u32 },
}

/// The sealed evidence an earlier attempt left, as replay reads it.
struct SealedTurn {
    prompt_digest: String,
    reply_digest: Option<String>,
}

/// What a replay verified: the number of session records read and the number of replayed turns
/// checked against sealed evidence.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SessionReplay {
    pub records: usize,
    pub replayed_turns: usize,
}

async fn open_bytes(
    opener: &dyn EvidenceOpener,
    scope: &RepositoryScope,
    sealed: &SealedEvidence,
) -> Result<Vec<u8>, SessionReplayError> {
    let bytes = opener
        .open(scope.clone(), sealed)
        .await
        .map_err(|_| SessionReplayError::Unreadable)?;
    Ok(bytes.consume(<[u8]>::to_vec))
}

/// The sealed items of `history`'s `node_outcome_recorded` events whose evidence id ends with
/// `suffix`, in journal order, each with the node it was recorded against.
fn sealed_items(
    store: &LocalEventRepository,
    scope: &RepositoryScope,
    history: &[EventEnvelope],
    suffix: &str,
) -> Result<Vec<(String, SealedEvidence)>, SessionReplayError> {
    let mut items = Vec::new();
    for envelope in history {
        let EventKind::NodeOutcomeRecorded(recorded) = &envelope.kind else {
            continue;
        };
        for reference in &envelope.evidence_refs {
            if !reference.evidence_id().as_str().ends_with(suffix) {
                continue;
            }
            match store.sealed_evidence(scope, reference.evidence_id())? {
                EvidenceRead::Available(sealed) => {
                    items.push((recorded.node_id.as_str().to_owned(), sealed));
                }
                EvidenceRead::Unavailable(_) => {
                    return Err(SessionReplayError::EvidenceUnavailable);
                }
            }
        }
    }
    Ok(items)
}

/// Replays every sealed `session-provenance@1` record of `stream` WITHOUT calling a model: each
/// replayed turn must name an earlier attempt that sealed its own record with the same prompt
/// digest, and whose sealed reply text has the listed reply digest.
///
/// # Errors
/// [`SessionReplayError`] on the first turn that does not match its sealed evidence, or when the
/// evidence cannot be read.
pub async fn verify_session_provenance(
    store: &LocalEventRepository,
    opener: &dyn EvidenceOpener,
    scope: &RepositoryScope,
    stream: &OpaqueId,
) -> Result<SessionReplay, SessionReplayError> {
    let history = store.read_replay_stream(scope, stream.as_str())?;
    let mut records = Vec::new();
    for (_, sealed) in sealed_items(store, scope, &history, SESSION_PROVENANCE_SUFFIX)? {
        let bytes = open_bytes(opener, scope, &sealed).await?;
        let record: SessionProvenanceRecord =
            serde_json::from_slice(&bytes).map_err(|_| SessionReplayError::Unreadable)?;
        records.push((sealed.reference().evidence_id().as_str().to_owned(), record));
    }
    // The sealed reply of each attempt, keyed by the evidence id prefix it shares with that
    // attempt's session record (`exec-{id}-{node}-a{n}-`).
    let mut replies: BTreeMap<String, String> = BTreeMap::new();
    for (_, sealed) in sealed_items(store, scope, &history, REPLY_SUFFIX)? {
        let id = sealed.reference().evidence_id().as_str();
        let prefix = id[..id.len() - REPLY_SUFFIX.len()].to_owned();
        let bytes = open_bytes(opener, scope, &sealed).await?;
        let reply: graphhelm_gateway::call::ModelReply =
            serde_json::from_slice(&bytes).map_err(|_| SessionReplayError::Unreadable)?;
        replies.insert(prefix, text_digest(&reply.text));
    }
    let mut turns: BTreeMap<(String, u32), SealedTurn> = BTreeMap::new();
    for (id, record) in &records {
        let prefix = &id[..id.len() - SESSION_PROVENANCE_SUFFIX.len() - 1];
        turns.insert(
            (record.node_id.clone(), record.attempt),
            SealedTurn {
                prompt_digest: record.prompt_digest.clone(),
                reply_digest: replies.get(prefix).cloned(),
            },
        );
    }
    let mut replay = SessionReplay {
        records: records.len(),
        replayed_turns: 0,
    };
    for (_, record) in &records {
        for turn in &record.replayed {
            let key = (turn.node_id.clone(), turn.attempt);
            let Some(sealed) = turns.get(&key) else {
                return Err(SessionReplayError::MissingTurn {
                    node_id: key.0,
                    attempt: key.1,
                });
            };
            if sealed.prompt_digest != turn.prompt_digest {
                return Err(SessionReplayError::PromptMismatch {
                    node_id: key.0,
                    attempt: key.1,
                });
            }
            if sealed.reply_digest.as_deref() != Some(turn.reply_digest.as_str()) {
                return Err(SessionReplayError::ReplyMismatch {
                    node_id: key.0,
                    attempt: key.1,
                });
            }
            replay.replayed_turns += 1;
        }
    }
    Ok(replay)
}

/// ADR-042 points 6 and 7: the bound for a dispatch, read back at dispatch.
///
/// `allocated` is the serving route's declared window (from `sessions`); `held` is what
/// `sessions` holds now; `used` is, per subagent recorded in `projection`, the LAST accounted
/// attempt's provider-reported input plus output, read from the accounting receipts sealed for
/// the nodes that subagent took. Any receipt of that subagent with either counter not measured,
/// any receipt that cannot be read, or no `opener` at all makes its `used` `None`.
///
/// # Errors
/// [`EventRepositoryError`] when the stream cannot be read.
pub async fn measured_bound(
    store_open: &crate::driver::StoreOpen,
    opener: Option<&Arc<dyn EvidenceOpener>>,
    scope: &RepositoryScope,
    stream: &OpaqueId,
    projection: &ExecutionProjection,
    sessions: Option<&SubagentSessions>,
) -> Result<ReuseBound, EventRepositoryError> {
    let Some(sessions) = sessions.filter(|sessions| sessions.supported()) else {
        return Ok(ReuseBound::unmeasured());
    };
    let mut bound = ReuseBound {
        allocated: sessions.allocated(),
        used: BTreeMap::new(),
        held: sessions.held_ids(),
        overflowed: sessions.overflowed_ids(),
    };
    // node -> subagent, from the journal's own `subagent_reused` records.
    let taken: BTreeMap<String, String> = projection
        .subagents
        .iter()
        .map(|(node, record)| (node.clone(), record.record.subagent_id.as_str().to_owned()))
        .collect();
    if taken.is_empty() {
        return Ok(bound);
    }
    let Some(opener) = opener else {
        for subagent in taken.values() {
            bound.used.insert(subagent.clone(), None);
        }
        return Ok(bound);
    };
    let (start, receipts) = {
        let store_open = store_open.clone();
        let scope = scope.clone();
        let stream = stream.clone();
        tokio::task::spawn_blocking(move || {
            let store = store_open()?;
            let history = store.read_replay_stream(&scope, stream.as_str())?;
            let start = history
                .iter()
                .find(|envelope| matches!(envelope.kind, EventKind::ExecutionStarted(_)))
                .cloned();
            let receipts = sealed_items(&store, &scope, &history, ACCOUNTING_RECEIPT_SUFFIX);
            Ok::<_, EventRepositoryError>((start, receipts))
        })
        .await
        .expect("the reader task is never cancelled")?
    };
    let receipts = receipts.ok();
    for subagent in taken.values() {
        bound.used.entry(subagent.clone()).or_insert(Some(0));
    }
    let (Some(start), Some(receipts)) = (start, receipts) else {
        for used in bound.used.values_mut() {
            *used = None;
        }
        return Ok(bound);
    };
    // Each subagent's chain in journal order; the last measured attempt is the session's size.
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for (node, sealed) in receipts {
        let Some(subagent) = taken.get(&node) else {
            continue;
        };
        let measured = match open_bytes(opener.as_ref(), scope, &sealed).await {
            Ok(bytes) => ExecutionAccountingReceipt::from_stable_bytes(&bytes, &start)
                .ok()
                .and_then(|receipt| receipt.provider_reported_total()),
            Err(_) => None,
        };
        let slot = bound.used.entry(subagent.clone()).or_insert(Some(0));
        *slot = match (seen.contains(subagent), *slot, measured) {
            // Once unavailable, the whole chain is.
            (true, None, _) | (_, _, None) => None,
            (_, _, Some(total)) => Some(total),
        };
        seen.insert(subagent.clone());
    }
    // A subagent with no accounted attempt at all has no measurement.
    for (subagent, used) in &mut bound.used {
        if !seen.contains(subagent) {
            *used = None;
        }
    }
    Ok(bound)
}
