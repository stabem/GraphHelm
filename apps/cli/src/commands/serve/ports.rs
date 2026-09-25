//! Milestone 05d Task 9 STEP 3: the serve-side port implementations that let the async driver
//! (`graphhelm_runtime::driver::drive_to_quiescence_async`) run against the real gateway and tool
//! host, plus the small supporting seams (`IdGenerator`, the fail-closed `RefusingSealer`) the
//! driver's other parameters need. Every port wraps a SYNC adapter call in `spawn_blocking` — the
//! adapters (`ByokAdapter::call`, `RuntimeAdapter::call`, `ToolHost::invoke`) are not `async`.

use std::path::PathBuf;
use std::sync::Arc;

use graphhelm_events::{
    EvidenceError, EvidenceOpener, EvidenceSealer, RepositoryFuture, SecretBytes,
};
use graphhelm_gateway::call::{ModelCall, ModelReply};
use graphhelm_gateway::manifest::{ModelRoute, RouteManifest, Transport};
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

/// The real-executor wiring parsed and validated once at `serve` startup: two HALVES (#1066),
/// each optional on its own, plus the keyring both halves seal under. `model` is the gateway
/// half (`{manifest, broker, route}`); `tools` is the host half (`{staging, allow-program}`).
/// A `RuntimeWiring` exists only when at least one half does — `build_wiring` answers `None`
/// for the fixture-only shape — and the executor a drive gets is composed from whichever halves
/// are present (`routes::drive`): both → `PortExecutor`; one → `SplitExecutor` with the fixture
/// executor standing in for the absent half.
pub(super) struct RuntimeWiring {
    pub(super) model: Option<ModelWiring>,
    pub(super) tools: Option<ToolWiring>,
    /// Required whenever anything real is wired: real work seals evidence, and a keyring is
    /// what it seals under. Also the directory the tool workspace must never overlap.
    pub(super) keyring_dir: PathBuf,
    pub(super) key_id: String,
}

/// The model half: a chosen route cloned out of the manifest and the broker its credential is
/// leased from.
pub(super) struct ModelWiring {
    /// The manifest file `--manifest` named, kept as the path (not the parsed value): the
    /// gateway read surface (05e Task 4) re-reads it through the SAME `gateway::routes`/
    /// `gateway::probe` command layer the CLI runs, so the two can never drift on parsing.
    pub(super) manifest_path: PathBuf,
    pub(super) route: ModelRoute,
    pub(super) broker_dir: PathBuf,
}

/// The tool half: the workspace's fixed inputs and the allow-list/PATH-prepend configuration
/// every drive's `ToolLease` and `ToolHost` are built from. No credential lives here and none
/// is needed: Tier 1 work is git, a runner and an allowlist (#1066).
pub(super) struct ToolWiring {
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

/// Finds an ENABLED route by id in a parsed manifest — the ONE place that comparison is written.
///
/// Startup (`--route`, `serve/mod.rs`) and per-drive selection (a request's `"route"`, see
/// `prepare_drive`) both come through here. They used to be one site because only startup could
/// choose; the moment a request can choose too, "which route does this id mean" exists in two
/// places, and two spellings of the same lookup is how the flag and the field come to disagree
/// about a manifest neither of them changed.
///
/// `enabled: false` is refused HERE, before any caller can lease the credential or reach a paid
/// network call (PR #467 review): eligibility (`core/gateway/src/eligibility.rs`) and probe
/// (`gateway/probe.rs`) already exclude disabled routes, and the Studio's own composer greys them
/// out — this lookup was the one surface that ignored the flag, which put the restriction on the
/// surface a human uses and not on the one an agent uses. A disabled route answers exactly like a
/// missing one: it is not available for driving, and the caller learns nothing further.
pub(super) fn find_route(manifest: &RouteManifest, route_id: &str) -> Option<ModelRoute> {
    manifest
        .routes()
        .iter()
        .find(|route| route.id() == route_id && route.enabled())
        .cloned()
}

impl ServeModelPort {
    /// Resolves `route`'s transport and, for `direct_api`, opens the broker and leases the
    /// configured credential — an async step, run here (inside the async handler) rather than
    /// deferred into `spawn_blocking`, matching the plan's explicit instruction.
    ///
    /// The route arrives as a PARAMETER rather than being read from `wiring`, which is what lets a
    /// request pick a model: `wiring.route` is the deployer's default, not the only answer. The
    /// broker/keyring coordinates still come from `wiring` — those are deployment facts, and a
    /// request that could redirect the credential lookup would be choosing whose key it spends.
    pub(super) async fn build(
        wiring: &RuntimeWiring,
        model: &ModelWiring,
        route: &ModelRoute,
    ) -> Result<Self, String> {
        match route.transport() {
            Transport::NativeRuntime => Ok(Self::NativeRuntime {
                route: route.clone(),
            }),
            Transport::DirectApi => {
                let passphrase = gateway_passphrase()?;
                let broker = CredentialBroker::open(
                    &model.broker_dir,
                    &wiring.keyring_dir,
                    &wiring.key_id,
                    passphrase,
                )
                .await
                .map_err(|error| error.to_string())?;
                let credential_ref = route
                    .credential_ref()
                    .ok_or_else(|| "direct_api route carries no credentialRef".to_owned())?;
                let key = broker
                    .lease(credential_ref, route.id())
                    .await
                    .map_err(|error| error.to_string())?;
                Ok(Self::DirectApi {
                    route: route.clone(),
                    key,
                })
            }
        }
    }

    /// The route this port was built for. `call` refuses any other id (see its own comment).
    fn route(&self) -> &ModelRoute {
        match self {
            Self::DirectApi { route, .. } | Self::NativeRuntime { route } => route,
        }
    }
}

impl ModelPort for ServeModelPort {
    fn call<'a>(
        &'a self,
        route_id: &'a str,
        call: &'a ModelCall,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<ModelReply, GatewayError>> + Send + 'a>,
    > {
        Box::pin(async move {
            // THE ID IS CHECKED, NOT IGNORED. This parameter used to be `_route_id`, which was
            // harmless only while a server had exactly one route and no request could name
            // another: the port's own route was necessarily the executor's. Now that a request
            // chooses, the two are separately derived — the port from the resolved `ModelRoute`,
            // the executor from `PreparedPorts::route_id` — and separately derived values drift.
            //
            // Drift here is silent and expensive in the worst way: the reply comes back from a
            // model the operator did not pick, spends the credential of a route they did not
            // choose, and every event records the id they asked for. Nothing downstream can
            // notice, because the id is the only thing downstream ever sees. Refusing costs one
            // comparison and turns an unobservable wrong answer into a named failure.
            if route_id != self.route().id() {
                return Err(GatewayError::PolicyDenied);
            }
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
    /// The execution every call of this port runs for (#1066): the host keys ONE Tier 1
    /// workspace on it, so node A's patch is still in the tree when node B tests it and node C
    /// commits it, and a `commit` lands `refs/graphhelm/executions/<id>` in the project. Bound
    /// by `drive` before the port is handed to the executor; a port is built per drive and a
    /// drive is one execution, so the binding is total.
    execution_id: String,
}

/// The handle `drive` keeps to tear an execution's workspace down once the drive ends (#1066),
/// separate from the port itself because the port has been moved into the executor by then.
pub(super) struct WorkspaceRelease {
    host: Arc<ToolHost>,
    execution_id: String,
}

impl WorkspaceRelease {
    /// Removes the execution's Tier 1 workspace; the ref it landed stays. Blocking (a
    /// `git worktree remove`), so the caller runs it off the reactor.
    pub(super) fn release(self) -> Result<(), String> {
        self.host
            .release(&self.execution_id)
            .map_err(|error| error.to_string())
    }
}

impl ServeToolPort {
    /// `protected` is every directory the tool workspace must never overlap — the keyring
    /// always, the broker when a model half is wired.
    pub(super) fn build(
        tools: &ToolWiring,
        protected: &[PathBuf],
        project: &std::path::Path,
        execution_id: &str,
    ) -> Result<Self, String> {
        // The staging area itself is `WorkspaceConfig::validated`'s second argument, not a
        // "protected" directory to check the staging area against — passing it in `protected`
        // too made every drive refuse with "the staging area must not overlap a protected
        // directory" (it always overlaps itself), observed directly while proving STEP 6 test 3
        // green. Only the keyring/broker are genuinely separate directories that must never
        // overlap the tool workspace.
        let workspace = WorkspaceConfig::validated(project, &tools.staging, protected)
            .map_err(|error| error.to_string())?;
        let host = ToolHost::new(HostConfig {
            workspace,
            limits: ProcessLimits {
                timeout: std::time::Duration::from_secs(TIMEOUT_SECONDS),
                max_output_bytes: MAX_OUTPUT_BYTES,
            },
            tests_runner: tools.tests_runner.clone(),
            tests_runner_env: std::collections::BTreeMap::new(),
            path_prepend: tools.path_prepend.clone(),
            keep_workspace: false,
        });
        Ok(Self {
            host: Arc::new(host),
            execution_id: execution_id.to_owned(),
        })
    }

    /// The context port over this execution's own Tier 1 tree (#1086): the same host and the
    /// same execution key the tool calls use, so the compile and the tools share one tree and
    /// one lock.
    pub(super) fn context_tree(&self) -> Arc<dyn graphhelm_runtime::ports::ExecutionTreePort> {
        Arc::new(
            graphhelm_tool_host::source_reader::ExecutionContextTree::new(
                self.host.clone(),
                &self.execution_id,
            ),
        )
    }

    /// The release handle for this port's execution — taken by `drive` BEFORE the port moves
    /// into the executor, used AFTER the drive returns.
    pub(super) fn releaser(&self) -> WorkspaceRelease {
        WorkspaceRelease {
            host: self.host.clone(),
            execution_id: self.execution_id.clone(),
        }
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
        let execution_id = self.execution_id.clone();
        let call = call.clone();
        let lease = lease.clone();
        let actor = actor.to_owned();
        Box::pin(async move {
            let (record, streams) = tokio::task::spawn_blocking(move || {
                host.invoke_for_execution(&execution_id, &call, &lease, &actor)
            })
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

    /// #180: cancelling a run kills the tool children it left running.
    ///
    /// The comment this replaces said `ToolHost` exposed no kill surface, and it was TRUE when
    /// written -- the host retained no handle to an in-flight child, so the default no-op was the
    /// honest thing to accept. It stopped being true in the same commit as this line: the host now
    /// carries a cancel signal that the spawn loop reads at its next poll, killing and reaping
    /// through the deadline's own path.
    ///
    /// A comment is a claim, so it goes out with the code it described rather than staying to
    /// certify the opposite.
    fn cancel_all(&self) {
        self.host.cancel_all();
    }
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
    Ok(Arc::new(graphhelm_events::EvidenceProtector::new(
        open_key_provider(sealing)?,
    )))
}

/// The context ports (#1065): the bounded search channel and the bounded prefix reader, both
/// over the SAME project root the tool port was built from, plus a fresh ledger for this drive.
///
/// Built beside `ServeToolPort::build` rather than inside it because they answer to a different
/// port and a different reader contract; a root the channel cannot admit (not a directory, a
/// link) refuses the drive with the same setup failure a bad tool workspace would.
///
/// `execution_tree` is the execution's own Tier 1 tree when the drive has a tool half (#1086):
/// a node compiled while that tree exists reads it instead of `project`.
pub(super) fn build_context_ports(
    project: &std::path::Path,
    execution_tree: Option<Arc<dyn graphhelm_runtime::ports::ExecutionTreePort>>,
) -> Result<graphhelm_runtime::context::ContextPorts, String> {
    let keel_enabled = std::env::var("GRAPHHELM_KEEL_CONTEXT").as_deref() == Ok("1");
    build_context_ports_with_keel(project, execution_tree, keel_enabled)
}

fn build_context_ports_with_keel(
    project: &std::path::Path,
    execution_tree: Option<Arc<dyn graphhelm_runtime::ports::ExecutionTreePort>>,
    keel_enabled: bool,
) -> Result<graphhelm_runtime::context::ContextPorts, String> {
    let live_search = graphhelm_tool_host::source_channel::WorkspaceSourceChannel::open(project)
        .map_err(|error| match error {
            graphhelm_runtime::ports::SourceSearchError::Unavailable => {
                "the project root is not a readable directory, so it cannot be searched".to_owned()
            }
            graphhelm_runtime::ports::SourceSearchError::BoundExceeded => {
                "the project root exceeds the search channel's admission bound".to_owned()
            }
        })?;
    let live_reader = graphhelm_tool_host::source_reader::WorkspaceExcerptReader::open(project)
        .map_err(|error| format!("the project root cannot be read: {error}"))?;
    // Tier 1 owns the search and reader for an execution tree. Building the project snapshot here
    // would spend its cold cost even when this compile never consults the project pair.
    if execution_tree.is_some() {
        return Ok(graphhelm_runtime::context::ContextPorts {
            search: Arc::new(live_search),
            reader: Arc::new(live_reader),
            ledger: graphhelm_runtime::context::ContextLedger::new(),
            execution_tree,
        });
    }
    if !keel_enabled {
        return Ok(graphhelm_runtime::context::ContextPorts {
            search: Arc::new(live_search),
            reader: Arc::new(live_reader),
            ledger: graphhelm_runtime::context::ContextLedger::new(),
            execution_tree,
        });
    }
    let (search, reader): (
        Arc<dyn graphhelm_runtime::ports::BoundedSourceSearch>,
        Arc<dyn graphhelm_runtime::ports::BoundedSourceReader>,
    ) = match graphhelm_tool_host::keel_source::KeelSnapshotPorts::try_open(project)? {
        graphhelm_tool_host::keel_source::KeelSnapshotSelection::Snapshot(snapshot) => {
            (Arc::new(snapshot.search()), Arc::new(snapshot.reader()))
        }
        graphhelm_tool_host::keel_source::KeelSnapshotSelection::LiveFallback { reason } => (
            Arc::new(
                graphhelm_tool_host::keel_source::LiveFallbackSourceSearch::with_reason(
                    live_search,
                    reason,
                ),
            ),
            Arc::new(live_reader),
        ),
    };
    Ok(graphhelm_runtime::context::ContextPorts {
        search,
        reader,
        ledger: graphhelm_runtime::context::ContextLedger::new(),
        execution_tree,
    })
}

/// The mirror of `build_sealer`: the same keyring, opened for READING sealed Evidence back.
///
/// `EvidenceProtector` implements both halves, so this is the same construction reached through
/// the other trait rather than a second key path — there is exactly one way this server gets at
/// the keyring, and adding a read surface did not add another.
///
/// NO `RefusingSealer` EQUIVALENT, deliberately. A server with no keyring configured can still
/// run (it seals nothing, and `RefusingSealer` is the honest answer to "seal this"), but it holds
/// no key, so "open this" has no answer at all — not a refusal to perform an action, an inability
/// to know. Returning an opener that always fails would push that discovery to call time and make
/// every caller's error look like a decryption failure instead of a server that was never wired
/// to decrypt.
pub(super) fn build_opener(
    sealing: Option<&crate::commands::execution::signal::SignalKeyring>,
) -> Result<Arc<dyn EvidenceOpener>, String> {
    let Some(sealing) = sealing else {
        return Err(
            "this server has no keyring configured, so sealed evidence cannot be opened".to_owned(),
        );
    };
    Ok(Arc::new(graphhelm_events::EvidenceProtector::new(
        open_key_provider(sealing)?,
    )))
}

fn open_key_provider(
    sealing: &crate::commands::execution::signal::SignalKeyring,
) -> Result<graphhelm_sealed_key_provider::SealedKeyProvider, String> {
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
    graphhelm_sealed_key_provider::SealedKeyProvider::open(
        &sealing.directory,
        sealing.key_id.clone(),
        SecretBytes::new(material),
    )
    .map_err(|_| "the sealed keyring could not be opened".to_owned())
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

#[cfg(test)]
mod keel_context_port_tests {
    use super::*;
    use graphhelm_runtime::context::SEARCH_BOUNDS;
    use graphhelm_runtime::ports::{
        BoundedSourceReader, BoundedSourceSearch, ExecutionTreeAccess, ExecutionTreePort,
        ScanCancel, SourceSearchOrigin,
    };
    use std::process::Command;
    use std::sync::Arc;

    struct TierOneOnly;

    impl ExecutionTreePort for TierOneOnly {
        fn with_tree(
            &self,
            _cancel: &ScanCancel,
            _compile: &mut dyn FnMut(&dyn BoundedSourceSearch, &dyn BoundedSourceReader),
        ) -> ExecutionTreeAccess {
            ExecutionTreeAccess::Absent
        }
    }

    #[test]
    fn production_context_ports_search_and_read_one_snapshot_generation() {
        // Contract: the serve wiring must pair Keel search with Keel read. A live reader here
        // would ship changed bytes after the search had selected an older snapshot generation.
        let repo = tempfile::tempdir().unwrap();
        assert!(
            Command::new("git")
                .args(["-C", repo.path().to_str().unwrap(), "init", "-q"])
                .status()
                .unwrap()
                .success()
        );
        std::fs::write(repo.path().join("README.md"), "old_keel_generation\n").unwrap();
        assert!(
            Command::new("git")
                .args(["-C", repo.path().to_str().unwrap(), "add", "README.md"])
                .status()
                .unwrap()
                .success()
        );

        let ports = build_context_ports_with_keel(repo.path(), None, true).unwrap();
        std::fs::write(repo.path().join("README.md"), "new_keel_generation\n").unwrap();
        let result = ports
            .search
            .search_with_provenance(&["old_keel_generation".to_owned()], &SEARCH_BOUNDS)
            .unwrap();
        assert_eq!(result.paths, ["README.md"]);
        assert_eq!(result.provenance.origin, SourceSearchOrigin::Snapshot);
        assert_eq!(
            ports.reader.read_prefix("README.md", 4096).unwrap().bytes,
            b"old_keel_generation\n"
        );
    }

    #[test]
    fn production_context_ports_default_to_live_without_keel_opt_in() {
        let repo = tempfile::tempdir().unwrap();
        assert!(
            Command::new("git")
                .args(["-C", repo.path().to_str().unwrap(), "init", "-q"])
                .status()
                .unwrap()
                .success()
        );
        std::fs::write(repo.path().join("README.md"), "live_default_marker\n").unwrap();

        let ports = build_context_ports_with_keel(repo.path(), None, false).unwrap();
        let result = ports
            .search
            .search_with_provenance(&["live_default_marker".to_owned()], &SEARCH_BOUNDS)
            .unwrap();
        assert_eq!(result.paths, ["README.md"]);
        assert_eq!(result.provenance.origin, SourceSearchOrigin::Live);
    }

    #[test]
    fn production_context_ports_skip_project_snapshot_when_tier_one_exists() {
        let repo = tempfile::tempdir().unwrap();
        assert!(
            Command::new("git")
                .args(["-C", repo.path().to_str().unwrap(), "init", "-q"])
                .status()
                .unwrap()
                .success()
        );
        std::fs::write(repo.path().join("README.md"), "live_project_marker\n").unwrap();

        let ports =
            build_context_ports_with_keel(repo.path(), Some(Arc::new(TierOneOnly)), true).unwrap();
        let result = ports
            .search
            .search_with_provenance(&["live_project_marker".to_owned()], &SEARCH_BOUNDS)
            .unwrap();
        assert_eq!(result.paths, ["README.md"]);
        assert_eq!(result.provenance.origin, SourceSearchOrigin::Live);
    }
}
