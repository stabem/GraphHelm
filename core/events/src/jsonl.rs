use graphhelm_protocols::{ArtifactReference, EventEnvelope, RepositoryScope};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PhysicalBatch {
    pub format_version: String,
    pub request_digest: String,
    pub scope: RepositoryScope,
    pub stream_id: String,
    pub expected_next_sequence: u64,
    pub checksum: String,
    pub evidence_ids: Vec<String>,
    pub artifacts: Vec<StoredArtifactRegistration>,
    pub events: Vec<EventEnvelope>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StoredArtifactRegistration {
    pub reference: ArtifactReference,
    pub producer_stream_id: String,
    pub producer_idempotency_key: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BatchChecksum<'a> {
    pub format_version: &'a str,
    pub request_digest: &'a str,
    pub scope: &'a RepositoryScope,
    pub stream_id: &'a str,
    pub expected_next_sequence: u64,
    pub evidence_ids: &'a [String],
    pub artifacts: &'a [StoredArtifactRegistration],
    pub events: &'a [EventEnvelope],
}
