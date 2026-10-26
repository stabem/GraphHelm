//! #1721 step 2: projection checkpoints at seals, so a reader of one stream never replays it
//! from genesis.
//!
//! `segments/<n>.<sha>.checkpoint.json` holds one stream's [`ProjectionGeneration`] folded to
//! the end of sealed segment `n`, bound to that seal by its digest; `<sha>` is the SHA-256 of
//! the stream key, so the name carries no caller text. A checkpoint is DERIVED and disposable:
//! it is written by the reader that first folds past the seal, never by an append, and deleting
//! every checkpoint only makes the next read fold the sealed segments again, one segment at a
//! time, from the events. A checkpoint that does not decode, is not canonical, names another
//! seal or stream, or does not end exactly at the seal's head for its stream is ignored, never
//! trusted; the fold that resumes from it still checks every later event's sequence and its
//! hash link to the checkpoint's last event.
//!
//! What one read walks is the sealed segments past the newest usable checkpoint plus the active
//! segment, so a stream far past `MAX_READ_ALL` is answered once its checkpoints exist, and
//! building them holds one sealed segment in memory at a time.

use super::{
    EventEnvelope, EventRepositoryError, File, LocalEventRepository, MAX_SEAL_STATE_BYTES,
    RepositoryScope, SEGMENTS_DIR, SealedSegment, canonical_bytes, open_child_directory,
    open_child_file, read_bounded_file, stream_key, wire_sha256,
};
use crate::limits::MAX_READ_PAGE;
use crate::projection::{ExecutionProjection, ProjectionGeneration, map_replay_error};
use serde::{Deserialize, Serialize};

const CHECKPOINT_FORMAT: &str = "graphhelm.projection-checkpoint/1";
const CHECKPOINT_PROJECTION: &str = "seal-checkpoint";

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProjectionCheckpoint {
    format: String,
    index: u64,
    seal_sha256: String,
    stream_key: String,
    generation: ProjectionGeneration,
}

/// `<index>.<sha256 of the stream key>.checkpoint.json`.
pub(super) fn checkpoint_file_name(index: u64, stream_key: &str) -> String {
    let digest = wire_sha256(stream_key.as_bytes());
    let hex = digest.strip_prefix("sha256:").unwrap_or(&digest);
    format!("{index:08}.{hex}.checkpoint.json")
}

impl LocalEventRepository {
    /// The projection of one stream and the sequence it was folded to, resumed from the newest
    /// seal checkpoint of that stream instead of replayed from its first event (#1721).
    ///
    /// Equal to `replay` over [`Self::read_replay_stream`] whenever that read answers; unlike it,
    /// this one answers for a stream longer than `MAX_READ_ALL`. Checkpoints missing on disk are
    /// written as the fold passes each seal; a failure to write one never fails the read.
    pub fn project_stream(
        &self,
        scope: &RepositoryScope,
        stream_id: &str,
    ) -> Result<(ExecutionProjection, u64), EventRepositoryError> {
        graphhelm_protocols::OpaqueId::parse(stream_id)
            .map_err(|_| EventRepositoryError::Invalid)?;
        self.with_shared_lock(|| {
            let state = self.load_state("project_stream")?;
            let key = stream_key(scope, stream_id)?;
            let range = |position: usize| -> (u64, u64) {
                let start = position
                    .checked_sub(1)
                    .and_then(|previous| state.sealed[previous].seal.next_sequence.get(&key))
                    .copied()
                    .unwrap_or(1);
                let end = state.sealed[position]
                    .seal
                    .next_sequence
                    .get(&key)
                    .copied()
                    .unwrap_or(1);
                (start, end)
            };
            let holds_stream = |position: usize| {
                let (start, end) = range(position);
                end > start
            };

            // The newest seal holding the stream that has a usable checkpoint; the fold goes
            // forward from there, one sealed segment at a time.
            let resumed = (0..state.sealed.len())
                .rev()
                .filter(|position| holds_stream(*position))
                .find_map(|position| {
                    self.load_checkpoint(&state.sealed[position], &key, range(position).1)
                        .map(|generation| (position + 1, generation))
                });
            let (first, mut generation) = match resumed {
                Some(found) => found,
                None => (
                    0,
                    ProjectionGeneration::new(
                        scope.clone(),
                        stream_id.to_owned(),
                        CHECKPOINT_PROJECTION.to_owned(),
                        1,
                        1,
                    )
                    .map_err(map_replay_error)?,
                ),
            };

            // Only a stream's newest checkpoint is ever read, so each write retires the one
            // before it: the directory holds one checkpoint per stream, not one per seal.
            let mut previous = first.checked_sub(1);
            for position in first..state.sealed.len() {
                if !holds_stream(position) {
                    continue;
                }
                let sealed = &state.sealed[position];
                generation.restart_walk_bounds();
                let events = self
                    .read_sealed_segment(sealed, Some(&key), 0)?
                    .into_iter()
                    .filter(|batch| &batch.scope == scope && batch.stream_id == stream_id)
                    .flat_map(|batch| batch.events)
                    .collect::<Vec<_>>();
                fold(&mut generation, &events)?;
                drop(events);
                if generation.watermark().last_sequence().saturating_add(1) != range(position).1 {
                    return Err(EventRepositoryError::IntegrityAt(
                        "checkpoint:segment-head-mismatch",
                    ));
                }
                // Best effort: a reader that cannot write the cache still answers.
                if self.write_checkpoint(sealed, &key, &generation).is_ok()
                    && let Some(older) = previous.replace(position)
                {
                    let _ = std::fs::remove_file(
                        self.root
                            .join(SEGMENTS_DIR)
                            .join(checkpoint_file_name(state.sealed[older].index(), &key)),
                    );
                }
            }

            generation.restart_walk_bounds();
            let tail = state
                .batches
                .iter()
                .filter(|batch| &batch.scope == scope && batch.stream_id == stream_id)
                .flat_map(|batch| batch.events.iter().cloned())
                .collect::<Vec<_>>();
            fold(&mut generation, &tail)?;

            let head = generation.watermark().last_sequence();
            let expected = state.next_sequence.get(&key).copied().unwrap_or(1);
            if head.saturating_add(1) != expected
                || generation
                    .watermark()
                    .last_event_hash()
                    .map(|hash| hash.as_str())
                    != state.last_hash.get(&key).map(String::as_str)
            {
                return Err(EventRepositoryError::IntegrityAt(
                    "checkpoint:stream-head-mismatch",
                ));
            }
            let mut projection = generation.projection().clone();
            if head > 0 {
                projection.stream_id = Some(stream_id.to_owned());
            }
            Ok((projection, head))
        })
    }

    /// A checkpoint for `sealed`, if one is on disk and describes exactly that seal, that stream
    /// and the stream's head at that seal. Anything else reads as absent.
    fn load_checkpoint(
        &self,
        sealed: &SealedSegment,
        key: &str,
        end: u64,
    ) -> Option<ProjectionGeneration> {
        let directory = open_child_directory(&self.root_handle, &self.root, SEGMENTS_DIR).ok()?;
        let mut file: File = open_child_file(
            &directory,
            &self.root.join(SEGMENTS_DIR),
            &checkpoint_file_name(sealed.index(), key),
            false,
            false,
        )
        .ok()?;
        let bytes = read_bounded_file(&mut file, MAX_SEAL_STATE_BYTES).ok()?;
        let checkpoint: ProjectionCheckpoint = serde_json::from_slice(&bytes).ok()?;
        let watermark = checkpoint.generation.watermark();
        (canonical_bytes(&checkpoint).ok()? == bytes
            && checkpoint.format == CHECKPOINT_FORMAT
            && checkpoint.index == sealed.index()
            && checkpoint.seal_sha256 == sealed.seal_sha256
            && checkpoint.stream_key == key
            && watermark.projection_name() == CHECKPOINT_PROJECTION
            && watermark.last_sequence().saturating_add(1) == end)
            .then_some(checkpoint.generation)
    }

    fn write_checkpoint(
        &self,
        sealed: &SealedSegment,
        key: &str,
        generation: &ProjectionGeneration,
    ) -> Result<(), EventRepositoryError> {
        let checkpoint = ProjectionCheckpoint {
            format: CHECKPOINT_FORMAT.to_owned(),
            index: sealed.index(),
            seal_sha256: sealed.seal_sha256.clone(),
            stream_key: key.to_owned(),
            generation: generation.clone(),
        };
        let bytes = canonical_bytes(&checkpoint)?;
        if bytes.len() as u64 > MAX_SEAL_STATE_BYTES {
            return Err(EventRepositoryError::LimitExceeded);
        }
        let name = checkpoint_file_name(sealed.index(), key);
        // A checkpoint already under this name that is not these bytes was ignored by the read
        // that is now replacing it (it is derived, never evidence), so it is retired first.
        let path = self.root.join(SEGMENTS_DIR).join(&name);
        if std::fs::metadata(&path).is_ok_and(|existing| {
            existing.len() != bytes.len() as u64
                || std::fs::read(&path).is_ok_and(|existing| existing != bytes)
        }) {
            std::fs::remove_file(&path)?;
        }
        let directory = open_child_directory(&self.root_handle, &self.root, SEGMENTS_DIR)?;
        self.publish_segment_file(&directory, &name, &bytes)
    }
}

fn fold(
    generation: &mut ProjectionGeneration,
    events: &[EventEnvelope],
) -> Result<(), EventRepositoryError> {
    events
        .chunks(MAX_READ_PAGE)
        .try_for_each(|page| generation.apply_page(page))
        .map_err(map_replay_error)
}
