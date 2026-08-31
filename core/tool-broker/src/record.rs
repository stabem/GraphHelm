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
    /// The verified identity of the binary that ran, when the call went through the verified
    /// doorway (#540): absolute path plus SHA-256 of the bytes at verification time. `None` for
    /// builtin in-process tools and for records that predate the field — absent, explicitly
    /// unknown, never invented (D-042: GraphHelm does not invent executable identity absent
    /// from `ToolCallRecord`; now the record has somewhere for a REAL one to live).
    ///
    /// COMPATIBILITY DEPENDENCY (L's #550 review): a new record CARRYING this field stays
    /// readable by an old reader only because `ToolCallRecord` has no `deny_unknown_fields` —
    /// the safety of that direction rests on the ABSENCE of an attribute. Hardening this struct
    /// with `deny_unknown_fields` later would silently convert that row into a reader break
    /// (#425's shape); whoever adds it must version the record instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified_executable: Option<VerifiedExecutableIdentity>,
    /// The broker-owned session this call ran in (#552). `None` for everything that is not a
    /// provider-session call, and for records that predate the field. Same compatibility
    /// dependency as `verified_executable` above: the new-record-to-old-reader direction rests
    /// on this struct NOT carrying `deny_unknown_fields` — hardening it later must version the
    /// record instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contained_session: Option<ContainedSessionIdentity>,
}

/// The identity half of #540, as the record carries it: which absolute path, which bytes.
#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VerifiedExecutableIdentity {
    pub path: String,
    pub sha256: String,
}

/// The contained-session half of D-042's primary clause (#552): which CONTAINED one-shot
/// spawn a provider call ran in, and over which snapshot generation -- the name says what the
/// mechanism DOES (a contained one-shot invocation), not the protocol a consumer may speak over
/// it. Beside `verified_executable`, this is what
/// lets a `RetrievalCoverageReceipt` bind "a named program in a named session".
#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContainedSessionIdentity {
    /// Derived deterministically from the composition (executable digest, snapshot generation,
    /// workspace), so an auditor can re-derive it from the record's own fields.
    pub session_id: String,
    pub snapshot_generation: String,
    /// The digest of the binary the session runs — self-contained on purpose, so the session
    /// identity names its program even when read apart from `verified_executable`.
    pub executable_sha256: String,
}
