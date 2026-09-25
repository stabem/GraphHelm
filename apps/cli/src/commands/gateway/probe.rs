//! `graphhelm gateway probe --manifest <file> --route <id> [--broker <dir> --keyring <dir>
//! --key-id <id>]`: a quota-free health check (§18) — never a real model call. A `direct_api`
//! route proves its credential leases from the broker; a `native_runtime` route proves its CLI
//! spawns and exits `0` on `--version` within a fixed 10s budget. Never prints a credential value:
//! the `direct_api` check only ever records whether `CredentialBroker::lease` succeeded, never the
//! bytes it returned.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use graphhelm_events::SecretBytes;
use graphhelm_gateway::manifest::{ModelRoute, RouteManifest, Transport};
use graphhelm_model_gateway::broker::{BrokerError, CredentialBroker};
use graphhelm_model_gateway::env;
use serde_json::json;

use super::{
    Failure, broker_failure, finish, invalid, load_manifest, passphrase_from_env, probe_error,
    require_keyring_directory, runtime,
};
use crate::output::Outcome;

const COMMAND: &str = "gateway.probe";

/// Fixed probe budget (plan Task 6): independent of `route.timeout_seconds()`, which bounds a real
/// call — a health probe must stay fast regardless of how long the route's own calls are allowed
/// to run.
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const POLL_INTERVAL: Duration = Duration::from_millis(50);

struct Check {
    name: &'static str,
    ok: bool,
}

pub(super) struct ProbeResult {
    route: String,
    checks: Vec<Check>,
    health: &'static str,
}

pub(in crate::commands) fn run(
    manifest: &Path,
    route_id: &str,
    broker: Option<&Path>,
    keyring: Option<&Path>,
    key_id: Option<&str>,
) -> Outcome {
    finish(
        COMMAND,
        execute(manifest, route_id, broker, keyring, key_id),
        render,
    )
}

fn execute(
    manifest: &Path,
    route_id: &str,
    broker: Option<&Path>,
    keyring: Option<&Path>,
    key_id: Option<&str>,
) -> Result<ProbeResult, Failure> {
    let manifest = load_manifest(manifest)?;
    probe_loaded(
        &manifest,
        route_id,
        broker,
        keyring,
        key_id,
        passphrase_from_env,
    )
}

/// The probe over an already-loaded manifest, with the broker passphrase supplied by `passphrase`
/// — read from `GRAPHHELM_GATEWAY_KEY` by [`run`], handed in from `serve.key` by `gateway setup`
/// (#1139), which has just stored the credential under that same passphrase. `passphrase` is
/// consulted only for a `direct_api` route, exactly as before: a `native_runtime` probe needs no
/// broker and must not fail for the lack of one.
pub(super) fn probe_loaded(
    manifest: &RouteManifest,
    route_id: &str,
    broker: Option<&Path>,
    keyring: Option<&Path>,
    key_id: Option<&str>,
    passphrase: impl FnOnce() -> Result<SecretBytes, Failure>,
) -> Result<ProbeResult, Failure> {
    let route = manifest
        .routes()
        .iter()
        .find(|route| route.id() == route_id)
        .ok_or_else(|| invalid("--route does not name a route in the manifest", "/route"))?;
    if !route.enabled() {
        return Err(probe_error(
            "the route is disabled and cannot be probed",
            "/route",
        ));
    }

    let (check, health) = match route.transport() {
        Transport::DirectApi => probe_direct_api(route, broker, keyring, key_id, passphrase)?,
        Transport::NativeRuntime => {
            let check = probe_native_runtime(route);
            let health = if check.ok { "available" } else { "unavailable" };
            (check, health)
        }
    };

    Ok(ProbeResult {
        route: route.id().to_owned(),
        checks: vec![check],
        health,
    })
}

/// The `direct_api` check: a successful [`CredentialBroker::lease`] and nothing else — no network
/// call is ever placed. Opening the broker itself (missing passphrase, missing keyring, unreadable
/// store) is a hard `GHCLI010_GATEWAY_CREDENTIAL` failure.
///
/// A `lease` failure is not automatically a hard failure, but it is not automatically
/// `auth_required` either (PR review IMPORTANT 8 — the pre-fix code collapsed every lease error
/// into `ok: false` / `health: "auth_required"`, which is wrong for a corrupt or unreadable
/// store): `NotFound`/`Revoked`/`NotUsableByRoute` are facts about the *route's authorization* —
/// still a successful probe reply, `health: "auth_required"`. `Sealed`/`Storage`/`Corrupt` (which
/// includes a tampered access-control MAC, BLOCKER 2) and `KeyProvider`/`InvalidReference` are
/// facts about the *store itself* being unhealthy — a distinct failed check name
/// (`credentialStoreIntact`) and `health: "unavailable"`, so an operator does not mistake "the
/// store is corrupt" for "rotate this credential."
fn probe_direct_api(
    route: &ModelRoute,
    broker: Option<&Path>,
    keyring: Option<&Path>,
    key_id: Option<&str>,
    passphrase: impl FnOnce() -> Result<SecretBytes, Failure>,
) -> Result<(Check, &'static str), Failure> {
    let (broker, keyring, key_id) = match (broker, keyring, key_id) {
        (Some(broker), Some(keyring), Some(key_id)) => (broker, keyring, key_id),
        _ => {
            return Err(invalid(
                "--broker, --keyring and --key-id are required to probe a direct_api route",
                "/arguments",
            ));
        }
    };
    require_keyring_directory(keyring)?;
    let passphrase = passphrase()?;

    let credential_ref = route
        .credential_ref()
        .expect("direct_api routes carry credentialRef — enforced by manifest validation")
        .to_owned();
    let route_id = route.id().to_owned();
    let broker_dir: PathBuf = broker.to_path_buf();
    let keyring_dir: PathBuf = keyring.to_path_buf();
    let key_id = key_id.to_owned();

    let lease_result = runtime()?.block_on(async move {
        let opened = CredentialBroker::open(&broker_dir, &keyring_dir, &key_id, passphrase)
            .await
            .map_err(|error| broker_failure(&error))?;
        Ok::<_, Failure>(opened.lease(&credential_ref, &route_id).await)
    })?;

    Ok(match lease_result {
        Ok(_) => (
            Check {
                name: "credential",
                ok: true,
            },
            "available",
        ),
        Err(
            BrokerError::NotFound { .. }
            | BrokerError::Revoked { .. }
            | BrokerError::NotUsableByRoute { .. },
        ) => (
            Check {
                name: "credential",
                ok: false,
            },
            "auth_required",
        ),
        Err(
            BrokerError::Sealed(_)
            | BrokerError::Storage { .. }
            | BrokerError::Corrupt { .. }
            | BrokerError::KeyProvider(_)
            | BrokerError::ProviderMismatch { .. }
            | BrokerError::InvalidReference { .. },
        ) => (
            Check {
                name: "credentialStoreIntact",
                ok: false,
            },
            "unavailable",
        ),
    })
}

/// The `native_runtime` check: spawn `route.command.program` with the single argument `--version`
/// (never the manifest's own configured `args` — this is a liveness probe of the CLI itself, not a
/// real invocation) under the same environment discipline as
/// `adapters/model-gateway/src/runtime.rs`'s adapter — `env_clear()` plus
/// `graphhelm_model_gateway::env::compose` (PR review MEDIUM 14: this module used to hand-copy
/// `runtime.rs`'s private `ENV_ALLOWLIST` verbatim into its own constant; importing the shared
/// `env` module instead means the two can never drift) — and require a clean exit within
/// [`PROBE_TIMEOUT`]. No prompt is sent: stdin is immediately closed.
fn probe_native_runtime(route: &ModelRoute) -> Check {
    let command_spec = route
        .command()
        .expect("native_runtime routes carry command — enforced by manifest validation");

    let mut command = Command::new(&command_spec.program);
    command
        .arg("--version")
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for (key, value) in env::compose(&[]) {
        command.env(key, value);
    }

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => {
            return Check {
                name: "runtime",
                ok: false,
            };
        }
    };

    let deadline = Instant::now() + PROBE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return Check {
                    name: "runtime",
                    ok: status.success(),
                };
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Check {
                        name: "runtime",
                        ok: false,
                    };
                }
                std::thread::sleep(POLL_INTERVAL);
            }
            Err(_) => {
                return Check {
                    name: "runtime",
                    ok: false,
                };
            }
        }
    }
}

pub(super) fn render(result: ProbeResult) -> serde_json::Value {
    json!({
        "route": result.route,
        "checks": result
            .checks
            .iter()
            .map(|check| json!({"name": check.name, "ok": check.ok}))
            .collect::<Vec<_>>(),
        "health": result.health,
    })
}
