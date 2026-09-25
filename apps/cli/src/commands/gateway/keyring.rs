//! `graphhelm gateway keyring init`: the sealing key on its own.
//!
//! A Runtime started with `--keyring`/`--key-id` seals evidence — a model's reply, a tool's
//! output, the words of a message — before the event that references it appends. That needs a key
//! in the keyring, and until this command there was exactly one way to put one there:
//! `gateway credential set`, which stores a BYOK model credential and reads a secret from stdin.
//!
//! The two needs are unrelated. An operator who wants the message path and no model at all was
//! obliged to invent an API key to get past the setup, and the Runtime gave no sign anything was
//! wrong until the first message came back refused (`the sealed keyring could not be opened`,
//! measured 2026-08-28 against a Runtime started with an empty keyring directory).
//!
//! THE VARIABLE MATTERS. The key is created under `GRAPHHELM_EVENTS_KEY`, because that is the
//! variable `execution::signal`'s `open_sealer` uses to open it. Creating it under the broker's
//! `GRAPHHELM_GATEWAY_KEY` would produce a keyring that exists, contains the named key, and cannot
//! be opened by the thing that needs it.

use std::path::Path;

use graphhelm_events::SecretBytes;
use graphhelm_sealed_key_provider::SealedKeyProvider;
use serde_json::json;

use super::{Failure, credential_error, finish, key_from_env, require_keyring_directory};
use crate::output::Outcome;

const INIT_COMMAND: &str = "gateway.keyring.init";

/// The variable the sealing key is created under. Named once, here, beside the reason.
pub(in crate::commands) const SEALING_KEY_ENVIRONMENT: &str = "GRAPHHELM_EVENTS_KEY";

pub(in crate::commands) fn init(keyring_dir: &Path, key_id: &str) -> Outcome {
    finish(INIT_COMMAND, execute_init(keyring_dir, key_id), |summary| {
        summary
    })
}

fn execute_init(keyring_dir: &Path, key_id: &str) -> Result<serde_json::Value, Failure> {
    require_keyring_directory(keyring_dir)?;
    let passphrase = key_from_env(SEALING_KEY_ENVIRONMENT)?;
    match create(keyring_dir, key_id, passphrase) {
        Ok(()) => {}
        // Refused rather than overwritten. Overwriting orphans every piece of evidence already
        // sealed under the old key: the events keep referencing content nothing can open again,
        // and the loss is silent because an unopenable reference looks exactly like one whose
        // Runtime lacks the key. A second `init` is far more likely to be a repeated setup step
        // than a deliberate rotation.
        //
        // The one retry that can succeed is a NEW DIRECTORY. "Choose another --key-id" used to
        // be offered too, and it cannot work: `SealedKeyProvider::create` refuses any directory
        // already holding a keyring whatever the id, so the operator who followed that advice
        // stayed blocked with a generic error (PR #467 review).
        Err(CreateError::AlreadyHoldsKey) => {
            return Err(credential_error(
                "this keyring already holds that key, and replacing it would orphan everything already sealed under it; point --keyring at a new directory",
                "/keyring",
            ));
        }
        Err(CreateError::Uncreatable) => {
            return Err(credential_error(
                "the sealing key could not be created in that keyring",
                "/keyring",
            ));
        }
    }

    Ok(json!({
        "keyId": key_id,
        "createdUnder": SEALING_KEY_ENVIRONMENT,
    }))
}

/// Why [`create`] did not create a key. Typed, not a `Failure`: `gateway keyring init` treats
/// [`CreateError::AlreadyHoldsKey`] as a refusal (rotation would orphan sealed evidence), while
/// `init` (#1062) treats the same answer as "already provisioned" and reports `existing` — the
/// two commands agree on the fact and differ on what it means for them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::commands) enum CreateError {
    /// `passphrase` already opens `key_id` in this directory.
    AlreadyHoldsKey,
    /// `SealedKeyProvider::create` refused: the directory is unwritable, or holds a keyring this
    /// passphrase does not open.
    Uncreatable,
}

/// The core of `gateway keyring init` with the passphrase supplied by the caller: the environment
/// is read by [`init`]'s wrapper, never here, so `init` (#1062) can hand in the passphrase it read
/// from the project's `serve.key` file. Pre-flights `open` so an existing key is reported as such
/// rather than clobbered; the passphrase is copied once for that pre-flight (see
/// [`passphrase_copy`]).
pub(in crate::commands) fn create(
    keyring_dir: &Path,
    key_id: &str,
    passphrase: SecretBytes,
) -> Result<(), CreateError> {
    if SealedKeyProvider::open(keyring_dir, key_id.to_owned(), passphrase_copy(&passphrase)).is_ok()
    {
        return Err(CreateError::AlreadyHoldsKey);
    }
    SealedKeyProvider::create(keyring_dir, key_id, passphrase)
        .map(|_| ())
        .map_err(|_| CreateError::Uncreatable)
}

/// `SecretBytes` is consumed by both `open` and `create`, and the pre-flight above needs one of
/// each. Copying it is not a widening: the plaintext is already in this process, and the copy lives
/// no longer than the call.
fn passphrase_copy(passphrase: &SecretBytes) -> SecretBytes {
    passphrase.expose(|bytes| SecretBytes::new(bytes.to_vec()))
}
