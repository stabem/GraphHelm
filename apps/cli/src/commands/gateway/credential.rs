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
    Failure, broker_failure, credential_error, finish, invalid, passphrase_from_env,
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
        execute_set(
            broker_dir,
            keyring_dir,
            key_id,
            reference,
            provider,
            usable_by,
        ),
        |summary| summary,
    )
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

fn execute_set(
    broker_dir: &Path,
    keyring_dir: &Path,
    key_id: &str,
    reference: &str,
    provider: &str,
    usable_by: &str,
) -> Result<serde_json::Value, Failure> {
    require_keyring_directory(keyring_dir)?;
    let passphrase = passphrase_from_env()?;
    let value = read_stdin_secret()?;
    let usable_by = parse_usable_by(usable_by)?;

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
