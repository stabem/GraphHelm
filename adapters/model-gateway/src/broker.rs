//! Credential broker over `graphhelm_events::EvidenceProtector<SealedKeyProvider>`.
//!
//! `docs/models/UNIVERSAL_MODEL_GATEWAY.md` §7 describes the broker; this module implements the
//! Milestone 05b subset of it (gateway-slice plan, Task 3): a
//! durable, revocable, route-scoped store for BYOK credentials, built entirely out of the sealing
//! primitives `core/events` already provides. It invents no cryptography of its own — `store`
//! seals a value with [`EvidenceProtector::seal`] and `lease` opens it with
//! [`EvidenceProtector::open`]; this module only owns the on-disk index (`credentials.json`) that
//! maps a reference id to its [`SealedEvidence`] parts, the route-scoping rule (`usable_by`), and
//! the revocation flag.
//!
//! The broker never caches plaintext: [`CredentialBroker`] holds only sealed evidence in memory,
//! and every [`CredentialBroker::lease`] call re-derives the value fresh from the sealed parts and
//! the key provider. Nothing here ever formats a credential value or a passphrase into an error or
//! a `Debug` — see [`BrokerError`].
//!
//! `revoked`, `usable_by`, and `provider` are access-control facts, not confidential values, so
//! they are stored as plaintext JSON rather than sealed alongside the credential value itself —
//! but plaintext access-control fields written straight to disk would let anyone with write access
//! to `broker_dir` flip `revoked` back to `false` or widen `usable_by` without ever touching the
//! AEAD-protected value, bypassing every guard in this module (PR review BLOCKER 2). Each entry
//! therefore carries an entry-level authentication tag — computed over exactly those fields plus
//! the sealed value's identity — via the same [`graphhelm_events::KeyProvider::authenticate`]/
//! `verify` primitive the sealed keyring itself uses for its revocation journal. The tag is
//! verified fail-closed on every load and on every lease/revoke read path; a mismatch reports
//! [`BrokerError::Corrupt`], never the tampered values.

use std::{
    collections::BTreeMap,
    fs::File,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use fs2::FileExt;
use graphhelm_events::{
    AuthenticateRequest, AuthenticationTag, EvidenceError, EvidenceInput, EvidenceOpener,
    EvidenceProtector, EvidenceSealer, KeyError, KeyProvider, SealedEvidence, SecretBytes,
    VerifyAuthenticationRequest, WrappedKey,
};
use graphhelm_protocols::{
    EvidenceId, EvidenceReference, ExecutionId, ProjectId, RawSha256, RepositoryScope, Sensitivity,
    WorkspaceId,
};
use graphhelm_sealed_key_provider::SealedKeyProvider;
use serde::{Deserialize, Serialize};

/// The on-disk index file inside `broker_dir`.
const STORE_FILE: &str = "credentials.json";
/// Advisory exclusive lock file held across every load-modify-persist cycle
/// (`store`/`revoke`), so two `CredentialBroker` handles (in this process or another) over the
/// same `broker_dir` cannot race a lost update. Mirrors `core/events/src/local.rs`'s
/// `repository.lock` pattern (`fs2::FileExt::lock_exclusive`).
const LOCK_FILE: &str = "broker.lock";
/// Reserved for future on-disk evolution; this milestone only requires it to be present.
const STORE_VERSION: u32 = 1;
/// Credential values are opaque bytes (API keys, tokens); the broker never interprets them.
const MEDIA_TYPE: &str = "application/octet-stream";
/// Durable until explicitly revoked or rotated — not `ephemeral`, and not under `legal_hold`
/// unless a future milestone adds a reason to hold one.
const RETENTION_CLASS: &str = "standard";
/// Every credential shares one fixed, stable repository scope: the broker is not multi-tenant
/// within a single keyring, and Evidence scoping exists to separate unrelated records, not to
/// namespace a single operator's own credential store.
const SCOPE_WORKSPACE_ID: &str = "gateway";
const SCOPE_PROJECT_ID: &str = "credentials";
/// Bound shared by [`SecretReference::id`], [`SecretReference::provider`], and each entry of
/// [`SecretReference::usable_by`].
const MAX_REFERENCE_TOKEN_LEN: usize = 64;
/// Domain-separating purpose string for the entry-level access-control MAC (PR review BLOCKER 2).
/// Distinct from every purpose `core/events`/`adapters/sealed-key-provider` use internally, so a
/// tag computed for one context can never verify in another.
const ENTRY_MAC_PURPOSE: &str = "gateway.credential.entry";
/// The only algorithm [`KeyProvider::authenticate`] produces; fixed here rather than trusted from
/// disk (see [`verify_entry_mac`]).
const ENTRY_MAC_ALGORITHM: &str = "hmac-sha256";

/// The identity, provider tag, and route scoping of one broker-held credential.
///
/// Construction is not gated by a smart constructor: the plan's own sketches build this as a
/// plain struct literal, and this module is the only consumer, so [`CredentialBroker::store`]
/// is where the charset and bound described below are actually enforced.
#[derive(Clone, Debug)]
pub struct SecretReference {
    /// 1..=64 bytes of `[a-z0-9_.-]`.
    pub id: String,
    /// Same charset and bound as `id`. A free-text provider tag (`"anthropic"`, `"openai"`, …),
    /// not validated against any closed provider vocabulary — that belongs to the BYOK adapters.
    pub provider: String,
    /// The route ids permitted to [`CredentialBroker::lease`] this credential. Must be non-empty;
    /// each entry follows `core/gateway`'s own route id rule (1..=64 bytes of `[a-z0-9_]`).
    pub usable_by: Vec<String>,
}

/// Non-secret summary returned by [`CredentialBroker::list`]: identity and status only, never a
/// value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SecretReferenceSummary {
    pub id: String,
    pub provider: String,
    pub usable_by: Vec<String>,
    pub revoked: bool,
}

/// Every way the broker can refuse an operation.
///
/// `Display` and the derived `Debug` name reference ids, route ids, storage operations, and fixed
/// rule descriptions only. No variant carries a credential value or a passphrase — both are
/// consumed as [`SecretBytes`] and never copied into an error, so there is nothing for these
/// impls to redact.
#[derive(Debug)]
pub enum BrokerError {
    /// [`SecretReference::id`], `::provider`, or an entry of `::usable_by` violated its charset
    /// or bound.
    InvalidReference { field: &'static str },
    /// The sealed key provider could not be created, opened, or used.
    KeyProvider(KeyError),
    /// A filesystem operation on `broker_dir` or its index file failed.
    Storage { operation: &'static str },
    /// The index file exists but is not a well-formed store: invalid JSON, a persisted entry
    /// whose fields do not reconstruct into valid Evidence types, or an entry whose access-control
    /// MAC does not authenticate (BLOCKER 2) — the last case fails closed exactly like a tampered
    /// AEAD ciphertext, and reports only that the entry is corrupt, never which field changed.
    Corrupt { detail: &'static str },
    /// No credential with this id is known to the broker.
    NotFound { id: String },
    /// The credential has been revoked and can no longer be leased.
    Revoked { id: String },
    /// A rotation attempted to change the provider of an existing reference.
    ProviderMismatch { id: String },
    /// The requesting route is not in the credential's `usable_by` list.
    NotUsableByRoute { id: String, route_id: String },
    /// Sealing or opening the credential's value failed — including AEAD authentication failure
    /// on a tampered store file, which fails closed here rather than returning partial bytes.
    Sealed(EvidenceError),
}

impl std::fmt::Display for BrokerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidReference { field } => {
                write!(formatter, "credential reference has an invalid {field}")
            }
            Self::KeyProvider(_) => write!(formatter, "the sealed keyring could not be used"),
            Self::Storage { operation } => {
                write!(formatter, "the credential store could not {operation}")
            }
            Self::Corrupt { detail } => {
                write!(formatter, "the credential store is corrupt: {detail}")
            }
            Self::NotFound { id } => {
                write!(formatter, "credential '{id}' is not known to the broker")
            }
            Self::Revoked { id } => write!(formatter, "credential '{id}' has been revoked"),
            Self::ProviderMismatch { id } => {
                write!(formatter, "credential '{id}' has a different provider")
            }
            Self::NotUsableByRoute { id, route_id } => write!(
                formatter,
                "credential '{id}' is not usable by route '{route_id}'"
            ),
            Self::Sealed(_) => write!(formatter, "the credential could not be sealed or opened"),
        }
    }
}

impl std::error::Error for BrokerError {}

/// A durable, revocable, route-scoped store of BYOK credentials, sealed at rest with
/// `EvidenceProtector<SealedKeyProvider>`.
pub struct CredentialBroker {
    broker_dir: PathBuf,
    protector: EvidenceProtector<SealedKeyProvider>,
    entries: BTreeMap<String, Entry>,
}

/// In-memory representation of one credential. `id` is always equal to
/// `sealed.reference().evidence_id()` by construction (both come from the same
/// [`SecretReference::id`] at `store` time); it is kept as a plain field so lookups and listing
/// never need to reach into the sealed evidence.
///
/// `mac_key_id`/`mac_tag` authenticate `(id, provider, usable_by, revoked, sealed.reference())` —
/// see [`entry_authentication_bytes`]. They are not secret (an HMAC tag reveals nothing about the
/// key), only integrity-bearing, so they are stored as plain fields like everything else here.
#[derive(Clone)]
struct Entry {
    id: String,
    provider: String,
    usable_by: Vec<String>,
    revoked: bool,
    sealed: SealedEvidence,
    mac_key_id: String,
    mac_tag: Vec<u8>,
}

impl CredentialBroker {
    /// Initializes a fresh broker: creates `broker_dir` if it does not already exist, creates a
    /// new sealed keyring at `keyring_dir` (which must already exist — the same precondition
    /// `SealedKeyProvider::create` itself has), and writes an empty index.
    ///
    /// The initial index write uses `create_new` filesystem semantics (see [`persist_new`]): it
    /// can never silently replace a `credentials.json` that already exists at `broker_dir`, even
    /// if a caller mistakenly points `create` at a broker directory with leftover data and a
    /// fresh, never-before-used `keyring_dir` (PR review IMPORTANT 9).
    pub async fn create(
        broker_dir: &Path,
        keyring_dir: &Path,
        key_id: &str,
        passphrase: SecretBytes,
    ) -> Result<Self, BrokerError> {
        std::fs::create_dir_all(broker_dir).map_err(|_| BrokerError::Storage {
            operation: "create the broker directory",
        })?;
        let provider = SealedKeyProvider::create(keyring_dir, key_id, passphrase)
            .map_err(BrokerError::KeyProvider)?;
        let entries = BTreeMap::new();
        persist_new(broker_dir, &entries)?;
        Ok(Self {
            broker_dir: broker_dir.to_path_buf(),
            protector: EvidenceProtector::new(provider),
            entries,
        })
    }

    /// Reopens a broker previously initialized with [`Self::create`]: opens the existing sealed
    /// keyring and loads the existing index, reconstructing each entry's [`SealedEvidence`] from
    /// its persisted parts via [`SealedEvidence::new`] and verifying every entry's access-control
    /// MAC (BLOCKER 2) — a tampered `revoked`/`usableBy`/`provider` field fails closed here, the
    /// same way a tampered AEAD ciphertext already did.
    pub async fn open(
        broker_dir: &Path,
        keyring_dir: &Path,
        key_id: &str,
        passphrase: SecretBytes,
    ) -> Result<Self, BrokerError> {
        let provider = SealedKeyProvider::open(keyring_dir, key_id, passphrase)
            .map_err(BrokerError::KeyProvider)?;
        let entries = load(broker_dir, &provider).await?;
        Ok(Self {
            broker_dir: broker_dir.to_path_buf(),
            protector: EvidenceProtector::new(provider),
            entries,
        })
    }

    /// Opens the broker at `broker_dir`/`keyring_dir` if a store already exists there, or
    /// initializes one if not — the one decision every caller needs and used to hand-copy
    /// (PR review IMPORTANT 9; see `apps/cli/src/commands/gateway/credential.rs`'s former
    /// `open_or_create_broker`, now deleted in favor of this).
    ///
    /// Two racing first callers cannot both create: [`SealedKeyProvider::create`]'s own
    /// exclusive-locked existence check refuses a second keyring publish over the same
    /// `keyring_dir`, so only one caller's [`Self::create`] ever reaches [`persist_new`]. That
    /// keyring-level exclusion is an accidental byproduct this milestone previously leaned on
    /// directly (a bare `CredentialBroker::create` call would surface the loser's failure as
    /// `BrokerError::KeyProvider(KeyError::Conflict)`); this method removes the need for callers
    /// to hit that path at all in the common sequential case, by checking for an existing index
    /// first and calling [`Self::open`] instead.
    ///
    /// A store that does not exist yet over a keyring that ALREADY holds `key_id` (#1139: the
    /// keyring `graphhelm init` provisions for the Runtime's sealer, which `gateway setup` then
    /// shares with the broker) is opened, not created: `SealedKeyProvider::create` refuses an
    /// existing keyring with `KeyError::Conflict`, so the first `store` against an `init`-made
    /// keyring used to fail with "the sealed keyring could not be used". The empty index is still
    /// written with [`persist_new`]'s `create_new` semantics.
    pub async fn open_or_create(
        broker_dir: &Path,
        keyring_dir: &Path,
        key_id: &str,
        passphrase: SecretBytes,
    ) -> Result<Self, BrokerError> {
        if broker_dir.join(STORE_FILE).is_file() {
            return Self::open(broker_dir, keyring_dir, key_id, passphrase).await;
        }
        // `open` and `create` each consume a passphrase; the copy lives no longer than this call.
        let for_open = passphrase.expose(|bytes| SecretBytes::new(bytes.to_vec()));
        match SealedKeyProvider::open(keyring_dir, key_id, for_open) {
            Ok(provider) => {
                std::fs::create_dir_all(broker_dir).map_err(|_| BrokerError::Storage {
                    operation: "create the broker directory",
                })?;
                let entries = BTreeMap::new();
                persist_new(broker_dir, &entries)?;
                Ok(Self {
                    broker_dir: broker_dir.to_path_buf(),
                    protector: EvidenceProtector::new(provider),
                    entries,
                })
            }
            // THE REASON SURVIVES (#1141 review, LOW). `open` fails for two different reasons:
            // the keyring does not hold this key id yet, or it holds it and the passphrase or the
            // file is wrong. `KeyError` does not separate those two cases in a way this layer may
            // assert, so the fallback stands — but when `create` then reports its OWN conflict
            // (the keyring does hold the key, so the open failure was the real one), the original
            // error is returned instead of the conflict that merely restates the collision.
            Err(open_error) => {
                match Self::create(broker_dir, keyring_dir, key_id, passphrase).await {
                    Ok(broker) => Ok(broker),
                    Err(BrokerError::KeyProvider(KeyError::Conflict)) => {
                        Err(BrokerError::KeyProvider(open_error))
                    }
                    Err(other) => Err(other),
                }
            }
        }
    }

    /// Validates `reference`, seals `value`, and durably persists the result. Storing over an
    /// existing id replaces it (a rotation), resetting `revoked` to `false`.
    ///
    /// Holds the broker directory's exclusive lock across a fresh reload of the on-disk index,
    /// the insert, and the persist (PR review IMPORTANT 7) — so a concurrent `store`/`revoke` from
    /// another `CredentialBroker` handle over the same `broker_dir` can never be lost to a stale
    /// in-memory snapshot.
    pub async fn store(
        &mut self,
        reference: SecretReference,
        value: SecretBytes,
    ) -> Result<(), BrokerError> {
        validate_reference(&reference)?;
        let SecretReference {
            id,
            provider,
            usable_by,
        } = reference;

        let input = EvidenceInput::new(
            id.clone(),
            MEDIA_TYPE,
            Sensitivity::Restricted,
            RETENTION_CLASS,
            value,
        )
        .map_err(BrokerError::Sealed)?;
        let sealed = self
            .protector
            .seal(gateway_scope(), input)
            .await
            .map_err(BrokerError::Sealed)?;
        let mac = compute_entry_mac(
            self.protector.key_provider(),
            &id,
            &provider,
            &usable_by,
            false,
            &sealed,
        )
        .await?;

        let _lock = acquire_broker_lock(&self.broker_dir)?;
        let mut candidate = load(&self.broker_dir, self.protector.key_provider()).await?;
        candidate.insert(
            id.clone(),
            Entry {
                id,
                provider,
                usable_by,
                revoked: false,
                sealed,
                mac_key_id: mac.key_id,
                mac_tag: mac.tag,
            },
        );
        persist(&self.broker_dir, &candidate)?;
        self.entries = candidate;
        Ok(())
    }

    /// Rotates a credential, keeping only those already-authorized routes that reach the SAME
    /// endpoint as the rotation (#1182).
    ///
    /// Unlike a caller-side `list` followed by [`Self::store`], this reloads the current index
    /// after acquiring the broker lock. Two HTTP rotations can therefore add different routes
    /// without one replacing the other's freshly-added scope. Provider changes fail closed based
    /// on that same fresh entry.
    ///
    /// WHICH EXISTING ROUTES SURVIVE. The sealed value is REPLACED, so every route that keeps the
    /// reference will be handed the new key. `provider` is a wire format, not a vendor: two routes
    /// can share `openai` and still point at different vendors, and a plain union once handed a
    /// rotated OpenAI key to a route that transmits to another vendor. So an existing route is kept
    /// only when the caller names it in `endpoint_peers` — the routes it has verified reach the
    /// same endpoint as the ones being rotated. An empty slice keeps nothing: the reference is
    /// scoped to `reference.usable_by` alone, which fails closed for any route left out.
    ///
    /// A REVOKED reference keeps nothing, whatever the peers: revocation withdrew every route,
    /// and rotating the value is not a decision to give any of them back. Only the routes the
    /// caller names in `reference.usable_by` are authorized afterwards.
    pub async fn store_preserving_existing_scope(
        &mut self,
        reference: SecretReference,
        value: SecretBytes,
        endpoint_peers: &[String],
    ) -> Result<SecretReference, BrokerError> {
        validate_reference(&reference)?;
        let SecretReference {
            id,
            provider,
            mut usable_by,
        } = reference;

        let input = EvidenceInput::new(
            id.clone(),
            MEDIA_TYPE,
            Sensitivity::Restricted,
            RETENTION_CLASS,
            value,
        )
        .map_err(BrokerError::Sealed)?;
        let sealed = self
            .protector
            .seal(gateway_scope(), input)
            .await
            .map_err(BrokerError::Sealed)?;

        let _lock = acquire_broker_lock(&self.broker_dir)?;
        let mut candidate = load(&self.broker_dir, self.protector.key_provider()).await?;
        if let Some(existing) = candidate.get(&id) {
            if existing.provider != provider {
                return Err(BrokerError::ProviderMismatch { id });
            }
            if !existing.revoked {
                usable_by.extend(
                    existing
                        .usable_by
                        .iter()
                        .filter(|route| endpoint_peers.contains(route))
                        .cloned(),
                );
            }
            usable_by.sort();
            usable_by.dedup();
        }
        let mac = compute_entry_mac(
            self.protector.key_provider(),
            &id,
            &provider,
            &usable_by,
            false,
            &sealed,
        )
        .await?;
        candidate.insert(
            id.clone(),
            Entry {
                id: id.clone(),
                provider: provider.clone(),
                usable_by: usable_by.clone(),
                revoked: false,
                sealed,
                mac_key_id: mac.key_id,
                mac_tag: mac.tag,
            },
        );
        persist(&self.broker_dir, &candidate)?;
        self.entries = candidate;
        Ok(SecretReference {
            id,
            provider,
            usable_by,
        })
    }

    /// Returns the plaintext value for `id`, provided it exists, is not revoked, and `route_id`
    /// is in its `usable_by` list. The value is derived fresh from sealed storage on every call —
    /// the broker holds no plaintext between leases.
    ///
    /// Re-verifies the entry's access-control MAC before consulting `revoked`/`usable_by`
    /// (BLOCKER 2): those fields are plaintext JSON, and trusting an in-memory copy of them
    /// without re-authentication on every read path would defeat the point of the MAC.
    pub async fn lease(&self, id: &str, route_id: &str) -> Result<SecretBytes, BrokerError> {
        let entry = self
            .entries
            .get(id)
            .ok_or_else(|| BrokerError::NotFound { id: id.to_owned() })?;
        verify_entry_mac(self.protector.key_provider(), entry).await?;
        if entry.revoked {
            return Err(BrokerError::Revoked { id: id.to_owned() });
        }
        if !entry.usable_by.iter().any(|usable| usable == route_id) {
            return Err(BrokerError::NotUsableByRoute {
                id: id.to_owned(),
                route_id: route_id.to_owned(),
            });
        }
        self.protector
            .open(gateway_scope(), &entry.sealed)
            .await
            .map_err(BrokerError::Sealed)
    }

    /// Durably marks `id` as revoked. A revoked credential can never be leased again, including
    /// after a reopen.
    ///
    /// Same lock-reload-persist discipline as [`Self::store`] (IMPORTANT 7); the reload's
    /// [`load`] call re-verifies every entry's MAC (BLOCKER 2) before this method computes the
    /// new one over the flipped `revoked` field.
    pub async fn revoke(&mut self, id: &str) -> Result<(), BrokerError> {
        let _lock = acquire_broker_lock(&self.broker_dir)?;
        let mut candidate = load(&self.broker_dir, self.protector.key_provider()).await?;
        let existing = candidate
            .get(id)
            .cloned()
            .ok_or_else(|| BrokerError::NotFound { id: id.to_owned() })?;
        let mac = compute_entry_mac(
            self.protector.key_provider(),
            &existing.id,
            &existing.provider,
            &existing.usable_by,
            true,
            &existing.sealed,
        )
        .await?;
        if let Some(entry) = candidate.get_mut(id) {
            entry.revoked = true;
            entry.mac_key_id = mac.key_id;
            entry.mac_tag = mac.tag;
        }
        persist(&self.broker_dir, &candidate)?;
        self.entries = candidate;
        Ok(())
    }

    /// Non-secret metadata for every known credential: ids, providers, routes, and revocation
    /// status. Never a value.
    #[must_use]
    pub fn list(&self) -> Vec<SecretReferenceSummary> {
        self.entries
            .values()
            .map(|entry| SecretReferenceSummary {
                id: entry.id.clone(),
                provider: entry.provider.clone(),
                usable_by: entry.usable_by.clone(),
                revoked: entry.revoked,
            })
            .collect()
    }
}

fn gateway_scope() -> RepositoryScope {
    RepositoryScope::new(
        WorkspaceId::parse(SCOPE_WORKSPACE_ID).expect("fixed gateway workspace id is valid"),
        ProjectId::parse(SCOPE_PROJECT_ID).expect("fixed gateway project id is valid"),
        None,
    )
}

fn validate_reference(reference: &SecretReference) -> Result<(), BrokerError> {
    if !valid_token(&reference.id) {
        return Err(BrokerError::InvalidReference { field: "id" });
    }
    if !valid_token(&reference.provider) {
        return Err(BrokerError::InvalidReference { field: "provider" });
    }
    if reference.usable_by.is_empty()
        || !reference
            .usable_by
            .iter()
            .all(|route| valid_route_id(route))
    {
        return Err(BrokerError::InvalidReference { field: "usableBy" });
    }
    Ok(())
}

fn valid_token(value: &str) -> bool {
    (1..=MAX_REFERENCE_TOKEN_LEN).contains(&value.len())
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'.' | b'-')
        })
}

/// Mirrors `core/gateway`'s own route id rule (`[a-z0-9_]`, 1..=64) — `usable_by` entries are
/// route ids, and this module has no manifest dependency to validate against directly.
fn valid_route_id(value: &str) -> bool {
    (1..=MAX_REFERENCE_TOKEN_LEN).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn corrupt(detail: &'static str) -> BrokerError {
    BrokerError::Corrupt { detail }
}

/// Result of [`compute_entry_mac`]: the key id the tag was produced under (needed to reconstruct
/// an [`AuthenticationTag`] for a later `verify` call) and the raw tag bytes.
struct EntryMac {
    key_id: String,
    tag: Vec<u8>,
}

/// Canonical, length-prefixed byte encoding of one entry's access-control facts: `id`, `provider`,
/// the sorted `usable_by` set, `revoked`, and the sealed value's identity (evidence id and both
/// SHA-256 digests) — never the plaintext value itself, and never the wrapped key. Each field is
/// individually length-prefixed (mirrors `core/events/src/retention.rs`'s own
/// `push_field`-based authentication-byte builders), so the encoding is unambiguous regardless of
/// field contents. `usable_by` is sorted before encoding so a route reordering (which changes
/// nothing about access) can never change the tag.
fn entry_authentication_bytes(
    id: &str,
    provider: &str,
    usable_by: &[String],
    revoked: bool,
    sealed: &SealedEvidence,
) -> Vec<u8> {
    let mut bytes = b"graphhelm-gateway-credential-entry-v1".to_vec();
    push_field(&mut bytes, id.as_bytes());
    push_field(&mut bytes, provider.as_bytes());
    let mut sorted_usable_by = usable_by.to_vec();
    sorted_usable_by.sort();
    for route in &sorted_usable_by {
        push_field(&mut bytes, route.as_bytes());
    }
    bytes.push(u8::from(revoked));
    let reference = sealed.reference();
    push_field(&mut bytes, reference.evidence_id().as_str().as_bytes());
    push_field(&mut bytes, reference.content_sha256().as_str().as_bytes());
    push_field(
        &mut bytes,
        reference.ciphertext_sha256().as_str().as_bytes(),
    );
    bytes
}

fn push_field(output: &mut Vec<u8>, value: &[u8]) {
    output.extend_from_slice(
        &u32::try_from(value.len())
            .expect("every field here is bounded well under u32::MAX")
            .to_be_bytes(),
    );
    output.extend_from_slice(value);
}

/// Computes a fresh entry-level access-control MAC via the sealed keyring's own
/// `authenticate` — the exact primitive `adapters/sealed-key-provider/src/journal.rs` uses for
/// its revocation journal, just with a distinct purpose string so the two can never cross-verify.
async fn compute_entry_mac<K: KeyProvider>(
    key_provider: &K,
    id: &str,
    provider: &str,
    usable_by: &[String],
    revoked: bool,
    sealed: &SealedEvidence,
) -> Result<EntryMac, BrokerError> {
    let bytes = entry_authentication_bytes(id, provider, usable_by, revoked, sealed);
    let request =
        AuthenticateRequest::new(ENTRY_MAC_PURPOSE, bytes).map_err(BrokerError::KeyProvider)?;
    let tag = key_provider
        .authenticate(request)
        .await
        .map_err(BrokerError::KeyProvider)?;
    Ok(EntryMac {
        key_id: tag.key_id().to_owned(),
        tag: tag.bytes().to_vec(),
    })
}

/// Re-derives an entry's access-control bytes and verifies its persisted MAC against them,
/// failing closed on any mismatch — a flipped `revoked` flag, a widened `usableBy`, a changed
/// `provider`, or a malformed persisted tag all land here (BLOCKER 2). Never reports which field
/// changed or what value it held.
async fn verify_entry_mac<K: KeyProvider>(
    key_provider: &K,
    entry: &Entry,
) -> Result<(), BrokerError> {
    let bytes = entry_authentication_bytes(
        &entry.id,
        &entry.provider,
        &entry.usable_by,
        entry.revoked,
        &entry.sealed,
    );
    let tag = AuthenticationTag::new(
        entry.mac_key_id.clone(),
        ENTRY_MAC_ALGORITHM,
        entry.mac_tag.clone(),
    )
    .map_err(|_| corrupt("credential entry integrity tag is malformed"))?;
    let request = VerifyAuthenticationRequest::new(ENTRY_MAC_PURPOSE, bytes, tag)
        .map_err(|_| corrupt("credential entry integrity tag is malformed"))?;
    key_provider
        .verify(request)
        .await
        .map_err(|_| corrupt("credential entry failed integrity verification"))
}

/// Acquires (creating if necessary) and locks `broker_dir/broker.lock` exclusively. Held across a
/// full load-modify-persist cycle by [`CredentialBroker::store`]/[`CredentialBroker::revoke`]
/// (PR review IMPORTANT 7) — dropping the returned handle releases the lock.
fn acquire_broker_lock(broker_dir: &Path) -> Result<File, BrokerError> {
    let lock_path = broker_dir.join(LOCK_FILE);
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)
        .map_err(|_| BrokerError::Storage {
            operation: "open the broker lock file",
        })?;
    file.lock_exclusive().map_err(|_| BrokerError::Storage {
        operation: "lock the broker directory",
    })?;
    Ok(file)
}

/// Unique temporary-file name for one persist attempt: the fixed name this module used to write
/// (`credentials.json.tmp`) let two concurrent persists (even under the lock discipline added by
/// IMPORTANT 7, e.g. a slow writer from another process not honoring the lock) collide on the same
/// staging file. `pid` plus a per-process monotonic counter makes every attempt's tmp path unique.
fn unique_tmp_name() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let pid = std::process::id();
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{STORE_FILE}.{pid}.{counter}.tmp")
}

/// Writes `entries` to a uniquely-named temporary file, `fsync`s it, renames it onto
/// [`STORE_FILE`] (atomic on every platform this crate targets), then `fsync`s the containing
/// directory so the rename itself is durable (PR review IMPORTANT 7 — mirrors
/// `adapters/sealed-key-provider/src/journal.rs`'s write/flush/`sync_all`/directory-sync sequence
/// and `core/events/src/local.rs`'s `sync_directory_handle`).
fn persist(broker_dir: &Path, entries: &BTreeMap<String, Entry>) -> Result<(), BrokerError> {
    let bytes = serialize_store(entries)?;
    let tmp_path = broker_dir.join(unique_tmp_name());
    let final_path = broker_dir.join(STORE_FILE);
    write_and_sync(&tmp_path, &bytes)?;
    std::fs::rename(&tmp_path, &final_path).map_err(|_| BrokerError::Storage {
        operation: "rename",
    })?;
    sync_directory(broker_dir)
}

/// Writes the *initial* (necessarily empty) index with `create_new` semantics: it fails rather
/// than overwrite anything already at [`STORE_FILE`] (PR review IMPORTANT 9). Only
/// [`CredentialBroker::create`] calls this — every later write goes through [`persist`], which
/// intentionally does replace the existing index (that is the whole point of `store`/`revoke`).
fn persist_new(broker_dir: &Path, entries: &BTreeMap<String, Entry>) -> Result<(), BrokerError> {
    let bytes = serialize_store(entries)?;
    let final_path = broker_dir.join(STORE_FILE);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&final_path)
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                BrokerError::Storage {
                    operation: "create (an index already exists at this path)",
                }
            } else {
                BrokerError::Storage {
                    operation: "create",
                }
            }
        })?;
    file.write_all(&bytes)
        .map_err(|_| BrokerError::Storage { operation: "write" })?;
    file.sync_all()
        .map_err(|_| BrokerError::Storage { operation: "sync" })?;
    drop(file);
    sync_directory(broker_dir)
}

fn serialize_store(entries: &BTreeMap<String, Entry>) -> Result<Vec<u8>, BrokerError> {
    let persisted = PersistedStore {
        store_version: STORE_VERSION,
        entries: entries.values().map(Entry::to_persisted).collect(),
    };
    serde_json::to_vec_pretty(&persisted)
        .map_err(|_| corrupt("the credential store could not be serialized"))
}

fn write_and_sync(path: &Path, bytes: &[u8]) -> Result<(), BrokerError> {
    let mut file =
        std::fs::File::create(path).map_err(|_| BrokerError::Storage { operation: "write" })?;
    file.write_all(bytes)
        .map_err(|_| BrokerError::Storage { operation: "write" })?;
    file.sync_all()
        .map_err(|_| BrokerError::Storage { operation: "sync" })
}

/// `fsync`s the directory itself so a completed rename cannot be lost to a crash before the
/// directory entry update reaches disk. Directory handles cannot be opened as a plain file for
/// `sync_all` on every Windows filesystem/driver combination; that specific failure is treated as
/// a best-effort no-op there, mirroring `core/events/src/local.rs::sync_directory_handle`'s own
/// documented limitation for exactly this call (an honest, not a silent, gap).
#[cfg(windows)]
fn sync_directory(dir: &Path) -> Result<(), BrokerError> {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    let handle = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(dir);
    let handle = match handle {
        Ok(handle) => handle,
        Err(_) => return Ok(()),
    };
    match handle.sync_all() {
        Ok(()) => Ok(()),
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::InvalidInput | std::io::ErrorKind::PermissionDenied
            ) =>
        {
            Ok(())
        }
        Err(_) => Err(BrokerError::Storage {
            operation: "sync the broker directory",
        }),
    }
}

#[cfg(unix)]
fn sync_directory(dir: &Path) -> Result<(), BrokerError> {
    let handle = std::fs::File::open(dir).map_err(|_| BrokerError::Storage {
        operation: "open the broker directory",
    })?;
    handle.sync_all().map_err(|_| BrokerError::Storage {
        operation: "sync the broker directory",
    })
}

/// Loads the on-disk index and reconstructs every entry, verifying each one's access-control MAC
/// before it is trusted (BLOCKER 2) — a mismatch fails the whole load closed with
/// [`BrokerError::Corrupt`], never partially trusting the entries that did verify.
async fn load<K: KeyProvider>(
    broker_dir: &Path,
    key_provider: &K,
) -> Result<BTreeMap<String, Entry>, BrokerError> {
    let path = broker_dir.join(STORE_FILE);
    let bytes = std::fs::read(&path).map_err(|_| BrokerError::Storage { operation: "read" })?;
    let persisted: PersistedStore = serde_json::from_slice(&bytes)
        .map_err(|_| corrupt("the credential store is not valid JSON"))?;
    let mut entries = BTreeMap::new();
    for raw in persisted.entries {
        let entry = Entry::from_persisted(raw)?;
        verify_entry_mac(key_provider, &entry).await?;
        entries.insert(entry.id.clone(), entry);
    }
    Ok(entries)
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedStore {
    store_version: u32,
    entries: Vec<PersistedEntry>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedEntry {
    id: String,
    provider: String,
    usable_by: Vec<String>,
    revoked: bool,
    evidence: PersistedEvidence,
    entry_mac: PersistedEntryMac,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedEntryMac {
    key_id: String,
    algorithm: String,
    tag: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedEvidence {
    evidence_id: String,
    content_sha256: String,
    ciphertext_sha256: String,
    workspace_id: String,
    project_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    execution_id: Option<String>,
    media_type: String,
    sensitivity: Sensitivity,
    retention_class: String,
    algorithm: String,
    nonce: String,
    ciphertext: String,
    wrapped_key: PersistedWrappedKey,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedWrappedKey {
    key_id: String,
    handle: String,
    algorithm: String,
    nonce: String,
    ciphertext: String,
    aad_sha256: String,
}

impl Entry {
    fn to_persisted(&self) -> PersistedEntry {
        let sealed = &self.sealed;
        let wrapped = sealed.wrapped_key();
        PersistedEntry {
            id: self.id.clone(),
            provider: self.provider.clone(),
            usable_by: self.usable_by.clone(),
            revoked: self.revoked,
            evidence: PersistedEvidence {
                evidence_id: sealed.reference().evidence_id().as_str().to_owned(),
                content_sha256: sealed.reference().content_sha256().as_str().to_owned(),
                ciphertext_sha256: sealed.reference().ciphertext_sha256().as_str().to_owned(),
                workspace_id: sealed.scope().workspace_id().as_str().to_owned(),
                project_id: sealed.scope().project_id().as_str().to_owned(),
                execution_id: sealed
                    .scope()
                    .execution_id()
                    .map(|id| id.as_str().to_owned()),
                media_type: sealed.media_type().as_str().to_owned(),
                sensitivity: sealed.sensitivity(),
                retention_class: sealed.retention_class().to_owned(),
                algorithm: sealed.algorithm().to_owned(),
                nonce: hex::encode(sealed.nonce()),
                ciphertext: hex::encode(sealed.ciphertext()),
                wrapped_key: PersistedWrappedKey {
                    key_id: wrapped.key_id().to_owned(),
                    handle: wrapped.handle().to_owned(),
                    algorithm: wrapped.algorithm().to_owned(),
                    nonce: hex::encode(wrapped.nonce()),
                    ciphertext: hex::encode(wrapped.ciphertext()),
                    aad_sha256: wrapped.aad_sha256().as_str().to_owned(),
                },
            },
            entry_mac: PersistedEntryMac {
                key_id: self.mac_key_id.clone(),
                algorithm: ENTRY_MAC_ALGORITHM.to_owned(),
                tag: hex::encode(&self.mac_tag),
            },
        }
    }

    fn from_persisted(raw: PersistedEntry) -> Result<Self, BrokerError> {
        if raw.id != raw.evidence.evidence_id {
            return Err(corrupt("id does not match the sealed evidence id"));
        }

        let evidence_id = EvidenceId::parse(raw.evidence.evidence_id)
            .map_err(|_| corrupt("evidence id is invalid"))?;
        let content_sha256 = RawSha256::parse(raw.evidence.content_sha256)
            .map_err(|_| corrupt("content sha256 is invalid"))?;
        let ciphertext_sha256 = RawSha256::parse(raw.evidence.ciphertext_sha256)
            .map_err(|_| corrupt("ciphertext sha256 is invalid"))?;
        let reference = EvidenceReference::new(evidence_id, content_sha256, ciphertext_sha256);

        let workspace_id = WorkspaceId::parse(raw.evidence.workspace_id)
            .map_err(|_| corrupt("workspace id is invalid"))?;
        let project_id = ProjectId::parse(raw.evidence.project_id)
            .map_err(|_| corrupt("project id is invalid"))?;
        let execution_id = raw
            .evidence
            .execution_id
            .map(ExecutionId::parse)
            .transpose()
            .map_err(|_| corrupt("execution id is invalid"))?;
        let scope = RepositoryScope::new(workspace_id, project_id, execution_id);

        let nonce =
            hex::decode(&raw.evidence.nonce).map_err(|_| corrupt("nonce is not valid hex"))?;
        let ciphertext = hex::decode(&raw.evidence.ciphertext)
            .map_err(|_| corrupt("ciphertext is not valid hex"))?;
        let wrapped_nonce = hex::decode(&raw.evidence.wrapped_key.nonce)
            .map_err(|_| corrupt("wrapped key nonce is not valid hex"))?;
        let wrapped_ciphertext = hex::decode(&raw.evidence.wrapped_key.ciphertext)
            .map_err(|_| corrupt("wrapped key ciphertext is not valid hex"))?;
        let wrapped_aad_sha256 = RawSha256::parse(raw.evidence.wrapped_key.aad_sha256)
            .map_err(|_| corrupt("wrapped key aad sha256 is invalid"))?;
        let wrapped_key = WrappedKey::new(
            raw.evidence.wrapped_key.key_id,
            raw.evidence.wrapped_key.handle,
            &raw.evidence.wrapped_key.algorithm,
            wrapped_nonce,
            wrapped_ciphertext,
            wrapped_aad_sha256,
        )
        .map_err(|_| corrupt("wrapped key is not well-formed"))?;

        let sealed = SealedEvidence::new(
            reference,
            scope,
            raw.evidence.media_type,
            raw.evidence.sensitivity,
            &raw.evidence.retention_class,
            &raw.evidence.algorithm,
            nonce,
            ciphertext,
            wrapped_key,
        )
        .map_err(|_| corrupt("sealed evidence is not well-formed"))?;

        let mac_tag = hex::decode(&raw.entry_mac.tag)
            .map_err(|_| corrupt("entry mac tag is not valid hex"))?;

        Ok(Self {
            id: raw.id,
            provider: raw.provider,
            usable_by: raw.usable_by,
            revoked: raw.revoked,
            sealed,
            mac_key_id: raw.entry_mac.key_id,
            mac_tag,
        })
    }
}
