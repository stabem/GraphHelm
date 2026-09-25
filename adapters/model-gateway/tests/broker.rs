//! Durability, lease scoping, and redaction contract for [`CredentialBroker`].
//!
//! Every test builds the broker over a throwaway keyring directory (a fresh `TempDir`, matching
//! `adapters/sealed-key-provider/tests/sealed_provider.rs`) with a fixed test passphrase. There is
//! no Tokio dependency here, on purpose: like `core/events/tests/evidence_crypto.rs`, `block_on`
//! is a small hand-rolled thread-parking executor, because the broker (like `EvidenceProtector`)
//! exposes plain async fns rather than requiring a real runtime.

use std::{
    fs,
    future::Future,
    path::Path,
    pin::Pin,
    sync::{Arc, Barrier},
    task::{Context, Poll, Wake, Waker},
    thread,
};

use graphhelm_events::KeyError;
use graphhelm_events::SecretBytes;
use graphhelm_model_gateway::broker::{BrokerError, CredentialBroker, SecretReference};

const SENTINEL: &str = "sk-ant-SENTINEL-0123456789abcdef";
const PASSPHRASE_SEED: u8 = 0x42;

fn block_on<F: Future>(future: F) -> F::Output {
    struct ThreadWaker(thread::Thread);
    impl Wake for ThreadWaker {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }

    let waker = Waker::from(Arc::new(ThreadWaker(thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        match Pin::as_mut(&mut future).poll(&mut context) {
            Poll::Ready(output) => return output,
            Poll::Pending => thread::park(),
        }
    }
}

/// A fresh 32-byte passphrase. `SecretBytes` is not `Clone` (by design), so every call site that
/// needs one constructs its own from this fixed test seed.
fn passphrase() -> SecretBytes {
    SecretBytes::new(
        (0..32)
            .map(|offset| PASSPHRASE_SEED.wrapping_add(offset))
            .collect(),
    )
}

fn sentinel_bytes() -> SecretBytes {
    SecretBytes::new(SENTINEL.as_bytes().to_vec())
}

/// `SecretBytes` deliberately does not implement `Debug` (so an accidental `{:?}` can never print
/// a credential value), which means `Result<SecretBytes, BrokerError>::unwrap_err()` does not
/// compile. This is the same generic `Ok`-panics helper `core/events/tests` uses for the same
/// reason (see `evidence_error`/`key_error` there).
fn lease_error<T>(result: Result<T, BrokerError>) -> BrokerError {
    match result {
        Ok(_) => panic!("expected lease to fail"),
        Err(error) => error,
    }
}

fn anthropic_reference() -> SecretReference {
    SecretReference {
        id: "secret_anthropic_primary".to_owned(),
        provider: "anthropic".to_owned(),
        usable_by: vec!["anthropic_byok".to_owned()],
    }
}

async fn create_broker(broker_dir: &Path, keyring_dir: &Path) -> CredentialBroker {
    CredentialBroker::create(
        broker_dir,
        keyring_dir,
        "gateway-credentials-v1",
        passphrase(),
    )
    .await
    .unwrap()
}

async fn open_broker(broker_dir: &Path, keyring_dir: &Path) -> CredentialBroker {
    CredentialBroker::open(
        broker_dir,
        keyring_dir,
        "gateway-credentials-v1",
        passphrase(),
    )
    .await
    .unwrap()
}

/// Flips one bit of the persisted ciphertext for `id` directly on disk, imitating bit-rot or a
/// tampered store file. The surrounding JSON, hex encoding, and lengths all stay valid — only the
/// authenticated content changes, so this can only be caught by AEAD verification, never by shape
/// checks alone.
fn tamper_ciphertext_byte(store_path: &Path, id: &str) {
    let text = fs::read_to_string(store_path).unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
    let entries = value
        .get_mut("entries")
        .and_then(serde_json::Value::as_array_mut)
        .expect("credentials.json has an entries array");
    let entry = entries
        .iter_mut()
        .find(|entry| entry.get("id").and_then(serde_json::Value::as_str) == Some(id))
        .expect("the target credential is present");
    let ciphertext_hex = entry["evidence"]["ciphertext"]
        .as_str()
        .expect("evidence.ciphertext is a hex string")
        .to_owned();
    let mut bytes = hex::decode(&ciphertext_hex).expect("evidence.ciphertext is valid hex");
    bytes[0] ^= 0xFF;
    entry["evidence"]["ciphertext"] = serde_json::Value::String(hex::encode(bytes));
    fs::write(store_path, serde_json::to_string(&value).unwrap()).unwrap();
}

#[test]
fn store_lease_roundtrip_is_durable_across_reopen() {
    let broker_dir = tempfile::TempDir::new().unwrap();
    let keyring_dir = tempfile::TempDir::new().unwrap();

    block_on(async {
        let mut broker = create_broker(broker_dir.path(), keyring_dir.path()).await;
        broker
            .store(anthropic_reference(), sentinel_bytes())
            .await
            .unwrap();
    });

    // Drop and reopen over the same directories: nothing but what was durably persisted survives.
    let leased = block_on(async {
        let broker = open_broker(broker_dir.path(), keyring_dir.path()).await;
        broker
            .lease("secret_anthropic_primary", "anthropic_byok")
            .await
            .unwrap()
    });

    assert_eq!(leased.expose(<[u8]>::to_vec), SENTINEL.as_bytes());
}

#[test]
fn lease_is_scoped_to_usable_by_routes() {
    let broker_dir = tempfile::TempDir::new().unwrap();
    let keyring_dir = tempfile::TempDir::new().unwrap();

    block_on(async {
        let mut broker = create_broker(broker_dir.path(), keyring_dir.path()).await;
        broker
            .store(anthropic_reference(), sentinel_bytes())
            .await
            .unwrap();

        let error = lease_error(
            broker
                .lease("secret_anthropic_primary", "openai_byok")
                .await,
        );

        match &error {
            BrokerError::NotUsableByRoute { id, route_id } => {
                assert_eq!(id, "secret_anthropic_primary");
                assert_eq!(route_id, "openai_byok");
            }
            other => panic!("expected NotUsableByRoute, got {other:?}"),
        }
        let display = format!("{error}");
        let debug = format!("{error:?}");
        assert!(display.contains("openai_byok"), "display was: {display}");
        assert!(!display.contains(SENTINEL));
        assert!(!debug.contains(SENTINEL));

        // The route that IS authorized still works: scoping excludes, it does not corrupt.
        let leased = broker
            .lease("secret_anthropic_primary", "anthropic_byok")
            .await
            .unwrap();
        assert_eq!(leased.expose(<[u8]>::to_vec), SENTINEL.as_bytes());
    });
}

#[test]
fn a_revoked_credential_cannot_be_leased_and_survives_reopen() {
    let broker_dir = tempfile::TempDir::new().unwrap();
    let keyring_dir = tempfile::TempDir::new().unwrap();

    block_on(async {
        let mut broker = create_broker(broker_dir.path(), keyring_dir.path()).await;
        broker
            .store(anthropic_reference(), sentinel_bytes())
            .await
            .unwrap();
        broker.revoke("secret_anthropic_primary").await.unwrap();

        let error = lease_error(
            broker
                .lease("secret_anthropic_primary", "anthropic_byok")
                .await,
        );
        assert!(matches!(error, BrokerError::Revoked { .. }));
    });

    block_on(async {
        let broker = open_broker(broker_dir.path(), keyring_dir.path()).await;
        let error = lease_error(
            broker
                .lease("secret_anthropic_primary", "anthropic_byok")
                .await,
        );
        assert!(matches!(error, BrokerError::Revoked { .. }));

        let summaries = broker.list();
        assert_eq!(summaries.len(), 1);
        assert!(summaries[0].revoked);
    });
}

#[test]
fn broker_errors_and_listings_never_carry_the_value() {
    let broker_dir = tempfile::TempDir::new().unwrap();
    let keyring_dir = tempfile::TempDir::new().unwrap();
    let passphrase_hex = hex::encode(
        (0..32)
            .map(|offset: u8| PASSPHRASE_SEED.wrapping_add(offset))
            .collect::<Vec<_>>(),
    );

    let mut rendered = Vec::new();

    block_on(async {
        let mut broker = create_broker(broker_dir.path(), keyring_dir.path()).await;

        let revocable = SecretReference {
            id: "secret_revocable".to_owned(),
            provider: "anthropic".to_owned(),
            usable_by: vec!["anthropic_byok".to_owned()],
        };
        let tamperable = SecretReference {
            id: "secret_tamperable".to_owned(),
            provider: "anthropic".to_owned(),
            usable_by: vec!["anthropic_byok".to_owned()],
        };
        broker.store(revocable, sentinel_bytes()).await.unwrap();
        broker.store(tamperable, sentinel_bytes()).await.unwrap();

        // Missing id.
        let missing = lease_error(broker.lease("does_not_exist", "anthropic_byok").await);
        rendered.push(format!("{missing}"));
        rendered.push(format!("{missing:?}"));

        // Wrong route.
        let wrong_route = lease_error(broker.lease("secret_revocable", "openai_byok").await);
        rendered.push(format!("{wrong_route}"));
        rendered.push(format!("{wrong_route:?}"));

        // Revoked.
        broker.revoke("secret_revocable").await.unwrap();
        let revoked = lease_error(broker.lease("secret_revocable", "anthropic_byok").await);
        rendered.push(format!("{revoked}"));
        rendered.push(format!("{revoked:?}"));

        // Listing metadata only: ids and providers, never values.
        for summary in broker.list() {
            rendered.push(format!("{summary:?}"));
        }
    });

    // Tampered store file, exercised across a reopen so the corrupted bytes are actually read
    // back from disk rather than compared in memory.
    tamper_ciphertext_byte(
        &broker_dir.path().join("credentials.json"),
        "secret_tamperable",
    );
    block_on(async {
        let broker = open_broker(broker_dir.path(), keyring_dir.path()).await;
        let tampered = lease_error(broker.lease("secret_tamperable", "anthropic_byok").await);
        rendered.push(format!("{tampered}"));
        rendered.push(format!("{tampered:?}"));
    });

    for text in &rendered {
        assert!(
            !text.contains(SENTINEL),
            "leaked the credential value in: {text}"
        );
        assert!(
            !text.contains(&passphrase_hex),
            "leaked the passphrase in: {text}"
        );
    }
}

/// Flips `entries[id].revoked` directly on disk from `true` to `false` — the exact bypass
/// BLOCKER 2 closes: `revoked`/`usableBy`/`provider` are plaintext JSON, so anyone with write
/// access to `broker_dir` could otherwise un-revoke a credential without ever touching the
/// AEAD-protected value.
fn tamper_revoked_flag(store_path: &Path, id: &str, revoked: bool) {
    let text = fs::read_to_string(store_path).unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
    let entries = value
        .get_mut("entries")
        .and_then(serde_json::Value::as_array_mut)
        .expect("credentials.json has an entries array");
    let entry = entries
        .iter_mut()
        .find(|entry| entry.get("id").and_then(serde_json::Value::as_str) == Some(id))
        .expect("the target credential is present");
    entry["revoked"] = serde_json::Value::Bool(revoked);
    fs::write(store_path, serde_json::to_string(&value).unwrap()).unwrap();
}

/// Appends `extra_route` to `entries[id].usableBy` directly on disk — the other BLOCKER 2 bypass
/// shape: widening which routes may lease a credential without ever touching the sealed value.
fn widen_usable_by(store_path: &Path, id: &str, extra_route: &str) {
    let text = fs::read_to_string(store_path).unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
    let entries = value
        .get_mut("entries")
        .and_then(serde_json::Value::as_array_mut)
        .expect("credentials.json has an entries array");
    let entry = entries
        .iter_mut()
        .find(|entry| entry.get("id").and_then(serde_json::Value::as_str) == Some(id))
        .expect("the target credential is present");
    entry["usableBy"]
        .as_array_mut()
        .expect("usableBy is an array")
        .push(serde_json::Value::String(extra_route.to_owned()));
    fs::write(store_path, serde_json::to_string(&value).unwrap()).unwrap();
}

/// Asserts `result` is `Err(BrokerError::Corrupt { .. })` — the fail-closed outcome for an
/// access-control MAC mismatch (BLOCKER 2). `CredentialBroker` carries no `Debug` impl (like
/// `SecretBytes`, deliberately — see `lease_error`'s own doc comment), so this takes a closure
/// producing the `Result` rather than the value itself, keeping the non-`Debug` `Ok` case out of
/// any `unwrap`-family call.
fn assert_corrupt<T>(result: Result<T, BrokerError>, context: &str) {
    match result {
        Err(BrokerError::Corrupt { .. }) => {}
        Err(other) => panic!("{context}: expected Corrupt, got {other:?}"),
        Ok(_) => panic!("{context}: expected Corrupt, got Ok — the tamper was not detected"),
    }
}

#[test]
fn tampering_the_revoked_flag_on_disk_fails_closed() {
    // BLOCKER 2, bypass shape (a): flipping `revoked` true -> false on disk must be refused as
    // corruption when the store is reopened, not silently accepted. Pre-fix, `revoked` was read
    // straight off disk with no integrity check at all, so this tamper would have succeeded and
    // `lease` would have gone on to return the plaintext value.
    let broker_dir = tempfile::TempDir::new().unwrap();
    let keyring_dir = tempfile::TempDir::new().unwrap();

    block_on(async {
        let mut broker = create_broker(broker_dir.path(), keyring_dir.path()).await;
        broker
            .store(anthropic_reference(), sentinel_bytes())
            .await
            .unwrap();
        broker.revoke("secret_anthropic_primary").await.unwrap();
    });

    tamper_revoked_flag(
        &broker_dir.path().join("credentials.json"),
        "secret_anthropic_primary",
        false,
    );

    block_on(async {
        let opened = CredentialBroker::open(
            broker_dir.path(),
            keyring_dir.path(),
            "gateway-credentials-v1",
            passphrase(),
        )
        .await;
        assert_corrupt(opened, "reopening after the revoked flag was tampered");
    });
}

#[test]
fn widening_usable_by_on_disk_fails_closed() {
    // BLOCKER 2, bypass shape (b): appending a route to `usableBy` on disk must be refused as
    // corruption on reopen, for the same reason.
    let broker_dir = tempfile::TempDir::new().unwrap();
    let keyring_dir = tempfile::TempDir::new().unwrap();

    block_on(async {
        let mut broker = create_broker(broker_dir.path(), keyring_dir.path()).await;
        broker
            .store(anthropic_reference(), sentinel_bytes())
            .await
            .unwrap();
    });

    widen_usable_by(
        &broker_dir.path().join("credentials.json"),
        "secret_anthropic_primary",
        "openai_byok",
    );

    block_on(async {
        let opened = CredentialBroker::open(
            broker_dir.path(),
            keyring_dir.path(),
            "gateway-credentials-v1",
            passphrase(),
        )
        .await;
        assert_corrupt(opened, "reopening after usableBy was widened");
    });
}

/// BLOCKER 2 (c): legitimate store/revoke/reopen round-trips must stay green with the
/// access-control MAC in place — the fix must not make ordinary use fail closed too.
#[test]
fn legitimate_store_revoke_reopen_round_trips_still_succeed() {
    let broker_dir = tempfile::TempDir::new().unwrap();
    let keyring_dir = tempfile::TempDir::new().unwrap();

    block_on(async {
        let mut broker = create_broker(broker_dir.path(), keyring_dir.path()).await;
        broker
            .store(anthropic_reference(), sentinel_bytes())
            .await
            .unwrap();
        broker
            .lease("secret_anthropic_primary", "anthropic_byok")
            .await
            .unwrap();
        broker.revoke("secret_anthropic_primary").await.unwrap();
    });

    block_on(async {
        let broker = open_broker(broker_dir.path(), keyring_dir.path()).await;
        let error = lease_error(
            broker
                .lease("secret_anthropic_primary", "anthropic_byok")
                .await,
        );
        assert!(matches!(error, BrokerError::Revoked { .. }));
    });
}

#[test]
fn a_tampered_store_file_fails_closed() {
    let broker_dir = tempfile::TempDir::new().unwrap();
    let keyring_dir = tempfile::TempDir::new().unwrap();

    block_on(async {
        let mut broker = create_broker(broker_dir.path(), keyring_dir.path()).await;
        broker
            .store(anthropic_reference(), sentinel_bytes())
            .await
            .unwrap();
    });

    tamper_ciphertext_byte(
        &broker_dir.path().join("credentials.json"),
        "secret_anthropic_primary",
    );

    block_on(async {
        let broker = open_broker(broker_dir.path(), keyring_dir.path()).await;
        let error = lease_error(
            broker
                .lease("secret_anthropic_primary", "anthropic_byok")
                .await,
        );
        // Authentication failure, never a partial-bytes success: `lease` returned `Err`, so there
        // is no `SecretBytes` to have been partially constructed at all.
        assert!(
            matches!(error, BrokerError::Sealed(_)),
            "expected a sealed-evidence authentication failure, got {error:?}"
        );
    });
}

/// IMPORTANT 7: two `CredentialBroker` handles opened separately over the same `broker_dir`
/// (imitating two threads/processes, not two calls on one handle) must not lose a concurrent
/// update to a lost-write race. One handle stores a second, unrelated credential while the other
/// revokes the first; run in both orders, and in both orders the revocation must survive — it
/// must never be "resurrected" by the other handle's store persisting a snapshot of `entries` that
/// predates the revoke.
#[test]
fn concurrent_store_and_revoke_from_two_broker_handles_never_resurrects_a_revocation() {
    for revoke_first in [false, true] {
        let broker_dir = tempfile::TempDir::new().unwrap();
        let keyring_dir = tempfile::TempDir::new().unwrap();

        block_on(async {
            let mut broker = create_broker(broker_dir.path(), keyring_dir.path()).await;
            broker
                .store(anthropic_reference(), sentinel_bytes())
                .await
                .unwrap();
        });

        let broker_dir_a = broker_dir.path().to_path_buf();
        let keyring_dir_a = keyring_dir.path().to_path_buf();
        let broker_dir_b = broker_dir.path().to_path_buf();
        let keyring_dir_b = keyring_dir.path().to_path_buf();

        let revoke_task = thread::spawn(move || {
            block_on(async {
                let mut broker = open_broker(&broker_dir_a, &keyring_dir_a).await;
                broker.revoke("secret_anthropic_primary").await.unwrap();
            });
        });
        let store_task = thread::spawn(move || {
            block_on(async {
                let mut broker = open_broker(&broker_dir_b, &keyring_dir_b).await;
                broker
                    .store(
                        SecretReference {
                            id: "secret_openai_primary".to_owned(),
                            provider: "openai".to_owned(),
                            usable_by: vec!["openai_byok".to_owned()],
                        },
                        sentinel_bytes(),
                    )
                    .await
                    .unwrap();
            });
        });

        if revoke_first {
            revoke_task.join().unwrap();
            store_task.join().unwrap();
        } else {
            store_task.join().unwrap();
            revoke_task.join().unwrap();
        }

        block_on(async {
            let broker = open_broker(broker_dir.path(), keyring_dir.path()).await;
            let error = lease_error(
                broker
                    .lease("secret_anthropic_primary", "anthropic_byok")
                    .await,
            );
            assert!(
                matches!(error, BrokerError::Revoked { .. }),
                "revoke_first={revoke_first}: the revocation must survive the concurrent store, \
                 got {error:?}"
            );
            let leased = broker
                .lease("secret_openai_primary", "openai_byok")
                .await
                .unwrap();
            assert_eq!(leased.expose(<[u8]>::to_vec), SENTINEL.as_bytes());
        });
    }
}

#[test]
fn preserving_scope_serializes_concurrent_rotations_and_checks_fresh_provider() {
    let broker_dir = tempfile::TempDir::new().unwrap();
    let keyring_dir = tempfile::TempDir::new().unwrap();

    block_on(async {
        let mut broker = create_broker(broker_dir.path(), keyring_dir.path()).await;
        broker
            .store(
                SecretReference {
                    id: "shared_scope".to_owned(),
                    provider: "anthropic".to_owned(),
                    usable_by: vec!["route_a".to_owned()],
                },
                sentinel_bytes(),
            )
            .await
            .unwrap();
    });

    let barrier = Arc::new(Barrier::new(2));
    let broker_dir_b = broker_dir.path().to_path_buf();
    let keyring_dir_b = keyring_dir.path().to_path_buf();
    let barrier_b = Arc::clone(&barrier);
    let rotate_b = thread::spawn(move || {
        block_on(async {
            let mut broker = open_broker(&broker_dir_b, &keyring_dir_b).await;
            barrier_b.wait();
            broker
                .store_preserving_existing_scope(
                    SecretReference {
                        id: "shared_scope".to_owned(),
                        provider: "anthropic".to_owned(),
                        usable_by: vec!["route_b".to_owned()],
                    },
                    sentinel_bytes(),
                    &[
                        "route_a".to_owned(),
                        "route_b".to_owned(),
                        "route_c".to_owned(),
                    ],
                )
                .await
                .unwrap();
        });
    });
    let broker_dir_c = broker_dir.path().to_path_buf();
    let keyring_dir_c = keyring_dir.path().to_path_buf();
    let barrier_c = Arc::clone(&barrier);
    let rotate_c = thread::spawn(move || {
        block_on(async {
            let mut broker = open_broker(&broker_dir_c, &keyring_dir_c).await;
            barrier_c.wait();
            broker
                .store_preserving_existing_scope(
                    SecretReference {
                        id: "shared_scope".to_owned(),
                        provider: "anthropic".to_owned(),
                        usable_by: vec!["route_c".to_owned()],
                    },
                    sentinel_bytes(),
                    &[
                        "route_a".to_owned(),
                        "route_b".to_owned(),
                        "route_c".to_owned(),
                    ],
                )
                .await
                .unwrap();
        });
    });
    rotate_b.join().unwrap();
    rotate_c.join().unwrap();

    block_on(async {
        let broker = open_broker(broker_dir.path(), keyring_dir.path()).await;
        assert_eq!(
            broker.list()[0].usable_by,
            vec!["route_a", "route_b", "route_c"]
        );
    });

    let stale_dir = broker_dir.path().to_path_buf();
    let stale_keyring = keyring_dir.path().to_path_buf();
    block_on(async {
        let mut stale = open_broker(&stale_dir, &stale_keyring).await;
        let mut fresh = open_broker(&stale_dir, &stale_keyring).await;
        fresh
            .store(
                SecretReference {
                    id: "shared_scope".to_owned(),
                    provider: "openai".to_owned(),
                    usable_by: vec!["route_a".to_owned()],
                },
                sentinel_bytes(),
            )
            .await
            .unwrap();
        let error = stale
            .store_preserving_existing_scope(
                SecretReference {
                    id: "shared_scope".to_owned(),
                    provider: "anthropic".to_owned(),
                    usable_by: vec!["route_d".to_owned()],
                },
                sentinel_bytes(),
                &[],
            )
            .await
            .unwrap_err();
        assert!(matches!(error, BrokerError::ProviderMismatch { .. }));
    });
}

const ROTATED: &str = "sk-ROTATED-fedcba9876543210";

fn rotated_bytes() -> SecretBytes {
    SecretBytes::new(ROTATED.as_bytes().to_vec())
}

fn shared_reference(usable_by: &[&str]) -> SecretReference {
    SecretReference {
        id: "shared_scope".to_owned(),
        provider: "openai".to_owned(),
        usable_by: usable_by.iter().map(|route| (*route).to_owned()).collect(),
    }
}

/// #1182: `provider` is a wire format, not a vendor. Two routes can share `openai` and one
/// reference while pointing at two vendors. Rotating the value from one of them must not hand
/// the new key to the other unless the caller vouched that it reaches the same endpoint; the
/// plain union this replaced did exactly that.
#[test]
fn a_rotation_keeps_only_the_existing_routes_named_as_endpoint_peers() {
    let broker_dir = tempfile::TempDir::new().unwrap();
    let keyring_dir = tempfile::TempDir::new().unwrap();

    block_on(async {
        let mut broker = create_broker(broker_dir.path(), keyring_dir.path()).await;
        broker
            .store(
                shared_reference(&["openai_route", "other_vendor"]),
                sentinel_bytes(),
            )
            .await
            .unwrap();

        let rotated = broker
            .store_preserving_existing_scope(
                shared_reference(&["openai_route"]),
                rotated_bytes(),
                &["openai_route".to_owned()],
            )
            .await
            .unwrap();
        assert_eq!(rotated.usable_by, vec!["openai_route"]);

        let error = lease_error(broker.lease("shared_scope", "other_vendor").await);
        assert!(
            matches!(error, BrokerError::NotUsableByRoute { .. }),
            "the route on another endpoint must lose the reference, got {error:?}"
        );
        let leased = broker.lease("shared_scope", "openai_route").await.unwrap();
        assert_eq!(leased.expose(<[u8]>::to_vec), ROTATED.as_bytes());
    });
}

/// #1182, the control for the cell above: a route the caller DOES name as a peer keeps the
/// reference, and it leases the rotated value rather than the old one.
#[test]
fn a_rotation_keeps_an_endpoint_peer_and_hands_it_the_rotated_value() {
    let broker_dir = tempfile::TempDir::new().unwrap();
    let keyring_dir = tempfile::TempDir::new().unwrap();

    block_on(async {
        let mut broker = create_broker(broker_dir.path(), keyring_dir.path()).await;
        broker
            .store(shared_reference(&["route_a", "route_b"]), sentinel_bytes())
            .await
            .unwrap();

        let rotated = broker
            .store_preserving_existing_scope(
                shared_reference(&["route_a"]),
                rotated_bytes(),
                &["route_a".to_owned(), "route_b".to_owned()],
            )
            .await
            .unwrap();
        assert_eq!(rotated.usable_by, vec!["route_a", "route_b"]);
        let leased = broker.lease("shared_scope", "route_b").await.unwrap();
        assert_eq!(leased.expose(<[u8]>::to_vec), ROTATED.as_bytes());
    });
}

/// #1182: revocation withdrew every route. Rotating the value re-authorizes exactly the routes
/// the caller names — even ones it lists as endpoint peers are not given back — and the
/// reference is live again for those alone.
#[test]
fn rotating_a_revoked_reference_authorizes_only_the_named_routes() {
    let broker_dir = tempfile::TempDir::new().unwrap();
    let keyring_dir = tempfile::TempDir::new().unwrap();

    block_on(async {
        let mut broker = create_broker(broker_dir.path(), keyring_dir.path()).await;
        broker
            .store(shared_reference(&["route_a", "route_b"]), sentinel_bytes())
            .await
            .unwrap();
        broker.revoke("shared_scope").await.unwrap();

        let rotated = broker
            .store_preserving_existing_scope(
                shared_reference(&["route_a"]),
                rotated_bytes(),
                &["route_a".to_owned(), "route_b".to_owned()],
            )
            .await
            .unwrap();
        assert_eq!(rotated.usable_by, vec!["route_a"]);

        let error = lease_error(broker.lease("shared_scope", "route_b").await);
        assert!(
            matches!(error, BrokerError::NotUsableByRoute { .. }),
            "a revoked route must not come back through a rotation, got {error:?}"
        );
        let leased = broker.lease("shared_scope", "route_a").await.unwrap();
        assert_eq!(leased.expose(<[u8]>::to_vec), ROTATED.as_bytes());
    });
}

/// #1139: a keyring that already holds the key (made by `graphhelm init` for the Runtime's
/// sealer, or by `gateway keyring init`) and NO store yet. `open_or_create` must open that
/// keyring and start an empty index over it; before the fix it reached `create`, which refused
/// the existing keyring with `KeyError::Conflict`. The credential then round-trips through a
/// plain `open`, proving the index was really written under that keyring.
#[test]
fn open_or_create_starts_a_store_over_a_keyring_that_already_holds_the_key() {
    let broker_dir = tempfile::TempDir::new().unwrap();
    let keyring_dir = tempfile::TempDir::new().unwrap();
    graphhelm_sealed_key_provider::SealedKeyProvider::create(
        keyring_dir.path(),
        "gateway-credentials-v1",
        passphrase(),
    )
    .unwrap();

    block_on(async {
        let mut broker = CredentialBroker::open_or_create(
            broker_dir.path(),
            keyring_dir.path(),
            "gateway-credentials-v1",
            passphrase(),
        )
        .await
        .unwrap();
        broker
            .store(anthropic_reference(), sentinel_bytes())
            .await
            .unwrap();
    });

    block_on(async {
        let broker = open_broker(broker_dir.path(), keyring_dir.path()).await;
        let leased = broker
            .lease("secret_anthropic_primary", "anthropic_byok")
            .await
            .unwrap();
        leased.expose(|bytes| assert_eq!(bytes, SENTINEL.as_bytes()));
    });
}

/// IMPORTANT 9: `CredentialBroker::open_or_create` opens an existing store rather than
/// re-creating it, so calling `credential set` twice against the same directories both succeeds
/// and preserves the first entry.
#[test]
fn open_or_create_opens_an_existing_store_and_preserves_prior_entries() {
    let broker_dir = tempfile::TempDir::new().unwrap();
    let keyring_dir = tempfile::TempDir::new().unwrap();

    block_on(async {
        let mut broker = CredentialBroker::open_or_create(
            broker_dir.path(),
            keyring_dir.path(),
            "gateway-credentials-v1",
            passphrase(),
        )
        .await
        .unwrap();
        broker
            .store(anthropic_reference(), sentinel_bytes())
            .await
            .unwrap();
    });

    block_on(async {
        let mut broker = CredentialBroker::open_or_create(
            broker_dir.path(),
            keyring_dir.path(),
            "gateway-credentials-v1",
            passphrase(),
        )
        .await
        .unwrap();
        broker
            .store(
                SecretReference {
                    id: "secret_openai_primary".to_owned(),
                    provider: "openai".to_owned(),
                    usable_by: vec!["openai_byok".to_owned()],
                },
                sentinel_bytes(),
            )
            .await
            .unwrap();

        let summaries = broker.list();
        assert_eq!(
            summaries.len(),
            2,
            "the first entry must survive: {summaries:?}"
        );
        assert!(
            summaries
                .iter()
                .any(|summary| summary.id == "secret_anthropic_primary")
        );
    });
}

/// IMPORTANT 9: `CredentialBroker::create` must never persist an empty index over a
/// `credentials.json` that already exists at `broker_dir` — even when called against a fresh,
/// never-before-used `keyring_dir`, which is the one case `SealedKeyProvider::create`'s own
/// exclusive-locked existence check cannot catch (that check only prevents two creates over the
/// SAME keyring directory). Pre-fix, `persist` always wrote via write-tmp-then-rename, which
/// unconditionally overwrites whatever was already at the final path — this is the reachable case
/// the review's "if the `KeyError::Conflict` wedge fires first" caveat does not cover, since a
/// second, different keyring directory never trips that wedge at all.
#[test]
fn create_never_overwrites_an_existing_index_even_with_a_fresh_keyring_dir() {
    let broker_dir = tempfile::TempDir::new().unwrap();
    let keyring_dir_one = tempfile::TempDir::new().unwrap();
    let keyring_dir_two = tempfile::TempDir::new().unwrap();

    block_on(async {
        let mut broker = create_broker(broker_dir.path(), keyring_dir_one.path()).await;
        broker
            .store(anthropic_reference(), sentinel_bytes())
            .await
            .unwrap();
    });

    block_on(async {
        let second_create = CredentialBroker::create(
            broker_dir.path(),
            keyring_dir_two.path(),
            "gateway-credentials-v1-second",
            passphrase(),
        )
        .await;
        assert!(
            second_create.is_err(),
            "a second `create` over an existing index must be refused, not silently truncate it"
        );
    });

    // The first keyring/entry must still be intact and leasable — never wiped by the refused
    // second create.
    block_on(async {
        let broker = open_broker(broker_dir.path(), keyring_dir_one.path()).await;
        let leased = broker
            .lease("secret_anthropic_primary", "anthropic_byok")
            .await
            .unwrap();
        assert_eq!(leased.expose(<[u8]>::to_vec), SENTINEL.as_bytes());
    });
}

/// #1141 re-read: the `open_or_create` fallback's preserved error was unobservable — every
/// `KeyProvider` renders as one string, so nothing could tell the two failures apart and
/// reverting the hunk reddened nothing. It IS observable on the variant, which is what this cell
/// reads: with a store absent and a keyring that holds the key under a DIFFERENT passphrase,
/// `open` fails for a real reason and `create` can only restate the collision as `Conflict`. The
/// error returned must be the open failure, not that restatement.
#[test]
fn a_wrong_passphrase_over_an_existing_keyring_keeps_the_open_failure_not_the_conflict() {
    let broker_dir = tempfile::TempDir::new().unwrap();
    let keyring_dir = tempfile::TempDir::new().unwrap();
    graphhelm_sealed_key_provider::SealedKeyProvider::create(
        keyring_dir.path(),
        "gateway-credentials-v1",
        passphrase(),
    )
    .unwrap();

    let other = SecretBytes::new((0..32).map(|offset| 0xA5_u8.wrapping_add(offset)).collect());
    let error = block_on(async {
        CredentialBroker::open_or_create(
            broker_dir.path(),
            keyring_dir.path(),
            "gateway-credentials-v1",
            other,
        )
        .await
    });
    let Err(error) = error else {
        panic!("a wrong passphrase must not open or create the store");
    };

    match error {
        BrokerError::KeyProvider(KeyError::Conflict) => {
            panic!("the conflict merely restates the collision; the open failure is what happened")
        }
        BrokerError::KeyProvider(_) => {}
        other => panic!("{other:?}"),
    }
}
