//! Wire-neutral types for one model call and its reply.
//!
//! The gateway-slice plan's Task 4 places these here, rather than in
//! `adapters/model-gateway`, so a later milestone (05d, the execution-side consumer) can depend on
//! them without pulling in the adapter crate's `ureq`/subprocess machinery. Both the BYOK adapters
//! (`adapters/model-gateway/src/byok.rs`) and the native-runtime adapters (Task 5) construct these
//! from whatever provider- or CLI-specific shape they actually parsed; nothing here knows about
//! Anthropic, OpenAI, Claude Code, or Codex.
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
    pub max_tokens: u32,
}

/// A successful reply: the model's text and whatever usage the provider reported.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelReply {
    pub text: String,
    pub usage: Usage,
}

/// Token counts for one call. §11.2: a figure the provider (or native-runtime CLI shape) did not
/// report is `None`, never invented — an absent `usage` object, or an absent field within one,
/// must never be filled in with a guess or a zero.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}
