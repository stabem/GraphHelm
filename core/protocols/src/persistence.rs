use std::fmt;

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

/// Validation failure at the durable wire boundary.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[error("invalid persisted {field}")]
pub struct PersistenceError {
    field: &'static str,
}

impl PersistenceError {
    pub(crate) const fn new(field: &'static str) -> Self {
        Self { field }
    }
}

macro_rules! validated_string {
    ($name:ident, $field:literal, $validator:ident) => {
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn parse(value: impl Into<String>) -> Result<Self, PersistenceError> {
                let value = value.into();
                if $validator(&value) {
                    Ok(Self(value))
                } else {
                    Err(PersistenceError::new($field))
                }
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::parse(value).map_err(serde::de::Error::custom)
            }
        }
    };
}

pub(crate) fn is_opaque_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| matches!(byte, 0x21..=0x2e | 0x30..=0x39 | 0x3b..=0x5b | 0x5d..=0x7e))
}

fn is_actor_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 256
        && bytes[0].is_ascii_alphanumeric()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn is_raw_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn is_wire_hash(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(is_raw_sha256)
}

const FORBIDDEN_SAFE_KEY_FRAGMENTS: &[&str] = &[
    "instruction",
    "prompt",
    "objective",
    "purpose",
    "output",
    "log",
    "credential",
    "environment",
    "message",
    "path",
    "source",
    "secret",
    "token",
    "password",
    "apikey",
    "privatekey",
    "rawtoolresult",
    "description",
    "displayname",
    "completioncontract",
    "policytext",
    "diagnosticdetail",
    "content",
    "response",
    "comment",
    "example",
    "title",
    "prose",
    "note",
];

pub(crate) fn is_safe_key(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty()
        || bytes.len() > 64
        || !bytes[0].is_ascii_alphabetic()
        || !bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return false;
    }
    let normalized = value
        .bytes()
        .filter(|byte| !matches!(byte, b'.' | b'_' | b'-'))
        .map(|byte| byte.to_ascii_lowercase() as char)
        .collect::<String>();
    !FORBIDDEN_SAFE_KEY_FRAGMENTS
        .iter()
        .any(|forbidden| normalized.contains(forbidden))
}

fn is_safe_value(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 128
        && bytes[0].is_ascii_alphanumeric()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
}

fn is_artifact_locator(value: &str) -> bool {
    value
        .strip_prefix("artifact://sha256/")
        .is_some_and(is_raw_sha256)
}

fn is_media_type(value: &str) -> bool {
    if !(3..=127).contains(&value.len()) {
        return false;
    }
    let mut parts = value.split('/');
    let valid_part = |part: &str| {
        let bytes = part.as_bytes();
        !bytes.is_empty()
            && bytes.len() <= 63
            && bytes[0]
                .is_ascii_lowercase()
                .then_some(())
                .or_else(|| bytes[0].is_ascii_digit().then_some(()))
                .is_some()
            && bytes[1..].iter().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(
                        byte,
                        b'!' | b'#' | b'$' | b'&' | b'^' | b'_' | b'.' | b'+' | b'-'
                    )
            })
    };
    matches!((parts.next(), parts.next(), parts.next()), (Some(first), Some(second), None) if valid_part(first) && valid_part(second))
}

fn is_semantic_version(value: &str) -> bool {
    if !(5..=32).contains(&value.len()) {
        return false;
    }
    let parts = value.split('.').collect::<Vec<_>>();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && (part.len() == 1 || !part.starts_with('0'))
        })
}

validated_string!(OpaqueId, "opaque id", is_opaque_id);
validated_string!(WorkspaceId, "workspace id", is_opaque_id);
validated_string!(ProjectId, "project id", is_opaque_id);
validated_string!(ExecutionId, "execution id", is_opaque_id);
validated_string!(EvidenceId, "evidence id", is_opaque_id);
validated_string!(ArtifactId, "artifact id", is_opaque_id);
validated_string!(EventHash, "event hash", is_wire_hash);
validated_string!(ActorId, "actor id", is_actor_id);
validated_string!(RawSha256, "raw sha256", is_raw_sha256);
validated_string!(WireHash, "wire hash", is_wire_hash);
validated_string!(SafeKey, "safe key", is_safe_key);
validated_string!(SafeValue, "safe value", is_safe_value);
validated_string!(ArtifactLocator, "artifact locator", is_artifact_locator);
validated_string!(MediaType, "media type", is_media_type);
validated_string!(SemanticVersion, "semantic version", is_semantic_version);

/// Closed confidentiality classification shared by persisted records.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sensitivity {
    Public,
    Internal,
    Confidential,
    Restricted,
}

/// Stable repository tenancy scope. Execution identity is optional for project-level records.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RepositoryScope {
    workspace_id: WorkspaceId,
    project_id: ProjectId,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_non_null"
    )]
    execution_id: Option<ExecutionId>,
}

/// Bounded reference to encrypted Evidence carried by event envelopes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvidenceReference {
    evidence_id: EvidenceId,
    content_sha256: RawSha256,
    ciphertext_sha256: RawSha256,
}

impl EvidenceReference {
    #[must_use]
    pub const fn new(
        evidence_id: EvidenceId,
        content_sha256: RawSha256,
        ciphertext_sha256: RawSha256,
    ) -> Self {
        Self {
            evidence_id,
            content_sha256,
            ciphertext_sha256,
        }
    }

    #[must_use]
    pub const fn evidence_id(&self) -> &EvidenceId {
        &self.evidence_id
    }

    #[must_use]
    pub const fn content_sha256(&self) -> &RawSha256 {
        &self.content_sha256
    }

    #[must_use]
    pub const fn ciphertext_sha256(&self) -> &RawSha256 {
        &self.ciphertext_sha256
    }
}

/// Content-addressed artifact metadata safe for durable event references.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactReference {
    artifact_id: ArtifactId,
    locator: ArtifactLocator,
    content_sha256: RawSha256,
    media_type: MediaType,
    byte_length: u64,
    sensitivity: Sensitivity,
    metadata_version: SemanticVersion,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawArtifactReference {
    artifact_id: ArtifactId,
    locator: ArtifactLocator,
    content_sha256: RawSha256,
    media_type: MediaType,
    byte_length: u64,
    sensitivity: Sensitivity,
    metadata_version: SemanticVersion,
}

impl ArtifactReference {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        artifact_id: ArtifactId,
        locator: ArtifactLocator,
        content_sha256: RawSha256,
        media_type: MediaType,
        byte_length: u64,
        sensitivity: Sensitivity,
        metadata_version: SemanticVersion,
    ) -> Result<Self, PersistenceError> {
        if !(1..=17_179_869_184).contains(&byte_length) {
            return Err(PersistenceError::new("artifact byte length"));
        }
        Ok(Self {
            artifact_id,
            locator,
            content_sha256,
            media_type,
            byte_length,
            sensitivity,
            metadata_version,
        })
    }

    #[must_use]
    pub const fn artifact_id(&self) -> &ArtifactId {
        &self.artifact_id
    }

    #[must_use]
    pub const fn locator(&self) -> &ArtifactLocator {
        &self.locator
    }

    #[must_use]
    pub const fn content_sha256(&self) -> &RawSha256 {
        &self.content_sha256
    }

    #[must_use]
    pub const fn media_type(&self) -> &MediaType {
        &self.media_type
    }

    #[must_use]
    pub const fn byte_length(&self) -> u64 {
        self.byte_length
    }

    #[must_use]
    pub const fn sensitivity(&self) -> Sensitivity {
        self.sensitivity
    }

    #[must_use]
    pub const fn metadata_version(&self) -> &SemanticVersion {
        &self.metadata_version
    }
}

impl TryFrom<RawArtifactReference> for ArtifactReference {
    type Error = PersistenceError;

    fn try_from(raw: RawArtifactReference) -> Result<Self, Self::Error> {
        Self::new(
            raw.artifact_id,
            raw.locator,
            raw.content_sha256,
            raw.media_type,
            raw.byte_length,
            raw.sensitivity,
            raw.metadata_version,
        )
    }
}

impl<'de> Deserialize<'de> for ArtifactReference {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        RawArtifactReference::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

impl RepositoryScope {
    #[must_use]
    pub const fn new(
        workspace_id: WorkspaceId,
        project_id: ProjectId,
        execution_id: Option<ExecutionId>,
    ) -> Self {
        Self {
            workspace_id,
            project_id,
            execution_id,
        }
    }

    #[must_use]
    pub const fn workspace_id(&self) -> &WorkspaceId {
        &self.workspace_id
    }

    #[must_use]
    pub const fn project_id(&self) -> &ProjectId {
        &self.project_id
    }

    #[must_use]
    pub const fn execution_id(&self) -> Option<&ExecutionId> {
        self.execution_id.as_ref()
    }
}

/// Closed actor class for the durable projection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersistedActorType {
    Owner,
    Human,
    Agent,
    System,
}

/// Validated actor identity used only by safe persistence types.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersistedActor {
    #[serde(rename = "type")]
    actor_type: PersistedActorType,
    id: ActorId,
}

impl PersistedActor {
    #[must_use]
    pub const fn new(actor_type: PersistedActorType, id: ActorId) -> Self {
        Self { actor_type, id }
    }

    #[must_use]
    pub const fn actor_type(&self) -> PersistedActorType {
        self.actor_type
    }

    #[must_use]
    pub const fn id(&self) -> &ActorId {
        &self.id
    }
}

/// Schema-bounded canonical RFC 3339 UTC timestamp.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct PersistedTimestamp(DateTime<Utc>);

impl PersistedTimestamp {
    pub fn parse(value: &str) -> Result<Self, PersistenceError> {
        if !(20..=30).contains(&value.chars().count()) || !is_schema_datetime(value) {
            return Err(PersistenceError::new("timestamp"));
        }
        DateTime::parse_from_rfc3339(value)
            .map(|timestamp| Self(timestamp.with_timezone(&Utc)))
            .map_err(|_| PersistenceError::new("timestamp"))
    }

    pub fn from_datetime(value: DateTime<Utc>) -> Result<Self, PersistenceError> {
        Self::parse(&value.to_rfc3339_opts(SecondsFormat::AutoSi, true))
    }

    #[must_use]
    pub const fn as_datetime(&self) -> &DateTime<Utc> {
        &self.0
    }
}

fn is_schema_datetime(value: &str) -> bool {
    let bytes = value.as_bytes();
    if !(20..=30).contains(&bytes.len())
        || bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || bytes.get(10) != Some(&b'T')
        || bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
        || [0..4, 5..7, 8..10, 11..13, 14..16, 17..19]
            .into_iter()
            .any(|range| {
                bytes
                    .get(range)
                    .is_none_or(|part| !part.iter().all(u8::is_ascii_digit))
            })
    {
        return false;
    }

    let Some(hour) = parse_two_ascii_digits(bytes.get(11..13)) else {
        return false;
    };
    let Some(minute) = parse_two_ascii_digits(bytes.get(14..16)) else {
        return false;
    };
    let Some(second) = parse_two_ascii_digits(bytes.get(17..19)) else {
        return false;
    };
    if hour > 23 || minute > 59 || second > 60 {
        return false;
    }

    let canonical_suffix = match bytes.get(19) {
        Some(b'Z') => bytes.len() == 20,
        Some(b'.') => {
            let fraction = bytes.get(20..bytes.len() - 1);
            bytes.last() == Some(&b'Z')
                && fraction.is_some_and(|digits| {
                    (1..=9).contains(&digits.len()) && digits.iter().all(u8::is_ascii_digit)
                })
        }
        _ => false,
    };

    canonical_suffix && (second != 60 || (hour == 23 && minute == 59))
}

fn parse_two_ascii_digits(bytes: Option<&[u8]>) -> Option<u8> {
    let [tens, units] = bytes? else {
        return None;
    };
    if !tens.is_ascii_digit() || !units.is_ascii_digit() {
        return None;
    }
    Some((tens - b'0') * 10 + (units - b'0'))
}

impl<'de> Deserialize<'de> for PersistedTimestamp {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

pub(crate) fn valid_positive_safe_integer(value: u64) -> bool {
    (1..=MAX_SAFE_INTEGER).contains(&value)
}

pub(crate) fn valid_safe_integer(value: i64) -> bool {
    (-9_007_199_254_740_991..=9_007_199_254_740_991).contains(&value)
}

pub(crate) fn valid_bounded_text(value: &str, min: usize, max: usize) -> bool {
    (min..=max).contains(&value.chars().count())
}

pub(crate) fn parse_timestamp(value: &str) -> Result<DateTime<Utc>, PersistenceError> {
    PersistedTimestamp::parse(value).map(|timestamp| timestamp.0)
}

pub(crate) fn deserialize_optional_non_null<'de, D, T>(
    deserializer: D,
) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

pub(crate) fn deserialize_required_nullable<'de, D, T>(
    deserializer: D,
) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}
