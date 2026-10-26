//! #1714 / #1721: the journal as a chain of sealed, immutable segments plus one active segment.
//!
//! Layout, inside the repository root:
//!
//! - `journal.jsonl` is segment 0, forever. A store that never rotated is byte-for-byte the
//!   single-file 1.0.0 layout, and its `journal.jsonl` is never renamed, rewritten or truncated
//!   by a rotation: an existing journal simply becomes the first sealed segment in place.
//! - `segments/<n>.jsonl` (n >= 1, eight digits) holds every later segment. The highest one
//!   without a seal is the ACTIVE segment, the only one that is appended to.
//! - `segments/<n>.seal.json` closes segment n: its byte length, its SHA-256, the SHA-256 of the
//!   previous seal (a hash chain from [`GENESIS_HASH`]), each stream's next sequence at its end,
//!   and the SHA-256 of the state file below.
//! - `segments/<n>.<sha>.state.json` is the verifier's end state at segment n (heads, idempotency
//!   keys, reachable evidence, artifacts, active versions and markers, graph publications and
//!   the stream list) without the batch bodies. Its name carries its own digest, so a rotation
//!   interrupted before its seal leaves an unreferenced file, never a conflicting one.
//!
//! An open verifies the seal chain (each seal's canonical bytes, its link to the previous seal
//! and the length of the segment it closes), seeds the verifier from the last seal's state and
//! walks ONLY the active segment, so its cost no longer grows with history. A reader that needs
//! history reads the sealed segments it needs on demand, each checked against its seal's length
//! and digest before a byte of it is parsed. Concatenating segment 0 and `segments/1..n` in order
//! reproduces the single-file journal exactly ([`concatenated_journal`]).
//!
//! No data file is ever renamed or truncated by a rotation, so there is no half-state to guess
//! at: a published seal with no next segment file is a sealed store whose active segment is
//! empty, and the next segment is created when it is first opened.

use super::{
    ActiveSegment, EventEnvelope, EventKind, EventRepositoryError, File, FileIdentity,
    GENESIS_HASH, LoadedState, LocalEventRepository, MAX_JOURNAL_BYTES, MAX_REPOSITORY_ENTRIES,
    MAX_ROOT_ENTRIES, Path, PhysicalBatch, RepositoryScope, StoredActiveMarker,
    StoredArtifactRegistration, VerifyCtx, canonical_bytes, collect_child_names_bounded,
    ensure_child_directory, ensure_inclusive_limit, link_retained_file_between,
    open_child_directory, open_child_file, parse_physical_batch, read_bounded_file,
    read_bounded_range, remove_planned_file, stream_key, sync_directory_handle, wire_sha256,
};
use crate::repository::ActiveVersion;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::sync::Arc;

/// The root entry that holds every segment after the first.
pub(super) const SEGMENTS_DIR: &str = "segments";
/// The active segment is sealed before an append once it holds this many bytes. Half of
/// `MAX_JOURNAL_BYTES`, so the batch that follows always fits under the hard cap.
pub(super) const SEGMENT_ROTATE_BYTES: u64 = MAX_JOURNAL_BYTES / 2;
/// The active segment is sealed before an append once it holds this many events: well under
/// `MAX_READ_ALL` (250,000), which bounds what one open walks.
pub(super) const SEGMENT_ROTATE_EVENTS: usize = 100_000;
const MAX_SEAL_BYTES: u64 = 16 * 1024 * 1024;
const MAX_SEAL_STATE_BYTES: u64 = 512 * 1024 * 1024;
const SEAL_FORMAT: &str = "graphhelm.journal-seal/1";
const SEAL_STATE_FORMAT: &str = "graphhelm.journal-seal-state/1";

/// The immutable record that closes one segment.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SegmentSeal {
    format: String,
    index: u64,
    segment_bytes: u64,
    segment_sha256: String,
    previous_seal_sha256: String,
    state_sha256: String,
    /// Each stream's next sequence at the END of this segment. With the previous seal's map it
    /// says which sequences of a stream this segment holds, so a reader skips the segments that
    /// hold none of what it asked for without opening them.
    next_sequence: BTreeMap<String, u64>,
}

/// One verified seal, as an open carries it in [`LoadedState`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SealedSegment {
    seal: SegmentSeal,
    seal_sha256: String,
}

impl SealedSegment {
    pub(super) const fn index(&self) -> u64 {
        self.seal.index
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SealedActiveVersion {
    number: u64,
    semantic_hash: String,
    sequence: u64,
    event_hash: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SealedStream {
    scope: RepositoryScope,
    stream_id: String,
}

/// The verifier's state at the end of a sealed segment, without the batch bodies.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SealState {
    format: String,
    index: u64,
    last_hash: BTreeMap<String, String>,
    /// Blob FILE NAMES, never paths: a store moved to another root keeps its seals valid.
    reachable_evidence: BTreeSet<String>,
    artifacts: BTreeMap<String, StoredArtifactRegistration>,
    active_versions: BTreeMap<String, SealedActiveVersion>,
    seen_idempotency: BTreeSet<String>,
    expected_markers: BTreeMap<String, StoredActiveMarker>,
    /// Every graph publication of the sealed history: the open republishes their active markers
    /// exactly as it did when the whole history was walked.
    publications: Vec<EventEnvelope>,
    streams: BTreeMap<String, SealedStream>,
}

pub(super) fn segment_file_name(index: u64) -> String {
    format!("{index:08}.jsonl")
}

fn seal_file_name(index: u64) -> String {
    format!("{index:08}.seal.json")
}

fn state_file_name(index: u64, state_sha256: &str) -> String {
    let hex = state_sha256.strip_prefix("sha256:").unwrap_or(state_sha256);
    format!("{index:08}.{hex}.state.json")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SegmentEntry {
    Segment(u64),
    Seal(u64),
    State(u64),
}

/// Classifies one name under `segments/`. Anything else refuses the store, exactly as an
/// unknown name in the root does.
fn classify_segment_entry(name: &str) -> Result<SegmentEntry, EventRepositoryError> {
    let digits = |text: &str| -> Option<u64> {
        (text.len() == 8 && text.bytes().all(|byte| byte.is_ascii_digit()))
            .then(|| text.parse().ok())
            .flatten()
    };
    if let Some(stem) = name.strip_suffix(".seal.json")
        && let Some(index) = digits(stem)
    {
        return Ok(SegmentEntry::Seal(index));
    }
    if let Some(stem) = name.strip_suffix(".state.json")
        && let Some((index, hex)) = stem.split_once('.')
        && let Some(index) = digits(index)
        && hex.len() == 64
        && hex
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Ok(SegmentEntry::State(index));
    }
    if let Some(stem) = name.strip_suffix(".jsonl")
        && let Some(index) = digits(stem)
        && index >= 1
    {
        return Ok(SegmentEntry::Segment(index));
    }
    Err(EventRepositoryError::UnsupportedFormat)
}

/// `segments/`, when this store has rotated at least once.
pub(super) fn open_segments_dir(
    root_handle: &File,
    root: &Path,
) -> Result<Option<File>, EventRepositoryError> {
    if !collect_child_names_bounded(root_handle, root, MAX_ROOT_ENTRIES + 1)?.contains(SEGMENTS_DIR)
    {
        return Ok(None);
    }
    open_child_directory(root_handle, root, SEGMENTS_DIR).map(Some)
}

/// The names under `segments/`, each one classified (a foreign name refuses).
fn segment_entries(
    directory: &File,
    root: &Path,
) -> Result<BTreeSet<(u64, u8)>, EventRepositoryError> {
    let names =
        collect_child_names_bounded(directory, &root.join(SEGMENTS_DIR), MAX_REPOSITORY_ENTRIES)?;
    names
        .iter()
        .map(|name| {
            Ok(match classify_segment_entry(name)? {
                SegmentEntry::Segment(index) => (index, 0),
                SegmentEntry::Seal(index) => (index, 1),
                SegmentEntry::State(index) => (index, 2),
            })
        })
        .collect()
}

/// How many seals the chain holds, which is the active segment's index. Names only: the cheap
/// question every locked operation asks to notice a rotation made by another handle.
pub(super) fn sealed_count(root_handle: &File, root: &Path) -> Result<u64, EventRepositoryError> {
    let Some(directory) = open_segments_dir(root_handle, root)? else {
        return Ok(0);
    };
    let entries = segment_entries(&directory, root)?;
    let mut count = 0_u64;
    while entries.contains(&(count, 1)) {
        count += 1;
    }
    Ok(count)
}

/// Opens the active segment `index` for appending, creating it when a rotation was interrupted
/// after its seal was published and before the next segment file existed. Creating an empty
/// file is idempotent, so a reader may do it too: there is nothing in it to disagree about.
pub(super) fn open_active_segment(
    root_handle: &File,
    root: &Path,
    index: u64,
) -> Result<File, EventRepositoryError> {
    if index == 0 {
        return open_child_file(root_handle, root, "journal.jsonl", true, false);
    }
    let directory = open_child_directory(root_handle, root, SEGMENTS_DIR)?;
    let path = root.join(SEGMENTS_DIR);
    let name = segment_file_name(index);
    match open_child_file(&directory, &path, &name, true, false) {
        Ok(file) => Ok(file),
        Err(EventRepositoryError::Integrity) => {
            match open_child_file(&directory, &path, &name, true, true) {
                Ok(created) => {
                    created.sync_all()?;
                    sync_directory_handle(&directory)?;
                    // The creating handle holds DELETE access (Windows), which would refuse
                    // every later open of this name that does not share delete. Reopen plain.
                    drop(created);
                    open_child_file(&directory, &path, &name, true, false)
                }
                // Another opener created it between the two calls.
                Err(_) => open_child_file(&directory, &path, &name, true, false),
            }
        }
        Err(error) => Err(error),
    }
}

/// Opens a SEALED segment read-only.
fn open_sealed_segment(
    root_handle: &File,
    root: &Path,
    index: u64,
) -> Result<File, EventRepositoryError> {
    if index == 0 {
        return open_child_file(root_handle, root, "journal.jsonl", false, false);
    }
    let directory = open_child_directory(root_handle, root, SEGMENTS_DIR)?;
    open_child_file(
        &directory,
        &root.join(SEGMENTS_DIR),
        &segment_file_name(index),
        false,
        false,
    )
}

/// Reads and verifies the whole seal chain. Every seal must be canonical, name its own index,
/// link to the previous seal's digest and close a segment of exactly its recorded length; no
/// segment file may exist past the active one. The segments' DIGESTS are checked by the readers
/// that read them, not here: an open must not cost a pass over history.
pub(super) fn read_seal_chain(
    root_handle: &File,
    root: &Path,
) -> Result<Vec<SealedSegment>, EventRepositoryError> {
    let Some(directory) = open_segments_dir(root_handle, root)? else {
        return Ok(Vec::new());
    };
    let entries = segment_entries(&directory, root)?;
    let path = root.join(SEGMENTS_DIR);
    let mut chain: Vec<SealedSegment> = Vec::new();
    while entries.contains(&(chain.len() as u64, 1)) {
        let index = chain.len() as u64;
        let mut file = open_child_file(&directory, &path, &seal_file_name(index), false, false)?;
        let bytes = read_bounded_file(&mut file, MAX_SEAL_BYTES)?;
        let seal: SegmentSeal = serde_json::from_slice(&bytes)
            .map_err(|_| EventRepositoryError::IntegrityAt("segments:seal-decode"))?;
        let previous = chain
            .last()
            .map_or_else(|| GENESIS_HASH.to_owned(), |last| last.seal_sha256.clone());
        if canonical_bytes(&seal)? != bytes
            || seal.format != SEAL_FORMAT
            || seal.index != index
            || seal.previous_seal_sha256 != previous
        {
            return Err(EventRepositoryError::IntegrityAt("segments:seal-chain"));
        }
        ensure_inclusive_limit(seal.segment_bytes, MAX_JOURNAL_BYTES)?;
        let segment = open_sealed_segment(root_handle, root, index)?;
        if segment.metadata()?.len() != seal.segment_bytes {
            return Err(EventRepositoryError::IntegrityAt("segments:sealed-length"));
        }
        chain.push(SealedSegment {
            seal,
            seal_sha256: wire_sha256(&bytes),
        });
    }
    let active = chain.len() as u64;
    if entries
        .iter()
        .any(|(index, kind)| (*kind == 0 && *index > active) || (*kind == 1 && *index >= active))
    {
        return Err(EventRepositoryError::IntegrityAt("segments:gap"));
    }
    Ok(chain)
}

impl LocalEventRepository {
    pub(super) fn active_segment(&self) -> Result<ActiveSegment, EventRepositoryError> {
        self.active
            .lock()
            .map(|active| *active)
            .map_err(|_| EventRepositoryError::StorageAt {
                site: "active-segment:mutex-poisoned",
                os: None,
            })
    }

    pub(super) fn set_active_segment(
        &self,
        identity: FileIdentity,
        index: u64,
    ) -> Result<(), EventRepositoryError> {
        *self
            .active
            .lock()
            .map_err(|_| EventRepositoryError::StorageAt {
                site: "active-segment:mutex-poisoned",
                os: None,
            })? = ActiveSegment { identity, index };
        Ok(())
    }

    #[allow(clippy::unused_self)]
    const fn rotation_thresholds(&self) -> (u64, usize) {
        #[cfg(test)]
        if let Some(thresholds) = self.rotate_at {
            return thresholds;
        }
        (SEGMENT_ROTATE_BYTES, SEGMENT_ROTATE_EVENTS)
    }

    /// The verifier context an active-segment walk starts from: genesis when nothing is sealed,
    /// else the last seal's state, read and bound to that seal by its digest.
    pub(super) fn seeded_ctx(&self) -> Result<VerifyCtx, EventRepositoryError> {
        let chain = read_seal_chain(&self.root_handle, &self.root)?;
        let Some(last) = chain.last().cloned() else {
            return Ok(VerifyCtx::fresh());
        };
        if self.active_segment()?.index != last.index() + 1 {
            return Err(EventRepositoryError::IntegrityAt(
                "segments:active-not-after-last-seal",
            ));
        }
        let directory = open_child_directory(&self.root_handle, &self.root, SEGMENTS_DIR)?;
        let mut file = open_child_file(
            &directory,
            &self.root.join(SEGMENTS_DIR),
            &state_file_name(last.index(), &last.seal.state_sha256),
            false,
            false,
        )?;
        let bytes = read_bounded_file(&mut file, MAX_SEAL_STATE_BYTES)?;
        if wire_sha256(&bytes) != last.seal.state_sha256 {
            return Err(EventRepositoryError::IntegrityAt("segments:state-digest"));
        }
        let sealed: SealState = serde_json::from_slice(&bytes)
            .map_err(|_| EventRepositoryError::IntegrityAt("segments:state-decode"))?;
        if canonical_bytes(&sealed)? != bytes
            || sealed.format != SEAL_STATE_FORMAT
            || sealed.index != last.index()
        {
            return Err(EventRepositoryError::IntegrityAt("segments:state-shape"));
        }
        let blobs = self.root.join("blobs");
        let state = LoadedState {
            batches: Vec::new(),
            next_sequence: last.seal.next_sequence.clone(),
            last_hash: sealed.last_hash,
            reachable_evidence: sealed
                .reachable_evidence
                .into_iter()
                .map(|name| blobs.join(name))
                .collect(),
            artifacts: sealed.artifacts,
            active_versions: sealed
                .active_versions
                .into_iter()
                .map(|(key, version)| {
                    (
                        key,
                        ActiveVersion {
                            number: version.number,
                            semantic_hash: version.semantic_hash,
                            sequence: version.sequence,
                            event_hash: version.event_hash,
                        },
                    )
                })
                .collect(),
            seen_idempotency: sealed.seen_idempotency,
            expected_markers: sealed.expected_markers,
            sealed: Arc::new(chain),
            sealed_publications: Arc::new(sealed.publications),
            sealed_streams: Arc::new(
                sealed
                    .streams
                    .into_iter()
                    .map(|(key, stream)| (key, (stream.scope, stream.stream_id)))
                    .collect(),
            ),
        };
        let mut ctx = VerifyCtx::fresh();
        ctx.state = Arc::new(state);
        Ok(ctx)
    }

    /// Reads one sealed segment's batches, after checking its bytes against the seal.
    fn read_sealed_segment(
        &self,
        sealed: &SealedSegment,
    ) -> Result<Vec<PhysicalBatch>, EventRepositoryError> {
        let mut file = open_sealed_segment(&self.root_handle, &self.root, sealed.index())?;
        if file.metadata()?.len() != sealed.seal.segment_bytes {
            return Err(EventRepositoryError::IntegrityAt("segments:sealed-length"));
        }
        let bytes = read_bounded_range(&mut file, 0, sealed.seal.segment_bytes)?;
        if wire_sha256(&bytes) != sealed.seal.segment_sha256 {
            return Err(EventRepositoryError::IntegrityAt("segments:sealed-digest"));
        }
        bytes
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| parse_physical_batch(self.schemas, line))
            .collect()
    }

    /// Visits the history's batches oldest first: the sealed segments, then the active one. With
    /// `stream` set, a sealed segment holding no event of that stream past `after` is skipped
    /// unopened, so a tail reader touches only the segments its cursor has not passed. `visit`
    /// returns whether to go on.
    pub(super) fn visit_history(
        &self,
        state: &LoadedState,
        stream: Option<&str>,
        after: u64,
        mut visit: impl FnMut(&PhysicalBatch) -> Result<bool, EventRepositoryError>,
    ) -> Result<(), EventRepositoryError> {
        for (position, sealed) in state.sealed.iter().enumerate() {
            if let Some(key) = stream {
                let start = position
                    .checked_sub(1)
                    .and_then(|previous| state.sealed[previous].seal.next_sequence.get(key))
                    .copied()
                    .unwrap_or(1);
                let end = sealed.seal.next_sequence.get(key).copied().unwrap_or(1);
                if end <= start || end <= after.saturating_add(1) {
                    continue;
                }
            }
            for batch in self.read_sealed_segment(sealed)? {
                if !visit(&batch)? {
                    return Ok(());
                }
            }
        }
        for batch in &state.batches {
            if !visit(batch)? {
                return Ok(());
            }
        }
        Ok(())
    }

    /// The events of one stream past `after`, oldest first, across every segment.
    pub(super) fn stream_events_after(
        &self,
        state: &LoadedState,
        scope: &RepositoryScope,
        stream_id: &str,
        after: u64,
        limit: usize,
    ) -> Result<Vec<EventEnvelope>, EventRepositoryError> {
        let key = stream_key(scope, stream_id)?;
        let mut events = Vec::new();
        self.visit_history(state, Some(&key), after, |batch| {
            if &batch.scope == scope && batch.stream_id == stream_id {
                for event in &batch.events {
                    if event.sequence > after {
                        if events.len() == limit {
                            return Ok(false);
                        }
                        events.push(event.clone());
                    }
                }
            }
            Ok(true)
        })?;
        Ok(events)
    }

    /// The committed batch of `scope`/`stream_id` that holds any of `keys`, if one does. Asks the
    /// verified idempotency set first, so a fresh key never opens a sealed segment.
    pub(super) fn batch_holding_keys(
        &self,
        state: &LoadedState,
        scope: &RepositoryScope,
        stream_id: &str,
        keys: &BTreeSet<&str>,
    ) -> Result<Option<PhysicalBatch>, EventRepositoryError> {
        let key = stream_key(scope, stream_id)?;
        if !keys
            .iter()
            .any(|item| state.seen_idempotency.contains(&format!("{key}:{item}")))
        {
            return Ok(None);
        }
        let mut found = None;
        self.visit_history(state, Some(&key), 0, |batch| {
            if &batch.scope == scope
                && batch.stream_id == stream_id
                && batch
                    .events
                    .iter()
                    .any(|event| keys.contains(event.idempotency_key.as_str()))
            {
                found = Some(batch.clone());
                return Ok(false);
            }
            Ok(true)
        })?;
        Ok(found)
    }

    /// Every distinct (scope, stream) in the history, keyed by stream key.
    pub(super) fn history_streams(
        state: &LoadedState,
    ) -> Result<BTreeMap<String, (RepositoryScope, String)>, EventRepositoryError> {
        let mut streams = (*state.sealed_streams).clone();
        for batch in &state.batches {
            streams.insert(
                stream_key(&batch.scope, &batch.stream_id)?,
                (batch.scope.clone(), batch.stream_id.clone()),
            );
        }
        Ok(streams)
    }

    /// Every graph publication in the history, for the active-marker checks of `open`.
    pub(super) fn history_publications(state: &LoadedState) -> Vec<EventEnvelope> {
        state
            .sealed_publications
            .iter()
            .cloned()
            .chain(
                state
                    .batches
                    .iter()
                    .flat_map(|batch| &batch.events)
                    .filter(|event| matches!(event.kind, EventKind::GraphVersionPublished(_)))
                    .cloned(),
            )
            .collect()
    }

    /// Whether the next append should first seal the active segment.
    pub(super) fn should_rotate(&self, state: &LoadedState) -> Result<bool, EventRepositoryError> {
        if state.batches.is_empty() {
            return Ok(false);
        }
        let (bytes_at, events_at) = self.rotation_thresholds();
        let length = self
            .journal
            .lock()
            .map_err(|_| EventRepositoryError::StorageAt {
                site: "rotate:journal-mutex-poisoned",
                os: None,
            })?
            .metadata()?
            .len();
        let events = state
            .batches
            .iter()
            .map(|batch| batch.events.len())
            .sum::<usize>();
        Ok(length >= bytes_at || events >= events_at)
    }

    /// Seals the active segment and makes the next one active. Callers hold the EXCLUSIVE lock.
    ///
    /// Order, each step durable before the next: the state file, then the seal (whose
    /// publication IS the sealing), then the next empty segment. A crash before the seal leaves
    /// an unreferenced state file and an unchanged active segment; a crash after it leaves a
    /// sealed store whose next segment is created on its next open. Nothing is renamed or
    /// truncated, so neither case needs a recovery that could guess wrong.
    pub(super) fn rotate_segment(&self, state: &LoadedState) -> Result<(), EventRepositoryError> {
        let index = self.active_segment()?.index;
        if state.sealed.len() as u64 != index {
            return Err(EventRepositoryError::IntegrityAt(
                "rotate:active-not-after-last-seal",
            ));
        }
        let mut journal = self
            .journal
            .lock()
            .map_err(|_| EventRepositoryError::StorageAt {
                site: "rotate:journal-mutex-poisoned",
                os: None,
            })?;
        journal.sync_data()?;
        let length = journal.metadata()?.len();
        ensure_inclusive_limit(length, MAX_JOURNAL_BYTES)?;
        let bytes = read_bounded_range(&mut journal, 0, length)?;
        if bytes.last() != Some(&b'\n') {
            return Err(EventRepositoryError::IntegrityAt(
                "rotate:active-segment-unterminated",
            ));
        }
        let mut streams = (*state.sealed_streams)
            .iter()
            .map(|(key, (scope, stream_id))| {
                (
                    key.clone(),
                    SealedStream {
                        scope: scope.clone(),
                        stream_id: stream_id.clone(),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        for batch in &state.batches {
            streams.insert(
                stream_key(&batch.scope, &batch.stream_id)?,
                SealedStream {
                    scope: batch.scope.clone(),
                    stream_id: batch.stream_id.clone(),
                },
            );
        }
        let sealed_state = SealState {
            format: SEAL_STATE_FORMAT.to_owned(),
            index,
            last_hash: state.last_hash.clone(),
            reachable_evidence: state
                .reachable_evidence
                .iter()
                .map(|path| {
                    path.file_name()
                        .and_then(std::ffi::OsStr::to_str)
                        .map(str::to_owned)
                        .ok_or(EventRepositoryError::Integrity)
                })
                .collect::<Result<_, _>>()?,
            artifacts: state.artifacts.clone(),
            active_versions: state
                .active_versions
                .iter()
                .map(|(key, version)| {
                    (
                        key.clone(),
                        SealedActiveVersion {
                            number: version.number,
                            semantic_hash: version.semantic_hash.clone(),
                            sequence: version.sequence,
                            event_hash: version.event_hash.clone(),
                        },
                    )
                })
                .collect(),
            seen_idempotency: state.seen_idempotency.clone(),
            expected_markers: state.expected_markers.clone(),
            publications: Self::history_publications(state),
            streams,
        };
        let state_bytes = canonical_bytes(&sealed_state)?;
        ensure_inclusive_limit(state_bytes.len() as u64, MAX_SEAL_STATE_BYTES)?;
        let seal = SegmentSeal {
            format: SEAL_FORMAT.to_owned(),
            index,
            segment_bytes: length,
            segment_sha256: wire_sha256(&bytes),
            previous_seal_sha256: state
                .sealed
                .last()
                .map_or_else(|| GENESIS_HASH.to_owned(), |last| last.seal_sha256.clone()),
            state_sha256: wire_sha256(&state_bytes),
            next_sequence: state.next_sequence.clone(),
        };
        let seal_bytes = canonical_bytes(&seal)?;
        ensure_inclusive_limit(seal_bytes.len() as u64, MAX_SEAL_BYTES)?;
        drop(bytes);

        let directory = ensure_child_directory(&self.root_handle, &self.root, SEGMENTS_DIR)?;
        sync_directory_handle(&self.root_handle)?;
        self.publish_segment_file(
            &directory,
            &state_file_name(index, &seal.state_sha256),
            &state_bytes,
        )?;
        sync_directory_handle(&directory)?;
        self.publish_segment_file(&directory, &seal_file_name(index), &seal_bytes)?;
        sync_directory_handle(&directory)?;

        let next = open_active_segment(&self.root_handle, &self.root, index + 1)?;
        let identity = super::file_identity(&next)?;
        *journal = next;
        self.set_active_segment(identity, index + 1)?;
        drop(journal);
        *self
            .verified
            .lock()
            .map_err(|_| EventRepositoryError::StorageAt {
                site: "rotate:verified-mutex-poisoned",
                os: None,
            })? = None;
        Ok(())
    }

    /// Publishes `bytes` under `segments/<name>` through a synced temp file and a link, never by
    /// overwriting: an existing file must already hold exactly these bytes.
    fn publish_segment_file(
        &self,
        directory: &File,
        name: &str,
        bytes: &[u8],
    ) -> Result<(), EventRepositoryError> {
        let (temp_name, mut file) = self.create_unique_temp("segment")?;
        file.write_all(bytes)?;
        file.sync_all()?;
        let path = self.root.join(SEGMENTS_DIR);
        match link_retained_file_between(
            &file,
            &self.temp_handle,
            &self.root.join(".tmp"),
            &temp_name,
            directory,
            &path,
            name,
        ) {
            Ok(()) => {}
            Err(EventRepositoryError::IdempotencyConflict) => {
                let mut existing = open_child_file(directory, &path, name, false, false)?;
                if read_bounded_file(&mut existing, MAX_SEAL_STATE_BYTES)? != bytes {
                    return Err(EventRepositoryError::IntegrityAt(
                        "rotate:segment-file-conflict",
                    ));
                }
            }
            Err(error) => return Err(error),
        }
        remove_planned_file(&self.temp_handle, &self.root.join(".tmp"), &temp_name, file)?;
        sync_directory_handle(&self.temp_handle)
    }
}

/// The single-file 1.0.0 journal this store's segments add up to: `journal.jsonl`, then
/// `segments/1..n` in order, sealed and active alike. Segments are byte-identical slices of one
/// hash-chained line sequence, so this IS the lossless downgrade: written as `journal.jsonl`
/// beside the same `blobs/`, it opens as the unsegmented history. `events backup` archives it.
///
/// Callers that need a consistent snapshot hold the repository's lock across this call.
pub fn concatenated_journal(root: &Path) -> Result<Vec<u8>, EventRepositoryError> {
    let root_handle = super::open_directory(root)?;
    let mut journal = open_child_file(&root_handle, root, "journal.jsonl", false, false)?;
    let length = journal.metadata()?.len();
    ensure_inclusive_limit(length, MAX_JOURNAL_BYTES)?;
    let mut bytes = read_bounded_range(&mut journal, 0, length)?;
    let Some(directory) = open_segments_dir(&root_handle, root)? else {
        return Ok(bytes);
    };
    let entries = segment_entries(&directory, root)?;
    let path = root.join(SEGMENTS_DIR);
    let mut index = 1_u64;
    while entries.contains(&(index, 0)) {
        let mut segment =
            open_child_file(&directory, &path, &segment_file_name(index), false, false)?;
        let length = segment.metadata()?.len();
        ensure_inclusive_limit(length, MAX_JOURNAL_BYTES)?;
        bytes.extend(read_bounded_range(&mut segment, 0, length)?);
        index += 1;
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests;
