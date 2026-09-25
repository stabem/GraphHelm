use std::{
    io::{Cursor, Write},
    path::PathBuf,
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use graphhelm_events::{
    AsyncEventRepository, AuthenticateRequest, AuthenticatedCheckpoint, AuthenticationTag,
    CleanupRequest, EvidenceInput, EvidenceProtector, EvidenceSealer, KeyError, KeyProvider,
    KeyProviderMetadata, PreparedAppend, ProjectionRebuildRequest, ProjectionRebuilder,
    RepositoryFuture, RetentionAuthority, RetentionClock, RetentionPolicy, RetentionRequest,
    RetentionService, RetentionTarget, RevocationReceipt, RevokeKeyRequest, SecretBytes,
    VerifyAuthenticationRequest, WrapKeyRequest, WrappedKey, cleanup_request_digest,
    retention_authority_authentication_bytes, revocation_receipt_authentication_bytes,
};
use graphhelm_postgres_event_store::backup::{
    BackupCodec, BackupCounts, BackupError, BackupManifest, DatabaseProcessProfile,
    DatabaseSemanticIdentity, PinnedTool, PostgresBackupOperator,
};
use graphhelm_protocols::{
    EventHash, EvidenceId, OpaqueId, PersistedTimestamp, RawSha256, SafeCode, SemanticVersion,
    Sensitivity,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::{
    AssertSqlSafe,
    postgres::{PgConnectOptions, PgPoolOptions},
};

mod support;

struct MemoryKeyProvider;
struct ReceiptFailingKeyProvider(MemoryKeyProvider);
#[derive(Clone)]
struct FixedClock(PersistedTimestamp);

struct FailingArchiveWriter;

impl Write for FailingArchiveWriter {
    fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("test-only archive write failure"))
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Err(std::io::Error::other("test-only archive flush failure"))
    }
}

impl RetentionClock for FixedClock {
    fn now(&self) -> PersistedTimestamp {
        self.0.clone()
    }
}

fn authentication_bytes(bytes: &[u8]) -> Vec<u8> {
    Sha256::digest([b"graphhelm-backup-test-key".as_slice(), bytes].concat()).to_vec()
}

#[test]
fn database_process_profile_rejects_option_and_secret_shaped_fields() {
    let absolute = std::env::temp_dir().join("graphhelm-test.pgpass");
    assert_eq!(
        DatabaseProcessProfile::new("--host=foreign", 5432, "postgres", "db", &absolute)
            .unwrap_err(),
        BackupError::InvalidBackup,
    );
    assert_eq!(
        DatabaseProcessProfile::new("127.0.0.1", 5432, "postgres:secret", "db", &absolute)
            .unwrap_err(),
        BackupError::InvalidBackup,
    );
    assert_eq!(
        DatabaseProcessProfile::new("127.0.0.1", 5432, "postgres", "db", "relative.pgpass")
            .unwrap_err(),
        BackupError::InvalidBackup,
    );
}

#[test]
fn database_process_profile_rejects_maintenance_and_template_targets() {
    let absolute = std::env::temp_dir().join("graphhelm-test.pgpass");
    for database in ["postgres", "template0", "template1"] {
        assert_eq!(
            DatabaseProcessProfile::new("127.0.0.1", 5432, "admin", database, &absolute),
            Err(BackupError::InvalidBackup),
        );
    }
}

impl KeyProvider for MemoryKeyProvider {
    fn metadata<'a>(&'a self) -> RepositoryFuture<'a, Result<KeyProviderMetadata, KeyError>> {
        Box::pin(async { KeyProviderMetadata::new("backup-test", "memory", "1", 7) })
    }

    fn wrap<'a>(
        &'a self,
        request: WrapKeyRequest,
    ) -> RepositoryFuture<'a, Result<WrappedKey, KeyError>> {
        Box::pin(async move {
            let (handle, key, aad) = request.into_parts();
            let mut ciphertext = key.consume(|bytes| bytes.to_vec());
            ciphertext.extend_from_slice(&[0_u8; 16]);
            WrappedKey::new(
                "backup-test",
                handle,
                "xchacha20poly1305",
                vec![3_u8; 24],
                ciphertext,
                RawSha256::parse(hex::encode(Sha256::digest(aad))).unwrap(),
            )
        })
    }

    fn unwrap<'a>(
        &'a self,
        wrapped: WrappedKey,
    ) -> RepositoryFuture<'a, Result<SecretBytes, KeyError>> {
        Box::pin(async move {
            let (_, _, _, ciphertext, _) = wrapped.into_parts();
            Ok(SecretBytes::new(ciphertext[..32].to_vec()))
        })
    }

    fn revoke<'a>(
        &'a self,
        request: RevokeKeyRequest,
    ) -> RepositoryFuture<'a, Result<RevocationReceipt, KeyError>> {
        Box::pin(async move {
            let (handle, idempotency_key) = request.into_parts();
            let draft = RevocationReceipt::new(
                handle.clone(),
                idempotency_key.clone(),
                7,
                AuthenticationTag::new("backup-test", "hmac-sha256", vec![0_u8; 32])?,
            )?;
            RevocationReceipt::new(
                handle,
                idempotency_key,
                7,
                AuthenticationTag::new(
                    "backup-test",
                    "hmac-sha256",
                    authentication_bytes(&revocation_receipt_authentication_bytes(&draft)),
                )?,
            )
        })
    }

    fn authenticate<'a>(
        &'a self,
        request: AuthenticateRequest,
    ) -> RepositoryFuture<'a, Result<AuthenticationTag, KeyError>> {
        Box::pin(async move {
            AuthenticationTag::new(
                "backup-test",
                "hmac-sha256",
                authentication_bytes(request.bytes()),
            )
        })
    }

    fn verify<'a>(
        &'a self,
        request: VerifyAuthenticationRequest,
    ) -> RepositoryFuture<'a, Result<(), KeyError>> {
        Box::pin(async move {
            if request.tag().bytes() == authentication_bytes(request.bytes()) {
                Ok(())
            } else {
                Err(KeyError::Integrity)
            }
        })
    }
}

impl KeyProvider for ReceiptFailingKeyProvider {
    fn metadata<'a>(&'a self) -> RepositoryFuture<'a, Result<KeyProviderMetadata, KeyError>> {
        self.0.metadata()
    }

    fn wrap<'a>(
        &'a self,
        request: WrapKeyRequest,
    ) -> RepositoryFuture<'a, Result<WrappedKey, KeyError>> {
        self.0.wrap(request)
    }

    fn unwrap<'a>(
        &'a self,
        wrapped: WrappedKey,
    ) -> RepositoryFuture<'a, Result<SecretBytes, KeyError>> {
        self.0.unwrap(wrapped)
    }

    fn revoke<'a>(
        &'a self,
        request: RevokeKeyRequest,
    ) -> RepositoryFuture<'a, Result<RevocationReceipt, KeyError>> {
        self.0.revoke(request)
    }

    fn authenticate<'a>(
        &'a self,
        request: AuthenticateRequest,
    ) -> RepositoryFuture<'a, Result<AuthenticationTag, KeyError>> {
        if request.purpose() == "graphhelm.restore.receipt.v1" {
            Box::pin(async { Err(KeyError::Unavailable) })
        } else {
            self.0.authenticate(request)
        }
    }

    fn verify<'a>(
        &'a self,
        request: VerifyAuthenticationRequest,
    ) -> RepositoryFuture<'a, Result<(), KeyError>> {
        self.0.verify(request)
    }
}

#[test]
fn configured_tools_require_an_absolute_path_digest_and_exact_version() {
    let digest = "11".repeat(32);

    assert_eq!(
        PinnedTool::new(PathBuf::from("pg_dump"), digest.clone(), "16.4").unwrap_err(),
        BackupError::InvalidBackup,
    );
    assert_eq!(
        PinnedTool::new(PathBuf::from(r"C:\PostgreSQL\pg_dump.exe"), "11", "16.4").unwrap_err(),
        BackupError::InvalidBackup,
    );
    assert_eq!(
        PinnedTool::new(PathBuf::from(r"C:\PostgreSQL\pg_dump.exe"), digest, "").unwrap_err(),
        BackupError::InvalidBackup,
    );
    assert_eq!(
        PinnedTool::new(
            PathBuf::from(r"C:\PostgreSQL\pg_dump.exe"),
            "AA".repeat(32),
            "16.4",
        )
        .unwrap_err(),
        BackupError::InvalidBackup,
    );
    assert_eq!(BackupError::InvalidBackup.code(), "GHB001_BACKUP_INVALID");
    assert_eq!(format!("{:?}", BackupError::InvalidBackup), "InvalidBackup");
}

#[test]
fn configured_tool_identity_is_verified_before_use() {
    let executable = std::env::current_exe().unwrap();
    assert_eq!(
        PinnedTool::new(executable, "00".repeat(32), "test-only")
            .unwrap()
            .verify_identity(Duration::from_secs(5))
            .unwrap_err(),
        BackupError::InvalidBackup,
    );
}

#[test]
fn unavailable_diagnostic_preserves_a_safe_stable_stage_code() {
    const CHILD: &str = "GRAPHHELM_TEST_UNAVAILABLE_STAGE_CHILD";
    if std::env::var_os(CHILD).is_some() {
        support::runtime().block_on(async {
            let error = BackupCodec::new(Arc::new(MemoryKeyProvider))
                .encrypt(&manifest(), Cursor::new(b"secret"), FailingArchiveWriter)
                .await
                .unwrap_err();
            assert_eq!(error, BackupError::Unavailable);
        });
        return;
    }

    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "unavailable_diagnostic_preserves_a_safe_stable_stage_code",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .output()
        .unwrap();
    assert!(output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("[graphhelm-backup] unavailable=archive.write"),
        "missing stable unavailable stage: {stderr:?}"
    );
    assert!(!stderr.contains("test-only archive write failure"));
    assert!(!stderr.contains(std::env::current_dir().unwrap().to_string_lossy().as_ref()));
}

#[derive(Debug, PartialEq, Eq)]
enum FailedRestoreState {
    CleanRelease,
    Preserved { replacement_owner: String },
}

#[derive(Debug, PartialEq, Eq)]
enum TerminationObservation {
    Retry,
    Satisfied,
}

fn classify_termination_observation(
    observed_owned_pid: i32,
    terminated: bool,
    observed_pid_owned: Option<bool>,
    remaining_owned_pid: Option<i32>,
) -> Result<TerminationObservation, &'static str> {
    if terminated {
        return Ok(TerminationObservation::Satisfied);
    }
    match observed_pid_owned {
        Some(true) => Ok(TerminationObservation::Retry),
        Some(false) => Err("observed restore PID changed ownership before termination"),
        None => match remaining_owned_pid {
            None => Ok(TerminationObservation::Satisfied),
            Some(pid) if pid == observed_owned_pid => {
                Err("observed restore PID was reused before absence was proven")
            }
            Some(_) => Err("marker-owned restore PID changed before termination"),
        },
    }
}

/// #880: what the restore terminator does on a tick where no marker-owned backend is visible.
///
/// The terminator used to poll on a fixed tick budget and panic at its end. A restore that finished
/// before the first poll never shows a marker-owned backend, so the terminator kept polling a
/// database nobody was restoring and panicked "not observed" about 30 s later, although nothing was
/// wrong. The restore's own completion is the signal that ends the wait: once it is set and the
/// backend is absent, the outcome is a typed `NotObserved`, which asserts nothing about a kill.
/// The tick budget stays as a backstop only while the restore is still running.
#[derive(Debug, PartialEq, Eq)]
enum UnobservedTick {
    KeepPolling,
    NotObserved,
}

fn classify_unobserved_tick(
    restore_finished: bool,
    budget_exhausted: bool,
) -> Result<UnobservedTick, &'static str> {
    if restore_finished {
        return Ok(UnobservedTick::NotObserved);
    }
    if budget_exhausted {
        return Err("restore process was not observed after its marker");
    }
    Ok(UnobservedTick::KeepPolling)
}

fn classify_failed_restore_state(
    closed: bool,
    marker: Option<&str>,
    restore_roles: &[String],
    target_objects: i64,
) -> Result<FailedRestoreState, String> {
    if !closed && marker.is_none() && restore_roles.is_empty() && target_objects == 0 {
        return Ok(FailedRestoreState::CleanRelease);
    }
    if !closed {
        return Err("target was not closed".to_owned());
    }
    let marker_text = marker.ok_or_else(|| "closed target lost marker".to_owned())?;
    let marker: serde_json::Value =
        serde_json::from_str(marker_text).map_err(|error| format!("invalid marker ({error})"))?;
    if marker["format"] != "graphhelm.restore.marker.v1" {
        return Err("wrong marker format".to_owned());
    }
    let replacement_owner = marker["replacementOwner"]
        .as_str()
        .ok_or_else(|| "marker has no replacement owner".to_owned())?
        .to_owned();
    if restore_roles != [replacement_owner.clone()] {
        return Err("marker owner must be the only restore role".to_owned());
    }
    Ok(FailedRestoreState::Preserved { replacement_owner })
}

#[test]
fn failed_restore_oracle_accepts_atomic_pre_acquisition_release_for_every_archive() {
    assert_eq!(
        classify_failed_restore_state(false, None, &[], 0),
        Ok(FailedRestoreState::CleanRelease),
    );
}

#[test]
fn failed_restore_oracle_rejects_open_target_with_committed_objects() {
    assert!(classify_failed_restore_state(false, None, &[], 1).is_err());
}

#[test]
fn failed_restore_oracle_rejects_partial_release_states() {
    let marker =
        r#"{"format":"graphhelm.restore.marker.v1","replacementOwner":"graphhelm_restore_o_test"}"#;
    for (closed, marker, restore_roles) in [
        (false, Some(marker), Vec::new()),
        (false, None, vec!["graphhelm_restore_o_test".to_owned()]),
        (true, None, Vec::new()),
        (true, Some(marker), Vec::new()),
    ] {
        assert!(classify_failed_restore_state(closed, marker, &restore_roles, 0).is_err());
    }
}

#[test]
fn marker_owned_backend_may_disappear_but_a_mismatched_backend_never_counts_as_success() {
    assert_eq!(
        classify_termination_observation(41, false, None, None),
        Ok(TerminationObservation::Satisfied),
    );
    assert_eq!(
        classify_termination_observation(41, false, Some(true), Some(41)),
        Ok(TerminationObservation::Retry),
    );
    assert_eq!(
        classify_termination_observation(41, false, None, Some(42)),
        Err("marker-owned restore PID changed before termination"),
    );
    assert_eq!(
        classify_termination_observation(41, false, Some(false), None),
        Err("observed restore PID changed ownership before termination"),
    );
}

#[test]
fn the_terminator_waits_on_the_restore_marker_signal_not_a_tick_budget() {
    // A restore that finished before any poll saw its backend is not a failure of the cell.
    assert_eq!(
        classify_unobserved_tick(true, false),
        Ok(UnobservedTick::NotObserved),
        "a finished restore must end the wait with a typed not-observed outcome",
    );
    assert_eq!(
        classify_unobserved_tick(true, true),
        Ok(UnobservedTick::NotObserved),
        "a restore that finished after the tick budget must not reach the exhaustion arm",
    );
    // While the restore still runs, keep polling, and the budget still bounds a hang.
    assert_eq!(
        classify_unobserved_tick(false, false),
        Ok(UnobservedTick::KeepPolling)
    );
    assert_eq!(
        classify_unobserved_tick(false, true),
        Err("restore process was not observed after its marker"),
    );
}

fn manifest() -> BackupManifest {
    BackupManifest::new(
        "22".repeat(32),
        DatabaseSemanticIdentity::new("UTF8", "c", "C", "C", None, None, None).unwrap(),
        3,
        "repository-v1",
        "33".repeat(32),
        vec!["44".repeat(32), "55".repeat(32), "66".repeat(32)],
        BackupCounts {
            stream_heads: 2,
            checkpoints: 1,
            artifact_references: 3,
            evidence: 4,
            tombstones: 1,
            retention_receipts: 2,
            projection_generations: 2,
        },
        "16.4",
        "77".repeat(32),
        "16.4",
        "88".repeat(32),
        "graphhelm_runtime",
        7,
    )
    .unwrap()
}

#[test]
fn encrypted_chunks_round_trip_and_tampering_fails_closed() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let codec = BackupCodec::new(Arc::new(MemoryKeyProvider));
            let plaintext = vec![0x5a; 1024 * 1024 + 17];
            let mut encrypted = Vec::new();
            let receipt = codec
                .encrypt(&manifest(), Cursor::new(&plaintext), &mut encrypted)
                .await
                .unwrap();
            assert_eq!(receipt.chunk_count(), 2);
            assert!(
                !encrypted
                    .windows(64)
                    .any(|window| window == &plaintext[..64])
            );

            let mut restored = Vec::new();
            let verified = codec
                .verify_then_decrypt(Cursor::new(&encrypted), 7, &mut restored)
                .await
                .unwrap();
            assert_eq!(verified.manifest(), &manifest());
            assert_eq!(restored, plaintext);

            for corrupt in [encrypted[..encrypted.len() - 1].to_vec(), {
                let mut value = encrypted.clone();
                let middle = value.len() / 2;
                value[middle] ^= 1;
                value
            }] {
                assert_eq!(
                    codec
                        .verify_then_decrypt(Cursor::new(corrupt), 7, Vec::new())
                        .await
                        .unwrap_err(),
                    BackupError::InvalidBackup,
                );
            }
            let mut extended = encrypted.clone();
            extended.extend_from_slice(b"authenticated-looking-trailer");
            assert_eq!(
                codec
                    .verify_then_decrypt(Cursor::new(extended), 7, Vec::new())
                    .await
                    .unwrap_err(),
                BackupError::InvalidBackup,
            );
            assert_eq!(
                codec
                    .verify_then_decrypt(Cursor::new(&encrypted), 8, Vec::new())
                    .await
                    .unwrap_err(),
                BackupError::InvalidRestore,
            );

            struct SwappingArchive {
                first: Cursor<Vec<u8>>,
                second: Cursor<Vec<u8>>,
                swapped: bool,
            }
            impl std::io::Read for SwappingArchive {
                fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                    if self.swapped {
                        std::io::Read::read(&mut self.second, buffer)
                    } else {
                        std::io::Read::read(&mut self.first, buffer)
                    }
                }
            }
            impl std::io::Seek for SwappingArchive {
                fn seek(&mut self, position: std::io::SeekFrom) -> std::io::Result<u64> {
                    if matches!(position, std::io::SeekFrom::Start(0)) && self.first.position() > 0
                    {
                        self.swapped = true;
                    }
                    if self.swapped {
                        std::io::Seek::seek(&mut self.second, position)
                    } else {
                        std::io::Seek::seek(&mut self.first, position)
                    }
                }
            }
            let mut different = Vec::new();
            codec
                .encrypt(
                    &manifest(),
                    Cursor::new(b"different archive"),
                    &mut different,
                )
                .await
                .unwrap();
            assert_eq!(
                codec
                    .verify_then_decrypt(
                        SwappingArchive {
                            first: Cursor::new(encrypted),
                            second: Cursor::new(different),
                            swapped: false,
                        },
                        7,
                        std::io::sink(),
                    )
                    .await,
                Err(BackupError::InvalidBackup)
            );
        });
}

#[test]
fn encrypted_publication_is_no_replace_and_cleans_its_owned_temporary() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let root = std::env::temp_dir().join(format!(
                "graphhelm-backup-test-{}",
                uuid::Uuid::new_v4().simple()
            ));
            std::fs::create_dir(&root).unwrap();
            // Independent of the caller's umask: Linux publication refuses a group-writable
            // parent, and Ubuntu's default umask 0002 makes one (#1306).
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
            }
            let destination = root.join("backup.ghb");
            std::fs::write(&destination, b"sentinel").unwrap();
            let codec = BackupCodec::new(Arc::new(MemoryKeyProvider));
            assert_eq!(
                codec
                    .encrypt_to_path(&manifest(), Cursor::new(b"secret"), &destination)
                    .await
                    .unwrap_err(),
                BackupError::InvalidBackup,
            );
            assert_eq!(std::fs::read(&destination).unwrap(), b"sentinel");
            assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);

            std::fs::remove_file(&destination).unwrap();
            codec
                .encrypt_to_path(&manifest(), Cursor::new(b"secret"), &destination)
                .await
                .unwrap();
            assert!(destination.exists());
            assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
            std::fs::remove_dir_all(root).unwrap();
        });
}

#[test]
#[ignore = "requires GRAPHHELM_TEST_ADMIN_URL and pinned PostgreSQL client tools"]
fn admin_operator_binds_pool_profile_and_source_identity() {
    support::runtime().block_on(async {
        let database = support::TestDatabase::new().await;
        let source_repository = Arc::new(
            graphhelm_postgres_event_store::PostgresEventStore::from_pool(
                database.runtime_pool.clone(),
                Arc::new(MemoryKeyProvider),
            )
            .await
            .unwrap(),
        );
        let source_scope = support::scope("backup-source");
        let sealed = EvidenceProtector::new(MemoryKeyProvider)
            .seal(
                source_scope.clone(),
                EvidenceInput::new(
                    "backup-evidence",
                    "application/json",
                    Sensitivity::Restricted,
                    "standard",
                    SecretBytes::new(br#"{"safe":"archive evidence"}"#.to_vec()),
                )
                .unwrap(),
            )
            .await
            .unwrap();
        let evidence_id = sealed.reference().evidence_id().as_str().to_owned();
        let evidence_handle = sealed.wrapped_key().handle().to_owned();
        let evidence_digest = sealed.reference().ciphertext_sha256().as_str().to_owned();
        let mut source_event = support::event("backup-event");
        source_event.evidence_refs.push(sealed.reference().clone());
        let source_events = source_repository
            .append_atomic(
                PreparedAppend::new(
                    source_scope.clone(),
                    OpaqueId::parse("backup-stream").unwrap(),
                    1,
                    vec![source_event],
                    vec![sealed],
                    vec![],
                )
                .unwrap(),
            )
            .await
            .unwrap();
        source_repository
            .append_atomic(
                PreparedAppend::new(
                    support::scope("backup-sibling-scope"),
                    OpaqueId::parse("backup-stream").unwrap(),
                    1,
                    vec![support::event("backup-sibling-event")],
                    vec![],
                    vec![],
                )
                .unwrap(),
            )
            .await
            .unwrap();
        let checkpoint_at = chrono::DateTime::parse_from_rfc3339("2026-08-11T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct CheckpointBytes<'a> {
            scope: &'a graphhelm_protocols::RepositoryScope,
            stream_id: &'a str,
            sequence: u64,
            event_hash: &'a str,
            repository_format_version: u32,
            created_at: chrono::DateTime<chrono::Utc>,
            key_version: &'a str,
            provider_epoch: u64,
            active_graph: Option<()>,
        }
        let checkpoint_bytes = support::canonical_bytes(&CheckpointBytes {
            scope: &source_scope,
            stream_id: "backup-stream",
            sequence: 1,
            event_hash: source_events[0].event_hash.as_str(),
            repository_format_version: 1,
            created_at: checkpoint_at,
            key_version: "1",
            provider_epoch: 7,
            active_graph: None,
        });
        let checkpoint_tag = MemoryKeyProvider
            .authenticate(
                AuthenticateRequest::new("graphhelm-checkpoint-v1", checkpoint_bytes).unwrap(),
            )
            .await
            .unwrap();
        source_repository
            .append_checkpoint(AuthenticatedCheckpoint {
                scope: source_scope.clone(),
                stream_id: "backup-stream".to_owned(),
                sequence: 1,
                event_hash: EventHash::parse(source_events[0].event_hash.to_string()).unwrap(),
                repository_format_version: 1,
                created_at: checkpoint_at,
                key_version: "1".to_owned(),
                provider_epoch: 7,
                active_graph: None,
                tag: checkpoint_tag,
            })
            .await
            .unwrap();
        ProjectionRebuilder::new(source_repository.clone(), source_repository.clone())
            .rebuild(
                ProjectionRebuildRequest::new(
                    source_scope.clone(),
                    "backup-stream".to_owned(),
                    "execution-projection".to_owned(),
                    1,
                    1,
                    100,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        let policy = RetentionPolicy::new(
            OpaqueId::parse("backup-retention-policy").unwrap(),
            SemanticVersion::parse("1.0.0").unwrap(),
            "standard",
            0,
            0,
        )
        .unwrap();
        let authority_id = OpaqueId::parse("backup-retention-authority").unwrap();
        let authority_tag = MemoryKeyProvider
            .authenticate(
                AuthenticateRequest::new(
                    "retention-authority",
                    retention_authority_authentication_bytes(
                        &authority_id,
                        &source_scope,
                        &policy,
                    ),
                )
                .unwrap(),
            )
            .await
            .unwrap();
        let retention_request = RetentionRequest::new(
            source_scope.clone(),
            OpaqueId::parse("backup-retention-operation").unwrap(),
            OpaqueId::parse("backup-retention-idempotency").unwrap(),
            policy.clone(),
            RetentionAuthority::new(
                authority_id,
                source_scope.clone(),
                policy.id().clone(),
                policy.version().clone(),
                authority_tag,
            ),
            SafeCode::parse("scheduled_expiry").unwrap(),
            vec![RetentionTarget::new(EvidenceId::parse(&evidence_id).unwrap()).unwrap()],
        )
        .unwrap();
        let retention_service = RetentionService::new(
            source_repository.clone(),
            Arc::new(MemoryKeyProvider),
            Arc::new(FixedClock(
                PersistedTimestamp::from_datetime(chrono::Utc::now()).unwrap(),
            )),
        );
        retention_service.execute(retention_request).await.unwrap();
        retention_service
            .cleanup(
                CleanupRequest::new(
                    source_scope.clone(),
                    OpaqueId::parse("backup-cleanup-operation").unwrap(),
                    OpaqueId::parse("backup-cleanup-idempotency").unwrap(),
                    vec![EvidenceId::parse(&evidence_id).unwrap()],
                    PersistedTimestamp::from_datetime(chrono::Utc::now()).unwrap(),
                )
                .unwrap(),
            )
            .await
            .unwrap();
        let database_name: String = sqlx::query_scalar("SELECT current_database()")
            .fetch_one(database.admin_pool())
            .await
            .unwrap();
        let root = std::env::current_dir().unwrap();
        let dump_path = PathBuf::from(std::env::var("GRAPHHELM_TEST_PG_DUMP").unwrap());
        let restore_path = PathBuf::from(std::env::var("GRAPHHELM_TEST_PG_RESTORE").unwrap());
        let tool = |path: PathBuf| {
            let digest = hex::encode(Sha256::digest(std::fs::read(&path).unwrap()));
            let output = std::process::Command::new(&path)
                .arg("--version")
                .output()
                .unwrap();
            PinnedTool::new(
                path,
                digest,
                String::from_utf8(output.stdout).unwrap().trim(),
            )
            .unwrap()
        };
        let passfile = root.join(format!(
            "target/task10-{}.pgpass",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(passfile.parent().unwrap()).unwrap();
        let url = std::env::var("GRAPHHELM_TEST_ADMIN_URL").unwrap();
        let authority = url.strip_prefix("postgres://").unwrap();
        let (credentials, endpoint) = authority.split_once('@').unwrap();
        let (user, password) = credentials.split_once(':').unwrap();
        let (host_port, _) = endpoint.split_once('/').unwrap();
        let (host, port) = host_port.rsplit_once(':').unwrap();
        let port = port.parse::<u16>().unwrap();
        std::fs::write(
            &passfile,
            format!("{host}:{port}:*:{user}:{password}{}", '\n'),
        )
        .unwrap();
        let profile =
            DatabaseProcessProfile::new(host, port, user, database_name, &passfile).unwrap();
        let operator = PostgresBackupOperator::new(
            database.admin_pool().clone(),
            Arc::new(MemoryKeyProvider),
            profile,
            tool(dump_path),
            tool(restore_path),
            Duration::from_secs(30),
        )
        .await
        .unwrap();
        let identity = operator.source_identity_sha256().await.unwrap();
        assert_eq!(identity.len(), 64);
        assert!(identity.bytes().all(|byte| byte.is_ascii_hexdigit()));
        let destination = root.join(format!(
            "target/task10-{}.ghb",
            uuid::Uuid::new_v4().simple()
        ));
        let receipt = operator.backup_to_path(&destination).await.unwrap();
        assert!(receipt.chunk_count() > 0);
        let original_checkpoint_tag: Vec<u8> =
            sqlx::query_scalar("SELECT tag FROM public.graphhelm_checkpoints LIMIT 1")
                .fetch_one(database.admin_pool())
                .await
                .unwrap();
        let mut checkpoint_tamper = database.admin_pool().begin().await.unwrap();
        sqlx::query("SET LOCAL session_replication_role = replica")
            .execute(&mut *checkpoint_tamper)
            .await
            .unwrap();
        sqlx::query("UPDATE public.graphhelm_checkpoints SET tag=$1")
            .bind(vec![0_u8; 32])
            .execute(&mut *checkpoint_tamper)
            .await
            .unwrap();
        checkpoint_tamper.commit().await.unwrap();
        let corrupt_checkpoint_archive = root.join(format!(
            "target/task10-corrupt-checkpoint-{}.ghb",
            uuid::Uuid::new_v4().simple()
        ));
        operator
            .backup_to_path(&corrupt_checkpoint_archive)
            .await
            .unwrap();
        let mut checkpoint_restore = database.admin_pool().begin().await.unwrap();
        sqlx::query("SET LOCAL session_replication_role = replica")
            .execute(&mut *checkpoint_restore)
            .await
            .unwrap();
        sqlx::query("UPDATE public.graphhelm_checkpoints SET tag=$1")
            .bind(original_checkpoint_tag)
            .execute(&mut *checkpoint_restore)
            .await
            .unwrap();
        checkpoint_restore.commit().await.unwrap();
        sqlx::query(
            "ALTER POLICY graphhelm_scope ON public.graphhelm_events \
             USING (true) WITH CHECK (true)",
        )
        .execute(database.admin_pool())
        .await
        .unwrap();
        let drift_destination = root.join(format!(
            "target/task10-drift-{}.ghb",
            uuid::Uuid::new_v4().simple()
        ));
        let drift_result = operator.backup_to_path(&drift_destination).await;
        // Restores the policy exactly as migration 0004 defines it, NULLIF included. Restoring the
        // pre-0004 form here would leave the schema contract permanently drifted for the rest of
        // this test, so every later backup would fail for a reason unrelated to what is asserted.
        sqlx::query(
            "ALTER POLICY graphhelm_scope ON public.graphhelm_events \
             USING (workspace_id=NULLIF(current_setting('graphhelm.workspace_id',true),'') \
                    AND project_id=NULLIF(current_setting('graphhelm.project_id',true),'') \
                    AND execution_id=COALESCE(current_setting('graphhelm.execution_id',true),'')) \
             WITH CHECK (workspace_id=NULLIF(current_setting('graphhelm.workspace_id',true),'') \
                    AND project_id=NULLIF(current_setting('graphhelm.project_id',true),'') \
                    AND execution_id=COALESCE(current_setting('graphhelm.execution_id',true),''))",
        )
        .execute(database.admin_pool())
        .await
        .unwrap();
        if drift_destination.exists() {
            std::fs::remove_file(&drift_destination).unwrap();
        }
        assert_eq!(drift_result, Err(BackupError::InvalidBackup));
        sqlx::query("CREATE PUBLICATION backup_forbidden_object")
            .execute(database.admin_pool())
            .await
            .unwrap();
        let extra_object_destination = root.join(format!(
            "target/task10-extra-object-{}.ghb",
            uuid::Uuid::new_v4().simple()
        ));
        let extra_object_result = operator.backup_to_path(&extra_object_destination).await;
        sqlx::query("DROP PUBLICATION backup_forbidden_object")
            .execute(database.admin_pool())
            .await
            .unwrap();
        assert_eq!(extra_object_result, Err(BackupError::InvalidBackup));
        assert!(!extra_object_destination.exists());
        sqlx::query("CREATE VIEW public.graphhelm_evil AS SELECT 1 AS value")
            .execute(database.admin_pool())
            .await
            .unwrap();
        let reserved_object_destination = root.join(format!(
            "target/task10-reserved-object-{}.ghb",
            uuid::Uuid::new_v4().simple()
        ));
        let reserved_object_result = operator
            .backup_to_path(&reserved_object_destination)
            .await;
        sqlx::query("DROP VIEW public.graphhelm_evil")
            .execute(database.admin_pool())
            .await
            .unwrap();
        assert_eq!(reserved_object_result, Err(BackupError::InvalidBackup));
        assert!(!reserved_object_destination.exists());
        sqlx::query("CREATE ROLE backup_forbidden_grantee NOLOGIN")
            .execute(database.admin_pool())
            .await
            .unwrap();
        sqlx::query(
            "GRANT SELECT ON public.graphhelm_evidence TO backup_forbidden_grantee",
        )
        .execute(database.admin_pool())
        .await
        .unwrap();
        let unsafe_privilege_destination = root.join(format!(
            "target/task10-unsafe-privilege-{}.ghb",
            uuid::Uuid::new_v4().simple()
        ));
        let unsafe_privilege_result = operator
            .backup_to_path(&unsafe_privilege_destination)
            .await;
        sqlx::query(
            "REVOKE SELECT ON public.graphhelm_evidence FROM backup_forbidden_grantee",
        )
        .execute(database.admin_pool())
        .await
        .unwrap();
        sqlx::query("DROP ROLE backup_forbidden_grantee")
            .execute(database.admin_pool())
            .await
            .unwrap();
        assert_eq!(unsafe_privilege_result, Err(BackupError::InvalidBackup));
        assert!(!unsafe_privilege_destination.exists());
        sqlx::query("GRANT CREATE ON SCHEMA public TO PUBLIC")
            .execute(database.admin_pool())
            .await
            .unwrap();
        let inherited_create_destination = root.join(format!(
            "target/task10-inherited-create-{}.ghb",
            uuid::Uuid::new_v4().simple()
        ));
        let inherited_create_result = operator
            .backup_to_path(&inherited_create_destination)
            .await;
        sqlx::query("REVOKE CREATE ON SCHEMA public FROM PUBLIC")
            .execute(database.admin_pool())
            .await
            .unwrap();
        assert_eq!(inherited_create_result, Err(BackupError::InvalidBackup));
        assert!(!inherited_create_destination.exists());
        let original_projection: serde_json::Value = sqlx::query_scalar(
            "SELECT state FROM public.graphhelm_projection_checkpoints \
             ORDER BY last_sequence DESC LIMIT 1",
        )
        .fetch_one(database.admin_pool())
        .await
        .unwrap();
        sqlx::query(
            "ALTER TABLE public.graphhelm_projection_checkpoints \
             DISABLE TRIGGER graphhelm_projection_checkpoints_immutable",
        )
        .execute(database.admin_pool())
        .await
        .unwrap();
        sqlx::query(
            "UPDATE public.graphhelm_projection_checkpoints \
             SET state=jsonb_set(state,'{aggregateBytes}','999'::jsonb)",
        )
        .execute(database.admin_pool())
        .await
        .unwrap();
        sqlx::query(
            "ALTER TABLE public.graphhelm_projection_checkpoints \
             ENABLE TRIGGER graphhelm_projection_checkpoints_immutable",
        )
        .execute(database.admin_pool())
        .await
        .unwrap();
        let cleanup_receipt: (String, String, String, String) = sqlx::query_as(
            "SELECT operation_id,idempotency_key,request_digest,requested_at \
             FROM public.graphhelm_cleanup_receipts WHERE evidence_id=$1",
        )
        .bind(&evidence_id)
        .fetch_one(database.admin_pool())
        .await
        .unwrap();
        sqlx::query(
            "ALTER TABLE public.graphhelm_cleanup_receipts \
             DISABLE TRIGGER graphhelm_cleanup_receipts_immutable",
        )
        .execute(database.admin_pool())
        .await
        .unwrap();
        let forged_requested_wire = "2026-08-12T12:34:56.123456Z";
        let forged_requested_at = PersistedTimestamp::parse(forged_requested_wire).unwrap();
        let forged_idempotency = OpaqueId::parse("forged-cleanup-idempotency").unwrap();
        let forged_request = CleanupRequest::new(
            source_scope.clone(),
            OpaqueId::parse(&cleanup_receipt.0).unwrap(),
            forged_idempotency.clone(),
            vec![EvidenceId::parse(&evidence_id).unwrap()],
            forged_requested_at.clone(),
        )
        .unwrap();
        let forged_digest = cleanup_request_digest(&forged_request);
        sqlx::query(
            "UPDATE public.graphhelm_cleanup_receipts SET \
             idempotency_key=$2,requested_at=$3,request_digest=$4 \
             WHERE evidence_id=$1",
        )
        .bind(&evidence_id)
        .bind(forged_idempotency.as_str())
        .bind(forged_requested_wire)
        .bind(forged_digest.as_str())
        .execute(database.admin_pool())
        .await
        .unwrap();
        sqlx::query(
            "ALTER TABLE public.graphhelm_cleanup_receipts \
             ENABLE TRIGGER graphhelm_cleanup_receipts_immutable",
        )
        .execute(database.admin_pool())
        .await
        .unwrap();
        let corrupt_cleanup_archive = root.join(format!(
            "target/task10-corrupt-cleanup-{}.ghb",
            uuid::Uuid::new_v4().simple()
        ));
        operator
            .backup_to_path(&corrupt_cleanup_archive)
            .await
            .unwrap();
        sqlx::query(
            "ALTER TABLE public.graphhelm_cleanup_receipts \
             DISABLE TRIGGER graphhelm_cleanup_receipts_immutable",
        )
        .execute(database.admin_pool())
        .await
        .unwrap();
        sqlx::query(
            "UPDATE public.graphhelm_cleanup_receipts SET operation_id=$2,idempotency_key=$3,request_digest=$4,requested_at=$5 \
             WHERE evidence_id=$1",
        )
        .bind(&evidence_id)
        .bind(cleanup_receipt.0)
        .bind(cleanup_receipt.1)
        .bind(cleanup_receipt.2)
        .bind(cleanup_receipt.3)
        .execute(database.admin_pool())
        .await
        .unwrap();
        sqlx::query(
            "ALTER TABLE public.graphhelm_cleanup_receipts \
             ENABLE TRIGGER graphhelm_cleanup_receipts_immutable",
        )
        .execute(database.admin_pool())
        .await
        .unwrap();
        let corrupt_projection_archive = root.join(format!(
            "target/task10-corrupt-projection-{}.ghb",
            uuid::Uuid::new_v4().simple()
        ));
        operator
            .backup_to_path(&corrupt_projection_archive)
            .await
            .unwrap();
        sqlx::query(
            "ALTER TABLE public.graphhelm_projection_checkpoints \
             DISABLE TRIGGER graphhelm_projection_checkpoints_immutable",
        )
        .execute(database.admin_pool())
        .await
        .unwrap();
        sqlx::query("UPDATE public.graphhelm_projection_checkpoints SET state=$1")
            .bind(original_projection)
            .execute(database.admin_pool())
            .await
            .unwrap();
        sqlx::query(
            "ALTER TABLE public.graphhelm_projection_checkpoints \
             ENABLE TRIGGER graphhelm_projection_checkpoints_immutable",
        )
        .execute(database.admin_pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO public.graphhelm_retention_policies(\
             workspace_id,project_id,execution_id,policy_id,policy_version,retention_class,minimum_age_seconds,cleanup_delay_seconds) \
             VALUES($1,$2,$3,'orphan-invalid-policy','not-semver','standard',0,0)",
        )
        .bind(source_scope.workspace_id().as_str())
        .bind(source_scope.project_id().as_str())
        .bind(source_scope.execution_id().unwrap().as_str())
        .execute(database.admin_pool())
        .await
        .unwrap();
        let corrupt_orphan_policy_archive = root.join(format!(
            "target/task10-corrupt-orphan-policy-{}.ghb",
            uuid::Uuid::new_v4().simple()
        ));
        operator
            .backup_to_path(&corrupt_orphan_policy_archive)
            .await
            .unwrap();
        let mut orphan_cleanup = database.admin_pool().begin().await.unwrap();
        sqlx::query("SET LOCAL session_replication_role = replica")
            .execute(&mut *orphan_cleanup)
            .await
            .unwrap();
        sqlx::query(
            "DELETE FROM public.graphhelm_retention_policies WHERE policy_id='orphan-invalid-policy'",
        )
        .execute(&mut *orphan_cleanup)
        .await
        .unwrap();
        orphan_cleanup.commit().await.unwrap();
        sqlx::query(
            "INSERT INTO public.graphhelm_retention_policies(\
             workspace_id,project_id,execution_id,policy_id,policy_version,retention_class,minimum_age_seconds,cleanup_delay_seconds) \
             VALUES($1,$2,$3,'forged-policy','1.0.0','standard',0,0)",
        )
        .bind(source_scope.workspace_id().as_str())
        .bind(source_scope.project_id().as_str())
        .bind(source_scope.execution_id().unwrap().as_str())
        .execute(database.admin_pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO public.graphhelm_retention_operations(\
             workspace_id,project_id,execution_id,operation_id,idempotency_key,request_digest,policy_id,policy_version,authority,\
             authority_key_id,authority_algorithm,authority_tag,reason_code,evaluated_at,requested_at,provider_epoch,\
             prepared_key_id,prepared_algorithm,prepared_tag,state) \
             VALUES($1,$2,$3,'forged-operation','forged-idempotency',$4,\
             'forged-policy','1.0.0','forged-authority','backup-test','hmac-sha256',$5,'retention-test',\
             '2026-08-11T12:00:00Z','2026-08-11T12:00:00Z',7,'backup-test','hmac-sha256',$5,'prepared')",
        )
        .bind(source_scope.workspace_id().as_str())
        .bind(source_scope.project_id().as_str())
        .bind(source_scope.execution_id().unwrap().as_str())
        .bind("11".repeat(32))
        .bind(vec![0_u8; 32])
        .execute(database.admin_pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO public.graphhelm_retention_targets(\
             workspace_id,project_id,execution_id,operation_id,ordinal,evidence_id,key_handle_id,ciphertext_sha256,classification,prior_state) \
             VALUES($1,$2,$3,'forged-operation',0,$4,$5,$6,'restricted','available')",
        )
        .bind(source_scope.workspace_id().as_str())
        .bind(source_scope.project_id().as_str())
        .bind(source_scope.execution_id().unwrap().as_str())
        .bind(&evidence_id)
        .bind(&evidence_handle)
        .bind(&evidence_digest)
        .execute(database.admin_pool())
        .await
        .unwrap();
        let corrupt_retention_archive = root.join(format!(
            "target/task10-corrupt-retention-{}.ghb",
            uuid::Uuid::new_v4().simple()
        ));
        operator
            .backup_to_path(&corrupt_retention_archive)
            .await
            .unwrap();
        let mut cleanup = database.admin_pool().begin().await.unwrap();
        sqlx::query("SET LOCAL session_replication_role = replica")
            .execute(&mut *cleanup)
            .await
            .unwrap();
        sqlx::query("DELETE FROM public.graphhelm_retention_targets WHERE operation_id='forged-operation'")
            .execute(&mut *cleanup)
            .await
            .unwrap();
        sqlx::query("DELETE FROM public.graphhelm_retention_operations WHERE operation_id='forged-operation'")
            .execute(&mut *cleanup)
            .await
            .unwrap();
        sqlx::query("DELETE FROM public.graphhelm_retention_policies WHERE policy_id='forged-policy'")
            .execute(&mut *cleanup)
            .await
            .unwrap();
        cleanup.commit().await.unwrap();
        let mut restored_dump = Vec::new();
        let archive = std::fs::File::open(&destination).unwrap();
        let verified = BackupCodec::new(Arc::new(MemoryKeyProvider))
            .verify_then_decrypt(std::io::BufReader::new(archive), 7, &mut restored_dump)
            .await
            .unwrap();
        assert_eq!(verified.manifest().source_identity_sha256(), identity);
        assert!(restored_dump.starts_with(b"PGDMP"));
        assert_eq!(
            operator.restore_from_path(&destination).await.unwrap_err(),
            BackupError::InvalidRestore,
        );
        let poisoned_name = format!(
            "graphhelm_restore_poisoned_{}",
            uuid::Uuid::new_v4().simple()
        );
        let root_options = PgConnectOptions::from_str(&url).unwrap();
        let root_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect_with(root_options.clone())
            .await
            .unwrap();
        sqlx::query(AssertSqlSafe(format!("CREATE DATABASE {poisoned_name}")))
            .execute(&root_pool)
            .await
            .unwrap();
        let poisoned_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect_with(root_options.clone().database(&poisoned_name))
            .await
            .unwrap();
        sqlx::query("CREATE COLLATION public.preexisting_user_object (provider=libc, locale='C')")
            .execute(&poisoned_pool)
            .await
            .unwrap();
        let poisoned_profile =
            DatabaseProcessProfile::new(host, port, user, &poisoned_name, &passfile).unwrap();
        let poisoned_operator = PostgresBackupOperator::new(
            poisoned_pool.clone(),
            Arc::new(MemoryKeyProvider),
            poisoned_profile,
            tool(PathBuf::from(
                std::env::var("GRAPHHELM_TEST_PG_DUMP").unwrap(),
            )),
            tool(PathBuf::from(
                std::env::var("GRAPHHELM_TEST_PG_RESTORE").unwrap(),
            )),
            Duration::from_secs(30),
        )
        .await
        .unwrap();
        let poisoned_result = poisoned_operator.restore_from_path(&destination).await;
        let retained_collation: i64 = sqlx::query_scalar(
            "SELECT count(*)::bigint FROM pg_collation c JOIN pg_namespace n ON n.oid=c.collnamespace \
             WHERE n.nspname='public' AND c.collname='preexisting_user_object'",
        )
        .fetch_one(&poisoned_pool)
        .await
        .unwrap();
        assert_eq!(poisoned_result, Err(BackupError::InvalidRestore));
        assert_eq!(retained_collation, 1);
        poisoned_pool.close().await;
        sqlx::query(AssertSqlSafe(format!("DROP DATABASE {poisoned_name}")))
            .execute(&root_pool)
            .await
            .unwrap();
        let populated_target = support::TestDatabase::new().await;
        populated_target.runtime_pool.close().await;
        let populated_name: String =
            sqlx::query_scalar("SELECT current_database()")
                .fetch_one(populated_target.admin_pool())
                .await
                .unwrap();
        let populated_profile =
            DatabaseProcessProfile::new(host, port, user, &populated_name, &passfile).unwrap();
        let populated_operator = PostgresBackupOperator::new(
            populated_target.admin_pool().clone(),
            Arc::new(MemoryKeyProvider),
            populated_profile,
            tool(PathBuf::from(
                std::env::var("GRAPHHELM_TEST_PG_DUMP").unwrap(),
            )),
            tool(PathBuf::from(
                std::env::var("GRAPHHELM_TEST_PG_RESTORE").unwrap(),
            )),
            Duration::from_secs(30),
        )
        .await
        .unwrap();
        assert_eq!(
            populated_operator.restore_from_path(&destination).await,
            Err(BackupError::InvalidRestore)
        );
        let retained_schema: Option<String> = sqlx::query_scalar(
            "SELECT to_regclass('public.graphhelm_events')::text",
        )
        .fetch_one(populated_target.admin_pool())
        .await
        .unwrap();
        assert_eq!(retained_schema.as_deref(), Some("graphhelm_events"));
        populated_target.cleanup().await;

        // pg_restore is single-transaction: a post-marker process failure may roll back every
        // object. The safety contract is the closed target plus authenticated marker, not a
        // load-sensitive count of whatever partial objects happen to have committed.
        for (failure, failing_archive, terminate_after_marker) in [
            ("terminated restore", &destination, true),
            ("projection", &corrupt_projection_archive, false),
            ("checkpoint", &corrupt_checkpoint_archive, false),
            ("retention", &corrupt_retention_archive, false),
            ("orphan-policy", &corrupt_orphan_policy_archive, false),
            ("cleanup", &corrupt_cleanup_archive, false),
        ] {
            let corrupt_name = format!(
                "graphhelm_restore_corrupt_{}",
                uuid::Uuid::new_v4().simple()
            );
            sqlx::query(AssertSqlSafe(format!("CREATE DATABASE {corrupt_name}")))
                .execute(&root_pool)
                .await
                .unwrap();
            let corrupt_pool = PgPoolOptions::new()
                .max_connections(1)
                .connect_with(root_options.clone().database(&corrupt_name))
                .await
                .unwrap();
            let corrupt_profile =
                DatabaseProcessProfile::new(host, port, user, &corrupt_name, &passfile).unwrap();
            let corrupt_operator = PostgresBackupOperator::new(
                corrupt_pool.clone(),
                Arc::new(MemoryKeyProvider),
                corrupt_profile,
                tool(PathBuf::from(
                    std::env::var("GRAPHHELM_TEST_PG_DUMP").unwrap(),
                )),
                tool(PathBuf::from(
                    std::env::var("GRAPHHELM_TEST_PG_RESTORE").unwrap(),
                )),
                Duration::from_secs(30),
            )
            .await
            .unwrap();
            let restore_finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let restore_terminator = if terminate_after_marker {
                let root_pool = root_pool.clone();
                let restore_finished = Arc::clone(&restore_finished);
                let corrupt_name = corrupt_name.clone();
                Some(tokio::spawn(async move {
                    // #880: TRUE only if `pg_terminate_backend` returned true in some iteration --
                    // that is, if THIS run killed the restore. `Satisfied` is also reached when the
                    // marker-owned backend is simply absent, which is correct for "stop polling"
                    // and says nothing about who ended it: a kill that landed, or a restore that
                    // finished first. Both used to return `()`, so the assertion below asserted the
                    // consequence of a kill in runs where none happened. Six occurrences across
                    // three lanes, always at that assertion and never at this task's own panic.
                    let mut killed = false;
                    let mut ticks = 0_u32;
                    loop {
                        // Read BEFORE the poll: a finished flag paired with an empty poll proves the
                        // backend is gone for good, because the restore returned before we looked.
                        let finished =
                            restore_finished.load(std::sync::atomic::Ordering::SeqCst);
                        let budget_exhausted = ticks >= 3_000;
                        let restore_pid: Option<i32> = sqlx::query_scalar(
                            "SELECT a.pid FROM pg_stat_activity a JOIN pg_database d ON d.oid=a.datid \
                             WHERE d.datname=$1 AND a.application_name LIKE 'graphhelm-restore-%' \
                             AND shobj_description(d.oid,'pg_database') LIKE '%graphhelm.restore.marker.v1%' \
                             LIMIT 1",
                        )
                        .bind(&corrupt_name)
                        .fetch_optional(&root_pool)
                        .await
                        .unwrap()
                        .flatten();
                        if let Some(pid) = restore_pid {
                            let terminated: bool =
                                sqlx::query_scalar("SELECT pg_terminate_backend($1)")
                                    .bind(pid)
                                    .fetch_one(&root_pool)
                                    .await
                                    .unwrap();
                            if terminated {
                                killed = true;
                            }
                            let observed_pid_owned: Option<bool> = if terminated {
                                None
                            } else {
                                sqlx::query_scalar(
                                    "SELECT COALESCE(d.datname=$1 \
                                     AND a.application_name LIKE 'graphhelm-restore-%' \
                                     AND shobj_description(d.oid,'pg_database') LIKE '%graphhelm.restore.marker.v1%',FALSE) \
                                     FROM pg_stat_activity a LEFT JOIN pg_database d ON d.oid=a.datid \
                                     WHERE a.pid=$2",
                                )
                                .bind(&corrupt_name)
                                .bind(pid)
                                .fetch_optional(&root_pool)
                                .await
                                .unwrap()
                            };
                            let remaining_owned_pid = if terminated || observed_pid_owned.is_some() {
                                None
                            } else {
                                sqlx::query_scalar(
                                    "SELECT a.pid FROM pg_stat_activity a JOIN pg_database d ON d.oid=a.datid \
                                     WHERE d.datname=$1 AND a.application_name LIKE 'graphhelm-restore-%' \
                                     AND shobj_description(d.oid,'pg_database') LIKE '%graphhelm.restore.marker.v1%' \
                                     LIMIT 1",
                                )
                                .bind(&corrupt_name)
                                .fetch_optional(&root_pool)
                                .await
                                .unwrap()
                                .flatten()
                            };
                            match classify_termination_observation(
                                pid,
                                terminated,
                                observed_pid_owned,
                                remaining_owned_pid,
                            )
                            .unwrap()
                            {
                                TerminationObservation::Satisfied => return killed,
                                TerminationObservation::Retry => {
                                    assert!(
                                        !budget_exhausted,
                                        "marker-owned restore backend {pid} survived the tick budget"
                                    );
                                }
                            }
                        } else {
                            match classify_unobserved_tick(finished, budget_exhausted).unwrap() {
                                UnobservedTick::NotObserved => return killed,
                                UnobservedTick::KeepPolling => {}
                            }
                        }
                        ticks += 1;
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                }))
            } else {
                None
            };
            let restore_result = corrupt_operator.restore_from_path(failing_archive).await;
            restore_finished.store(true, std::sync::atomic::Ordering::SeqCst);
            let terminator_killed = match restore_terminator {
                Some(restore_terminator) => Some(restore_terminator.await.unwrap()),
                None => None,
            };
            // #880: assert the consequence of a termination only where a termination happened.
            //
            // NOT a widening of the assertion. A restore that IS killed and still reports success
            // fails exactly as before -- that path has `terminated == true`, so `killed` is true and
            // the assertion runs. What no longer fails is the run where the restore finished before
            // the terminator could reach it: the cell did not establish a termination there, so it
            // is not entitled to a verdict about one. The alternative on the table -- accepting both
            // outcomes unconditionally -- would have accepted a killed restore reporting success,
            // which is the one thing this cell exists to catch.
            match terminator_killed {
                Some(false) => eprintln!(
                    "#880: the restore finished before the terminator could kill it, so this run \
                     asserts nothing about its verdict ({failure}); result was {restore_result:?}"
                ),
                _ => assert_eq!(restore_result, Err(BackupError::InvalidRestore), "{failure}"),
            }
            let preserved: (bool, Option<String>) = sqlx::query_as(
                "SELECT NOT datallowconn,shobj_description(oid,'pg_database') \
                 FROM pg_database WHERE datname=$1",
            )
            .bind(&corrupt_name)
            .fetch_one(&root_pool)
            .await
            .unwrap();
            let restore_roles: Vec<String> =
                sqlx::query_scalar("SELECT rolname FROM pg_roles WHERE rolname LIKE 'graphhelm_restore_o_%' ORDER BY rolname")
                    .fetch_all(&root_pool)
                    .await
                    .unwrap();
            let target_objects: i64 = sqlx::query_scalar(
                "SELECT count(*)::bigint FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace \
                 WHERE n.nspname NOT IN ('pg_catalog','information_schema') \
                   AND n.nspname NOT LIKE 'pg_toast%'",
            )
            .fetch_one(&corrupt_pool)
            .await
            .unwrap();
            let state = format!(
                "closed={}, marker={:?}, restore_roles={restore_roles:?}, target_objects={target_objects}",
                preserved.0, preserved.1,
            );
            // Failure may win before acquisition for any archive, not only the deliberately
            // terminated one. Judge the exact persisted state instead of the scenario label.
            //
            // #880, second consequence of the same precondition. `classify_failed_restore_state`'s
            // NAME is its premise: it classifies the state left by a restore that FAILED. Where the
            // restore ran to completion the premise does not hold -- the target is open with its
            // objects committed, so the classifier correctly returns `target was not closed` and the
            // cell panics HERE instead of at the assertion above. Skipping the assertion without
            // skipping this left the race fatal forty lines lower down; ISSUES 3 reproduced it twice
            // at `30d01b25`, the skip line printed first and then `closed=false, marker=None,
            // restore_roles=[], target_objects=39`. Conditioning an assertion on its precondition
            // has to cover every consequence drawn from that precondition, not the first one.
            //
            // `None` rather than a role guessed from `restore_roles`: that query is GLOBAL
            // (`pg_roles LIKE 'graphhelm_restore_o_%'`), so a role seen here cannot be attributed to
            // this iteration, and dropping one another run owns is worse than leaving it. Nothing is
            // taken on trust -- the leak assertion a few lines below re-reads the same global set
            // after cleanup and fails loudly if anything survives, so a completed restore that DOES
            // leave a replacement owner is reported rather than silently tolerated.
            let replacement_owner = match terminator_killed {
                Some(false) => {
                    eprintln!(
                        "#880: not classifying failed-restore state for {failure}: the restore \
                         completed, so there is no failed restore to classify; {state}"
                    );
                    None
                }
                _ => match classify_failed_restore_state(
                    preserved.0,
                    preserved.1.as_deref(),
                    &restore_roles,
                    target_objects,
                )
                .unwrap_or_else(|reason| panic!("{failure}: {reason}; {state}"))
                {
                    FailedRestoreState::CleanRelease => None,
                    FailedRestoreState::Preserved { replacement_owner } => Some(replacement_owner),
                },
            };
            drop(corrupt_operator);
            corrupt_pool.close().await;
            sqlx::query(AssertSqlSafe(format!(
                "DROP DATABASE {corrupt_name} WITH (FORCE)"
            )))
            .execute(&root_pool)
            .await
            .unwrap();
            if let Some(replacement_owner) = replacement_owner {
                sqlx::query(AssertSqlSafe(format!(
                    "DROP ROLE \"{}\"",
                    replacement_owner.replace('"', "\"\"")
                )))
                .execute(&root_pool)
                .await
                .unwrap();
            }
            let leaked_restore_roles: Vec<String> = sqlx::query_scalar(
                "SELECT rolname FROM pg_roles WHERE rolname LIKE 'graphhelm_restore_o_%' ORDER BY rolname",
            )
            .fetch_all(&root_pool)
            .await
            .unwrap();
            assert!(
                leaked_restore_roles.is_empty(),
                "{failure}: cleanup leaked restore roles: {leaked_restore_roles:?}"
            );
        }
        std::fs::remove_file(&corrupt_projection_archive).unwrap();
        std::fs::remove_file(&corrupt_checkpoint_archive).unwrap();
        std::fs::remove_file(&corrupt_retention_archive).unwrap();
        std::fs::remove_file(&corrupt_orphan_policy_archive).unwrap();
        std::fs::remove_file(&corrupt_cleanup_archive).unwrap();

        let failed_name = format!(
            "graphhelm_restore_failed_{}",
            uuid::Uuid::new_v4().simple()
        );
        sqlx::query(AssertSqlSafe(format!("CREATE DATABASE {failed_name}")))
            .execute(&root_pool)
            .await
            .unwrap();
        let failed_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect_with(root_options.clone().database(&failed_name))
            .await
            .unwrap();
        let failed_profile =
            DatabaseProcessProfile::new(host, port, user, &failed_name, &passfile).unwrap();
        let failed_operator = PostgresBackupOperator::new(
            failed_pool.clone(),
            Arc::new(ReceiptFailingKeyProvider(MemoryKeyProvider)),
            failed_profile,
            tool(PathBuf::from(
                std::env::var("GRAPHHELM_TEST_PG_DUMP").unwrap(),
            )),
            tool(PathBuf::from(
                std::env::var("GRAPHHELM_TEST_PG_RESTORE").unwrap(),
            )),
            Duration::from_secs(30),
        )
        .await
        .unwrap();
        assert_eq!(
            failed_operator.restore_from_path(&destination).await,
            Err(BackupError::InvalidRestore)
        );
        let failed_objects: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace \
             WHERE n.nspname='public' AND c.relkind IN ('r','p','v','m','S','f')",
        )
        .fetch_one(&failed_pool)
        .await
        .unwrap();
        let failed_preserved: (bool, String) = sqlx::query_as(
            "SELECT NOT datallowconn,shobj_description(oid,'pg_database') \
             FROM pg_database WHERE datname=$1",
        )
        .bind(&failed_name)
        .fetch_one(&root_pool)
        .await
        .unwrap();
        assert!(failed_objects > 0, "failed restore lost its audit state");
        assert!(failed_preserved.0, "failed restore left a usable target");
        let failed_marker: serde_json::Value =
            serde_json::from_str(&failed_preserved.1).unwrap();
        let failed_owner = failed_marker["replacementOwner"]
            .as_str()
            .unwrap()
            .to_owned();
        drop(failed_operator);
        failed_pool.close().await;
        sqlx::query(AssertSqlSafe(format!(
            "DROP DATABASE {failed_name} WITH (FORCE)"
        )))
            .execute(&root_pool)
            .await
            .unwrap();
        sqlx::query(AssertSqlSafe(format!(
            "DROP ROLE \"{}\"",
            failed_owner.replace('"', "\"\"")
        )))
        .execute(&root_pool)
        .await
        .unwrap();

        let commented_name = format!(
            "graphhelm_restore_commented_{}",
            uuid::Uuid::new_v4().simple()
        );
        sqlx::query(AssertSqlSafe(format!("CREATE DATABASE {commented_name}")))
            .execute(&root_pool)
            .await
            .unwrap();
        sqlx::query(AssertSqlSafe(format!(
            "COMMENT ON DATABASE {commented_name} IS 'operator-owned comment'"
        )))
        .execute(&root_pool)
        .await
        .unwrap();
        let commented_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect_with(root_options.clone().database(&commented_name))
            .await
            .unwrap();
        let commented_profile =
            DatabaseProcessProfile::new(host, port, user, &commented_name, &passfile).unwrap();
        assert_eq!(
            PostgresBackupOperator::new(
                commented_pool.clone(),
                Arc::new(MemoryKeyProvider),
                commented_profile,
                tool(PathBuf::from(std::env::var("GRAPHHELM_TEST_PG_DUMP").unwrap())),
                tool(PathBuf::from(
                    std::env::var("GRAPHHELM_TEST_PG_RESTORE").unwrap()
                )),
                Duration::from_secs(30),
            )
            .await
            .unwrap_err(),
            BackupError::InvalidBackup
        );
        let retained_comment: Option<String> = sqlx::query_scalar(
            "SELECT shobj_description(oid,'pg_database') FROM pg_database WHERE datname=$1",
        )
        .bind(&commented_name)
        .fetch_one(&root_pool)
        .await
        .unwrap();
        assert_eq!(retained_comment.as_deref(), Some("operator-owned comment"));
        commented_pool.close().await;
        sqlx::query(AssertSqlSafe(format!("DROP DATABASE {commented_name}")))
            .execute(&root_pool)
        .await
        .unwrap();

        let divergent_comment_name = format!(
            "graphhelm_divergent_{}",
            uuid::Uuid::new_v4().simple()
        );
        sqlx::query(AssertSqlSafe(format!(
            "CREATE DATABASE {divergent_comment_name}"
        )))
        .execute(&root_pool)
        .await
        .unwrap();
        let divergent_comment = serde_json::json!({
            "format": "graphhelm.restore.marker.v1x",
            "database": divergent_comment_name,
            "targetIdentitySha256": "0".repeat(64),
            "application": "forged",
            "quarantine": "graphhelm_restore_q_forged",
            "replacementOwner": "graphhelm_restore_o_forged",
            "replacementOwnerIdentitySha256": "0".repeat(64),
            "replacementIdentitySha256": null,
            "access": {"owner":"postgres","allowConnections":true,"connectionLimit":-1,
                "semantics":{"encoding":"UTF8","localeProvider":"c","collate":"C","ctype":"C","icuLocale":null,"icuRules":null,"collationVersion":null},"acl":[]},
            "keyId": "forged",
            "algorithm": "hmac-sha256",
            "tagHex": "00"
        })
        .to_string();
        let quoted_divergent_comment = divergent_comment.replace('\'', "''");
        sqlx::query(AssertSqlSafe(format!(
            "COMMENT ON DATABASE {divergent_comment_name} IS '{quoted_divergent_comment}'"
        )))
        .execute(&root_pool)
        .await
        .unwrap();
        let divergent_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect_with(root_options.clone().database(&divergent_comment_name))
            .await
            .unwrap();
        let divergent_profile = DatabaseProcessProfile::new(
            host,
            port,
            user,
            &divergent_comment_name,
            &passfile,
        )
        .unwrap();
        assert_eq!(
            PostgresBackupOperator::new(
                divergent_pool.clone(),
                Arc::new(MemoryKeyProvider),
                divergent_profile,
                tool(PathBuf::from(std::env::var("GRAPHHELM_TEST_PG_DUMP").unwrap())),
                tool(PathBuf::from(
                    std::env::var("GRAPHHELM_TEST_PG_RESTORE").unwrap()
                )),
                Duration::from_secs(30),
            )
            .await
            .unwrap_err(),
            BackupError::InvalidBackup
        );
        let retained_divergent_comment: Option<String> = sqlx::query_scalar(
            "SELECT shobj_description(oid,'pg_database') FROM pg_database WHERE datname=$1",
        )
        .bind(&divergent_comment_name)
        .fetch_one(&root_pool)
        .await
        .unwrap();
        assert_eq!(
            retained_divergent_comment.as_deref(),
            Some(divergent_comment.as_str())
        );
        divergent_pool.close().await;
        sqlx::query(AssertSqlSafe(format!(
            "DROP DATABASE {divergent_comment_name}"
        )))
        .execute(&root_pool)
        .await
        .unwrap();

        let limited_role = format!("graphhelm_limited_admin_{}", uuid::Uuid::new_v4().simple());
        let limited_database = format!("graphhelm_limited_db_{}", uuid::Uuid::new_v4().simple());
        let limited_password = format!("test_{}", uuid::Uuid::new_v4().simple());
        sqlx::query(AssertSqlSafe(format!(
            "CREATE ROLE {limited_role} LOGIN CREATEDB PASSWORD '{limited_password}'"
        )))
        .execute(&root_pool)
        .await
        .unwrap();
        sqlx::query(AssertSqlSafe(format!(
            "CREATE DATABASE {limited_database} OWNER {limited_role}"
        )))
        .execute(&root_pool)
        .await
        .unwrap();
        let limited_options = PgConnectOptions::new()
            .host(host)
            .port(port)
            .username(&limited_role)
            .password(&limited_password)
            .database(&limited_database);
        let limited_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect_with(limited_options)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE public.preexisting_owner_data(value integer)")
            .execute(&limited_pool)
            .await
            .unwrap();
        let limited_passfile = root.join(format!(
            "target/task10-limited-{}.pgpass",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::write(
            &limited_passfile,
            format!("{host}:{port}:*:{limited_role}:{limited_password}{}", '\n'),
        )
        .unwrap();
        let limited_profile = DatabaseProcessProfile::new(
            host,
            port,
            &limited_role,
            &limited_database,
            &limited_passfile,
        )
        .unwrap();
        assert_eq!(
            PostgresBackupOperator::new(
                limited_pool.clone(),
                Arc::new(MemoryKeyProvider),
                limited_profile,
                tool(PathBuf::from(std::env::var("GRAPHHELM_TEST_PG_DUMP").unwrap())),
                tool(PathBuf::from(
                    std::env::var("GRAPHHELM_TEST_PG_RESTORE").unwrap()
                )),
                Duration::from_secs(30),
            )
            .await
            .unwrap_err(),
            BackupError::InvalidBackup
        );
        let limited_intact: (String, Option<String>) = sqlx::query_as(
            "SELECT owner.rolname,to_regclass('public.preexisting_owner_data')::text \
             FROM pg_database d JOIN pg_roles owner ON owner.oid=d.datdba \
             WHERE d.datname=current_database()",
        )
        .fetch_one(&limited_pool)
        .await
        .unwrap();
        assert_eq!(
            limited_intact,
            (limited_role.clone(), Some("preexisting_owner_data".to_owned()))
        );
        limited_pool.close().await;
        sqlx::query(AssertSqlSafe(format!(
            "DROP DATABASE {limited_database} WITH (FORCE)"
        )))
        .execute(&root_pool)
        .await
        .unwrap();
        sqlx::query(AssertSqlSafe(format!("DROP ROLE {limited_role}")))
            .execute(&root_pool)
            .await
            .unwrap();
        std::fs::remove_file(&limited_passfile).unwrap();

        let recovery_name = format!("graphhelm_restore_recovery_{}", uuid::Uuid::new_v4().simple());
        let recovery_owner = format!("graphhelm recovery owner {}", uuid::Uuid::new_v4().simple());
        let recovery_grantor =
            format!("graphhelm recovery grantor {}", uuid::Uuid::new_v4().simple());
        let recovery_reader = format!("graphhelm recovery reader {}", uuid::Uuid::new_v4().simple());
        let quoted_role = |role: &str| format!("\"{}\"", role.replace('"', "\"\""));
        sqlx::query(AssertSqlSafe(format!(
            "CREATE ROLE {} NOLOGIN",
            quoted_role(&recovery_owner)
        )))
            .execute(&root_pool)
            .await
            .unwrap();
        sqlx::query(AssertSqlSafe(format!(
            "CREATE ROLE {} NOLOGIN",
            quoted_role(&recovery_grantor)
        )))
        .execute(&root_pool)
        .await
        .unwrap();
        sqlx::query(AssertSqlSafe(format!(
            "CREATE ROLE {} NOLOGIN",
            quoted_role(&recovery_reader)
        )))
            .execute(&root_pool)
            .await
            .unwrap();
        sqlx::query(AssertSqlSafe(format!("CREATE DATABASE {recovery_name}")))
            .execute(&root_pool)
            .await
            .unwrap();
        sqlx::query(AssertSqlSafe(format!(
            "ALTER DATABASE {recovery_name} CONNECTION LIMIT 7"
        )))
        .execute(&root_pool)
        .await
        .unwrap();
        sqlx::query(AssertSqlSafe(format!(
            "ALTER DATABASE {recovery_name} OWNER TO {}",
            quoted_role(&recovery_owner)
        )))
        .execute(&root_pool)
        .await
        .unwrap();
        sqlx::query(AssertSqlSafe(format!(
            "REVOKE CONNECT ON DATABASE {recovery_name} FROM PUBLIC"
        )))
        .execute(&root_pool)
        .await
        .unwrap();
        sqlx::query(AssertSqlSafe(format!(
            "GRANT CONNECT ON DATABASE {recovery_name} TO {} WITH GRANT OPTION",
            quoted_role(&recovery_grantor)
        )))
        .execute(&root_pool)
        .await
        .unwrap();
        let mut delegated_grant = root_pool.begin().await.unwrap();
        sqlx::query(AssertSqlSafe(format!(
            "SET LOCAL ROLE {}",
            quoted_role(&recovery_grantor)
        )))
        .execute(&mut *delegated_grant)
        .await
        .unwrap();
        sqlx::query(AssertSqlSafe(format!(
            "GRANT CONNECT ON DATABASE {recovery_name} TO {}",
            quoted_role(&recovery_reader)
        )))
        .execute(&mut *delegated_grant)
        .await
        .unwrap();
        delegated_grant.commit().await.unwrap();
        let recovery_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect_with(root_options.clone().database(&recovery_name))
            .await
            .unwrap();
        let recovery_profile =
            DatabaseProcessProfile::new(host, port, user, &recovery_name, &passfile).unwrap();
        let recovery_operator = PostgresBackupOperator::new(
            recovery_pool.clone(),
            Arc::new(MemoryKeyProvider),
            recovery_profile.clone(),
            tool(PathBuf::from(std::env::var("GRAPHHELM_TEST_PG_DUMP").unwrap())),
            tool(PathBuf::from(std::env::var("GRAPHHELM_TEST_PG_RESTORE").unwrap())),
            Duration::from_secs(30),
        )
        .await
        .unwrap();
        let recovery_identity = recovery_operator.source_identity_sha256().await.unwrap();
        let application = "crashed-restore";
        let recovery_quarantine = format!("graphhelm_restore_q_{}", &recovery_identity[..32]);
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct MarkerAclEntry {
            grantor: String,
            grantee: Option<String>,
            privilege: String,
            grantable: bool,
        }
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct MarkerAccess {
            owner: String,
            principal_identities: Vec<MarkerPrincipalIdentity>,
            allow_connections: bool,
            connection_limit: i32,
            semantics: MarkerSemantics,
            acl: Vec<MarkerAclEntry>,
        }
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct MarkerPrincipalIdentity {
            name: String,
            identity_sha256: String,
        }
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct MarkerSemantics {
            encoding: String,
            locale_provider: String,
            collate: String,
            ctype: String,
            icu_locale: Option<String>,
            icu_rules: Option<String>,
            collation_version: Option<String>,
        }
        let marker_acl: Vec<(String, Option<String>, String, bool)> = sqlx::query_as(
            "SELECT grantor.rolname,grantee.rolname,acl.privilege_type::text,acl.is_grantable \
             FROM pg_database d \
             CROSS JOIN LATERAL aclexplode(COALESCE(d.datacl,acldefault('d',d.datdba))) acl \
             JOIN pg_roles grantor ON grantor.oid=acl.grantor \
             LEFT JOIN pg_roles grantee ON grantee.oid=acl.grantee \
             WHERE d.datname=$1 \
             ORDER BY COALESCE(grantee.rolname,''),acl.privilege_type,acl.is_grantable,grantor.rolname",
        )
        .bind(&recovery_name)
        .fetch_all(&root_pool)
        .await
        .unwrap();
        let system_identifier: String =
            sqlx::query_scalar("SELECT system_identifier::text FROM pg_control_system()")
                .fetch_one(&root_pool)
                .await
                .unwrap();
        let mut principal_names = vec![
            recovery_owner.clone(),
            recovery_grantor.clone(),
            recovery_reader.clone(),
        ];
        principal_names.sort();
        let mut principal_identities = Vec::new();
        for name in principal_names {
            let oid: String = sqlx::query_scalar("SELECT oid::text FROM pg_roles WHERE rolname=$1")
                .bind(&name)
                .fetch_one(&root_pool)
                .await
                .unwrap();
            principal_identities.push(MarkerPrincipalIdentity {
                identity_sha256: hex::encode(Sha256::digest(support::canonical_bytes(&(
                    "graphhelm-postgres-role-identity-v1",
                    &system_identifier,
                    oid,
                    &name,
                )))),
                name,
            });
        }
        let marker_access = MarkerAccess {
            owner: recovery_owner.clone(),
            principal_identities,
            allow_connections: true,
            connection_limit: 7,
            semantics: {
                let row: (String, String, String, String, Option<String>, Option<String>, Option<String>) = sqlx::query_as(
                    // `daticulocale` became `datlocale` in PostgreSQL 17. The adapter reads it
                    // through to_jsonb for exactly that reason; the test must be no less tolerant,
                    // because the project supports PostgreSQL 16+.
                    "SELECT pg_encoding_to_char(encoding)::text,datlocprovider::text,datcollate::text,datctype::text,\
                     COALESCE(to_jsonb(pg_database)->>'datlocale',to_jsonb(pg_database)->>'daticulocale'),\
                     daticurules::text,datcollversion::text FROM pg_database WHERE datname=$1"
                ).bind(&recovery_name).fetch_one(&root_pool).await.unwrap();
                MarkerSemantics {
                    encoding: row.0,
                    locale_provider: row.1,
                    collate: row.2,
                    ctype: row.3,
                    icu_locale: row.4,
                    icu_rules: row.5,
                    collation_version: row.6,
                }
            },
            acl: marker_acl
                .into_iter()
                .map(|(grantor, grantee, privilege, grantable)| MarkerAclEntry {
                    grantor,
                    grantee,
                    privilege,
                    grantable,
                })
                .collect(),
        };
        let recovery_replacement_owner =
            format!("graphhelm_restore_o_{}", &recovery_identity[..32]);
        sqlx::query(AssertSqlSafe(format!(
            "CREATE ROLE {recovery_replacement_owner} NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION NOBYPASSRLS"
        )))
        .execute(&root_pool)
        .await
        .unwrap();
        let recovery_owner_identity_values: (String, String) = sqlx::query_as(
            "SELECT system_identifier::text,r.oid::text FROM pg_control_system(),pg_roles r WHERE r.rolname=$1",
        )
        .bind(&recovery_replacement_owner)
        .fetch_one(&root_pool)
        .await
        .unwrap();
        let recovery_replacement_owner_identity = hex::encode(Sha256::digest(
            support::canonical_bytes(&(
                "graphhelm-restore-owner-role-v1",
                recovery_owner_identity_values.0,
                recovery_owner_identity_values.1,
                &recovery_replacement_owner,
            )),
        ));
        let marker_bytes = support::canonical_bytes(&(
            "graphhelm.restore.marker.v1",
            &recovery_name,
            &recovery_identity,
            application,
            &recovery_quarantine,
            &recovery_replacement_owner,
            &recovery_replacement_owner_identity,
            Option::<&str>::None,
            &marker_access,
        ));
        let marker_tag = MemoryKeyProvider
            .authenticate(
                AuthenticateRequest::new("graphhelm.restore.marker.v1", marker_bytes).unwrap(),
            )
            .await
            .unwrap();
        let marker = serde_json::json!({
            "format": "graphhelm.restore.marker.v1",
            "database": recovery_name,
            "targetIdentitySha256": recovery_identity,
            "application": application,
            "quarantine": recovery_quarantine,
            "replacementOwner": recovery_replacement_owner,
            "replacementOwnerIdentitySha256": recovery_replacement_owner_identity,
            "replacementIdentitySha256": null,
            "access": marker_access,
            "keyId": marker_tag.key_id(),
            "algorithm": marker_tag.algorithm(),
            "tagHex": hex::encode(marker_tag.bytes()),
        })
        .to_string();
        let quoted_marker = marker.replace('\'', "''");
        sqlx::query(AssertSqlSafe(format!(
            "COMMENT ON DATABASE \"{recovery_name}\" IS '{quoted_marker}'"
        )))
        .execute(&root_pool)
        .await
        .unwrap();
        drop(recovery_operator);
        recovery_pool.close().await;
        sqlx::query(AssertSqlSafe(format!(
            "ALTER DATABASE \"{recovery_name}\" ALLOW_CONNECTIONS false"
        )))
        .execute(&root_pool)
        .await
        .unwrap();
        sqlx::query(AssertSqlSafe(format!(
            "ALTER DATABASE \"{recovery_name}\" RENAME TO \"{recovery_quarantine}\""
        )))
        .execute(&root_pool)
        .await
        .unwrap();
        sqlx::query(AssertSqlSafe(format!("CREATE DATABASE {recovery_name}")))
            .execute(&root_pool)
            .await
            .unwrap();
        let hostile_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect_with(root_options.clone().database(&recovery_name))
            .await
            .unwrap();
        sqlx::query("CREATE TABLE public.unowned_replacement(value integer)")
            .execute(&hostile_pool)
            .await
            .unwrap();
        hostile_pool.close().await;
        sqlx::query(AssertSqlSafe(format!(
            "ALTER DATABASE {recovery_name} ALLOW_CONNECTIONS false"
        )))
        .execute(&root_pool)
        .await
        .unwrap();
        sqlx::query(AssertSqlSafe(format!(
            "ALTER DATABASE {recovery_name} CONNECTION LIMIT 0"
        )))
        .execute(&root_pool)
        .await
        .unwrap();
        let hostile_lazy_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect_lazy_with(root_options.clone().database(&recovery_name));
        assert_eq!(
            PostgresBackupOperator::new(
                hostile_lazy_pool.clone(),
                Arc::new(MemoryKeyProvider),
                recovery_profile.clone(),
                tool(PathBuf::from(std::env::var("GRAPHHELM_TEST_PG_DUMP").unwrap())),
                tool(PathBuf::from(
                    std::env::var("GRAPHHELM_TEST_PG_RESTORE").unwrap()
                )),
                Duration::from_secs(30),
            )
            .await
            .unwrap_err(),
            BackupError::InvalidBackup
        );
        hostile_lazy_pool.close().await;
        let hostile_owner: String = sqlx::query_scalar(
            "SELECT owner.rolname FROM pg_database d JOIN pg_roles owner ON owner.oid=d.datdba \
             WHERE d.datname=$1",
        )
        .bind(&recovery_name)
        .fetch_one(&root_pool)
        .await
        .unwrap();
        assert_eq!(hostile_owner, "postgres");
        sqlx::query(AssertSqlSafe(format!(
            "ALTER DATABASE {recovery_name} ALLOW_CONNECTIONS true"
        )))
        .execute(&root_pool)
        .await
        .unwrap();
        let hostile_verify_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect_with(root_options.clone().database(&recovery_name))
            .await
            .unwrap();
        let hostile_object: Option<String> =
            sqlx::query_scalar("SELECT to_regclass('public.unowned_replacement')::text")
                .fetch_one(&hostile_verify_pool)
                .await
                .unwrap();
        assert_eq!(hostile_object.as_deref(), Some("unowned_replacement"));
        hostile_verify_pool.close().await;
        sqlx::query(AssertSqlSafe(format!(
            "DROP DATABASE {recovery_name} WITH (FORCE)"
        )))
        .execute(&root_pool)
        .await
        .unwrap();
        sqlx::query(AssertSqlSafe(format!(
            "CREATE DATABASE \"{recovery_name}\" OWNER {recovery_replacement_owner} TEMPLATE template0 CONNECTION LIMIT 0"
        )))
        .execute(&root_pool)
        .await
        .unwrap();
        let exact_owner_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect_with(root_options.clone().database(&recovery_name))
            .await
            .unwrap();
        sqlx::query("CREATE TABLE public.unproven_exact_owner(value integer)")
            .execute(&exact_owner_pool)
            .await
            .unwrap();
        exact_owner_pool.close().await;
        sqlx::query(AssertSqlSafe(format!(
            "ALTER DATABASE {recovery_name} ALLOW_CONNECTIONS false"
        )))
        .execute(&root_pool)
        .await
        .unwrap();
        let unproven_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect_lazy_with(root_options.clone().database(&recovery_name));
        assert_eq!(
            PostgresBackupOperator::new(
                unproven_pool.clone(),
                Arc::new(MemoryKeyProvider),
                recovery_profile.clone(),
                tool(PathBuf::from(std::env::var("GRAPHHELM_TEST_PG_DUMP").unwrap())),
                tool(PathBuf::from(
                    std::env::var("GRAPHHELM_TEST_PG_RESTORE").unwrap()
                )),
                Duration::from_secs(30),
            )
            .await
            .unwrap_err(),
            BackupError::InvalidBackup
        );
        unproven_pool.close().await;
        let unproven_owner: String = sqlx::query_scalar(
            "SELECT owner.rolname FROM pg_database d JOIN pg_roles owner ON owner.oid=d.datdba WHERE d.datname=$1",
        )
        .bind(&recovery_name)
        .fetch_one(&root_pool)
        .await
        .unwrap();
        assert_eq!(unproven_owner, recovery_replacement_owner);
        sqlx::query(AssertSqlSafe(format!(
            "ALTER DATABASE {recovery_name} ALLOW_CONNECTIONS true"
        )))
        .execute(&root_pool)
        .await
        .unwrap();
        let unproven_verify_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect_with(root_options.clone().database(&recovery_name))
            .await
            .unwrap();
        let unproven_object: Option<String> =
            sqlx::query_scalar("SELECT to_regclass('public.unproven_exact_owner')::text")
                .fetch_one(&unproven_verify_pool)
                .await
                .unwrap();
        assert_eq!(unproven_object.as_deref(), Some("unproven_exact_owner"));
        unproven_verify_pool.close().await;
        sqlx::query(AssertSqlSafe(format!(
            "DROP DATABASE {recovery_name} WITH (FORCE)"
        )))
        .execute(&root_pool)
        .await
        .unwrap();
        let recovered_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect_lazy_with(root_options.clone().database(&recovery_name));
        let recovered_operator = PostgresBackupOperator::new(
            recovered_pool.clone(),
            Arc::new(MemoryKeyProvider),
            recovery_profile.clone(),
            tool(PathBuf::from(std::env::var("GRAPHHELM_TEST_PG_DUMP").unwrap())),
            tool(PathBuf::from(std::env::var("GRAPHHELM_TEST_PG_RESTORE").unwrap())),
            Duration::from_secs(30),
        )
        .await
        .unwrap();
        let recovered_objects: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace \
             WHERE n.nspname='public' AND c.relkind IN ('r','p','v','m','S','f')",
        )
        .fetch_one(&recovered_pool)
        .await
        .unwrap();
        assert_eq!(recovered_objects, 0);
        let recovered_connection_limit: i32 = sqlx::query_scalar(
            "SELECT datconnlimit FROM pg_database WHERE datname=current_database()",
        )
        .fetch_one(&recovered_pool)
        .await
        .unwrap();
        assert_eq!(recovered_connection_limit, 7);
        let recovered_access: (String, bool, bool, bool) = sqlx::query_as(
            "SELECT owner.rolname, \
             has_database_privilege('public',d.oid,'CONNECT'), \
             has_database_privilege($2,d.oid,'CONNECT'), \
             has_database_privilege($3,d.oid,'CONNECT WITH GRANT OPTION') \
             FROM pg_database d JOIN pg_roles owner ON owner.oid=d.datdba WHERE d.datname=$1",
        )
        .bind(&recovery_name)
        .bind(&recovery_reader)
        .bind(&recovery_grantor)
        .fetch_one(&root_pool)
        .await
        .unwrap();
        assert_eq!(
            recovered_access,
            (recovery_owner.clone(), false, true, true)
        );
        drop(recovered_operator);
        recovered_pool.close().await;

        let residual_quarantine: (bool, bool) = sqlx::query_as(
            "SELECT NOT datallowconn,shobj_description(oid,'pg_database') IS NOT NULL \
             FROM pg_database WHERE datname=$1",
        )
        .bind(&recovery_quarantine)
        .fetch_one(&root_pool)
        .await
        .unwrap();
        assert_eq!(residual_quarantine, (true, true));
        let replacement_role_exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_roles WHERE rolname=$1)")
                .bind(&recovery_replacement_owner)
                .fetch_one(&root_pool)
                .await
                .unwrap();
        assert!(!replacement_role_exists);

        let reopened_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect_lazy_with(root_options.clone().database(&recovery_name));
        let reopened_operator = PostgresBackupOperator::new(
            reopened_pool.clone(),
            Arc::new(MemoryKeyProvider),
            recovery_profile.clone(),
            tool(PathBuf::from(std::env::var("GRAPHHELM_TEST_PG_DUMP").unwrap())),
            tool(PathBuf::from(
                std::env::var("GRAPHHELM_TEST_PG_RESTORE").unwrap(),
            )),
            Duration::from_secs(30),
        )
        .await
        .unwrap();
        drop(reopened_operator);
        reopened_pool.close().await;

        sqlx::query(AssertSqlSafe(format!("DROP DATABASE {recovery_name}")))
            .execute(&root_pool)
            .await
            .unwrap();
        sqlx::query(AssertSqlSafe(format!("CREATE DATABASE {recovery_name}")))
            .execute(&root_pool)
            .await
            .unwrap();
        let replacement_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect_with(root_options.clone().database(&recovery_name))
            .await
            .unwrap();
        sqlx::query("CREATE TABLE public.replacement_owner_data(value integer)")
            .execute(&replacement_pool)
            .await
            .unwrap();
        sqlx::query(AssertSqlSafe(format!(
            "COMMENT ON DATABASE \"{recovery_name}\" IS '{quoted_marker}'"
        )))
        .execute(&root_pool)
        .await
        .unwrap();
        assert_eq!(
            PostgresBackupOperator::new(
                replacement_pool.clone(),
                Arc::new(MemoryKeyProvider),
                recovery_profile,
                tool(PathBuf::from(std::env::var("GRAPHHELM_TEST_PG_DUMP").unwrap())),
                tool(PathBuf::from(
                    std::env::var("GRAPHHELM_TEST_PG_RESTORE").unwrap()
                )),
                Duration::from_secs(30),
            )
            .await
            .unwrap_err(),
            BackupError::InvalidBackup
        );
        let replacement_intact: Option<String> =
            sqlx::query_scalar("SELECT to_regclass('public.replacement_owner_data')::text")
                .fetch_one(&replacement_pool)
                .await
                .unwrap();
        assert_eq!(replacement_intact.as_deref(), Some("replacement_owner_data"));
        replacement_pool.close().await;
        sqlx::query(AssertSqlSafe(format!(
            "DROP DATABASE {recovery_name} WITH (FORCE)"
        )))
        .execute(&root_pool)
        .await
        .unwrap();
        sqlx::query(AssertSqlSafe(format!(
            "DROP DATABASE \"{recovery_quarantine}\" WITH (FORCE)"
        )))
        .execute(&root_pool)
        .await
        .unwrap();
        sqlx::query(AssertSqlSafe(format!(
            "DROP ROLE {}",
            quoted_role(&recovery_reader)
        )))
            .execute(&root_pool)
            .await
            .unwrap();
        sqlx::query(AssertSqlSafe(format!(
            "DROP ROLE {}",
            quoted_role(&recovery_grantor)
        )))
        .execute(&root_pool)
        .await
        .unwrap();
        sqlx::query(AssertSqlSafe(format!(
            "DROP ROLE {}",
            quoted_role(&recovery_owner)
        )))
            .execute(&root_pool)
            .await
            .unwrap();

        let target_name = format!("graphhelm_restore_{}", uuid::Uuid::new_v4().simple());
        sqlx::query(AssertSqlSafe(format!("CREATE DATABASE {target_name}")))
            .execute(&root_pool)
            .await
            .unwrap();
        sqlx::query(AssertSqlSafe(format!(
            "ALTER DATABASE {target_name} CONNECTION LIMIT 5"
        )))
        .execute(&root_pool)
        .await
        .unwrap();
        sqlx::query(AssertSqlSafe(format!(
            "REVOKE CONNECT ON DATABASE {target_name} FROM PUBLIC"
        )))
        .execute(&root_pool)
        .await
        .unwrap();
        let target_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect_with(root_options.clone().database(&target_name))
            .await
            .unwrap();
        let target_profile =
            DatabaseProcessProfile::new(host, port, user, &target_name, &passfile).unwrap();
        let target_operator = PostgresBackupOperator::new(
            target_pool.clone(),
            Arc::new(MemoryKeyProvider),
            target_profile,
            tool(PathBuf::from(
                std::env::var("GRAPHHELM_TEST_PG_DUMP").unwrap(),
            )),
            tool(PathBuf::from(
                std::env::var("GRAPHHELM_TEST_PG_RESTORE").unwrap(),
            )),
            Duration::from_secs(30),
        )
        .await
        .unwrap();
        let restore_receipt = target_operator
            .restore_from_path(&destination)
            .await
            .unwrap();
        restore_receipt.verify(&MemoryKeyProvider).await.unwrap();
        let receipt_bytes = restore_receipt.to_bytes().unwrap();
        let transported_receipt =
            graphhelm_postgres_event_store::backup::RestoreReceipt::from_bytes(&receipt_bytes)
                .unwrap();
        transported_receipt
            .verify(&MemoryKeyProvider)
            .await
            .unwrap();
        assert_eq!(transported_receipt, restore_receipt);
        assert_eq!(restore_receipt.source_identity_sha256(), identity);
        assert_ne!(
            restore_receipt.source_identity_sha256(),
            restore_receipt.target_identity_sha256()
        );
        let migration_count: i64 = sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations")
            .fetch_one(&target_pool)
            .await
            .unwrap();
        assert_eq!(migration_count, 4);
        let restored_database_contract: (i32, bool) = sqlx::query_as(
            "SELECT datconnlimit, has_database_privilege('public', current_database(), 'CONNECT') \
             FROM pg_database WHERE datname=current_database()",
        )
        .fetch_one(&target_pool)
        .await
        .unwrap();
        assert_eq!(restored_database_contract, (5, false));
        let restored_retention: (String, i64, String, i64, i64) = sqlx::query_as(
            "SELECT \
             (SELECT state FROM public.graphhelm_retention_operations WHERE operation_id='backup-retention-operation'),\
             (SELECT count(*) FROM public.graphhelm_evidence_tombstones WHERE operation_id='backup-retention-operation'),\
             (SELECT state FROM public.graphhelm_evidence WHERE evidence_id=$1),\
             (SELECT count(*) FROM public.graphhelm_checkpoints),\
             (SELECT count(*) FROM public.graphhelm_projection_active)",
        )
        .bind(&evidence_id)
        .fetch_one(&target_pool)
        .await
        .unwrap();
        assert_eq!(restored_retention, ("finalized".to_owned(), 1, "erased".to_owned(), 1, 1));
        target_pool.close().await;
        sqlx::query(AssertSqlSafe(format!("DROP DATABASE {target_name}")))
            .execute(&root_pool)
            .await
            .unwrap();
        root_pool.close().await;
        std::fs::remove_file(destination).unwrap();
        std::fs::remove_file(passfile).unwrap();
        database.cleanup().await;
    });
}

#[test]
#[ignore = "requires GRAPHHELM_TEST_ADMIN_URL and pinned PostgreSQL client tools"]
fn constructor_bounds_reconciliation_catalog_locks() {
    support::runtime().block_on(async {
        let database = support::TestDatabase::new().await;
        database.runtime_pool.close().await;
        let url = std::env::var("GRAPHHELM_TEST_ADMIN_URL").unwrap();
        let root_options = PgConnectOptions::from_str(&url).unwrap();
        let root_pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(root_options.clone())
            .await
            .unwrap();
        let database_name: String = sqlx::query_scalar("SELECT current_database()")
            .fetch_one(database.admin_pool())
            .await
            .unwrap();
        let authority = url.strip_prefix("postgres://").unwrap();
        let (credentials, endpoint) = authority.split_once('@').unwrap();
        let (user, password) = credentials.split_once(':').unwrap();
        let (host_port, _) = endpoint.split_once('/').unwrap();
        let (host, port) = host_port.rsplit_once(':').unwrap();
        let passfile = std::env::current_dir().unwrap().join(format!(
            "target/task10-lock-{}.pgpass",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::write(&passfile, format!("{host}:{port}:*:{user}:{password}\n")).unwrap();
        let profile = DatabaseProcessProfile::new(
            host,
            port.parse().unwrap(),
            user,
            &database_name,
            &passfile,
        )
        .unwrap();
        let pinned = |environment: &str| {
            let path = PathBuf::from(std::env::var(environment).unwrap());
            let digest = hex::encode(Sha256::digest(std::fs::read(&path).unwrap()));
            let output = std::process::Command::new(&path)
                .arg("--version")
                .output()
                .unwrap();
            PinnedTool::new(
                path,
                digest,
                String::from_utf8(output.stdout).unwrap().trim(),
            )
            .unwrap()
        };
        let mut lock = root_pool.begin().await.unwrap();
        sqlx::query("LOCK TABLE pg_catalog.pg_authid IN ACCESS EXCLUSIVE MODE")
            .execute(&mut *lock)
            .await
            .unwrap();
        // The OBSERVER budget, and it is not a behavioural bound (#19). Its only job is
        // to stop a hang; its elapse is a FAILURE, not the assertion. It was 2 s -
        // twenty times the constructor's own 100 ms deadline, which reads generous and
        // is not, because the wall time needed to OBSERVE a 100 ms deadline is not
        // bounded by 100 ms under full-gate load. `source_invariants.rs` holds the
        // floor and the subject/observer split that this number belongs to.
        let result = tokio::time::timeout(
            Duration::from_secs(60),
            PostgresBackupOperator::new(
                database.admin_pool().clone(),
                Arc::new(MemoryKeyProvider),
                profile,
                pinned("GRAPHHELM_TEST_PG_DUMP"),
                pinned("GRAPHHELM_TEST_PG_RESTORE"),
                Duration::from_millis(100),
            ),
        )
        .await;
        // #81 casualty, found by the gate (the seal's census covered src pins and missed
        // this integration pin — three pins of the old fused mapping existed, not one):
        // a constructor blocked behind a catalog lock until its 100 ms budget elapsed is
        // a DEADLINE fact, and this assertion is now the integration-grain guard for the
        // constructor wrapper's elapsed mapping — the exact site whose flake #19 recorded
        // as "invalid backup" under load.
        let observed = match &result {
            Ok(Err(error)) => format!("constructor.error.{error}"),
            Ok(Ok(_)) => "constructor.success".to_owned(),
            Err(_) => "observer.outer_timeout".to_owned(),
        };
        assert_eq!(
            observed, "constructor.error.GHB003_DEADLINE_ELAPSED",
            "constructor returned an unexpected safe result class while pg_authid was locked"
        );
        lock.rollback().await.unwrap();
        std::fs::remove_file(passfile).unwrap();
        root_pool.close().await;
        database.cleanup().await;
    });
}
