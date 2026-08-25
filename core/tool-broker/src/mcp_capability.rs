//! Per-contribution MCP capability tokens (#213): a pure `identity → digest → allowlist`
//! pipeline over a token minted for one extension contribution, sibling to [`crate::lease`]'s
//! pipeline for a different domain (MCP tool names, not shell/repository capabilities). No
//! clock, no randomness, no filesystem — `now` and every bound field are the caller's.
//!
//! **Why the audit record below is NOT a `graphhelm_protocols::EventKind` variant** (a decision
//! made once, here, so the next person who thinks "this should be an event" finds the reasoning
//! at the declaration site instead of re-deriving it): `EventKind` is the execution-domain
//! replay log — hash-chained, closed-vocabulary, enforced by
//! `core/schema-evolution/tests/conformance.rs`'s per-variant conformance table — built to
//! reconstruct EXECUTION STATE by replay. An MCP capability decision is a security/infra audit
//! fact, not a fact execution replay needs; folding it into `EventKind` would flatten two
//! different domains into one vocabulary and pull `core/schema-evolution` into this task's scope
//! for a concern it was never designed to carry. The audit trail this module produces is
//! persisted separately (apps/cli's impure MCP layer, `--capability-audit-log`, one JSON object
//! per line, append-only) — read by any line-oriented JSON tool (`jq '.decision.outcome'
//! audit.jsonl`, for example); a dedicated `graphhelm mcp audit-log` reader/summarizer is
//! explicitly OUT of scope for #213 and is a named follow-up, not a silently dropped intent.
//! Bridging this trail into the real execution event store (if ever wanted) is the same kind of
//! follow-up, for the same reason: that bridge is a design decision about `EventKind` itself, not
//! a thing this task should decide by side effect.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// What one contribution was granted. Bound at mint time and never re-derived from a live
/// manifest at verify time — `allowed_tools` is the token's own frozen claim. The wire form is
/// the file-backed claim store's on-disk shape (impure layer, apps/cli) — this type carries no
/// I/O of its own, only the shape.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpCapabilityToken {
    pub package_digest: String,
    pub contribution_id: String,
    pub actor: String,
    pub allowed_tools: BTreeSet<String>,
    /// The contribution's own declared `effects`, carried for audit/observability only (the
    /// issue's own Deliverables list names this field explicitly). NEVER consulted by
    /// `authorize_mcp_call` for tool-name gating — `allowed_tools`, derived from `surfaces`
    /// alone, is the only thing that grants a tool (blueprint T4).
    #[serde(default)]
    pub effects: BTreeSet<String>,
    pub revoked: bool,
    pub expires_at: u64,
}

/// A typed refusal, content-free: never a tool call's arguments, only the bound identity that
/// failed to match.
#[derive(Clone, PartialEq, Eq, Debug, thiserror::Error)]
pub enum McpCapabilityRefusal {
    #[error("the token has been revoked")]
    Revoked,
    #[error("the token has expired")]
    Expired,
    #[error("the caller is not the token's actor")]
    ActorMismatch,
    #[error("the token's package digest does not match the package being served")]
    StaleDigest,
    #[error("the token does not allow this tool")]
    ToolNotAllowlisted,
}

impl McpCapabilityRefusal {
    /// The stable wire spelling for the audit trail — never the `Display` prose, which is for a
    /// human reading a JSON-RPC error and is not a contract an audit-log reader should parse.
    #[must_use]
    pub const fn wire_code(&self) -> &'static str {
        match self {
            Self::Revoked => "revoked",
            Self::Expired => "expired",
            Self::ActorMismatch => "actor_mismatch",
            Self::StaleDigest => "stale_digest",
            Self::ToolNotAllowlisted => "tool_not_allowlisted",
        }
    }
}

/// One capability decision, redacted (#213 blueprint T7): never a call's arguments, never the
/// bearer token's own bytes — `record_call`'s signature has neither parameter, so there is
/// nothing to leak by construction, not by discipline.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpCapabilityAuditRecord {
    pub actor: String,
    pub contribution_id: String,
    pub package_digest: String,
    pub tool_name: String,
    pub decision: McpCapabilityDecision,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum McpCapabilityDecision {
    Allowed,
    Refused { code: String },
}

/// Builds one audit record from a decision already made — this function never calls
/// `authorize_mcp_call` itself, so a caller cannot "forget" the real check and only log a
/// decision it invented.
#[must_use]
pub fn record_call(
    actor: &str,
    contribution_id: &str,
    package_digest: &str,
    tool_name: &str,
    decision: &Result<(), McpCapabilityRefusal>,
) -> McpCapabilityAuditRecord {
    McpCapabilityAuditRecord {
        actor: actor.to_owned(),
        contribution_id: contribution_id.to_owned(),
        package_digest: package_digest.to_owned(),
        tool_name: tool_name.to_owned(),
        decision: match decision {
            Ok(()) => McpCapabilityDecision::Allowed,
            Err(refusal) => McpCapabilityDecision::Refused {
                code: refusal.wire_code().to_owned(),
            },
        },
    }
}

/// The `tool:` half of one contribution's own declared `surfaces`, prefix stripped. CLI surfaces
/// (`cli:`) are a different consumer's concern and are dropped here, not carried forward opaque.
/// `effects`/`permissions` are never read here (#213 blueprint T4) — the schema validator's
/// authority-escalation check already requires a matching effect for a mutation surface, in the
/// direction surface-requires-effect, never effect-grants-surface.
#[must_use]
pub fn derive_allowed_tools(surfaces: &[String]) -> BTreeSet<String> {
    surfaces
        .iter()
        .filter_map(|surface| surface.strip_prefix("tool:"))
        .map(str::to_owned)
        .collect()
}

/// Mint one token, freezing `allowed_tools` from `surfaces` as they are RIGHT NOW. A later edit
/// to the package's own declared surfaces never reaches back into a token already returned by
/// this call — re-minting is the only way to widen or narrow what an existing token can do.
/// `effects` rides along verbatim, for audit only (see the field's own doc for why it is never
/// gating).
#[must_use]
pub fn mint(
    package_digest: String,
    contribution_id: String,
    actor: String,
    surfaces: &[String],
    effects: &[String],
    now: u64,
    ttl_seconds: u64,
) -> McpCapabilityToken {
    McpCapabilityToken {
        package_digest,
        contribution_id,
        actor,
        allowed_tools: derive_allowed_tools(surfaces),
        effects: effects.iter().cloned().collect(),
        revoked: false,
        expires_at: now + ttl_seconds,
    }
}

/// # Errors
/// The first [`McpCapabilityRefusal`] the call violates, in pipeline order.
pub fn authorize_mcp_call(
    token: &McpCapabilityToken,
    tool_name: &str,
    actor: &str,
    current_package_digest: &str,
    now: u64,
) -> Result<(), McpCapabilityRefusal> {
    if token.revoked {
        return Err(McpCapabilityRefusal::Revoked);
    }
    if now >= token.expires_at {
        return Err(McpCapabilityRefusal::Expired);
    }
    if actor != token.actor {
        return Err(McpCapabilityRefusal::ActorMismatch);
    }
    if current_package_digest != token.package_digest {
        return Err(McpCapabilityRefusal::StaleDigest);
    }
    if !token.allowed_tools.contains(tool_name) {
        return Err(McpCapabilityRefusal::ToolNotAllowlisted);
    }
    Ok(())
}
