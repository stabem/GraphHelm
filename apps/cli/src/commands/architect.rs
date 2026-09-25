//! `graphhelm graph synthesize` — the CLI door of the Graph Architect (#107, spec D7/D8).
//!
//! The command is thin on purpose: the compiler lives in `core/architect`, and this module only
//! turns arguments into a `TaskProfile` and a `CapabilityCatalog`, chooses which model door
//! answers (the recorded fixture, or a gateway route), writes the document to `--out`, and
//! prints the one JSON every door returns. It never publishes and never starts an execution:
//! the file it writes enters the system through `execution start --file` exactly as an authored
//! graph does.
//!
//! The gateway-backed door is built the way `gateway probe` builds its lease (passphrase from
//! the environment, credential leased from the broker; `gateway/probe.rs`) and places the call
//! the way `serve/ports.rs` does (`ByokAdapter`/`RuntimeAdapter` with a fixed `max_tokens`),
//! so there is no new credential path. A credential value crosses this module exactly once, as
//! the leased [`SecretBytes`] the adapter is handed, and is never formatted into a failure, a
//! reply, or a `Debug` impl.
//!
//! Two failure classes:
//!
//! - `GHCLI001_ARGUMENT_INVALID` — an argument the command cannot honour: `--out` not a `.json`
//!   path, `--out` already present (the architect never overwrites), neither or both model
//!   doors named, or a door named without its coordinates. Manifest, route and broker problems
//!   keep the codes `gateway probe` reports for them (`GHCLI009`/`GHCLI010`).
//! - `GHCLI026_ARCHITECT_REFUSED` — the compiler refused. ONE diagnostic at `/goal` whose
//!   message is the [`ArchitectRefusal`] serialized as compact JSON, `kind`-tagged, so an
//!   operator reads `{"kind":"capabilityMissing","node":"…","program":"python"}` and a caller can
//!   match on the kind without parsing prose.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use graphhelm_architect::{
    ArchitectRefusal, CapabilityCatalog, DraftModel, DraftReply, Extras, GraphLibrary, JudgeModel,
    RecordedDraftModel, RecordedJudgeModel, TaskProfile, synthesize_with,
};
use graphhelm_events::SecretBytes;
use graphhelm_gateway::call::ModelCall;
use graphhelm_gateway::judgment::{JudgeReply, JudgeRequest};
use graphhelm_gateway::manifest::{ModelRoute, Transport};
use graphhelm_model_gateway::broker::CredentialBroker;
use graphhelm_model_gateway::byok::ByokAdapter;
use graphhelm_model_gateway::runtime::RuntimeAdapter;
use graphhelm_model_gateway::systemone::{SystemOneAdapter, TYPESAFE_PROVIDER};
use graphhelm_model_gateway::transport::UreqTransport;
use graphhelm_protocols::Diagnostic;
use serde_json::Value;

use super::gateway;
use crate::output::Outcome;

const COMMAND: &str = "graph.synthesize";
const SOURCE: &str = "architect-cli";
const ARGUMENT_CODE: &str = crate::error_codes::GHCLI001_ARGUMENT_INVALID;
const REFUSED_CODE: &str = crate::error_codes::GHCLI026_ARCHITECT_REFUSED;
/// The pointer every compiler refusal is reported at: the goal is what the operator asked for,
/// and every refusal is a reason that goal did not compile.
const REFUSAL_POINTER: &str = "/goal";
/// The output budget of one draft call, as `serve/ports.rs` sizes a model call. A graph document
/// of six nodes is a few thousand tokens; this leaves room for a repair round's larger draft.
pub(crate) const MAX_TOKENS: u32 = 8192;

/// A redaction-safe operator failure, the shape every command family defines for itself
/// (`commands::gateway::Failure`, `commands::execution::Failure`).
pub(crate) struct Failure {
    pub(crate) code: &'static str,
    pub(crate) message: String,
    pub(crate) pointer: String,
}

impl Failure {
    pub(crate) fn into_outcome(self, command: &'static str) -> Outcome {
        Outcome::domain(
            command,
            vec![Diagnostic::error(
                self.code,
                self.message,
                &self.pointer,
                SOURCE,
            )],
        )
    }
}

impl From<gateway::Failure> for Failure {
    fn from(failure: gateway::Failure) -> Self {
        Self {
            code: failure.code,
            message: failure.message,
            pointer: failure.pointer,
        }
    }
}

fn argument(message: &str, pointer: &str) -> Failure {
    Failure {
        code: ARGUMENT_CODE,
        message: message.to_owned(),
        pointer: pointer.to_owned(),
    }
}

/// The compiler's refusal as the one diagnostic the operator sees: compact JSON, `kind`-tagged.
/// Shared with the HTTP route, so a refusal reads the same on every door.
pub(crate) fn refused(refusal: &ArchitectRefusal) -> Failure {
    let message = serde_json::to_string(refusal).unwrap_or_else(|_| refusal.to_string());
    Failure {
        code: REFUSED_CODE,
        message,
        pointer: REFUSAL_POINTER.to_owned(),
    }
}

/// What the caller asks the architect to compile. Shared by every door: the CLI builds it from
/// flags, the HTTP route and the MCP tool from their request bodies.
pub(crate) struct SynthesizeRequest<'a> {
    pub(crate) goal: &'a str,
    /// The profile's default (`supervised`) when absent, so the default has one home.
    pub(crate) mode: Option<&'a str>,
    pub(crate) max_nodes: Option<usize>,
    pub(crate) allow_programs: &'a [String],
    pub(crate) wait_within_seconds: Option<u64>,
    pub(crate) clearance_within_seconds: Option<u64>,
    /// How many drafts to ask for and rank; `1` (today's road) when absent. The bound `1..=3`
    /// and the "more than one needs a judge" rule are the compiler's own (`InvalidProfile`),
    /// never pre-empted here, so every door refuses with the same words.
    pub(crate) drafts: Option<u8>,
}

/// Which model answers the prompt.
pub(crate) enum ModelSource<'a> {
    /// The recorded door: keyless, offline, the model in every test.
    Fixture(&'a Path),
    /// A route of a gateway manifest; `broker`/`keyring`/`key_id` are required for a
    /// `direct_api` route and ignored for a `native_runtime` one.
    Gateway {
        manifest: &'a Path,
        route: &'a str,
        broker: Option<&'a Path>,
        keyring: Option<&'a Path>,
        key_id: Option<&'a str>,
    },
}

/// Which judge answers the compiler's closed questions (spec D1, D10), when one is named.
pub(crate) enum JudgeSource<'a> {
    /// The recorded door: keyless, offline, the judge in every test.
    Fixture(&'a Path),
    /// A `direct_api` route of a gateway manifest whose provider is `typesafe`, leased exactly
    /// as the draft door's `direct_api` route is.
    Gateway {
        manifest: &'a Path,
        route: &'a str,
        broker: Option<&'a Path>,
        keyring: Option<&'a Path>,
        key_id: Option<&'a str>,
    },
}

/// Compiles the request through `model` — and `judge` and `library` when the caller named
/// them — and returns the one JSON of spec D8 (`document`, `rationale`, `stampedCustoms`,
/// `templateSha256`, `rounds`, `promptSha256s`, `usage`, plus `judgments`/`ranking`/`reuse`
/// when a judge was asked). No judge, one draft, no library is today's road byte for byte.
///
/// # Errors
/// `GHCLI026_ARCHITECT_REFUSED` at `/goal`, carrying the refusal as compact JSON.
pub(crate) fn execute(
    request: &SynthesizeRequest<'_>,
    model: &dyn DraftModel,
    judge: Option<&dyn JudgeModel>,
    library: Option<&GraphLibrary>,
) -> Result<Value, Failure> {
    let mut profile = TaskProfile::new(request.goal);
    if let Some(mode) = request.mode {
        profile.mode = mode.to_owned();
    }
    if let Some(max_nodes) = request.max_nodes {
        profile.max_nodes = max_nodes;
    }
    if let Some(seconds) = request.wait_within_seconds {
        profile.wait_within_seconds = seconds;
    }
    if let Some(seconds) = request.clearance_within_seconds {
        profile.clearance_within_seconds = seconds;
    }
    let catalog = CapabilityCatalog::from_runtime(request.allow_programs);
    let extras = Extras {
        judge,
        drafts: request.drafts.unwrap_or(1),
        library,
    };
    let synthesized =
        synthesize_with(&profile, &catalog, model, &extras).map_err(|refusal| refused(&refusal))?;
    serde_json::to_value(&synthesized).map_err(|_| {
        refused(&ArchitectRefusal::ModelUnavailable {
            message: "the synthesized graph could not be serialized".to_owned(),
        })
    })
}

/// Opens the model door `source` names.
///
/// # Errors
/// A fixture that cannot be read or parsed is the compiler's own `ModelUnavailable` refusal. A
/// gateway door fails the way `gateway probe` does: `GHCLI009` for the manifest or the route,
/// `GHCLI010` for the passphrase, the keyring directory or the broker; a lease the broker
/// declines is a `GHCLI010` here too, because unlike a probe this command has no reply to carry
/// `auth_required` in.
pub(crate) fn build_model(source: &ModelSource<'_>) -> Result<Box<dyn DraftModel>, Failure> {
    match source {
        ModelSource::Fixture(path) => {
            let model = RecordedDraftModel::from_file(path).map_err(|refusal| refused(&refusal))?;
            Ok(Box::new(model))
        }
        ModelSource::Gateway {
            manifest,
            route,
            broker,
            keyring,
            key_id,
        } => {
            let manifest = gateway::load_manifest(manifest)?;
            let route = manifest
                .routes()
                .iter()
                .find(|candidate| candidate.id() == *route)
                .ok_or_else(|| {
                    gateway::invalid("--route does not name a route in the manifest", "/route")
                })?;
            if !route.enabled() {
                return Err(gateway::invalid("the route is disabled", "/route").into());
            }
            let model = match route.transport() {
                Transport::NativeRuntime => GatewayDraftModel::native_runtime(route.clone()),
                Transport::DirectApi => {
                    let key = lease_credential(route, *broker, *keyring, *key_id)?;
                    GatewayDraftModel::direct_api(route.clone(), key)
                }
            };
            Ok(Box::new(model))
        }
    }
}

/// Opens the judge door `source` names.
///
/// # Errors
/// A recording that cannot be read or parsed is the compiler's own `JudgeUnavailable` refusal.
/// A gateway door fails as [`build_model`]'s does, plus `GHCLI009` at `/judgeRoute` when the
/// route is not a `direct_api` route of provider `typesafe`: that is the only transport the
/// System One adapter speaks, and refusing here costs no lease.
pub(crate) fn build_judge(source: &JudgeSource<'_>) -> Result<Box<dyn JudgeModel>, Failure> {
    match source {
        JudgeSource::Fixture(path) => {
            let judge = RecordedJudgeModel::from_file(path).map_err(|refusal| refused(&refusal))?;
            Ok(Box::new(judge))
        }
        JudgeSource::Gateway {
            manifest,
            route,
            broker,
            keyring,
            key_id,
        } => {
            let manifest = gateway::load_manifest(manifest)?;
            let route = manifest
                .routes()
                .iter()
                .find(|candidate| candidate.id() == *route)
                .ok_or_else(|| {
                    gateway::invalid(
                        "--judge-route does not name a route in the manifest",
                        "/judgeRoute",
                    )
                })?;
            if !route.enabled() {
                return Err(gateway::invalid("the judge route is disabled", "/judgeRoute").into());
            }
            if route.transport() != Transport::DirectApi || route.provider() != TYPESAFE_PROVIDER {
                return Err(gateway::invalid(
                    "--judge-route must name a direct_api typesafe route",
                    "/judgeRoute",
                )
                .into());
            }
            let key = lease_credential(route, *broker, *keyring, *key_id)?;
            Ok(Box::new(GatewayJudgeModel::new(route.clone(), key)))
        }
    }
}

/// Loads the graph library at `dir`.
///
/// # Errors
/// The compiler's own `LibraryInvalid` refusal (a file path, an unreadable directory, a bad
/// sidecar), as `GHCLI026` — the crate names the offending file, never the directory path.
pub(crate) fn build_library(dir: &Path) -> Result<GraphLibrary, Failure> {
    GraphLibrary::load(dir).map_err(|refusal| refused(&refusal))
}

/// The `direct_api` lease, exactly as `gateway/probe.rs` performs it: keyring directory present,
/// passphrase from `GRAPHHELM_GATEWAY_KEY`, broker opened and the route's credential leased
/// inside the bounded current-thread runtime. The leased bytes go straight into the adapter's
/// hands and nowhere else.
fn lease_credential(
    route: &ModelRoute,
    broker: Option<&Path>,
    keyring: Option<&Path>,
    key_id: Option<&str>,
) -> Result<SecretBytes, Failure> {
    let (broker, keyring, key_id) = match (broker, keyring, key_id) {
        (Some(broker), Some(keyring), Some(key_id)) => (broker, keyring, key_id),
        _ => {
            return Err(gateway::invalid(
                "--broker, --keyring and --key-id are required for a direct_api route",
                "/arguments",
            )
            .into());
        }
    };
    gateway::require_keyring_directory(keyring)?;
    let passphrase = gateway::passphrase_from_env()?;

    let credential_ref = route
        .credential_ref()
        .expect("direct_api routes carry credentialRef — enforced by manifest validation")
        .to_owned();
    let route_id = route.id().to_owned();
    let broker_dir: PathBuf = broker.to_path_buf();
    let keyring_dir: PathBuf = keyring.to_path_buf();
    let key_id = key_id.to_owned();

    let key = gateway::runtime()?.block_on(async move {
        let opened = CredentialBroker::open(&broker_dir, &keyring_dir, &key_id, passphrase)
            .await
            .map_err(|error| gateway::broker_failure(&error))?;
        opened
            .lease(&credential_ref, &route_id)
            .await
            .map_err(|error| gateway::broker_failure(&error))
    })?;
    Ok(key)
}

/// The gateway-backed [`DraftModel`]: a route and, for `direct_api`, the credential leased for
/// it. `draft` places one synchronous call the way `serve/ports.rs` does — `ByokAdapter` over
/// `UreqTransport` for `direct_api`, `RuntimeAdapter` with no extra environment for
/// `native_runtime` — and maps a [`graphhelm_gateway::taxonomy::GatewayError`] onto
/// [`ArchitectRefusal::ModelUnavailable`] with the error's own `Display` text, which is fixed
/// static prose: never a path, never a key.
pub(crate) struct GatewayDraftModel {
    route: ModelRoute,
    credential: Credential,
}

enum Credential {
    /// A `direct_api` route: the leased key the adapter exposes only while building headers.
    Leased(SecretBytes),
    /// A `native_runtime` route: the CLI the route names authenticates itself.
    None,
}

impl GatewayDraftModel {
    pub(crate) fn direct_api(route: ModelRoute, key: SecretBytes) -> Self {
        Self {
            route,
            credential: Credential::Leased(key),
        }
    }

    pub(crate) fn native_runtime(route: ModelRoute) -> Self {
        Self {
            route,
            credential: Credential::None,
        }
    }
}

impl DraftModel for GatewayDraftModel {
    fn draft(&self, prompt: &str) -> Result<DraftReply, ArchitectRefusal> {
        let call = ModelCall {
            prompt: prompt.to_owned(),
            max_tokens: MAX_TOKENS,
        };
        let reply = match &self.credential {
            Credential::Leased(key) => {
                ByokAdapter::new(&self.route, Arc::new(UreqTransport::new())).call(key, &call)
            }
            Credential::None => RuntimeAdapter::new(&self.route, Vec::new()).call(&call),
        };
        reply
            .map(|reply| DraftReply {
                text: reply.text,
                usage: Some(reply.usage),
            })
            .map_err(|error| ArchitectRefusal::ModelUnavailable {
                message: error.to_string(),
            })
    }
}

/// The gateway-backed [`JudgeModel`]: a `direct_api` `typesafe` route and the credential leased
/// for it. `judge` places one synchronous call through [`SystemOneAdapter`] over
/// `UreqTransport` and maps a [`graphhelm_gateway::taxonomy::GatewayError`] onto
/// [`ArchitectRefusal::JudgeUnavailable`] with the error's own `Display` text — fixed static
/// prose, never a path, never a key. Shared with the HTTP route, which leases the same way.
pub(crate) struct GatewayJudgeModel {
    route: ModelRoute,
    key: SecretBytes,
}

impl GatewayJudgeModel {
    pub(crate) fn new(route: ModelRoute, key: SecretBytes) -> Self {
        Self { route, key }
    }
}

impl JudgeModel for GatewayJudgeModel {
    fn judge(&self, request: &JudgeRequest) -> Result<JudgeReply, ArchitectRefusal> {
        SystemOneAdapter::new(&self.route, Arc::new(UreqTransport::new()))
            .call(&self.key, request)
            .map_err(|error| ArchitectRefusal::JudgeUnavailable {
                message: error.to_string(),
            })
    }
}

/// The flags of `graph synthesize`, grouped the way `tool::invoke::InvokeArguments` groups its
/// own, so the dispatch arm in `commands/mod.rs` stays one call.
pub struct SynthesizeArguments {
    pub goal: String,
    pub out: PathBuf,
    pub mode: Option<String>,
    pub max_nodes: Option<usize>,
    pub allow_programs: Vec<String>,
    pub fixture: Option<PathBuf>,
    pub manifest: Option<PathBuf>,
    pub route: Option<String>,
    pub broker: Option<PathBuf>,
    pub keyring: Option<PathBuf>,
    pub key_id: Option<String>,
    pub judge_route: Option<String>,
    pub judge_fixture: Option<PathBuf>,
    pub drafts: Option<u8>,
    pub library: Option<PathBuf>,
}

pub fn run(arguments: &SynthesizeArguments) -> Outcome {
    match run_inner(arguments) {
        Ok(value) => Outcome::success(COMMAND, value),
        Err(failure) => failure.into_outcome(COMMAND),
    }
}

fn run_inner(arguments: &SynthesizeArguments) -> Result<Value, Failure> {
    check_out_path(&arguments.out)?;
    let source = model_source(arguments)?;
    let model = build_model(&source)?;
    let judge = match judge_source(arguments)? {
        Some(source) => Some(build_judge(&source)?),
        None => None,
    };
    let library = match arguments.library.as_deref() {
        Some(dir) => Some(build_library(dir)?),
        None => None,
    };
    let request = SynthesizeRequest {
        goal: &arguments.goal,
        mode: arguments.mode.as_deref(),
        max_nodes: arguments.max_nodes,
        allow_programs: &arguments.allow_programs,
        wait_within_seconds: None,
        clearance_within_seconds: None,
        drafts: arguments.drafts,
    };
    let mut reply = execute(&request, model.as_ref(), judge.as_deref(), library.as_ref())?;
    let document = reply
        .get("document")
        .ok_or_else(|| argument("the synthesized reply carries no document", "/out"))?;
    write_document(&arguments.out, document)?;
    if let Some(object) = reply.as_object_mut() {
        object.insert(
            "out".to_owned(),
            Value::String(arguments.out.to_string_lossy().into_owned()),
        );
    }
    Ok(reply)
}

/// `--out` must be a `.json` path that does not exist yet. Checked before any model is asked,
/// so a refused write costs no draft; the write itself is `create_new`, so a file that appears
/// between this check and the write is still never overwritten.
fn check_out_path(out: &Path) -> Result<(), Failure> {
    if out.extension().and_then(|extension| extension.to_str()) != Some("json") {
        return Err(argument("--out must end in .json", "/out"));
    }
    if std::fs::symlink_metadata(out).is_ok() {
        return Err(argument(
            "--out already exists; the architect never overwrites a file",
            "/out",
        ));
    }
    Ok(())
}

/// Exactly one DRAFT door: `--fixture`, or `--manifest` with `--route`. The broker coordinates
/// travel with the gateway door and are checked there, where the route's transport says whether
/// they are needed.
///
/// **`--manifest` is not a draft door on its own, so `--fixture` beside it is not two doors**
/// (#1137). The manifest also carries the JUDGE's route, and `serve/routes.rs` has always
/// accepted `fixture` + `judgeRoute` in one request while this function refused the same pairing
/// on the CLI — so a recorded draft could be paired with a real judge over HTTP and not from a
/// terminal. `docs/harness/GRAPH_ARCHITECT.md` §10.8 recorded that asymmetry rather than repairing
/// it, and `architect_cli.rs::the_judge_route_spends_the_leased_key_only_in_the_authorization_header`
/// drafts through a fake gateway route only because of it.
///
/// What is still refused is the real conflict: `--fixture` with `--route`, which is two draft
/// doors. `--fixture` with `--manifest` and NO judge door is refused too, because the manifest
/// would then serve nothing — a silent no-op is the worse answer to a typo than a refusal.
fn model_source(arguments: &SynthesizeArguments) -> Result<ModelSource<'_>, Failure> {
    match (
        arguments.fixture.as_deref(),
        arguments.manifest.as_deref(),
        arguments.route.as_deref(),
    ) {
        (Some(_), _, Some(_)) => Err(argument(
            "--fixture and --route are mutually exclusive: one draft door per run",
            "/fixture",
        )),
        (Some(fixture), Some(_), None) if arguments.judge_route.is_some() => {
            Ok(ModelSource::Fixture(fixture))
        }
        (Some(_), Some(_), None) => Err(argument(
            "--fixture with --manifest requires --judge-route: the fixture serves the draft and \
             the manifest serves the judge, so without a judge route the manifest serves nothing",
            "/judgeRoute",
        )),
        (Some(fixture), None, None) => Ok(ModelSource::Fixture(fixture)),
        (None, Some(manifest), Some(route)) => Ok(ModelSource::Gateway {
            manifest,
            route,
            broker: arguments.broker.as_deref(),
            keyring: arguments.keyring.as_deref(),
            key_id: arguments.key_id.as_deref(),
        }),
        (None, Some(_), None) => Err(argument("--manifest requires --route", "/route")),
        (None, None, Some(_)) => Err(argument("--route requires --manifest", "/manifest")),
        (None, None, None) => Err(argument(
            "one model door is required: --fixture, or --manifest with --route",
            "/fixture",
        )),
    }
}

/// At most one judge door: `--judge-fixture`, or `--judge-route` against the same `--manifest`
/// (and broker coordinates) the draft route uses. clap already refuses the pair and a
/// `--judge-route` without `--manifest`; this only reads what survived.
fn judge_source(arguments: &SynthesizeArguments) -> Result<Option<JudgeSource<'_>>, Failure> {
    match (
        arguments.judge_fixture.as_deref(),
        arguments.judge_route.as_deref(),
    ) {
        (Some(_), Some(_)) => Err(argument(
            "--judge-fixture and --judge-route are mutually exclusive",
            "/judgeFixture",
        )),
        (Some(fixture), None) => Ok(Some(JudgeSource::Fixture(fixture))),
        (None, Some(route)) => {
            let manifest = arguments
                .manifest
                .as_deref()
                .ok_or_else(|| argument("--judge-route requires --manifest", "/manifest"))?;
            Ok(Some(JudgeSource::Gateway {
                manifest,
                route,
                broker: arguments.broker.as_deref(),
                keyring: arguments.keyring.as_deref(),
                key_id: arguments.key_id.as_deref(),
            }))
        }
        (None, None) => Ok(None),
    }
}

/// Writes the document pretty-printed with a trailing newline — the same bytes
/// `core/architect/fixtures/first-compile/expected.json` holds — through `create_new`, and syncs
/// it. A failure names the error kind and never the path.
fn write_document(out: &Path, document: &Value) -> Result<(), Failure> {
    let mut bytes = serde_json::to_vec_pretty(document)
        .map_err(|_| argument("the document could not be serialized", "/out"))?;
    bytes.push(b'\n');
    let unwritable = |error: &std::io::Error| {
        argument(
            &format!("--out could not be written: {}", error.kind()),
            "/out",
        )
    };
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(out)
        .map_err(|error| unwritable(&error))?;
    file.write_all(&bytes).map_err(|error| unwritable(&error))?;
    file.sync_all().map_err(|error| unwritable(&error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(out: &Path) -> SynthesizeArguments {
        SynthesizeArguments {
            goal: "g".to_owned(),
            out: out.to_path_buf(),
            mode: None,
            max_nodes: None,
            allow_programs: Vec::new(),
            fixture: None,
            manifest: None,
            route: None,
            broker: None,
            keyring: None,
            key_id: None,
            judge_route: None,
            judge_fixture: None,
            drafts: None,
            library: None,
        }
    }

    #[test]
    fn exactly_one_model_door_is_accepted() {
        let directory = tempfile::tempdir().unwrap();
        let out = directory.path().join("o.json");
        let fixture = directory.path().join("f.json");
        let manifest = directory.path().join("m.json");

        let none = arguments(&out);
        assert_eq!(model_source(&none).err().unwrap().pointer, "/fixture");

        let mut both = arguments(&out);
        both.fixture = Some(fixture.clone());
        both.manifest = Some(manifest.clone());
        both.route = Some("r".to_owned());
        assert_eq!(model_source(&both).err().unwrap().pointer, "/fixture");

        let mut half = arguments(&out);
        half.manifest = Some(manifest);
        assert_eq!(model_source(&half).err().unwrap().pointer, "/route");

        let mut other_half = arguments(&out);
        other_half.route = Some("r".to_owned());
        assert_eq!(
            model_source(&other_half).err().unwrap().pointer,
            "/manifest"
        );

        let mut only_fixture = arguments(&out);
        only_fixture.fixture = Some(fixture.clone());
        assert!(matches!(
            model_source(&only_fixture),
            Ok(ModelSource::Fixture(path)) if path == fixture
        ));
    }

    #[test]
    fn out_must_be_a_fresh_json_path() {
        let directory = tempfile::tempdir().unwrap();
        assert_eq!(
            check_out_path(&directory.path().join("graph.yaml"))
                .err()
                .unwrap()
                .message,
            "--out must end in .json"
        );
        let taken = directory.path().join("taken.json");
        std::fs::write(&taken, b"x").unwrap();
        assert!(check_out_path(&taken).is_err());
        assert_eq!(std::fs::read(&taken).unwrap(), b"x");
        assert!(check_out_path(&directory.path().join("fresh.json")).is_ok());
    }

    #[test]
    fn a_refusal_is_one_diagnostic_of_compact_json_at_the_goal() {
        let failure = refused(&ArchitectRefusal::CapabilityMissing {
            node: "build_check".to_owned(),
            program: "python".to_owned(),
        });
        assert_eq!(failure.code, REFUSED_CODE);
        assert_eq!(failure.pointer, "/goal");
        assert_eq!(
            failure.message,
            r#"{"kind":"capabilityMissing","node":"build_check","program":"python"}"#
        );
    }

    /// The mapping is measured on a real adapter call: a `native_runtime` route whose program
    /// does not exist fails to spawn, and the refusal carries the taxonomy's own static text —
    /// not the program path the route named.
    #[test]
    fn a_gateway_error_becomes_model_unavailable_with_its_display_text_only() {
        let directory = tempfile::tempdir().unwrap();
        let program = directory.path().join("no-such-runtime");
        let manifest = graphhelm_gateway::manifest::RouteManifest::from_json(
            &serde_json::json!({
                "manifestVersion": 1,
                "routes": [{
                    "id": "native",
                    "provider": "anthropic",
                    "transport": "native_runtime",
                    "runtime": "claude_code",
                    "authentication": "account_subscription",
                    "billingMode": "subscription_quota",
                    "command": { "program": program.to_str().unwrap(), "args": [] },
                    "profiles": ["software_execution"],
                    "enabled": true
                }]
            })
            .to_string(),
        )
        .unwrap();
        let model = GatewayDraftModel::native_runtime(manifest.routes()[0].clone());
        let refusal = model.draft("prompt").err().unwrap();
        let ArchitectRefusal::ModelUnavailable { message } = &refusal else {
            panic!("{refusal:?}");
        };
        assert_eq!(
            message,
            &graphhelm_gateway::taxonomy::GatewayError::ProviderUnavailable.to_string()
        );
        assert!(
            !message.contains("no-such-runtime"),
            "the route's program path leaked: {message}"
        );
        assert_eq!(
            refused(&refusal).message,
            r#"{"kind":"modelUnavailable","message":"the provider is unavailable"}"#
        );
    }
}
