//! `graphhelm gateway credential set|remove`: a durable, revocable, route-scoped BYOK credential
//! store over `adapters/model-gateway/src/broker.rs`'s `CredentialBroker`. The credential value
//! itself crosses this module exactly once per `set` — read from stdin as one trimmed line,
//! sealed, and never held past that — and is never present in `remove`'s output or in any failure
//! either command can produce (rule 6 of the plan's binding process rules).

use std::io::BufRead;
use std::path::Path;

use graphhelm_events::SecretBytes;
use graphhelm_model_gateway::broker::{CredentialBroker, SecretReference};
use serde_json::json;

use super::{
    Failure, broker_failure, credential_error, finish, invalid, load_manifest, passphrase_from_env,
    require_keyring_directory, runtime,
};
use crate::output::Outcome;

const SET_COMMAND: &str = "gateway.credential.set";
const REMOVE_COMMAND: &str = "gateway.credential.remove";

pub(in crate::commands) fn set(
    broker_dir: &Path,
    keyring_dir: &Path,
    key_id: &str,
    reference: &str,
    provider: &str,
    usable_by: &str,
) -> Outcome {
    finish(
        SET_COMMAND,
        parse_usable_by(usable_by).and_then(|routes| {
            let value = read_stdin_secret()?;
            store(
                broker_dir,
                keyring_dir,
                key_id,
                reference,
                provider,
                routes,
                value,
            )
        }),
        |summary| summary,
    )
}

/// The same store, with the value handed in rather than read from stdin (#1171).
///
/// STDIN IS A PROPERTY OF THE CLI, NOT OF THE STORE. `PUT /v1/gateway/credentials/{ref}` has no
/// stdin to read and must reach the same broker through the same call, or the two doors are two
/// stores that can drift — D-039's rule again. Everything that makes `set` safe lives below this
/// line and is shared: the keyring precondition, the passphrase from the process environment, the
/// reference's own charset and bound inside `CredentialBroker::store`, and a reply that names the
/// reference and never the value.
/// HTTP rotation keeps the broker's existing route scope, but only for the routes that reach the
/// same endpoint (#1182). The browser only knows the route it is editing, while a credential
/// reference may intentionally be shared by several routes; replacing the scope with the
/// browser's one-item list would silently revoke the other routes. Keeping EVERY other route
/// would hand the rotated key to a route that sends it to a different vendor under the same wire
/// format, so the peers are read from `manifest_path`: a route is a peer when its provider and
/// base URL equal those of a route being rotated. An unreadable manifest, or a rotated route it
/// does not name, yields no peers, and the rotation then fails closed to `usable_by` alone.
#[allow(clippy::too_many_arguments)]
pub(in crate::commands) fn set_value_preserving_scope(
    broker_dir: &Path,
    keyring_dir: &Path,
    key_id: &str,
    manifest_path: &Path,
    reference: &str,
    provider: &str,
    usable_by: Vec<String>,
    value: SecretBytes,
) -> Outcome {
    let peers = endpoint_peers(manifest_path, &usable_by);
    finish(
        SET_COMMAND,
        store_preserving_scope(
            broker_dir,
            keyring_dir,
            key_id,
            reference,
            provider,
            usable_by,
            &peers,
            value,
        ),
        |summary| summary,
    )
}

/// The manifest routes that reach the same endpoint — equal provider and equal base URL, a
/// trailing `/` ignored — as any route in `rotated`. Empty when the manifest cannot be read or
/// names none of `rotated`: no evidence of a shared endpoint is never read as one.
fn endpoint_peers(manifest_path: &Path, rotated: &[String]) -> Vec<String> {
    let Ok(manifest) = load_manifest(manifest_path) else {
        return Vec::new();
    };
    let endpoint = |route: &graphhelm_gateway::manifest::ModelRoute| {
        (
            route.provider().to_owned(),
            route
                .base_url()
                .map(|url| url.trim_end_matches('/').to_owned()),
        )
    };
    let targets: Vec<_> = manifest
        .routes()
        .iter()
        .filter(|route| rotated.iter().any(|id| id == route.id()))
        .map(endpoint)
        .collect();
    manifest
        .routes()
        .iter()
        .filter(|route| targets.contains(&endpoint(route)))
        .map(|route| route.id().to_owned())
        .collect()
}

pub(in crate::commands) fn remove(
    broker_dir: &Path,
    keyring_dir: &Path,
    key_id: &str,
    reference: &str,
) -> Outcome {
    finish(
        REMOVE_COMMAND,
        execute_remove(broker_dir, keyring_dir, key_id, reference),
        |summary| summary,
    )
}

fn store(
    broker_dir: &Path,
    keyring_dir: &Path,
    key_id: &str,
    reference: &str,
    provider: &str,
    usable_by: Vec<String>,
    value: SecretBytes,
) -> Result<serde_json::Value, Failure> {
    require_keyring_directory(keyring_dir)?;
    let passphrase = passphrase_from_env()?;

    let reference = SecretReference {
        id: reference.to_owned(),
        provider: provider.to_owned(),
        usable_by,
    };
    let broker_dir = broker_dir.to_path_buf();
    let keyring_dir = keyring_dir.to_path_buf();
    let key_id = key_id.to_owned();

    runtime()?.block_on(async move {
        let mut broker =
            CredentialBroker::open_or_create(&broker_dir, &keyring_dir, &key_id, passphrase)
                .await
                .map_err(|error| broker_failure(&error))?;
        broker
            .store(reference.clone(), value)
            .await
            .map_err(|error| broker_failure(&error))?;
        Ok(json!({
            "id": reference.id,
            "provider": reference.provider,
            "routes": reference.usable_by,
        }))
    })
}

#[allow(clippy::too_many_arguments)]
fn store_preserving_scope(
    broker_dir: &Path,
    keyring_dir: &Path,
    key_id: &str,
    reference: &str,
    provider: &str,
    usable_by: Vec<String>,
    peers: &[String],
    value: SecretBytes,
) -> Result<serde_json::Value, Failure> {
    require_keyring_directory(keyring_dir)?;
    let passphrase = passphrase_from_env()?;
    let broker_dir = broker_dir.to_path_buf();
    let keyring_dir = keyring_dir.to_path_buf();
    let key_id = key_id.to_owned();
    let reference = reference.to_owned();
    let provider = provider.to_owned();
    let peers = peers.to_vec();
    runtime()?.block_on(async move {
        let mut broker =
            CredentialBroker::open_or_create(&broker_dir, &keyring_dir, &key_id, passphrase)
                .await
                .map_err(|error| broker_failure(&error))?;
        let reference_value = SecretReference {
            id: reference.clone(),
            provider: provider.clone(),
            usable_by,
        };
        let reference_value = broker
            .store_preserving_existing_scope(reference_value, value, &peers)
            .await
            .map_err(|error| match error {
                graphhelm_model_gateway::broker::BrokerError::ProviderMismatch { .. } => {
                    credential_error(
                        "credential provider does not match the existing reference",
                        "/provider",
                    )
                }
                other => broker_failure(&other),
            })?;
        Ok(json!({
            "id": reference_value.id,
            "provider": reference_value.provider,
            "routes": reference_value.usable_by,
        }))
    })
}

fn execute_remove(
    broker_dir: &Path,
    keyring_dir: &Path,
    key_id: &str,
    reference: &str,
) -> Result<serde_json::Value, Failure> {
    require_keyring_directory(keyring_dir)?;
    let passphrase = passphrase_from_env()?;

    let reference = reference.to_owned();
    let broker_dir = broker_dir.to_path_buf();
    let keyring_dir = keyring_dir.to_path_buf();
    let key_id = key_id.to_owned();

    runtime()?.block_on(async move {
        let mut broker = CredentialBroker::open(&broker_dir, &keyring_dir, &key_id, passphrase)
            .await
            .map_err(|error| broker_failure(&error))?;
        broker
            .revoke(&reference)
            .await
            .map_err(|error| broker_failure(&error))?;
        Ok(json!({ "id": reference, "revoked": true }))
    })
}

/// `--usable-by route_a,route_b`: a non-empty, comma-separated list of route ids. Splitting is all
/// this does — `CredentialBroker::store` already validates each entry's charset and bound and
/// reports `BrokerError::InvalidReference` if one is malformed, which `broker_failure` maps to
/// `GHCLI010_GATEWAY_CREDENTIAL`.
fn parse_usable_by(raw: &str) -> Result<Vec<String>, Failure> {
    let routes: Vec<String> = raw
        .split(',')
        .map(str::trim)
        .filter(|route| !route.is_empty())
        .map(str::to_owned)
        .collect();
    if routes.is_empty() {
        return Err(invalid(
            "--usable-by must name at least one route id",
            "/usableBy",
        ));
    }
    Ok(routes)
}

/// Reads exactly one line from stdin and trims its line ending, refusing to proceed if nothing was
/// piped or the line is blank. This is the only channel a credential value may enter through (rule
/// 6 of the plan's binding process rules: secrets never in argv).
///
/// Reads into a `Zeroizing<Vec<u8>>` rather than a plain `String` (PR review IMPORTANT 5a): a
/// bare `String` grown by `read_line` leaves its backing allocation unzeroized once dropped, so an
/// error path taken after the bytes were read (the empty-line check, or the UTF-8 check below)
/// would otherwise free the credential's bytes without clearing them first. Invalid UTF-8 is
/// refused explicitly with a named-encoding message under `GHCLI010_GATEWAY_CREDENTIAL` rather
/// than silently repaired via a lossy conversion, which would corrupt the credential value instead
/// of reporting the problem.
fn read_stdin_secret() -> Result<SecretBytes, Failure> {
    let mut buffer = zeroize::Zeroizing::new(Vec::<u8>::new());
    let read = std::io::stdin()
        .lock()
        .read_until(b'\n', &mut buffer)
        .map_err(|_| {
            invalid(
                "the credential value could not be read from stdin",
                "/stdin",
            )
        })?;
    if read == 0 {
        return Err(invalid("a credential value is required on stdin", "/stdin"));
    }
    while matches!(buffer.last(), Some(b'\r' | b'\n')) {
        buffer.pop();
    }
    if buffer.is_empty() {
        return Err(invalid(
            "the credential value on stdin must not be empty",
            "/stdin",
        ));
    }
    if std::str::from_utf8(&buffer).is_err() {
        return Err(credential_error(
            "the credential value on stdin is not valid UTF-8",
            "/stdin",
        ));
    }
    Ok(SecretBytes::new(std::mem::take(&mut *buffer)))
}
