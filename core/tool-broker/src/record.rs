//! The durable shape of a tool call's outcome.
//!
//! The record is what Milestone 05d externalizes beside Evidence: dispositions, digests and
//! sizes only. The stream bytes themselves are operator/Evidence material and never enter the
//! record — D-036's free-form discipline applied one layer early, so there is nowhere for
//! content to leak into an event payload later. Translation into `NodeOutcome` is deliberately
//! absent here: that mapping belongs to the real executor (05d), not the broker.

use crate::effect::IsolationTier;
use std::collections::BTreeSet;

/// SHA-256 of `bytes`, lowercase hex. Pinned to the algorithm by test against the standard
/// empty-input vector so it can never silently change.
#[must_use]
pub fn digest_hex(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    hex::encode(sha2::Sha256::digest(bytes))
}

/// How a tool call ended. Internally tagged as `kind` so a reader can dispatch without
/// guessing shape; every variant is content-free (rules and stable codes, never caller
/// arguments or stream text).
#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolDisposition {
    /// The tool ran to completion; `exit_code` is the child's (or 0 for in-process reads).
    Completed { exit_code: i32 },
    /// `authorize` refused; `rule` is the refusal's stable rule name, never its content.
    Denied { rule: String },
    /// The deadline killed the child.
    TimedOut,
    /// The host failed around the tool; `code` is the host's stable internal code.
    HostError { code: String },
}

/// One tool call's durable record: identity, tier, disposition, and digest-only references to
/// the captured streams. The bytes travel separately (operator files now, Evidence in 05d).
#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallRecord {
    pub tool: String,
    pub action: String,
    pub actor: String,
    /// Complete bare-program authorization set presented with this call. `BTreeSet` gives one
    /// canonical wire order; old records decode as an empty, explicitly unknown set.
    #[serde(default)]
    pub program_allowlist: BTreeSet<String>,
    pub tier: IsolationTier,
    pub disposition: ToolDisposition,
    pub stdout_sha256: String,
    pub stdout_bytes: u64,
    pub stderr_sha256: String,
    pub stderr_bytes: u64,
    pub truncated: bool,
    /// Whether this record was served from the snapshot-keyed read cache instead of a fresh
    /// execution (Task 9b). A reused record's digests are byte-identical to the original's.
    pub reused: bool,
}
