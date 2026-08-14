//! Error taxonomy (§17), route health (§18), and the capacity→outcome mapping (§12).
//!
//! This module is the pure center of the gateway's failure policy: given a [`GatewayError`] an
//! adapter observed, it says exactly which [`NodeOutcome`] the execution engine should record for
//! the node that made the call, and separately, what that observation implies about the route's
//! ongoing health. The adapters that actually detect failures (`adapters/model-gateway`, Milestone
//! 05b Tasks 3-5) call into this module rather than deciding consequences for themselves — the
//! policy lives in one place.

use graphhelm_protocols::NodeOutcome;

/// The closed error vocabulary a route adapter reports, per §17. Every variant here must have an
/// arm in [`outcome_for_error`] — see that function's doc comment for why the match has no
/// wildcard.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GatewayError {
    AuthRequired,
    AuthRevoked,
    QuotaExhausted,
    RateLimited,
    ProviderUnavailable,
    ModelRemoved,
    ContextTooLarge,
    MalformedOutput,
    ToolDenied,
    RuntimeCrashed,
    UnsupportedCapability,
    PolicyDenied,
    Cancelled,
    Timeout,
}

impl std::fmt::Display for GatewayError {
    /// Every arm is fixed, static text: `GatewayError` carries no fields (it is a `Copy` enum),
    /// so there is nothing dynamic here for a redaction test to catch — but callers format this
    /// alongside adapter-level context (`adapters/model-gateway/src/byok.rs`), and every error
    /// type elsewhere in this workspace (`ManifestError`, `BrokerError`) implements `Display`, so
    /// this keeps `GatewayError` consistent with that convention rather than being the exception.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Self::AuthRequired => "authentication is required",
            Self::AuthRevoked => "authentication was revoked",
            Self::QuotaExhausted => "quota is exhausted",
            Self::RateLimited => "the route is rate limited",
            Self::ProviderUnavailable => "the provider is unavailable",
            Self::ModelRemoved => "the model has been removed",
            Self::ContextTooLarge => "the context exceeds what the model accepts",
            Self::MalformedOutput => "the provider's reply could not be parsed",
            Self::ToolDenied => "the requested tool use was denied",
            Self::RuntimeCrashed => "the runtime crashed",
            Self::UnsupportedCapability => "the route does not support this capability",
            Self::PolicyDenied => "the request was denied by policy",
            Self::Cancelled => "the call was cancelled",
            Self::Timeout => "the call timed out",
        };
        write!(f, "{text}")
    }
}

impl std::error::Error for GatewayError {}

/// A route's operating state, per §18. Distinct from [`GatewayError`]: an error is what one call
/// reported; health is what that implies, if anything, about the route going forward.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RouteHealth {
    Available,
    Degraded,
    WaitingReset,
    AuthRequired,
    Unavailable,
    Disabled,
}

/// Maps one call's failure to the outcome the execution engine records for the node that made it.
///
/// §12 is the normative policy encoded here: capacity exhaustion — `quota_exhausted`,
/// `rate_limited`, and the two auth failures (§12 step 1 names "limit/throttle/auth failure"
/// together as the pause triggers) — parks the node as [`NodeOutcome::NeedsCapacity`] with no
/// automatic paid fallback; transient provider or runtime trouble is retryable; a request the
/// gateway can never satisfy as asked is terminal; cancellation passes through unchanged.
///
/// This is one exhaustive match with **no wildcard arm**, deliberately: every [`GatewayError`]
/// variant must have a chosen consequence. Adding a fifteenth variant without deciding its outcome
/// here must fail the build, not silently fall through to a default.
#[must_use]
pub const fn outcome_for_error(error: GatewayError) -> NodeOutcome {
    use GatewayError as E;
    use NodeOutcome as O;

    match error {
        E::QuotaExhausted | E::RateLimited | E::AuthRequired | E::AuthRevoked => O::NeedsCapacity,

        E::ProviderUnavailable | E::Timeout | E::RuntimeCrashed | E::MalformedOutput => {
            O::RetryableFailure
        }

        E::ContextTooLarge
        | E::ModelRemoved
        | E::UnsupportedCapability
        | E::PolicyDenied
        | E::ToolDenied => O::TerminalFailure,

        E::Cancelled => O::Cancelled,
    }
}

/// What one call's failure implies about the route's ongoing health, if anything.
///
/// Most errors are a fact about that one call, not about the route: a context that was too large
/// this time, or a call the owner cancelled, says nothing about whether the route will serve the
/// next request. Only the classes below update route health; everything else returns `None` and
/// leaves health exactly where the caller last observed it.
#[must_use]
pub const fn health_for_error(error: GatewayError) -> Option<RouteHealth> {
    use GatewayError as E;

    match error {
        E::QuotaExhausted | E::RateLimited => Some(RouteHealth::WaitingReset),
        E::AuthRequired | E::AuthRevoked => Some(RouteHealth::AuthRequired),
        E::ProviderUnavailable => Some(RouteHealth::Degraded),
        _ => None,
    }
}
