use graphhelm_protocols::Diagnostic;
use serde_json::Value;
use thiserror::Error;

use crate::MAX_JSON_DEPTH;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
pub struct SchemaDigest(String);

impl SchemaDigest {
    pub fn parse(value: &str) -> Result<Self, EvolutionError> {
        let Some(hexadecimal) = value.strip_prefix("sha256:") else {
            return Err(EvolutionError::InvalidDigest);
        };
        if hexadecimal.len() != 64
            || !hexadecimal.bytes().all(|byte| {
                byte.is_ascii_digit() || (byte.is_ascii_lowercase() && byte.is_ascii_hexdigit())
            })
        {
            return Err(EvolutionError::InvalidDigest);
        }

        Ok(Self(value.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> serde::Deserialize<'de> for SchemaDigest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = <String as serde::Deserialize>::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Error)]
pub enum EvolutionError {
    #[error("invalid schema SHA-256 digest")]
    InvalidDigest,
    #[error("JSON nesting exceeds the maximum depth of {MAX_JSON_DEPTH}")]
    JsonDepthExceeded,
    #[error("failed to serialize canonical JSON")]
    CanonicalSerialization(#[source] serde_json::Error),
}

impl EvolutionError {
    #[must_use]
    pub fn diagnostic(&self) -> Diagnostic {
        Diagnostic::error(
            "GHC001_CATALOG_INVALID",
            self.to_string(),
            "/",
            "schema-evolution",
        )
    }
}

/// #1378: the workspace's one canonical writer, with this crate's depth refusal.
pub fn canonical_json(value: &Value) -> Result<Vec<u8>, EvolutionError> {
    graphhelm_protocols::canonical::canonical_bytes_within(value, MAX_JSON_DEPTH)
        .map_err(|_| EvolutionError::JsonDepthExceeded)
}

/// Computed over the canonical structure AFTER parsing (#359, measured by F): the source file's
/// own EOL convention and whitespace never reach this function, since `value` is already a
/// parsed `Value` by the time it arrives here -- only key order and formatting inside the
/// document itself could move the digest, and `canonical_json` fixes both.
pub fn schema_digest(value: &Value) -> Result<SchemaDigest, EvolutionError> {
    let canonical = canonical_json(value)?;
    Ok(SchemaDigest(graphhelm_protocols::hash::wire_sha256(
        canonical.as_ref(),
    )))
}
