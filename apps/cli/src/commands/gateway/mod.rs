//! `graphhelm gateway` — CLI surface for the Universal Model Gateway's route manifest, quota-free
//! health probe, and credential broker (`docs/superpowers/plans/2026-08-14-gateway-slice.md`
//! Task 6). Three failure classes, following every sibling command family's own redaction-safe
//! `Failure` pattern (`commands::events`, `commands::execution`):
//!
//! - `GHCLI009_GATEWAY_INVALID` — the manifest does not parse/validate, or an argument is
//!   malformed, missing, or does not name a route the manifest declares. Never echoes manifest
//!   file contents: [`graphhelm_gateway::manifest::ManifestError`]'s own `Display` already only
//!   ever names a route id and the violated rule (or, for `Oversize`, sizes), never source bytes.
//! - `GHCLI010_GATEWAY_CREDENTIAL` — the credential broker itself could not be used: a missing or
//!   malformed `GRAPHHELM_GATEWAY_KEY`, a keyring directory that does not exist yet (Task 3's own
//!   finding — `SealedKeyProvider::create`/`open` open a directory handle and never create one,
//!   see [`require_keyring_directory`]), or any [`BrokerError`] the broker itself reports. A
//!   *denied* lease (revoked, wrong route, unknown id) is not routed through this code: `probe`
//!   reports that as a successful reply with `health: "auth_required"` (§18) — a declined
//!   credential is a fact about the route, not a CLI failure.
//! - `GHCLI011_GATEWAY_PROBE` — the probe itself refuses to run for a reason specific to probing
//!   that neither code above covers. This milestone's one case: probing a route the manifest marks
//!   `enabled: false` — the manifest and `--route` are both valid, but nothing about a disabled
//!   route is meaningful to health-check.
//!
//! Every credential value crosses this module exactly once, as a [`SecretBytes`] read from stdin
//! (`credential.rs`) or leased from the broker (`probe.rs`), and is never formatted into a
//! `Failure`, a success payload, or a `Debug` impl — rule 6 of the plan's binding process rules.

pub(super) mod credential;
pub(super) mod probe;
pub(super) mod routes;

use std::io::Read;
use std::path::Path;

use graphhelm_events::SecretBytes;
use graphhelm_gateway::manifest::{MAX_MANIFEST_BYTES, RouteManifest};
use graphhelm_model_gateway::broker::BrokerError;
use graphhelm_protocols::Diagnostic;

use crate::output::Outcome;

pub(super) const INVALID_CODE: &str = "GHCLI009_GATEWAY_INVALID";
pub(super) const CREDENTIAL_CODE: &str = "GHCLI010_GATEWAY_CREDENTIAL";
pub(super) const PROBE_CODE: &str = "GHCLI011_GATEWAY_PROBE";
const SOURCE: &str = "gateway-cli";

/// Mirrors `commands::events::config`'s `GRAPHHELM_EVENTS_KEY` precedent exactly
/// (`apps/cli/src/commands/events/config.rs`): 64 lowercase hexadecimal characters, kept out of
/// every configuration file and argument list, and read fresh from the environment for each
/// invocation.
const KEY_ENVIRONMENT: &str = "GRAPHHELM_GATEWAY_KEY";

/// A redaction-safe operator failure — the same shape `commands::events` and `commands::execution`
/// each define for themselves rather than share (see `commands::execution::Failure`'s own doc
/// comment on why this codebase mirrors small per-family helpers instead of widening visibility).
pub(super) struct Failure {
    pub(super) code: &'static str,
    pub(super) message: String,
    pub(super) pointer: String,
}

impl Failure {
    fn into_outcome(self, command: &'static str) -> Outcome {
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

pub(super) fn invalid(message: &str, pointer: &str) -> Failure {
    Failure {
        code: INVALID_CODE,
        message: message.to_owned(),
        pointer: pointer.to_owned(),
    }
}

pub(super) fn credential_error(message: &str, pointer: &str) -> Failure {
    Failure {
        code: CREDENTIAL_CODE,
        message: message.to_owned(),
        pointer: pointer.to_owned(),
    }
}

pub(super) fn probe_error(message: &str, pointer: &str) -> Failure {
    Failure {
        code: PROBE_CODE,
        message: message.to_owned(),
        pointer: pointer.to_owned(),
    }
}

pub(super) fn finish<T>(
    command: &'static str,
    result: Result<T, Failure>,
    render: impl FnOnce(T) -> serde_json::Value,
) -> Outcome {
    match result {
        Ok(value) => Outcome::success(command, render(value)),
        Err(failure) => failure.into_outcome(command),
    }
}

/// Loads and validates a route manifest from `path`. Never echoes the file's contents: a read
/// failure reports only that the file could not be read, and a validation failure reports only
/// `ManifestError`'s own route-id-and-rule `Display` text.
pub(super) fn load_manifest(path: &Path) -> Result<RouteManifest, Failure> {
    let bytes = read_bounded_manifest(path)?;
    let text = String::from_utf8(bytes)
        .map_err(|_| invalid("--manifest is not valid UTF-8", "/manifest"))?;
    RouteManifest::from_json(&text).map_err(|error| invalid(&error.to_string(), "/manifest"))
}

/// Reads `path` bounded by [`MAX_MANIFEST_BYTES`], refusing an oversize file via its metadata
/// length BEFORE reading any of its bytes (PR review IMPORTANT 15 — mirrors
/// `commands::events::config::read_bounded`,
/// `apps/cli/src/commands/events/config.rs:154`). `RouteManifest::from_json` itself only enforces
/// the bound after its caller has already handed it a whole in-memory string; a malformed or
/// hostile file that is merely large must never be read into memory at all just to be refused.
/// The metadata check is the actual guard; `Read::take(MAX_MANIFEST_BYTES + 1)` is defense in
/// depth against a file that grows between the metadata call and the read.
fn read_bounded_manifest(path: &Path) -> Result<Vec<u8>, Failure> {
    let unreadable = || invalid("--manifest does not name a readable file", "/manifest");
    let too_large = || invalid("--manifest exceeds the maximum supported size", "/manifest");

    let metadata = std::fs::metadata(path).map_err(|_| unreadable())?;
    if metadata.len() > MAX_MANIFEST_BYTES as u64 {
        return Err(too_large());
    }

    let file = std::fs::File::open(path).map_err(|_| unreadable())?;
    let mut bytes = Vec::new();
    file.take(MAX_MANIFEST_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| unreadable())?;
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(too_large());
    }
    Ok(bytes)
}

/// Confirms the keyring directory already exists. `SealedKeyProvider::create`/`open`
/// (`adapters/sealed-key-provider/src/lib.rs`) each open an existing directory handle
/// (`AnchoredDirectory::open`) and never create one; `CredentialBroker::create`'s own doc comment
/// states the same precondition (Task 3 finding). The broker directory *is* created on demand
/// (`CredentialBroker::create` calls `create_dir_all` on it), so only the keyring is checked here.
pub(super) fn require_keyring_directory(path: &Path) -> Result<(), Failure> {
    let metadata = std::fs::metadata(path)
        .map_err(|_| credential_error("the keyring directory does not exist", "/keyring"))?;
    if !metadata.is_dir() {
        return Err(credential_error(
            "the keyring path is not a directory",
            "/keyring",
        ));
    }
    Ok(())
}

/// Reads the gateway passphrase from `GRAPHHELM_GATEWAY_KEY`: 64 lowercase hexadecimal characters,
/// decoded to 32 bytes. Byte-for-byte the same shape and error message
/// `commands::events::config::decode_key` uses for `GRAPHHELM_EVENTS_KEY` — duplicated rather than
/// shared, matching this codebase's own precedent for small per-command-family helpers.
///
/// `std::env::var` hands back an owned `String` copied out of the process environment by the
/// standard library itself — the OS's/CRT's own copy of the variable is beyond anything this
/// process can zeroize (PR review IMPORTANT 5b's honest limit: see
/// `docs/superpowers/plans/2026-08-14-gateway-slice.md`'s honest-limits section, which now notes
/// this alongside `ureq`'s/the OS's own retained copies). What this function *does* control is
/// never letting a second, unzeroized copy of that string exist afterward:
/// [`String::into_bytes`] reuses the same allocation (no copy), which is immediately moved into a
/// `Zeroizing` buffer, so the only remaining plaintext copy is the one std itself made — dropped
/// and zeroized here as early as possible.
pub(super) fn passphrase_from_env() -> Result<SecretBytes, Failure> {
    let encoded = std::env::var(KEY_ENVIRONMENT).map_err(|_| {
        credential_error(
            "GRAPHHELM_GATEWAY_KEY must supply 64 lowercase hexadecimal characters",
            "/keyring",
        )
    })?;
    let encoded = zeroize::Zeroizing::new(encoded.into_bytes());
    decode_key(&encoded)
}

fn decode_key(encoded: &[u8]) -> Result<SecretBytes, Failure> {
    let invalid = || {
        credential_error(
            "GRAPHHELM_GATEWAY_KEY must supply 64 lowercase hexadecimal characters",
            "/keyring",
        )
    };
    if encoded.len() != 64 {
        return Err(invalid());
    }
    let mut bytes = zeroize::Zeroizing::new(Vec::<u8>::with_capacity(32));
    for pair in encoded.chunks_exact(2) {
        let high = hex_value(pair[0]).ok_or_else(invalid)?;
        let low = hex_value(pair[1]).ok_or_else(invalid)?;
        bytes.push((high << 4) | low);
    }
    Ok(SecretBytes::new(std::mem::take(&mut *bytes)))
}

const fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

/// Maps any broker failure onto the credential failure class. Covers both "the broker mechanism
/// itself could not be used" (open/store/revoke I/O, corrupt store, sealing failure) and "the
/// reference was malformed" (`BrokerError::InvalidReference`) — both are the broker refusing the
/// operation outright, never a manifest or probe-specific concern. A *denied lease* specifically
/// (`BrokerError::NotFound`/`Revoked`/`NotUsableByRoute`) is deliberately never routed through
/// here by `probe.rs`, which reports those as a successful reply instead (see the module doc
/// comment).
pub(super) fn broker_failure(error: &BrokerError) -> Failure {
    credential_error(&error.to_string(), "/broker")
}

/// The bounded current-thread runtime every broker-touching command bridges through, identical in
/// shape to `commands::events::runtime` (`apps/cli/src/commands/events/mod.rs`) — the broker's own
/// `create`/`open`/`store`/`lease`/`revoke` are `async fn`s over `EvidenceProtector`, and this is
/// the CLI's synchronous command layer's bridge onto them (plan Task 3's own note; precedent
/// `apps/cli/src/commands/events/backup.rs:32`).
pub(super) fn runtime() -> Result<tokio::runtime::Runtime, Failure> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| credential_error("the operator runtime could not be started", "/"))
}
