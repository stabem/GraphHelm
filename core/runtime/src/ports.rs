//! The dependency-inverting ports: the executor is testable without any adapter crate, and the
//! adapter crates implement these traits in `apps/cli`'s wiring — never the reverse (the
//! source-invariant suite pins the arrow from both sides).
//!
//! Shapes are the merged 05b/05c reality (Task 0 reconciliation): the model side mirrors
//! `core/gateway`'s call types; the tool side mirrors `core/tool-broker`'s. The streams and
//! reuse-summary types are OWNED HERE rather than borrowed from `adapters/tool-host` — this
//! crate must not name an adapter, so the wiring converts the adapter's `CapturedStreams` and
//! reuse decision into these runtime-owned values (reported as a Task 1 decision: the plan's
//! sketch wrote "(ToolCallRecord, CapturedStreams)", but that type lives in the adapter).

use std::future::Future;
use std::pin::Pin;

use graphhelm_gateway::call::{ModelCall, ModelReply};
use graphhelm_gateway::taxonomy::GatewayError;
use graphhelm_tool_broker::call::ToolCall;
use graphhelm_tool_broker::lease::ToolLease;
use graphhelm_tool_broker::record::ToolCallRecord;

/// A model call through the 05b gateway. The wiring wraps the synchronous adapters
/// (`ByokAdapter::call`, `RuntimeAdapter::call` — both sync, Task 0) in `spawn_blocking`.
pub trait ModelPort: Send + Sync {
    fn call<'a>(
        &'a self,
        route_id: &'a str,
        call: &'a ModelCall,
    ) -> Pin<Box<dyn Future<Output = Result<ModelReply, GatewayError>> + Send + 'a>>;

    /// Immediate-stop's reach into blocking work (Task 8, the plan review's finding):
    /// dropping a future does not stop a blocking body. Semantics per adapter: a
    /// subprocess-backed port kills its children; an HTTP-backed port CANNOT abort a request
    /// mid-flight — its bound is the transport timeout, and a reply arriving after cancel is
    /// discarded (recorded as `Interrupted`, never as the late reply). Default: nothing to
    /// cancel.
    fn cancel_all(&self) {}
}

/// The runtime-owned mirror of the host's captured streams: free-form bytes bound for
/// Evidence, never for an event payload (D-036).
pub struct ToolStreams {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// What a tool invocation produced, port-shaped: the digest-only record, the stream bytes,
/// and — when the host consulted its read cache — the reuse-decision summary the Task 0
/// discrepancy adds to the tool-host surface (the record alone does not expose the cache
/// key's identity).
pub struct ToolPortResult {
    pub record: ToolCallRecord,
    pub streams: ToolStreams,
    pub reuse: Option<ReuseSummary>,
}

/// The port-shaped reuse summary the 05d producer turns into a `ReuseDecision` event. Fields
/// mirror the wire payload's identity half; the wiring fills them from the tool-host's public
/// summary (its exact adapter-side shape is Task 5/6's in-milestone extension).
#[derive(Clone)]
pub struct ReuseSummary {
    pub decision: graphhelm_protocols::ReuseOutcome,
    pub forced_reason: Option<graphhelm_protocols::ForcedFreshReason>,
    pub freshness_class: Option<graphhelm_protocols::FreshnessClass>,
    pub key_components: Vec<graphhelm_protocols::ReuseKeyComponent>,
    pub key_digest: graphhelm_protocols::WireHash,
    pub evidence_ref: Option<graphhelm_protocols::EvidenceId>,
    pub provenance_erased: bool,
}

/// A tool call through the 05c broker. Errors do not exist on this seam: the host's own
/// contract is that every path ends in a record (denial, timeout and host error included), so
/// the port returns what the host returns.
pub trait ToolPort: Send + Sync {
    fn invoke<'a>(
        &'a self,
        call: &'a ToolCall,
        lease: &'a ToolLease,
        actor: &'a str,
    ) -> Pin<Box<dyn Future<Output = ToolPortResult> + Send + 'a>>;

    /// Kills in-flight tool children (the 05c host's kill+reap is the mechanism). Default:
    /// nothing to cancel. See [`ModelPort::cancel_all`] for the contract.
    fn cancel_all(&self) {}
}

/// Reads repository bytes for #219's retrieval plans.
///
/// The port exists to answer ONE question the binding cannot answer about itself: *are the bytes I
/// would serve still the bytes this binding names?* A `SnapshotBinding` can only compare its two
/// ids to each other, so it detects staleness that was already visible and is blind to the case
/// where both ids agree and the bytes underneath moved.
///
/// **This reader is workspace-scoped: it reports the identity of what it would serve NOW, and
/// cannot read at a historical generation.** That is a decision, not an omission — see
/// `.factory/h-agent-219-blueprint.md` section 8. Its consequence is that a stale coordinate has
/// TWO permitted exits rather than three: reindex, or refuse `index_stale`. Serving
/// snapshot-owned bytes from an earlier generation is not available, so nothing in this lane may
/// be written as though it were.
pub trait SourceReader: Send + Sync {
    /// The identity of the bytes this reader would serve right now.
    ///
    /// Must be derived from CONTENT, never from a ref: a commit id is the identity of a commit, so
    /// an uncommitted edit would change the bytes without moving it and this port would answer
    /// "unchanged" about bytes that changed. The runtime cannot check that — a content digest and
    /// a commit id are both opaque ids — so an implementor that gets this wrong is caught by the
    /// guard, not by the type.
    fn current_snapshot(&self) -> graphhelm_protocols::OpaqueId;
}

/// A brand-neutral structural-index provider. This port is synchronous because the first #219
/// slice is a deterministic fake; live MCP sessions remain unavailable under D-042 and will need
/// a separately designed broker-owned asynchronous adapter.
pub trait StructuralCodeIndex: Send + Sync {
    fn retrieve(
        &self,
        request: &crate::retrieval::StructuralIndexRequest,
    ) -> Result<StructuralIndexResponse, StructuralCodeIndexError>;
}

/// The only provider-side failure this slice can observe. Free-form provider errors do not enter
/// the receipt or a stable diagnostic.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum StructuralCodeIndexError {
    #[error("structural code index unavailable")]
    Unavailable,
}

/// Untrusted evidence returned by [`StructuralCodeIndex`]. Runtime checks every echoed binding,
/// bound and pagination fact before it can become an immutable receipt.
#[derive(Clone, Debug)]
pub struct StructuralIndexResponse {
    pub plan_binding: graphhelm_protocols::ArtifactBinding,
    pub step: graphhelm_protocols::RetrievalStepBinding,
    pub scope: graphhelm_protocols::DevelopmentScope,
    pub snapshots: graphhelm_protocols::SnapshotBinding,
    pub provider: graphhelm_protocols::RetrievalProviderBinding,
    pub broker_record: ToolCallRecord,
    pub confidence: graphhelm_protocols::ProviderCoverageConfidence,
    pub coverage: graphhelm_protocols::CoverageState,
    pub entries: Vec<graphhelm_protocols::RetrievalCoverageEntry>,
    pub pages: Vec<graphhelm_protocols::RetrievalPageEvidence>,
    pub total_results: u32,
    pub hits: Vec<String>,
}

/// Declared bounds for one bounded source search (#219).
///
/// The bounds are the whole reason this port can exist beside a graph index: a channel that
/// walks the workspace is unbounded by nature, and an unbounded evidence path inside plan
/// compilation is a denial-of-service the plan itself invites. All three are ceilings the
/// implementor must refuse to exceed rather than silently truncate -- a truncated search wearing
/// a finished search's clothes is the same defect `compile_plan_within` refuses over-budget for.
#[derive(Clone, Copy, Debug)]
pub struct SourceSearchBounds {
    /// Maximum directory entries the implementor may VISIT, whether or not it opens them.
    ///
    /// Distinct from `max_files_scanned` because the two bound different costs and fail
    /// independently (G on #622): a tree of a million empty directories, or one whose entries are
    /// all filtered out by suffix, opens NO files and reads NO bytes while the walk itself runs
    /// unbounded. The file and byte ceilings are about what is READ; this one is about what is
    /// TRAVERSED, and only it can stop a walk that never reads anything.
    ///
    /// It lives here rather than as an internal cap inside an implementor because an internal cap
    /// would be a SECOND bounds mechanism: two places deciding how much traversal is allowed, with
    /// the caller's declaration and the implementor's constant able to disagree silently. One
    /// declared struct, one oracle.
    pub max_entries_visited: usize,
    /// Maximum files the implementor may OPEN. Names the cost that actually scales.
    pub max_files_scanned: usize,
    /// Maximum bytes the implementor may READ across those files.
    pub max_bytes_scanned: u64,
    /// Maximum paths the implementor may RETURN.
    pub max_results: u32,
    /// Maximum QUERY TERMS the caller may supply, and the maximum total bytes across them.
    ///
    /// The other four ceilings bound the CORPUS side (how much the implementor walks, opens,
    /// reads, returns); these bound the QUERY side (Codex #608). A compliant channel searches
    /// every scanned file once per term, so an untrusted plan supplying an arbitrarily large term
    /// slice — or arbitrarily long terms — makes the work scale as `files x terms` while every
    /// corpus ceiling stays green. Enforced at the trust boundary (the compiler) BEFORE `search`
    /// is invoked, so the bound holds for every implementor and not only the one that remembered
    /// to cap its own input. One declared struct, one oracle, for the caller-controlled inputs too.
    pub max_terms: usize,
    pub max_term_bytes: u64,
}

// RE-EXPORTED, not declared here (#724, Codex P1 on #745).
//
// The vocabulary travels in the development envelope, so `core/protocols/src/development.rs:89`
// puts it under that contract's jurisdiction: schema as authority plus a set-equality guard
// between the Rust type and the schema. `core/runtime` cannot carry that pair -- the schema lives
// with the contract -- so the declaration moved and this is the same type under the name every
// existing caller already uses. Its cells moved with it, beside the declaration.
pub use graphhelm_protocols::SourceSearchError;

/// A bounded, workspace-scoped search over SOURCE BYTES — the second evidence channel #219's
/// acceptance calls the verified source fallback.
///
/// It exists because a structural index answers about what it INDEXED and parsed into
/// constructs: measured against the shipped index, required evidence living in non-code files
/// (JSON schemas, changelogs) is unreachable through `StructuralCodeIndex` at any query, because
/// the provider filters non-construct nodes by design. That is a true finding about the
/// instrument, not a gap in the question — so the compiler gets a second channel rather than the
/// corpus getting easier questions.
///
/// **Returns paths only.** Ranking, snippets and reads stay out: this port decides WHICH files
/// are candidate evidence, and every existing bound, escape check and budget in
/// `compile_plan_within` then applies to its output exactly as it does to the index's.
pub trait BoundedSourceSearch: Send + Sync {
    /// Candidate repository-relative paths for `terms`, within `bounds`.
    ///
    /// # Errors
    /// [`SourceSearchError::Unavailable`] when no channel is wired;
    /// [`SourceSearchError::BoundExceeded`] when the search would cross a declared ceiling.
    fn search(
        &self,
        terms: &[String],
        bounds: &SourceSearchBounds,
    ) -> Result<Vec<String>, SourceSearchError>;
}
