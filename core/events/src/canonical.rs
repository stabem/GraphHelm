use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

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

pub(crate) fn canonical_bytes(value: &impl Serialize) -> Result<Vec<u8>, EventRepositoryError> {
    let value = serde_json::to_value(value).map_err(|_| EventRepositoryError::Invalid)?;
    serde_json::to_vec(&canonical_value(value)).map_err(|_| EventRepositoryError::Invalid)
}

fn canonical_value(value: Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.into_iter().map(canonical_value).collect()),
        Value::Object(values) => {
            let mut entries = values.into_iter().collect::<Vec<_>>();
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, canonical_value(value)))
                    .collect(),
            )
        }
        scalar => scalar,
    }
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub(crate) fn wire_sha256(bytes: &[u8]) -> String {
    format!("sha256:{}", sha256_hex(bytes))
}

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
