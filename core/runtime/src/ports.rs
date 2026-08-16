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
