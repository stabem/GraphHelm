//! Milestone 05d Task 9 STEP 3: the serve-side port implementations that let the async driver
//! (`graphhelm_runtime::driver::drive_to_quiescence_async`) run against the real gateway and tool
//! host, plus the small supporting seams (`IdGenerator`, the fail-closed `RefusingSealer`) the
//! driver's other parameters need. Every port wraps a SYNC adapter call in `spawn_blocking` — the
//! adapters (`ByokAdapter::call`, `RuntimeAdapter::call`, `ToolHost::invoke`) are not `async`.

use std::path::PathBuf;
use std::sync::Arc;

use graphhelm_events::{EvidenceError, EvidenceSealer, RepositoryFuture, SecretBytes};
use graphhelm_gateway::call::{ModelCall, ModelReply};
use graphhelm_gateway::manifest::{ModelRoute, Transport};
use graphhelm_gateway::taxonomy::GatewayError;
use graphhelm_model_gateway::broker::CredentialBroker;
use graphhelm_model_gateway::byok::ByokAdapter;
use graphhelm_model_gateway::runtime::RuntimeAdapter;
use graphhelm_model_gateway::transport::UreqTransport;
use graphhelm_runtime::ports::{ModelPort, ToolPort, ToolPortResult, ToolStreams};
use graphhelm_tool_broker::call::ToolCall;
use graphhelm_tool_broker::lease::ToolLease;
use graphhelm_tool_host::host::{HostConfig, ToolHost};
use graphhelm_tool_host::process::ProcessLimits;
use graphhelm_tool_host::workspace::WorkspaceConfig;

/// Mirrors `commands::tool::invoke`'s own fixed budget — this module has no per-call channel to
/// vary it from, and the plan names no separate one for `serve`.
const TIMEOUT_SECONDS: u64 = 300;
const MAX_OUTPUT_BYTES: usize = 8 * 1024 * 1024;

/// The real-executor wiring parsed and validated once at `serve` startup (STEP 2's grouping
/// rule): a chosen route cloned out of the manifest, the broker/keyring coordinates, the tool
/// workspace's fixed inputs, and the allow-list/PATH-prepend configuration every drive's
/// `ToolLease` and `ToolHost` are built from.
pub(super) struct RuntimeWiring {
    /// The manifest file `--manifest` named, kept as the path (not the parsed value): the
    /// gateway read surface (05e Task 4) re-reads it through the SAME `gateway::routes`/
    /// `gateway::probe` command layer the CLI runs, so the two can never drift on parsing.
    pub(super) manifest_path: PathBuf,
    pub(super) route: ModelRoute,
    pub(super) broker_dir: PathBuf,
    pub(super) keyring_dir: PathBuf,
    pub(super) key_id: String,
    pub(super) staging: PathBuf,
    /// The deployer's own default for a request's absent `"project"` (issue #82) — `--project`
    /// at startup, never required. `None` preserves the prior behavior exactly: `drive`'s own
    /// fallback to the server process's working directory.
    pub(super) project: Option<PathBuf>,
    pub(super) tests_runner: String,
    pub(super) allow_programs: Vec<String>,
    pub(super) path_prepend: Vec<PathBuf>,
}

/// A model port over the real gateway, built once per drive (never once per server run — Task 9's
/// FIXED decision): for a `direct_api` route the credential is leased from the broker at
/// construction, and `call` wraps the synchronous `ByokAdapter`/`RuntimeAdapter` in
/// `spawn_blocking`.
pub(super) enum ServeModelPort {
    DirectApi { route: ModelRoute, key: SecretBytes },
    NativeRuntime { route: ModelRoute },
}

impl ServeModelPort {
    /// Resolves `wiring.route`'s transport and, for `direct_api`, opens the broker and leases the
    /// configured credential — an async step, run here (inside the async handler) rather than
    /// deferred into `spawn_blocking`, matching the plan's explicit instruction.
    pub(super) async fn build(wiring: &RuntimeWiring) -> Result<Self, String> {
        match wiring.route.transport() {
            Transport::NativeRuntime => Ok(Self::NativeRuntime {
                route: wiring.route.clone(),
            }),
            Transport::DirectApi => {
                let passphrase = gateway_passphrase()?;
                let broker = CredentialBroker::open(
                    &wiring.broker_dir,
                    &wiring.keyring_dir,
                    &wiring.key_id,
                    passphrase,
                )
                .await
                .map_err(|error| error.to_string())?;
                let credential_ref = wiring
                    .route
                    .credential_ref()
                    .ok_or_else(|| "direct_api route carries no credentialRef".to_owned())?;
                let key = broker
                    .lease(credential_ref, wiring.route.id())
                    .await
                    .map_err(|error| error.to_string())?;
                Ok(Self::DirectApi {
                    route: wiring.route.clone(),
                    key,
                })
            }
        }
    }
}

impl ModelPort for ServeModelPort {
    fn call<'a>(
        &'a self,
        _route_id: &'a str,
        call: &'a ModelCall,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<ModelReply, GatewayError>> + Send + 'a>,
    > {
        Box::pin(async move {
            // `ByokAdapter`/`RuntimeAdapter` are neither `Send` nor cheap to hold across an
            // await point (they borrow `route` and, for BYOK, a `SecretBytes` key) — this
            // clones the small config each call needs and constructs the adapter INSIDE the
            // blocking closure, per the plan's own fallback instruction ("if not, construct the
            // adapter inside the blocking closure from cloned config, report which"). Reported:
            // the adapters are borrow-shaped (`ByokAdapter<'a>`/`RuntimeAdapter<'a>`), so they
            // are not `Send` across the `.await` a plain wrap would need; constructing inside
            // `spawn_blocking` sidesteps that entirely rather than requiring `Send`.
            match self {
                Self::DirectApi { route, key } => {
                    let route = route.clone();
                    // `SecretBytes` deliberately carries no `Clone` impl (it zeroizes on drop and
                    // permits only callback-scoped exposure) — the bytes are copied out here and
                    // rebuilt as a fresh, independent `SecretBytes` inside the blocking closure,
                    // matching `gateway_passphrase`'s own pattern.
                    let key_bytes = key.expose(<[u8]>::to_vec);
                    let call = call.clone();
                    tokio::task::spawn_blocking(move || {
                        let key = SecretBytes::new(key_bytes);
                        let transport = Arc::new(UreqTransport::new());
                        let adapter = ByokAdapter::new(&route, transport);
                        adapter.call(&key, &call)
                    })
                    .await
                    .unwrap_or(Err(GatewayError::ProviderUnavailable))
                }
                Self::NativeRuntime { route } => {
                    let route = route.clone();
                    let call = call.clone();
                    tokio::task::spawn_blocking(move || {
                        let adapter = RuntimeAdapter::new(&route, Vec::new());
                        adapter.call(&call)
                    })
                    .await
                    .unwrap_or(Err(GatewayError::ProviderUnavailable))
                }
            }
        })
    }

    // No cancel hook: an HTTP-backed direct_api call cannot be aborted mid-flight (its bound is
    // the transport timeout — `ports.rs`'s own trait doc), and a native_runtime call is a child
    // process this port does not itself track across calls (each `call` spawns and reaps its own
    // child). Default (`fn cancel_all(&self) {}`) is accepted here, matching the trait's
    // documented default for a port with nothing durable to kill.
}

/// A tool port over the real `ToolHost`, built once per drive from the request's optional
/// `"project"` field (FIXED decision: defaults to the server process's current working
/// directory when absent — documented here since the request shape itself carries no other
/// signal).
pub(super) struct ServeToolPort {
    host: Arc<ToolHost>,
}

impl ServeToolPort {
    pub(super) fn build(wiring: &RuntimeWiring, project: &std::path::Path) -> Result<Self, String> {
        // The staging area itself is `WorkspaceConfig::validated`'s second argument, not a
        // "protected" directory to check the staging area against — passing it in `protected`
        // too made every drive refuse with "the staging area must not overlap a protected
        // directory" (it always overlaps itself), observed directly while proving STEP 6 test 3
        // green. Only the keyring/broker are genuinely separate directories that must never
        // overlap the tool workspace.
        let protected = vec![wiring.keyring_dir.clone(), wiring.broker_dir.clone()];
        let workspace = WorkspaceConfig::validated(project, &wiring.staging, &protected)
            .map_err(|error| error.to_string())?;
        let host = ToolHost::new(HostConfig {
            workspace,
            limits: ProcessLimits {
                timeout: std::time::Duration::from_secs(TIMEOUT_SECONDS),
                max_output_bytes: MAX_OUTPUT_BYTES,
            },
            tests_runner: wiring.tests_runner.clone(),
            tests_runner_env: std::collections::BTreeMap::new(),
            path_prepend: wiring.path_prepend.clone(),
            keep_workspace: false,
        });
        Ok(Self {
            host: Arc::new(host),
        })
    }
}

impl ToolPort for ServeToolPort {
    fn invoke<'a>(
        &'a self,
        call: &'a ToolCall,
        lease: &'a ToolLease,
        actor: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ToolPortResult> + Send + 'a>> {
        let host = self.host.clone();
        let call = call.clone();
        let lease = lease.clone();
        let actor = actor.to_owned();
        Box::pin(async move {
            let (record, streams) =
                tokio::task::spawn_blocking(move || host.invoke(&call, &lease, &actor))
                    .await
                    .expect("the tool-host blocking task is never cancelled");
            ToolPortResult {
                record,
                streams: ToolStreams {
                    stdout: streams.stdout,
                    stderr: streams.stderr,
                },
                // Reported (Task 9 STEP 3 FIXED decision): `ToolHost::invoke` returns
                // `(ToolCallRecord, CapturedStreams)` with no separate reuse-summary accessor —
                // the host's read-cache hit/miss is folded into `ToolCallRecord::reused` only, a
                // `bool` with no key-component/freshness detail a `ReuseSummary` needs. The
                // ledger's `ReuseDecision` event is therefore never produced on the serve path;
                // it stays a CLI-only (`graphhelm tool invoke`-adjacent, Task 6-era) capability.
                reuse: None,
            }
        })
    }

    // No cancel hook: `ToolHost` exposes no kill surface for in-flight children beyond what the
    // process's own deadline already bounds (`ProcessLimits`) — the default no-op is accepted
    // and reported, matching `ServeModelPort`'s own note.
}

/// A sealer that always refuses. Wired in when `serve` runs without a keyring: a fixture story
/// never calls `seal` (fixtures produce no sealable material — see `graphhelm_runtime::fixture`'s
/// own doc comment), so this never fires for the 05a fixture-only shape; a REAL story attempted
/// without a keyring fails closed here rather than silently sealing nothing.
pub(super) struct RefusingSealer;

impl EvidenceSealer for RefusingSealer {
    fn seal<'a>(
        &'a self,
        _scope: graphhelm_protocols::RepositoryScope,
        _input: graphhelm_events::EvidenceInput,
    ) -> RepositoryFuture<'a, Result<graphhelm_events::SealedEvidence, EvidenceError>> {
        Box::pin(async { Err(EvidenceError::Unavailable) })
    }
}

/// Builds the driver's `EvidenceSealer` from `sealing` (STEP 2's `{keyring, key-id}` group):
/// `Some` opens the same `SealedKeyProvider` shape `execution::signal::open_sealer` already uses
/// for the CLI's own evidence sealing — same passphrase source (`GRAPHHELM_EVENTS_KEY`, the
/// events-keyring convention `commands::events::config` established, distinct from the gateway
/// broker's own `GRAPHHELM_GATEWAY_KEY`) — so a story's node-outcome evidence and its signal
/// evidence seal under the one keyring an operator configured with `--keyring`/`--key-id`.
/// `None` (no keyring configured) wires the fail-closed `RefusingSealer`.
pub(super) fn build_sealer(
    sealing: Option<&crate::commands::execution::signal::SignalKeyring>,
) -> Result<Arc<dyn EvidenceSealer>, String> {
    let Some(sealing) = sealing else {
        return Ok(Arc::new(RefusingSealer));
    };
    let encoded = std::env::var("GRAPHHELM_EVENTS_KEY").map_err(|_| {
        "GRAPHHELM_EVENTS_KEY must supply 64 lowercase hexadecimal characters".to_owned()
    })?;
    if encoded.len() != 64
        || !encoded
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(
            "GRAPHHELM_EVENTS_KEY must supply 64 lowercase hexadecimal characters".to_owned(),
        );
    }
    let mut material = Vec::with_capacity(32);
    for pair in encoded.as_bytes().chunks_exact(2) {
        let byte = u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16)
            .map_err(|_| "GRAPHHELM_EVENTS_KEY is not valid hex".to_owned())?;
        material.push(byte);
    }
    let provider = graphhelm_sealed_key_provider::SealedKeyProvider::open(
        &sealing.directory,
        sealing.key_id.clone(),
        SecretBytes::new(material),
    )
    .map_err(|_| "the sealed keyring could not be opened".to_owned())?;
    Ok(Arc::new(graphhelm_events::EvidenceProtector::new(provider)))
}

/// Reads `GRAPHHELM_GATEWAY_KEY` fresh (the gateway CLI's own precedent —
/// `commands::gateway::passphrase_from_env`, not reused directly because that module's `Failure`
/// type differs from this one's `String`-based port error shape).
fn gateway_passphrase() -> Result<SecretBytes, String> {
    let encoded = std::env::var("GRAPHHELM_GATEWAY_KEY").map_err(|_| {
        "GRAPHHELM_GATEWAY_KEY must supply 64 lowercase hexadecimal characters".to_owned()
    })?;
    if encoded.len() != 64
        || !encoded
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(
            "GRAPHHELM_GATEWAY_KEY must supply 64 lowercase hexadecimal characters".to_owned(),
        );
    }
    let mut bytes = Vec::with_capacity(32);
    for pair in encoded.as_bytes().chunks_exact(2) {
        let byte = u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16)
            .map_err(|_| "GRAPHHELM_GATEWAY_KEY is not valid hex".to_owned())?;
        bytes.push(byte);
    }
    Ok(SecretBytes::new(bytes))
}
