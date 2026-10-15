use serde::Serialize;

use crate::EventRepositoryError;
use graphhelm_protocols::EventEnvelope;

pub(crate) fn serialized_len_bounded(
    value: &impl Serialize,
    limit: usize,
) -> Result<usize, EventRepositoryError> {
    struct CountingWriter {
        count: usize,
        limit: usize,
    }
    impl std::io::Write for CountingWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.count = self
                .count
                .checked_add(bytes.len())
                .ok_or_else(|| std::io::Error::other("limit"))?;
            if self.count > self.limit {
                return Err(std::io::Error::other("limit"));
            }
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let mut writer = CountingWriter { count: 0, limit };
    serde_json::to_writer(&mut writer, value).map_err(|_| EventRepositoryError::LimitExceeded)?;
    Ok(writer.count)
}

/// #1378: the workspace's one canonical writer over the serialized value.
pub(crate) fn canonical_bytes(value: &impl Serialize) -> Result<Vec<u8>, EventRepositoryError> {
    let value = serde_json::to_value(value).map_err(|_| EventRepositoryError::Invalid)?;
    Ok(graphhelm_protocols::canonical::canonical_bytes(&value))
}

// #1272: the one pair, re-exported under the names this crate already uses.
pub(crate) use graphhelm_protocols::hash::{sha256_hex, wire_sha256};

pub(crate) fn event_hash(
    event: &EventEnvelope,
    previous_hash: &str,
) -> Result<String, EventRepositoryError> {
    let mut value = serde_json::to_value(event).map_err(|_| EventRepositoryError::Invalid)?;
    value
        .as_object_mut()
        .ok_or(EventRepositoryError::Invalid)?
        .remove("eventHash");
    let mut bytes = b"graphhelm-event-hash-v1\0".to_vec();
    bytes.extend_from_slice(previous_hash.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&canonical_bytes(&value)?);
    Ok(wire_sha256(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    /// #1378: the event chain hash is persisted, so its canonical bytes are history. Every event
    /// in the committed fixture store is re-hashed here and must equal the `eventHash` it was
    /// written with (an absolute golden: these bytes came from the store, not from this code).
    #[test]
    fn event_hash_reproduces_every_persisted_hash_of_the_fixture_store() {
        let journal = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/graph-version-published/events/journal.jsonl"),
        )
        .expect("fixture journal");
        let mut checked = 0;
        for line in journal.lines().filter(|line| !line.trim().is_empty()) {
            let record: Value = serde_json::from_str(line).unwrap();
            for event in record["events"].as_array().into_iter().flatten() {
                let envelope: EventEnvelope = serde_json::from_value(event.clone()).unwrap();
                let recomputed = event_hash(&envelope, envelope.previous_hash.as_str()).unwrap();
                assert_eq!(
                    recomputed,
                    envelope.event_hash.as_str(),
                    "{}",
                    envelope.event_id
                );
                checked += 1;
            }
        }
        assert!(
            checked > 0,
            "HARNESS-BROKE: no event in the fixture journal"
        );
    }

    #[test]
    fn bounded_counter_is_inclusive_at_exact_serialized_byte_limits() {
        let limit = 1024_usize;
        for (length, accepted) in [(limit - 1, true), (limit, true), (limit + 1, false)] {
            let value = "x".repeat(length - 2);
            assert_eq!(
                serialized_len_bounded(&value, limit).is_ok(),
                accepted,
                "serialized length {length}"
            );
        }
    }
}
