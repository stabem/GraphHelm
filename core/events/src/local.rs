use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

use fs2::FileExt;
use graphhelm_protocols::{
    ArtifactId, Clock, EventEnvelope, EventHash, EventKind, EvidenceId, IdGenerator, NewEvent,
    OpaqueId, PersistedTimestamp, RawSha256, RepositoryScope,
};
use serde::{Deserialize, Serialize};

use crate::canonical::{canonical_bytes, serialized_len_bounded, sha256_hex, wire_sha256};
use crate::jsonl::{BatchChecksum, PhysicalBatch, StoredArtifactRegistration};
use crate::limits::{
    MAX_BATCH_BYTES, MAX_CURSOR_BYTES, MAX_EVENT_BYTES, MAX_JOURNAL_BYTES, MAX_READ_ALL,
    MAX_SAFE_INTEGER,
};
use crate::repository::{
    ActiveVersion, EventPage, EventRepository, PreparedAppend, StreamHead, validate_page_limit,
};
use crate::{EventRepositoryError, SealedEvidence, WrappedKey, evidence::validate_sealed_metadata};

const FORMAT_BYTES: &[u8] = b"{\"formatVersion\":\"1.0.0\"}\n";
const FORMAT_VERSION: &str = "1.0.0";
const MAX_STORED_EVIDENCE_BYTES: u64 = 2 * (16 * 1024 * 1024 + 16) + 64 * 1024;
const MAX_REPOSITORY_ENTRIES: usize = 100_000;
const MAX_REPOSITORY_NAME_BYTES: usize = 16 * 1024 * 1024;
const MAX_REPOSITORY_METADATA_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_ROOT_ENTRIES: usize = 6;
const MAX_LOAD_WORK_UNITS: usize = 800_000;
const GENESIS_HASH: &str =
    "sha256:35c8ab0717bef1684ad07efcf3bedd4648c778a2c944cbd2c7e6a4802e2237b3";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LoadLimits {
    batches: usize,
    events: usize,
    evidence_refs: usize,
    evidence_registrations: usize,
    unique_evidence: usize,
    artifact_refs: usize,
    artifact_registrations: usize,
    unique_artifacts: usize,
    graph_publications: usize,
    work_units: usize,
}

const DEFAULT_LOAD_LIMITS: LoadLimits = LoadLimits {
    batches: MAX_READ_ALL,
    events: MAX_READ_ALL,
    evidence_refs: MAX_READ_ALL,
    evidence_registrations: MAX_READ_ALL,
    unique_evidence: MAX_READ_ALL,
    artifact_refs: MAX_READ_ALL,
    artifact_registrations: MAX_READ_ALL,
    unique_artifacts: MAX_READ_ALL,
    graph_publications: MAX_READ_ALL,
    work_units: MAX_LOAD_WORK_UNITS,
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct LoadBudget {
    limits: LoadLimits,
    batches: usize,
    events: usize,
    evidence_refs: usize,
    evidence_registrations: usize,
    unique_evidence: usize,
    artifact_refs: usize,
    artifact_registrations: usize,
    unique_artifacts: usize,
    graph_publications: usize,
    work_units: usize,
}

impl LoadBudget {
    const fn new(limits: LoadLimits) -> Self {
        Self {
            limits,
            batches: 0,
            events: 0,
            evidence_refs: 0,
            evidence_registrations: 0,
            unique_evidence: 0,
            artifact_refs: 0,
            artifact_registrations: 0,
            unique_artifacts: 0,
            graph_publications: 0,
            work_units: 0,
        }
    }

    fn add(current: &mut usize, amount: usize, limit: usize) -> Result<(), EventRepositoryError> {
        let next = current
            .checked_add(amount)
            .ok_or(EventRepositoryError::LimitExceeded)?;
        if next > limit {
            return Err(EventRepositoryError::LimitExceeded);
        }
        *current = next;
        Ok(())
    }

    fn work(&mut self, amount: usize) -> Result<(), EventRepositoryError> {
        Self::add(&mut self.work_units, amount, self.limits.work_units)
    }

    fn account_batch(&mut self, batches: usize, events: usize) -> Result<(), EventRepositoryError> {
        Self::add(&mut self.batches, batches, self.limits.batches)?;
        Self::add(&mut self.events, events, self.limits.events)?;
        self.work(
            batches
                .checked_add(events)
                .ok_or(EventRepositoryError::LimitExceeded)?,
        )
    }

    fn account_evidence_registrations(
        &mut self,
        amount: usize,
    ) -> Result<(), EventRepositoryError> {
        Self::add(
            &mut self.evidence_registrations,
            amount,
            self.limits.evidence_registrations,
        )?;
        self.work(amount)
    }

    fn account_evidence_refs(&mut self, amount: usize) -> Result<(), EventRepositoryError> {
        Self::add(&mut self.evidence_refs, amount, self.limits.evidence_refs)?;
        self.work(amount)
    }

    fn account_unique_evidence(&mut self) -> Result<(), EventRepositoryError> {
        Self::add(&mut self.unique_evidence, 1, self.limits.unique_evidence)?;
        self.work(1)
    }

    fn account_artifacts(
        &mut self,
        references: usize,
        registrations: usize,
    ) -> Result<(), EventRepositoryError> {
        Self::add(
            &mut self.artifact_refs,
            references,
            self.limits.artifact_refs,
        )?;
        Self::add(
            &mut self.artifact_registrations,
            registrations,
            self.limits.artifact_registrations,
        )?;
        self.work(
            references
                .checked_add(registrations)
                .ok_or(EventRepositoryError::LimitExceeded)?,
        )
    }

    fn account_unique_artifact(&mut self) -> Result<(), EventRepositoryError> {
        Self::add(&mut self.unique_artifacts, 1, self.limits.unique_artifacts)?;
        self.work(1)
    }

    fn account_graph_publication(&mut self) -> Result<(), EventRepositoryError> {
        Self::add(
            &mut self.graph_publications,
            1,
            self.limits.graph_publications,
        )?;
        self.work(1)
    }
}

impl Default for LoadLimits {
    fn default() -> Self {
        DEFAULT_LOAD_LIMITS
    }
}

struct DirectoryBudget {
    entries: usize,
    name_bytes: usize,
    max_entries: usize,
    max_name_bytes: usize,
}

impl DirectoryBudget {
    const fn with_limits(max_entries: usize, max_name_bytes: usize) -> Self {
        Self {
            entries: 0,
            name_bytes: 0,
            max_entries,
            max_name_bytes,
        }
    }

    fn account(&mut self, name: &std::ffi::OsStr) -> Result<(), EventRepositoryError> {
        let entries = self
            .entries
            .checked_add(1)
            .ok_or(EventRepositoryError::LimitExceeded)?;
        let name_bytes = self
            .name_bytes
            .checked_add(name.as_encoded_bytes().len())
            .ok_or(EventRepositoryError::LimitExceeded)?;
        if entries > self.max_entries || name_bytes > self.max_name_bytes {
            return Err(EventRepositoryError::LimitExceeded);
        }
        self.entries = entries;
        self.name_bytes = name_bytes;
        Ok(())
    }
}

/// Deterministic local crash points used by acceptance tests and host fault injection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalFailpoint {
    Validation,
    EvidenceStaging,
    BlobSync,
    BlobPublish,
    PhysicalBatchAppend,
    JournalSync,
    ActiveMarker,
}

impl LocalFailpoint {
    #[must_use]
    pub const fn all() -> [Self; 7] {
        [
            Self::Validation,
            Self::EvidenceStaging,
            Self::BlobSync,
            Self::BlobPublish,
            Self::PhysicalBatchAppend,
            Self::JournalSync,
            Self::ActiveMarker,
        ]
    }
}

/// Whether an operation needs the store to itself, or only needs no WRITER in the middle.
#[derive(Clone, Copy)]
enum Exclusivity {
    Exclusive,
    Shared,
}

/// Crash-consistent repository-v1 directory for offline/local execution.
#[derive(Clone)]
pub struct LocalEventRepository {
    root: PathBuf,
    root_handle: Arc<File>,
    root_identity: FileIdentity,
    blobs_handle: Arc<File>,
    blobs_identity: FileIdentity,
    temp_handle: Arc<File>,
    temp_identity: FileIdentity,
    active_handle: Arc<File>,
    active_identity: FileIdentity,
    journal: Arc<Mutex<File>>,
    journal_identity: FileIdentity,
    lock: Arc<Mutex<File>>,
    lock_identity: FileIdentity,
    operation_gate: Arc<Mutex<()>>,
    clock: Arc<dyn Clock>,
    ids: Arc<dyn IdGenerator>,
    temp_counter: Arc<AtomicU64>,
    /// The verified prefix (#87): everything a prior load PROVED about the journal's
    /// first `verified_offset` bytes, retained so the next load verifies only what is
    /// new. In-memory only — a crash discards it and the next open replays from zero,
    /// which is exactly today's behavior. Guarded by the same per-handle serialization
    /// (`operation_gate` + the journal mutex) every load already runs under; the
    /// CROSS-PROCESS story is unchanged because the check happens under the same
    /// per-operation file lock that today's full reload runs under, and every writer
    /// needs the exclusive lock — an append cannot land between this handle's check
    /// and its use.
    verified: Arc<Mutex<Option<VerifiedPrefix>>>,
    #[cfg(test)]
    load_count: Arc<AtomicU64>,
    #[cfg(test)]
    full_load_count: Arc<AtomicU64>,
    #[cfg(test)]
    suffix_load_count: Arc<AtomicU64>,
    /// Per-OPERATION-KIND load accounting (design section 2: the metric must name WHICH
    /// operation paid — one flat bucket made F2-H2 unfalsifiable). Keyed by the calling
    /// operation's name; value = (full, suffix, hit) counts.
    #[cfg(test)]
    loads_by_kind: Arc<Mutex<LoadsByKind>>,
    /// #143: counts opens that completed on the SHARED fast path (clean store, no
    /// recovery writes) — the observable the readers-wait-for-readers guard needs.
    #[cfg(test)]
    shared_fast_open_count: Arc<AtomicU64>,
    #[cfg(test)]
    journal_sync_count: Arc<AtomicU64>,
    failpoint: Option<LocalFailpoint>,
    schemas: &'static graphhelm_schema::RepositorySchemaSet,
}

impl LocalEventRepository {
    /// Recognizes an existing v1 repository without creating or reconciling it.
    pub fn inspect_format(root: &Path) -> Result<(), EventRepositoryError> {
        if !root.exists() {
            return Ok(());
        }
        reject_link_or_non_directory(root)?;
        let root_handle = open_directory(root)?;
        let mut format = open_child_file(&root_handle, root, "format.json", false, false)
            .map_err(|_| EventRepositoryError::UnsupportedFormat)?;
        if read_bounded_file(&mut format, 1024)? != FORMAT_BYTES {
            return Err(EventRepositoryError::UnsupportedFormat);
        }
        Ok(())
    }

    pub fn open(
        root: impl Into<PathBuf>,
        clock: Arc<dyn Clock>,
        ids: Arc<dyn IdGenerator>,
    ) -> Result<Self, EventRepositoryError> {
        Self::open_inner(root.into(), clock, ids, None)
    }

    pub fn open_with_failpoint(
        root: impl Into<PathBuf>,
        clock: Arc<dyn Clock>,
        ids: Arc<dyn IdGenerator>,
        failpoint: LocalFailpoint,
    ) -> Result<Self, EventRepositoryError> {
        Self::open_inner(root.into(), clock, ids, Some(failpoint))
    }

    fn open_inner(
        root: PathBuf,
        clock: Arc<dyn Clock>,
        ids: Arc<dyn IdGenerator>,
        failpoint: Option<LocalFailpoint>,
    ) -> Result<Self, EventRepositoryError> {
        let schemas = graphhelm_schema::repository_schema_set()
            .map_err(|_| EventRepositoryError::IntegrityAt("open:schema-set"))?;
        let root_handle = ensure_root_path(&root)?;
        let root_identity = file_identity(&root_handle)?;
        lock_root_exclusive(&root_handle)?;
        // #143: a COMPLETE layout first tries the SHARED fast path — eight concurrent
        // clean opens must not wait on each other (the readers-wait-for-readers guard).
        // Anything less than provably-clean falls back to the exclusive path below,
        // byte-for-byte today's recovery.
        let (lock, shared_open) = match initialize_root_shared_fast(&root, &root_handle) {
            Ok(Some(lock)) => (lock, true),
            Ok(None) => match initialize_root_locked(&root, &root_handle) {
                Ok(lock) => (lock, false),
                Err(error) => {
                    let _ = unlock_root(&root_handle);
                    return Err(error);
                }
            },
            Err(error) => {
                let _ = unlock_root(&root_handle);
                return Err(error);
            }
        };
        if file_identity(&open_directory(&root)?)? != root_identity {
            return Err(EventRepositoryError::IntegrityAt(
                "open:root-identity-after-lock",
            ));
        }
        let blobs_handle = open_child_directory(&root_handle, &root, "blobs")?;
        let blobs_identity = file_identity(&blobs_handle)?;
        let temp_handle = open_child_directory(&root_handle, &root, ".tmp")?;
        let temp_identity = file_identity(&temp_handle)?;
        let active_handle = open_child_directory(&root_handle, &root, "active")?;
        let active_identity = file_identity(&active_handle)?;
        let journal = open_child_file(&root_handle, &root, "journal.jsonl", true, false)?;
        if file_identity(&open_directory(&root)?)? != root_identity {
            return Err(EventRepositoryError::IntegrityAt(
                "open:root-identity-after-children",
            ));
        }
        let lock_identity = file_identity(&lock)?;
        let journal_identity = file_identity(&journal)?;
        let repository = Self {
            root,
            root_handle: Arc::new(root_handle),
            root_identity,
            blobs_handle: Arc::new(blobs_handle),
            blobs_identity,
            temp_handle: Arc::new(temp_handle),
            temp_identity,
            active_handle: Arc::new(active_handle),
            active_identity,
            journal: Arc::new(Mutex::new(journal)),
            journal_identity,
            lock: Arc::new(Mutex::new(lock)),
            lock_identity,
            operation_gate: Arc::new(Mutex::new(())),
            clock,
            ids,
            temp_counter: Arc::new(AtomicU64::new(0)),
            verified: Arc::new(Mutex::new(None)),
            #[cfg(test)]
            load_count: Arc::new(AtomicU64::new(0)),
            #[cfg(test)]
            full_load_count: Arc::new(AtomicU64::new(0)),
            #[cfg(test)]
            suffix_load_count: Arc::new(AtomicU64::new(0)),
            #[cfg(test)]
            loads_by_kind: Arc::new(Mutex::new(BTreeMap::new())),
            #[cfg(test)]
            shared_fast_open_count: Arc::new(AtomicU64::new(0)),
            #[cfg(test)]
            journal_sync_count: Arc::new(AtomicU64::new(0)),
            failpoint,
            schemas,
        };
        let recovery = (|| {
            repository.validate_anchors()?;
            let state = repository.load_state("open")?;
            let published = state
                .batches
                .iter()
                .flat_map(|batch| &batch.events)
                .filter(|event| matches!(event.kind, EventKind::GraphVersionPublished(_)))
                .cloned()
                .collect::<Vec<_>>();
            // The journal sync happens exactly ONCE per open, on whichever branch runs
            // (the resync-once contract a recovery guard pins): after the read-only
            // clean checks on the fast path, or inside the redo on the upgrade path,
            // or before recovery on the plain exclusive path — always before any
            // recovery write and always before open returns.
            if shared_open {
                // The clean checks are READ-ONLY mirrors of what recovery would write:
                // nothing to reconcile, nothing to republish -> nothing needs the
                // exclusive lock, and concurrent clean opens proceed in parallel.
                if repository.plan_reconcile(&state)?.is_noop()
                    && repository.active_markers_clean(&published)?
                {
                    repository.sync_loaded_journal(&state)?;
                    #[cfg(test)]
                    repository
                        .shared_fast_open_count
                        .fetch_add(1, Ordering::SeqCst);
                    return Ok(());
                }
                // UPGRADE: release shared, take exclusive, and REDO from scratch —
                // another process may have acted in the gap, so the cache is dropped
                // and the world re-read; correctness equals a fresh exclusive open.
                {
                    let lock = repository
                        .lock
                        .lock()
                        .map_err(|_| EventRepositoryError::Storage)?;
                    FileExt::unlock(&*lock).map_err(|_| EventRepositoryError::Storage)?;
                    lock.lock_exclusive()
                        .map_err(|_| EventRepositoryError::Storage)?;
                }
                // D's finding (a): the Complete verdict predates this lock — re-run
                // the same post-lock repair a fresh exclusive open would.
                repair_layout_locked(&repository.root, &repository.root_handle)?;
                *repository
                    .verified
                    .lock()
                    .map_err(|_| EventRepositoryError::Storage)? = None;
                repository.validate_anchors()?;
                let state = repository.load_state("open")?;
                repository.sync_loaded_journal(&state)?;
                let published = state
                    .batches
                    .iter()
                    .flat_map(|batch| &batch.events)
                    .filter(|event| matches!(event.kind, EventKind::GraphVersionPublished(_)))
                    .cloned()
                    .collect::<Vec<_>>();
                repository.reconcile_orphans(&state)?;
                return repository.publish_active_marker(&published, false);
            }
            repository.sync_loaded_journal(&state)?;
            repository.reconcile_orphans(&state)?;
            repository.publish_active_marker(&published, false)
        })();
        let named_unlock = {
            let lock = repository
                .lock
                .lock()
                .map_err(|_| EventRepositoryError::Storage)?;
            FileExt::unlock(&*lock).map_err(|_| EventRepositoryError::Storage)
        };
        let root_unlock = unlock_root(&repository.root_handle);
        match (recovery, named_unlock, root_unlock) {
            (Ok(()), Ok(()), Ok(())) => {}
            (Err(error), _, _) | (Ok(()), Err(error), _) | (Ok(()), Ok(()), Err(error)) => {
                return Err(error);
            }
        }
        Ok(repository)
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn append_atomic(
        &self,
        request: &PreparedAppend,
    ) -> Result<Vec<EventEnvelope>, EventRepositoryError> {
        <Self as EventRepository>::append_atomic(self, request)
    }

    /// The instant this repository would stamp on an append made right now.
    ///
    /// Exposed for the one caller that has to compare a caller-supplied instant against the
    /// instant an append WOULD carry — the sweep, which refuses to be asked about the future. It
    /// reads the same clock through the same conversion `build_envelopes` uses, deliberately: a
    /// second way of asking the time would be a second answer, and the comparison would then be
    /// against an instant no event ever gets.
    #[allow(clippy::missing_errors_doc)]
    pub fn now(&self) -> Result<PersistedTimestamp, EventRepositoryError> {
        PersistedTimestamp::from_datetime(self.clock.now()).map_err(|_| {
            // Unrepresentable is not "later" and not "earlier": refusing keeps a broken clock from
            // being read as permission.
            EventRepositoryError::Invalid
        })
    }

    pub fn read_stream(
        &self,
        scope: &RepositoryScope,
        stream_id: &str,
        limit: usize,
        cursor: Option<&str>,
    ) -> Result<EventPage, EventRepositoryError> {
        <Self as EventRepository>::read_stream(self, scope, stream_id, limit, cursor)
    }

    pub fn evidence_exists(
        &self,
        scope: &RepositoryScope,
        evidence_id: &EvidenceId,
    ) -> Result<bool, EventRepositoryError> {
        <Self as EventRepository>::evidence_exists(self, scope, evidence_id)
    }

    pub fn active_version(
        &self,
        scope: &RepositoryScope,
        stream_id: &str,
    ) -> Result<Option<ActiveVersion>, EventRepositoryError> {
        <Self as EventRepository>::active_version(self, scope, stream_id)
    }

    pub fn read_replay_stream(
        &self,
        scope: &RepositoryScope,
        stream_id: &str,
    ) -> Result<Vec<EventEnvelope>, EventRepositoryError> {
        <Self as EventRepository>::read_replay_stream(self, scope, stream_id)
    }

    pub fn next_sequence(
        &self,
        scope: &RepositoryScope,
        stream_id: &str,
    ) -> Result<u64, EventRepositoryError> {
        <Self as EventRepository>::next_sequence(self, scope, stream_id)
    }

    /// Atomically selects and reads only when the validated repository has one stream.
    pub fn read_unique_replay_stream(
        &self,
    ) -> Result<(crate::RepositoryStream, Vec<EventEnvelope>), EventRepositoryError> {
        self.with_shared_lock(|| {
            let state = self.load_state("read_unique_replay_stream")?;
            let mut streams = BTreeMap::new();
            for batch in &state.batches {
                streams.insert(
                    stream_key(&batch.scope, &batch.stream_id)?,
                    (batch.scope.clone(), batch.stream_id.clone()),
                );
            }
            if streams.len() != 1 {
                return Err(EventRepositoryError::StreamSelectionRequired);
            }
            let (_, (scope, stream_id)) = streams
                .into_iter()
                .next()
                .ok_or(EventRepositoryError::StreamSelectionRequired)?;
            let events = state
                .batches
                .iter()
                .filter(|batch| batch.scope == scope && batch.stream_id == stream_id)
                .flat_map(|batch| batch.events.iter().cloned())
                .collect::<Vec<_>>();
            if events.len() > MAX_READ_ALL {
                return Err(EventRepositoryError::LimitExceeded);
            }
            Ok((crate::RepositoryStream { scope, stream_id }, events))
        })
    }

    /// Every distinct (scope, stream) the repository holds, sorted — the read-only
    /// enumeration the 05f monitor's index renders as links. A read under the same
    /// exclusive lock every other read takes; nothing here can write.
    pub fn list_streams(&self) -> Result<Vec<crate::RepositoryStream>, EventRepositoryError> {
        self.with_shared_lock(|| {
            let state = self.load_state("list_streams")?;
            let mut streams = BTreeMap::new();
            for batch in &state.batches {
                streams.insert(
                    stream_key(&batch.scope, &batch.stream_id)?,
                    (batch.scope.clone(), batch.stream_id.clone()),
                );
            }
            Ok(streams
                .into_values()
                .map(|(scope, stream_id)| crate::RepositoryStream { scope, stream_id })
                .collect())
        })
    }

    /// Runs `operation` with the store held against WRITERS -- the append path.
    fn with_exclusive_lock<T>(
        &self,
        operation: impl FnOnce() -> Result<T, EventRepositoryError>,
    ) -> Result<T, EventRepositoryError> {
        self.with_lock(Exclusivity::Exclusive, operation)
    }

    /// Runs `operation` with the store held against WRITERS ONLY -- the read path.
    ///
    /// Every operation used to take the exclusive lock, reads included, so the API built to
    /// serve several agents at once served them one at a time. Measured before the change:
    /// eight concurrent reads of one store cost the same wall time as eight sequential ones.
    /// And reads are not a niche path -- in a mutation-heavy HTTP storm they were 89% of the
    /// store operations, because even a mutation reads the sequence, the stream, the active
    /// version and the idempotency record before appending once.
    ///
    /// Appends keep the exclusive lock. One writer at a time is the store's correctness model,
    /// not a defect to be optimised away; what was wrong was readers waiting on readers.
    fn with_shared_lock<T>(
        &self,
        operation: impl FnOnce() -> Result<T, EventRepositoryError>,
    ) -> Result<T, EventRepositoryError> {
        self.with_lock(Exclusivity::Shared, operation)
    }

    fn with_lock<T>(
        &self,
        exclusivity: Exclusivity,
        operation: impl FnOnce() -> Result<T, EventRepositoryError>,
    ) -> Result<T, EventRepositoryError> {
        // The in-process gate is per-INSTANCE, and every serve request opens its own instance,
        // so it never was the thing serialising concurrent requests -- the file lock was. It
        // still guards one instance shared across threads, and readers may share it.
        // The in-process gate stays a plain mutex, deliberately. It is per-INSTANCE, and serve
        // opens an instance per request, so it never was what serialised concurrent requests --
        // measured: making it an `RwLock` moved the cross-handle guard by nothing at all. What
        // DOES still serialise two threads sharing ONE handle is the journal mutex inside
        // `validate_anchors`, and no surface shares a handle today. Left as a seed rather than
        // changed without a guard that can fail.
        let _gate = self
            .operation_gate
            .lock()
            .map_err(|_| EventRepositoryError::Storage)?;
        lock_root_exclusive(&self.root_handle)?;
        let file = self
            .lock
            .lock()
            .map_err(|_| EventRepositoryError::Storage)
            .inspect_err(|_| {
                let _ = unlock_root(&self.root_handle);
            })?;
        let taken = match exclusivity {
            Exclusivity::Exclusive => file.lock_exclusive(),
            Exclusivity::Shared => FileExt::lock_shared(&*file),
        };
        if taken.is_err() {
            let _ = unlock_root(&self.root_handle);
            return Err(EventRepositoryError::Storage);
        }
        let result = self.validate_anchors().and_then(|()| operation());
        let result = match (result, self.validate_anchors()) {
            (Ok(value), Ok(())) => Ok(value),
            (_, Err(error)) => Err(error),
            (Err(error), Ok(())) => Err(error),
        };
        let named_unlock = FileExt::unlock(&*file).map_err(|_| EventRepositoryError::Storage);
        let root_unlock = unlock_root(&self.root_handle);
        match (result, named_unlock, root_unlock) {
            (Ok(value), Ok(()), Ok(())) => Ok(value),
            (Err(error), _, _) => Err(error),
            (Ok(_), Err(error), _) | (Ok(_), Ok(()), Err(error)) => Err(error),
        }
    }

    fn validate_anchors(&self) -> Result<(), EventRepositoryError> {
        reject_link_or_non_directory(&self.root)?;
        let root_path = open_directory(&self.root)?;
        let lock_path = open_child_file(
            &self.root_handle,
            &self.root,
            "repository.lock",
            true,
            false,
        )?;
        let blobs_path = open_child_directory(&self.root_handle, &self.root, "blobs")?;
        let temp_path = open_child_directory(&self.root_handle, &self.root, ".tmp")?;
        let active_path = open_child_directory(&self.root_handle, &self.root, "active")?;
        let journal_path =
            open_child_file(&self.root_handle, &self.root, "journal.jsonl", true, false)?;
        let root_path_identity = file_identity(&root_path)?;
        let lock_path_identity = file_identity(&lock_path)?;
        let journal_guard = self
            .journal
            .lock()
            .map_err(|_| EventRepositoryError::Storage)?;
        if root_path_identity != self.root_identity
            || file_identity(&self.root_handle)? != self.root_identity
            || lock_path_identity != self.lock_identity
            || file_identity(&blobs_path)? != self.blobs_identity
            || file_identity(&temp_path)? != self.temp_identity
            || file_identity(&active_path)? != self.active_identity
            || file_identity(&journal_path)? != self.journal_identity
            || file_identity(&self.blobs_handle)? != self.blobs_identity
            || file_identity(&self.temp_handle)? != self.temp_identity
            || file_identity(&self.active_handle)? != self.active_identity
            || file_identity(&journal_guard)? != self.journal_identity
        {
            return Err(EventRepositoryError::Integrity);
        }
        Ok(())
    }

    fn sync_loaded_journal(&self, state: &LoadedState) -> Result<(), EventRepositoryError> {
        if state.batches.is_empty() {
            return Ok(());
        }
        if self.failpoint == Some(LocalFailpoint::JournalSync) {
            return Err(EventRepositoryError::Storage);
        }
        let journal = self
            .journal
            .lock()
            .map_err(|_| EventRepositoryError::Storage)?;
        journal.sync_data()?;
        #[cfg(test)]
        self.journal_sync_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn append_locked(
        &self,
        request: &PreparedAppend,
    ) -> Result<Vec<EventEnvelope>, EventRepositoryError> {
        let state = self.load_state("append")?;
        self.sync_loaded_journal(&state)?;
        validate_request_preflight(request, &state)?;
        self.validate_reference_availability(request, &state)?;
        let request_digest = crate::prepared_append_digest(request)?;

        let requested_keys = request
            .events()
            .iter()
            .map(|event| event.idempotency_key.as_str())
            .collect::<BTreeSet<_>>();
        for batch in &state.batches {
            if batch.scope != *request.scope() || batch.stream_id != request.stream_id().as_str() {
                continue;
            }
            let batch_keys = batch
                .events
                .iter()
                .map(|event| event.idempotency_key.as_str())
                .collect::<BTreeSet<_>>();
            if !requested_keys.is_disjoint(&batch_keys) {
                if batch.request_digest == request_digest && batch_keys == requested_keys {
                    self.publish_active_marker(&batch.events, false)?;
                    return Ok(batch.events.clone());
                }
                return Err(EventRepositoryError::IdempotencyConflict);
            }
        }

        validate_graph_successors(request, &state)?;

        let actual = state
            .next_sequence
            .get(&stream_key(request.scope(), request.stream_id().as_str())?)
            .copied()
            .unwrap_or(1);
        if actual != request.expected_next_sequence() {
            return Err(EventRepositoryError::SequenceConflict);
        }

        let envelopes = self.build_envelopes(request, &state)?;
        let mut evidence_ids = request
            .evidence()
            .iter()
            .map(|item| item.reference().evidence_id().to_string())
            .collect::<Vec<_>>();
        evidence_ids.sort();
        let artifacts = request
            .artifacts()
            .iter()
            .map(|item| StoredArtifactRegistration {
                reference: item.reference().clone(),
                producer_stream_id: request.stream_id().to_string(),
                producer_idempotency_key: item.producer_idempotency_key().to_string(),
            })
            .collect::<Vec<_>>();
        let mut batch = PhysicalBatch {
            format_version: FORMAT_VERSION.into(),
            request_digest,
            scope: request.scope().clone(),
            stream_id: request.stream_id().to_string(),
            expected_next_sequence: request.expected_next_sequence(),
            checksum: String::new(),
            evidence_ids,
            artifacts,
            events: envelopes.clone(),
        };
        batch.checksum = batch_checksum(&batch)?;
        serialized_len_bounded(&batch, MAX_BATCH_BYTES)?;
        let mut line = canonical_bytes(&batch)?;
        line.push(b'\n');
        ensure_inclusive_limit(line.len() as u64, MAX_BATCH_BYTES as u64)?;
        if self.failpoint == Some(LocalFailpoint::Validation) {
            return Err(EventRepositoryError::Storage);
        }

        let mut staged = self.stage_evidence(request.evidence())?;
        if self.failpoint == Some(LocalFailpoint::EvidenceStaging) {
            return Err(EventRepositoryError::Storage);
        }
        self.sync_staged(&staged)?;
        if self.failpoint == Some(LocalFailpoint::BlobSync) {
            return Err(EventRepositoryError::Storage);
        }
        self.publish_staged(&mut staged)?;
        if self.failpoint == Some(LocalFailpoint::BlobPublish) {
            return Err(EventRepositoryError::Storage);
        }

        let mut journal = self
            .journal
            .lock()
            .map_err(|_| EventRepositoryError::Storage)?;
        let current_len = journal.metadata()?.len();
        let resulting_len = current_len
            .checked_add(
                u64::try_from(line.len()).map_err(|_| EventRepositoryError::LimitExceeded)?,
            )
            .ok_or(EventRepositoryError::LimitExceeded)?;
        if resulting_len > MAX_JOURNAL_BYTES {
            return Err(EventRepositoryError::LimitExceeded);
        }
        journal.seek(SeekFrom::End(0))?;
        if self.failpoint == Some(LocalFailpoint::PhysicalBatchAppend) {
            journal.write_all(&line[..line.len() / 2])?;
            journal.flush()?;
            return Err(EventRepositoryError::Storage);
        }
        journal.write_all(&line)?;
        journal.flush()?;
        if self.failpoint == Some(LocalFailpoint::JournalSync) {
            return Err(EventRepositoryError::Storage);
        }
        journal.sync_data()?;
        sync_directory_handle(&self.root_handle)?;
        if self.failpoint == Some(LocalFailpoint::ActiveMarker) {
            return Err(EventRepositoryError::Storage);
        }
        self.publish_active_marker(&envelopes, true)?;
        Ok(envelopes)
    }

    fn build_envelopes(
        &self,
        request: &PreparedAppend,
        state: &LoadedState,
    ) -> Result<Vec<EventEnvelope>, EventRepositoryError> {
        let key = stream_key(request.scope(), request.stream_id().as_str())?;
        let mut previous_hash = state
            .last_hash
            .get(&key)
            .cloned()
            .unwrap_or_else(|| GENESIS_HASH.into());
        let occurred_at = PersistedTimestamp::from_datetime(self.clock.now())
            .map_err(|_| EventRepositoryError::Invalid)?;
        let mut envelopes = Vec::with_capacity(request.events().len());
        for (offset, event) in request.events().iter().cloned().enumerate() {
            let sequence = request
                .expected_next_sequence()
                .checked_add(
                    u64::try_from(offset).map_err(|_| EventRepositoryError::LimitExceeded)?,
                )
                .ok_or(EventRepositoryError::LimitExceeded)?;
            if sequence > MAX_SAFE_INTEGER {
                return Err(EventRepositoryError::LimitExceeded);
            }
            let event_id = OpaqueId::parse(self.ids.next_id("event"))
                .map_err(|_| EventRepositoryError::Invalid)?;
            let previous = EventHash::parse(previous_hash.clone())
                .map_err(|_| EventRepositoryError::Integrity)?;
            let placeholder =
                EventHash::parse(GENESIS_HASH).map_err(|_| EventRepositoryError::Integrity)?;
            let mut envelope = EventEnvelope::new(
                event_id,
                request.scope().clone(),
                request.stream_id().clone(),
                sequence,
                occurred_at.clone(),
                event,
                previous,
                placeholder,
            );
            crate::validate_envelope_content(&envelope)?;
            let hash = crate::compute_event_hash(&envelope, &previous_hash)?;
            envelope.event_hash =
                EventHash::parse(hash.clone()).map_err(|_| EventRepositoryError::Integrity)?;
            crate::validate_envelope(&envelope)?;
            previous_hash = hash;
            envelopes.push(envelope);
        }
        Ok(envelopes)
    }

    fn validate_reference_availability(
        &self,
        request: &PreparedAppend,
        state: &LoadedState,
    ) -> Result<(), EventRepositoryError> {
        let prepared_evidence = request
            .evidence()
            .iter()
            .map(|item| (item.reference().evidence_id().as_str(), item.reference()))
            .collect::<BTreeMap<_, _>>();
        crate::validate_evidence_references(request.events(), &prepared_evidence, |reference| {
            let path = self.blob_path(request.scope(), reference.evidence_id())?;
            Ok(state.reachable_evidence.contains(&path)
                && self.verify_blob(&path, request.scope(), reference)?)
        })?;
        let prepared_artifacts = request
            .artifacts()
            .iter()
            .map(|item| (item.reference().artifact_id().as_str(), item))
            .collect::<BTreeMap<_, _>>();
        for event in request.events() {
            for reference in &event.artifact_refs {
                let prepared = prepared_artifacts
                    .get(reference.artifact_id().as_str())
                    .is_some_and(|item| item.reference() == reference);
                let committed = state
                    .artifacts
                    .get(&artifact_key(request.scope(), reference.artifact_id())?)
                    .is_some_and(|item| {
                        item.producer_stream_id == request.stream_id().as_str()
                            && &item.reference == reference
                    });
                if !prepared && !committed {
                    return Err(EventRepositoryError::Invalid);
                }
            }
        }
        Ok(())
    }

    fn stage_evidence(
        &self,
        evidence: &[SealedEvidence],
    ) -> Result<Vec<StagedBlob>, EventRepositoryError> {
        let mut staged = Vec::with_capacity(evidence.len());
        for item in evidence {
            let stored = StoredEvidence::from_sealed(item)?;
            let bytes = canonical_bytes(&stored)?;
            let final_name = self.blob_name(item.scope(), item.reference().evidence_id())?;
            let (temp_name, mut file) = self.create_unique_temp("blob")?;
            file.write_all(&bytes)?;
            staged.push(StagedBlob {
                temp_name,
                final_name,
                bytes,
                file: Some(file),
            });
        }
        Ok(staged)
    }

    fn sync_staged(&self, staged: &[StagedBlob]) -> Result<(), EventRepositoryError> {
        for item in staged {
            item.file
                .as_ref()
                .ok_or(EventRepositoryError::Integrity)?
                .sync_all()?;
        }
        sync_directory_handle(&self.temp_handle)
    }

    fn publish_staged(&self, staged: &mut [StagedBlob]) -> Result<(), EventRepositoryError> {
        for item in staged {
            let retained = item.file.as_ref().ok_or(EventRepositoryError::Integrity)?;
            match link_retained_file_between(
                retained,
                &self.temp_handle,
                &self.root.join(".tmp"),
                &item.temp_name,
                &self.blobs_handle,
                &self.root.join("blobs"),
                &item.final_name,
            ) {
                Ok(()) => {}
                Err(EventRepositoryError::IdempotencyConflict) => {
                    let mut existing = open_child_file(
                        &self.blobs_handle,
                        &self.root.join("blobs"),
                        &item.final_name,
                        false,
                        false,
                    )?;
                    let existing = read_bounded_file(&mut existing, MAX_STORED_EVIDENCE_BYTES)?;
                    if existing != item.bytes {
                        return Err(EventRepositoryError::IdempotencyConflict);
                    }
                }
                Err(error) => return Err(error),
            }
            let retained = item.file.take().ok_or(EventRepositoryError::Integrity)?;
            remove_planned_file(
                &self.temp_handle,
                &self.root.join(".tmp"),
                &item.temp_name,
                retained,
            )?;
        }
        sync_directory_handle(&self.blobs_handle)?;
        sync_directory_handle(&self.temp_handle)
    }

    fn publish_active_marker(
        &self,
        envelopes: &[EventEnvelope],
        fail_if_injected: bool,
    ) -> Result<(), EventRepositoryError> {
        if fail_if_injected && self.failpoint == Some(LocalFailpoint::ActiveMarker) {
            return Err(EventRepositoryError::Storage);
        }
        let mut marker_indexes = BTreeMap::<String, BTreeMap<String, String>>::new();
        let mut marker_budget =
            DirectoryBudget::with_limits(MAX_REPOSITORY_ENTRIES, MAX_REPOSITORY_NAME_BYTES);
        for envelope in envelopes {
            let EventKind::GraphVersionPublished(payload) = &envelope.kind else {
                continue;
            };
            let stream_name = object_key(&envelope.scope, envelope.stream_id.as_str())?;
            let directory = self.root.join("active").join(&stream_name);
            let directory_handle = ensure_child_directory(
                &self.active_handle,
                &self.root.join("active"),
                &stream_name,
            )?;
            let marker = StoredActiveMarker {
                format_version: FORMAT_VERSION.into(),
                scope: envelope.scope.clone(),
                stream_id: envelope.stream_id.to_string(),
                number: payload.version.number(),
                semantic_hash: payload.version.semantic_hash().to_string(),
                sequence: envelope.sequence,
                event_hash: envelope.event_hash.to_string(),
            };
            let bytes = canonical_bytes(&marker)?;
            let marker_name = format!("{}.json", envelope.sequence);
            if !marker_indexes.contains_key(&stream_name) {
                marker_indexes.insert(
                    stream_name.clone(),
                    derived_marker_index(&directory_handle, &directory, &mut marker_budget)?,
                );
            }
            let marker_digest = sha256_hex(&bytes);
            let indexed_match = marker_indexes
                .get(&stream_name)
                .and_then(|index| index.get(&marker_digest))
                .is_some_and(|name| {
                    derived_marker_matches(&directory_handle, &directory, name, &bytes)
                });
            if derived_marker_matches(&directory_handle, &directory, &marker_name, &bytes)
                || indexed_match
            {
                continue;
            }
            let (temp_name, mut file) = self.create_unique_temp("active")?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            let mut destination_name = marker_name;
            match link_retained_file_between(
                &file,
                &self.temp_handle,
                &self.root.join(".tmp"),
                &temp_name,
                &directory_handle,
                &directory,
                &destination_name,
            ) {
                Ok(()) => {}
                Err(EventRepositoryError::IdempotencyConflict) => {
                    if !derived_marker_matches(
                        &directory_handle,
                        &directory,
                        &destination_name,
                        &bytes,
                    ) {
                        let mut published = false;
                        for _ in 0..16_u8 {
                            let entropy = self.ids.next_id("active-marker-repair");
                            destination_name = format!(
                                "repair-{}.json",
                                sha256_hex(
                                    format!(
                                        "graphhelm-active-repair-v2\0{marker_digest}\0{entropy}"
                                    )
                                    .as_bytes()
                                )
                            );
                            match link_retained_file_between(
                                &file,
                                &self.temp_handle,
                                &self.root.join(".tmp"),
                                &temp_name,
                                &directory_handle,
                                &directory,
                                &destination_name,
                            ) {
                                Ok(()) => {
                                    published = true;
                                    break;
                                }
                                Err(EventRepositoryError::IdempotencyConflict) => {
                                    if derived_marker_matches(
                                        &directory_handle,
                                        &directory,
                                        &destination_name,
                                        &bytes,
                                    ) {
                                        published = true;
                                        break;
                                    }
                                }
                                Err(error) => return Err(error),
                            }
                        }
                        if !published {
                            return Err(EventRepositoryError::IdempotencyConflict);
                        }
                    }
                }
                Err(error) => return Err(error),
            }
            marker_indexes
                .get_mut(&stream_name)
                .ok_or(EventRepositoryError::Integrity)?
                .insert(marker_digest, destination_name.clone());
            remove_planned_file(&self.temp_handle, &self.root.join(".tmp"), &temp_name, file)?;
            sync_directory_handle(&directory_handle)?;
        }
        Ok(())
    }

    /// Read-only mirror (#143) of `publish_active_marker`'s match check: true when every
    /// published version's marker already exists with the exact canonical bytes (directly
    /// or under a repair name). A miss answers false; the exclusive path republishes.
    fn active_markers_clean(
        &self,
        envelopes: &[EventEnvelope],
    ) -> Result<bool, EventRepositoryError> {
        let mut budget =
            DirectoryBudget::with_limits(MAX_REPOSITORY_ENTRIES, MAX_REPOSITORY_NAME_BYTES);
        for envelope in envelopes {
            let EventKind::GraphVersionPublished(payload) = &envelope.kind else {
                continue;
            };
            let stream_name = object_key(&envelope.scope, envelope.stream_id.as_str())?;
            let directory = self.root.join("active").join(&stream_name);
            let Ok(directory_handle) =
                open_child_directory(&self.active_handle, &self.root.join("active"), &stream_name)
            else {
                return Ok(false);
            };
            let marker = StoredActiveMarker {
                format_version: FORMAT_VERSION.into(),
                scope: envelope.scope.clone(),
                stream_id: envelope.stream_id.to_string(),
                number: payload.version.number(),
                semantic_hash: payload.version.semantic_hash().to_string(),
                sequence: envelope.sequence,
                event_hash: envelope.event_hash.to_string(),
            };
            let bytes = canonical_bytes(&marker)?;
            let marker_name = format!("{}.json", envelope.sequence);
            if derived_marker_matches(&directory_handle, &directory, &marker_name, &bytes) {
                continue;
            }
            let digest = sha256_hex(&bytes);
            let index = derived_marker_index(&directory_handle, &directory, &mut budget)?;
            let indexed = index.get(&digest).is_some_and(|name| {
                derived_marker_matches(&directory_handle, &directory, name, &bytes)
            });
            if !indexed {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn create_unique_temp(&self, prefix: &str) -> Result<(String, File), EventRepositoryError> {
        for _ in 0..=MAX_REPOSITORY_ENTRIES {
            let counter = self.temp_counter.fetch_add(1, Ordering::SeqCst);
            let material = format!("{}:{counter}", self.ids.next_id("repository-temp"));
            let name = format!("{prefix}-{}.tmp", sha256_hex(material.as_bytes()));
            match create_temp_child(&self.temp_handle, &self.root.join(".tmp"), &name) {
                Ok(file) => return Ok((name, file)),
                Err(EventRepositoryError::IdempotencyConflict) => continue,
                Err(error) => return Err(error),
            }
        }
        Err(EventRepositoryError::Storage)
    }

    /// Loads the journal's state, verifying ONLY what this handle has not already
    /// proven (#87). The journal is append-only, so the cached verified prefix stays
    /// true as long as the file's identity is unchanged and its length has not shrunk;
    /// a longer file means new lines, which are verified chaining from the CACHED
    /// per-stream hashes and sequence heads — every byte is still verified exactly once
    /// per handle, from genesis. A shorter file or an unknown cache means a full reload
    /// from zero, byte-for-byte the pre-#87 behavior (a line-boundary truncation
    /// reloads as a valid shorter history, exactly as it always has — hardening that is
    /// a separate, default-OFF decision). The check-to-use window is closed by the
    /// LOCK, not by timing: this runs under the same per-operation file lock the full
    /// reload always ran under, and every writer needs the exclusive lock.
    fn load_state(&self, kind: &'static str) -> Result<Arc<LoadedState>, EventRepositoryError> {
        // `kind` names WHICH operation is paying for this load (design section 2: never
        // one flat bucket). It feeds the cfg(test) per-kind accounting only; production
        // pays one &'static str argument.
        let _ = kind;
        #[cfg(test)]
        self.load_count.fetch_add(1, Ordering::SeqCst);
        let mut journal = self
            .journal
            .lock()
            .map_err(|_| EventRepositoryError::Storage)?;
        let length = journal
            .metadata()
            .map_err(|_| EventRepositoryError::Storage)?
            .len();
        ensure_inclusive_limit(length, MAX_JOURNAL_BYTES)?;
        let mut verified = self
            .verified
            .lock()
            .map_err(|_| EventRepositoryError::Storage)?;
        // The prefix is TAKEN out while working: if suffix verification fails partway,
        // a half-updated context must not survive as "verified" — the next load runs
        // the full path and surfaces the same error the full path always surfaced.
        //
        // The reuse test below catches replacement (identity) and truncation below the
        // offset (length); it CANNOT catch an in-place rewrite of already-verified
        // bytes, and the lock is no answer there — the per-line verification exists for
        // writers that never take the lock (corruption, rogue processes, bad hardware).
        // That exposure is bounded by HANDLE LIFETIME, which is milliseconds under
        // per-operation opens. NAMED TRIGGER (#87): when handles become long-lived (the
        // serve commit), this trade-off must be revisited or the reuse bounded
        // (full re-verify every N loads or on a time bound).
        //
        // SECOND consequence on the SAME trigger (D's CONV-1 report): the `verified`
        // mutex below is held across the suffix READ and verification, so the critical
        // section is O(suffix bytes), not constant. Uncontended by construction while
        // handles are per-operation; on a long-lived SHARED handle, a handle that fell
        // far behind pays a long read with the mutex held and every sibling operation
        // waits — a CONTENTION exposure distinct from the staleness one above, arriving
        // at the same commit.
        let reusable = verified.take().filter(|prefix| {
            prefix.journal_identity == self.journal_identity && prefix.verified_offset <= length
        });
        let (mut ctx, offset) = match reusable {
            Some(prefix) if prefix.verified_offset == length => {
                let state = prefix.state.clone();
                *verified = Some(prefix);
                #[cfg(test)]
                self.record_load_kind(kind, LoadPath::Hit);
                return Ok(state);
            }
            Some(prefix) => {
                #[cfg(test)]
                self.suffix_load_count.fetch_add(1, Ordering::SeqCst);
                #[cfg(test)]
                self.record_load_kind(kind, LoadPath::Suffix);
                let offset = prefix.verified_offset;
                (VerifyCtx::from_prefix(prefix), offset)
            }
            None => {
                #[cfg(test)]
                self.full_load_count.fetch_add(1, Ordering::SeqCst);
                #[cfg(test)]
                self.record_load_kind(kind, LoadPath::Full);
                (VerifyCtx::fresh(), 0)
            }
        };
        let bytes = read_bounded_range(&mut journal, offset, length)?;
        if !bytes.is_empty() && bytes.last() != Some(&b'\n') {
            return Err(EventRepositoryError::IntegrityAt(
                "load_state:journal-tail-unterminated",
            ));
        }
        self.verify_lines(&bytes, &mut ctx)?;
        let state = ctx.state.clone();
        *verified = Some(ctx.into_prefix(self.journal_identity, length));
        Ok(state)
    }

    /// Records which operation paid for a load, and through which path (design section
    /// 2: the metric names the payer, never one flat bucket).
    #[cfg(test)]
    fn record_load_kind(&self, kind: &'static str, path: LoadPath) {
        if let Ok(mut by_kind) = self.loads_by_kind.lock() {
            let entry = by_kind.entry(kind).or_insert((0, 0, 0));
            match path {
                LoadPath::Full => entry.0 += 1,
                LoadPath::Suffix => entry.1 += 1,
                LoadPath::Hit => entry.2 += 1,
            }
        }
    }

    /// Verifies `bytes` (whole lines, ending on a newline) into `ctx`, exactly as the
    /// monolithic load always did — this IS that loop, extracted so a verified prefix
    /// can resume it mid-journal. Chain heads, sequence heads, budgets and uniqueness
    /// sets all come from `ctx`, so a suffix is judged against the cached prefix with
    /// the same blades a full load judges it against genesis.
    fn verify_lines(&self, bytes: &[u8], ctx: &mut VerifyCtx) -> Result<(), EventRepositoryError> {
        let VerifyCtx {
            state: shared_state,
            budget: load_budget,
            counted_evidence,
            counted_artifacts,
            verified_evidence,
            verified_evidence_metadata_bytes,
        } = ctx;
        // Callers drop their Arc at the end of their operation, so this is normally a
        // refcount-1 in-place mutation; a retained reference costs one clone, never
        // correctness.
        //
        // An auditor looking for the chain head and sequence head in VerifyCtx will not
        // find them by name: `expected` and `previous_hash` are per-batch locals SEEDED
        // from `state.next_sequence`/`state.last_hash` and written back at the end of
        // each batch — they survive a splice through `state`, which is why they need no
        // ctx field.
        let state = Arc::make_mut(shared_state);
        for line in bytes
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
        {
            ensure_inclusive_limit(line.len() as u64, MAX_BATCH_BYTES as u64)?;
            let batch = parse_physical_batch(self.schemas, line)?;
            if batch.checksum != batch_checksum(&batch)? {
                // The batch failed its own checksum. Reporting this as a generic integrity failure
                // left an operator unable to distinguish a corrupt stored line from a broken event
                // hash chain, which have different recovery paths.
                return Err(EventRepositoryError::CorruptBatch);
            }
            if canonical_bytes(&batch)? != line || batch.format_version != FORMAT_VERSION {
                return Err(EventRepositoryError::Integrity);
            }
            if batch.events.is_empty()
                || batch.events.first().map(|event| event.sequence)
                    != Some(batch.expected_next_sequence)
            {
                return Err(EventRepositoryError::Integrity);
            }
            load_budget.account_batch(1, batch.events.len())?;
            load_budget.account_evidence_registrations(batch.evidence_ids.len())?;
            let artifact_references = batch.events.iter().try_fold(0_usize, |total, event| {
                total
                    .checked_add(event.artifact_refs.len())
                    .ok_or(EventRepositoryError::LimitExceeded)
            })?;
            load_budget.account_artifacts(artifact_references, batch.artifacts.len())?;
            let evidence_references = batch.events.iter().try_fold(0_usize, |total, event| {
                total
                    .checked_add(event.evidence_refs.len())
                    .ok_or(EventRepositoryError::LimitExceeded)
            })?;
            load_budget.account_evidence_refs(evidence_references)?;
            for event in &batch.events {
                if matches!(event.kind, EventKind::GraphVersionPublished(_)) {
                    load_budget.account_graph_publication()?;
                }
            }
            let key = stream_key(&batch.scope, &batch.stream_id)?;
            let expected = state.next_sequence.get(&key).copied().unwrap_or(1);
            if expected != batch.expected_next_sequence {
                return Err(EventRepositoryError::Integrity);
            }
            let mut previous_hash = state
                .last_hash
                .get(&key)
                .cloned()
                .unwrap_or_else(|| GENESIS_HASH.into());
            let batch_artifacts = batch
                .artifacts
                .iter()
                .map(|item| (item.reference.artifact_id().as_str(), item))
                .collect::<BTreeMap<_, _>>();
            let producer_artifacts = batch
                .events
                .iter()
                .flat_map(|event| {
                    event.artifact_refs.iter().map(move |reference| {
                        (
                            (
                                event.idempotency_key.as_str(),
                                reference.artifact_id().as_str(),
                            ),
                            reference,
                        )
                    })
                })
                .collect::<BTreeMap<_, _>>();
            let producer_artifact_count =
                batch.events.iter().try_fold(0_usize, |total, event| {
                    total
                        .checked_add(event.artifact_refs.len())
                        .ok_or(EventRepositoryError::LimitExceeded)
                })?;
            if batch_artifacts.len() != batch.artifacts.len()
                || producer_artifacts.len() != producer_artifact_count
                || batch.artifacts.iter().any(|item| {
                    item.producer_stream_id != batch.stream_id
                        || producer_artifacts.get(&(
                            item.producer_idempotency_key.as_str(),
                            item.reference.artifact_id().as_str(),
                        )) != Some(&&item.reference)
                })
            {
                return Err(EventRepositoryError::Integrity);
            }
            for item in &batch.artifacts {
                let identity = artifact_key(&batch.scope, item.reference.artifact_id())?;
                if state
                    .artifacts
                    .get(&identity)
                    .is_some_and(|existing| existing != item)
                {
                    return Err(EventRepositoryError::Integrity);
                }
            }
            let mut batch_evidence_refs = BTreeMap::new();
            for reference in batch.events.iter().flat_map(|event| &event.evidence_refs) {
                match batch_evidence_refs.insert(reference.evidence_id().as_str(), reference) {
                    Some(existing) if existing != reference => {
                        return Err(EventRepositoryError::Integrity);
                    }
                    _ => {}
                }
            }
            let declared_evidence = batch.evidence_ids.iter().collect::<BTreeSet<_>>();
            if declared_evidence.len() != batch.evidence_ids.len()
                || batch
                    .evidence_ids
                    .iter()
                    .any(|id| !batch_evidence_refs.contains_key(id.as_str()))
            {
                return Err(EventRepositoryError::Integrity);
            }
            let mut evidence_digests = Vec::with_capacity(batch.evidence_ids.len());
            for id in &batch.evidence_ids {
                let reference = batch_evidence_refs
                    .get(id.as_str())
                    .copied()
                    .ok_or(EventRepositoryError::Integrity)?;
                let identity = object_key(&batch.scope, reference.evidence_id().as_str())?;
                if !counted_evidence.contains(&identity) {
                    load_budget.account_unique_evidence()?;
                    counted_evidence.insert(identity.clone());
                }
                let digest = if let Some(existing) = verified_evidence.get(&identity) {
                    if existing.scope != batch.scope || &existing.reference != reference {
                        return Err(EventRepositoryError::Integrity);
                    }
                    existing.clone()
                } else {
                    let path = self.blob_path(&batch.scope, reference.evidence_id())?;
                    let sealed = self.read_verified_blob(&path, &batch.scope, reference)?;
                    let digest = evidence_digest(&sealed);
                    *verified_evidence_metadata_bytes = verified_evidence_metadata_bytes
                        .checked_add(
                            u64::try_from(serialized_len_bounded(&digest, MAX_EVENT_BYTES)?)
                                .map_err(|_| EventRepositoryError::LimitExceeded)?,
                        )
                        .ok_or(EventRepositoryError::LimitExceeded)?;
                    ensure_inclusive_limit(
                        *verified_evidence_metadata_bytes,
                        MAX_REPOSITORY_METADATA_BYTES,
                    )?;
                    verified_evidence.insert(identity, digest.clone());
                    digest
                };
                evidence_digests.push(digest);
            }
            let new_events = batch
                .events
                .iter()
                .map(|event| {
                    NewEvent::new(
                        event.idempotency_key.clone(),
                        event.actor.clone(),
                        event.sensitivity,
                        event.kind.clone(),
                        event.evidence_refs.clone(),
                        event.artifact_refs.clone(),
                    )
                })
                .collect::<Vec<_>>();
            if request_digest_from_evidence_digests(
                &batch.scope,
                &OpaqueId::parse(batch.stream_id.clone())
                    .map_err(|_| EventRepositoryError::Integrity)?,
                batch.expected_next_sequence,
                &new_events,
                evidence_digests,
                batch.artifacts.clone(),
            )? != batch.request_digest
            {
                return Err(EventRepositoryError::Integrity);
            }
            for (offset, event) in batch.events.iter().enumerate() {
                crate::validate_envelope(event).map_err(|_| EventRepositoryError::Integrity)?;
                if let EventKind::GraphVersionPublished(payload) = &event.kind {
                    graphhelm_graph::validate_persisted_projection(&payload.version)
                        .map_err(|_| EventRepositoryError::Integrity)?;
                    match state.active_versions.get(&key) {
                        Some(active)
                            if !crate::is_graph_successor(
                                &payload.version,
                                active.number,
                                &active.semantic_hash,
                            ) =>
                        {
                            return Err(EventRepositoryError::Integrity);
                        }
                        None if payload.version.number() != 1
                            || payload.version.predecessor().is_some() =>
                        {
                            return Err(EventRepositoryError::Integrity);
                        }
                        _ => {}
                    }
                    state.active_versions.insert(
                        key.clone(),
                        ActiveVersion {
                            number: payload.version.number(),
                            semantic_hash: payload.version.semantic_hash().to_string(),
                            sequence: event.sequence,
                            event_hash: event.event_hash.to_string(),
                        },
                    );
                    let marker = StoredActiveMarker {
                        format_version: FORMAT_VERSION.into(),
                        scope: event.scope.clone(),
                        stream_id: event.stream_id.to_string(),
                        number: payload.version.number(),
                        semantic_hash: payload.version.semantic_hash().to_string(),
                        sequence: event.sequence,
                        event_hash: event.event_hash.to_string(),
                    };
                    state.expected_markers.insert(
                        active_marker_key(&event.scope, event.stream_id.as_str(), event.sequence)?,
                        marker,
                    );
                }
                if event.scope != batch.scope
                    || event.stream_id.as_str() != batch.stream_id
                    || event.sequence
                        != expected
                            .checked_add(
                                u64::try_from(offset)
                                    .map_err(|_| EventRepositoryError::Integrity)?,
                            )
                            .ok_or(EventRepositoryError::Integrity)?
                    || event.previous_hash.as_str() != previous_hash
                    || crate::compute_event_hash(event, &previous_hash)?
                        != event.event_hash.as_str()
                {
                    return Err(EventRepositoryError::Integrity);
                }
                let idempotency_identity = format!("{key}:{}", event.idempotency_key);
                if !state.seen_idempotency.insert(idempotency_identity) {
                    return Err(EventRepositoryError::Integrity);
                }
                if state.seen_idempotency.len() > MAX_READ_ALL {
                    return Err(EventRepositoryError::LimitExceeded);
                }
                previous_hash = event.event_hash.to_string();
                for reference in &event.evidence_refs {
                    let identity = object_key(&event.scope, reference.evidence_id().as_str())?;
                    if !counted_evidence.contains(&identity) {
                        load_budget.account_unique_evidence()?;
                        counted_evidence.insert(identity.clone());
                    }
                    let path = self.blob_path(&event.scope, reference.evidence_id())?;
                    if verified_evidence.get(&identity).is_none_or(|digest| {
                        digest.scope != event.scope || digest.reference != *reference
                    }) {
                        return Err(EventRepositoryError::Integrity);
                    }
                    state.reachable_evidence.insert(path);
                }
                for artifact in &event.artifact_refs {
                    let identity = artifact_key(&event.scope, artifact.artifact_id())?;
                    if !counted_artifacts.contains(&identity) {
                        load_budget.account_unique_artifact()?;
                        counted_artifacts.insert(identity.clone());
                    }
                    let existing = state.artifacts.get(&identity);
                    let registered = batch_artifacts
                        .get(artifact.artifact_id().as_str())
                        .filter(|item| item.producer_stream_id == event.stream_id.as_str())
                        .map(|item| &item.reference);
                    if existing.is_none_or(|item| {
                        item.producer_stream_id != event.stream_id.as_str()
                            || &item.reference != artifact
                    }) && registered != Some(artifact)
                    {
                        return Err(EventRepositoryError::Integrity);
                    }
                }
            }
            let batch_event_count =
                u64::try_from(batch.events.len()).map_err(|_| EventRepositoryError::Integrity)?;
            state.next_sequence.insert(
                key.clone(),
                expected
                    .checked_add(batch_event_count)
                    .ok_or(EventRepositoryError::Integrity)?,
            );
            state.last_hash.insert(key, previous_hash);
            for artifact in &batch.artifacts {
                let identity = artifact_key(&batch.scope, artifact.reference.artifact_id())?;
                if !counted_artifacts.contains(&identity) {
                    load_budget.account_unique_artifact()?;
                    counted_artifacts.insert(identity.clone());
                }
                match state.artifacts.get(&identity) {
                    Some(existing) if existing != artifact => {
                        return Err(EventRepositoryError::Integrity);
                    }
                    Some(_) => {}
                    None => {
                        state.artifacts.insert(identity, artifact.clone());
                    }
                }
            }
            state.batches.push(batch);
        }
        Ok(())
    }

    fn verify_blob(
        &self,
        path: &Path,
        scope: &RepositoryScope,
        reference: &graphhelm_protocols::EvidenceReference,
    ) -> Result<bool, EventRepositoryError> {
        let name = path
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .ok_or(EventRepositoryError::UnsupportedFormat)?;
        let mut file = open_child_file(
            &self.blobs_handle,
            &self.root.join("blobs"),
            name,
            false,
            false,
        )?;
        let bytes = read_bounded_file(&mut file, MAX_STORED_EVIDENCE_BYTES)?;
        let stored: StoredEvidence =
            serde_json::from_slice(&bytes).map_err(|_| EventRepositoryError::Integrity)?;
        if canonical_bytes(&stored)? != bytes {
            return Err(EventRepositoryError::Integrity);
        }
        stored.verify(scope, reference)
    }

    fn read_verified_blob(
        &self,
        path: &Path,
        scope: &RepositoryScope,
        reference: &graphhelm_protocols::EvidenceReference,
    ) -> Result<SealedEvidence, EventRepositoryError> {
        let name = path
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .ok_or(EventRepositoryError::UnsupportedFormat)?;
        let mut file = open_child_file(
            &self.blobs_handle,
            &self.root.join("blobs"),
            name,
            false,
            false,
        )?;
        let bytes = read_bounded_file(&mut file, MAX_STORED_EVIDENCE_BYTES)?;
        let stored: StoredEvidence =
            serde_json::from_slice(&bytes).map_err(|_| EventRepositoryError::Integrity)?;
        if canonical_bytes(&stored)? != bytes {
            return Err(EventRepositoryError::Integrity);
        }
        if stored.format_version != FORMAT_VERSION
            || &stored.scope != scope
            || &stored.reference != reference
        {
            return Err(EventRepositoryError::Integrity);
        }
        stored.to_sealed()
    }

    /// Recovery, unchanged in behavior (#143 split it in two so the shared fast path
    /// can ask the REAL planner whether there is anything to do, instead of mirroring
    /// its rules and drifting): plan (read-only scans, refusals included), then apply.
    fn reconcile_orphans(&self, state: &LoadedState) -> Result<(), EventRepositoryError> {
        let plan = self.plan_reconcile(state)?;
        self.apply_reconcile(plan)
    }

    /// The scan half: every rule reconcile enforces runs here — foreign names refuse,
    /// orphans and stale temps are PLANNED for deletion, the active tree is validated —
    /// and nothing is written. `is_noop` on the result is the fast path's clean check.
    fn plan_reconcile(&self, state: &LoadedState) -> Result<ReconcilePlan, EventRepositoryError> {
        let mut directory_budget =
            DirectoryBudget::with_limits(MAX_REPOSITORY_ENTRIES, MAX_REPOSITORY_NAME_BYTES);
        let mut delete_blobs = Vec::new();
        let mut metadata_bytes = 0_u64;
        for_each_child_name(
            &self.blobs_handle,
            &self.root.join("blobs"),
            &mut directory_budget,
            |name, _| {
                let path = self.root.join("blobs").join(name);
                if !is_digest_json_name(name) {
                    return Err(EventRepositoryError::UnsupportedFormat);
                }
                let mut file =
                    open_child_file_for_delete(&self.blobs_handle, &self.root.join("blobs"), name)?;
                metadata_bytes = metadata_bytes
                    .checked_add(file.metadata()?.len())
                    .ok_or(EventRepositoryError::LimitExceeded)?;
                ensure_inclusive_limit(metadata_bytes, MAX_REPOSITORY_METADATA_BYTES)?;
                if !state.reachable_evidence.contains(&path) {
                    let bytes = read_bounded_file(&mut file, MAX_STORED_EVIDENCE_BYTES)?;
                    let stored: StoredEvidence = serde_json::from_slice(&bytes)
                        .map_err(|_| EventRepositoryError::UnsupportedFormat)?;
                    let expected_name = format!(
                        "{}.json",
                        object_key(&stored.scope, stored.reference.evidence_id().as_str())?
                    );
                    if stored.format_version != FORMAT_VERSION
                        || name != expected_name
                        || canonical_bytes(&stored)? != bytes
                        || !stored.verify(&stored.scope, &stored.reference)?
                    {
                        return Err(EventRepositoryError::UnsupportedFormat);
                    }
                    delete_blobs.push(PlannedDelete {
                        name: name.to_owned(),
                        file,
                    });
                }
                Ok(())
            },
        )?;
        let mut delete_temps = Vec::new();
        for_each_child_name(
            &self.temp_handle,
            &self.root.join(".tmp"),
            &mut directory_budget,
            |name, _| {
                if !is_owned_temp_name(name) {
                    return Err(EventRepositoryError::UnsupportedFormat);
                }
                let file =
                    open_child_file_for_delete(&self.temp_handle, &self.root.join(".tmp"), name)?;
                metadata_bytes = metadata_bytes
                    .checked_add(file.metadata()?.len())
                    .ok_or(EventRepositoryError::LimitExceeded)?;
                ensure_inclusive_limit(metadata_bytes, MAX_REPOSITORY_METADATA_BYTES)?;
                delete_temps.push(PlannedDelete {
                    name: name.to_owned(),
                    file,
                });
                Ok(())
            },
        )?;
        for_each_child_name(
            &self.active_handle,
            &self.root.join("active"),
            &mut directory_budget,
            |stream_name, directory_budget| {
                let stream_path = self.root.join("active").join(stream_name);
                if stream_name.len() != 64
                    || !stream_name
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                {
                    return Err(EventRepositoryError::UnsupportedFormat);
                }
                let stream_handle = open_child_directory(
                    &self.active_handle,
                    &self.root.join("active"),
                    stream_name,
                )?;
                for_each_child_name(
                    &stream_handle,
                    &stream_path,
                    directory_budget,
                    |marker_name, _| {
                        let mut marker_file = open_child_file(
                            &stream_handle,
                            &stream_path,
                            marker_name,
                            false,
                            false,
                        )?;
                        let Ok(bytes) = read_bounded_file(&mut marker_file, MAX_EVENT_BYTES as u64)
                        else {
                            return Ok(());
                        };
                        metadata_bytes = metadata_bytes
                            .checked_add(bytes.len() as u64)
                            .ok_or(EventRepositoryError::LimitExceeded)?;
                        ensure_inclusive_limit(metadata_bytes, MAX_REPOSITORY_METADATA_BYTES)?;
                        let Ok(marker) = serde_json::from_slice::<StoredActiveMarker>(&bytes)
                        else {
                            return Ok(());
                        };
                        let Ok(key) =
                            active_marker_key(&marker.scope, &marker.stream_id, marker.sequence)
                        else {
                            return Ok(());
                        };
                        if canonical_bytes(&marker).ok().as_deref() != Some(bytes.as_slice())
                            || state.expected_markers.get(&key) != Some(&marker)
                            || !key.starts_with(&format!("{stream_name}/"))
                        {
                            return Ok(());
                        }
                        Ok(())
                    },
                )?;
                Ok(())
            },
        )?;
        Ok(ReconcilePlan {
            delete_blobs,
            delete_temps,
        })
    }

    /// The write half of recovery: exclusively-locked callers only.
    fn apply_reconcile(&self, plan: ReconcilePlan) -> Result<(), EventRepositoryError> {
        for planned in plan.delete_blobs {
            remove_reconciled_file(
                &self.blobs_handle,
                &self.root.join("blobs"),
                &planned.name,
                planned.file,
            )?;
        }
        for planned in plan.delete_temps {
            remove_reconciled_file(
                &self.temp_handle,
                &self.root.join(".tmp"),
                &planned.name,
                planned.file,
            )?;
        }
        Ok(())
    }

    fn blob_path(
        &self,
        scope: &RepositoryScope,
        evidence_id: &EvidenceId,
    ) -> Result<PathBuf, EventRepositoryError> {
        Ok(self
            .root
            .join("blobs")
            .join(self.blob_name(scope, evidence_id)?))
    }

    fn blob_name(
        &self,
        scope: &RepositoryScope,
        evidence_id: &EvidenceId,
    ) -> Result<String, EventRepositoryError> {
        Ok(format!("{}.json", object_key(scope, evidence_id.as_str())?))
    }
}

impl EventRepository for LocalEventRepository {
    fn append_atomic(
        &self,
        request: &PreparedAppend,
    ) -> Result<Vec<EventEnvelope>, EventRepositoryError> {
        self.with_exclusive_lock(|| self.append_locked(request))
    }

    fn read_stream(
        &self,
        scope: &RepositoryScope,
        stream_id: &str,
        limit: usize,
        cursor: Option<&str>,
    ) -> Result<EventPage, EventRepositoryError> {
        validate_page_limit(limit)?;
        OpaqueId::parse(stream_id).map_err(|_| EventRepositoryError::Invalid)?;
        self.with_shared_lock(|| {
            let state = self.load_state("read_stream")?;
            let key = stream_key(scope, stream_id)?;
            let head = state
                .next_sequence
                .get(&key)
                .map(|next_sequence| {
                    Ok::<_, EventRepositoryError>(StreamHead {
                        next_sequence: *next_sequence,
                        last_event_hash: EventHash::parse(
                            state
                                .last_hash
                                .get(&key)
                                .cloned()
                                .unwrap_or_else(|| GENESIS_HASH.into()),
                        )
                        .map_err(|_| EventRepositoryError::Integrity)?,
                    })
                })
                .transpose()?;
            let start = cursor_start(cursor, scope, stream_id)?;
            let matching = state
                .batches
                .iter()
                .filter(|batch| &batch.scope == scope && batch.stream_id == stream_id)
                .flat_map(|batch| batch.events.iter().cloned())
                .filter(|event| event.sequence > start)
                .take(limit + 1)
                .collect::<Vec<_>>();
            let has_more = matching.len() > limit;
            let events = matching.into_iter().take(limit).collect::<Vec<_>>();
            let next_cursor = if has_more {
                events
                    .last()
                    .map(|event| encode_cursor(scope, stream_id, event.sequence))
                    .transpose()?
            } else {
                None
            };
            Ok(EventPage {
                events,
                next_cursor,
                head,
            })
        })
    }

    fn read_replay_stream(
        &self,
        scope: &RepositoryScope,
        stream_id: &str,
    ) -> Result<Vec<EventEnvelope>, EventRepositoryError> {
        OpaqueId::parse(stream_id).map_err(|_| EventRepositoryError::Invalid)?;
        self.with_shared_lock(|| {
            let state = self.load_state("read_replay_stream")?;
            let events = state
                .batches
                .iter()
                .filter(|batch| &batch.scope == scope && batch.stream_id == stream_id)
                .flat_map(|batch| batch.events.iter().cloned())
                .collect::<Vec<_>>();
            if events.len() > MAX_READ_ALL {
                return Err(EventRepositoryError::LimitExceeded);
            }
            Ok(events)
        })
    }

    fn next_sequence(
        &self,
        scope: &RepositoryScope,
        stream_id: &str,
    ) -> Result<u64, EventRepositoryError> {
        OpaqueId::parse(stream_id).map_err(|_| EventRepositoryError::Invalid)?;
        self.with_shared_lock(|| {
            let state = self.load_state("next_sequence")?;
            Ok(state
                .next_sequence
                .get(&stream_key(scope, stream_id)?)
                .copied()
                .unwrap_or(1))
        })
    }

    fn evidence_exists(
        &self,
        scope: &RepositoryScope,
        evidence_id: &EvidenceId,
    ) -> Result<bool, EventRepositoryError> {
        self.with_shared_lock(|| {
            let state = self.load_state("evidence_exists")?;
            let path = self.blob_path(scope, evidence_id)?;
            Ok(state.reachable_evidence.contains(&path))
        })
    }

    fn artifact_exists(
        &self,
        scope: &RepositoryScope,
        artifact_id: &ArtifactId,
    ) -> Result<bool, EventRepositoryError> {
        self.with_shared_lock(|| {
            let state = self.load_state("artifact_exists")?;
            Ok(state
                .artifacts
                .contains_key(&artifact_key(scope, artifact_id)?))
        })
    }

    fn active_version(
        &self,
        scope: &RepositoryScope,
        stream_id: &str,
    ) -> Result<Option<ActiveVersion>, EventRepositoryError> {
        self.with_shared_lock(|| {
            let state = self.load_state("active_version")?;
            Ok(state
                .active_versions
                .get(&stream_key(scope, stream_id)?)
                .cloned())
        })
    }

    fn committed_events_for_idempotency(
        &self,
        scope: &RepositoryScope,
        stream_id: &str,
        idempotency_key: &OpaqueId,
    ) -> Result<Option<Vec<EventEnvelope>>, EventRepositoryError> {
        self.with_shared_lock(|| {
            let state = self.load_state("committed_events_for_idempotency")?;
            self.sync_loaded_journal(&state)?;
            Ok(state
                .batches
                .iter()
                .find(|batch| {
                    &batch.scope == scope
                        && batch.stream_id == stream_id
                        && batch
                            .events
                            .iter()
                            .any(|event| event.idempotency_key == *idempotency_key)
                })
                .map(|batch| batch.events.clone()))
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
struct LoadedState {
    batches: Vec<PhysicalBatch>,
    next_sequence: BTreeMap<String, u64>,
    last_hash: BTreeMap<String, String>,
    reachable_evidence: BTreeSet<PathBuf>,
    artifacts: BTreeMap<String, StoredArtifactRegistration>,
    active_versions: BTreeMap<String, ActiveVersion>,
    seen_idempotency: BTreeSet<String>,
    expected_markers: BTreeMap<String, StoredActiveMarker>,
}

/// Per-kind load accounting map (#87): operation name -> (full, suffix, hit) counts.
#[cfg(test)]
type LoadsByKind = BTreeMap<&'static str, (u64, u64, u64)>;

/// Which path a load took, for the per-kind accounting (#87).
#[cfg(test)]
#[derive(Clone, Copy)]
enum LoadPath {
    Full,
    Suffix,
    Hit,
}

/// #87: the verified prefix, retained per handle between loads. Everything here was
/// proven by the same per-line verification the full path runs; `verified_offset`
/// advances only past fully verified lines, so it always ends on a newline boundary.
struct VerifiedPrefix {
    journal_identity: FileIdentity,
    verified_offset: u64,
    state: Arc<LoadedState>,
    budget: LoadBudget,
    counted_evidence: BTreeSet<String>,
    counted_artifacts: BTreeSet<String>,
    verified_evidence: BTreeMap<String, EvidenceDigest>,
    verified_evidence_metadata_bytes: u64,
}

/// The full verification context `verify_lines` mutates — the cache is exactly this,
/// frozen at a byte offset. Kept separate from `VerifiedPrefix` so a failed suffix
/// verification can drop a half-mutated context without ever storing it as verified.
struct VerifyCtx {
    state: Arc<LoadedState>,
    budget: LoadBudget,
    counted_evidence: BTreeSet<String>,
    counted_artifacts: BTreeSet<String>,
    verified_evidence: BTreeMap<String, EvidenceDigest>,
    verified_evidence_metadata_bytes: u64,
}

impl VerifyCtx {
    fn fresh() -> Self {
        Self {
            state: Arc::new(LoadedState::default()),
            budget: LoadBudget::new(DEFAULT_LOAD_LIMITS),
            counted_evidence: BTreeSet::new(),
            counted_artifacts: BTreeSet::new(),
            verified_evidence: BTreeMap::new(),
            verified_evidence_metadata_bytes: 0,
        }
    }

    fn from_prefix(prefix: VerifiedPrefix) -> Self {
        Self {
            state: prefix.state,
            budget: prefix.budget,
            counted_evidence: prefix.counted_evidence,
            counted_artifacts: prefix.counted_artifacts,
            verified_evidence: prefix.verified_evidence,
            verified_evidence_metadata_bytes: prefix.verified_evidence_metadata_bytes,
        }
    }

    fn into_prefix(self, journal_identity: FileIdentity, verified_offset: u64) -> VerifiedPrefix {
        VerifiedPrefix {
            journal_identity,
            verified_offset,
            state: self.state,
            budget: self.budget,
            counted_evidence: self.counted_evidence,
            counted_artifacts: self.counted_artifacts,
            verified_evidence: self.verified_evidence,
            verified_evidence_metadata_bytes: self.verified_evidence_metadata_bytes,
        }
    }
}

struct StagedBlob {
    temp_name: String,
    final_name: String,
    bytes: Vec<u8>,
    file: Option<File>,
}

struct PlannedDelete {
    name: String,
    file: File,
}

/// What recovery would write (#143): produced read-only by `plan_reconcile`; empty
/// means a clean store and the shared fast path may keep its lock.
struct ReconcilePlan {
    delete_blobs: Vec<PlannedDelete>,
    delete_temps: Vec<PlannedDelete>,
}

impl ReconcilePlan {
    fn is_noop(&self) -> bool {
        self.delete_blobs.is_empty() && self.delete_temps.is_empty()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredEvidence {
    format_version: String,
    reference: graphhelm_protocols::EvidenceReference,
    scope: RepositoryScope,
    media_type: String,
    sensitivity: graphhelm_protocols::Sensitivity,
    retention_class: String,
    algorithm: String,
    nonce_hex: String,
    ciphertext_hex: String,
    wrapped_key: StoredWrappedKey,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredWrappedKey {
    key_id: String,
    handle: String,
    algorithm: String,
    nonce_hex: String,
    ciphertext_hex: String,
    aad_sha256: RawSha256,
}

impl StoredEvidence {
    fn from_sealed(value: &SealedEvidence) -> Result<Self, EventRepositoryError> {
        if sha256_hex(value.ciphertext()) != value.reference().ciphertext_sha256().as_str() {
            return Err(EventRepositoryError::Invalid);
        }
        Ok(Self {
            format_version: FORMAT_VERSION.into(),
            reference: value.reference().clone(),
            scope: value.scope().clone(),
            media_type: value.media_type().to_string(),
            sensitivity: value.sensitivity(),
            retention_class: value.retention_class().into(),
            algorithm: value.algorithm().into(),
            nonce_hex: hex::encode(value.nonce()),
            ciphertext_hex: hex::encode(value.ciphertext()),
            wrapped_key: StoredWrappedKey {
                key_id: value.wrapped_key().key_id().into(),
                handle: value.wrapped_key().handle().into(),
                algorithm: value.wrapped_key().algorithm().into(),
                nonce_hex: hex::encode(value.wrapped_key().nonce()),
                ciphertext_hex: hex::encode(value.wrapped_key().ciphertext()),
                aad_sha256: value.wrapped_key().aad_sha256().clone(),
            },
        })
    }

    fn verify(
        &self,
        scope: &RepositoryScope,
        reference: &graphhelm_protocols::EvidenceReference,
    ) -> Result<bool, EventRepositoryError> {
        if self.format_version != FORMAT_VERSION
            || &self.scope != scope
            || &self.reference != reference
        {
            return Ok(false);
        }
        let sealed = self.to_sealed()?;
        Ok(validate_sealed_metadata(&sealed).is_ok())
    }

    fn to_sealed(&self) -> Result<SealedEvidence, EventRepositoryError> {
        fn decode_canonical(value: &str) -> Result<Vec<u8>, EventRepositoryError> {
            let decoded = hex::decode(value).map_err(|_| EventRepositoryError::Integrity)?;
            if hex::encode(&decoded) != value {
                return Err(EventRepositoryError::Integrity);
            }
            Ok(decoded)
        }
        let wrapped = WrappedKey::new(
            self.wrapped_key.key_id.clone(),
            self.wrapped_key.handle.clone(),
            &self.wrapped_key.algorithm,
            decode_canonical(&self.wrapped_key.nonce_hex)?,
            decode_canonical(&self.wrapped_key.ciphertext_hex)?,
            self.wrapped_key.aad_sha256.clone(),
        )
        .map_err(|_| EventRepositoryError::Integrity)?;
        let sealed = SealedEvidence::new(
            self.reference.clone(),
            self.scope.clone(),
            self.media_type.clone(),
            self.sensitivity,
            &self.retention_class,
            &self.algorithm,
            decode_canonical(&self.nonce_hex)?,
            decode_canonical(&self.ciphertext_hex)?,
            wrapped,
        )
        .map_err(|_| EventRepositoryError::Integrity)?;
        validate_sealed_metadata(&sealed).map_err(|_| EventRepositoryError::Integrity)?;
        Ok(sealed)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredActiveMarker {
    format_version: String,
    scope: RepositoryScope,
    stream_id: String,
    number: u64,
    semantic_hash: String,
    sequence: u64,
    event_hash: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RequestDigest<'a> {
    scope: &'a RepositoryScope,
    stream_id: &'a OpaqueId,
    expected_next_sequence: u64,
    events: &'a [NewEvent],
    evidence: Vec<EvidenceDigest>,
    artifacts: Vec<StoredArtifactRegistration>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct EvidenceDigest {
    reference: graphhelm_protocols::EvidenceReference,
    scope: RepositoryScope,
    media_type: String,
    sensitivity: graphhelm_protocols::Sensitivity,
    retention_class: String,
    algorithm: String,
    nonce_sha256: String,
    wrapped_key_sha256: String,
}

fn evidence_digest(item: &SealedEvidence) -> EvidenceDigest {
    let mut wrapped = Vec::new();
    wrapped.extend_from_slice(item.wrapped_key().key_id().as_bytes());
    wrapped.extend_from_slice(item.wrapped_key().handle().as_bytes());
    wrapped.extend_from_slice(item.wrapped_key().nonce());
    wrapped.extend_from_slice(item.wrapped_key().ciphertext());
    EvidenceDigest {
        reference: item.reference().clone(),
        scope: item.scope().clone(),
        media_type: item.media_type().to_string(),
        sensitivity: item.sensitivity(),
        retention_class: item.retention_class().into(),
        algorithm: item.algorithm().into(),
        nonce_sha256: wire_sha256(item.nonce()),
        wrapped_key_sha256: wire_sha256(&wrapped),
    }
}

fn request_digest_from_evidence_digests(
    scope: &RepositoryScope,
    stream_id: &OpaqueId,
    expected_next_sequence: u64,
    events: &[NewEvent],
    mut evidence: Vec<EvidenceDigest>,
    artifacts: Vec<StoredArtifactRegistration>,
) -> Result<String, EventRepositoryError> {
    evidence.sort_by(|left, right| {
        left.reference
            .evidence_id()
            .as_str()
            .cmp(right.reference.evidence_id().as_str())
    });
    let digest_input = RequestDigest {
        scope,
        stream_id,
        expected_next_sequence,
        events,
        evidence,
        artifacts,
    };
    serialized_len_bounded(&digest_input, MAX_BATCH_BYTES)?;
    Ok(wire_sha256(&canonical_bytes(&digest_input)?))
}

fn validate_request_preflight(
    request: &PreparedAppend,
    state: &LoadedState,
) -> Result<(), EventRepositoryError> {
    crate::validate_prepared_append(request)?;
    let mut committed = BTreeMap::new();
    for artifact_id in request
        .artifacts()
        .iter()
        .map(|item| item.reference().artifact_id())
        .chain(
            request
                .events()
                .iter()
                .flat_map(|event| &event.artifact_refs)
                .map(|reference| reference.artifact_id()),
        )
    {
        if let Some(existing) = state
            .artifacts
            .get(&artifact_key(request.scope(), artifact_id)?)
        {
            committed.insert(
                artifact_id.to_string(),
                crate::CommittedArtifact {
                    reference: existing.reference.clone(),
                    producer_stream_id: existing.producer_stream_id.clone(),
                    producer_idempotency_key: existing.producer_idempotency_key.clone(),
                },
            );
        }
    }
    crate::validate_artifact_relations(request, &committed)
}

fn validate_graph_successors(
    request: &PreparedAppend,
    state: &LoadedState,
) -> Result<(), EventRepositoryError> {
    let stream_identity = stream_key(request.scope(), request.stream_id().as_str())?;
    let active = state
        .active_versions
        .get(&stream_identity)
        .map(|active| crate::ActiveGraphIdentity::new(active.number, active.semantic_hash.clone()))
        .transpose()?;
    crate::validate_graph_lineage(request, active.as_ref())
}

fn batch_checksum(batch: &PhysicalBatch) -> Result<String, EventRepositoryError> {
    Ok(wire_sha256(&canonical_bytes(&BatchChecksum {
        format_version: &batch.format_version,
        request_digest: &batch.request_digest,
        scope: &batch.scope,
        stream_id: &batch.stream_id,
        expected_next_sequence: batch.expected_next_sequence,
        evidence_ids: &batch.evidence_ids,
        artifacts: &batch.artifacts,
        events: &batch.events,
    })?))
}

/// Verify that one stored journal line still round-trips through this crate's canonical form.
///
/// **PURPOSE-BUILT, NOT A GENERAL API (#181).** This exists so an accounting suite OUTSIDE this
/// crate can check committed journals that have no store around them — bare `journal.jsonl`
/// bundles under `docs/acceptance/` with no `events/` directory, which `LocalEventRepository::open`
/// cannot reach. It promises nothing about any other use, and it is deliberately the narrowest
/// surface that answers that question: `&str` in, this crate's existing error out, and **no new
/// public types** — `PhysicalBatch` and `canonical_bytes` stay `pub(crate)`.
///
/// **It WRAPS `load_state`'s check; it does not reimplement it.** The same `parse_physical_batch`,
/// the same `batch_checksum`, the same `canonical_bytes`, in the same order, so a caller is
/// comparing against THIS store's notion of canonical form. Re-deriving canonical form in the
/// caller would be a duplicated ORACLE, and a duplicated oracle diverges in silence — the journals
/// would then be checked against the test's idea of canonical rather than the store's.
///
/// # Errors
///
/// - [`EventRepositoryError::Integrity`] — the line is not canonical, fails schema validation, or
///   carries a `formatVersion` this build does not write.
/// - [`EventRepositoryError::CorruptBatch`] — the batch fails its own checksum, which is a
///   different recovery path from a broken hash chain and is reported separately for that reason.
pub fn journal_line_roundtrips(line: &str) -> Result<(), EventRepositoryError> {
    // The same three checks `load_state` runs over every stored line, in the same order and
    // through the same functions. Kept as a call sequence rather than a shared helper on purpose:
    // extracting one would let `load_state`'s copy drift from this one, and this exists precisely
    // so an outside caller measures against what the store actually does.
    let schemas =
        graphhelm_schema::repository_schema_set().map_err(|_| EventRepositoryError::Integrity)?;
    let bytes = line.as_bytes();
    let batch = parse_physical_batch(schemas, bytes)?;
    if batch.checksum != batch_checksum(&batch)? {
        return Err(EventRepositoryError::CorruptBatch);
    }
    if canonical_bytes(&batch)? != bytes || batch.format_version != FORMAT_VERSION {
        return Err(EventRepositoryError::Integrity);
    }
    Ok(())
}

fn parse_physical_batch(
    schemas: &graphhelm_schema::RepositorySchemaSet,
    line: &[u8],
) -> Result<PhysicalBatch, EventRepositoryError> {
    let raw: serde_json::Value =
        serde_json::from_slice(line).map_err(|_| EventRepositoryError::Integrity)?;
    if canonical_bytes(&raw)? != line || !schemas.validate_batch(&raw).is_empty() {
        return Err(EventRepositoryError::Integrity);
    }
    serde_json::from_value(raw).map_err(|_| EventRepositoryError::Integrity)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LayoutState {
    RecognizedPartial,
    /// A recognized repository missing only `.tmp` and/or `active`.
    ///
    /// Those two directories are transient workspace, not evidence: they hold nothing the
    /// journal does not already carry, and an active marker is republished from history on
    /// every open. Empty, they are dropped by git, zip and rsync alike — which is how two
    /// committed acceptance stores spent their whole life unopenable while every checksum
    /// stayed green. `blobs` is deliberately NOT in this state: a missing evidence directory
    /// is a real signal and must keep failing.
    RecoverableDirs,
    Complete,
}

fn collect_child_names_bounded(
    directory: &File,
    path: &Path,
    max_entries: usize,
) -> Result<BTreeSet<String>, EventRepositoryError> {
    let mut budget = DirectoryBudget::with_limits(max_entries, MAX_REPOSITORY_NAME_BYTES);
    let mut names = BTreeSet::new();
    for_each_child_name(directory, path, &mut budget, |name, _| {
        names.insert(name.to_owned());
        Ok(())
    })?;
    Ok(names)
}

fn classify_layout(root: &Path, root_handle: &File) -> Result<LayoutState, EventRepositoryError> {
    reject_link_or_non_directory(root)?;
    let names = collect_child_names_bounded(root_handle, root, MAX_ROOT_ENTRIES + 1)?;
    let has_format = names.contains("format.json");
    let allowed = BTreeSet::from([
        "blobs".to_owned(),
        ".tmp".to_owned(),
        "active".to_owned(),
        "format.json".to_owned(),
        "journal.jsonl".to_owned(),
        "repository.lock".to_owned(),
    ]);
    for name in &names {
        if !allowed.contains(name) {
            return Err(EventRepositoryError::UnsupportedFormat);
        }
        match name.as_str() {
            "blobs" | ".tmp" | "active" => {
                let directory = open_child_directory(root_handle, root, name)?;
                if !has_format {
                    let mut budget = DirectoryBudget::with_limits(1, MAX_REPOSITORY_NAME_BYTES);
                    for_each_child_name(&directory, &root.join(name), &mut budget, |_, _| {
                        Err(EventRepositoryError::UnsupportedFormat)
                    })?;
                }
            }
            "journal.jsonl" | "repository.lock" => {
                let file = open_child_file(root_handle, root, name, true, false)?;
                if !has_format && file.metadata()?.len() != 0 {
                    return Err(EventRepositoryError::UnsupportedFormat);
                }
            }
            "format.json" => {
                let mut file = open_child_file(root_handle, root, name, false, false)?;
                if read_bounded_file(&mut file, 1024)? != FORMAT_BYTES {
                    return Err(EventRepositoryError::UnsupportedFormat);
                }
            }
            _ => return Err(EventRepositoryError::UnsupportedFormat),
        }
    }
    if has_format {
        if names == allowed {
            return Ok(LayoutState::Complete);
        }
        // Every name present is already known to be allowed (the loop above refuses anything
        // else), so the difference is exactly what the layout is missing. Only the transient
        // workspace directories may be missing and still be recoverable.
        if allowed
            .difference(&names)
            .all(|name| name == ".tmp" || name == "active")
        {
            return Ok(LayoutState::RecoverableDirs);
        }
        return Err(EventRepositoryError::Integrity);
    }
    Ok(LayoutState::RecognizedPartial)
}

/// #143: the shared fast path's acquisition — ONLY a layout already Complete with an
/// existing lock file qualifies; everything else answers None and the caller runs the
/// exclusive bootstrap exactly as before. Re-classifies under the shared lock (the same
/// pure classifier, so the check still means something).
fn initialize_root_shared_fast(
    root: &Path,
    root_handle: &File,
) -> Result<Option<File>, EventRepositoryError> {
    if classify_layout(root, root_handle)? != LayoutState::Complete {
        return Ok(None);
    }
    if !collect_child_names_bounded(root_handle, root, MAX_ROOT_ENTRIES + 1)?
        .contains("repository.lock")
    {
        return Ok(None);
    }
    let lock = open_child_file(root_handle, root, "repository.lock", true, false)?;
    if FileExt::lock_shared(&lock).is_err() {
        return Err(EventRepositoryError::Storage);
    }
    if classify_layout(root, root_handle)? != LayoutState::Complete {
        let _ = FileExt::unlock(&lock);
        return Ok(None);
    }
    Ok(Some(lock))
}

fn initialize_root_locked(root: &Path, root_handle: &File) -> Result<File, EventRepositoryError> {
    let initial = classify_layout(root, root_handle)?;
    let lock_present = collect_child_names_bounded(root_handle, root, MAX_ROOT_ENTRIES + 1)?
        .contains("repository.lock");
    let lock = if lock_present {
        open_child_file(root_handle, root, "repository.lock", true, false)?
    } else {
        if initial == LayoutState::Complete {
            return Err(EventRepositoryError::Integrity);
        }
        open_or_create_repository_lock(root_handle, root)?
    };
    lock.lock_exclusive()
        .map_err(|_| EventRepositoryError::Storage)?;

    repair_layout_locked(root, root_handle)?;
    Ok(lock)
}

/// The post-lock repair half of `initialize_root_locked`, callable by anyone HOLDING the
/// exclusive lock (#143 review, D's finding (a)): the upgrade path re-runs this after
/// re-acquiring exclusive, because its Complete verdict was confirmed under a lock it no
/// longer holds — re-classifying under the NEW lock is what the other two acquisition
/// paths already pay for, and "correctness equals a fresh exclusive open" is only true
/// if the redo does too.
fn repair_layout_locked(root: &Path, root_handle: &File) -> Result<(), EventRepositoryError> {
    match classify_layout(root, root_handle)? {
        LayoutState::Complete => return Ok(()),
        // Create ONLY what is missing, and nothing else. The partial path below rewrites
        // `format.json` and may create `journal.jsonl`; doing that to a recognized repository
        // would write file bytes into an archive that asked for two empty directories, and an
        // archive is exactly the thing that may be checksummed, mounted read-only, or both.
        // The re-classification afterwards is the same PURE classifier, so the check still
        // means something.
        LayoutState::RecoverableDirs => {
            for name in [".tmp", "active"] {
                let _ = ensure_child_directory(root_handle, root, name)?;
            }
            sync_directory_handle(root_handle)?;
            if classify_layout(root, root_handle)? != LayoutState::Complete {
                return Err(EventRepositoryError::Integrity);
            }
            return Ok(());
        }
        LayoutState::RecognizedPartial => {}
    }
    for name in ["blobs", ".tmp", "active"] {
        let _ = ensure_child_directory(root_handle, root, name)?;
    }
    let names = collect_child_names_bounded(root_handle, root, MAX_ROOT_ENTRIES + 1)?;
    if !names.contains("journal.jsonl") {
        open_child_file(root_handle, root, "journal.jsonl", true, true)?.sync_all()?;
    }
    sync_directory_handle(root_handle)?;
    let mut file = open_child_file(root_handle, root, "format.json", true, true)?;
    file.write_all(FORMAT_BYTES)?;
    file.sync_all()?;
    drop(file);
    sync_directory_handle(root_handle)?;
    if classify_layout(root, root_handle)? != LayoutState::Complete {
        return Err(EventRepositoryError::Integrity);
    }
    Ok(())
}

#[cfg(unix)]
fn lock_root_exclusive(root: &File) -> Result<(), EventRepositoryError> {
    root.lock_exclusive()
        .map_err(|_| EventRepositoryError::Storage)
}

#[cfg(unix)]
fn unlock_root(root: &File) -> Result<(), EventRepositoryError> {
    FileExt::unlock(root).map_err(|_| EventRepositoryError::Storage)
}

#[cfg(windows)]
fn lock_root_exclusive(_root: &File) -> Result<(), EventRepositoryError> {
    Ok(())
}

#[cfg(windows)]
fn unlock_root(_root: &File) -> Result<(), EventRepositoryError> {
    Ok(())
}

fn ensure_root_path(root: &Path) -> Result<File, EventRepositoryError> {
    ensure_root_path_observed(root, &mut |_| {})
}

#[cfg(unix)]
fn ensure_root_path_observed(
    root: &Path,
    after_create: &mut dyn FnMut(&Path),
) -> Result<File, EventRepositoryError> {
    reject_linked_ancestors(root)?;
    let absolute = if root.is_absolute() {
        root.to_path_buf()
    } else {
        std::env::current_dir()?.join(root)
    };
    let mut ancestor = absolute.as_path();
    while !ancestor.exists() {
        ancestor = ancestor
            .parent()
            .ok_or(EventRepositoryError::UnsupportedFormat)?;
    }
    reject_link_or_non_directory(ancestor)?;
    let mut retained = open_directory(ancestor)?;
    let mut current = ancestor.to_path_buf();
    let suffix = absolute
        .strip_prefix(ancestor)
        .map_err(|_| EventRepositoryError::UnsupportedFormat)?;
    for component in suffix.components() {
        let std::path::Component::Normal(component) = component else {
            return Err(EventRepositoryError::UnsupportedFormat);
        };
        let name = component
            .to_str()
            .ok_or(EventRepositoryError::UnsupportedFormat)?;
        retained = create_and_open_root_child(&retained, &current, name, after_create)?;
        current.push(component);
    }
    Ok(retained)
}

#[cfg(windows)]
fn ensure_root_path_observed(
    root: &Path,
    after_create: &mut dyn FnMut(&Path),
) -> Result<File, EventRepositoryError> {
    let absolute = if root.is_absolute() {
        root.to_path_buf()
    } else {
        std::env::current_dir()?.join(root)
    };
    let mut components = absolute.components();
    let prefix = components
        .next()
        .ok_or(EventRepositoryError::UnsupportedFormat)?;
    let root_dir = components
        .next()
        .ok_or(EventRepositoryError::UnsupportedFormat)?;
    if !matches!(prefix, std::path::Component::Prefix(_))
        || root_dir != std::path::Component::RootDir
    {
        return Err(EventRepositoryError::UnsupportedFormat);
    }
    let mut current = PathBuf::new();
    current.push(prefix.as_os_str());
    current.push(root_dir.as_os_str());
    let mut retained = open_directory(&current)?;
    for component in components {
        let std::path::Component::Normal(component) = component else {
            return Err(EventRepositoryError::UnsupportedFormat);
        };
        let name = component
            .to_str()
            .ok_or(EventRepositoryError::UnsupportedFormat)?;
        retained = create_and_open_root_child(&retained, &current, name, after_create)?;
        current.push(component);
    }
    Ok(retained)
}

#[cfg(unix)]
fn create_and_open_root_child(
    directory: &File,
    path: &Path,
    name: &str,
    after_create: &mut dyn FnMut(&Path),
) -> Result<File, EventRepositoryError> {
    use std::ffi::CString;
    use std::os::fd::AsRawFd;
    validate_child_name(name)?;
    let component = CString::new(name).map_err(|_| EventRepositoryError::UnsupportedFormat)?;
    // SAFETY: the retained directory and bounded component are live.
    let created = if unsafe { libc::mkdirat(directory.as_raw_fd(), component.as_ptr(), 0o700) } == 0
    {
        true
    } else {
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::AlreadyExists {
            return Err(EventRepositoryError::Storage);
        }
        false
    };
    if created {
        after_create(&path.join(name));
    }
    open_child_directory(directory, path, name)
}

#[cfg(windows)]
fn create_and_open_root_child(
    directory: &File,
    path: &Path,
    name: &str,
    after_create: &mut dyn FnMut(&Path),
) -> Result<File, EventRepositoryError> {
    validate_child_name(name)?;
    let (file, created) = nt_open_child_directory(directory, name, true)?;
    if created {
        after_create(&path.join(name));
    }
    validate_opened_directory(&file)?;
    Ok(file)
}

#[cfg(unix)]
fn reject_linked_ancestors(path: &Path) -> Result<(), EventRepositoryError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    for ancestor in absolute.ancestors() {
        if !ancestor.exists() {
            continue;
        }
        let metadata = std::fs::symlink_metadata(ancestor)?;
        if metadata.file_type().is_symlink() || is_reparse_point(&metadata) {
            return Err(EventRepositoryError::UnsupportedFormat);
        }
    }
    Ok(())
}

fn reject_link_or_non_directory(path: &Path) -> Result<(), EventRepositoryError> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() || is_reparse_point(&metadata) {
        return Err(EventRepositoryError::UnsupportedFormat);
    }
    Ok(())
}

#[cfg(windows)]
fn is_reparse_point(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
const fn is_reparse_point(_: &std::fs::Metadata) -> bool {
    false
}

fn read_bounded_file(file: &mut File, limit: u64) -> Result<Vec<u8>, EventRepositoryError> {
    let length = file.metadata()?.len();
    ensure_inclusive_limit(length, limit)?;
    let capacity = usize::try_from(length).map_err(|_| EventRepositoryError::LimitExceeded)?;
    let mut bytes = Vec::with_capacity(capacity);
    file.seek(SeekFrom::Start(0))?;
    Read::by_ref(file).take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(EventRepositoryError::LimitExceeded);
    }
    Ok(bytes)
}

/// Reads `[offset..length)` of the file (#87). The caller has already bounded `length`
/// by the journal limit and `offset` always sits on a newline boundary, because it only
/// ever comes from a fully verified prefix. A short read means the file shrank between
/// the metadata call and this read — impossible under the operation's lock for any
/// writer using the store API, so it reports as Storage rather than being silently
/// verified as a shorter suffix.
fn read_bounded_range(
    file: &mut File,
    offset: u64,
    length: u64,
) -> Result<Vec<u8>, EventRepositoryError> {
    let span = length
        .checked_sub(offset)
        .ok_or(EventRepositoryError::Storage)?;
    let capacity = usize::try_from(span).map_err(|_| EventRepositoryError::LimitExceeded)?;
    let mut bytes = Vec::with_capacity(capacity);
    file.seek(SeekFrom::Start(offset))?;
    Read::by_ref(file).take(span).read_to_end(&mut bytes)?;
    if bytes.len() as u64 != span {
        return Err(EventRepositoryError::Storage);
    }
    Ok(bytes)
}

fn ensure_inclusive_limit(length: u64, limit: u64) -> Result<(), EventRepositoryError> {
    if length > limit {
        Err(EventRepositoryError::LimitExceeded)
    } else {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileIdentity {
    device: u64,
    file: u64,
}

#[cfg(windows)]
fn file_identity(file: &File) -> Result<FileIdentity, EventRepositoryError> {
    use std::mem::MaybeUninit;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };

    let mut information = MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
    // SAFETY: `file` owns a live OS handle and the API initializes the provided
    // fixed-size structure on success. The return value is checked before read.
    let success =
        unsafe { GetFileInformationByHandle(file.as_raw_handle(), information.as_mut_ptr()) };
    if success == 0 {
        return Err(EventRepositoryError::Storage);
    }
    // SAFETY: successful GetFileInformationByHandle initialized all fields.
    let information = unsafe { information.assume_init() };
    Ok(FileIdentity {
        device: u64::from(information.dwVolumeSerialNumber),
        file: (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow),
    })
}

#[cfg(unix)]
fn file_identity(file: &File) -> Result<FileIdentity, EventRepositoryError> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata()?;
    Ok(FileIdentity {
        device: metadata.dev(),
        file: metadata.ino(),
    })
}

fn open_directory(path: &Path) -> Result<File, EventRepositoryError> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
            FILE_SHARE_WRITE,
        };
        let file = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?;
        validate_opened_directory(&file)?;
        Ok(file)
    }
    #[cfg(unix)]
    {
        use std::ffi::CString;
        use std::os::fd::{AsRawFd, FromRawFd};
        use std::os::unix::ffi::OsStrExt;
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()?.join(path)
        };
        let root = CString::new("/").map_err(|_| EventRepositoryError::Storage)?;
        // SAFETY: fixed root path is NUL-terminated and descriptor is transferred once.
        let descriptor = unsafe {
            libc::open(
                root.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            )
        };
        if descriptor < 0 {
            return Err(EventRepositoryError::Storage);
        }
        // SAFETY: descriptor is fresh and uniquely owned.
        let mut current = unsafe { File::from_raw_fd(descriptor) };
        for component in absolute.components() {
            match component {
                std::path::Component::RootDir | std::path::Component::CurDir => {}
                std::path::Component::ParentDir | std::path::Component::Prefix(_) => {
                    return Err(EventRepositoryError::UnsupportedFormat);
                }
                std::path::Component::Normal(part) => {
                    let name = CString::new(part.as_bytes())
                        .map_err(|_| EventRepositoryError::UnsupportedFormat)?;
                    // SAFETY: retained descriptor and fixed component are live; no links followed.
                    let next = unsafe {
                        libc::openat(
                            current.as_raw_fd(),
                            name.as_ptr(),
                            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                        )
                    };
                    if next < 0 {
                        return Err(EventRepositoryError::Storage);
                    }
                    // SAFETY: next is fresh and replaces the previous retained descriptor.
                    current = unsafe { File::from_raw_fd(next) };
                }
            }
        }
        Ok(current)
    }
}

fn validate_child_name(name: &str) -> Result<(), EventRepositoryError> {
    if name.is_empty()
        || name.len() > 128
        || !name.is_ascii()
        || name.contains('/')
        || name.contains('\\')
        || matches!(name, "." | "..")
    {
        return Err(EventRepositoryError::UnsupportedFormat);
    }
    Ok(())
}

#[cfg(unix)]
fn create_repository_lock(directory: &File, path: &Path) -> Result<File, EventRepositoryError> {
    open_child_file(directory, path, "repository.lock", true, true)
}

#[cfg(windows)]
fn create_repository_lock(_directory: &File, path: &Path) -> Result<File, EventRepositoryError> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .access_mode(GENERIC_READ | GENERIC_WRITE)
        .create_new(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path.join("repository.lock"))
        .map_err(|_| EventRepositoryError::Storage)?;
    validate_opened_regular(&file)?;
    Ok(file)
}

fn open_or_create_repository_lock(
    directory: &File,
    path: &Path,
) -> Result<File, EventRepositoryError> {
    match create_repository_lock(directory, path) {
        Ok(file) => Ok(file),
        Err(EventRepositoryError::Storage) => {
            open_child_file(directory, path, "repository.lock", true, false)
        }
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
fn create_temp_child(
    directory: &File,
    _path: &Path,
    name: &str,
) -> Result<File, EventRepositoryError> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    validate_child_name(name)?;
    let name = CString::new(name).map_err(|_| EventRepositoryError::UnsupportedFormat)?;
    // SAFETY: the retained directory descriptor and bounded child name are live.
    let descriptor = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDWR | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_CREAT | libc::O_EXCL,
            0o600,
        )
    };
    if descriptor < 0 {
        let error = std::io::Error::last_os_error();
        return Err(if error.kind() == std::io::ErrorKind::AlreadyExists {
            EventRepositoryError::IdempotencyConflict
        } else {
            EventRepositoryError::Storage
        });
    }
    // SAFETY: the fresh descriptor is transferred exactly once.
    Ok(unsafe { File::from_raw_fd(descriptor) })
}

#[cfg(windows)]
fn create_temp_child(
    _directory: &File,
    path: &Path,
    name: &str,
) -> Result<File, EventRepositoryError> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE};
    use windows_sys::Win32::Storage::FileSystem::{
        DELETE, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    validate_child_name(name)?;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .access_mode(GENERIC_READ | GENERIC_WRITE | DELETE)
        .create_new(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path.join(name))
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                EventRepositoryError::IdempotencyConflict
            } else {
                EventRepositoryError::Storage
            }
        })?;
    validate_opened_regular(&file)?;
    Ok(file)
}

#[cfg(unix)]
fn open_child_file(
    directory: &File,
    _path: &Path,
    name: &str,
    write: bool,
    create_new: bool,
) -> Result<File, EventRepositoryError> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    validate_child_name(name)?;
    let name = CString::new(name).map_err(|_| EventRepositoryError::UnsupportedFormat)?;
    let mut flags = if write { libc::O_RDWR } else { libc::O_RDONLY };
    flags |= libc::O_CLOEXEC | libc::O_NOFOLLOW;
    if create_new {
        flags |= libc::O_CREAT | libc::O_EXCL;
    }
    // SAFETY: retained directory descriptor and NUL-terminated child are valid.
    let descriptor = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags, 0o600) };
    if descriptor < 0 {
        return Err(if create_new {
            EventRepositoryError::Storage
        } else {
            EventRepositoryError::Integrity
        });
    }
    // SAFETY: descriptor is fresh and uniquely owned.
    Ok(unsafe { File::from_raw_fd(descriptor) })
}

#[cfg(windows)]
fn open_child_file(
    _directory: &File,
    path: &Path,
    name: &str,
    write: bool,
    create: bool,
) -> Result<File, EventRepositoryError> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    validate_child_name(name)?;
    use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE};
    use windows_sys::Win32::Storage::FileSystem::DELETE;
    let desired =
        GENERIC_READ | if write { GENERIC_WRITE } else { 0 } | if create { DELETE } else { 0 };
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(write)
        .access_mode(desired)
        .create_new(create)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path.join(name))
        .map_err(|_| {
            if create {
                EventRepositoryError::Storage
            } else {
                EventRepositoryError::Integrity
            }
        })?;
    validate_opened_regular(&file)?;
    Ok(file)
}

#[cfg(unix)]
fn open_child_file_for_delete(
    directory: &File,
    path: &Path,
    name: &str,
) -> Result<File, EventRepositoryError> {
    open_child_file(directory, path, name, false, false)
}

#[cfg(windows)]
fn open_child_file_for_delete(
    _directory: &File,
    path: &Path,
    name: &str,
) -> Result<File, EventRepositoryError> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Foundation::GENERIC_READ;
    use windows_sys::Win32::Storage::FileSystem::{
        DELETE, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    validate_child_name(name)?;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .access_mode(GENERIC_READ | DELETE)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path.join(name))
        .map_err(|_| EventRepositoryError::Integrity)?;
    validate_opened_regular(&file)?;
    Ok(file)
}

#[cfg(unix)]
fn open_child_directory(
    directory: &File,
    _path: &Path,
    name: &str,
) -> Result<File, EventRepositoryError> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    validate_child_name(name)?;
    let name = CString::new(name).map_err(|_| EventRepositoryError::UnsupportedFormat)?;
    // SAFETY: retained directory descriptor and NUL-terminated child are valid.
    let descriptor = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if descriptor < 0 {
        return Err(EventRepositoryError::Integrity);
    }
    // SAFETY: descriptor is fresh and uniquely owned.
    Ok(unsafe { File::from_raw_fd(descriptor) })
}

#[cfg(unix)]
fn ensure_child_directory(
    directory: &File,
    path: &Path,
    name: &str,
) -> Result<File, EventRepositoryError> {
    use std::ffi::CString;
    use std::os::fd::AsRawFd;
    validate_child_name(name)?;
    let component = CString::new(name).map_err(|_| EventRepositoryError::UnsupportedFormat)?;
    // SAFETY: retained directory descriptor and fixed NUL-terminated child are live.
    if unsafe { libc::mkdirat(directory.as_raw_fd(), component.as_ptr(), 0o700) } != 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::AlreadyExists {
            return Err(EventRepositoryError::Storage);
        }
    }
    open_child_directory(directory, path, name)
}

#[cfg(windows)]
fn ensure_child_directory(
    directory: &File,
    _path: &Path,
    name: &str,
) -> Result<File, EventRepositoryError> {
    validate_child_name(name)?;
    let (file, _) = nt_open_child_directory(directory, name, true)?;
    validate_opened_directory(&file)?;
    Ok(file)
}

#[cfg(unix)]
fn link_retained_file_between(
    source_file: &File,
    source_directory: &File,
    _source_path: &Path,
    source: &str,
    destination_directory: &File,
    _destination_path: &Path,
    destination: &str,
) -> Result<(), EventRepositoryError> {
    use std::ffi::CString;
    use std::os::fd::AsRawFd;
    validate_child_name(source)?;
    validate_child_name(destination)?;
    let destination =
        CString::new(destination).map_err(|_| EventRepositoryError::UnsupportedFormat)?;
    #[cfg(target_os = "linux")]
    let linked = {
        let _ = (source_directory, source);
        // SAFETY: the source descriptor pins the staged inode; AT_EMPTY_PATH avoids
        // resolving the mutable source directory entry during publication.
        let direct = unsafe {
            libc::linkat(
                source_file.as_raw_fd(),
                c"".as_ptr(),
                destination_directory.as_raw_fd(),
                destination.as_ptr(),
                libc::AT_EMPTY_PATH,
            )
        };
        if direct == 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::EPERM) {
            direct
        } else {
            let proc_fd = CString::new(format!("/proc/self/fd/{}", source_file.as_raw_fd()))
                .map_err(|_| EventRepositoryError::Storage)?;
            // SAFETY: /proc/self/fd resolves the retained descriptor, not the mutable
            // source name; AT_SYMLINK_FOLLOW links that pinned inode for unprivileged Linux.
            unsafe {
                libc::linkat(
                    libc::AT_FDCWD,
                    proc_fd.as_ptr(),
                    destination_directory.as_raw_fd(),
                    destination.as_ptr(),
                    libc::AT_SYMLINK_FOLLOW,
                )
            }
        }
    };
    #[cfg(not(target_os = "linux"))]
    let linked = {
        let source = CString::new(source).map_err(|_| EventRepositoryError::UnsupportedFormat)?;
        if named_file_identity(source_directory, source.to_bytes())? != file_identity(source_file)?
        {
            return Err(EventRepositoryError::Integrity);
        }
        // SAFETY: both retained directory descriptors and fixed child names are live.
        unsafe {
            libc::linkat(
                source_directory.as_raw_fd(),
                source.as_ptr(),
                destination_directory.as_raw_fd(),
                destination.as_ptr(),
                0,
            )
        }
    };
    if linked == 0 {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    if error.kind() == std::io::ErrorKind::AlreadyExists {
        Err(EventRepositoryError::IdempotencyConflict)
    } else {
        Err(EventRepositoryError::Storage)
    }
}

#[cfg(windows)]
fn link_retained_file_between(
    source_file: &File,
    _source_directory: &File,
    _source_path: &Path,
    source: &str,
    destination_directory: &File,
    _destination_path: &Path,
    destination: &str,
) -> Result<(), EventRepositoryError> {
    validate_child_name(source)?;
    validate_child_name(destination)?;
    validate_opened_regular(source_file)?;
    validate_opened_directory(destination_directory)?;
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Foundation::GENERIC_READ;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    let named = std::fs::OpenOptions::new()
        .read(true)
        .access_mode(GENERIC_READ)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(_source_path.join(source))
        .map_err(|_| EventRepositoryError::Integrity)?;
    validate_opened_regular(&named)?;
    if file_identity(&named)? != file_identity(source_file)? {
        return Err(EventRepositoryError::Integrity);
    }
    std::fs::hard_link(
        _source_path.join(source),
        _destination_path.join(destination),
    )
    .map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            EventRepositoryError::IdempotencyConflict
        } else {
            EventRepositoryError::Storage
        }
    })?;
    let published = std::fs::OpenOptions::new()
        .read(true)
        .access_mode(GENERIC_READ)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(_destination_path.join(destination))
        .map_err(|_| EventRepositoryError::Integrity)?;
    validate_opened_regular(&published)?;
    if file_identity(&published)? != file_identity(source_file)? {
        return Err(EventRepositoryError::Integrity);
    }
    Ok(())
}

#[cfg(unix)]
fn remove_planned_file(
    directory: &File,
    _path: &Path,
    name: &str,
    retained: File,
) -> Result<(), EventRepositoryError> {
    validate_child_name(name)?;
    if named_file_identity(directory, name.as_bytes())? != file_identity(&retained)? {
        return Err(EventRepositoryError::Integrity);
    }
    // POSIX exposes no atomic compare-and-unlink-by-retained-FD primitive.
    // Preserve the validated inode; collision-free names keep later recovery safe.
    Ok(())
}

#[cfg(unix)]
fn remove_reconciled_file(
    directory: &File,
    _path: &Path,
    name: &str,
    retained: File,
) -> Result<(), EventRepositoryError> {
    validate_child_name(name)?;
    if named_file_identity(directory, name.as_bytes())? != file_identity(&retained)? {
        return Err(EventRepositoryError::Integrity);
    }
    // POSIX has no compare-and-unlink-by-handle primitive. Preserve the exact
    // validated orphan rather than risk unlinking a replacement inode.
    Ok(())
}

#[cfg(unix)]
fn named_file_identity(
    directory: &File,
    name: &[u8],
) -> Result<FileIdentity, EventRepositoryError> {
    use std::ffi::CString;
    use std::mem::MaybeUninit;
    use std::os::fd::AsRawFd;
    let name = CString::new(name).map_err(|_| EventRepositoryError::UnsupportedFormat)?;
    let mut stat = MaybeUninit::<libc::stat>::zeroed();
    // SAFETY: retained directory, fixed child and output storage are live.
    if unsafe {
        libc::fstatat(
            directory.as_raw_fd(),
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } != 0
    {
        return Err(EventRepositoryError::Integrity);
    }
    // SAFETY: successful fstatat initialized the structure.
    let stat = unsafe { stat.assume_init() };
    if stat.st_mode & libc::S_IFMT != libc::S_IFREG {
        return Err(EventRepositoryError::Integrity);
    }
    Ok(FileIdentity {
        device: stat.st_dev,
        file: stat.st_ino,
    })
}

#[cfg(unix)]
fn for_each_child_name(
    directory: &File,
    _path: &Path,
    budget: &mut DirectoryBudget,
    mut visitor: impl FnMut(&str, &mut DirectoryBudget) -> Result<(), EventRepositoryError>,
) -> Result<(), EventRepositoryError> {
    use std::ffi::CStr;
    use std::os::fd::AsRawFd;
    let dot = std::ffi::CString::new(".").map_err(|_| EventRepositoryError::Storage)?;
    // SAFETY: openat creates an independent open file description rooted at the retained directory.
    let duplicate = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            dot.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if duplicate < 0 {
        return Err(EventRepositoryError::Storage);
    }
    // SAFETY: fdopendir consumes the duplicate descriptor on success.
    let stream = unsafe { libc::fdopendir(duplicate) };
    if stream.is_null() {
        // SAFETY: fdopendir failed and did not consume the descriptor.
        unsafe { libc::close(duplicate) };
        return Err(EventRepositoryError::Storage);
    }
    let result = (|| {
        loop {
            // SAFETY: stream remains live until closed below; readdir returns an internal entry.
            let entry = unsafe { libc::readdir(stream) };
            if entry.is_null() {
                break;
            }
            // SAFETY: d_name is NUL-terminated for the returned live entry.
            let bytes = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
            if matches!(bytes, b"." | b"..") {
                continue;
            }
            let name =
                std::str::from_utf8(bytes).map_err(|_| EventRepositoryError::UnsupportedFormat)?;
            budget.account(std::ffi::OsStr::new(name))?;
            visitor(name, budget)?;
        }
        Ok(())
    })();
    // SAFETY: stream is live and closed exactly once; it owns the duplicate descriptor.
    if unsafe { libc::closedir(stream) } != 0 {
        return Err(EventRepositoryError::Storage);
    }
    result
}

#[cfg(windows)]
fn for_each_child_name(
    _directory: &File,
    path: &Path,
    budget: &mut DirectoryBudget,
    mut visitor: impl FnMut(&str, &mut DirectoryBudget) -> Result<(), EventRepositoryError>,
) -> Result<(), EventRepositoryError> {
    for entry in std::fs::read_dir(path)? {
        let name = entry?.file_name();
        let name = name
            .to_str()
            .ok_or(EventRepositoryError::UnsupportedFormat)?;
        budget.account(std::ffi::OsStr::new(name))?;
        visitor(name, budget)?;
    }
    Ok(())
}

#[cfg(windows)]
fn remove_planned_file(
    _directory: &File,
    _path: &Path,
    name: &str,
    retained: File,
) -> Result<(), EventRepositoryError> {
    use std::mem::size_of;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_DISPOSITION_INFO, FileDispositionInfo, SetFileInformationByHandle,
    };
    validate_child_name(name)?;
    validate_opened_regular(&retained)?;
    let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
    // SAFETY: retained owns a live DELETE-capable handle and the fixed structure
    // matches FileDispositionInfo for the supplied exact byte size.
    if unsafe {
        SetFileInformationByHandle(
            retained.as_raw_handle(),
            FileDispositionInfo,
            (&disposition as *const FILE_DISPOSITION_INFO).cast(),
            size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    } == 0
    {
        return Err(EventRepositoryError::Storage);
    }
    drop(retained);
    Ok(())
}

#[cfg(windows)]
fn remove_reconciled_file(
    directory: &File,
    path: &Path,
    name: &str,
    retained: File,
) -> Result<(), EventRepositoryError> {
    remove_planned_file(directory, path, name, retained)
}

#[cfg(windows)]
fn open_child_directory(
    directory: &File,
    _path: &Path,
    name: &str,
) -> Result<File, EventRepositoryError> {
    validate_child_name(name)?;
    let (file, _) = nt_open_child_directory(directory, name, false)?;
    validate_opened_directory(&file)?;
    Ok(file)
}

#[cfg(windows)]
fn nt_open_child_directory(
    directory: &File,
    name: &str,
    create_if_missing: bool,
) -> Result<(File, bool), EventRepositoryError> {
    use std::mem::{size_of, zeroed};
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
    use windows_sys::Wdk::Storage::FileSystem::{
        FILE_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_IF, FILE_OPEN_REPARSE_POINT,
        FILE_SYNCHRONOUS_IO_NONALERT, NtCreateFile,
    };
    use windows_sys::Win32::Foundation::{HANDLE, OBJ_CASE_INSENSITIVE, UNICODE_STRING};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_NORMAL, FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_SHARE_READ,
        FILE_SHARE_WRITE, FILE_TRAVERSE, SYNCHRONIZE,
    };
    use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

    validate_child_name(name)?;
    let mut wide = name.encode_utf16().collect::<Vec<_>>();
    let byte_length = wide
        .len()
        .checked_mul(2)
        .and_then(|length| u16::try_from(length).ok())
        .ok_or(EventRepositoryError::UnsupportedFormat)?;
    let unicode = UNICODE_STRING {
        Length: byte_length,
        MaximumLength: byte_length,
        Buffer: wide.as_mut_ptr(),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: directory.as_raw_handle(),
        ObjectName: &unicode,
        Attributes: OBJ_CASE_INSENSITIVE,
        SecurityDescriptor: std::ptr::null(),
        SecurityQualityOfService: std::ptr::null(),
    };
    // SAFETY: all pointers reference live stack/Vec storage for the duration of the
    // synchronous call; the returned handle is transferred exactly once on success.
    let mut handle: HANDLE = std::ptr::null_mut();
    let mut status: IO_STATUS_BLOCK = unsafe { zeroed() };
    let result = unsafe {
        NtCreateFile(
            &mut handle,
            FILE_LIST_DIRECTORY | FILE_TRAVERSE | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            &attributes,
            &mut status,
            std::ptr::null(),
            FILE_ATTRIBUTE_NORMAL,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            if create_if_missing {
                FILE_OPEN_IF
            } else {
                FILE_OPEN
            },
            FILE_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
            std::ptr::null(),
            0,
        )
    };
    if result < 0 || handle.is_null() {
        return Err(if create_if_missing {
            EventRepositoryError::Storage
        } else {
            EventRepositoryError::Integrity
        });
    }
    // FILE_CREATED is the documented IO_STATUS_BLOCK.Information value 2.
    let created = status.Information == 2;
    // SAFETY: successful NtCreateFile returned an owned live handle.
    Ok((unsafe { File::from_raw_handle(handle) }, created))
}

#[cfg(windows)]
fn validate_opened_regular(file: &File) -> Result<(), EventRepositoryError> {
    let metadata = file.metadata()?;
    if !metadata.is_file() || is_reparse_point(&metadata) {
        return Err(EventRepositoryError::UnsupportedFormat);
    }
    Ok(())
}

#[cfg(windows)]
fn validate_opened_directory(file: &File) -> Result<(), EventRepositoryError> {
    let metadata = file.metadata()?;
    if !metadata.is_dir() || is_reparse_point(&metadata) {
        return Err(EventRepositoryError::UnsupportedFormat);
    }
    Ok(())
}

fn sync_directory_handle(directory: &File) -> Result<(), EventRepositoryError> {
    if let Err(error) = directory.sync_all() {
        #[cfg(windows)]
        if matches!(
            error.kind(),
            std::io::ErrorKind::InvalidInput | std::io::ErrorKind::PermissionDenied
        ) {
            return Ok(());
        }
        drop(error);
        return Err(EventRepositoryError::Storage);
    }
    Ok(())
}

fn object_key(scope: &RepositoryScope, id: &str) -> Result<String, EventRepositoryError> {
    let mut bytes = canonical_bytes(scope)?;
    bytes.push(0);
    bytes.extend_from_slice(id.as_bytes());
    Ok(sha256_hex(&bytes))
}

fn stream_key(scope: &RepositoryScope, stream_id: &str) -> Result<String, EventRepositoryError> {
    object_key(scope, stream_id)
}

fn active_marker_key(
    scope: &RepositoryScope,
    stream_id: &str,
    sequence: u64,
) -> Result<String, EventRepositoryError> {
    Ok(format!(
        "{}/{}.json",
        object_key(scope, stream_id)?,
        sequence
    ))
}

fn artifact_key(
    scope: &RepositoryScope,
    artifact_id: &ArtifactId,
) -> Result<String, EventRepositoryError> {
    object_key(scope, artifact_id.as_str())
}

fn is_digest_json_name(name: &str) -> bool {
    name.len() == 69
        && name.ends_with(".json")
        && name[..64]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn derived_marker_matches(
    directory_handle: &File,
    directory: &Path,
    name: &str,
    expected: &[u8],
) -> bool {
    let Ok(mut marker) = open_child_file(directory_handle, directory, name, false, false) else {
        return false;
    };
    read_bounded_file(&mut marker, MAX_EVENT_BYTES as u64).is_ok_and(|bytes| bytes == expected)
}

fn derived_marker_index(
    directory_handle: &File,
    directory: &Path,
    budget: &mut DirectoryBudget,
) -> Result<BTreeMap<String, String>, EventRepositoryError> {
    let mut index = BTreeMap::new();
    for_each_child_name(directory_handle, directory, budget, |name, _| {
        let Ok(mut marker) = open_child_file(directory_handle, directory, name, false, false)
        else {
            return Ok(());
        };
        if let Ok(bytes) = read_bounded_file(&mut marker, MAX_EVENT_BYTES as u64) {
            index
                .entry(sha256_hex(&bytes))
                .or_insert_with(|| name.to_owned());
        }
        Ok(())
    })?;
    Ok(index)
}

fn is_owned_temp_name(name: &str) -> bool {
    ["blob-", "active-"].iter().any(|prefix| {
        name.strip_prefix(prefix)
            .and_then(|value| value.strip_suffix(".tmp"))
            .is_some_and(|token| {
                token.len() == 64
                    && token
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
            })
    })
}

fn encode_cursor(
    scope: &RepositoryScope,
    stream_id: &str,
    sequence: u64,
) -> Result<String, EventRepositoryError> {
    let value = format!("v1:{}:{sequence}", stream_key(scope, stream_id)?);
    if value.len() > MAX_CURSOR_BYTES {
        return Err(EventRepositoryError::LimitExceeded);
    }
    Ok(value)
}

fn cursor_start(
    cursor: Option<&str>,
    scope: &RepositoryScope,
    stream_id: &str,
) -> Result<u64, EventRepositoryError> {
    let Some(cursor) = cursor else {
        return Ok(0);
    };
    if cursor.len() > MAX_CURSOR_BYTES {
        return Err(EventRepositoryError::LimitExceeded);
    }
    let expected_prefix = format!("v1:{}:", stream_key(scope, stream_id)?);
    let sequence = cursor
        .strip_prefix(&expected_prefix)
        .ok_or(EventRepositoryError::Invalid)?
        .parse::<u64>()
        .map_err(|_| EventRepositoryError::Invalid)?;
    if sequence == 0 || sequence > MAX_SAFE_INTEGER {
        return Err(EventRepositoryError::Invalid);
    }
    Ok(sequence)
}

#[cfg(test)]
mod limit_tests {
    use std::collections::BTreeMap;

    use chrono::{TimeZone, Utc};
    use graphhelm_protocols::{
        ActorId, ArtifactId, ArtifactLocator, ArtifactReference, ContentSlot, EvidenceReference,
        GraphImported, GraphSourceKind, GraphVersionPublished, MediaType, NodeType, Optionality,
        PersistedActor, PersistedActorType, PersistedBudgets, PersistedControl,
        PersistedGraphVersion, PersistedGraphVersionRef, PersistedNode, PersistedTopology,
        SafeValue, SemanticVersion, Sensitivity, WireHash,
    };

    use super::*;

    #[test]
    fn retained_temp_inode_is_published_even_after_name_replacement() {
        let directory = tempfile::tempdir().unwrap();
        let source_path = directory.path().join("source");
        let destination_path = directory.path().join("destination");
        std::fs::create_dir(&source_path).unwrap();
        std::fs::create_dir(&destination_path).unwrap();
        let source_directory = open_directory(&source_path).unwrap();
        let destination_directory = open_directory(&destination_path).unwrap();
        let mut source =
            open_child_file(&source_directory, &source_path, "object.tmp", true, true).unwrap();
        source.write_all(b"retained").unwrap();
        source.sync_all().unwrap();

        let displaced = source_path.join("displaced.tmp");
        let replaced = match std::fs::rename(source_path.join("object.tmp"), &displaced) {
            Ok(()) => {
                std::fs::write(source_path.join("object.tmp"), b"replacement").unwrap();
                true
            }
            Err(error) => {
                assert!(matches!(error.raw_os_error(), Some(5 | 32)));
                false
            }
        };

        let result = link_retained_file_between(
            &source,
            &source_directory,
            &source_path,
            "object.tmp",
            &destination_directory,
            &destination_path,
            "object.json",
        );
        if cfg!(windows) && replaced {
            assert!(matches!(result, Err(EventRepositoryError::Integrity)));
            assert!(!destination_path.join("object.json").exists());
            return;
        }
        result.unwrap();
        assert_eq!(
            std::fs::read(destination_path.join("object.json")).unwrap(),
            b"retained"
        );
    }

    #[test]
    fn planned_delete_refuses_a_replacement_inode() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("objects");
        std::fs::create_dir(&path).unwrap();
        let directory_handle = open_directory(&path).unwrap();
        std::fs::write(path.join("orphan.json"), b"validated").unwrap();
        let planned = open_child_file_for_delete(&directory_handle, &path, "orphan.json").unwrap();
        let displaced = path.join("displaced.json");
        match std::fs::rename(path.join("orphan.json"), &displaced) {
            Ok(()) => std::fs::write(path.join("orphan.json"), b"replacement").unwrap(),
            Err(error) => {
                assert!(matches!(error.raw_os_error(), Some(5 | 32)));
            }
        }
        let outcome = remove_planned_file(&directory_handle, &path, "orphan.json", planned);
        if cfg!(windows) {
            outcome.unwrap();
            assert!(!path.join("orphan.json").exists());
        } else {
            assert!(matches!(outcome, Err(EventRepositoryError::Integrity)));
            assert_eq!(
                std::fs::read(path.join("orphan.json")).unwrap(),
                b"replacement"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn root_fd_lock_prevents_split_writer_after_named_lock_replacement() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("repository");
        let repository =
            LocalEventRepository::open(&root, Arc::new(FixedClock), Arc::new(FixedIds)).unwrap();
        let result = repository.with_exclusive_lock(|| {
            std::fs::rename(root.join("repository.lock"), root.join("displaced.lock"))?;
            std::fs::write(root.join("repository.lock"), [])?;
            let replacement = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(root.join("repository.lock"))?;
            replacement.try_lock_exclusive()?;
            let second_root = open_directory(&root)?;
            assert!(
                second_root.try_lock_exclusive().is_err(),
                "a second writer acquired the stable root inode"
            );
            FileExt::unlock(&replacement)?;
            Ok(())
        });
        assert!(matches!(result, Err(EventRepositoryError::Integrity)));
    }

    #[cfg(unix)]
    #[test]
    fn reconciliation_preserves_the_validated_inode_without_name_based_unlink() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("objects");
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("orphan.json"), b"validated").unwrap();
        let directory_handle = open_directory(&path).unwrap();
        let retained = open_child_file_for_delete(&directory_handle, &path, "orphan.json").unwrap();
        remove_reconciled_file(&directory_handle, &path, "orphan.json", retained).unwrap();
        assert_eq!(
            std::fs::read(path.join("orphan.json")).unwrap(),
            b"validated"
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_identity_gate_rejects_a_swapped_temp_name() {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE};
        use windows_sys::Win32::Storage::FileSystem::{
            DELETE, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ,
            FILE_SHARE_WRITE,
        };

        let directory = tempfile::tempdir().unwrap();
        let source_path = directory.path().join("source");
        let destination_path = directory.path().join("destination");
        std::fs::create_dir(&source_path).unwrap();
        std::fs::create_dir(&destination_path).unwrap();
        let source_directory = open_directory(&source_path).unwrap();
        let destination_directory = open_directory(&destination_path).unwrap();
        let source = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .access_mode(GENERIC_READ | GENERIC_WRITE | DELETE)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(source_path.join("object.tmp"))
            .unwrap();
        std::fs::write(source_path.join("object.tmp"), b"retained").unwrap();
        std::fs::rename(
            source_path.join("object.tmp"),
            source_path.join("displaced.tmp"),
        )
        .unwrap();
        std::fs::write(source_path.join("object.tmp"), b"replacement").unwrap();

        assert!(matches!(
            link_retained_file_between(
                &source,
                &source_directory,
                &source_path,
                "object.tmp",
                &destination_directory,
                &destination_path,
                "object.json",
            ),
            Err(EventRepositoryError::Integrity)
        ));
        assert!(!destination_path.join("object.json").exists());
    }

    #[cfg(windows)]
    #[test]
    fn windows_delete_by_handle_never_deletes_a_replacement_name() {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Foundation::GENERIC_READ;
        use windows_sys::Win32::Storage::FileSystem::{
            DELETE, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ,
            FILE_SHARE_WRITE,
        };

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("objects");
        std::fs::create_dir(&path).unwrap();
        let directory_handle = open_directory(&path).unwrap();
        std::fs::write(path.join("orphan.json"), b"validated").unwrap();
        let retained = std::fs::OpenOptions::new()
            .read(true)
            .access_mode(GENERIC_READ | DELETE)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path.join("orphan.json"))
            .unwrap();
        std::fs::rename(path.join("orphan.json"), path.join("displaced.json")).unwrap();
        std::fs::write(path.join("orphan.json"), b"replacement").unwrap();

        remove_planned_file(&directory_handle, &path, "orphan.json", retained).unwrap();
        assert_eq!(
            std::fs::read(path.join("orphan.json")).unwrap(),
            b"replacement"
        );
        assert!(!path.join("displaced.json").exists());
    }

    #[cfg(windows)]
    #[test]
    fn windows_opened_file_handle_rejects_reparse_points() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("objects");
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("target.json"), b"outside").unwrap();
        if let Err(error) =
            std::os::windows::fs::symlink_file(path.join("target.json"), path.join("linked.json"))
        {
            if error.raw_os_error() == Some(1314) {
                return;
            }
            panic!("failed to create file reparse fixture: {error}");
        }
        let handle = open_directory(&path).unwrap();
        assert!(matches!(
            open_child_file(&handle, &path, "linked.json", false, false),
            Err(EventRepositoryError::UnsupportedFormat)
        ));
    }

    #[test]
    fn event_batch_and_journal_limits_are_inclusive_at_exact_boundaries() {
        for limit in [
            MAX_EVENT_BYTES as u64,
            MAX_BATCH_BYTES as u64,
            MAX_JOURNAL_BYTES,
        ] {
            assert!(ensure_inclusive_limit(limit - 1, limit).is_ok());
            assert!(ensure_inclusive_limit(limit, limit).is_ok());
            assert!(matches!(
                ensure_inclusive_limit(limit + 1, limit),
                Err(EventRepositoryError::LimitExceeded)
            ));
        }
    }

    #[test]
    fn directory_budget_stops_before_visiting_limit_plus_one() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("one"), []).unwrap();
        std::fs::write(directory.path().join("two"), []).unwrap();
        let handle = open_directory(directory.path()).unwrap();
        let mut budget = DirectoryBudget::with_limits(1, usize::MAX);
        let mut visited = 0;

        let error = for_each_child_name(&handle, directory.path(), &mut budget, |_, _| {
            visited += 1;
            Ok(())
        })
        .unwrap_err();

        assert_eq!(error.code(), "GHE006_LIMIT_EXCEEDED");
        assert_eq!(visited, 1);
    }

    #[test]
    fn directory_name_budget_is_inclusive_at_the_exact_boundary() {
        for length in [7_usize, 8, 9] {
            let mut budget = DirectoryBudget::with_limits(1, 8);
            let result = budget.account(std::ffi::OsStr::new(&"x".repeat(length)));
            assert_eq!(result.is_ok(), length <= 8, "length {length}");
        }
    }

    #[test]
    fn bootstrap_lock_open_tolerates_an_initializer_that_won_the_create_race() {
        let directory = tempfile::tempdir().unwrap();
        let handle = open_directory(directory.path()).unwrap();
        drop(create_repository_lock(&handle, directory.path()).unwrap());
        assert!(open_or_create_repository_lock(&handle, directory.path()).is_ok());
    }

    #[test]
    fn load_budget_rejects_every_counter_at_limit_plus_one() {
        let limits = LoadLimits {
            batches: 1,
            events: 1,
            evidence_refs: 1,
            evidence_registrations: 1,
            unique_evidence: 1,
            artifact_refs: 1,
            artifact_registrations: 1,
            unique_artifacts: 1,
            graph_publications: 1,
            work_units: usize::MAX,
        };
        let mut batches = LoadBudget::new(limits);
        batches.account_batch(1, 1).unwrap();
        assert!(batches.account_batch(1, 0).is_err());

        let mut events = LoadBudget::new(limits);
        events.account_batch(0, 1).unwrap();
        assert!(events.account_batch(0, 1).is_err());

        let mut evidence_refs = LoadBudget::new(limits);
        evidence_refs.account_evidence_refs(1).unwrap();
        assert!(evidence_refs.account_evidence_refs(1).is_err());

        let mut evidence_registrations = LoadBudget::new(limits);
        evidence_registrations
            .account_evidence_registrations(1)
            .unwrap();
        assert!(
            evidence_registrations
                .account_evidence_registrations(1)
                .is_err()
        );

        let mut unique_evidence = LoadBudget::new(limits);
        unique_evidence.account_unique_evidence().unwrap();
        assert!(unique_evidence.account_unique_evidence().is_err());

        let mut artifacts = LoadBudget::new(limits);
        artifacts.account_artifacts(1, 1).unwrap();
        assert!(artifacts.account_artifacts(1, 0).is_err());
        let mut artifact_registrations = LoadBudget::new(limits);
        artifact_registrations.account_artifacts(0, 1).unwrap();
        assert!(artifact_registrations.account_artifacts(0, 1).is_err());

        let mut unique_artifacts = LoadBudget::new(limits);
        unique_artifacts.account_unique_artifact().unwrap();
        assert!(unique_artifacts.account_unique_artifact().is_err());

        let mut publications = LoadBudget::new(limits);
        publications.account_graph_publication().unwrap();
        assert!(publications.account_graph_publication().is_err());

        let mut work = LoadBudget::new(LoadLimits {
            work_units: 1,
            ..limits
        });
        work.work(1).unwrap();
        assert!(work.work(1).is_err());
    }

    #[test]
    fn invalid_batch_shape_is_rejected_by_schema_before_typed_deserialization() {
        let raw = serde_json::json!({
            "formatVersion":"1.0.0",
            "requestDigest":"not-a-sha256",
            "scope":{"workspaceId":"workspace-1","projectId":"project-1","executionId":"execution-1"},
            "streamId":"stream-1",
            "expectedNextSequence":1,
            "checksum":format!("sha256:{}", "3".repeat(64)),
            "evidenceIds":[],
            "artifacts":[],
            "events":[{
                "schemaVersion":"1.0.0",
                "eventId":"event-1",
                "scope":{"workspaceId":"workspace-1","projectId":"project-1","executionId":"execution-1"},
                "streamId":"stream-1",
                "sequence":1,
                "occurredAt":"2026-08-10T12:00:00Z",
                "idempotencyKey":"request-1",
                "actor":{"type":"system","id":"system-1"},
                "sensitivity":"internal",
                "kind":{"type":"graph_imported","data":{"sourceSha256":"a".repeat(64),"sourceKind":"graph_document"}},
                "evidenceRefs":[],
                "artifactRefs":[],
                "previousHash":format!("sha256:{}", "0".repeat(64)),
                "eventHash":format!("sha256:{}", "1".repeat(64))
            }]
        });
        let bytes = canonical_bytes(&raw).unwrap();
        assert!(matches!(
            parse_physical_batch(graphhelm_schema::repository_schema_set().unwrap(), &bytes),
            Err(EventRepositoryError::Integrity)
        ));
    }

    #[test]
    fn repeated_committed_evidence_references_are_verified_once() {
        let reference = EvidenceReference::new(
            EvidenceId::parse("evidence-1").unwrap(),
            RawSha256::parse("a".repeat(64)).unwrap(),
            RawSha256::parse("b".repeat(64)).unwrap(),
        );
        let actor = PersistedActor::new(
            PersistedActorType::System,
            ActorId::parse("system-test").unwrap(),
        );
        let events = ["request-1", "request-2"]
            .into_iter()
            .map(|key| {
                NewEvent::new(
                    OpaqueId::parse(key).unwrap(),
                    actor.clone(),
                    Sensitivity::Internal,
                    EventKind::GraphImported(GraphImported {
                        source_sha256: RawSha256::parse("c".repeat(64)).unwrap(),
                        source_kind: GraphSourceKind::GraphDocument,
                    }),
                    vec![reference.clone()],
                    vec![],
                )
            })
            .collect::<Vec<_>>();
        let mut reads = 0;
        crate::validate_evidence_references(&events, &BTreeMap::new(), |_| {
            reads += 1;
            Ok(true)
        })
        .unwrap();
        assert_eq!(reads, 1);
    }

    fn invalid_graph_request() -> PreparedAppend {
        let completion = PersistedControl::new(
            SafeValue::parse("all_terminal").unwrap(),
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
        )
        .unwrap();
        let node = PersistedNode::new(
            NodeType::Tool,
            Optionality::Required,
            vec![],
            vec![],
            None,
            None,
        )
        .unwrap();
        let topology = PersistedTopology::new(
            OpaqueId::parse("graph-1").unwrap(),
            graphhelm_protocols::ExecutionId::parse("execution-1").unwrap(),
            BTreeMap::new(),
            vec![OpaqueId::parse("missing-entrypoint").unwrap()],
            BTreeMap::from([(OpaqueId::parse("node-1").unwrap(), node)]),
            vec![],
            PersistedBudgets::default(),
            vec![],
            completion,
        )
        .unwrap();
        let actor = PersistedActor::new(
            PersistedActorType::System,
            ActorId::parse("system-test").unwrap(),
        );
        let version = PersistedGraphVersion::new(
            1,
            None,
            topology,
            WireHash::parse(format!("sha256:{}", "a".repeat(64))).unwrap(),
            WireHash::parse(format!("sha256:{}", "b".repeat(64))).unwrap(),
            vec![],
            actor.clone(),
            PersistedTimestamp::from_datetime(Utc.with_ymd_and_hms(2026, 8, 10, 12, 0, 0).unwrap())
                .unwrap(),
        )
        .unwrap();
        let scope = RepositoryScope::new(
            graphhelm_protocols::WorkspaceId::parse("workspace-1").unwrap(),
            graphhelm_protocols::ProjectId::parse("project-1").unwrap(),
            Some(graphhelm_protocols::ExecutionId::parse("execution-1").unwrap()),
        );
        PreparedAppend::new(
            scope,
            OpaqueId::parse("stream-1").unwrap(),
            1,
            vec![NewEvent::new(
                OpaqueId::parse("request-1").unwrap(),
                actor,
                Sensitivity::Internal,
                EventKind::GraphVersionPublished(Box::new(GraphVersionPublished { version })),
                vec![],
                vec![],
            )],
            vec![],
            vec![],
        )
        .unwrap()
    }

    fn valid_graph_request() -> PreparedAppend {
        valid_graph_request_for(1, None, None)
    }

    fn valid_graph_request_for(
        number: u64,
        predecessor: Option<PersistedGraphVersionRef>,
        envelope_actor: Option<PersistedActor>,
    ) -> PreparedAppend {
        fn push_aad(output: &mut Vec<u8>, value: &[u8]) {
            output.extend_from_slice(&u32::try_from(value.len()).unwrap().to_be_bytes());
            output.extend_from_slice(value);
        }
        let original: PersistedGraphVersion = serde_json::from_str(include_str!(
            "../../../conformance/schemas/valid/persisted-graph-version.json"
        ))
        .unwrap();
        let scope = RepositoryScope::new(
            graphhelm_protocols::WorkspaceId::parse("workspace-1").unwrap(),
            graphhelm_protocols::ProjectId::parse("project-1").unwrap(),
            Some(graphhelm_protocols::ExecutionId::parse("execution-fixture").unwrap()),
        );
        let slots = original
            .content_slots()
            .iter()
            .map(|slot| {
                ContentSlot::new(
                    slot.slot_id().clone(),
                    slot.owner_kind(),
                    slot.owner_id().clone(),
                    slot.field_kind(),
                    slot.ordinal(),
                    graphhelm_graph::derive_publication_evidence_id(
                        &scope,
                        number,
                        original.semantic_hash(),
                        slot,
                    )
                    .unwrap(),
                    slot.content_sha256().clone(),
                    slot.sensitivity(),
                    slot.required_for_execution(),
                )
            })
            .collect::<Vec<_>>();
        let version = PersistedGraphVersion::new(
            number,
            predecessor,
            original.topology().clone(),
            original.topology_hash().clone(),
            original.semantic_hash().clone(),
            slots,
            original.created_by().clone(),
            original.created_at().clone(),
        )
        .unwrap();
        let evidence = version
            .content_slots()
            .iter()
            .enumerate()
            .map(|(index, slot)| {
                let ciphertext = vec![u8::try_from(index + 1).unwrap(); 16];
                let reference = EvidenceReference::new(
                    slot.evidence_id().clone(),
                    slot.content_sha256().clone(),
                    RawSha256::parse(sha256_hex(&ciphertext)).unwrap(),
                );
                let mut aad = Vec::new();
                push_aad(&mut aad, b"graphhelm-evidence-aad-v1");
                push_aad(&mut aad, b"workspace-1");
                push_aad(&mut aad, b"project-1");
                aad.push(1);
                push_aad(&mut aad, b"execution-fixture");
                push_aad(&mut aad, slot.evidence_id().as_str().as_bytes());
                push_aad(&mut aad, b"1.0.0");
                push_aad(&mut aad, b"application/json");
                push_aad(
                    &mut aad,
                    match slot.sensitivity() {
                        Sensitivity::Public => b"public",
                        Sensitivity::Internal => b"internal",
                        Sensitivity::Confidential => b"confidential",
                        Sensitivity::Restricted => b"restricted",
                    },
                );
                push_aad(&mut aad, b"standard");
                push_aad(&mut aad, slot.content_sha256().as_str().as_bytes());
                let wrapped = WrappedKey::new(
                    "key-1",
                    slot.evidence_id().as_str(),
                    "xchacha20poly1305",
                    vec![1; 24],
                    vec![2; 48],
                    RawSha256::parse(sha256_hex(&aad)).unwrap(),
                )
                .unwrap();
                SealedEvidence::new(
                    reference,
                    scope.clone(),
                    "application/json",
                    slot.sensitivity(),
                    "standard",
                    "xchacha20poly1305",
                    vec![3; 24],
                    ciphertext,
                    wrapped,
                )
                .unwrap()
            })
            .collect::<Vec<_>>();
        let references = evidence
            .iter()
            .map(|item| item.reference().clone())
            .collect();
        PreparedAppend::new(
            scope,
            OpaqueId::parse("stream-1").unwrap(),
            1,
            vec![NewEvent::new(
                OpaqueId::parse("request-graph-1").unwrap(),
                envelope_actor.unwrap_or_else(|| original.created_by().clone()),
                Sensitivity::Internal,
                EventKind::GraphVersionPublished(Box::new(GraphVersionPublished { version })),
                references,
                vec![],
            )],
            evidence,
            vec![],
        )
        .unwrap()
    }

    #[test]
    fn empty_repository_rejects_a_non_genesis_graph_publication() {
        let original: PersistedGraphVersion = serde_json::from_str(include_str!(
            "../../../conformance/schemas/valid/persisted-graph-version.json"
        ))
        .unwrap();
        let request = valid_graph_request_for(
            2,
            Some(PersistedGraphVersionRef::new(1, original.semantic_hash().clone()).unwrap()),
            None,
        );
        let directory = tempfile::tempdir().unwrap();
        let repository =
            LocalEventRepository::open(directory.path(), Arc::new(FixedClock), Arc::new(FixedIds))
                .unwrap();

        assert_eq!(
            repository.append_atomic(&request).unwrap_err().code(),
            "GHE004_INVALID_EVENT"
        );
        assert_eq!(
            std::fs::metadata(directory.path().join("journal.jsonl"))
                .unwrap()
                .len(),
            0
        );
    }

    #[test]
    fn graph_publication_actor_must_match_the_version_creator_on_append() {
        let request = valid_graph_request_for(
            1,
            None,
            Some(PersistedActor::new(
                PersistedActorType::Owner,
                ActorId::parse("owner-envelope-mismatch").unwrap(),
            )),
        );
        assert!(matches!(
            validate_request_preflight(&request, &LoadedState::default()),
            Err(EventRepositoryError::Invalid)
        ));
        let directory = tempfile::tempdir().unwrap();
        let repository =
            LocalEventRepository::open(directory.path(), Arc::new(FixedClock), Arc::new(FixedIds))
                .unwrap();

        assert_eq!(
            repository.append_atomic(&request).unwrap_err().code(),
            "GHE004_INVALID_EVENT"
        );
        assert_eq!(
            std::fs::metadata(directory.path().join("journal.jsonl"))
                .unwrap()
                .len(),
            0
        );
    }

    #[test]
    fn durable_load_rejects_a_rehashed_and_redigested_actor_mismatch() {
        let directory = tempfile::tempdir().unwrap();
        let repository =
            LocalEventRepository::open(directory.path(), Arc::new(FixedClock), Arc::new(FixedIds))
                .unwrap();
        let request = valid_graph_request();
        repository.append_atomic(&request).unwrap();
        drop(repository);

        let journal_path = directory.path().join("journal.jsonl");
        let line = std::fs::read(&journal_path).unwrap();
        let mut batch: PhysicalBatch = serde_json::from_slice(&line[..line.len() - 1]).unwrap();
        batch.events[0].actor = PersistedActor::new(
            PersistedActorType::Owner,
            ActorId::parse("owner-load-mismatch").unwrap(),
        );
        batch.events[0].event_hash =
            EventHash::parse(crate::compute_event_hash(&batch.events[0], GENESIS_HASH).unwrap())
                .unwrap();
        let new_events = batch
            .events
            .iter()
            .map(|event| {
                NewEvent::new(
                    event.idempotency_key.clone(),
                    event.actor.clone(),
                    event.sensitivity,
                    event.kind.clone(),
                    event.evidence_refs.clone(),
                    event.artifact_refs.clone(),
                )
            })
            .collect::<Vec<_>>();
        let evidence_digests = request.evidence().iter().map(evidence_digest).collect();
        batch.request_digest = request_digest_from_evidence_digests(
            &batch.scope,
            &OpaqueId::parse(batch.stream_id.clone()).unwrap(),
            batch.expected_next_sequence,
            &new_events,
            evidence_digests,
            batch.artifacts.clone(),
        )
        .unwrap();
        batch.checksum = batch_checksum(&batch).unwrap();
        let mut rewritten = canonical_bytes(&batch).unwrap();
        rewritten.push(b'\n');
        std::fs::write(journal_path, rewritten).unwrap();

        assert!(matches!(
            LocalEventRepository::open(directory.path(), Arc::new(FixedClock), Arc::new(FixedIds),),
            Err(EventRepositoryError::Integrity)
        ));
    }

    #[test]
    fn public_replay_rejects_a_rehashed_graph_publication_actor_mismatch() {
        let directory = tempfile::tempdir().unwrap();
        let repository =
            LocalEventRepository::open(directory.path(), Arc::new(FixedClock), Arc::new(FixedIds))
                .unwrap();
        let request = valid_graph_request();
        let mut events = repository.append_atomic(&request).unwrap();
        events[0].actor = PersistedActor::new(
            PersistedActorType::Owner,
            ActorId::parse("owner-replay-mismatch").unwrap(),
        );
        events[0].event_hash =
            EventHash::parse(crate::compute_event_hash(&events[0], GENESIS_HASH).unwrap()).unwrap();

        assert_eq!(
            crate::replay(request.scope(), request.stream_id().as_str(), &events),
            Err(crate::ReplayError::Corrupt)
        );
    }

    #[test]
    fn exact_graph_publication_retry_precedes_successor_preflight() {
        let directory = tempfile::tempdir().unwrap();
        let repository =
            LocalEventRepository::open(directory.path(), Arc::new(FixedClock), Arc::new(FixedIds))
                .unwrap();
        let request = valid_graph_request();
        let first = repository.append_atomic(&request).unwrap();
        assert_eq!(repository.append_atomic(&request).unwrap(), first);

        let crashed = tempfile::tempdir().unwrap();
        let crash_repository = LocalEventRepository::open_with_failpoint(
            crashed.path(),
            Arc::new(FixedClock),
            Arc::new(FixedIds),
            LocalFailpoint::ActiveMarker,
        )
        .unwrap();
        assert_eq!(
            crash_repository.append_atomic(&request).unwrap_err().code(),
            "GHE008_STORAGE_FAILURE"
        );
        let recovered = crash_repository.append_atomic(&request).unwrap();
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].idempotency_key.as_str(), "request-graph-1");
    }

    #[test]
    fn complete_journal_is_resynced_before_retry_or_marker_recovery() {
        let directory = tempfile::tempdir().unwrap();
        let request = valid_graph_request();
        let crash_repository = LocalEventRepository::open_with_failpoint(
            directory.path(),
            Arc::new(FixedClock),
            Arc::new(FixedIds),
            LocalFailpoint::JournalSync,
        )
        .unwrap();
        assert_eq!(
            crash_repository.append_atomic(&request).unwrap_err().code(),
            "GHE008_STORAGE_FAILURE"
        );
        assert_eq!(
            crash_repository.append_atomic(&request).unwrap_err().code(),
            "GHE008_STORAGE_FAILURE"
        );
        assert_eq!(
            std::fs::read_dir(directory.path().join("active"))
                .unwrap()
                .count(),
            0
        );
        drop(crash_repository);

        let failed_reopen = LocalEventRepository::open_with_failpoint(
            directory.path(),
            Arc::new(FixedClock),
            Arc::new(FixedIds),
            LocalFailpoint::JournalSync,
        );
        assert!(matches!(failed_reopen, Err(EventRepositoryError::Storage)));
        assert_eq!(
            std::fs::read_dir(directory.path().join("active"))
                .unwrap()
                .count(),
            0
        );

        let recovered =
            LocalEventRepository::open(directory.path(), Arc::new(FixedClock), Arc::new(FixedIds))
                .unwrap();
        assert_eq!(recovered.journal_sync_count.load(Ordering::SeqCst), 1);
        assert_eq!(
            recovered
                .active_version(request.scope(), request.stream_id().as_str())
                .unwrap()
                .unwrap()
                .number,
            1
        );
    }

    #[test]
    fn corrupt_active_marker_is_disposable_and_rebuilt_from_the_journal() {
        let directory = tempfile::tempdir().unwrap();
        let repository =
            LocalEventRepository::open(directory.path(), Arc::new(FixedClock), Arc::new(FixedIds))
                .unwrap();
        let request = valid_graph_request();
        let committed = repository.append_atomic(&request).unwrap();
        let expected = committed
            .iter()
            .find_map(|event| {
                matches!(event.kind, EventKind::GraphVersionPublished(_)).then_some(
                    StoredActiveMarker {
                        format_version: FORMAT_VERSION.into(),
                        scope: event.scope.clone(),
                        stream_id: event.stream_id.to_string(),
                        number: match &event.kind {
                            EventKind::GraphVersionPublished(value) => value.version.number(),
                            _ => unreachable!(),
                        },
                        semantic_hash: match &event.kind {
                            EventKind::GraphVersionPublished(value) => {
                                value.version.semantic_hash().to_string()
                            }
                            _ => unreachable!(),
                        },
                        sequence: event.sequence,
                        event_hash: event.event_hash.to_string(),
                    },
                )
            })
            .unwrap();
        drop(repository);
        let marker = std::fs::read_dir(directory.path().join("active"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let marker = std::fs::read_dir(marker)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        std::fs::write(&marker, b"{").unwrap();
        let journal_before = std::fs::read(directory.path().join("journal.jsonl")).unwrap();
        let reopened =
            LocalEventRepository::open(directory.path(), Arc::new(FixedClock), Arc::new(FixedIds))
                .unwrap();
        assert_eq!(
            std::fs::read(directory.path().join("journal.jsonl")).unwrap(),
            journal_before
        );
        assert_eq!(
            reopened
                .active_version(request.scope(), "stream-1")
                .unwrap()
                .unwrap()
                .number,
            expected.number
        );
        let stream_dir = std::fs::read_dir(directory.path().join("active"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert!(
            std::fs::read_dir(stream_dir)
                .unwrap()
                .filter_map(Result::ok)
                .any(|entry| {
                    serde_json::from_slice::<StoredActiveMarker>(
                        &std::fs::read(entry.path()).unwrap(),
                    )
                    .ok()
                    .as_ref()
                        == Some(&expected)
                })
        );
    }

    #[test]
    fn repeated_reopen_reuses_one_valid_repair_marker() {
        struct AdvancingIds(AtomicU64);
        impl IdGenerator for AdvancingIds {
            fn next_id(&self, prefix: &'static str) -> String {
                format!("{prefix}-{}", self.0.fetch_add(1, Ordering::SeqCst))
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let ids = Arc::new(AdvancingIds(AtomicU64::new(0)));
        let repository =
            LocalEventRepository::open(directory.path(), Arc::new(FixedClock), ids.clone())
                .unwrap();
        let request = valid_graph_request();
        repository.append_atomic(&request).unwrap();
        drop(repository);

        let stream_directory = std::fs::read_dir(directory.path().join("active"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        std::fs::write(stream_directory.join("1.json"), b"{").unwrap();

        let mut stable_count = None;
        for _ in 0..4 {
            let reopened =
                LocalEventRepository::open(directory.path(), Arc::new(FixedClock), ids.clone())
                    .unwrap();
            drop(reopened);
            let count = std::fs::read_dir(&stream_directory).unwrap().count();
            if let Some(expected) = stable_count {
                assert_eq!(
                    count, expected,
                    "unchanged journal created another repair marker"
                );
            } else {
                stable_count = Some(count);
            }
        }
    }

    #[test]
    fn oversized_active_marker_is_disposable_and_rebuilt_from_the_journal() {
        let directory = tempfile::tempdir().unwrap();
        let repository =
            LocalEventRepository::open(directory.path(), Arc::new(FixedClock), Arc::new(FixedIds))
                .unwrap();
        let request = valid_graph_request();
        let committed = repository.append_atomic(&request).unwrap();
        let expected_number = committed
            .iter()
            .find_map(|event| match &event.kind {
                EventKind::GraphVersionPublished(payload) => Some(payload.version.number()),
                _ => None,
            })
            .unwrap();
        drop(repository);

        let stream_dir = std::fs::read_dir(directory.path().join("active"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let marker = std::fs::read_dir(&stream_dir)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        std::fs::write(&marker, vec![b'x'; MAX_EVENT_BYTES + 1]).unwrap();
        let journal_before = std::fs::read(directory.path().join("journal.jsonl")).unwrap();

        let reopened =
            LocalEventRepository::open(directory.path(), Arc::new(FixedClock), Arc::new(FixedIds))
                .unwrap();
        assert_eq!(
            std::fs::read(directory.path().join("journal.jsonl")).unwrap(),
            journal_before
        );
        assert_eq!(
            reopened
                .active_version(request.scope(), "stream-1")
                .unwrap()
                .unwrap()
                .number,
            expected_number
        );
    }

    #[test]
    fn corrupt_canonical_and_deterministic_repair_markers_do_not_block_recovery() {
        struct RepairIds(AtomicU64);
        impl IdGenerator for RepairIds {
            fn next_id(&self, prefix: &'static str) -> String {
                if prefix == "active-marker-repair" {
                    let ordinal = self.0.fetch_add(1, Ordering::SeqCst);
                    if ordinal < 4 {
                        format!("collision-{ordinal}")
                    } else {
                        format!("success-{ordinal}")
                    }
                } else {
                    format!("{prefix}-safe")
                }
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let repository =
            LocalEventRepository::open(directory.path(), Arc::new(FixedClock), Arc::new(FixedIds))
                .unwrap();
        let request = valid_graph_request();
        let committed = repository.append_atomic(&request).unwrap();
        let event = &committed[0];
        let EventKind::GraphVersionPublished(payload) = &event.kind else {
            unreachable!();
        };
        let expected = StoredActiveMarker {
            format_version: FORMAT_VERSION.into(),
            scope: event.scope.clone(),
            stream_id: event.stream_id.to_string(),
            number: payload.version.number(),
            semantic_hash: payload.version.semantic_hash().to_string(),
            sequence: event.sequence,
            event_hash: event.event_hash.to_string(),
        };
        let expected_bytes = canonical_bytes(&expected).unwrap();
        drop(repository);

        let stream_dir = std::fs::read_dir(directory.path().join("active"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let canonical = stream_dir.join(format!("{}.json", expected.sequence));
        std::fs::write(&canonical, b"{").unwrap();
        let marker_digest = sha256_hex(&expected_bytes);
        for attempt in 0..16_u8 {
            let old_name = format!(
                "repair-{}.json",
                sha256_hex(
                    format!("graphhelm-active-repair-v1\0{marker_digest}\0{attempt}").as_bytes()
                )
            );
            std::fs::write(stream_dir.join(old_name), b"{").unwrap();
        }
        for ordinal in 0..4_u8 {
            let entropy = format!("collision-{ordinal}");
            let colliding_name = format!(
                "repair-{}.json",
                sha256_hex(
                    format!("graphhelm-active-repair-v2\0{marker_digest}\0{entropy}").as_bytes()
                )
            );
            std::fs::write(stream_dir.join(colliding_name), b"{").unwrap();
        }
        let journal_before = std::fs::read(directory.path().join("journal.jsonl")).unwrap();

        let reopened = LocalEventRepository::open(
            directory.path(),
            Arc::new(FixedClock),
            Arc::new(RepairIds(AtomicU64::new(0))),
        )
        .unwrap();
        assert_eq!(
            std::fs::read(directory.path().join("journal.jsonl")).unwrap(),
            journal_before
        );
        assert_eq!(
            reopened
                .active_version(request.scope(), "stream-1")
                .unwrap()
                .unwrap()
                .number,
            expected.number
        );
        assert!(
            std::fs::read_dir(stream_dir)
                .unwrap()
                .filter_map(Result::ok)
                .any(|entry| serde_json::from_slice::<StoredActiveMarker>(
                    &std::fs::read(entry.path()).unwrap()
                )
                .ok()
                .as_ref()
                    == Some(&expected))
        );
    }

    #[test]
    fn append_preflight_rejects_semantically_invalid_persisted_graph() {
        let request = invalid_graph_request();
        assert!(matches!(
            validate_request_preflight(&request, &LoadedState::default()),
            Err(EventRepositoryError::Invalid)
        ));
    }

    #[test]
    fn append_preflight_rejects_divergent_graph_lineage_before_persistence() {
        let active: PersistedGraphVersion = serde_json::from_str(include_str!(
            "../../../conformance/schemas/valid/persisted-graph-version.json"
        ))
        .unwrap();
        let divergent = PersistedGraphVersion::new(
            active.number() + 1,
            Some(
                PersistedGraphVersionRef::new(
                    active.number(),
                    WireHash::parse(format!("sha256:{}", "f".repeat(64))).unwrap(),
                )
                .unwrap(),
            ),
            active.topology().clone(),
            active.topology_hash().clone(),
            active.semantic_hash().clone(),
            active.content_slots().to_vec(),
            active.created_by().clone(),
            active.created_at().clone(),
        )
        .unwrap();
        assert!(graphhelm_graph::validate_persisted_projection(&divergent).is_ok());
        let scope = RepositoryScope::new(
            graphhelm_protocols::WorkspaceId::parse("workspace-1").unwrap(),
            graphhelm_protocols::ProjectId::parse("project-1").unwrap(),
            Some(graphhelm_protocols::ExecutionId::parse("execution-1").unwrap()),
        );
        let stream_id = OpaqueId::parse("stream-1").unwrap();
        let references = divergent
            .content_slots()
            .iter()
            .map(|slot| {
                EvidenceReference::new(
                    slot.evidence_id().clone(),
                    slot.content_sha256().clone(),
                    RawSha256::parse("0".repeat(64)).unwrap(),
                )
            })
            .collect();
        let request = PreparedAppend::new(
            scope.clone(),
            stream_id.clone(),
            1,
            vec![NewEvent::new(
                OpaqueId::parse("request-1").unwrap(),
                active.created_by().clone(),
                Sensitivity::Internal,
                EventKind::GraphVersionPublished(Box::new(GraphVersionPublished {
                    version: divergent,
                })),
                references,
                vec![],
            )],
            vec![],
            vec![],
        )
        .unwrap();
        let mut state = LoadedState::default();
        state.active_versions.insert(
            stream_key(&scope, stream_id.as_str()).unwrap(),
            ActiveVersion {
                number: active.number(),
                semantic_hash: active.semantic_hash().to_string(),
                sequence: 1,
                event_hash: GENESIS_HASH.into(),
            },
        );

        assert!(matches!(
            validate_request_preflight(&request, &state),
            Err(EventRepositoryError::Invalid)
        ));
    }

    struct FixedClock;

    impl Clock for FixedClock {
        fn now(&self) -> chrono::DateTime<Utc> {
            Utc.with_ymd_and_hms(2026, 8, 10, 12, 0, 0).unwrap()
        }
    }

    struct FixedIds;

    impl IdGenerator for FixedIds {
        fn next_id(&self, prefix: &'static str) -> String {
            format!("{prefix}-1")
        }
    }

    #[test]
    fn fixed_id_source_still_allocates_distinct_bounded_temp_names() {
        let directory = tempfile::tempdir().unwrap();
        let repository =
            LocalEventRepository::open(directory.path(), Arc::new(FixedClock), Arc::new(FixedIds))
                .unwrap();
        let (first, first_file) = repository.create_unique_temp("active").unwrap();
        let (second, second_file) = repository.create_unique_temp("active").unwrap();
        assert_ne!(first, second);
        assert!(is_owned_temp_name(&first));
        assert!(is_owned_temp_name(&second));
        drop((first_file, second_file));
    }

    #[test]
    fn temp_allocator_retries_a_preexisting_fixed_id_collision() {
        let directory = tempfile::tempdir().unwrap();
        let repository =
            LocalEventRepository::open(directory.path(), Arc::new(FixedClock), Arc::new(FixedIds))
                .unwrap();
        let (first, first_file) = repository.create_unique_temp("active").unwrap();
        drop(first_file);
        repository.temp_counter.store(0, Ordering::SeqCst);
        let (second, second_file) = repository.create_unique_temp("active").unwrap();
        assert_ne!(first, second);
        drop(second_file);
    }

    #[test]
    fn next_sequence_performs_one_bounded_state_load() {
        let directory = tempfile::tempdir().unwrap();
        let repository =
            LocalEventRepository::open(directory.path(), Arc::new(FixedClock), Arc::new(FixedIds))
                .unwrap();
        let before = repository.load_count.load(Ordering::SeqCst);
        assert_eq!(
            repository
                .next_sequence(
                    &RepositoryScope::new(
                        graphhelm_protocols::WorkspaceId::parse("workspace-1").unwrap(),
                        graphhelm_protocols::ProjectId::parse("project-1").unwrap(),
                        Some(graphhelm_protocols::ExecutionId::parse("execution-1").unwrap(),),
                    ),
                    "stream-1",
                )
                .unwrap(),
            1
        );
        assert_eq!(repository.load_count.load(Ordering::SeqCst) - before, 1);
    }

    #[test]
    fn preserved_crash_temp_never_bricks_a_later_active_marker_allocation() {
        let directory = tempfile::tempdir().unwrap();
        let repository =
            LocalEventRepository::open(directory.path(), Arc::new(FixedClock), Arc::new(FixedIds))
                .unwrap();
        let (crash_name, mut crash_file) = repository.create_unique_temp("active").unwrap();
        crash_file.write_all(b"interrupted").unwrap();
        crash_file.sync_all().unwrap();
        drop((crash_file, repository));

        let reopened =
            LocalEventRepository::open(directory.path(), Arc::new(FixedClock), Arc::new(FixedIds))
                .unwrap();
        let (next_name, next_file) = reopened.create_unique_temp("active").unwrap();
        if cfg!(unix) {
            assert_ne!(next_name, crash_name);
        }
        drop(next_file);
    }

    #[test]
    fn root_initialization_rejects_a_link_swapped_between_component_create_and_open() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("nested-root");
        let displaced = directory.path().join("displaced-root");
        let outside = directory.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("canary"), b"outside").unwrap();
        let mut callback_seen = false;
        let mut attacked = false;
        let mut anchored_denial = false;
        let result = ensure_root_path_observed(&root, &mut |created| {
            callback_seen = true;
            if attacked {
                return;
            }
            if let Err(error) = std::fs::rename(created, &displaced) {
                if cfg!(windows) && matches!(error.raw_os_error(), Some(5 | 32)) {
                    anchored_denial = true;
                    return;
                }
                panic!("failed to displace root component: {error}");
            }
            match create_test_directory_link(&outside, created) {
                Ok(()) => attacked = true,
                Err(error) if cfg!(windows) && error.raw_os_error() == Some(1314) => {
                    std::fs::rename(&displaced, created).unwrap();
                }
                Err(error) => panic!("failed to create root swap: {error}"),
            }
        });
        assert!(callback_seen, "root component creation was not observed");
        if cfg!(windows) {
            assert!(
                anchored_denial,
                "the retained parent allowed a component swap"
            );
            assert!(result.is_ok());
            assert_eq!(std::fs::read(outside.join("canary")).unwrap(), b"outside");
            return;
        }
        if !attacked {
            return;
        }
        assert!(result.is_err());
        assert_eq!(std::fs::read(outside.join("canary")).unwrap(), b"outside");
        assert!(!outside.join("format.json").exists());
    }

    #[cfg(unix)]
    fn create_test_directory_link(target: &Path, link: &Path) -> std::io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }

    #[cfg(windows)]
    fn create_test_directory_link(target: &Path, link: &Path) -> std::io::Result<()> {
        std::os::windows::fs::symlink_dir(target, link)
    }

    #[cfg(unix)]
    #[test]
    fn unix_normal_temp_cleanup_preserves_the_validated_inode() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("objects");
        std::fs::create_dir(&path).unwrap();
        let directory_handle = open_directory(&path).unwrap();
        std::fs::write(path.join("active-safe.tmp"), b"validated").unwrap();
        let retained =
            open_child_file_for_delete(&directory_handle, &path, "active-safe.tmp").unwrap();
        remove_planned_file(&directory_handle, &path, "active-safe.tmp", retained).unwrap();
        assert_eq!(
            std::fs::read(path.join("active-safe.tmp")).unwrap(),
            b"validated"
        );
    }

    fn artifact_request(expected: u64, key: &str, digest: char) -> PreparedAppend {
        let digest = digest.to_string().repeat(64);
        let reference = ArtifactReference::new(
            ArtifactId::parse("artifact-1").unwrap(),
            ArtifactLocator::parse(format!("artifact://sha256/{digest}")).unwrap(),
            RawSha256::parse(digest).unwrap(),
            MediaType::parse("application/json").unwrap(),
            7,
            Sensitivity::Internal,
            SemanticVersion::parse("1.0.0").unwrap(),
        )
        .unwrap();
        let scope = RepositoryScope::new(
            graphhelm_protocols::WorkspaceId::parse("workspace-1").unwrap(),
            graphhelm_protocols::ProjectId::parse("project-1").unwrap(),
            Some(graphhelm_protocols::ExecutionId::parse("execution-1").unwrap()),
        );
        let actor = PersistedActor::new(
            PersistedActorType::System,
            ActorId::parse("system-test").unwrap(),
        );
        PreparedAppend::new(
            scope,
            OpaqueId::parse("stream-1").unwrap(),
            expected,
            vec![NewEvent::new(
                OpaqueId::parse(key).unwrap(),
                actor,
                Sensitivity::Internal,
                EventKind::GraphImported(GraphImported {
                    source_sha256: RawSha256::parse("c".repeat(64)).unwrap(),
                    source_kind: GraphSourceKind::GraphDocument,
                }),
                vec![],
                vec![reference.clone()],
            )],
            vec![],
            vec![crate::ArtifactRegistration::new(reference, key).unwrap()],
        )
        .unwrap()
    }

    #[test]
    fn durable_load_rejects_a_divergent_artifact_catalog_entry() {
        let directory = tempfile::tempdir().unwrap();
        let repository =
            LocalEventRepository::open(directory.path(), Arc::new(FixedClock), Arc::new(FixedIds))
                .unwrap();
        repository
            .append_atomic(&artifact_request(1, "producer-1", 'a'))
            .unwrap();
        let state = repository
            .load_state("durable_load_rejects_a_divergent_artifact_catalog_entry")
            .unwrap();
        let divergent = artifact_request(2, "producer-2", 'b');
        let events = repository.build_envelopes(&divergent, &state).unwrap();
        let mut batch = PhysicalBatch {
            format_version: FORMAT_VERSION.into(),
            request_digest: crate::prepared_append_digest(&divergent).unwrap(),
            scope: divergent.scope().clone(),
            stream_id: divergent.stream_id().to_string(),
            expected_next_sequence: 2,
            checksum: String::new(),
            evidence_ids: vec![],
            artifacts: divergent
                .artifacts()
                .iter()
                .map(|item| StoredArtifactRegistration {
                    reference: item.reference().clone(),
                    producer_stream_id: divergent.stream_id().to_string(),
                    producer_idempotency_key: item.producer_idempotency_key().to_string(),
                })
                .collect(),
            events,
        };
        batch.checksum = batch_checksum(&batch).unwrap();
        let mut line = canonical_bytes(&batch).unwrap();
        line.push(b'\n');
        drop(repository);
        std::fs::OpenOptions::new()
            .append(true)
            .open(directory.path().join("journal.jsonl"))
            .unwrap()
            .write_all(&line)
            .unwrap();

        assert!(matches!(
            LocalEventRepository::open(directory.path(), Arc::new(FixedClock), Arc::new(FixedIds),),
            Err(EventRepositoryError::Integrity)
        ));
    }

    #[test]
    fn durable_load_rejects_semantically_invalid_persisted_graph() {
        let directory = tempfile::tempdir().unwrap();
        let repository =
            LocalEventRepository::open(directory.path(), Arc::new(FixedClock), Arc::new(FixedIds))
                .unwrap();
        let request = invalid_graph_request();
        let mut batch = PhysicalBatch {
            format_version: FORMAT_VERSION.into(),
            request_digest: crate::prepared_append_digest(&request).unwrap(),
            scope: request.scope().clone(),
            stream_id: request.stream_id().to_string(),
            expected_next_sequence: request.expected_next_sequence(),
            checksum: String::new(),
            evidence_ids: vec![],
            artifacts: vec![],
            events: repository
                .build_envelopes(&request, &LoadedState::default())
                .unwrap(),
        };
        batch.checksum = batch_checksum(&batch).unwrap();
        let mut bytes = canonical_bytes(&batch).unwrap();
        bytes.push(b'\n');
        drop(repository);
        std::fs::write(directory.path().join("journal.jsonl"), bytes).unwrap();

        assert!(matches!(
            LocalEventRepository::open(directory.path(), Arc::new(FixedClock), Arc::new(FixedIds),),
            Err(EventRepositoryError::Integrity)
        ));
    }

    // ------------------------------------------------------------------------------------
    // #87: the verified-prefix cache. Naming convention: the guard says what the cache
    // must and must not change. Every "same error as a fresh handle" assertion compares
    // against an actual fresh open on the same directory, so today's behavior is the
    // oracle rather than a hand-written expectation.
    // ------------------------------------------------------------------------------------

    fn wake_scope() -> RepositoryScope {
        RepositoryScope::new(
            graphhelm_protocols::WorkspaceId::parse("workspace-1").unwrap(),
            graphhelm_protocols::ProjectId::parse("project-1").unwrap(),
            Some(graphhelm_protocols::ExecutionId::parse("execution-fixture").unwrap()),
        )
    }

    fn wake_append(sequence: u64, key: &str) -> PreparedAppend {
        let actor = PersistedActor::new(
            PersistedActorType::System,
            ActorId::parse("system-test").unwrap(),
        );
        PreparedAppend::new(
            wake_scope(),
            OpaqueId::parse("stream-1").unwrap(),
            sequence,
            vec![NewEvent::new(
                OpaqueId::parse(key).unwrap(),
                actor,
                Sensitivity::Internal,
                EventKind::WakeLease(graphhelm_protocols::WakeLease {
                    execution_id: OpaqueId::parse("execution-fixture").unwrap(),
                    session_id: OpaqueId::parse(format!("session-{sequence}")).unwrap(),
                    cursor: 1,
                    rendezvous_id: OpaqueId::parse(format!("rdv-{sequence}")).unwrap(),
                    matures_in_seconds: None,
                }),
                vec![],
                vec![],
            )],
            vec![],
            vec![],
        )
        .unwrap()
    }

    fn cache_repository(directory: &std::path::Path) -> LocalEventRepository {
        LocalEventRepository::open(directory, Arc::new(FixedClock), Arc::new(FixedIds)).unwrap()
    }

    fn counters(repository: &LocalEventRepository) -> (u64, u64) {
        (
            repository.full_load_count.load(Ordering::SeqCst),
            repository.suffix_load_count.load(Ordering::SeqCst),
        )
    }

    #[test]
    fn a_second_read_reuses_the_verified_prefix_instead_of_reloading() {
        let directory = tempfile::tempdir().unwrap();
        let repository = cache_repository(directory.path());
        repository.append_atomic(&valid_graph_request()).unwrap();
        let scope = wake_scope();
        assert_eq!(repository.next_sequence(&scope, "stream-1").unwrap(), 2);
        let warmed = counters(&repository);
        // POSITIVE CONTROL (M's sealed requirement): a broken counter that always reads
        // zero must not be able to confirm this guard — the open itself was a full load,
        // so the full counter has provably MOVED before the zero-delta claim below.
        assert!(warmed.0 >= 1, "the full counter never moved: {warmed:?}");
        assert_eq!(repository.next_sequence(&scope, "stream-1").unwrap(), 2);
        // The second read paid NEITHER a full load NOR a suffix load: pure cache hit.
        assert_eq!(counters(&repository), warmed);
        // Per-KIND accounting (design section 2: the metric names WHICH operation
        // paid). Exact derivation for this scenario: the open paid the one full load;
        // the append hit the open's cache; the FIRST next_sequence paid one suffix
        // (verifying the append's line); the second was a pure hit.
        by_kind_probe(&repository, &scope);
        let by_kind = repository.loads_by_kind.lock().unwrap();
        assert_eq!(by_kind.get("open"), Some(&(1, 0, 0)));
        assert_eq!(by_kind.get("append"), Some(&(0, 0, 1)));
        // NOTE: per-kind triples are AGGREGATED ACROSS THE SCENARIO, not per call —
        // (0, 1, 1) is two next_sequence calls summed (the first paid the append's
        // suffix, the second was a pure hit).
        assert_eq!(by_kind.get("next_sequence"), Some(&(0, 1, 1)));
        // Label verification for the read-path stamps (M's L5: an asserted-by-hand
        // stamp is checked by nothing until a guard exercises it): each op ran exactly
        // once on the warm, quiescent handle, so each label must show one pure hit.
        for kind in [
            "read_stream",
            "read_replay_stream",
            "read_unique_replay_stream",
            "list_streams",
            "active_version",
            "evidence_exists",
            "artifact_exists",
        ] {
            assert_eq!(
                by_kind.get(kind),
                Some(&(0, 0, 1)),
                "label {kind} did not record exactly one pure hit"
            );
        }
    }

    /// Exercises each read operation ONCE on a warm handle, so its kind label is
    /// verified by observation instead of asserted by hand (M's L5).
    fn by_kind_probe(repository: &LocalEventRepository, scope: &RepositoryScope) {
        let _ = repository.read_stream(scope, "stream-1", 10, None);
        let _ = repository.read_replay_stream(scope, "stream-1");
        let _ = repository.read_unique_replay_stream();
        let _ = repository.list_streams();
        let _ = repository.active_version(scope, "stream-1");
        let _ = repository.evidence_exists(scope, &EvidenceId::parse("evidence-none").unwrap());
        let _ = EventRepository::artifact_exists(
            repository,
            scope,
            &ArtifactId::parse("artifact-none").unwrap(),
        );
    }

    #[test]
    fn an_append_is_verified_as_a_suffix_never_as_a_reload() {
        let directory = tempfile::tempdir().unwrap();
        let repository = cache_repository(directory.path());
        repository.append_atomic(&valid_graph_request()).unwrap();
        let scope = wake_scope();
        assert_eq!(repository.next_sequence(&scope, "stream-1").unwrap(), 2);
        let (full_before, _) = counters(&repository);
        repository
            .append_atomic(&wake_append(2, "wake-append-1"))
            .unwrap();
        assert_eq!(repository.next_sequence(&scope, "stream-1").unwrap(), 3);
        let (full_after, suffix_after) = counters(&repository);
        // The appended line was verified by SUFFIX reads only; the full count is frozen.
        // The suffix counter having MOVED is this guard's positive control: a dead pair
        // of counters cannot fake the frozen-full claim.
        assert_eq!(full_after, full_before);
        assert!(suffix_after >= 1, "the suffix counter never moved");
    }

    #[test]
    fn a_rival_handles_append_is_seen_by_the_cached_handle() {
        let directory = tempfile::tempdir().unwrap();
        let cached = cache_repository(directory.path());
        cached.append_atomic(&valid_graph_request()).unwrap();
        let scope = wake_scope();
        assert_eq!(cached.next_sequence(&scope, "stream-1").unwrap(), 2);
        // A SECOND handle appends — the cross-handle shape the per-request serve uses.
        let rival = cache_repository(directory.path());
        rival
            .append_atomic(&wake_append(2, "wake-rival-1"))
            .unwrap();
        // The cached handle's next read must see the rival's append (length grew, the
        // suffix is verified from the cached chain) — no stale answer, no full reload.
        let (full_before, suffix_before) = counters(&cached);
        assert_eq!(cached.next_sequence(&scope, "stream-1").unwrap(), 3);
        let (full_after, suffix_after) = counters(&cached);
        assert_eq!(full_after, full_before);
        assert_eq!(suffix_after, suffix_before + 1);
    }

    #[test]
    fn a_corrupt_suffix_fails_the_cached_handle_exactly_as_a_fresh_one() {
        let directory = tempfile::tempdir().unwrap();
        let repository = cache_repository(directory.path());
        repository.append_atomic(&valid_graph_request()).unwrap();
        let scope = wake_scope();
        assert_eq!(repository.next_sequence(&scope, "stream-1").unwrap(), 2);
        let journal = directory.path().join("journal.jsonl");
        let mut bytes = std::fs::read(&journal).unwrap();
        bytes.extend_from_slice(b"{\"garbage\":true}\n");
        std::fs::write(&journal, bytes).unwrap();
        let cached_error = repository.next_sequence(&scope, "stream-1").unwrap_err();
        let fresh_error = match LocalEventRepository::open(
            directory.path(),
            Arc::new(FixedClock),
            Arc::new(FixedIds),
        ) {
            Err(error) => error,
            Ok(_) => panic!("a fresh open must refuse the corrupt journal"),
        };
        assert_eq!(
            std::mem::discriminant(&cached_error),
            std::mem::discriminant(&fresh_error),
            "cached: {cached_error:?}, fresh: {fresh_error:?}"
        );
    }

    #[test]
    fn a_chain_break_in_the_suffix_is_judged_against_the_cached_hashes() {
        // SC6: the suffix verifier must chain from the CACHE. Sabotage the cached
        // last_hash directly — a LEGITIMATE next append must then fail verification on
        // the cached handle, proving the chain check reads the cache and not a re-read.
        let directory = tempfile::tempdir().unwrap();
        let repository = cache_repository(directory.path());
        repository.append_atomic(&valid_graph_request()).unwrap();
        let scope = wake_scope();
        assert_eq!(repository.next_sequence(&scope, "stream-1").unwrap(), 2);
        {
            // NOTE (D's review): if the cache were ever REMOVED, this test dies here in
            // SETUP (`expect`), not in its own assertion — a setup-panic red means "the
            // seam is gone", not "the guard caught the sabotage". Read the panic site.
            let mut verified = repository.verified.lock().unwrap();
            let prefix = verified.as_mut().expect("the cache is warm");
            let state = Arc::make_mut(&mut prefix.state);
            let key = stream_key(&scope, "stream-1").unwrap();
            state
                .last_hash
                .insert(key, format!("sha256:{}", "e".repeat(64)));
        }
        // Append THROUGH A RIVAL handle so the journal itself stays legitimate.
        let rival = cache_repository(directory.path());
        rival
            .append_atomic(&wake_append(2, "wake-chain-1"))
            .unwrap();
        assert!(
            repository.next_sequence(&scope, "stream-1").is_err(),
            "a suffix verified against a sabotaged cached hash must fail — a pass here \
             would mean the verifier re-derives the chain instead of using the cache"
        );
    }

    #[test]
    fn a_mid_line_truncation_fails_and_a_line_boundary_truncation_reloads_shorter() {
        let directory = tempfile::tempdir().unwrap();
        let repository = cache_repository(directory.path());
        repository.append_atomic(&valid_graph_request()).unwrap();
        repository
            .append_atomic(&wake_append(2, "wake-trunc-1"))
            .unwrap();
        let scope = wake_scope();
        assert_eq!(repository.next_sequence(&scope, "stream-1").unwrap(), 3);
        let journal = directory.path().join("journal.jsonl");
        let full = std::fs::read(&journal).unwrap();
        let first_line_end = full.iter().position(|byte| *byte == b'\n').unwrap() + 1;

        // Mid-line truncation: the cached handle must fail, as a fresh one would.
        std::fs::write(&journal, &full[..full.len() - 3]).unwrap();
        assert!(repository.next_sequence(&scope, "stream-1").is_err());

        // Line-boundary truncation: EXPECTED GREEN by the #87 ruling — the cached
        // handle drops its prefix (the file shrank) and reloads the shorter history,
        // byte-for-byte today's behavior. Hardening this is a separate decision.
        std::fs::write(&journal, &full[..first_line_end]).unwrap();
        assert_eq!(repository.next_sequence(&scope, "stream-1").unwrap(), 2);
    }

    #[test]
    fn prefix_corruption_is_invisible_to_a_warm_handle_and_fatal_to_a_fresh_one() {
        // SC1, BOTH halves committed: the honest widening (R1) documented as a pair of
        // assertions instead of hidden. A warm handle does not re-read its verified
        // prefix, so an in-place corruption of already-verified bytes passes it
        // (EXPECTED GREEN); the same corruption fails a fresh open exactly as today.
        let directory = tempfile::tempdir().unwrap();
        let repository = cache_repository(directory.path());
        repository.append_atomic(&valid_graph_request()).unwrap();
        repository
            .append_atomic(&wake_append(2, "wake-prefix-1"))
            .unwrap();
        let scope = wake_scope();
        assert_eq!(repository.next_sequence(&scope, "stream-1").unwrap(), 3);
        let journal = directory.path().join("journal.jsonl");
        let mut bytes = std::fs::read(&journal).unwrap();
        bytes[10] = bytes[10].wrapping_add(1);
        std::fs::write(&journal, bytes).unwrap();
        assert_eq!(
            repository.next_sequence(&scope, "stream-1").unwrap(),
            3,
            "EXPECTED GREEN: the warm handle trusts its verified prefix"
        );
        assert!(
            LocalEventRepository::open(directory.path(), Arc::new(FixedClock), Arc::new(FixedIds))
                .is_err(),
            "the fresh open is where prefix corruption is caught, exactly as today"
        );
    }

    #[test]
    fn incremental_budget_accounting_equals_the_from_zero_accounting() {
        // SC7: limits must be path-independent — the budget a handle accumulated
        // through suffix loads must equal the budget a fresh handle computes from zero
        // over the same journal, on EVERY counter.
        let directory = tempfile::tempdir().unwrap();
        let incremental = cache_repository(directory.path());
        incremental.append_atomic(&valid_graph_request()).unwrap();
        let scope = wake_scope();
        for sequence in 2..=6 {
            assert_eq!(
                incremental.next_sequence(&scope, "stream-1").unwrap(),
                sequence
            );
            incremental
                .append_atomic(&wake_append(sequence, &format!("wake-budget-{sequence}")))
                .unwrap();
        }
        assert_eq!(incremental.next_sequence(&scope, "stream-1").unwrap(), 7);
        let from_zero = cache_repository(directory.path());
        assert_eq!(from_zero.next_sequence(&scope, "stream-1").unwrap(), 7);
        let lhs_guard = incremental.verified.lock().unwrap();
        let rhs_guard = from_zero.verified.lock().unwrap();
        let lhs_prefix = lhs_guard.as_ref().expect("warm");
        let rhs_prefix = rhs_guard.as_ref().expect("warm");
        // P6 (M's sealed cell): the HOT state must equal the from-scratch state BY
        // VALUE — a cache can be fast, count right, error right, and still serve stale
        // data; this row is the one that catches that. WHOLE-STRUCT equality (D's
        // review): a hand-enumerated field list stops protecting the moment someone
        // adds a field; the derive makes the omission impossible instead of remembered.
        assert_eq!(*lhs_prefix.state, *rhs_prefix.state);
        assert_eq!(lhs_prefix.verified_offset, rhs_prefix.verified_offset);
        let lhs = &lhs_prefix.budget;
        let rhs = &rhs_prefix.budget;
        // POSITIVE CONTROL: the equality below is vacuous if the fixture consumed no
        // budget — prove the counters are non-zero before claiming they are equal.
        assert!(lhs.batches > 0 && lhs.events > 0 && lhs.work_units > 0);
        // Whole-struct equality for the same reason as the state compare above; the
        // tuple enumeration this replaces would have silently exempted a future field.
        assert_eq!(lhs, rhs);
    }

    /// #87 measurement harness (run explicitly: `cargo test -p graphhelm-events --lib
    /// measure_87 -- --ignored --nocapture`). Uses ONLY `load_count` and public ops so
    /// the IDENTICAL code runs on the baseline tree (which lacks the cache and its
    /// counters) — the paired before/after comparison is same-harness by construction.
    /// Prints one row per (size, op, iteration): wall micros + load_state calls.
    #[test]
    #[ignore]
    fn measure_87_paired_rows() {
        let iterations = 10usize;
        for size in [100u64, 1000, 5000] {
            let directory = tempfile::tempdir().unwrap();
            let build = cache_repository(directory.path());
            build.append_atomic(&valid_graph_request()).unwrap();
            let scope = wake_scope();
            for sequence in 2..=size {
                build
                    .append_atomic(&wake_append(sequence, &format!("wake-m-{sequence}")))
                    .unwrap();
            }
            drop(build);
            // open: cold handle, N iterations
            for i in 0..iterations {
                let t = std::time::Instant::now();
                let repository = cache_repository(directory.path());
                let wall = t.elapsed().as_micros();
                let loads = repository.load_count.load(Ordering::SeqCst);
                println!("ROW size={size} op=open iter={i} wall_us={wall} loads={loads}");
            }
            // warm handle: repeated next_sequence
            let warm = cache_repository(directory.path());
            let _ = warm.next_sequence(&scope, "stream-1").unwrap();
            for i in 0..iterations {
                let before = warm.load_count.load(Ordering::SeqCst);
                let t = std::time::Instant::now();
                let head = warm.next_sequence(&scope, "stream-1").unwrap();
                let wall = t.elapsed().as_micros();
                let loads = warm.load_count.load(Ordering::SeqCst) - before;
                println!(
                    "ROW size={size} op=next_sequence_warm iter={i} wall_us={wall} \
                     loads={loads} head={head}"
                );
            }
            // warm handle: read_replay_stream
            for i in 0..iterations {
                let before = warm.load_count.load(Ordering::SeqCst);
                let t = std::time::Instant::now();
                let events = warm.read_replay_stream(&scope, "stream-1").unwrap();
                let wall = t.elapsed().as_micros();
                let loads = warm.load_count.load(Ordering::SeqCst) - before;
                println!(
                    "ROW size={size} op=read_replay_warm iter={i} wall_us={wall} \
                     loads={loads} events={}",
                    events.len()
                );
            }
            // append on the warm handle (mutates: sequence advances per iteration)
            for i in 0..iterations {
                let sequence = size + 1 + i as u64;
                let request = wake_append(sequence, &format!("wake-a-{sequence}"));
                let before = warm.load_count.load(Ordering::SeqCst);
                let t = std::time::Instant::now();
                warm.append_atomic(&request).unwrap();
                let wall = t.elapsed().as_micros();
                let loads = warm.load_count.load(Ordering::SeqCst) - before;
                println!("ROW size={size} op=append_warm iter={i} wall_us={wall} loads={loads}");
            }
        }
    }

    // --------------------------------------------------------------------------------
    // #143: the shared fast path for clean opens (readers must not wait for readers).
    // --------------------------------------------------------------------------------

    #[test]
    fn a_clean_store_opens_on_the_shared_fast_path() {
        let directory = tempfile::tempdir().unwrap();
        {
            let bootstrap = cache_repository(directory.path());
            bootstrap.append_atomic(&valid_graph_request()).unwrap();
            // The bootstrap open created the layout (exclusive by necessity).
            assert_eq!(bootstrap.shared_fast_open_count.load(Ordering::SeqCst), 0);
        }
        let reopened = cache_repository(directory.path());
        assert_eq!(
            reopened.shared_fast_open_count.load(Ordering::SeqCst),
            1,
            "a complete, clean store must open on the shared fast path"
        );
        // And the fast path still produced a fully usable, correct view.
        assert_eq!(
            reopened.next_sequence(&wake_scope(), "stream-1").unwrap(),
            2
        );
    }

    #[test]
    fn a_missing_marker_forces_the_exclusive_upgrade_and_is_republished() {
        let directory = tempfile::tempdir().unwrap();
        let scope = wake_scope();
        {
            let bootstrap = cache_repository(directory.path());
            bootstrap.append_atomic(&valid_graph_request()).unwrap();
        }
        let stream_name = object_key(&scope, "stream-1").unwrap();
        let marker = directory
            .path()
            .join("active")
            .join(&stream_name)
            .join("1.json");
        assert!(marker.exists(), "the publish left a marker to delete");
        std::fs::remove_file(&marker).unwrap();

        let reopened = cache_repository(directory.path());
        // Dirty store: the fast path must have DECLINED (counter untouched) and the
        // exclusive upgrade republished the marker exactly as today's recovery does.
        assert_eq!(
            reopened.shared_fast_open_count.load(Ordering::SeqCst),
            0,
            "a store with recovery work must not count as a fast open"
        );
        assert!(
            marker.exists(),
            "the upgrade path must republish the missing marker"
        );
    }
}
