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
    /// The commit a `repository`/`commit` call produced, as its full lowercase hex object id
    /// (#1066). `None` for every other call, for a commit that did not complete, and for
    /// records that predate the field. Content-free by construction: an object id names the
    /// tree, it does not carry it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    /// The ref in the PROJECT that `commit` now points at `commit` — always under
    /// [`EXECUTION_REF_NAMESPACE`], never a branch, never the operator's checkout (#1066).
    /// `Some` only when the call ran inside an execution's workspace (a per-call workspace has
    /// no execution to land under, so its commit stays reachable only from the record).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub landed_ref: Option<String>,
    /// Whether the host reclaimed a STALE workspace before this call ran (#1073): the leftover
    /// of an earlier drive of the same execution whose server died before releasing it. The
    /// call then ran in a fresh tree provisioned from the execution's ref (or `HEAD`), exactly
    /// as if nothing had been left behind; this flag is the audit trail that something was.
    /// Absent on the wire when false.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub recovered_workspace: bool,
}

/// Where an execution's commits land in the project: a plain ref namespace, deliberately
/// outside `refs/heads` so nothing an execution does can move a branch the operator has
/// checked out (sovereignty — the operator merges with `git merge <ref>` when they choose).
pub const EXECUTION_REF_NAMESPACE: &str = "refs/graphhelm/executions/";

/// The prefix of the digest spelling under [`EXECUTION_REF_NAMESPACE`]; an id carrying it is
/// never spelled verbatim, so the two spellings cannot collide.
const DIGEST_SPELLING_PREFIX: &str = "sha256-";

/// The ref an execution's commits land under: [`EXECUTION_REF_NAMESPACE`] followed by the
/// execution id when the id is a legal LOWERCASE ref segment, otherwise by `sha256-<first 32 hex
/// of the id's sha256>`. Deterministic, so an auditor holding the record can re-derive it; an
/// execution id may carry bytes git refuses in a ref name (`~`, `^`, `:`, `?`, `*`, `[`, `..`),
/// and the record names the ref that was actually written either way.
///
/// Lowercase only (Codex, on #1073): loose refs are files, and on the case-insensitive
/// filesystems Git for Windows normally runs on `Build-1` and `build-1` would alias one path —
/// the second execution would resume from the first's commit and overwrite its ref. An id with
/// any uppercase byte takes the digest spelling, whose hex is lowercase by construction, so two
/// ids that differ only by case land under two refs.
///
/// An id that itself begins with `sha256-` ALWAYS takes the digest spelling (#1073): spelled
/// verbatim, `sha256-<32 hex>` is exactly the digest spelling of some other id, so
/// `sha256-a5bb…` as a literal id and `Build-1` (whose digest that is) would land under one ref.
/// Digesting every `sha256-`-prefixed id keeps the verbatim and digest spellings disjoint —
/// the verbatim branch can no longer produce a name the digest branch can produce.
#[must_use]
pub fn execution_ref(execution_id: &str) -> String {
    if is_ref_segment(execution_id) && !execution_id.starts_with(DIGEST_SPELLING_PREFIX) {
        format!("{EXECUTION_REF_NAMESPACE}{execution_id}")
    } else {
        let digest = digest_hex(execution_id.as_bytes());
        format!(
            "{EXECUTION_REF_NAMESPACE}{DIGEST_SPELLING_PREFIX}{}",
            &digest[..32]
        )
    }
}

/// `git check-ref-format`'s rules for ONE component, applied conservatively and case-safely:
/// ASCII LOWERCASE letters, digits, `.`, `_` and `-` only; no leading `.`, no trailing `.`, no
/// `..`, no `.lock` suffix, bounded length. Anything else takes the digest spelling.
fn is_ref_segment(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
        && !value.starts_with('.')
        && !value.ends_with('.')
        && !value.contains("..")
        && !value.ends_with(".lock")
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
