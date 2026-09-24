//! The System One adapter: `POST {baseUrl}/v1/systemone` with a Bearer key, a [`JudgeRequest`]
//! body and a [`JudgeReply`] reply (docs.typesafe.ai/api). Mirrors `byok.rs`'s shape over the
//! same [`HttpTransport`]; serves the JUDGE door only
//! (architect-judgments plan, D1): a chat provider here, or
//! this provider on the draft door (`byok.rs`), is [`GatewayError::UnsupportedCapability`] before
//! any request is built.

use std::sync::Arc;
use std::time::Duration;

use graphhelm_events::SecretBytes;
use graphhelm_gateway::judgment::{JudgeReply, JudgeRequest};
use graphhelm_gateway::manifest::{ModelRoute, Transport};
use graphhelm_gateway::taxonomy::GatewayError;
use zeroize::Zeroize;

use crate::transport::{HttpTransport, TransportRequest};

/// The `direct_api` provider name a manifest route carries to be served by this adapter.
pub const TYPESAFE_PROVIDER: &str = "typesafe";
/// The path appended to the route's `baseUrl`.
pub const SYSTEMONE_PATH: &str = "/v1/systemone";

/// One System One adapter bound to a specific `direct_api`/`typesafe` [`ModelRoute`] and
/// [`HttpTransport`]. Construction does not touch the network; each [`Self::call`] places exactly
/// one HTTP request.
pub struct SystemOneAdapter<'a> {
    route: &'a ModelRoute,
    transport: Arc<dyn HttpTransport>,
}

impl<'a> SystemOneAdapter<'a> {
    #[must_use]
    pub fn new(route: &'a ModelRoute, transport: Arc<dyn HttpTransport>) -> Self {
        Self { route, transport }
    }

    /// Places one judgment call. The route's `model` overrides `request.model` when present, so
    /// the manifest decides which System One model answers. `key` is exposed only while the
    /// Bearer header is built and that header value is zeroized the instant the transport
    /// returns, as in `byok.rs`.
    ///
    /// # Errors
    /// `UnsupportedCapability` for a route that is not `direct_api`/`typesafe`; the documented
    /// statuses mapped by [`map_status`]; `MalformedOutput` for a 2xx body that is not a reply;
    /// `ProviderUnavailable`/`Timeout` from the transport.
    pub fn call(
        &self,
        key: &SecretBytes,
        request: &JudgeRequest,
    ) -> Result<JudgeReply, GatewayError> {
        if self.route.transport() != Transport::DirectApi
            || self.route.provider() != TYPESAFE_PROVIDER
        {
            return Err(GatewayError::UnsupportedCapability);
        }
        let base_url = self
            .route
            .base_url()
            .expect("direct_api routes carry baseUrl — enforced by manifest validation");
        let mut request = request.clone();
        if let Some(model) = self.route.model() {
            request.model = model.to_owned();
        }
        let body =
            serde_json::to_vec(&request).expect("JudgeRequest is plain data and always serializes");
        let mut transport_request = TransportRequest {
            method: "POST",
            url: format!("{base_url}{SYSTEMONE_PATH}"),
            headers: bearer_headers(key),
            body,
            timeout: Duration::from_secs(self.route.timeout_seconds()),
        };
        let result = self.transport.execute(&transport_request);
        for (_name, value) in &mut transport_request.headers {
            value.zeroize();
        }
        let response = result.map_err(|error| crate::byok::map_transport_error(&error))?;
        if (200..300).contains(&response.status) {
            serde_json::from_slice::<JudgeReply>(&response.body)
                .map_err(|_| GatewayError::MalformedOutput)
        } else {
            Err(map_status(response.status))
        }
    }
}

fn bearer_headers(key: &SecretBytes) -> Vec<(String, String)> {
    key.expose(|bytes| {
        vec![
            ("content-type".to_owned(), "application/json".to_owned()),
            (
                "authorization".to_owned(),
                format!("Bearer {}", String::from_utf8_lossy(bytes)),
            ),
        ]
    })
}

/// docs.typesafe.ai/api "Errors": 401, 422, 429, 529; 403 and 5xx by the same rule the
/// Anthropic adapter uses. A 422 is the compiler having built a bad question — a defect to see,
/// never a retry — so it is `MalformedOutput`, the taxonomy's "could not be understood" arm.
fn map_status(status: u16) -> GatewayError {
    match status {
        401 => GatewayError::AuthRequired,
        403 => GatewayError::PolicyDenied,
        429 => GatewayError::RateLimited,
        529 => GatewayError::ProviderUnavailable,
        status if status >= 500 => GatewayError::ProviderUnavailable,
        _ => GatewayError::MalformedOutput,
    }
}
