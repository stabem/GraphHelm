use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use fs2::FileExt;
use graphhelm_protocols::{Clock, EventEnvelope, IdGenerator, NewEvent};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{EventStore, EventStoreError};

const MAX_STORE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_LINE_BYTES: usize = 16 * 1024 * 1024;
const MAX_EVENT_BYTES: usize = 1024 * 1024;
const MAX_BATCH_EVENTS: usize = 10_000;

#[derive(Clone)]
pub struct JsonlEventStore {
    path: PathBuf,
    clock: Arc<dyn Clock>,
    ids: Arc<dyn IdGenerator>,
}

impl JsonlEventStore {
    #[must_use]
    pub fn new(path: impl Into<PathBuf>, clock: Arc<dyn Clock>, ids: Arc<dyn IdGenerator>) -> Self {
        Self {
            path: path.into(),
            clock,
            ids,
        }
    }

    fn open(&self) -> Result<File, EventStoreError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&self.path)?)
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Reads every committed event after validating all physical batches.
    pub fn read_all(&self) -> Result<Vec<EventEnvelope>, EventStoreError> {
        let mut file = self.open()?;
        file.lock_shared()?;
        let result = read_batches(&mut file)
            .map(|(batches, _)| batches.into_iter().flat_map(|batch| batch.events).collect());
        FileExt::unlock(&file)?;
        result
    }
}

impl EventStore for JsonlEventStore {
    fn append_batch(
        &self,
        stream_id: &str,
        expected_next_sequence: u64,
        events: &[NewEvent],
    ) -> Result<Vec<EventEnvelope>, EventStoreError> {
        let unique_keys: BTreeSet<_> = events
            .iter()
            .map(|event| event.idempotency_key.as_str())
            .collect();
        if events.len() > MAX_BATCH_EVENTS
            || unique_keys.len() != events.len()
            || events.iter().any(|event| {
                serde_json::to_vec(event)
                    .map_or(true, |serialized| serialized.len() > MAX_EVENT_BYTES)
            })
        {
            return Err(EventStoreError::CorruptBatch(
                "event batch exceeds configured bounds".into(),
            ));
        }
        let mut file = self.open()?;
        file.lock_exclusive()?;
        let result = (|| {
            let (batches, committed_len) = read_batches(&mut file)?;
            let existing: Vec<_> = batches
                .iter()
                .flat_map(|batch| batch.events.iter())
                .filter(|event| event.stream_id == stream_id)
                .cloned()
                .collect();
            let existing_by_key: BTreeMap<_, _> = existing
                .iter()
                .map(|event| (event.idempotency_key.as_str(), event))
                .collect();
            if let Some(batch) = batches.iter().find(|batch| {
                batch.stream_id == stream_id
                    && batch.start_sequence == expected_next_sequence
                    && batch.events.len() == events.len()
                    && batch.events.iter().zip(events).all(|(stored, requested)| {
                        stored.idempotency_key == requested.idempotency_key
                            && stored.kind == requested.kind
                    })
            }) {
                return Ok(batch.events.clone());
            }
            let actual = existing.last().map_or(1, |event| event.sequence + 1);
            if actual != expected_next_sequence {
                return Err(EventStoreError::SequenceConflict {
                    expected: expected_next_sequence,
                    actual,
                });
            }
            if events
                .iter()
                .any(|event| existing_by_key.contains_key(event.idempotency_key.as_str()))
            {
                return Err(EventStoreError::CorruptBatch(
                    "conflicting idempotency key".into(),
                ));
            }

            let envelopes: Vec<_> = events
                .iter()
                .enumerate()
                .map(|(offset, event)| EventEnvelope {
                    id: self.ids.next_id("event"),
                    stream_id: stream_id.to_owned(),
                    sequence: expected_next_sequence + offset as u64,
                    occurred_at: self.clock.now(),
                    idempotency_key: event.idempotency_key.clone(),
                    kind: event.kind.clone(),
                })
                .collect();
            if envelopes.is_empty() {
                return Ok(envelopes);
            }
            let batch = StoredBatch {
                stream_id: stream_id.to_owned(),
                start_sequence: expected_next_sequence,
                checksum: checksum(&envelopes)?,
                events: envelopes.clone(),
            };
            let mut line = serde_json::to_vec(&batch)
                .map_err(|error| EventStoreError::CorruptBatch(error.to_string()))?;
            line.push(b'\n');
            let resulting_len = committed_len
                .checked_add(line.len())
                .ok_or_else(|| EventStoreError::CorruptBatch("event store size overflow".into()))?;
            if line.len() > MAX_LINE_BYTES || resulting_len as u64 > MAX_STORE_BYTES {
                return Err(EventStoreError::CorruptBatch(
                    "serialized batch exceeds configured bounds".into(),
                ));
            }
            file.set_len(committed_len as u64)?;
            file.seek(SeekFrom::Start(committed_len as u64))?;
            file.write_all(&line)?;
            file.flush()?;
            file.sync_data()?;
            Ok(envelopes)
        })();
        FileExt::unlock(&file)?;
        result
    }

    fn read_stream(&self, stream_id: &str) -> Result<Vec<EventEnvelope>, EventStoreError> {
        let mut file = self.open()?;
        file.lock_shared()?;
        let result = read_batches(&mut file).and_then(|(batches, _)| {
            let events: Vec<_> = batches
                .into_iter()
                .flat_map(|batch| batch.events)
                .filter(|event| event.stream_id == stream_id)
                .collect();
            verify_contiguous(&events)?;
            Ok(events)
        });
        FileExt::unlock(&file)?;
        result
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredBatch {
    stream_id: String,
    start_sequence: u64,
    checksum: String,
    events: Vec<EventEnvelope>,
}

fn read_batches(file: &mut File) -> Result<(Vec<StoredBatch>, usize), EventStoreError> {
    if file.metadata()?.len() > MAX_STORE_BYTES {
        return Err(EventStoreError::CorruptBatch(
            "event store exceeds configured bounds".into(),
        ));
    }
    file.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let committed_len = bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1);
    let mut batches = Vec::new();
    let mut expected_by_stream = BTreeMap::<String, u64>::new();
    for line in bytes[..committed_len].split(|byte| *byte == b'\n') {
        if line.is_empty() {
            continue;
        }
        if line.len() > MAX_LINE_BYTES {
            return Err(EventStoreError::CorruptBatch(
                "event batch line exceeds configured bounds".into(),
            ));
        }
        let batch: StoredBatch = serde_json::from_slice(line)
            .map_err(|error| EventStoreError::CorruptBatch(error.to_string()))?;
        if checksum(&batch.events)? != batch.checksum {
            return Err(EventStoreError::CorruptBatch("checksum mismatch".into()));
        }
        if batch.events.first().map(|event| event.sequence) != Some(batch.start_sequence)
            || batch
                .events
                .iter()
                .any(|event| event.stream_id != batch.stream_id)
        {
            return Err(EventStoreError::CorruptBatch(
                "batch stream or starting sequence mismatch".into(),
            ));
        }
        verify_contiguous(&batch.events)?;
        let expected = expected_by_stream
            .entry(batch.stream_id.clone())
            .or_insert(1);
        if batch.start_sequence != *expected {
            return Err(EventStoreError::CorruptBatch(
                "non-contiguous batch sequence".into(),
            ));
        }
        *expected += batch.events.len() as u64;
        batches.push(batch);
    }
    Ok((batches, committed_len))
}

fn verify_contiguous(events: &[EventEnvelope]) -> Result<(), EventStoreError> {
    for (index, event) in events.iter().enumerate() {
        let expected = events.first().map_or(1, |first| first.sequence) + index as u64;
        if event.sequence != expected {
            return Err(EventStoreError::CorruptBatch(
                "event sequences are not contiguous".into(),
            ));
        }
    }
    Ok(())
}

fn checksum(events: &[EventEnvelope]) -> Result<String, EventStoreError> {
    let bytes = serde_json::to_vec(events)
        .map_err(|error| EventStoreError::CorruptBatch(error.to_string()))?;
    Ok(format!("sha256:{}", hex::encode(Sha256::digest(bytes))))
}
