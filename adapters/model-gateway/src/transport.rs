//! The `HttpTransport` boundary the BYOK adapters (`byok.rs`) speak through, and its production
//! implementation over `ureq` (ADR-025, `docs/reference/REFERENCE_STACK_AND_ADRS.md`).
//!
//! A non-2xx HTTP response is not a transport failure: [`TransportError`] covers only the cases
//! where no HTTP response was ever obtained at all (a connection/IO failure, or the request's own
//! deadline elapsing first). Everything else — a 429, a 401, a 200 with an unparseable body — is a
//! fully-formed [`TransportResponse`] that `byok.rs`'s per-provider status-mapping tables decide
//! the meaning of. [`UreqTransport`] is therefore configured so `ureq` never turns a non-2xx status
//! into an `Err` on our behalf.

use std::time::Duration;

use ureq::RequestExt as _;

/// One outbound HTTP request. `headers` may carry a credential value (`x-api-key`,
/// `Authorization: Bearer ...`) — see the manual [`std::fmt::Debug`] impl below, which redacts
/// every header *value* while still naming the header, so a log or test failure message is
/// diagnosable without leaking the secret.
pub struct TransportRequest {
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub timeout: Duration,
}

impl std::fmt::Debug for TransportRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let redacted_headers: Vec<(&str, &str)> = self
            .headers
            .iter()
            .map(|(name, _value)| (name.as_str(), "[redacted]"))
            .collect();
        f.debug_struct("TransportRequest")
            .field("method", &self.method)
            .field("url", &self.url)
            .field("headers", &redacted_headers)
            .field("body_len", &self.body.len())
            .field("timeout", &self.timeout)
            .finish()
    }
}

/// The raw HTTP response: status and body, unparsed. Provider-specific parsing happens in
/// `byok.rs`. Never carries a credential — providers do not echo the caller's API key back.
#[derive(Debug)]
pub struct TransportResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

/// Every way a transport can fail to obtain an HTTP response at all. A non-2xx status is not a
/// member of this enum — see the module doc comment.
#[derive(Debug)]
pub enum TransportError {
    /// A connection, TLS, protocol, or read/write failure. Carries a short diagnostic message —
    /// built from the underlying error's own `Display`, which (like this enum) never has access
    /// to a header value or request body to leak in the first place.
    Io(String),
    /// The request's own deadline (`TransportRequest::timeout`) elapsed before a response
    /// arrived.
    Timeout,
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(message) => write!(f, "transport error: {message}"),
            Self::Timeout => write!(f, "transport error: request timed out"),
        }
    }
}

impl std::error::Error for TransportError {}

/// The boundary the BYOK adapters call through. Production code uses [`UreqTransport`]; tests
/// substitute a fake bound to a local `TcpListener` (`tests/byok_adapters.rs`), which is exactly
/// why this is a trait object (`Arc<dyn HttpTransport>`) rather than a concrete `UreqTransport`
/// baked into `ByokAdapter`.
///
/// Takes `request` by reference rather than by value (PR review IMPORTANT 5c): `byok.rs`'s
/// `ByokAdapter::execute` needs its own `TransportRequest` back after the call completes, so it
/// can zeroize the credential-bearing header value it built rather than leaving that copy for an
/// unzeroized drop. A borrowing signature is what makes that possible without this trait growing
/// a second "give the request back" return channel.
pub trait HttpTransport: Send + Sync {
    /// # Errors
    /// Returns [`TransportError`] only when no HTTP response was obtained — see the module doc
    /// comment. A non-2xx status is `Ok`.
    fn execute(&self, request: &TransportRequest) -> Result<TransportResponse, TransportError>;
}

/// The production [`HttpTransport`]: a synchronous `ureq` agent — matching the CLI's synchronous
/// command layer; the async driver of Milestone 05d bridges with `spawn_blocking` when it arrives
/// (ADR-025) — configured so a non-2xx response comes back as an `Ok(TransportResponse)`, never an
/// `Err`.
pub struct UreqTransport {
    agent: ureq::Agent,
}

impl UreqTransport {
    #[must_use]
    pub fn new() -> Self {
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            // ureq's redirect handling (`ureq-proto`'s `Call<Redirect>::as_new_call`) strips only
            // the `Authorization`, `Cookie`, and `Content-Length` headers from the request it
            // re-sends to a redirect's `Location` — a custom credential header like Anthropic's
            // `x-api-key` (or OpenAI's own `Authorization`, kept by `RedirectAuthHeaders::Never`
            // only being the *default*, not a hard stop) survives untouched and would be re-sent
            // to whatever host a 302 names. `/v1/messages` and `/v1/chat/completions` never
            // legitimately redirect, and each hop consumes its own `timeout_per_call` budget, so
            // a malicious or misconfigured redirect chain could otherwise both exfiltrate the
            // caller's credential to an arbitrary host and stack this request's timeout up to
            // ureq's default 10 hops deep. `max_redirects(0)` disables redirect-following
            // outright: a 3xx response comes back as an ordinary `Ok(TransportResponse)` (ureq
            // never treats an un-followed redirect as an error), which `byok.rs`'s catch-all
            // status mapping — no fixed rule names any 3xx — turns into `MalformedOutput`.
            .max_redirects(0)
            .build();
        Self {
            agent: ureq::Agent::new_with_config(config),
        }
    }
}

impl Default for UreqTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpTransport for UreqTransport {
    fn execute(&self, request: &TransportRequest) -> Result<TransportResponse, TransportError> {
        let mut builder = ureq::http::Request::builder()
            .method(request.method)
            .uri(request.url.as_str());
        for (name, value) in &request.headers {
            builder = builder.header(name.as_str(), value.as_str());
        }
        let http_request = builder
            .body(request.body.clone())
            .map_err(|error| TransportError::Io(error.to_string()))?;

        let outcome = http_request
            .with_agent(&self.agent)
            .configure()
            .timeout_per_call(Some(request.timeout))
            .run();

        let mut response = outcome.map_err(map_ureq_error)?;

        let status = response.status().as_u16();
        // PR review MEDIUM 13: a stall while reading the response BODY (the server accepts the
        // request, then never finishes sending it) times out through the exact same
        // `ureq::Error::Timeout` path as a stall establishing the connection or awaiting headers
        // — `ureq`'s transport layer (`tcp.rs`) maps the underlying socket's
        // `io::ErrorKind::TimedOut` onto `Error::Timeout` before it ever reaches this call, for
        // every read on the connection, not only the first. The previous code funneled every
        // `body_mut().read_to_vec()` failure through the `Io` arm regardless, which reported a
        // body-read timeout as `GatewayError::ProviderUnavailable` (retried as an ordinary
        // transport failure) instead of `GatewayError::Timeout`. [`map_ureq_error`] is the same
        // classification used above so both call sites can never drift apart again.
        let body = response.body_mut().read_to_vec().map_err(map_ureq_error)?;

        Ok(TransportResponse { status, body })
    }
}

/// Classifies a `ureq::Error` the same way at every call site in this module: a timeout —
/// wherever in the request/response lifecycle it happened — is [`TransportError::Timeout`];
/// everything else is [`TransportError::Io`], carrying only the error's own `Display` text (never
/// anything from the request, which may carry a credential header).
fn map_ureq_error(error: ureq::Error) -> TransportError {
    match error {
        ureq::Error::Timeout(_) => TransportError::Timeout,
        other => TransportError::Io(other.to_string()),
    }
}
