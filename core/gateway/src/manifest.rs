//! Route manifest types and structural validation.
//!
//! `docs/models/UNIVERSAL_MODEL_GATEWAY.md` §4 defines the route manifest; this module implements
//! the Milestone 05b subset of it (gateway-slice plan, Task 1): the
//! two transports the gateway drives this milestone — a direct HTTPS API call the gateway places
//! itself, or an official CLI the gateway spawns as a native runtime and lets authenticate on its
//! own — and the structural rule that keeps their billing and authentication mutually exclusive
//! (§20: BYOK and subscription are distinct billing relationships; a route cannot claim both).
//! `capabilities`, `restrictions`, `health` and `capacity` from the full spec shape are deferred, as
//! is the YAML encoding itself (this milestone's on-disk manifest is JSON) — see the plan's file map.
//!
//! A `RouteManifest` obtained from [`RouteManifest::from_json`] has already passed every structural
//! rule this module enforces; there is no other way to construct one.

use std::collections::BTreeSet;
use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};

/// Manifest source text larger than this is refused before it is even parsed, so a malformed or
/// hostile file cannot force an unbounded parse.
pub const MAX_MANIFEST_BYTES: usize = 256 * 1024;

/// Routes declared beyond this count are refused. A manifest is hand-authored operator
/// configuration, not a database; there is no legitimate case for more.
pub const MAX_ROUTES: usize = 64;

/// A route's `timeoutSeconds` beyond this is refused (Milestone 05b final review). The
/// native-runtime adapter (`adapters/model-gateway/src/runtime.rs`) computes
/// `Instant::now() + Duration::from_secs(route.timeout_seconds())` for its deadline, and `Instant`
/// addition can overflow — panicking — for a large enough duration; 24 hours is comfortably beyond
/// any legitimate native-runtime CLI invocation this milestone targets. The bound applies to every
/// route regardless of transport, mirroring how `timeout_seconds` itself is not restricted to
/// [`Transport::NativeRuntime`] structurally (only the native-runtime adapter reads it today).
pub const MAX_TIMEOUT_SECONDS: u64 = 86_400;

/// Default deadline, in seconds, for a route that does not declare `timeoutSeconds` explicitly.
///
/// Task 1 shipped no per-route timeout field at all — nothing in this milestone's manifest shape
/// needed one yet. Milestone 05b Task 5
/// (gateway-slice plan) adds this field and constant: the
/// native-runtime adapter (`adapters/model-gateway/src/runtime.rs`) spawns an official CLI as a
/// subprocess and must bound how long it waits before killing a hung one, and a spawned CLI's own
/// work can legitimately take much longer than one HTTP call, which is worth a real per-route
/// manifest control rather than a hardcoded adapter-crate constant. Task 4's BYOK adapters
/// (`adapters/model-gateway/src/byok.rs`) initially fell back to a private `BYOK_REQUEST_TIMEOUT`
/// constant for exactly this gap, since `timeoutSeconds` did not exist yet when Task 4 landed;
/// the PR review that closed out this milestone (finding MEDIUM 13) retired that constant once
/// this field did exist, so every route — `direct_api` and `native_runtime` alike — now reads its
/// deadline from here.
pub const DEFAULT_TIMEOUT_SECONDS: u64 = 300;

const MAX_ROUTE_ID_LEN: usize = 64;

/// How a route reaches the model: a direct HTTPS call the gateway makes itself, or an official CLI
/// the gateway spawns as a subprocess.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    DirectApi,
    NativeRuntime,
}

/// How the caller proves who they are. Tied 1:1 to [`Transport`] and to [`BillingMode`] — see
/// [`RouteManifest::from_json`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Authentication {
    ApiKey,
    AccountSubscription,
}

/// How usage against a route is metered. BYOK and subscription are distinct billing relationships
/// (§20); this milestone recognizes no third mode.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BillingMode {
    PerToken,
    SubscriptionQuota,
}

/// The official CLI a `native_runtime` route spawns.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeKind {
    ClaudeCode,
    Codex,
}

/// The closed work-profile vocabulary a route may advertise itself for. Scoring routes within a
/// profile (§8.3) is deferred; this milestone only tags routes with the profiles they serve.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkProfile {
    FastClassification,
    CheapExtraction,
    BalancedReasoning,
    CriticalReasoning,
    LongContextSynthesis,
    SoftwareExecution,
    VisionReasoning,
    CreativeGeneration,
    SourceGroundedResearch,
    LocalPrivate,
    HighReliabilityStructuredOutput,
}

/// The subprocess command a `native_runtime` route spawns. Nothing here is a secret: the manifest
/// structurally cannot give a native route a `credentialRef` to smuggle one in from (see
/// [`RouteManifest::from_json`]), and the adapter that eventually spawns this command sends the
/// prompt over stdin, never as an argument (Milestone 05b Task 5).
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeCommand {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
}

/// One validated route. The only way to obtain one is through [`RouteManifest::from_json`], so a
/// live `ModelRoute` has already satisfied every structural rule this module enforces.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelRoute {
    id: String,
    provider: String,
    transport: Transport,
    authentication: Authentication,
    billing_mode: BillingMode,
    base_url: Option<String>,
    model: Option<String>,
    credential_ref: Option<String>,
    runtime: Option<RuntimeKind>,
    command: Option<RuntimeCommand>,
    profiles: Vec<WorkProfile>,
    enabled: bool,
    /// How long a caller may wait on this route before giving up. Only the native-runtime
    /// adapter reads this today (Milestone 05b Task 5); it is not restricted to
    /// [`Transport::NativeRuntime`] routes structurally, the same way `profiles`/`enabled` apply
    /// to both transports, so a future `direct_api` consumer can start reading it without a
    /// manifest shape change.
    #[serde(default = "default_timeout_seconds")]
    timeout_seconds: u64,
}

const fn default_timeout_seconds() -> u64 {
    DEFAULT_TIMEOUT_SECONDS
}

impl ModelRoute {
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[must_use]
    pub fn provider(&self) -> &str {
        &self.provider
    }

    #[must_use]
    pub const fn transport(&self) -> Transport {
        self.transport
    }

    #[must_use]
    pub const fn authentication(&self) -> Authentication {
        self.authentication
    }

    #[must_use]
    pub const fn billing_mode(&self) -> BillingMode {
        self.billing_mode
    }

    #[must_use]
    pub fn base_url(&self) -> Option<&str> {
        self.base_url.as_deref()
    }

    #[must_use]
    pub fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }

    #[must_use]
    pub fn credential_ref(&self) -> Option<&str> {
        self.credential_ref.as_deref()
    }

    #[must_use]
    pub const fn runtime(&self) -> Option<RuntimeKind> {
        self.runtime
    }

    #[must_use]
    pub fn command(&self) -> Option<&RuntimeCommand> {
        self.command.as_ref()
    }

    #[must_use]
    pub fn profiles(&self) -> &[WorkProfile] {
        &self.profiles
    }

    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.enabled
    }

    #[must_use]
    pub const fn timeout_seconds(&self) -> u64 {
        self.timeout_seconds
    }
}

/// The full manifest: a version tag (reserved for future evolution; this milestone only requires it
/// to be present) and the routes it declares.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RouteManifest {
    manifest_version: u32,
    routes: Vec<ModelRoute>,
}

/// Every way [`RouteManifest::from_json`] can refuse a manifest. `Display` names the route id and
/// the rule that was violated; it never echoes manifest content back — [`Self::Oversize`] in
/// particular reports sizes, never the bytes that made the manifest oversize.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ManifestError {
    /// The source text exceeded [`MAX_MANIFEST_BYTES`] before it was parsed.
    Oversize { bytes: usize, max: usize },
    /// The manifest declared more routes than [`MAX_ROUTES`].
    TooManyRoutes { count: usize, max: usize },
    /// The text was not valid JSON for this schema, or carried a field this schema does not
    /// recognize (`deny_unknown_fields`).
    ///
    /// Carries only `serde_json`'s error *position* and broad *category* — never its message.
    /// `serde_json`'s own `Display` embeds manifest content verbatim (an unknown field's name, an
    /// unrecognized enum variant, or a wrong-typed value's literal string all appear quoted
    /// inside the message it builds); an operator who pastes a credential into the wrong field by
    /// mistake would otherwise see it echoed straight through `ManifestError::Display` into
    /// `GHCLI009`'s CLI/CI output (Milestone 05b final review).
    Parse {
        /// 1-indexed line at which `serde_json` stopped.
        line: usize,
        /// 1-indexed column at which `serde_json` stopped.
        column: usize,
        /// `serde_json::error::Category`, rendered as one of `"io"`/`"syntax"`/`"data"`/`"eof"` —
        /// see [`parse_error_category`].
        category: &'static str,
    },
    /// Two routes declared the same `id`.
    DuplicateRouteId { id: String },
    /// A route's `authentication` and `billingMode` did not match the pair its `transport` requires.
    /// §20: BYOK (`api_key` / `per_token`) and subscription (`account_subscription` /
    /// `subscription_quota`) are distinct billing relationships; a route cannot mix them.
    BillingTransportMismatch { route_id: String },
    /// A route's `baseUrl` used `http://` against a host that is not loopback. TLS is the default
    /// posture; cleartext is permitted only to a local fake or a local endpoint.
    ///
    /// Carries only the route id, never the `baseUrl` value: an operator who accidentally pastes
    /// a credential (or any other sensitive value) into `baseUrl` must not see it echoed back
    /// through `Display`/`Debug` into CLI or CI logs (Milestone 05b PR review, finding 16a).
    CleartextRemoteUrl { route_id: String },
    /// A route's `baseUrl` carried a trailing `/`. A clear rule (no trailing slash, ever) beats
    /// silently normalizing it away — callers that join `base_url` with a path must not have to
    /// guess whether a double slash can occur.
    ///
    /// Carries only the route id, for the same reason as [`Self::CleartextRemoteUrl`].
    TrailingSlashBaseUrl { route_id: String },
    /// A route's `baseUrl` authority carried userinfo, which could expose credentials through
    /// listings, logs, or URL clients.
    UserinfoBaseUrl { route_id: String },
    /// Any other per-route structural rule: route id shape, a `direct_api` route missing
    /// `baseUrl`/`model`/`credentialRef` or carrying `runtime`/`command`, or a `native_runtime`
    /// route missing `runtime`/`command` (or carrying an empty `command.program`) or carrying
    /// `credentialRef`/`baseUrl`.
    StructuralViolation {
        route_id: String,
        rule: &'static str,
    },
    /// A route's `timeoutSeconds` exceeded [`MAX_TIMEOUT_SECONDS`].
    TimeoutTooLarge {
        route_id: String,
        seconds: u64,
        max: u64,
    },
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Oversize { bytes, max } => {
                write!(
                    f,
                    "manifest is {bytes} bytes, exceeding the {max}-byte limit"
                )
            }
            Self::TooManyRoutes { count, max } => write!(
                f,
                "manifest declares {count} routes, exceeding the {max}-route limit"
            ),
            Self::Parse {
                line,
                column,
                category,
            } => {
                write!(
                    f,
                    "manifest is not valid ({category} error at line {line}, column {column})"
                )
            }
            Self::DuplicateRouteId { id } => {
                write!(f, "route id '{id}' is declared more than once")
            }
            Self::BillingTransportMismatch { route_id } => write!(
                f,
                "route '{route_id}': authentication and billingMode must match the transport"
            ),
            Self::CleartextRemoteUrl { route_id } => write!(
                f,
                "route '{route_id}': baseUrl uses http:// against a non-loopback host"
            ),
            Self::TrailingSlashBaseUrl { route_id } => write!(
                f,
                "route '{route_id}': baseUrl must not end with a trailing '/'"
            ),
            Self::UserinfoBaseUrl { route_id } => {
                write!(f, "route '{route_id}': baseUrl must not contain userinfo")
            }
            Self::StructuralViolation { route_id, rule } => {
                write!(f, "route '{route_id}': {rule}")
            }
            Self::TimeoutTooLarge {
                route_id,
                seconds,
                max,
            } => write!(
                f,
                "route '{route_id}': timeoutSeconds {seconds} exceeds the {max}-second limit"
            ),
        }
    }
}

impl std::error::Error for ManifestError {}

impl RouteManifest {
    /// Parses and validates a manifest, in order: byte bound, then JSON structure (rejecting
    /// unrecognized fields), then each route's structural rules, then a duplicate-id scan.
    ///
    /// # Errors
    /// See [`ManifestError`] for every way this can refuse the input.
    pub fn from_json(text: &str) -> Result<Self, ManifestError> {
        if text.len() > MAX_MANIFEST_BYTES {
            return Err(ManifestError::Oversize {
                bytes: text.len(),
                max: MAX_MANIFEST_BYTES,
            });
        }

        let manifest: Self = serde_json::from_str(text).map_err(|source| ManifestError::Parse {
            line: source.line(),
            column: source.column(),
            category: parse_error_category(&source),
        })?;

        if manifest.routes.len() > MAX_ROUTES {
            return Err(ManifestError::TooManyRoutes {
                count: manifest.routes.len(),
                max: MAX_ROUTES,
            });
        }

        for route in &manifest.routes {
            validate_route(route)?;
        }

        let mut seen = BTreeSet::new();
        for route in &manifest.routes {
            if !seen.insert(route.id.as_str()) {
                return Err(ManifestError::DuplicateRouteId {
                    id: route.id.clone(),
                });
            }
        }

        Ok(manifest)
    }

    #[must_use]
    pub const fn manifest_version(&self) -> u32 {
        self.manifest_version
    }

    #[must_use]
    pub fn routes(&self) -> &[ModelRoute] {
        &self.routes
    }
}

/// The `(authentication, billingMode)` pair a transport requires. §20 treats agreement between the
/// three as one structural fact, not independent choices.
const fn required_authentication_and_billing(
    transport: Transport,
) -> (Authentication, BillingMode) {
    match transport {
        Transport::DirectApi => (Authentication::ApiKey, BillingMode::PerToken),
        Transport::NativeRuntime => (
            Authentication::AccountSubscription,
            BillingMode::SubscriptionQuota,
        ),
    }
}

/// Maps a `serde_json` parse failure's [`serde_json::error::Category`] onto the static string
/// [`ManifestError::Parse`] carries — never the error's own `Display`, which embeds manifest
/// content (see that variant's doc comment).
fn parse_error_category(error: &serde_json::Error) -> &'static str {
    use serde_json::error::Category;
    match error.classify() {
        Category::Io => "io",
        Category::Syntax => "syntax",
        Category::Data => "data",
        Category::Eof => "eof",
    }
}

fn validate_route(route: &ModelRoute) -> Result<(), ManifestError> {
    validate_route_id(&route.id)?;

    if route.timeout_seconds > MAX_TIMEOUT_SECONDS {
        return Err(ManifestError::TimeoutTooLarge {
            route_id: route.id.clone(),
            seconds: route.timeout_seconds,
            max: MAX_TIMEOUT_SECONDS,
        });
    }

    let (required_auth, required_billing) = required_authentication_and_billing(route.transport);
    if route.authentication != required_auth || route.billing_mode != required_billing {
        return Err(ManifestError::BillingTransportMismatch {
            route_id: route.id.clone(),
        });
    }

    match route.transport {
        Transport::DirectApi => validate_direct_api(route),
        Transport::NativeRuntime => validate_native_runtime(route),
    }
}

fn validate_route_id(id: &str) -> Result<(), ManifestError> {
    let valid_length = !id.is_empty() && id.len() <= MAX_ROUTE_ID_LEN;
    let valid_chars = id
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_');
    if valid_length && valid_chars {
        Ok(())
    } else {
        Err(ManifestError::StructuralViolation {
            route_id: id.to_owned(),
            rule: "route id must be 1-64 characters of [a-z0-9_]",
        })
    }
}

/// `direct_api` requires everything the gateway needs to place the call itself, and forbids the
/// native-runtime fields — a route cannot be both kinds at once.
fn validate_direct_api(route: &ModelRoute) -> Result<(), ManifestError> {
    if route.runtime.is_some() || route.command.is_some() {
        return Err(ManifestError::StructuralViolation {
            route_id: route.id.clone(),
            rule: "direct_api routes must not carry runtime or command",
        });
    }

    // Milestone 05b Task 4 (gateway-slice plan): the BYOK
    // adapters in `adapters/model-gateway/src/byok.rs` speak exactly two chat provider wire
    // formats. `typesafe` is served by `adapters/model-gateway/src/systemone.rs` on the JUDGE
    // door only (architect-judgments plan, Task 3): a System One
    // model answers typed questions and never drafts text. A `direct_api` route naming any other
    // provider would parse here but have no adapter able to place its call — refusing it at
    // manifest load time turns that into a load-time error instead of a confusing runtime one
    // the first time the route is dispatched.
    if !matches!(route.provider.as_str(), "anthropic" | "openai" | "typesafe") {
        return Err(ManifestError::StructuralViolation {
            route_id: route.id.clone(),
            rule: "direct_api routes must use provider \"anthropic\", \"openai\" or \"typesafe\"",
        });
    }

    let base_url = route
        .base_url
        .as_ref()
        .ok_or_else(|| ManifestError::StructuralViolation {
            route_id: route.id.clone(),
            rule: "direct_api routes require baseUrl",
        })?;

    if route.model.is_none() {
        return Err(ManifestError::StructuralViolation {
            route_id: route.id.clone(),
            rule: "direct_api routes require model",
        });
    }

    if route.credential_ref.is_none() {
        return Err(ManifestError::StructuralViolation {
            route_id: route.id.clone(),
            rule: "direct_api routes require credentialRef",
        });
    }

    validate_base_url(&route.id, base_url)
}

/// `native_runtime` owns its own authentication (design §6.3): the manifest must not be able to
/// route a broker secret into it, so `credentialRef`/`baseUrl` are structurally forbidden here, not
/// merely unused.
fn validate_native_runtime(route: &ModelRoute) -> Result<(), ManifestError> {
    if route.base_url.is_some() {
        return Err(ManifestError::StructuralViolation {
            route_id: route.id.clone(),
            rule: "native_runtime routes must not carry baseUrl",
        });
    }

    if route.credential_ref.is_some() {
        return Err(ManifestError::StructuralViolation {
            route_id: route.id.clone(),
            rule: "native_runtime routes must not carry credentialRef",
        });
    }

    if route.runtime.is_none() {
        return Err(ManifestError::StructuralViolation {
            route_id: route.id.clone(),
            rule: "native_runtime routes require runtime",
        });
    }

    let command = route
        .command
        .as_ref()
        .ok_or_else(|| ManifestError::StructuralViolation {
            route_id: route.id.clone(),
            rule: "native_runtime routes require command",
        })?;

    if command.program.trim().is_empty() {
        return Err(ManifestError::StructuralViolation {
            route_id: route.id.clone(),
            rule: "native_runtime command.program must not be empty",
        });
    }

    Ok(())
}

/// `https://` is always allowed. `http://` is allowed only to loopback (`localhost`, `127.0.0.0/8`,
/// `::1`) — local fakes in tests and, later, local OpenAI-compatible endpoints. Parsing stays
/// dependency-free: split the scheme, take the authority up to the first `/`, strip the port, then
/// test the host.
fn validate_base_url(route_id: &str, base_url: &str) -> Result<(), ManifestError> {
    if base_url.ends_with('/') {
        return Err(ManifestError::TrailingSlashBaseUrl {
            route_id: route_id.to_owned(),
        });
    }

    if let Some(authority_and_path) = base_url.strip_prefix("https://") {
        if authority_and_path
            .split(['/', '?', '#'])
            .next()
            .unwrap_or_default()
            .contains('@')
        {
            return Err(ManifestError::UserinfoBaseUrl {
                route_id: route_id.to_owned(),
            });
        }
        return Ok(());
    }

    if let Some(authority_and_path) = base_url.strip_prefix("http://") {
        let authority = authority_and_path
            .split(['/', '?', '#'])
            .next()
            .unwrap_or_default();
        if authority.contains('@') {
            return Err(ManifestError::UserinfoBaseUrl {
                route_id: route_id.to_owned(),
            });
        }
        if is_loopback_authority(authority) {
            return Ok(());
        }
        return Err(ManifestError::CleartextRemoteUrl {
            route_id: route_id.to_owned(),
        });
    }

    Err(ManifestError::StructuralViolation {
        route_id: route_id.to_owned(),
        rule: "baseUrl must use http:// or https://",
    })
}

fn is_loopback_authority(authority: &str) -> bool {
    // Userinfo (`user:pass@`) must be stripped BEFORE any host inspection. Otherwise a
    // bracketed IPv6 loopback or a bare `localhost` label sitting in the userinfo position
    // (e.g. `[::1]@evil.com` or `localhost:tok@attacker.example`) would read as loopback while
    // the host the connection actually reaches — the text after the LAST `@` — is remote. Same
    // pattern as `parse_admin_url` in `apps/cli/src/commands/events/config.rs`.
    let authority = authority
        .rsplit_once('@')
        .map_or(authority, |(_userinfo, host)| host);

    if let Some(bracketed) = authority.strip_prefix('[') {
        let host = bracketed.split(']').next().unwrap_or_default();
        return host == "::1";
    }

    let host = authority.split(':').next().unwrap_or_default();
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }

    host.parse::<Ipv4Addr>()
        .is_ok_and(|address| address.is_loopback())
}
