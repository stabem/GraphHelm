//! Wire-neutral types for one model call and its reply.
//!
//! The gateway-slice plan's Task 4 places these here, rather than in
//! `adapters/model-gateway`, so a later milestone (05d, the execution-side consumer) can depend on
//! them without pulling in the adapter crate's `ureq`/subprocess machinery. Both the BYOK adapters
//! (`adapters/model-gateway/src/byok.rs`) and the native-runtime adapters (Task 5) construct these
//! from whatever provider- or CLI-specific shape they actually parsed. Reporting-source labels
//! retain provenance, but provider parsing, SDKs and I/O remain exclusively in adapters.
//!
//! Plain serde derives only: no validation, no smart constructor, no `deny_unknown_fields`. A
//! `RouteManifest` is a hand-authored operator artifact worth refusing structurally (§4); a
//! `ModelCall`/`ModelReply` is a value passed between trusted, already-typed Rust code in the same
//! process, so there is nothing here for a smart constructor to guard.
//!
//! `#[serde(rename_all = "camelCase")]` matches every other JSON-facing type in this workspace
//! (`manifest.rs`, `adapters/model-gateway/src/broker.rs`'s persisted entries) even though nothing
//! in Task 4 itself serializes these to JSON — the BYOK adapters speak provider-specific wire
//! shapes defined in `byok.rs` and only construct these as plain Rust values.

use serde::{Deserialize, Serialize};

/// One request to a model: the resolved prompt text and the caller's max-output-tokens ask.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCall {
    pub prompt: String,
    /// Legacy hint: required by Anthropic, historically ignored by OpenAI and native runtimes.
    pub max_tokens: u32,
    /// An explicit ceiling, separate from the legacy hint; absence preserves old wire behavior.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<std::num::NonZeroU32>,
}

/// A transport-successful reply: text, usage and any reported completion state.
/// Consumers must inspect termination before accepting the text as completed work.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelReply {
    pub text: String,
    pub usage: Usage,
    /// Reported completion state. Missing means unknown legacy reporting, never invented success.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub termination: Option<ModelTermination>,
}

/// The provider's reported reason, normalized without retaining arbitrary provider prose.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelTermination {
    pub reason: ModelStopReason,
    /// Recognized provider identifier, or the literal `unknown` for any unrecognized value.
    pub provider_reason: String,
}

/// Completion states understood by text-only cognitive consumers. Tool/pause continuations
/// require a different protocol and must not masquerade as finished text here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelStopReason {
    Completed,
    OutputLimit,
    ContextLimit,
    ToolCall,
    ContentFilter,
    Refusal,
    Paused,
    Unknown,
}

impl ModelReply {
    /// Whether the provider explicitly reported an incomplete/non-text completion. An absent
    /// termination field retains legacy behavior; it is not evidence of provider completion.
    #[must_use]
    pub fn is_incomplete(&self) -> bool {
        self.termination
            .as_ref()
            .is_some_and(|termination| termination.reason != ModelStopReason::Completed)
    }
}

/// Token counts for one call. §11.2: a figure the provider (or native-runtime CLI shape) did not
/// report is `None`, never invented — an absent `usage` object, or an absent field within one,
/// must never be filled in with a guess or a zero.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    /// Provider/native counters, never local estimates or monetary charges. Missing stays unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_token_semantics: Option<InputTokenSemantics>,
    /// The wire boundary that reported the counters; no provenance is invented for legacy replies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<UsageSource>,
}

/// Whether the reported input already contains the separately reported cache counters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputTokenSemantics {
    IncludesCache,
    ExcludesCache,
}

/// Reporting boundary, not a claim that tokens were billed or money was charged.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageSource {
    AnthropicMessages,
    OpenaiChatCompletions,
    ClaudeCode,
    Codex,
}
