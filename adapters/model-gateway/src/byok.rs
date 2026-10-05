//! BYOK (bring-your-own-key) adapters: Anthropic and OpenAI over [`HttpTransport`].
//!
//! `docs/models/UNIVERSAL_MODEL_GATEWAY.md` §2/§7 describes direct-API routes; this module
//! implements the Milestone 05b subset of it
//! (gateway-slice plan, Task 4): one [`ByokAdapter`] per
//! `direct_api` [`ModelRoute`], dispatching on [`ModelRoute::provider`] to the wire shape a
//! request/response actually has for that provider, and mapping every non-success outcome onto
//! the closed [`GatewayError`] taxonomy `core/gateway` already owns.
//!
//! Every status-mapping table below is a *fixed rule*, not a heuristic scored against evidence:
//! the plan enumerates each status/body combination and the [`GatewayError`] it becomes. The one
//! genuine judgment call — OpenAI's `429` being ambiguous between hard quota exhaustion and
//! ordinary throttling — is isolated in [`openai_error_is_insufficient_quota`], the function the
//! plan's sabotage step targets.
//!
//! §11: usage a provider did not report is `None`, never invented. §17: an unclassifiable reply
//! (an unrecognized status, or a 200 whose body does not parse into the expected shape) becomes
//! [`GatewayError::MalformedOutput`], which `core/gateway`'s taxonomy maps to `RetryableFailure` —
//! a mystery reply must not park capacity (`NeedsCapacity` is reserved for the auth/quota/rate
//! classes §12 names explicitly), but it also must not invent a more specific meaning it cannot
//! support.

use std::sync::Arc;
use std::time::Duration;

use graphhelm_events::SecretBytes;
use graphhelm_gateway::call::{
    InputTokenSemantics, ModelCall, ModelReply, ModelStopReason, ModelTermination, Usage,
    UsageSource,
};
use graphhelm_gateway::manifest::{ModelRoute, OpenAiOutputTokenParameter, Transport};
use graphhelm_gateway::taxonomy::GatewayError;
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use crate::transport::{HttpTransport, TransportError, TransportRequest, TransportResponse};

const ANTHROPIC_VERSION: &str = "2023-06-01";
const OPENAI_INSUFFICIENT_QUOTA: &str = "insufficient_quota";

/// One BYOK adapter bound to a specific `direct_api` [`ModelRoute`] and [`HttpTransport`].
/// Construction does not itself touch the network; each [`Self::call`] places exactly one HTTP
/// request.
pub struct ByokAdapter<'a> {
    route: &'a ModelRoute,
    transport: Arc<dyn HttpTransport>,
}

impl<'a> ByokAdapter<'a> {
    #[must_use]
    pub fn new(route: &'a ModelRoute, transport: Arc<dyn HttpTransport>) -> Self {
        Self { route, transport }
    }

    /// Places one call against `self.route`'s provider and maps the outcome onto
    /// [`GatewayError`]. `key` is exposed only for the lifetime of building the request headers
    /// (see [`anthropic_headers`]/[`openai_headers`]) — this function never copies it anywhere
    /// that outlives that scope, and no error path below ever formats it.
    ///
    /// # Errors
    /// See [`GatewayError`]; in particular [`GatewayError::UnsupportedCapability`] if
    /// `self.route` is not a [`Transport::DirectApi`] route. `ByokAdapter::new` takes any
    /// `&ModelRoute` — nothing at the type level narrows it to `direct_api` — so a structurally
    /// *valid* route (per
    /// [`RouteManifest::from_json`](graphhelm_gateway::manifest::RouteManifest::from_json)) can
    /// still be the wrong *kind* for this adapter, most concretely a `native_runtime` route,
    /// which carries no `baseUrl` at all. This function never panics: the transport check below
    /// is what makes that true, since without it a `native_runtime` route would reach
    /// `call_anthropic`/`call_openai`'s `base_url()`/`model()` `.expect()`s, both of which are
    /// guaranteed present only for a `direct_api` route.
    pub fn call(&self, key: &SecretBytes, request: &ModelCall) -> Result<ModelReply, GatewayError> {
        if self.route.transport() != Transport::DirectApi {
            return Err(GatewayError::UnsupportedCapability);
        }
        match self.route.provider() {
            "anthropic" => self.call_anthropic(key, request),
            "openai" => self.call_openai(key, request),
            // A System One model answers typed questions and cannot draft text; it is served by
            // `systemone.rs` on the judge door. Refused here before any request is built.
            crate::systemone::TYPESAFE_PROVIDER => Err(GatewayError::UnsupportedCapability),
            _ => {
                // Unreachable for any `direct_api` route obtained through
                // `RouteManifest::from_json`: Task 4 closed this gap in
                // `core/gateway/src/manifest.rs`'s `validate_direct_api`, which now refuses a
                // `direct_api` route naming any provider but these three. Kept as a non-panicking
                // fallback anyway — this function does not re-derive that invariant itself, and a
                // closed match with no `ModelRoute` smart constructor reachable from here is a
                // fact about today's callers, not a proof.
                Err(GatewayError::UnsupportedCapability)
            }
        }
    }

    fn call_anthropic(
        &self,
        key: &SecretBytes,
        call: &ModelCall,
    ) -> Result<ModelReply, GatewayError> {
        let base_url = self
            .route
            .base_url()
            .expect("direct_api routes carry baseUrl — enforced by manifest validation");
        let model = self
            .route
            .model()
            .expect("direct_api routes carry model — enforced by manifest validation");

        let content = match self.cache_parts(call)? {
            Some((prefix, variable)) => AnthropicMessageContent::Blocks(vec![
                AnthropicTextBlock {
                    kind: "text",
                    text: prefix,
                    cache_control: Some(CacheControl { kind: "ephemeral" }),
                },
                AnthropicTextBlock {
                    kind: "text",
                    text: variable,
                    cache_control: None,
                },
            ]),
            None => AnthropicMessageContent::Text(&call.prompt),
        };
        // ADR-042: a held session's prior turns are replayed first, in order, as alternating
        // user/assistant messages; the new briefing is the final user message.
        let mut messages: Vec<AnthropicMessage<'_>> = call
            .history
            .iter()
            .flat_map(|turn| {
                [
                    AnthropicMessage {
                        role: "user",
                        content: AnthropicMessageContent::Text(&turn.prompt),
                    },
                    AnthropicMessage {
                        role: "assistant",
                        content: AnthropicMessageContent::Text(&turn.reply),
                    },
                ]
            })
            .collect();
        messages.push(AnthropicMessage {
            role: "user",
            content,
        });
        let payload = AnthropicRequestBody {
            model,
            max_tokens: self.explicit_output_cap(call).unwrap_or(call.max_tokens),
            messages,
        };
        let body = serde_json::to_vec(&payload)
            .expect("AnthropicRequestBody is plain data and always serializes");

        let response = self.execute(TransportRequest {
            method: "POST",
            url: format!("{base_url}/v1/messages"),
            headers: anthropic_headers(key),
            body,
            timeout: Duration::from_secs(self.route.timeout_seconds()),
        })?;

        if (200..300).contains(&response.status) {
            parse_anthropic_success(&response.body).ok_or(GatewayError::MalformedOutput)
        } else {
            Err(map_anthropic_error(response.status))
        }
    }

    fn call_openai(&self, key: &SecretBytes, call: &ModelCall) -> Result<ModelReply, GatewayError> {
        let base_url = self
            .route
            .base_url()
            .expect("direct_api routes carry baseUrl — enforced by manifest validation");
        let model = self
            .route
            .model()
            .expect("direct_api routes carry model — enforced by manifest validation");

        let cap = self.explicit_output_cap(call);
        if cap.is_some() && self.route.output_token_parameter().is_none() {
            return Err(GatewayError::UnsupportedCapability);
        }
        let current = match self.cache_parts(call)? {
            Some((prefix, variable)) => vec![
                OpenAiRequestMessage {
                    role: "user",
                    content: prefix,
                },
                OpenAiRequestMessage {
                    role: "user",
                    content: variable,
                },
            ],
            None => vec![OpenAiRequestMessage {
                role: "user",
                content: &call.prompt,
            }],
        };
        // ADR-042: replayed session turns precede the new briefing, in order.
        let messages: Vec<OpenAiRequestMessage<'_>> = call
            .history
            .iter()
            .flat_map(|turn| {
                [
                    OpenAiRequestMessage {
                        role: "user",
                        content: &turn.prompt,
                    },
                    OpenAiRequestMessage {
                        role: "assistant",
                        content: &turn.reply,
                    },
                ]
            })
            .chain(current)
            .collect();
        let payload = OpenAiRequestBody {
            model,
            max_tokens: cap.filter(|_| {
                self.route.output_token_parameter() == Some(OpenAiOutputTokenParameter::MaxTokens)
            }),
            max_completion_tokens: cap.filter(|_| {
                self.route.output_token_parameter()
                    == Some(OpenAiOutputTokenParameter::MaxCompletionTokens)
            }),
            messages,
        };
        let body = serde_json::to_vec(&payload)
            .expect("OpenAiRequestBody is plain data and always serializes");

        let response = self.execute(TransportRequest {
            method: "POST",
            url: format!("{base_url}/v1/chat/completions"),
            headers: openai_headers(key),
            body,
            timeout: Duration::from_secs(self.route.timeout_seconds()),
        })?;

        if (200..300).contains(&response.status) {
            parse_openai_success(&response.body).ok_or(GatewayError::MalformedOutput)
        } else {
            Err(map_openai_error(response.status, &response.body))
        }
    }

    fn cache_parts<'b>(
        &self,
        call: &'b ModelCall,
    ) -> Result<Option<(&'b str, &'b str)>, GatewayError> {
        if self.route.prompt_cache().is_none() {
            return Ok(None);
        }
        let Some(prefix) = call
            .stable_prefix
            .as_deref()
            .filter(|prefix| !prefix.trim().is_empty())
        else {
            return Ok(None);
        };
        let variable = call
            .prompt
            .strip_prefix(prefix)
            .ok_or(GatewayError::UnsupportedCapability)?;
        Ok((!variable.is_empty()).then_some((prefix, variable)))
    }

    fn explicit_output_cap(&self, call: &ModelCall) -> Option<u32> {
        match (call.max_output_tokens, self.route.max_output_tokens()) {
            (Some(node), Some(route)) => Some(node.min(route).get()),
            (Some(cap), None) | (None, Some(cap)) => Some(cap.get()),
            (None, None) => None,
        }
    }

    /// Runs `request` through `self.transport` and maps a transport-level failure (no response
    /// obtained at all) onto [`GatewayError`]. A timeout is the taxonomy's own `Timeout`; every
    /// other transport failure (connection refused, TLS failure, protocol error) is treated as
    /// the provider being unreachable right now, which §12/`outcome_for_error` retries rather
    /// than parking — the same posture as an HTTP 5xx from the provider itself.
    ///
    /// `request` is taken by value and kept, not moved into [`HttpTransport::execute`] (PR review
    /// IMPORTANT 5c): the header value built by [`anthropic_headers`]/[`openai_headers`] holds
    /// the exposed API key's plaintext, and `HttpTransport::execute` now borrows rather than
    /// consumes its argument specifically so this function can zeroize every header value the
    /// instant the call returns — success or failure — instead of leaving that copy for an
    /// ordinary, unzeroized drop whenever `request` eventually goes out of scope. `ureq`'s own
    /// internal buffering of the same bytes while it writes the request line-by-line onto the
    /// wire is outside this process's reach; the milestone doc's honest-limits section records
    /// that.
    fn execute(&self, mut request: TransportRequest) -> Result<TransportResponse, GatewayError> {
        let result = self.transport.execute(&request);
        for (_name, value) in &mut request.headers {
            value.zeroize();
        }
        result.map_err(|error| map_transport_error(&error))
    }
}

/// The one transport-failure table for every adapter in this crate (`systemone.rs` reuses it): a
/// timeout is the taxonomy's own `Timeout`; every other transport failure (connection refused,
/// TLS failure, protocol error) is the provider being unreachable right now, which
/// §12/`outcome_for_error` retries rather than parks — the same posture as an HTTP 5xx.
pub(crate) fn map_transport_error(error: &TransportError) -> GatewayError {
    match error {
        TransportError::Timeout => GatewayError::Timeout,
        TransportError::Io(_) => GatewayError::ProviderUnavailable,
    }
}

fn anthropic_headers(key: &SecretBytes) -> Vec<(String, String)> {
    key.expose(|bytes| {
        vec![
            ("content-type".to_owned(), "application/json".to_owned()),
            (
                "x-api-key".to_owned(),
                String::from_utf8_lossy(bytes).into_owned(),
            ),
            ("anthropic-version".to_owned(), ANTHROPIC_VERSION.to_owned()),
        ]
    })
}

fn openai_headers(key: &SecretBytes) -> Vec<(String, String)> {
    key.expose(|bytes| {
        vec![
            ("content-type".to_owned(), "application/json".to_owned()),
            (
                "Authorization".to_owned(),
                format!("Bearer {}", String::from_utf8_lossy(bytes)),
            ),
        ]
    })
}

// ---------------------------------------------------------------------------------------------
// Anthropic wire shapes (Messages API).
// ---------------------------------------------------------------------------------------------

#[derive(Serialize)]
struct AnthropicRequestBody<'a> {
    model: &'a str,
    max_tokens: u32,
    messages: Vec<AnthropicMessage<'a>>,
}

#[derive(Serialize)]
struct AnthropicMessage<'a> {
    role: &'a str,
    content: AnthropicMessageContent<'a>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum AnthropicMessageContent<'a> {
    Text(&'a str),
    Blocks(Vec<AnthropicTextBlock<'a>>),
}

#[derive(Serialize)]
struct AnthropicTextBlock<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    text: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_control: Option<CacheControl>,
}

#[derive(Serialize)]
struct CacheControl {
    #[serde(rename = "type")]
    kind: &'static str,
}

#[derive(Deserialize)]
struct AnthropicSuccessBody {
    #[serde(default)]
    stop_reason: Option<String>,
    content: Vec<AnthropicContentBlock>,
    #[serde(default)]
    usage: Option<AnthropicUsageBody>,
}

#[derive(Deserialize)]
struct AnthropicContentBlock {
    /// PR review MEDIUM 10: a real Anthropic response can lead with a `thinking` block (extended
    /// thinking) before the `text` block a reply is built from — blindly taking `content[0]`
    /// would read the thinking block's own `text: None` and misreport a real success as
    /// `MalformedOutput`. This field is read to find the first block whose `type` is `"text"`,
    /// never assumed to be at any fixed index.
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Default, Deserialize)]
struct AnthropicUsageBody {
    #[serde(default)]
    input_tokens: Option<u64>,
    #[serde(default)]
    output_tokens: Option<u64>,
    #[serde(default)]
    cache_read_input_tokens: Option<u64>,
    #[serde(default)]
    cache_creation_input_tokens: Option<u64>,
}

/// `None` on anything that does not shape up as a successful reply: invalid JSON, no content
/// blocks, or no block whose `type` is `"text"` (e.g. only `thinking`/tool-use blocks — tool use
/// is out of scope for this milestone, §14/05c). Takes the FIRST `"text"` block, never
/// `content[0]` unconditionally (PR review MEDIUM 10): extended thinking puts a `thinking` block
/// ahead of the `text` block in the same array, and blindly indexing `content[0]` would read that
/// thinking block's own `text: None` and misreport a genuine success as
/// [`GatewayError::MalformedOutput`].
fn parse_anthropic_success(body: &[u8]) -> Option<ModelReply> {
    let parsed: AnthropicSuccessBody = serde_json::from_slice(body).ok()?;
    let termination = normalize_termination("anthropic", parsed.stop_reason.as_deref());
    let text = parsed
        .content
        .into_iter()
        .find(|block| block.kind.as_deref() == Some("text"))
        .and_then(|block| block.text)
        .or_else(|| {
            termination
                .as_ref()
                .filter(|stop| stop.reason != ModelStopReason::Completed)
                .map(|_| String::new())
        })?;
    let reported = parsed.usage.unwrap_or_default();
    let usage = Usage {
        input_tokens: reported.input_tokens,
        output_tokens: reported.output_tokens,
        cache_read_tokens: reported.cache_read_input_tokens,
        cache_write_tokens: reported.cache_creation_input_tokens,
        input_token_semantics: Some(InputTokenSemantics::ExcludesCache),
        source: Some(UsageSource::AnthropicMessages),
    };
    Some(ModelReply {
        text,
        usage,
        termination,
    })
}

/// Fixed status-code rules (plan Task 4): Anthropic's status codes are unambiguous on their own —
/// unlike OpenAI's `429`, no Anthropic status this milestone maps needs the body to decide between
/// two different outcomes, so this does not parse one.
fn map_anthropic_error(status: u16) -> GatewayError {
    match status {
        401 => GatewayError::AuthRequired,
        403 => GatewayError::PolicyDenied,
        429 => GatewayError::RateLimited,
        529 => GatewayError::ProviderUnavailable,
        status if status >= 500 => GatewayError::ProviderUnavailable,
        _ => GatewayError::MalformedOutput,
    }
}

// ---------------------------------------------------------------------------------------------
// OpenAI wire shapes (Chat Completions API).
// ---------------------------------------------------------------------------------------------

#[derive(Serialize)]
struct OpenAiRequestBody<'a> {
    model: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_completion_tokens: Option<u32>,
    messages: Vec<OpenAiRequestMessage<'a>>,
}

#[derive(Serialize)]
struct OpenAiRequestMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct OpenAiSuccessBody {
    choices: Vec<OpenAiChoice>,
    #[serde(default)]
    usage: Option<OpenAiUsageBody>,
}

#[derive(Deserialize)]
struct OpenAiChoice {
    #[serde(default)]
    finish_reason: Option<String>,
    message: OpenAiMessageBody,
}

#[derive(Deserialize)]
struct OpenAiMessageBody {
    content: Option<String>,
}

#[derive(Default, Deserialize)]
struct OpenAiUsageBody {
    #[serde(default)]
    prompt_tokens: Option<u64>,
    #[serde(default)]
    completion_tokens: Option<u64>,
    #[serde(default)]
    prompt_tokens_details: Option<OpenAiPromptTokenDetails>,
}

#[derive(Default, Deserialize)]
struct OpenAiPromptTokenDetails {
    #[serde(default)]
    cached_tokens: Option<u64>,
    #[serde(default)]
    cache_write_tokens: Option<u64>,
}

/// `None` on anything that does not shape up as a successful reply — see
/// [`parse_anthropic_success`]'s doc comment; the same reasoning applies here.
fn parse_openai_success(body: &[u8]) -> Option<ModelReply> {
    let parsed: OpenAiSuccessBody = serde_json::from_slice(body).ok()?;
    let choice = parsed.choices.into_iter().next()?;
    let termination = normalize_termination("openai", choice.finish_reason.as_deref());
    let text = choice.message.content.or_else(|| {
        termination
            .as_ref()
            .filter(|stop| stop.reason != ModelStopReason::Completed)
            .map(|_| String::new())
    })?;
    let reported = parsed.usage.unwrap_or_default();
    let details = reported.prompt_tokens_details.unwrap_or_default();
    let usage = Usage {
        input_tokens: reported.prompt_tokens,
        output_tokens: reported.completion_tokens,
        cache_read_tokens: details.cached_tokens,
        cache_write_tokens: details.cache_write_tokens,
        input_token_semantics: Some(InputTokenSemantics::IncludesCache),
        source: Some(UsageSource::OpenaiChatCompletions),
    };
    Some(ModelReply {
        text,
        usage,
        termination,
    })
}

/// Only known bounded identifiers survive into sealed metadata. Unknown provider values may
/// contain arbitrary text (including credentials), so retain their class without echoing them.
fn normalize_termination(provider: &str, raw: Option<&str>) -> Option<ModelTermination> {
    raw.map(|raw| {
        let reason = match (provider, raw) {
            ("anthropic", "end_turn" | "stop_sequence") | ("openai", "stop") => {
                ModelStopReason::Completed
            }
            ("anthropic", "max_tokens") | ("openai", "length") => ModelStopReason::OutputLimit,
            ("anthropic", "model_context_window_exceeded") => ModelStopReason::ContextLimit,
            ("anthropic", "tool_use") | ("openai", "tool_calls" | "function_call") => {
                ModelStopReason::ToolCall
            }
            ("anthropic", "pause_turn") => ModelStopReason::Paused,
            ("anthropic", "refusal") => ModelStopReason::Refusal,
            ("openai", "content_filter") => ModelStopReason::ContentFilter,
            _ => ModelStopReason::Unknown,
        };
        ModelTermination {
            reason,
            provider_reason: if reason == ModelStopReason::Unknown {
                "unknown"
            } else {
                raw
            }
            .to_owned(),
        }
    })
}

#[derive(Deserialize)]
struct OpenAiErrorBody {
    error: OpenAiErrorDetail,
}

#[derive(Deserialize, Default)]
struct OpenAiErrorDetail {
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    code: Option<String>,
}

/// Fixed status-code rules (plan Task 4), with one genuine disambiguation: OpenAI's `429` covers
/// both hard quota exhaustion (billing/plan limit) and ordinary throttling, distinguished only by
/// the error body's `type`/`code` — see [`openai_error_is_insufficient_quota`].
fn map_openai_error(status: u16, body: &[u8]) -> GatewayError {
    match status {
        401 => GatewayError::AuthRequired,
        403 => GatewayError::PolicyDenied,
        429 => {
            if openai_error_is_insufficient_quota(body) {
                GatewayError::QuotaExhausted
            } else {
                GatewayError::RateLimited
            }
        }
        status if status >= 500 => GatewayError::ProviderUnavailable,
        _ => GatewayError::MalformedOutput,
    }
}

/// The one place in this file the plan's sabotage step (Task 4 Step 6) targets: inverting the
/// `if`/`else` in [`map_openai_error`]'s `429` arm — not this predicate itself — is the prescribed
/// sabotage, chosen because it is observable as exactly the two OpenAI 429 tests swapping their
/// expected error. An unparseable body (or a body with neither field set to
/// `"insufficient_quota"`) is "not quota", i.e. ordinary rate limiting — the more common case, and
/// the one that does not imply the account is out of budget.
fn openai_error_is_insufficient_quota(body: &[u8]) -> bool {
    let Ok(parsed) = serde_json::from_slice::<OpenAiErrorBody>(body) else {
        return false;
    };
    parsed.error.kind.as_deref() == Some(OPENAI_INSUFFICIENT_QUOTA)
        || parsed.error.code.as_deref() == Some(OPENAI_INSUFFICIENT_QUOTA)
}
