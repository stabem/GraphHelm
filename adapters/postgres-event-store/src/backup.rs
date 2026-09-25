//! Encrypted PostgreSQL backup and verified-restore administration boundary.

use std::{
    collections::BTreeSet,
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    time::{Duration, Instant},
};

use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use graphhelm_events::{
    AsyncEventRepository, AuthenticateRequest, AuthenticationTag, EvidenceOpener,
    EvidenceProtector, KeyProvider, ProjectionGeneration, ProjectionRepository, ReadStart,
    ReadStreamRequest, SecretBytes, VerifyAuthenticationRequest, VerifyRangeRequest,
    WrapKeyRequest, WrappedKey,
};
use graphhelm_protocols::{ExecutionId, ProjectId, RawSha256, RepositoryScope, WorkspaceId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{
    Acquire, AssertSqlSafe, PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use zeroize::{Zeroize, Zeroizing};

const MAGIC: &[u8; 8] = b"GHBAK001";
const CHUNK_MARKER: &[u8; 4] = b"CHNK";
const FOOTER_MARKER: &[u8; 4] = b"MNFT";
const CHUNK_BYTES: usize = 1024 * 1024;
const MAX_BACKUP_BYTES: u64 = 64 * 1024 * 1024 * 1024;
const MAX_HEADER_BYTES: usize = 64 * 1024;
const MAX_MANIFEST_BYTES: usize = 1024 * 1024;
const MAX_TOOL_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_PROCESS_OUTPUT_BYTES: usize = 64 * 1024;
const MAX_CHUNKS: u64 = MAX_BACKUP_BYTES / CHUNK_BYTES as u64;
const MAX_ARCHIVE_BYTES: u64 = MAX_BACKUP_BYTES
    + MAX_CHUNKS * (CHUNK_MARKER.len() as u64 + 8 + 4 + 4 + 16)
    + MAGIC.len() as u64
    + 4
    + MAX_HEADER_BYTES as u64
    + FOOTER_MARKER.len() as u64
    + 4
    + MAX_MANIFEST_BYTES as u64;
const BACKUP_PURPOSE_HEADER: &str = "graphhelm.backup.header.v1";
const BACKUP_PURPOSE_MANIFEST: &str = "graphhelm.backup.manifest.v1";
const RESTORE_MARKER_PURPOSE: &str = "graphhelm.restore.marker.v1";
/// Digest of the schema contract as of migration `0004_scope_guard`.
///
/// Updated from `48ef7a42...`, which was computed before `0004` existed. The drift is entirely
/// that migration's: sixteen `graphhelm_scope_not_empty` CHECK constraints added and sixteen
/// `graphhelm_scope` policies rewritten, with nothing removed and no unexpected object. Recompute
/// this only when a migration is intended to change the schema, and prove the delta first — the
/// point of the pin is to reject drift nobody authorised.
const EXPECTED_SCHEMA_CONTRACT_SHA256: &str =
    "21a0832a6d144837f88acf9121db021b6d2e28fa57b2f2f26ab8780b57e0f479";
const UNEXPECTED_DATABASE_OBJECTS_SQL: &str = "SELECT (\
 (SELECT count(*) FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace \
  WHERE n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname NOT LIKE 'pg_toast%' \
    AND NOT (n.nspname='public' AND (c.relname LIKE 'graphhelm\\_%' ESCAPE '\\' \
      OR c.relname IN ('_sqlx_migrations','_sqlx_migrations_pkey')))) + \
 (SELECT count(*) FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace \
  WHERE n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname NOT LIKE 'pg_toast%' \
    AND NOT (n.nspname='public' AND p.proname LIKE 'graphhelm\\_%' ESCAPE '\\')) + \
 (SELECT count(*) FROM pg_type t JOIN pg_namespace n ON n.oid=t.typnamespace \
  WHERE n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname NOT LIKE 'pg_toast%' \
    AND (t.typtype IN ('d','e','r','m') OR (t.typtype='c' AND t.typrelid=0))) + \
 (SELECT count(*) FROM pg_namespace n WHERE n.nspname NOT IN ('public','pg_catalog','information_schema') \
    AND n.nspname NOT LIKE 'pg_toast%') + (SELECT count(*) FROM pg_extension WHERE extname <> 'plpgsql') + \
 (SELECT count(*) FROM pg_publication) + (SELECT count(*) FROM pg_subscription) + \
 (SELECT count(*) FROM pg_event_trigger) + (SELECT count(*) FROM pg_foreign_server) + \
 (SELECT count(*) FROM pg_foreign_data_wrapper) + (SELECT count(*) FROM pg_user_mapping) + \
 (SELECT count(*) FROM pg_largeobject_metadata) + (SELECT count(*) FROM pg_default_acl) + \
 (SELECT count(*) FROM pg_prepared_xacts WHERE database=current_database()) + \
 (SELECT count(*) FROM pg_replication_slots WHERE database=current_database()) + \
 (SELECT count(*) FROM pg_db_role_setting s JOIN pg_database d ON d.oid=s.setdatabase \
  WHERE d.datname=current_database()) + \
 (SELECT count(*) FROM pg_database WHERE datname=current_database() AND datistemplate) + \
 (SELECT count(*) FROM pg_cast WHERE oid >= 16384) + (SELECT count(*) FROM pg_transform WHERE oid >= 16384) + \
 (SELECT count(*) FROM pg_language WHERE oid >= 16384 AND lanname <> 'plpgsql') + \
 (SELECT count(*) FROM pg_am WHERE oid >= 16384) + \
 (SELECT count(*) FROM pg_collation c JOIN pg_namespace n ON n.oid=c.collnamespace \
  WHERE n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname NOT LIKE 'pg_toast%') + \
 (SELECT count(*) FROM pg_conversion c JOIN pg_namespace n ON n.oid=c.connamespace \
  WHERE n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname NOT LIKE 'pg_toast%') + \
 (SELECT count(*) FROM pg_operator o JOIN pg_namespace n ON n.oid=o.oprnamespace \
  WHERE n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname NOT LIKE 'pg_toast%') + \
 (SELECT count(*) FROM pg_opclass o JOIN pg_namespace n ON n.oid=o.opcnamespace \
  WHERE n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname NOT LIKE 'pg_toast%') + \
 (SELECT count(*) FROM pg_opfamily o JOIN pg_namespace n ON n.oid=o.opfnamespace \
  WHERE n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname NOT LIKE 'pg_toast%') + \
 (SELECT count(*) FROM pg_ts_config o JOIN pg_namespace n ON n.oid=o.cfgnamespace \
  WHERE n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname NOT LIKE 'pg_toast%') + \
 (SELECT count(*) FROM pg_ts_dict o JOIN pg_namespace n ON n.oid=o.dictnamespace \
  WHERE n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname NOT LIKE 'pg_toast%') + \
 (SELECT count(*) FROM pg_ts_parser o JOIN pg_namespace n ON n.oid=o.prsnamespace \
  WHERE n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname NOT LIKE 'pg_toast%') + \
 (SELECT count(*) FROM pg_ts_template o JOIN pg_namespace n ON n.oid=o.tmplnamespace \
  WHERE n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname NOT LIKE 'pg_toast%') + \
 (SELECT count(*) FROM pg_statistic_ext o JOIN pg_namespace n ON n.oid=o.stxnamespace \
  WHERE n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname NOT LIKE 'pg_toast%'))::bigint";
const FRESH_TARGET_OBJECTS_SQL: &str = "SELECT (\
 (SELECT count(*) FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace \
  WHERE n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname NOT LIKE 'pg_toast%') + \
 (SELECT count(*) FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace \
  WHERE n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname NOT LIKE 'pg_toast%') + \
 (SELECT count(*) FROM pg_type t JOIN pg_namespace n ON n.oid=t.typnamespace \
  WHERE n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname NOT LIKE 'pg_toast%' \
    AND (t.typtype IN ('d','e','r','m') OR (t.typtype='c' AND t.typrelid=0))) + \
 (SELECT count(*) FROM pg_namespace n WHERE n.nspname NOT IN ('public','pg_catalog','information_schema') \
  AND n.nspname NOT LIKE 'pg_toast%') + (SELECT count(*) FROM pg_extension WHERE extname <> 'plpgsql') + \
 (SELECT count(*) FROM pg_publication) + (SELECT count(*) FROM pg_subscription) + \
 (SELECT count(*) FROM pg_event_trigger) + (SELECT count(*) FROM pg_foreign_server) + \
 (SELECT count(*) FROM pg_foreign_data_wrapper) + (SELECT count(*) FROM pg_user_mapping) + \
 (SELECT count(*) FROM pg_largeobject_metadata) + (SELECT count(*) FROM pg_default_acl) + \
 (SELECT count(*) FROM pg_prepared_xacts WHERE database=current_database()) + \
 (SELECT count(*) FROM pg_replication_slots WHERE database=current_database()) + \
 (SELECT count(*) FROM pg_db_role_setting s JOIN pg_database d ON d.oid=s.setdatabase \
  WHERE d.datname=current_database()) + \
 (SELECT count(*) FROM pg_database WHERE datname=current_database() AND datistemplate) + \
 (SELECT count(*) FROM pg_cast WHERE oid >= 16384) + (SELECT count(*) FROM pg_transform WHERE oid >= 16384) + \
 (SELECT count(*) FROM pg_language WHERE oid >= 16384 AND lanname <> 'plpgsql') + \
 (SELECT count(*) FROM pg_am WHERE oid >= 16384) + \
 (SELECT count(*) FROM pg_collation c JOIN pg_namespace n ON n.oid=c.collnamespace \
  WHERE n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname NOT LIKE 'pg_toast%') + \
 (SELECT count(*) FROM pg_conversion c JOIN pg_namespace n ON n.oid=c.connamespace \
  WHERE n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname NOT LIKE 'pg_toast%') + \
 (SELECT count(*) FROM pg_operator o JOIN pg_namespace n ON n.oid=o.oprnamespace \
  WHERE n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname NOT LIKE 'pg_toast%') + \
 (SELECT count(*) FROM pg_opclass o JOIN pg_namespace n ON n.oid=o.opcnamespace \
  WHERE n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname NOT LIKE 'pg_toast%') + \
 (SELECT count(*) FROM pg_opfamily o JOIN pg_namespace n ON n.oid=o.opfnamespace \
  WHERE n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname NOT LIKE 'pg_toast%') + \
 (SELECT count(*) FROM pg_ts_config o JOIN pg_namespace n ON n.oid=o.cfgnamespace \
  WHERE n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname NOT LIKE 'pg_toast%') + \
 (SELECT count(*) FROM pg_ts_dict o JOIN pg_namespace n ON n.oid=o.dictnamespace \
  WHERE n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname NOT LIKE 'pg_toast%') + \
 (SELECT count(*) FROM pg_ts_parser o JOIN pg_namespace n ON n.oid=o.prsnamespace \
  WHERE n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname NOT LIKE 'pg_toast%') + \
 (SELECT count(*) FROM pg_ts_template o JOIN pg_namespace n ON n.oid=o.tmplnamespace \
  WHERE n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname NOT LIKE 'pg_toast%') + \
 (SELECT count(*) FROM pg_statistic_ext o JOIN pg_namespace n ON n.oid=o.stxnamespace \
  WHERE n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname NOT LIKE 'pg_toast%'))::bigint";
const SCHEMA_CONTRACT_QUERY: &str = r#"
WITH objects AS (
SELECT 'relation' category,c.relname name,jsonb_build_object('kind',c.relkind,'rls',c.relrowsecurity,'force',c.relforcerowsecurity) payload FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public' AND (c.relname LIKE 'graphhelm_%' OR c.relname='_sqlx_migrations') AND c.relkind IN ('r','p')
UNION ALL SELECT 'column',c.relname||'.'||a.attnum::text,jsonb_build_object('name',a.attname,'type',format_type(a.atttypid,a.atttypmod),'notnull',a.attnotnull,'default',pg_get_expr(d.adbin,d.adrelid)) FROM pg_attribute a JOIN pg_class c ON c.oid=a.attrelid JOIN pg_namespace n ON n.oid=c.relnamespace LEFT JOIN pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum WHERE n.nspname='public' AND (c.relname LIKE 'graphhelm_%' OR c.relname='_sqlx_migrations') AND c.relkind IN ('r','p') AND a.attnum>0 AND NOT a.attisdropped
UNION ALL SELECT 'constraint',c.relname||'.'||co.conname,jsonb_build_object('type',co.contype,'def',pg_get_constraintdef(co.oid,true)) FROM pg_constraint co JOIN pg_class c ON c.oid=co.conrelid JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public' AND (c.relname LIKE 'graphhelm_%' OR c.relname='_sqlx_migrations')
UNION ALL SELECT 'index',c.relname||'.'||i.relname,jsonb_build_object('def',pg_get_indexdef(i.oid)) FROM pg_index x JOIN pg_class c ON c.oid=x.indrelid JOIN pg_class i ON i.oid=x.indexrelid JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public' AND (c.relname LIKE 'graphhelm_%' OR c.relname='_sqlx_migrations')
UNION ALL SELECT 'policy',c.relname||'.'||p.polname,jsonb_build_object('cmd',p.polcmd,'permissive',p.polpermissive,'roles',p.polroles,'qual',pg_get_expr(p.polqual,p.polrelid),'check',pg_get_expr(p.polwithcheck,p.polrelid)) FROM pg_policy p JOIN pg_class c ON c.oid=p.polrelid JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public' AND c.relname LIKE 'graphhelm_%'
UNION ALL SELECT 'trigger',c.relname||'.'||t.tgname,jsonb_build_object('def',pg_get_triggerdef(t.oid,true)) FROM pg_trigger t JOIN pg_class c ON c.oid=t.tgrelid JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public' AND c.relname LIKE 'graphhelm_%' AND NOT t.tgisinternal
UNION ALL SELECT 'function',p.proname||'.'||pg_get_function_identity_arguments(p.oid),jsonb_build_object('def',pg_get_functiondef(p.oid),'config',p.proconfig,'security',p.prosecdef) FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname='public' AND p.proname LIKE 'graphhelm_%'
) SELECT jsonb_agg(jsonb_build_object('category',category,'name',name,'payload',payload) ORDER BY category COLLATE "C",name COLLATE "C",payload::text COLLATE "C")::text FROM objects
"#;
const PRIVILEGE_CONTRACT_QUERY: &str = r#"
WITH grants AS (
SELECT 'relation' kind,c.relname object_name,COALESCE(grantee.rolname,'PUBLIC') grantee_name,
       COALESCE(grantor.rolname,'PUBLIC') grantor_name,a.privilege_type,a.is_grantable
FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
CROSS JOIN LATERAL aclexplode(COALESCE(c.relacl,acldefault('r',c.relowner))) a
LEFT JOIN pg_roles grantee ON grantee.oid=a.grantee LEFT JOIN pg_roles grantor ON grantor.oid=a.grantor
WHERE n.nspname='public' AND (c.relname LIKE 'graphhelm_%' OR c.relname='_sqlx_migrations') AND c.relkind IN ('r','p')
UNION ALL
SELECT 'function',p.proname||'.'||pg_get_function_identity_arguments(p.oid),COALESCE(grantee.rolname,'PUBLIC'),
       COALESCE(grantor.rolname,'PUBLIC'),a.privilege_type,a.is_grantable
FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
CROSS JOIN LATERAL aclexplode(COALESCE(p.proacl,acldefault('f',p.proowner))) a
LEFT JOIN pg_roles grantee ON grantee.oid=a.grantee LEFT JOIN pg_roles grantor ON grantor.oid=a.grantor
WHERE n.nspname='public' AND p.proname LIKE 'graphhelm_%'
UNION ALL
SELECT 'schema',n.nspname,COALESCE(grantee.rolname,'PUBLIC'),COALESCE(grantor.rolname,'PUBLIC'),a.privilege_type,a.is_grantable
FROM pg_namespace n CROSS JOIN LATERAL aclexplode(COALESCE(n.nspacl,acldefault('n',n.nspowner))) a
LEFT JOIN pg_roles grantee ON grantee.oid=a.grantee LEFT JOIN pg_roles grantor ON grantor.oid=a.grantor WHERE n.nspname='public'
) SELECT jsonb_agg(jsonb_build_object('kind',kind,'object',object_name,'grantee',grantee_name,
         'grantor',grantor_name,'privilege',privilege_type,'grantable',is_grantable)
         ORDER BY kind COLLATE "C",object_name COLLATE "C",grantee_name COLLATE "C",
                  grantor_name COLLATE "C",privilege_type COLLATE "C",is_grantable)::text FROM grants
"#;
const STATE_TABLES: &[&str] = &[
    "graphhelm_streams",
    "graphhelm_idempotency",
    "graphhelm_events",
    "graphhelm_evidence",
    "graphhelm_artifacts",
    "graphhelm_evidence_refs",
    "graphhelm_artifact_refs",
    "graphhelm_checkpoints",
    "graphhelm_retention_policies",
    "graphhelm_retention_operations",
    "graphhelm_retention_targets",
    "graphhelm_legal_holds",
    "graphhelm_evidence_tombstones",
    "graphhelm_cleanup_receipts",
    "graphhelm_projection_checkpoints",
    "graphhelm_projection_active",
];
const EXPECTED_RELATION_COUNT: i64 = 39;

#[derive(Default)]
struct PlaintextBudget(u64);

impl PlaintextBudget {
    fn account(&mut self, bytes: u64) -> Result<(), BackupError> {
        self.0 = self
            .0
            .checked_add(bytes)
            .ok_or(BackupError::LimitExceeded)?;
        if self.0 > MAX_BACKUP_BYTES {
            return Err(BackupError::LimitExceeded);
        }
        Ok(())
    }

    const fn total(&self) -> u64 {
        self.0
    }
}

/// Stable, redacted backup and restore failures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackupError {
    /// Backup configuration or authenticated material is invalid.
    InvalidBackup,
    /// Restore target or verification is invalid.
    InvalidRestore,
    /// A bounded step ran OUT OF TIME (#81). Its own variant and its own CODE because "the
    /// machine was busy" and "this backup is not trustworthy" have opposite operator
    /// responses — retry later versus never retry — and before this variant existed the
    /// elapsed paths scattered across THREE other codes (GHB001 via `Unavailable`, GHB002
    /// via `InvalidRestore`, GHE006 via `LimitExceeded`), so the operator could not tell a
    /// slow machine from a corrupt archive. Timing is never laundered into a verdict about
    /// the data.
    DeadlineElapsed,
    /// A deterministic public bound was exceeded.
    LimitExceeded,
    /// The configured key provider rejected the operation.
    KeyUnavailable,
    /// An external process or storage operation failed.
    Unavailable,
}

struct AbortTaskOnDrop<T> {
    task: Option<tokio::task::JoinHandle<T>>,
    cancelled: Arc<AtomicBool>,
}

impl<T> AbortTaskOnDrop<T> {
    fn new(task: tokio::task::JoinHandle<T>, cancelled: Arc<AtomicBool>) -> Self {
        Self {
            task: Some(task),
            cancelled,
        }
    }

    async fn wait(mut self) -> Result<T, tokio::task::JoinError> {
        let result = self.task.as_mut().expect("owned task").await;
        self.task.take();
        result
    }
}

impl<T> Drop for AbortTaskOnDrop<T> {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            self.cancelled.store(true, Ordering::Release);
            task.abort();
        }
    }
}
impl BackupError {
    /// Stable diagnostic code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::InvalidBackup => "GHB001_BACKUP_INVALID",
            Self::InvalidRestore => "GHB002_RESTORE_INVALID",
            // Its own code, not a reuse: GHB001 already carries two variants, which is how
            // "timed out" hid inside "backup invalid" for a milestone. Tests assert at THIS
            // grain, where the operator reads.
            Self::DeadlineElapsed => "GHB003_DEADLINE_ELAPSED",
            Self::LimitExceeded => "GHE006_LIMIT_EXCEEDED",
            Self::KeyUnavailable => "GHK001_KEY_UNAVAILABLE",
            Self::Unavailable => "GHB001_BACKUP_INVALID",
        }
    }
}

impl std::fmt::Display for BackupError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for BackupError {}

#[derive(Clone, Copy)]
enum UnavailableStage {
    ArchiveWrite,
    Cancellation,
    // Raised only by the Windows directory opener (#869).
    #[cfg_attr(not(windows), allow(dead_code))]
    DirectoryOpen,
    FileIdentity,
    FileIo,
    JobSetup,
    PipeRead,
    ProcessResume,
    ProcessTerminate,
    ProcessWait,
    Random,
    TaskJoin,
    TemporaryCreate,
}

impl UnavailableStage {
    const fn code(self) -> &'static str {
        match self {
            Self::ArchiveWrite => "archive.write",
            Self::Cancellation => "operation.cancel",
            Self::DirectoryOpen => "directory.open",
            Self::FileIdentity => "file.identity",
            Self::FileIo => "file.io",
            Self::JobSetup => "process.job.setup",
            Self::PipeRead => "process.pipe.read",
            Self::ProcessResume => "process.resume",
            Self::ProcessTerminate => "process.terminate",
            Self::ProcessWait => "process.wait",
            Self::Random => "random.source",
            Self::TaskJoin => "task.join",
            Self::TemporaryCreate => "temporary.create",
        }
    }
}

fn unavailable(stage: UnavailableStage) -> BackupError {
    eprintln!("[graphhelm-backup] unavailable={}", stage.code());
    BackupError::Unavailable
}

/// `unavailable` plus the raw OS error code, an integer from the kernel and never caller input.
#[cfg(target_os = "linux")]
fn unavailable_os(stage: UnavailableStage, os: Option<i32>) -> BackupError {
    eprintln!("[graphhelm-backup] unavailable={} os={os:?}", stage.code());
    BackupError::Unavailable
}

/// Exact executable identity admitted at the process boundary.
#[derive(Clone, PartialEq, Eq)]
pub struct PinnedTool {
    path: PathBuf,
    sha256: [u8; 32],
    exact_version: String,
}

impl std::fmt::Debug for PinnedTool {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PinnedTool")
            .field("path", &"[redacted]")
            .field("sha256", &"[redacted]")
            .field("exact_version", &"[redacted]")
            .finish()
    }
}

impl PinnedTool {
    fn sha256_hex(&self) -> String {
        hex::encode(self.sha256)
    }

    /// Validates an absolute tool path, raw lowercase SHA-256, and exact bounded version token.
    pub fn new(
        path: PathBuf,
        sha256: impl AsRef<str>,
        exact_version: impl Into<String>,
    ) -> Result<Self, BackupError> {
        let digest = sha256.as_ref();
        let exact_version = exact_version.into();
        if !path.is_absolute()
            || digest.len() != 64
            || !digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
            || exact_version.is_empty()
            || exact_version.len() > 64
            || !exact_version
                .bytes()
                .all(|byte| byte.is_ascii_graphic() || byte == b' ')
        {
            return Err(BackupError::InvalidBackup);
        }
        let mut bytes = [0_u8; 32];
        hex::decode_to_slice(digest, &mut bytes).map_err(|_| BackupError::InvalidBackup)?;
        Ok(Self {
            path,
            sha256: bytes,
            exact_version,
        })
    }

    /// Revalidates the executable bytes and exact `--version` response before use.
    pub fn verify_identity(&self, timeout: Duration) -> Result<(), BackupError> {
        self.verified_for_use(timeout).map(|_| ())
    }

    fn verified_for_use(&self, timeout: Duration) -> Result<VerifiedTool, BackupError> {
        if timeout.is_zero() || timeout > Duration::from_secs(300) {
            return Err(BackupError::InvalidBackup);
        }
        let verified = self.verified_execution()?;
        let result = run_bounded_process(verified.path(), &["--version"], &[], timeout)?;
        if !result.success
            || result.stdout_truncated
            || result.stderr_truncated
            || std::str::from_utf8(&result.stdout).map(str::trim).ok()
                != Some(self.exact_version.as_str())
        {
            return Err(BackupError::InvalidBackup);
        }
        Ok(verified)
    }

    fn verified_execution(&self) -> Result<VerifiedTool, BackupError> {
        #[cfg(all(unix, not(target_os = "linux")))]
        return Err(unavailable(UnavailableStage::FileIo));
        #[cfg(windows)]
        let anchors = pin_execution_ancestors(&self.path)?;
        #[cfg(not(windows))]
        let anchors = Vec::new();
        let file = open_pinned_executable(&self.path)?;
        let size = file
            .metadata()
            .map_err(|_| BackupError::InvalidBackup)?
            .len();
        if size == 0 || size > MAX_TOOL_BYTES {
            return Err(BackupError::InvalidBackup);
        }
        let mut reader = std::io::BufReader::new(&file);
        let mut digest = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = reader
                .read(&mut buffer)
                .map_err(|_| BackupError::InvalidBackup)?;
            if read == 0 {
                break;
            }
            digest.update(&buffer[..read]);
        }
        if digest.finalize().as_slice() != self.sha256 {
            return Err(BackupError::InvalidBackup);
        }
        let named = open_pinned_executable(&self.path)?;
        if !same_identity(&file, &named)? {
            return Err(BackupError::InvalidBackup);
        }
        Ok(VerifiedTool::new(file, self.path.clone(), anchors))
    }
}

struct VerifiedTool {
    _file: File,
    _anchors: Vec<File>,
    execution_path: PathBuf,
}

impl VerifiedTool {
    fn new(file: File, configured_path: PathBuf, anchors: Vec<File>) -> Self {
        #[cfg(target_os = "linux")]
        let execution_path = {
            use std::os::fd::AsRawFd;
            let _ = configured_path;
            PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd()))
        };
        #[cfg(windows)]
        let execution_path = configured_path;
        Self {
            _file: file,
            _anchors: anchors,
            execution_path,
        }
    }

    fn path(&self) -> &Path {
        &self.execution_path
    }
}

#[cfg(unix)]
fn open_pinned_executable(path: &Path) -> Result<File, BackupError> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| BackupError::InvalidBackup)
}

#[cfg(windows)]
fn open_pinned_executable(path: &Path) -> Result<File, BackupError> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::{
        Foundation::GENERIC_READ,
        Storage::FileSystem::{FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ},
    };
    let file = OpenOptions::new()
        .access_mode(GENERIC_READ)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|_| BackupError::InvalidBackup)?;
    validate_windows_handle(&file, false)?;
    Ok(file)
}

#[cfg(windows)]
fn pin_execution_ancestors(path: &Path) -> Result<Vec<File>, BackupError> {
    let parent = path.parent().ok_or(BackupError::InvalidBackup)?;
    let mut paths = parent.ancestors().collect::<Vec<_>>();
    paths.reverse();
    paths
        .into_iter()
        .map(open_pinned_directory)
        .collect::<Result<Vec<_>, _>>()
}

#[cfg(windows)]
fn open_pinned_directory(path: &Path) -> Result<File, BackupError> {
    use std::os::windows::{ffi::OsStrExt, io::FromRawHandle};
    use windows_sys::Win32::{
        Foundation::INVALID_HANDLE_VALUE,
        Storage::FileSystem::{
            CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
            FILE_SHARE_WRITE, OPEN_EXISTING,
        },
    };
    let mut wide = path.as_os_str().encode_wide().collect::<Vec<_>>();
    wide.push(0);
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(BackupError::InvalidBackup);
    }
    let file = unsafe { File::from_raw_handle(handle) };
    validate_windows_handle(&file, true)?;
    Ok(file)
}

#[cfg(windows)]
fn validate_windows_handle(file: &File, directory: bool) -> Result<(), BackupError> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO,
        FileAttributeTagInfo, GetFileInformationByHandleEx,
    };
    let mut attributes = FILE_ATTRIBUTE_TAG_INFO::default();
    let result = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle() as _,
            FileAttributeTagInfo,
            std::ptr::addr_of_mut!(attributes).cast(),
            std::mem::size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        )
    };
    let is_directory = attributes.FileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0;
    if result == 0
        || attributes.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || is_directory != directory
    {
        return Err(BackupError::InvalidBackup);
    }
    Ok(())
}

/// Exact libpq process profile. Password material remains solely in the referenced passfile.
#[derive(Clone, PartialEq, Eq)]
pub struct DatabaseProcessProfile {
    host: String,
    port: u16,
    user: String,
    database: String,
    passfile: PathBuf,
}

impl std::fmt::Debug for DatabaseProcessProfile {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DatabaseProcessProfile")
            .field("host", &"[redacted]")
            .field("port", &self.port)
            .field("user", &"[redacted]")
            .field("database", &"[redacted]")
            .field("passfile", &"[redacted]")
            .finish()
    }
}

impl DatabaseProcessProfile {
    pub fn new(
        host: impl Into<String>,
        port: u16,
        user: impl Into<String>,
        database: impl Into<String>,
        passfile: impl Into<PathBuf>,
    ) -> Result<Self, BackupError> {
        let host = host.into();
        let user = user.into();
        let database = database.into();
        let passfile = passfile.into();
        let parsed_host = host
            .strip_prefix('[')
            .and_then(|host| host.strip_suffix(']'))
            .unwrap_or(&host)
            .parse::<std::net::IpAddr>()
            .map_err(|_| BackupError::InvalidBackup)?;
        let identifier_valid = |value: &str| {
            !value.is_empty()
                && value.len() <= 128
                && !value.starts_with('-')
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        };
        if !parsed_host.is_ipv4()
            || port == 0
            || !identifier_valid(&user)
            || !identifier_valid(&database)
            || matches!(database.as_str(), "postgres" | "template0" | "template1")
            || !passfile.is_absolute()
        {
            return Err(BackupError::InvalidBackup);
        }
        Ok(Self {
            host: parsed_host.to_string(),
            port,
            user,
            database,
            passfile,
        })
    }
}

fn quoted_identifier(value: &str) -> Result<String, BackupError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return Err(BackupError::InvalidRestore);
    }
    Ok(format!("\"{value}\""))
}

fn quoted_pg_role_identifier(value: &str) -> Result<String, BackupError> {
    if value.is_empty() || value.len() > 63 || value.as_bytes().contains(&0) {
        return Err(BackupError::InvalidRestore);
    }
    Ok(format!("\"{}\"", value.replace('"', "\"\"")))
}

fn quoted_literal(value: &str) -> Result<String, BackupError> {
    if value.len() > 16 * 1024 || value.as_bytes().contains(&0) {
        return Err(BackupError::InvalidRestore);
    }
    Ok(format!("'{}'", value.replace('\'', "''")))
}

/// Separate administrative backup/restore capability bound to one verified pool and profile.
#[derive(Clone, PartialEq, Eq)]
pub struct RestoreReceipt {
    source_identity_sha256: String,
    target_identity_sha256: String,
    manifest_sha256: String,
    authentication_tag: AuthenticationTag,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RestoreReceiptWire {
    source_identity_sha256: String,
    target_identity_sha256: String,
    manifest_sha256: String,
    authentication_key_id: String,
    authentication_algorithm: String,
    authentication_tag: String,
}

impl std::fmt::Debug for RestoreReceipt {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RestoreReceipt")
            .field("source_identity_sha256", &self.source_identity_sha256)
            .field("target_identity_sha256", &self.target_identity_sha256)
            .field("manifest_sha256", &self.manifest_sha256)
            .field("authentication_tag", &"[redacted]")
            .finish()
    }
}

impl RestoreReceipt {
    #[must_use]
    pub fn source_identity_sha256(&self) -> &str {
        &self.source_identity_sha256
    }

    #[must_use]
    pub fn target_identity_sha256(&self) -> &str {
        &self.target_identity_sha256
    }

    #[must_use]
    pub fn authentication_key_id(&self) -> &str {
        self.authentication_tag.key_id()
    }

    #[must_use]
    pub const fn authentication_algorithm(&self) -> &'static str {
        self.authentication_tag.algorithm()
    }

    #[must_use]
    pub fn manifest_sha256(&self) -> &str {
        &self.manifest_sha256
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, BackupError> {
        canonical_bytes(&RestoreReceiptWire {
            source_identity_sha256: self.source_identity_sha256.clone(),
            target_identity_sha256: self.target_identity_sha256.clone(),
            manifest_sha256: self.manifest_sha256.clone(),
            authentication_key_id: self.authentication_tag.key_id().to_owned(),
            authentication_algorithm: self.authentication_tag.algorithm().to_owned(),
            authentication_tag: hex::encode(self.authentication_tag.bytes()),
        })
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, BackupError> {
        if bytes.is_empty() || bytes.len() > 16 * 1024 {
            return Err(BackupError::InvalidRestore);
        }
        let wire: RestoreReceiptWire =
            serde_json::from_slice(bytes).map_err(|_| BackupError::InvalidRestore)?;
        let valid_hash = |value: &str| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        };
        if !valid_hash(&wire.source_identity_sha256)
            || !valid_hash(&wire.target_identity_sha256)
            || !valid_hash(&wire.manifest_sha256)
        {
            return Err(BackupError::InvalidRestore);
        }
        let tag = hex::decode(wire.authentication_tag).map_err(|_| BackupError::InvalidRestore)?;
        Ok(Self {
            source_identity_sha256: wire.source_identity_sha256,
            target_identity_sha256: wire.target_identity_sha256,
            manifest_sha256: wire.manifest_sha256,
            authentication_tag: AuthenticationTag::new(
                wire.authentication_key_id,
                &wire.authentication_algorithm,
                tag,
            )
            .map_err(|_| BackupError::InvalidRestore)?,
        })
    }

    pub async fn verify(&self, provider: &dyn KeyProvider) -> Result<(), BackupError> {
        provider
            .verify(
                VerifyAuthenticationRequest::new(
                    "graphhelm.restore.receipt.v1",
                    restore_receipt_bytes(
                        &self.source_identity_sha256,
                        &self.target_identity_sha256,
                        &self.manifest_sha256,
                    )?,
                    self.authentication_tag.clone(),
                )
                .map_err(|_| BackupError::InvalidRestore)?,
            )
            .await
            .map_err(|_| BackupError::InvalidRestore)
    }
}

#[derive(Clone)]
pub struct PostgresBackupOperator {
    admin_pool: PgPool,
    control_pool: PgPool,
    key_provider: Arc<dyn KeyProvider>,
    profile: DatabaseProcessProfile,
    pg_dump: PinnedTool,
    pg_restore: PinnedTool,
    process_timeout: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DatabaseAclEntry {
    grantor: String,
    grantee: Option<String>,
    privilege: String,
    grantable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DatabaseAccessContract {
    owner: String,
    principal_identities: Vec<DatabasePrincipalIdentity>,
    allow_connections: bool,
    connection_limit: i32,
    semantics: DatabaseSemanticIdentity,
    acl: Vec<DatabaseAclEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DatabasePrincipalIdentity {
    name: String,
    identity_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DatabaseSemanticIdentity {
    encoding: String,
    locale_provider: String,
    collate: String,
    ctype: String,
    icu_locale: Option<String>,
    icu_rules: Option<String>,
    collation_version: Option<String>,
}

impl DatabaseSemanticIdentity {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        encoding: impl Into<String>,
        locale_provider: impl Into<String>,
        collate: impl Into<String>,
        ctype: impl Into<String>,
        icu_locale: Option<String>,
        icu_rules: Option<String>,
        collation_version: Option<String>,
    ) -> Result<Self, BackupError> {
        let identity = Self {
            encoding: encoding.into(),
            locale_provider: locale_provider.into(),
            collate: collate.into(),
            ctype: ctype.into(),
            icu_locale,
            icu_rules,
            collation_version,
        };
        validate_database_semantic_contract(&identity)?;
        Ok(identity)
    }
}

type RawDatabaseAclRow = (Option<String>, Option<String>, bool, String, bool);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RestoreMarker {
    format: String,
    database: String,
    target_identity_sha256: String,
    application: String,
    quarantine: String,
    replacement_owner: String,
    replacement_owner_identity_sha256: String,
    replacement_identity_sha256: Option<String>,
    access: DatabaseAccessContract,
    key_id: String,
    algorithm: String,
    tag_hex: String,
}

#[allow(clippy::too_many_arguments)]
fn restore_marker_bytes(
    database: &str,
    target_identity_sha256: &str,
    application: &str,
    quarantine: &str,
    replacement_owner: &str,
    replacement_owner_identity_sha256: &str,
    replacement_identity_sha256: Option<&str>,
    access: &DatabaseAccessContract,
) -> Result<Vec<u8>, BackupError> {
    canonical_bytes(&(
        RESTORE_MARKER_PURPOSE,
        database,
        target_identity_sha256,
        application,
        quarantine,
        replacement_owner,
        replacement_owner_identity_sha256,
        replacement_identity_sha256,
        access,
    ))
}

impl std::fmt::Debug for PostgresBackupOperator {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PostgresBackupOperator")
            .field("admin_pool", &"[redacted]")
            .field("control_pool", &"[redacted]")
            .field("key_provider", &"[redacted]")
            .field("profile", &self.profile)
            .field("pg_dump", &self.pg_dump)
            .field("pg_restore", &self.pg_restore)
            .field("process_timeout", &self.process_timeout)
            .finish()
    }
}

impl PostgresBackupOperator {
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        admin_pool: PgPool,
        key_provider: Arc<dyn KeyProvider>,
        profile: DatabaseProcessProfile,
        pg_dump: PinnedTool,
        pg_restore: PinnedTool,
        process_timeout: Duration,
    ) -> Result<Self, BackupError> {
        if process_timeout.is_zero() || process_timeout > Duration::from_secs(24 * 60 * 60) {
            return Err(BackupError::InvalidBackup);
        }
        if admin_pool.options().get_max_connections() != 1 {
            return Err(BackupError::InvalidBackup);
        }
        let passfile = File::open(&profile.passfile).map_err(|_| BackupError::InvalidBackup)?;
        if !passfile
            .metadata()
            .map_err(|_| BackupError::InvalidBackup)?
            .is_file()
        {
            return Err(BackupError::InvalidBackup);
        }
        // Tool verification is synchronous, so an outer Tokio timeout cannot pre-empt it on a
        // current-thread runtime. Spend one constructor deadline across both processes and the
        // async reconciliation instead of giving each preflight a fresh private allowance.
        let constructor_deadline =
            OperationDeadline::new(process_timeout.min(Duration::from_secs(30)));
        let dump_budget = constructor_deadline.step(Duration::from_secs(30));
        if dump_budget.is_zero() {
            return Err(BackupError::DeadlineElapsed);
        }
        pg_dump.verify_identity(dump_budget)?;
        let restore_budget = constructor_deadline.step(Duration::from_secs(30));
        if restore_budget.is_zero() {
            return Err(BackupError::DeadlineElapsed);
        }
        pg_restore.verify_identity(restore_budget)?;
        let reconciliation_budget = constructor_deadline.remaining();
        if reconciliation_budget.is_zero() {
            return Err(BackupError::DeadlineElapsed);
        }
        tokio::time::timeout(
            reconciliation_budget,
            Self::new_bounded(
                admin_pool,
                key_provider,
                profile,
                pg_dump,
                pg_restore,
                process_timeout,
            ),
        )
        .await
        .map_err(|_| BackupError::DeadlineElapsed)?
    }

    async fn new_bounded(
        admin_pool: PgPool,
        key_provider: Arc<dyn KeyProvider>,
        profile: DatabaseProcessProfile,
        pg_dump: PinnedTool,
        pg_restore: PinnedTool,
        process_timeout: Duration,
    ) -> Result<Self, BackupError> {
        key_provider
            .metadata()
            .await
            .map_err(|_| BackupError::KeyUnavailable)?;
        let control_pool = verify_profile_endpoint(&profile).await?;
        let admin_options = admin_pool.connect_options();
        if admin_options.get_database() != Some(profile.database.as_str())
            || admin_options.get_username() != profile.user
            || admin_options.get_host() != profile.host
            || admin_options.get_port() != profile.port
        {
            return Err(BackupError::InvalidBackup);
        }
        let admin_control_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect_with(admin_options.as_ref().clone().database("postgres"))
            .await
            .map_err(|_| BackupError::InvalidBackup)?;
        let admin_system: String =
            sqlx::query_scalar("SELECT system_identifier::text FROM pg_control_system()")
                .fetch_one(&admin_control_pool)
                .await
                .map_err(|_| BackupError::InvalidBackup)?;
        admin_control_pool.close().await;
        let control_system: String =
            sqlx::query_scalar("SELECT system_identifier::text FROM pg_control_system()")
                .fetch_one(&control_pool)
                .await
                .map_err(|_| BackupError::InvalidBackup)?;
        if admin_system != control_system {
            return Err(BackupError::InvalidBackup);
        }
        let is_superuser: bool =
            sqlx::query_scalar("SELECT rolsuper FROM pg_roles WHERE rolname=current_user")
                .fetch_one(&control_pool)
                .await
                .map_err(|_| BackupError::InvalidBackup)?;
        if !is_superuser {
            return Err(BackupError::InvalidBackup);
        }
        reconcile_interrupted_restore(&control_pool, &profile, &key_provider)
            .await
            .map_err(|_| BackupError::InvalidBackup)?;
        let target: (String, String, String) = sqlx::query_as(
            "SELECT current_database(),current_user,system_identifier::text FROM pg_control_system()",
        )
        .fetch_one(&admin_pool)
        .await
        .map_err(|_| BackupError::InvalidBackup)?;
        if target.0 != profile.database || target.1 != profile.user || target.2 != control_system {
            return Err(BackupError::InvalidBackup);
        }
        Ok(Self {
            admin_pool,
            control_pool,
            key_provider,
            profile,
            pg_dump,
            pg_restore,
            process_timeout,
        })
    }

    /// Digest of PostgreSQL system identifier plus exact database OID and name.
    pub async fn source_identity_sha256(&self) -> Result<String, BackupError> {
        self.key_provider
            .metadata()
            .await
            .map_err(|_| BackupError::KeyUnavailable)?;
        let (system_identifier, database_oid, database_name): (String, String, String) =
            sqlx::query_as(
                "SELECT system_identifier::text, d.oid::text, d.datname::text \
                 FROM pg_control_system(), pg_database d WHERE d.datname=current_database()",
            )
            .fetch_one(&self.admin_pool)
            .await
            .map_err(|_| BackupError::InvalidBackup)?;
        let bytes = canonical_bytes(&(
            "graphhelm-postgres-database-identity-v1",
            system_identifier,
            database_oid,
            database_name,
        ))?;
        Ok(hex::encode(Sha256::digest(bytes)))
    }

    /// Exports one repeatable-read snapshot through pinned `pg_dump` into an encrypted archive.
    pub async fn backup_to_path(&self, destination: &Path) -> Result<BackupReceipt, BackupError> {
        let operator = self.clone();
        let destination = destination.to_path_buf();
        let cancelled = Arc::new(AtomicBool::new(false));
        let task_cancelled = Arc::clone(&cancelled);
        AbortTaskOnDrop::new(
            tokio::spawn(async move {
                operator
                    .backup_to_path_owned(&destination, task_cancelled)
                    .await
            }),
            cancelled,
        )
        .wait()
        .await
        .map_err(|_| unavailable(UnavailableStage::TaskJoin))?
    }

    async fn backup_to_path_owned(
        &self,
        destination: &Path,
        cancelled: Arc<AtomicBool>,
    ) -> Result<BackupReceipt, BackupError> {
        // #81 item 2: ONE budget for the whole backup; every forward step spends from it.
        let budget = OperationDeadline::new(self.process_timeout);
        let pg_dump = self
            .pg_dump
            .verified_for_use(budget.step(Duration::from_secs(30)))?;
        let mut owned = OwnedTemporary::for_destination(destination)?;
        let mut transaction = self
            .admin_pool
            .begin()
            .await
            .map_err(|_| BackupError::InvalidBackup)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *transaction)
            .await
            .map_err(|_| BackupError::InvalidBackup)?;
        sqlx::query("SET LOCAL statement_timeout='30000'")
            .execute(&mut *transaction)
            .await
            .map_err(|_| BackupError::InvalidBackup)?;
        sqlx::query("SET LOCAL lock_timeout='5000'")
            .execute(&mut *transaction)
            .await
            .map_err(|_| BackupError::InvalidBackup)?;
        let snapshot: String = sqlx::query_scalar("SELECT pg_export_snapshot()")
            .fetch_one(&mut *transaction)
            .await
            .map_err(|_| BackupError::InvalidBackup)?;
        if snapshot.is_empty()
            || snapshot.len() > 128
            || !snapshot
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.'))
        {
            return Err(BackupError::InvalidBackup);
        }
        let identity = source_identity_in(&mut transaction).await?;
        if unexpected_source_object_count(&mut transaction).await? != 0 {
            return Err(BackupError::InvalidBackup);
        }
        if schema_contract_sha256_in(&mut transaction).await? != EXPECTED_SCHEMA_CONTRACT_SHA256 {
            return Err(BackupError::InvalidBackup);
        }
        let actual_privileges = privilege_contract_sha256_in(&mut transaction).await?;
        if !privilege_contract_is_safe(&mut *transaction).await? {
            return Err(BackupError::InvalidBackup);
        }
        validate_restore_domain_bounds(&mut transaction).await?;
        let runtime_role = configured_runtime_role_in(&mut transaction).await?;
        let counts: (i64, i64, i64, i64, i64, i64, i64) = sqlx::query_as(
            "SELECT (SELECT count(*) FROM public.graphhelm_streams), \
             (SELECT count(*) FROM public.graphhelm_checkpoints), \
             (SELECT count(*) FROM public.graphhelm_artifact_refs), \
             (SELECT count(*) FROM public.graphhelm_evidence), \
             (SELECT count(*) FROM public.graphhelm_evidence_tombstones), \
             (SELECT count(*) FROM public.graphhelm_retention_operations WHERE state='finalized'), \
             (SELECT count(*) FROM public.graphhelm_projection_checkpoints)",
        )
        .fetch_one(&mut *transaction)
        .await
        .map_err(|_| BackupError::InvalidBackup)?;
        let count = |value: i64| u64::try_from(value).map_err(|_| BackupError::InvalidBackup);
        let provider = self
            .key_provider
            .metadata()
            .await
            .map_err(|_| BackupError::KeyUnavailable)?;
        let migration_sha256 = [
            crate::INITIAL_MIGRATION,
            crate::RETENTION_MIGRATION,
            crate::PROJECTION_MIGRATION,
        ]
        .into_iter()
        .map(|migration| hex::encode(Sha256::digest(migration.as_bytes())))
        .collect();
        let state_summary_sha256 = tokio::time::timeout(
            budget.step(Duration::from_secs(30)),
            state_summary_connection(&mut transaction),
        )
        .await
        .map_err(|_| BackupError::DeadlineElapsed)??;
        let privilege_summary_sha256 = actual_privileges;
        let source_semantics = database_semantic_contract_in(&mut transaction).await?;
        let mut manifest = BackupManifest::new(
            identity,
            source_semantics,
            3,
            "repository-v1",
            hex::encode(Sha256::digest(include_bytes!(
                "../../../schemas/catalog.json"
            ))),
            migration_sha256,
            BackupCounts {
                stream_heads: count(counts.0)?,
                checkpoints: count(counts.1)?,
                artifact_references: count(counts.2)?,
                evidence: count(counts.3)?,
                tombstones: count(counts.4)?,
                retention_receipts: count(counts.5)?,
                projection_generations: count(counts.6)?,
            },
            self.pg_dump.exact_version.clone(),
            self.pg_dump.sha256_hex(),
            self.pg_restore.exact_version.clone(),
            self.pg_restore.sha256_hex(),
            runtime_role,
            provider.current_revocation_epoch(),
        )?;
        manifest.state_summary_sha256 = state_summary_sha256;
        manifest.privilege_summary_sha256 = privilege_summary_sha256;
        let mut child = self.spawn_dump(&pg_dump, &snapshot)?;
        let pipes = (child.stdout.take(), child.stderr.take());
        let (stdout, stderr) = match pipes {
            (Some(stdout), Some(stderr)) => (stdout, stderr),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(BackupError::InvalidBackup);
            }
        };
        let mut watchdog =
            ProcessWatchdog::start_with_cancellation(child, budget.remaining(), cancelled)?;
        let stderr_reader = std::thread::spawn(move || read_bounded_output(stderr));
        let encrypted = BackupCodec::new(Arc::clone(&self.key_provider))
            .encrypt(&manifest, stdout, owned.file_mut())
            .await;
        if encrypted.is_err() {
            watchdog.terminate();
        }
        let status = watchdog.finish()?;
        let (_, stderr_truncated) = stderr_reader
            .join()
            .map_err(|_| BackupError::InvalidBackup)??;
        transaction
            .rollback()
            .await
            .map_err(|_| BackupError::InvalidBackup)?;
        if !status.success() || stderr_truncated {
            return Err(BackupError::InvalidBackup);
        }
        let receipt = encrypted?;
        owned.publish(destination)?;
        Ok(receipt)
    }

    fn spawn_dump(
        &self,
        executable: &VerifiedTool,
        snapshot: &str,
    ) -> Result<std::process::Child, BackupError> {
        let mut command = dump_command(executable.path(), &self.profile, snapshot);
        configure_process_group(&mut command);
        command.spawn().map_err(|_| BackupError::InvalidBackup)
    }

    /// Restores only into a database that has no GraphHelm or user relations.
    pub async fn restore_from_path(&self, archive: &Path) -> Result<RestoreReceipt, BackupError> {
        let operator = self.clone();
        let archive = archive.to_path_buf();
        let cancelled = Arc::new(AtomicBool::new(false));
        let task_cancelled = Arc::clone(&cancelled);
        AbortTaskOnDrop::new(
            tokio::spawn(async move {
                operator
                    .restore_from_path_owned(&archive, task_cancelled)
                    .await
            }),
            cancelled,
        )
        .wait()
        .await
        .map_err(|_| BackupError::InvalidRestore)?
    }

    async fn restore_from_path_owned(
        &self,
        archive: &Path,
        cancelled: Arc<AtomicBool>,
    ) -> Result<RestoreReceipt, BackupError> {
        if !archive.is_absolute() || !archive.is_file() {
            return Err(BackupError::InvalidRestore);
        }
        if fresh_target_object_count(&self.admin_pool).await? != 0 {
            return Err(BackupError::InvalidRestore);
        }
        // #81 item 2: ONE budget for the whole restore; every forward step spends from it.
        // Cleanup and release are NOT bounded by it (see their own sites): they are
        // compensation, and starving cleanup because the operation elapsed would leak the
        // quarantine database exactly when it most needs removing.
        let budget = OperationDeadline::new(self.process_timeout);
        let pg_restore = self
            .pg_restore
            .verified_for_use(budget.step(Duration::from_secs(30)))?;
        let codec = BackupCodec::new(Arc::clone(&self.key_provider));
        let mut archive_file = File::open(archive).map_err(|_| BackupError::InvalidRestore)?;
        if archive_file
            .metadata()
            .map_err(|_| BackupError::InvalidRestore)?
            .len()
            > MAX_ARCHIVE_BYTES
        {
            return Err(BackupError::LimitExceeded);
        }
        let provider = self
            .key_provider
            .metadata()
            .await
            .map_err(|_| BackupError::KeyUnavailable)?;
        let verified = codec
            .verify_then_decrypt(
                &mut archive_file,
                provider.current_revocation_epoch(),
                std::io::sink(),
            )
            .await?;
        validate_manifest_for_restore(verified.manifest(), &self.pg_dump, &self.pg_restore)?;
        if database_semantic_contract(&self.admin_pool, &self.profile.database).await?
            != verified.manifest().source_database_semantics
        {
            return Err(BackupError::InvalidRestore);
        }
        let target_identity_before_restore = self.source_identity_sha256().await?;
        if target_identity_before_restore == verified.manifest().source_identity_sha256 {
            return Err(BackupError::InvalidRestore);
        }
        if other_database_sessions(&self.admin_pool).await? != 0 {
            return Err(BackupError::InvalidRestore);
        }
        if fresh_target_object_count(&self.admin_pool).await? != 0
            || other_database_sessions(&self.admin_pool).await? != 0
        {
            return Err(BackupError::InvalidRestore);
        }
        let mut application_random = [0_u8; 16];
        getrandom::fill(&mut application_random)
            .map_err(|_| unavailable(UnavailableStage::Random))?;
        let restore_application = format!("graphhelm-restore-{}", hex::encode(application_random));
        application_random.zeroize();
        let mut child = self.spawn_restore(&pg_restore, &restore_application)?;
        let pipes = (child.stdin.take(), child.stdout.take(), child.stderr.take());
        let (stdin, stdout, stderr) = match pipes {
            (Some(stdin), Some(stdout), Some(stderr)) => (stdin, stdout, stderr),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(BackupError::InvalidRestore);
            }
        };
        let mut watchdog = match ProcessWatchdog::start_with_cancellation(
            child,
            budget.remaining(),
            Arc::clone(&cancelled),
        ) {
            Ok(watchdog) => watchdog,
            Err(error) => return Err(error),
        };
        let stdout_reader = std::thread::spawn(move || read_bounded_output(stdout));
        let stderr_reader = std::thread::spawn(move || read_bounded_output(stderr));
        let stream_provider = Arc::clone(&self.key_provider);
        let stream_verified = verified.clone();
        let stream_cancelled = Arc::clone(&cancelled);
        let provider_epoch = provider.current_revocation_epoch();
        let stream = AbortTaskOnDrop::new(
            tokio::spawn(async move {
                let mut stdin = stdin;
                let result = if archive_file.seek(SeekFrom::Start(0)).is_ok() {
                    BackupCodec::new(stream_provider)
                        .decrypt_verified_once(
                            &mut archive_file,
                            provider_epoch,
                            &mut stdin,
                            &stream_verified,
                        )
                        .await
                } else {
                    Err(BackupError::InvalidRestore)
                };
                (result, stdin)
            }),
            stream_cancelled,
        );
        let mut cleanup_guard = RestoreCleanupGuard::new(self.clone());
        let acquired = tokio::time::timeout(
            budget.step(Duration::from_secs(30)),
            self.acquire_target_exclusivity(
                &restore_application,
                cleanup_guard.ownership(),
                &mut watchdog,
                budget,
            ),
        )
        .await;
        if let Err(error) = acquired
            .map_err(|_| BackupError::DeadlineElapsed)
            .and_then(std::convert::identity)
        {
            watchdog.terminate();
            drop(stream);
            let _ = watchdog.finish();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err(error);
        }
        let (streamed, stdin) = stream
            .wait()
            .await
            .map_err(|_| BackupError::InvalidRestore)?;
        if let Err(error) = streamed.as_ref() {
            watchdog.terminate();
            drop(stdin);
            let _ = watchdog.finish();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            cleanup_guard.cleanup().await?;
            return Err(*error);
        }
        // pg_restore may commit its single transaction and disappear between the PID checks in
        // acquisition and this point. Object presence is therefore not itself a failure here;
        // the authenticated semantic verification below must judge committed content before it
        // can ever be released.
        drop(stdin);
        let status = watchdog.finish();
        let stdout_result = stdout_reader
            .join()
            .map_err(|_| BackupError::InvalidRestore);
        let stderr_result = stderr_reader
            .join()
            .map_err(|_| BackupError::InvalidRestore);
        let outcome = async {
            let status = status?;
            let streamed = streamed?;
            let (_, stdout_truncated) = stdout_result??;
            let (_, stderr_truncated) = stderr_result??;
            if streamed != verified || !status.success() || stdout_truncated || stderr_truncated {
                return Err(BackupError::InvalidRestore);
            }
            self.configure_restored_runtime_role(&verified.manifest().runtime_role)
                .await?;
            tokio::time::timeout(
                budget.step(Duration::from_secs(30)),
                self.verify_restored_state(verified.manifest()),
            )
            .await
            .map_err(|_| BackupError::DeadlineElapsed)??;
            if target_user_object_count(&self.admin_pool).await? != 0 {
                return Err(BackupError::InvalidRestore);
            }
            let receipt_provider = self
                .key_provider
                .metadata()
                .await
                .map_err(|_| BackupError::KeyUnavailable)?;
            ensure_provider_epoch(
                verified.manifest().provider_epoch,
                receipt_provider.current_revocation_epoch(),
            )?;
            if receipt_provider != provider {
                return Err(BackupError::InvalidRestore);
            }
            let target_identity = self.source_identity_sha256().await?;
            if target_identity != target_identity_before_restore
                || target_identity == verified.manifest().source_identity_sha256
            {
                return Err(BackupError::InvalidRestore);
            }
            let manifest_sha256 =
                hex::encode(Sha256::digest(canonical_bytes(verified.manifest())?));
            let receipt_bytes = restore_receipt_bytes(
                &verified.manifest().source_identity_sha256,
                &target_identity,
                &manifest_sha256,
            )?;
            let tag = self
                .key_provider
                .authenticate(
                    AuthenticateRequest::new("graphhelm.restore.receipt.v1", receipt_bytes)
                        .map_err(|_| BackupError::KeyUnavailable)?,
                )
                .await
                .map_err(|_| BackupError::KeyUnavailable)?;
            self.key_provider
                .verify(
                    VerifyAuthenticationRequest::new(
                        "graphhelm.restore.receipt.v1",
                        restore_receipt_bytes(
                            &verified.manifest().source_identity_sha256,
                            &target_identity,
                            &manifest_sha256,
                        )?,
                        tag.clone(),
                    )
                    .map_err(|_| BackupError::KeyUnavailable)?,
                )
                .await
                .map_err(|_| BackupError::KeyUnavailable)?;
            let completed_provider = self
                .key_provider
                .metadata()
                .await
                .map_err(|_| BackupError::KeyUnavailable)?;
            if completed_provider != provider || tag.key_id() != provider.key_id() {
                return Err(BackupError::InvalidRestore);
            }
            Ok(RestoreReceipt {
                source_identity_sha256: verified.manifest().source_identity_sha256.clone(),
                target_identity_sha256: target_identity,
                manifest_sha256,
                authentication_tag: tag,
            })
        }
        .await;
        if outcome.is_err() {
            cleanup_guard.cleanup().await?;
        } else {
            cleanup_guard.ownership().authorize_release();
            self.release_target_exclusivity(cleanup_guard.ownership())
                .await?;
            cleanup_guard.disarm();
        }
        outcome
    }

    async fn cleanup_failed_restore(
        &self,
        ownership: &RestoreOwnership,
    ) -> Result<(), BackupError> {
        // #81: deliberately NOT bounded by the operation budget - cleanup is compensation,
        // and an exhausted budget must not starve the step that removes the quarantine.
        tokio::time::timeout(
            self.process_timeout.min(Duration::from_secs(30)),
            self.cleanup_failed_restore_bounded(ownership),
        )
        .await
        .map_err(|_| BackupError::DeadlineElapsed)?
    }

    async fn cleanup_failed_restore_bounded(
        &self,
        ownership: &RestoreOwnership,
    ) -> Result<(), BackupError> {
        match ownership.recovery_action() {
            RestoreRecoveryAction::EnsureClosedForRecovery => {
                self.ensure_target_closed_for_recovery_bounded(ownership)
                    .await
            }
            RestoreRecoveryAction::CleanupFailedRestore => {
                reset_restore_target(
                    &self.control_pool,
                    &self.profile,
                    &ownership.marker()?,
                    &self.key_provider,
                )
                .await
            }
            RestoreRecoveryAction::ReleaseProvedState => {
                self.release_target_exclusivity(ownership).await
            }
            RestoreRecoveryAction::Inactive
            | RestoreRecoveryAction::PreserveForRecovery
            | RestoreRecoveryAction::Complete => Ok(()),
        }
    }

    async fn ensure_target_closed_for_recovery_bounded(
        &self,
        ownership: &RestoreOwnership,
    ) -> Result<(), BackupError> {
        let marker = ownership.marker()?;
        close_restore_target_after_minimal_validation(
            &self.control_pool,
            &self.profile.database,
            &marker,
            marker.target_identity_sha256.as_str(),
        )
        .await?;
        ownership.mark_closed_for_recovery();

        // The close above is already committed. Everything below is read-only, so a missing
        // provider, changed role, or changed principal can reject recovery without reopening an
        // unverified restore target.
        let mut transaction = self
            .control_pool
            .begin()
            .await
            .map_err(|_| BackupError::InvalidRestore)?;
        validate_final_restore_ownership(
            &mut transaction,
            &self.profile.database,
            &marker,
            &self.key_provider,
            marker.target_identity_sha256.as_str(),
        )
        .await?;
        validate_closed_restore_access(&mut transaction, &self.profile.database, &marker).await?;
        transaction
            .commit()
            .await
            .map_err(|_| BackupError::InvalidRestore)
    }

    async fn acquire_target_exclusivity(
        &self,
        application: &str,
        ownership: &RestoreOwnership,
        watchdog: &mut ProcessWatchdog,
        budget: OperationDeadline,
    ) -> Result<(), BackupError> {
        // #81, NOT a `tokio::time::timeout` wrapper: this deadline is hand-rolled, so a
        // grep for the wrapper misses it and the fix has to include it BY HAND. The exit
        // decision lives in `classify_exclusivity` — a pure function, so the split below
        // is unit-testable without a database.
        // `tokio::time::Instant`, not `std::time::Instant`: this is the clock `timeout_at`
        // enforces, so the bound and the classification read the same one. Unpaused -- which is
        // every non-test runtime -- it IS the system clock.
        let deadline = tokio::time::Instant::now() + budget.step(Duration::from_secs(10));
        let admin_pool = &self.admin_pool;
        poll_until_exclusive(deadline, || {
            sqlx::query_scalar(
                "SELECT count(*)::bigint FROM pg_stat_activity \
                 WHERE datid=(SELECT oid FROM pg_database WHERE datname=current_database()) \
                   AND application_name=$1",
            )
            .bind(application)
            .fetch_one(admin_pool)
        })
        .await?;
        let access = database_access_contract(&self.admin_pool).await?;
        let target_identity = self.source_identity_sha256().await?;
        let quarantine = format!("graphhelm_restore_q_{}", &target_identity[..32]);
        let replacement_owner = format!("graphhelm_restore_o_{}", &target_identity[..32]);
        let quarantine_exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname=$1)")
                .bind(&quarantine)
                .fetch_one(&self.control_pool)
                .await
                .map_err(|_| BackupError::InvalidRestore)?;
        if quarantine_exists {
            return Err(BackupError::InvalidRestore);
        }
        let replacement_owner_exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_roles WHERE rolname=$1)")
                .bind(&replacement_owner)
                .fetch_one(&self.control_pool)
                .await
                .map_err(|_| BackupError::InvalidRestore)?;
        if replacement_owner_exists {
            return Err(BackupError::InvalidRestore);
        }
        let database = quoted_identifier(&self.profile.database)?;
        let replacement_owner_quoted = quoted_pg_role_identifier(&replacement_owner)?;
        let mut marker_transaction = self
            .control_pool
            .begin()
            .await
            .map_err(|_| BackupError::InvalidRestore)?;
        lock_restore_catalogs(
            &mut marker_transaction,
            RestoreCatalogLockScope::FullOwnership,
        )
        .await?;
        sqlx::query(AssertSqlSafe(format!(
            "CREATE ROLE {replacement_owner_quoted} NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION NOBYPASSRLS"
        )))
        .execute(&mut *marker_transaction)
        .await
        .map_err(|_| BackupError::InvalidRestore)?;
        let replacement_owner_identity =
            role_identity_in(&mut marker_transaction, &replacement_owner).await?;
        let tag = self
            .key_provider
            .authenticate(
                AuthenticateRequest::new(
                    RESTORE_MARKER_PURPOSE,
                    restore_marker_bytes(
                        &self.profile.database,
                        &target_identity,
                        application,
                        &quarantine,
                        &replacement_owner,
                        &replacement_owner_identity,
                        None,
                        &access,
                    )?,
                )
                .map_err(|_| BackupError::InvalidRestore)?,
            )
            .await
            .map_err(|_| BackupError::KeyUnavailable)?;
        let marker_contract = RestoreMarker {
            format: RESTORE_MARKER_PURPOSE.to_owned(),
            database: self.profile.database.clone(),
            target_identity_sha256: target_identity,
            application: application.to_owned(),
            quarantine,
            replacement_owner,
            replacement_owner_identity_sha256: replacement_owner_identity,
            replacement_identity_sha256: None,
            access,
            key_id: tag.key_id().to_owned(),
            algorithm: tag.algorithm().to_owned(),
            tag_hex: hex::encode(tag.bytes()),
        };
        ownership.set_marker(marker_contract.clone())?;
        let marker =
            serde_json::to_string(&marker_contract).map_err(|_| BackupError::InvalidRestore)?;
        let marker = quoted_literal(&marker)?;
        sqlx::query(AssertSqlSafe(format!(
            "COMMENT ON DATABASE {database} IS {marker}"
        )))
        .execute(&mut *marker_transaction)
        .await
        .map_err(|_| BackupError::InvalidRestore)?;
        ownership.authorize_ensure_closed();
        marker_transaction
            .commit()
            .await
            .map_err(|_| BackupError::InvalidRestore)?;
        sqlx::query(AssertSqlSafe(format!(
            "ALTER DATABASE {database} ALLOW_CONNECTIONS false"
        )))
        .execute(&self.control_pool)
        .await
        .map_err(|_| BackupError::InvalidRestore)?;
        ownership.mark_closed_for_recovery();
        let admin_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&self.admin_pool)
            .await
            .map_err(|_| BackupError::InvalidRestore)?;
        let restore_pids: Vec<i32> = loop {
            let restore_pids = tokio::time::timeout_at(
                deadline,
                sqlx::query_scalar::<_, i32>(
                    "SELECT pid FROM pg_stat_activity WHERE datname=$2 AND application_name=$1 \
                     AND pid<>$3 ORDER BY pid LIMIT 2",
                )
                .bind(application)
                .bind(&self.profile.database)
                .bind(admin_pid)
                .fetch_all(&self.control_pool),
            )
            .await
            .map_err(|_| BackupError::InvalidRestore)?
            .map_err(|_| BackupError::InvalidRestore)?;
            if restore_pids.len() == 1 {
                break restore_pids;
            }
            // A completed pg_restore has no PID but may have committed its single transaction.
            // Release only a stable, proved-empty target after the child itself has been observed
            // exited. The database snapshot can otherwise race pg_restore startup: there may be
            // no session and no objects yet while the child is still about to connect.
            let other_sessions = if restore_pids.is_empty() {
                Some(
                    tokio::time::timeout_at(deadline, other_database_sessions(&self.admin_pool))
                        .await
                        .map_err(|_| BackupError::InvalidRestore)?
                        .map_err(|_| BackupError::InvalidRestore)?,
                )
            } else {
                None
            };
            let target_objects = if other_sessions == Some(0) {
                Some(
                    tokio::time::timeout_at(deadline, fresh_target_object_count(&self.admin_pool))
                        .await
                        .map_err(|_| BackupError::InvalidRestore)?
                        .map_err(|_| BackupError::InvalidRestore)?,
                )
            } else {
                None
            };
            let restore_completed = if restore_pids.is_empty() {
                watchdog.leader_exited()
            } else {
                false
            };
            match classify_post_marker_acquisition(
                restore_pids.len(),
                other_sessions,
                target_objects,
                restore_completed,
            ) {
                PostMarkerAcquisition::ReleaseEmpty => {
                    ownership.authorize_release();
                    self.release_target_exclusivity(ownership).await?;
                    ownership.mark_complete();
                    return Err(BackupError::InvalidRestore);
                }
                PostMarkerAcquisition::VerifyCompleted => {
                    ownership.authorize_cleanup();
                    return Ok(());
                }
                PostMarkerAcquisition::AwaitRestore => {
                    let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                    if remaining.is_zero() {
                        return Err(BackupError::InvalidRestore);
                    }
                    tokio::time::sleep(Duration::from_millis(10).min(remaining)).await;
                    continue;
                }
                PostMarkerAcquisition::PreserveForRecovery => {
                    return Err(BackupError::InvalidRestore);
                }
            }
        };
        let others: i64 = tokio::time::timeout_at(
            deadline,
            sqlx::query_scalar::<_, i64>(
                "SELECT count(*)::bigint FROM pg_stat_activity \
                 WHERE datid=(SELECT oid FROM pg_database WHERE datname=$1) \
                   AND pid<>$2 AND pid<>$3",
            )
            .bind(&self.profile.database)
            .bind(admin_pid)
            .bind(restore_pids[0])
            .fetch_one(&self.control_pool),
        )
        .await
        .map_err(|_| BackupError::InvalidRestore)?
        .map_err(|_| BackupError::InvalidRestore)?;
        if others != 0 {
            return Err(BackupError::InvalidRestore);
        }
        ownership.authorize_cleanup();
        Ok(())
    }

    async fn release_target_exclusivity(
        &self,
        ownership: &RestoreOwnership,
    ) -> Result<(), BackupError> {
        // #81: same compensation exemption as cleanup - release must run even when the
        // operation's budget is spent.
        tokio::time::timeout(
            self.process_timeout.min(Duration::from_secs(30)),
            self.release_target_exclusivity_bounded(ownership),
        )
        .await
        .map_err(|_| BackupError::DeadlineElapsed)?
    }

    async fn release_target_exclusivity_bounded(
        &self,
        ownership: &RestoreOwnership,
    ) -> Result<(), BackupError> {
        let database = quoted_identifier(&self.profile.database)?;
        let access = ownership.access_contract()?;
        let marker = ownership.marker()?;
        let role = quoted_pg_role_identifier(&marker.replacement_owner)?;
        let mut transaction = self
            .control_pool
            .begin()
            .await
            .map_err(|_| BackupError::InvalidRestore)?;
        validate_final_restore_ownership(
            &mut transaction,
            &self.profile.database,
            &marker,
            &self.key_provider,
            marker.target_identity_sha256.as_str(),
        )
        .await?;
        restore_database_access_contract_in(&mut transaction, &self.profile.database, &access)
            .await?;
        validate_closed_restore_access(&mut transaction, &self.profile.database, &marker).await?;
        sqlx::query(AssertSqlSafe(format!(
            "ALTER DATABASE {database} CONNECTION LIMIT {}",
            access.connection_limit
        )))
        .execute(&mut *transaction)
        .await
        .map_err(|_| BackupError::InvalidRestore)?;
        sqlx::query(AssertSqlSafe(format!(
            "ALTER DATABASE {database} ALLOW_CONNECTIONS {}",
            access.allow_connections
        )))
        .execute(&mut *transaction)
        .await
        .map_err(|_| BackupError::InvalidRestore)?;
        sqlx::query(AssertSqlSafe(format!(
            "COMMENT ON DATABASE {database} IS NULL"
        )))
        .execute(&mut *transaction)
        .await
        .map_err(|_| BackupError::InvalidRestore)?;
        sqlx::query(AssertSqlSafe(format!("DROP ROLE {role}")))
            .execute(&mut *transaction)
            .await
            .map_err(|_| BackupError::InvalidRestore)?;
        transaction
            .commit()
            .await
            .map_err(|_| BackupError::InvalidRestore)?;
        Ok(())
    }

    async fn configure_restored_runtime_role(&self, role: &str) -> Result<(), BackupError> {
        sqlx::query(
            "REVOKE ALL ON FUNCTION public.graphhelm_migrations_are_current(bytea,bytea,bytea,bytea) FROM PUBLIC",
        )
        .execute(&self.admin_pool)
        .await
        .map_err(|_| BackupError::InvalidRestore)?;
        sqlx::query(
            "REVOKE ALL ON FUNCTION public.graphhelm_migration_is_current(bytea), \
             public.graphhelm_migrations_are_current(bytea,bytea) FROM PUBLIC",
        )
        .execute(&self.admin_pool)
        .await
        .map_err(|_| BackupError::InvalidRestore)?;
        sqlx::query(
            "REVOKE ALL ON FUNCTION public.graphhelm_configure_runtime_role(name) FROM PUBLIC",
        )
        .execute(&self.admin_pool)
        .await
        .map_err(|_| BackupError::InvalidRestore)?;
        sqlx::query("SELECT public.graphhelm_configure_runtime_role($1::name)")
            .bind(role)
            .execute(&self.admin_pool)
            .await
            .map_err(|_| BackupError::InvalidRestore)?;
        Ok(())
    }

    fn spawn_restore(
        &self,
        executable: &VerifiedTool,
        application: &str,
    ) -> Result<std::process::Child, BackupError> {
        let mut command = restore_command(executable.path(), &self.profile, application);
        configure_process_group(&mut command);
        command.spawn().map_err(|_| BackupError::InvalidRestore)
    }

    async fn verify_restored_state(&self, manifest: &BackupManifest) -> Result<(), BackupError> {
        sqlx::query("SET statement_timeout='30000'")
            .execute(&self.admin_pool)
            .await
            .map_err(|_| BackupError::InvalidRestore)?;
        sqlx::query("SET lock_timeout='5000'")
            .execute(&self.admin_pool)
            .await
            .map_err(|_| BackupError::InvalidRestore)?;
        let result = self.verify_restored_state_bounded(manifest).await;
        let reset_statement = sqlx::query("SET statement_timeout=0")
            .execute(&self.admin_pool)
            .await;
        let reset_lock = sqlx::query("SET lock_timeout=0")
            .execute(&self.admin_pool)
            .await;
        if reset_statement.is_err() || reset_lock.is_err() {
            return Err(BackupError::InvalidRestore);
        }
        result
    }

    async fn verify_restored_state_bounded(
        &self,
        manifest: &BackupManifest,
    ) -> Result<(), BackupError> {
        let schema_contract = schema_contract_sha256(&self.admin_pool).await?;
        if schema_contract != EXPECTED_SCHEMA_CONTRACT_SHA256 {
            return Err(BackupError::InvalidRestore);
        }
        let restored_privileges = privilege_contract_sha256(&self.admin_pool).await?;
        if restored_privileges != manifest.privilege_summary_sha256 {
            return Err(BackupError::InvalidRestore);
        }
        let migrations_current: bool =
            sqlx::query_scalar("SELECT public.graphhelm_migrations_are_current($1,$2,$3,$4)")
                .bind(crate::initial_migration().checksum.as_ref())
                .bind(crate::retention_migration().checksum.as_ref())
                .bind(crate::projection_migration().checksum.as_ref())
                .bind(crate::scope_guard_migration().checksum.as_ref())
                .fetch_one(&self.admin_pool)
                .await
                .map_err(|_| BackupError::InvalidRestore)?;
        let forced_rls: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace \
             WHERE n.nspname='public' AND c.relname LIKE 'graphhelm_%' \
             AND c.relrowsecurity AND c.relforcerowsecurity",
        )
        .fetch_one(&self.admin_pool)
        .await
        .map_err(|_| BackupError::InvalidRestore)?;
        let counts: (i64, i64, i64, i64, i64, i64, i64) = sqlx::query_as(
            "SELECT (SELECT count(*) FROM public.graphhelm_streams), \
             (SELECT count(*) FROM public.graphhelm_checkpoints), \
             (SELECT count(*) FROM public.graphhelm_artifact_refs), \
             (SELECT count(*) FROM public.graphhelm_evidence), \
             (SELECT count(*) FROM public.graphhelm_evidence_tombstones), \
             (SELECT count(*) FROM public.graphhelm_retention_operations WHERE state='finalized'), \
             (SELECT count(*) FROM public.graphhelm_projection_checkpoints)",
        )
        .fetch_one(&self.admin_pool)
        .await
        .map_err(|_| BackupError::InvalidRestore)?;
        let expected = &manifest.counts;
        let mut connection = self
            .admin_pool
            .acquire()
            .await
            .map_err(|_| BackupError::InvalidRestore)?;
        let mut summary_transaction = connection
            .begin()
            .await
            .map_err(|_| BackupError::InvalidRestore)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *summary_transaction)
            .await
            .map_err(|_| BackupError::InvalidRestore)?;
        let restored_summary = state_summary_connection(&mut summary_transaction).await?;
        summary_transaction
            .rollback()
            .await
            .map_err(|_| BackupError::InvalidRestore)?;
        drop(connection);
        if !migrations_current
            || forced_rls != 16
            || counts.0 != expected.stream_heads as i64
            || counts.1 != expected.checkpoints as i64
            || counts.2 != expected.artifact_references as i64
            || counts.3 != expected.evidence as i64
            || counts.4 != expected.tombstones as i64
            || counts.5 != expected.retention_receipts as i64
            || counts.6 != expected.projection_generations as i64
            || restored_summary != manifest.state_summary_sha256
        {
            return Err(BackupError::InvalidRestore);
        }
        let invalid_relations: i64 = sqlx::query_scalar(
            "SELECT (\
             (SELECT count(*) FROM public.graphhelm_evidence_refs r LEFT JOIN public.graphhelm_evidence e \
              USING(workspace_id,project_id,execution_id,evidence_id) WHERE e.evidence_id IS NULL) + \
             (SELECT count(*) FROM public.graphhelm_artifact_refs r LEFT JOIN public.graphhelm_artifacts a \
              USING(workspace_id,project_id,execution_id,artifact_id) WHERE a.artifact_id IS NULL))::bigint",
        )
        .fetch_one(&self.admin_pool)
        .await
        .map_err(|_| BackupError::InvalidRestore)?;
        if invalid_relations != 0 {
            return Err(BackupError::InvalidRestore);
        }
        let verifier = crate::PostgresEventStore::for_admin_verification(
            self.admin_pool.clone(),
            Arc::clone(&self.key_provider),
            configured_runtime_role(&self.admin_pool).await?,
        );
        crate::retention::verify_restored_retention(&verifier)
            .await
            .map_err(|_| BackupError::InvalidRestore)?;
        verify_evidence_records(&self.admin_pool, Arc::clone(&self.key_provider)).await?;
        crate::integrity::verify_restored_checkpoints(&verifier)
            .await
            .map_err(|_| BackupError::InvalidRestore)?;
        let mut stream_cursor: Option<(String, String, String, String)> = None;
        let mut stream_count = 0_u64;
        loop {
            let (workspace_cursor, project_cursor, execution_cursor, stream_cursor_value) =
                stream_cursor.clone().unwrap_or_default();
            let streams: Vec<(String, String, String, String, i64)> = sqlx::query_as(
                "SELECT workspace_id,project_id,execution_id,stream_id,next_sequence \
                 FROM public.graphhelm_streams \
                 WHERE ($1='' OR (workspace_id,project_id,execution_id,stream_id)>($1,$2,$3,$4)) \
                 ORDER BY workspace_id COLLATE \"C\",project_id COLLATE \"C\",execution_id COLLATE \"C\",stream_id COLLATE \"C\" LIMIT 100",
            )
            .bind(&workspace_cursor)
            .bind(&project_cursor)
            .bind(&execution_cursor)
            .bind(&stream_cursor_value)
            .fetch_all(&self.admin_pool)
            .await
            .map_err(|_| BackupError::InvalidRestore)?;
            if streams.is_empty() {
                break;
            }
            stream_count = stream_count
                .checked_add(u64::try_from(streams.len()).map_err(|_| BackupError::LimitExceeded)?)
                .ok_or(BackupError::LimitExceeded)?;
            if stream_count > 10_000_000 {
                return Err(BackupError::LimitExceeded);
            }
            for (workspace, project, execution, stream, next_sequence) in streams {
                let next_cursor = (
                    workspace.clone(),
                    project.clone(),
                    execution.clone(),
                    stream.clone(),
                );
                let scope = RepositoryScope::new(
                    WorkspaceId::parse(&workspace).map_err(|_| BackupError::InvalidRestore)?,
                    ProjectId::parse(&project).map_err(|_| BackupError::InvalidRestore)?,
                    if execution.is_empty() {
                        None
                    } else {
                        Some(
                            ExecutionId::parse(&execution)
                                .map_err(|_| BackupError::InvalidRestore)?,
                        )
                    },
                );
                let event_count = u64::try_from(next_sequence)
                    .map_err(|_| BackupError::InvalidRestore)?
                    .checked_sub(1)
                    .ok_or(BackupError::InvalidRestore)?;
                for (start_sequence, count) in verification_ranges(event_count) {
                    verifier
                        .verify_range(
                            VerifyRangeRequest::new(
                                scope.clone(),
                                stream.clone(),
                                start_sequence,
                                count,
                            )
                            .map_err(|_| BackupError::InvalidRestore)?,
                        )
                        .await
                        .map_err(|_| BackupError::InvalidRestore)?;
                }
                stream_cursor = Some(next_cursor);
            }
        }
        verify_active_projections(&verifier).await?;
        Ok(())
    }
}

fn dump_command(
    executable: &Path,
    profile: &DatabaseProcessProfile,
    snapshot: &str,
) -> std::process::Command {
    use std::process::{Command, Stdio};
    let mut command = Command::new(executable);
    command
        .env_clear()
        .env("PGHOST", &profile.host)
        .env("PGPORT", profile.port.to_string())
        .env("PGUSER", &profile.user)
        .env("PGDATABASE", &profile.database)
        .env("PGPASSFILE", &profile.passfile)
        .args([
            "--format=custom",
            "--no-owner",
            "--no-privileges",
            "--snapshot",
            snapshot,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    if let Some(system_root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", system_root);
    }
    command
}

fn restore_command(
    executable: &Path,
    profile: &DatabaseProcessProfile,
    application: &str,
) -> std::process::Command {
    use std::process::{Command, Stdio};
    let mut command = Command::new(executable);
    command
        .env_clear()
        .env("PGHOST", &profile.host)
        .env("PGPORT", profile.port.to_string())
        .env("PGUSER", &profile.user)
        .env("PGDATABASE", &profile.database)
        .env("PGPASSFILE", &profile.passfile)
        .env("PGAPPNAME", application)
        .args([
            "--exit-on-error",
            "--single-transaction",
            "--no-owner",
            "--no-privileges",
            "--dbname",
            &profile.database,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    if let Some(system_root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", system_root);
    }
    command
}

fn verification_ranges(event_count: u64) -> impl Iterator<Item = (u64, u32)> {
    (0..event_count).step_by(100_000).map(move |offset| {
        let remaining = event_count - offset;
        (
            offset + 1,
            u32::try_from(remaining.min(100_000)).expect("bounded verification range"),
        )
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum RestoreRecoveryAction {
    Inactive = 0,
    EnsureClosedForRecovery = 1,
    PreserveForRecovery = 2,
    ReleaseProvedState = 3,
    CleanupFailedRestore = 4,
    Complete = 5,
}

impl RestoreRecoveryAction {
    const fn requires_automatic_compensation(self) -> bool {
        matches!(
            self,
            Self::EnsureClosedForRecovery | Self::ReleaseProvedState | Self::CleanupFailedRestore
        )
    }
}

#[derive(Default)]
struct RestoreOwnership {
    recovery_action: AtomicU8,
    marker: Mutex<Option<RestoreMarker>>,
}

impl RestoreOwnership {
    fn set_marker(&self, marker: RestoreMarker) -> Result<(), BackupError> {
        let mut stored = self
            .marker
            .lock()
            .map_err(|_| BackupError::InvalidRestore)?;
        if stored.is_some() {
            return Err(BackupError::InvalidRestore);
        }
        *stored = Some(marker);
        Ok(())
    }

    fn access_contract(&self) -> Result<DatabaseAccessContract, BackupError> {
        Ok(self.marker()?.access)
    }

    fn marker(&self) -> Result<RestoreMarker, BackupError> {
        self.marker
            .lock()
            .map_err(|_| BackupError::InvalidRestore)?
            .clone()
            .ok_or(BackupError::InvalidRestore)
    }

    fn recovery_action(&self) -> RestoreRecoveryAction {
        match self.recovery_action.load(Ordering::Acquire) {
            0 => RestoreRecoveryAction::Inactive,
            1 => RestoreRecoveryAction::EnsureClosedForRecovery,
            2 => RestoreRecoveryAction::PreserveForRecovery,
            3 => RestoreRecoveryAction::ReleaseProvedState,
            4 => RestoreRecoveryAction::CleanupFailedRestore,
            5 => RestoreRecoveryAction::Complete,
            _ => RestoreRecoveryAction::EnsureClosedForRecovery,
        }
    }

    fn authorize_ensure_closed(&self) {
        self.set_recovery_action(RestoreRecoveryAction::EnsureClosedForRecovery);
    }

    fn mark_closed_for_recovery(&self) {
        self.set_recovery_action(RestoreRecoveryAction::PreserveForRecovery);
    }

    fn authorize_release(&self) {
        self.set_recovery_action(RestoreRecoveryAction::ReleaseProvedState);
    }

    fn authorize_cleanup(&self) {
        self.set_recovery_action(RestoreRecoveryAction::CleanupFailedRestore);
    }

    fn mark_complete(&self) {
        self.set_recovery_action(RestoreRecoveryAction::Complete);
    }

    fn set_recovery_action(&self, action: RestoreRecoveryAction) {
        self.recovery_action.store(action as u8, Ordering::Release);
    }
}

trait RestoreDropCompensator {
    fn compensate_on_drop(self, ownership: Arc<RestoreOwnership>);
}

impl RestoreDropCompensator for PostgresBackupOperator {
    fn compensate_on_drop(self, ownership: Arc<RestoreOwnership>) {
        let cleanup = std::thread::spawn(move || {
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            let _ = runtime.block_on(self.cleanup_failed_restore(&ownership));
        });
        let _ = cleanup.join();
    }
}

struct RestoreCleanupGuard<C: RestoreDropCompensator = PostgresBackupOperator> {
    operator: Option<C>,
    ownership: Arc<RestoreOwnership>,
}

impl<C: RestoreDropCompensator> RestoreCleanupGuard<C> {
    fn with_compensator(operator: C) -> Self {
        Self {
            operator: Some(operator),
            ownership: Arc::new(RestoreOwnership::default()),
        }
    }

    fn ownership(&self) -> &RestoreOwnership {
        &self.ownership
    }

    fn disarm(&mut self) {
        self.ownership.mark_complete();
        self.operator.take();
    }
}

impl RestoreCleanupGuard<PostgresBackupOperator> {
    fn new(operator: PostgresBackupOperator) -> Self {
        Self::with_compensator(operator)
    }

    async fn cleanup(&mut self) -> Result<(), BackupError> {
        let action = self.ownership.recovery_action();
        if !action.requires_automatic_compensation() {
            self.operator.take();
            return Ok(());
        }
        let operator = self.operator.as_ref().ok_or(BackupError::InvalidRestore)?;
        operator.cleanup_failed_restore(&self.ownership).await?;
        self.disarm();
        Ok(())
    }
}

impl<C: RestoreDropCompensator> Drop for RestoreCleanupGuard<C> {
    fn drop(&mut self) {
        let Some(operator) = self.operator.take() else {
            return;
        };
        let action = self.ownership.recovery_action();
        if !action.requires_automatic_compensation() {
            return;
        }
        let ownership = Arc::clone(&self.ownership);
        operator.compensate_on_drop(ownership);
    }
}

async fn verify_active_projections(
    verifier: &crate::PostgresEventStore,
) -> Result<(), BackupError> {
    let mut cursor: Option<(String, String, String, String, String, i32)> = None;
    let mut total = 0_u64;
    loop {
        let (
            workspace_cursor,
            project_cursor,
            execution_cursor,
            stream_cursor,
            name_cursor,
            version_cursor,
        ) = cursor.clone().unwrap_or_default();
        let active: Vec<(String, String, String, String, String, i32)> = sqlx::query_as(
            "SELECT workspace_id,project_id,execution_id,stream_id,projection_name,projection_version \
             FROM public.graphhelm_projection_active \
             WHERE ($1='' OR (workspace_id,project_id,execution_id,stream_id,projection_name,projection_version)>($1,$2,$3,$4,$5,$6)) \
             ORDER BY workspace_id COLLATE \"C\",project_id COLLATE \"C\",execution_id COLLATE \"C\",stream_id COLLATE \"C\",projection_name COLLATE \"C\",projection_version LIMIT 100",
        )
        .bind(&workspace_cursor)
        .bind(&project_cursor)
        .bind(&execution_cursor)
        .bind(&stream_cursor)
        .bind(&name_cursor)
        .bind(version_cursor)
        .fetch_all(verifier.pool())
        .await
        .map_err(|_| BackupError::InvalidRestore)?;
        if active.is_empty() {
            break;
        }
        total = total
            .checked_add(u64::try_from(active.len()).map_err(|_| BackupError::LimitExceeded)?)
            .ok_or(BackupError::LimitExceeded)?;
        if total > 10_000_000 {
            return Err(BackupError::LimitExceeded);
        }
        for (workspace, project, execution, stream, name, version) in active {
            let next_cursor = (
                workspace.clone(),
                project.clone(),
                execution.clone(),
                stream.clone(),
                name.clone(),
                version,
            );
            let scope = RepositoryScope::new(
                WorkspaceId::parse(workspace).map_err(|_| BackupError::InvalidRestore)?,
                ProjectId::parse(project).map_err(|_| BackupError::InvalidRestore)?,
                if execution.is_empty() {
                    None
                } else {
                    Some(ExecutionId::parse(execution).map_err(|_| BackupError::InvalidRestore)?)
                },
            );
            let version = u32::try_from(version).map_err(|_| BackupError::InvalidRestore)?;
            let stored = ProjectionRepository::load_active(
                verifier,
                scope.clone(),
                stream.clone(),
                name.clone(),
                version,
            )
            .await
            .map_err(|_| BackupError::InvalidRestore)?
            .ok_or(BackupError::InvalidRestore)?;
            let mut rebuilt = ProjectionGeneration::new(
                scope.clone(),
                stream.clone(),
                name,
                version,
                stored.watermark().generation(),
            )
            .map_err(|_| BackupError::InvalidRestore)?;
            let mut start = ReadStart::Beginning;
            loop {
                let page = verifier
                    .read_stream(
                        ReadStreamRequest::new(scope.clone(), stream.clone(), start, 1_000)
                            .map_err(|_| BackupError::InvalidRestore)?,
                    )
                    .await
                    .map_err(|_| BackupError::InvalidRestore)?;
                if page.events.is_empty() {
                    break;
                }
                rebuilt
                    .apply_page(&page.events)
                    .map_err(|_| BackupError::InvalidRestore)?;
                match page.next_cursor {
                    Some(cursor) => start = ReadStart::Cursor(cursor),
                    None => break,
                }
            }
            if rebuilt != stored {
                return Err(BackupError::InvalidRestore);
            }
            cursor = Some(next_cursor);
        }
    }
    Ok(())
}

async fn target_user_object_count(pool: &PgPool) -> Result<i64, BackupError> {
    let unexpected = unexpected_database_objects(pool).await?;
    let graphhelm_relations: i64 = sqlx::query_scalar(
        "SELECT count(*)::bigint FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace \
         WHERE n.nspname='public' AND (c.relname LIKE 'graphhelm\\_%' ESCAPE '\\' \
           OR c.relname IN ('_sqlx_migrations','_sqlx_migrations_pkey'))",
    )
    .fetch_one(pool)
    .await
    .map_err(|_| BackupError::InvalidRestore)?;
    if graphhelm_relations != 0 && !expected_relation_contract(pool).await? {
        return Ok(unexpected.saturating_add(1));
    }
    Ok(unexpected)
}

async fn fresh_target_object_count(pool: &PgPool) -> Result<i64, BackupError> {
    sqlx::query_scalar(FRESH_TARGET_OBJECTS_SQL)
        .fetch_one(pool)
        .await
        .map_err(|_| BackupError::InvalidRestore)
}

async fn unexpected_database_objects<'e, E>(executor: E) -> Result<i64, BackupError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let unexpected: i64 = sqlx::query_scalar(UNEXPECTED_DATABASE_OBJECTS_SQL)
        .fetch_one(executor)
        .await
        .map_err(|_| BackupError::InvalidRestore)?;
    Ok(unexpected)
}

async fn expected_relation_contract<'e, E>(executor: E) -> Result<bool, BackupError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let (count, invalid): (i64, i64) = sqlx::query_as(
        "SELECT count(*)::bigint, \
         count(*) FILTER (WHERE NOT ((c.relname='_sqlx_migrations' AND c.relkind='r') \
          OR (c.relname='_sqlx_migrations_pkey' AND c.relkind='i') \
          OR (c.relname LIKE 'graphhelm\\_%_pkey' ESCAPE '\\' AND c.relkind='i') \
          OR (c.relname LIKE 'graphhelm\\_%_key' ESCAPE '\\' AND c.relkind='i') \
          OR (c.relname LIKE 'graphhelm\\_%_key1' ESCAPE '\\' AND c.relkind='i') \
          OR (c.relname LIKE 'graphhelm\\_%_key2' ESCAPE '\\' AND c.relkind='i') \
          OR (c.relname IN (SELECT unnest($1::text[])) AND c.relkind='r')))::bigint \
         FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public' \
         AND (c.relname LIKE 'graphhelm\\_%' ESCAPE '\\' OR c.relname IN ('_sqlx_migrations','_sqlx_migrations_pkey'))",
    )
    .bind(STATE_TABLES)
    .fetch_one(executor)
    .await
    .map_err(|_| BackupError::InvalidRestore)?;
    Ok(count == EXPECTED_RELATION_COUNT && invalid == 0)
}

async fn unexpected_source_object_count(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<i64, BackupError> {
    let unexpected: i64 = sqlx::query_scalar(UNEXPECTED_DATABASE_OBJECTS_SQL)
        .fetch_one(&mut **transaction)
        .await
        .map_err(|_| BackupError::InvalidBackup)?;
    if !expected_relation_contract(&mut **transaction).await? {
        return Ok(unexpected.saturating_add(1));
    }
    Ok(unexpected)
}

async fn other_database_sessions(pool: &PgPool) -> Result<i64, BackupError> {
    sqlx::query_scalar(
        "SELECT count(*)::bigint FROM pg_stat_activity \
         WHERE datid=(SELECT oid FROM pg_database WHERE datname=current_database()) \
           AND pid<>pg_backend_pid()",
    )
    .fetch_one(pool)
    .await
    .map_err(|_| BackupError::InvalidRestore)
}

fn ensure_provider_epoch(expected: u64, current: u64) -> Result<(), BackupError> {
    if expected == current {
        Ok(())
    } else {
        Err(BackupError::InvalidRestore)
    }
}

async fn source_identity_in(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<String, BackupError> {
    let values: (String, String, String) = sqlx::query_as(
        "SELECT system_identifier::text, d.oid::text, d.datname::text \
         FROM pg_control_system(), pg_database d WHERE d.datname=current_database()",
    )
    .fetch_one(&mut **transaction)
    .await
    .map_err(|_| BackupError::InvalidBackup)?;
    Ok(hex::encode(Sha256::digest(canonical_bytes(&(
        "graphhelm-postgres-database-identity-v1",
        values.0,
        values.1,
        values.2,
    ))?)))
}

async fn verify_profile_endpoint(profile: &DatabaseProcessProfile) -> Result<PgPool, BackupError> {
    let passfile = read_profile_passfile(&profile.passfile)?;
    if passfile.lines().count() != 1 {
        return Err(BackupError::InvalidBackup);
    }
    let fields = passfile
        .trim_end_matches(['\r', '\n'])
        .split(':')
        .collect::<Vec<_>>();
    if fields.len() != 5
        || fields[0] != profile.host
        || fields[1] != profile.port.to_string()
        || fields[2] != "*"
        || fields[3] != profile.user
        || fields[4].is_empty()
        || fields[4].len() > 1024
    {
        return Err(BackupError::InvalidBackup);
    }
    let control_options = PgConnectOptions::new()
        .host(&profile.host)
        .port(profile.port)
        .username(&profile.user)
        .password(fields[4])
        .database("postgres");
    let control_pool = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(10))
        .connect_with(control_options)
        .await
        .map_err(|_| BackupError::InvalidBackup)?;
    let control: (String, String) =
        sqlx::query_as("SELECT current_database(),current_user FROM pg_control_system()")
            .fetch_one(&control_pool)
            .await
            .map_err(|_| BackupError::InvalidBackup)?;
    if control.0 != "postgres" || control.1 != profile.user {
        control_pool.close().await;
        return Err(BackupError::InvalidBackup);
    }
    Ok(control_pool)
}

async fn reconcile_interrupted_restore(
    control_pool: &PgPool,
    profile: &DatabaseProcessProfile,
    key_provider: &Arc<dyn KeyProvider>,
) -> Result<(), BackupError> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT datname,shobj_description(oid,'pg_database') FROM pg_database \
         WHERE (datname=$1 OR datname LIKE 'graphhelm_restore_q_%') \
           AND shobj_description(oid,'pg_database') IS NOT NULL LIMIT 1025",
    )
    .bind(&profile.database)
    .fetch_all(control_pool)
    .await
    .map_err(|_| BackupError::InvalidBackup)?;
    if rows.len() > 1024 {
        return Err(BackupError::InvalidBackup);
    }
    let mut candidates = Vec::new();
    for (holder, raw) in rows {
        if raw.len() > 16 * 1024 || !raw.contains(RESTORE_MARKER_PURPOSE) {
            if holder == profile.database {
                return Err(BackupError::InvalidBackup);
            }
            continue;
        }
        let marker: RestoreMarker = match serde_json::from_str(&raw) {
            Ok(marker) => marker,
            Err(_) if holder != profile.database => continue,
            Err(_) => return Err(BackupError::InvalidBackup),
        };
        if marker.format == RESTORE_MARKER_PURPOSE && marker.database == profile.database {
            candidates.push((holder, marker));
        } else if holder == profile.database {
            return Err(BackupError::InvalidBackup);
        }
    }
    if candidates.is_empty() {
        return Ok(());
    }
    let marker = candidates[0].1.clone();
    if candidates.iter().any(|(_, candidate)| candidate != &marker) {
        return Err(BackupError::InvalidBackup);
    }
    if marker.quarantine.len() > 63
        || !marker.quarantine.starts_with("graphhelm_restore_q_")
        || !marker
            .quarantine
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(BackupError::InvalidBackup);
    }
    if marker.replacement_owner.len() > 63
        || !marker.replacement_owner.starts_with("graphhelm_restore_o_")
        || !marker
            .replacement_owner
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(BackupError::InvalidBackup);
    }
    if !valid_sha256(&marker.replacement_owner_identity_sha256) {
        return Err(BackupError::InvalidBackup);
    }
    if marker
        .replacement_identity_sha256
        .as_deref()
        .is_some_and(|identity| {
            identity.len() != 64
                || !identity
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        })
    {
        return Err(BackupError::InvalidBackup);
    }
    key_provider
        .verify(
            VerifyAuthenticationRequest::new(
                RESTORE_MARKER_PURPOSE,
                restore_marker_bytes(
                    &marker.database,
                    &marker.target_identity_sha256,
                    &marker.application,
                    &marker.quarantine,
                    &marker.replacement_owner,
                    &marker.replacement_owner_identity_sha256,
                    marker.replacement_identity_sha256.as_deref(),
                    &marker.access,
                )?,
                AuthenticationTag::new(
                    marker.key_id.clone(),
                    &marker.algorithm,
                    hex::decode(&marker.tag_hex).map_err(|_| BackupError::InvalidBackup)?,
                )
                .map_err(|_| BackupError::InvalidBackup)?,
            )
            .map_err(|_| BackupError::InvalidBackup)?,
        )
        .await
        .map_err(|_| BackupError::InvalidBackup)?;
    let role_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_roles WHERE rolname=$1)")
            .bind(&marker.replacement_owner)
            .fetch_one(control_pool)
            .await
            .map_err(|_| BackupError::InvalidBackup)?;
    if !role_exists {
        let completed_with_residual_quarantine = marker
            .replacement_identity_sha256
            .as_deref()
            .is_some_and(|_| candidates.len() == 1 && candidates[0].0 == marker.quarantine);
        if completed_with_residual_quarantine {
            let replacement_identity = marker
                .replacement_identity_sha256
                .as_deref()
                .ok_or(BackupError::InvalidBackup)?;
            let quarantine_identity =
                database_identity_by_name(control_pool, &marker.quarantine, &profile.database)
                    .await?;
            let target_identity =
                database_identity_by_name(control_pool, &profile.database, &profile.database)
                    .await?;
            let target_access =
                database_access_contract_by_name(control_pool, &profile.database).await?;
            if quarantine_identity == marker.target_identity_sha256
                && target_identity == replacement_identity
                && target_access == marker.access
                && database_marker(control_pool, &profile.database)
                    .await?
                    .is_none()
            {
                return Ok(());
            }
        }
        return Err(BackupError::InvalidBackup);
    }
    validate_restore_owner_role(control_pool, &marker.replacement_owner)
        .await
        .map_err(|_| BackupError::InvalidBackup)?;
    if role_identity(control_pool, &marker.replacement_owner)
        .await
        .map_err(|_| BackupError::InvalidBackup)?
        != marker.replacement_owner_identity_sha256
    {
        return Err(BackupError::InvalidBackup);
    }
    for (holder, _) in &candidates {
        let identity = database_identity_by_name(control_pool, holder, &profile.database).await?;
        if holder == &marker.quarantine {
            if identity != marker.target_identity_sha256 {
                return Err(BackupError::InvalidBackup);
            }
        } else if holder == &profile.database {
            let expected = marker
                .replacement_identity_sha256
                .as_deref()
                .unwrap_or(&marker.target_identity_sha256);
            if identity != expected {
                return Err(BackupError::InvalidBackup);
            }
        } else {
            return Err(BackupError::InvalidBackup);
        }
    }
    reset_restore_target(control_pool, profile, &marker, key_provider).await?;
    Ok(())
}

async fn database_identity_by_name(
    control_pool: &PgPool,
    lookup_name: &str,
    bound_name: &str,
) -> Result<String, BackupError> {
    let values: (String, String) = sqlx::query_as(
        "SELECT system_identifier::text,d.oid::text FROM pg_control_system(),pg_database d \
         WHERE d.datname=$1",
    )
    .bind(lookup_name)
    .fetch_one(control_pool)
    .await
    .map_err(|_| BackupError::InvalidBackup)?;
    Ok(hex::encode(Sha256::digest(canonical_bytes(&(
        "graphhelm-postgres-database-identity-v1",
        values.0,
        values.1,
        bound_name,
    ))?)))
}

async fn validate_restore_owner_role(
    control_pool: &PgPool,
    role_name: &str,
) -> Result<(), BackupError> {
    let state: Option<(bool, bool, bool, bool, bool, bool)> = sqlx::query_as(
        "SELECT rolcanlogin,rolsuper,rolcreatedb,rolcreaterole,rolreplication,rolbypassrls \
         FROM pg_roles WHERE rolname=$1",
    )
    .bind(role_name)
    .fetch_optional(control_pool)
    .await
    .map_err(|_| BackupError::InvalidRestore)?;
    match state {
        Some((false, false, false, false, false, false)) => Ok(()),
        _ => Err(BackupError::InvalidRestore),
    }
}

async fn role_identity_in(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    role_name: &str,
) -> Result<String, BackupError> {
    let values: (String, String) = sqlx::query_as(
        "SELECT system_identifier::text,r.oid::text FROM pg_control_system(),pg_roles r \
         WHERE r.rolname=$1",
    )
    .bind(role_name)
    .fetch_one(&mut **transaction)
    .await
    .map_err(|_| BackupError::InvalidRestore)?;
    Ok(hex::encode(Sha256::digest(canonical_bytes(&(
        "graphhelm-restore-owner-role-v1",
        values.0,
        values.1,
        role_name,
    ))?)))
}

async fn role_identity(control_pool: &PgPool, role_name: &str) -> Result<String, BackupError> {
    let mut transaction = control_pool
        .begin()
        .await
        .map_err(|_| BackupError::InvalidRestore)?;
    let identity = role_identity_in(&mut transaction, role_name).await?;
    transaction
        .rollback()
        .await
        .map_err(|_| BackupError::InvalidRestore)?;
    Ok(identity)
}

async fn database_access_contract(pool: &PgPool) -> Result<DatabaseAccessContract, BackupError> {
    let database_name: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(pool)
        .await
        .map_err(|_| BackupError::InvalidRestore)?;
    database_access_contract_by_name(pool, &database_name).await
}

async fn database_access_contract_by_name(
    pool: &PgPool,
    database_name: &str,
) -> Result<DatabaseAccessContract, BackupError> {
    let (owner, allow_connections, connection_limit): (String, bool, i32) = sqlx::query_as(
        "SELECT owner.rolname,d.datallowconn,d.datconnlimit FROM pg_database d \
         JOIN pg_roles owner ON owner.oid=d.datdba WHERE d.datname=$1",
    )
    .bind(database_name)
    .fetch_one(pool)
    .await
    .map_err(|_| BackupError::InvalidRestore)?;
    let rows: Vec<RawDatabaseAclRow> = sqlx::query_as(
        "SELECT grantor.rolname,grantee.rolname,acl.grantee=0,acl.privilege_type::text,acl.is_grantable \
         FROM pg_database d \
         CROSS JOIN LATERAL aclexplode(COALESCE(d.datacl,acldefault('d',d.datdba))) acl \
         LEFT JOIN pg_roles grantor ON grantor.oid=acl.grantor \
         LEFT JOIN pg_roles grantee ON grantee.oid=acl.grantee \
         WHERE d.datname=$1 \
         ORDER BY COALESCE(grantee.rolname,'') COLLATE \"C\",acl.privilege_type COLLATE \"C\",acl.is_grantable,grantor.rolname COLLATE \"C\"",
    )
    .bind(database_name)
    .fetch_all(pool)
    .await
    .map_err(|_| BackupError::InvalidRestore)?;
    let mut acl = Vec::with_capacity(rows.len());
    for (grantor, grantee, is_public, privilege, grantable) in rows {
        let grantor = grantor.ok_or(BackupError::InvalidRestore)?;
        if is_public == grantee.is_some() {
            return Err(BackupError::InvalidRestore);
        }
        acl.push(DatabaseAclEntry {
            grantor,
            grantee,
            privilege,
            grantable,
        });
    }
    let principal_identities = database_principal_identities(pool, &owner, &acl).await?;
    let contract = DatabaseAccessContract {
        owner,
        principal_identities,
        allow_connections,
        connection_limit,
        semantics: database_semantic_contract(pool, database_name).await?,
        acl,
    };
    validate_database_access_contract(&contract)?;
    Ok(contract)
}

async fn database_access_contract_by_name_in(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    database_name: &str,
) -> Result<DatabaseAccessContract, BackupError> {
    let (owner, allow_connections, connection_limit): (String, bool, i32) = sqlx::query_as(
        "SELECT owner.rolname,d.datallowconn,d.datconnlimit FROM pg_database d \
         JOIN pg_roles owner ON owner.oid=d.datdba WHERE d.datname=$1",
    )
    .bind(database_name)
    .fetch_one(&mut **transaction)
    .await
    .map_err(|_| BackupError::InvalidRestore)?;
    let rows: Vec<RawDatabaseAclRow> = sqlx::query_as(
        "SELECT grantor.rolname,grantee.rolname,acl.grantee=0,acl.privilege_type::text,acl.is_grantable \
         FROM pg_database d \
         CROSS JOIN LATERAL aclexplode(COALESCE(d.datacl,acldefault('d',d.datdba))) acl \
         LEFT JOIN pg_roles grantor ON grantor.oid=acl.grantor \
         LEFT JOIN pg_roles grantee ON grantee.oid=acl.grantee \
         WHERE d.datname=$1 \
         ORDER BY COALESCE(grantee.rolname,'') COLLATE \"C\",acl.privilege_type COLLATE \"C\",acl.is_grantable,grantor.rolname COLLATE \"C\"",
    )
    .bind(database_name)
    .fetch_all(&mut **transaction)
    .await
    .map_err(|_| BackupError::InvalidRestore)?;
    let mut acl = Vec::with_capacity(rows.len());
    for (grantor, grantee, is_public, privilege, grantable) in rows {
        let grantor = grantor.ok_or(BackupError::InvalidRestore)?;
        if is_public == grantee.is_some() {
            return Err(BackupError::InvalidRestore);
        }
        acl.push(DatabaseAclEntry {
            grantor,
            grantee,
            privilege,
            grantable,
        });
    }
    let semantics: (
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
    ) = sqlx::query_as(
        "SELECT pg_encoding_to_char(encoding)::text,datlocprovider::text,datcollate::text,datctype::text, \
         COALESCE(to_jsonb(pg_database)->>'datlocale',to_jsonb(pg_database)->>'daticulocale'), \
         daticurules::text,datcollversion::text FROM pg_database WHERE datname=$1",
    )
    .bind(database_name)
    .fetch_one(&mut **transaction)
    .await
    .map_err(|_| BackupError::InvalidRestore)?;
    let principal_identities = database_principal_identities_in(transaction, &owner, &acl).await?;
    let contract = DatabaseAccessContract {
        owner,
        principal_identities,
        allow_connections,
        connection_limit,
        semantics: DatabaseSemanticIdentity {
            encoding: semantics.0,
            locale_provider: semantics.1,
            collate: semantics.2,
            ctype: semantics.3,
            icu_locale: semantics.4,
            icu_rules: semantics.5,
            collation_version: semantics.6,
        },
        acl,
    };
    validate_database_access_contract(&contract)?;
    Ok(contract)
}

fn database_principal_names(owner: &str, acl: &[DatabaseAclEntry]) -> Vec<String> {
    let mut names = acl
        .iter()
        .flat_map(|entry| {
            std::iter::once(entry.grantor.clone()).chain(entry.grantee.iter().cloned())
        })
        .chain(std::iter::once(owner.to_owned()))
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    names
}

async fn database_principal_identities(
    pool: &PgPool,
    owner: &str,
    acl: &[DatabaseAclEntry],
) -> Result<Vec<DatabasePrincipalIdentity>, BackupError> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(|_| BackupError::InvalidRestore)?;
    let identities = database_principal_identities_in(&mut transaction, owner, acl).await?;
    transaction
        .rollback()
        .await
        .map_err(|_| BackupError::InvalidRestore)?;
    Ok(identities)
}

async fn database_principal_identities_in(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    owner: &str,
    acl: &[DatabaseAclEntry],
) -> Result<Vec<DatabasePrincipalIdentity>, BackupError> {
    let system_identifier: String =
        sqlx::query_scalar("SELECT system_identifier::text FROM pg_control_system()")
            .fetch_one(&mut **transaction)
            .await
            .map_err(|_| BackupError::InvalidRestore)?;
    let mut identities = Vec::new();
    for name in database_principal_names(owner, acl) {
        let oid: String = sqlx::query_scalar("SELECT oid::text FROM pg_roles WHERE rolname=$1")
            .bind(&name)
            .fetch_one(&mut **transaction)
            .await
            .map_err(|_| BackupError::InvalidRestore)?;
        identities.push(DatabasePrincipalIdentity {
            identity_sha256: hex::encode(Sha256::digest(canonical_bytes(&(
                "graphhelm-postgres-role-identity-v1",
                &system_identifier,
                oid,
                &name,
            ))?)),
            name,
        });
    }
    Ok(identities)
}

async fn database_semantic_contract(
    pool: &PgPool,
    database_name: &str,
) -> Result<DatabaseSemanticIdentity, BackupError> {
    let contract: DatabaseSemanticIdentity = sqlx::query_as::<
        _,
        (
            String,
            String,
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
        ),
    >(
        "SELECT pg_encoding_to_char(encoding)::text,datlocprovider::text,datcollate::text,datctype::text, \
         COALESCE(to_jsonb(pg_database)->>'datlocale',to_jsonb(pg_database)->>'daticulocale'), \
         daticurules::text,datcollversion::text FROM pg_database WHERE datname=$1",
    )
    .bind(database_name)
    .fetch_one(pool)
    .await
    .map_err(|_| BackupError::InvalidRestore)
    .map(|row| DatabaseSemanticIdentity {
        encoding: row.0,
        locale_provider: row.1,
        collate: row.2,
        ctype: row.3,
        icu_locale: row.4,
        icu_rules: row.5,
        collation_version: row.6,
    })?;
    validate_database_semantic_contract(&contract)?;
    Ok(contract)
}

async fn database_semantic_contract_in(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<DatabaseSemanticIdentity, BackupError> {
    let row: (
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
    ) = sqlx::query_as(
        "SELECT pg_encoding_to_char(encoding)::text,datlocprovider::text,datcollate::text,datctype::text, \
         COALESCE(to_jsonb(pg_database)->>'datlocale',to_jsonb(pg_database)->>'daticulocale'), \
         daticurules::text,datcollversion::text FROM pg_database WHERE datname=current_database()",
    )
    .fetch_one(&mut **transaction)
    .await
    .map_err(|_| BackupError::InvalidBackup)?;
    let contract = DatabaseSemanticIdentity {
        encoding: row.0,
        locale_provider: row.1,
        collate: row.2,
        ctype: row.3,
        icu_locale: row.4,
        icu_rules: row.5,
        collation_version: row.6,
    };
    validate_database_semantic_contract(&contract).map_err(|_| BackupError::InvalidBackup)?;
    Ok(contract)
}

fn validate_database_semantic_contract(
    contract: &DatabaseSemanticIdentity,
) -> Result<(), BackupError> {
    let bounded = |value: &str| !value.is_empty() && value.len() <= 1024 && !value.contains('\0');
    if !bounded(&contract.encoding)
        || !contract
            .encoding
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        || !matches!(contract.locale_provider.as_str(), "c" | "i" | "b")
        || !bounded(&contract.collate)
        || !bounded(&contract.ctype)
        || contract
            .icu_locale
            .as_deref()
            .is_some_and(|value| !bounded(value))
        || contract
            .icu_rules
            .as_deref()
            .is_some_and(|value| value.len() > 4096 || value.contains('\0'))
        || contract
            .collation_version
            .as_deref()
            .is_some_and(|value| !bounded(value))
    {
        return Err(BackupError::InvalidRestore);
    }
    Ok(())
}

fn validate_database_access_contract(contract: &DatabaseAccessContract) -> Result<(), BackupError> {
    if contract.owner.is_empty()
        || contract.owner.len() > 63
        || contract.owner.as_bytes().contains(&0)
        || contract.connection_limit < -1
        || contract.principal_identities.is_empty()
        || contract.principal_identities.len() > 1024
        || contract.acl.is_empty()
        || contract.acl.len() > 1024
    {
        return Err(BackupError::InvalidRestore);
    }
    let expected_names = database_principal_names(&contract.owner, &contract.acl);
    let actual_names = contract
        .principal_identities
        .iter()
        .map(|identity| identity.name.clone())
        .collect::<Vec<_>>();
    if actual_names != expected_names
        || contract.principal_identities.iter().any(|identity| {
            identity.name.is_empty()
                || identity.name.len() > 63
                || identity.name.as_bytes().contains(&0)
                || !valid_sha256(&identity.identity_sha256)
        })
    {
        return Err(BackupError::InvalidRestore);
    }
    validate_database_semantic_contract(&contract.semantics)?;
    for entry in &contract.acl {
        if entry.grantor.is_empty()
            || entry.grantor.len() > 63
            || entry.grantor.as_bytes().contains(&0)
            || entry.grantee.as_ref().is_some_and(|role| {
                role.is_empty() || role.len() > 63 || role.as_bytes().contains(&0)
            })
            || !matches!(entry.privilege.as_str(), "CONNECT" | "CREATE" | "TEMPORARY")
        {
            return Err(BackupError::InvalidRestore);
        }
    }
    ordered_database_acl(contract)?;
    Ok(())
}

fn ordered_database_acl(contract: &DatabaseAccessContract) -> Result<Vec<usize>, BackupError> {
    let mut available = BTreeSet::new();
    for privilege in ["CONNECT", "CREATE", "TEMPORARY"] {
        available.insert((contract.owner.clone(), privilege.to_owned()));
    }
    let mut pending = (0..contract.acl.len()).collect::<Vec<_>>();
    pending.sort_by(|left, right| {
        let left = &contract.acl[*left];
        let right = &contract.acl[*right];
        (
            &left.privilege,
            &left.grantor,
            &left.grantee,
            !left.grantable,
        )
            .cmp(&(
                &right.privilege,
                &right.grantor,
                &right.grantee,
                !right.grantable,
            ))
    });
    let mut ordered = Vec::with_capacity(pending.len());
    while !pending.is_empty() {
        let Some(position) = pending.iter().position(|index| {
            let entry = &contract.acl[*index];
            available.contains(&(entry.grantor.clone(), entry.privilege.clone()))
        }) else {
            return Err(BackupError::InvalidRestore);
        };
        let index = pending.remove(position);
        let entry = &contract.acl[index];
        if entry.grantable
            && let Some(grantee) = &entry.grantee
        {
            available.insert((grantee.clone(), entry.privilege.clone()));
        }
        ordered.push(index);
    }
    Ok(ordered)
}

async fn restore_database_access_contract(
    control_pool: &PgPool,
    database_name: &str,
    contract: &DatabaseAccessContract,
) -> Result<(), BackupError> {
    let mut transaction = control_pool
        .begin()
        .await
        .map_err(|_| BackupError::InvalidRestore)?;
    restore_database_access_contract_in(&mut transaction, database_name, contract).await?;
    transaction
        .commit()
        .await
        .map_err(|_| BackupError::InvalidRestore)
}

async fn restore_database_access_contract_in(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    database_name: &str,
    contract: &DatabaseAccessContract,
) -> Result<(), BackupError> {
    validate_database_access_contract(contract)?;
    let database = quoted_identifier(database_name)?;
    let owner = quoted_pg_role_identifier(&contract.owner)?;
    sqlx::query(AssertSqlSafe(format!(
        "ALTER DATABASE {database} OWNER TO {owner}"
    )))
    .execute(&mut **transaction)
    .await
    .map_err(|_| BackupError::InvalidRestore)?;
    sqlx::query(AssertSqlSafe(format!(
        "REVOKE ALL PRIVILEGES ON DATABASE {database} FROM PUBLIC"
    )))
    .execute(&mut **transaction)
    .await
    .map_err(|_| BackupError::InvalidRestore)?;
    let mut roles = contract
        .acl
        .iter()
        .filter_map(|entry| entry.grantee.as_deref())
        .chain(std::iter::once(contract.owner.as_str()))
        .collect::<Vec<_>>();
    roles.sort_unstable();
    roles.dedup();
    for role in roles {
        let role = quoted_pg_role_identifier(role)?;
        sqlx::query(AssertSqlSafe(format!(
            "REVOKE ALL PRIVILEGES ON DATABASE {database} FROM {role}"
        )))
        .execute(&mut **transaction)
        .await
        .map_err(|_| BackupError::InvalidRestore)?;
    }
    for index in ordered_database_acl(contract)? {
        let entry = &contract.acl[index];
        let grantee = match &entry.grantee {
            Some(role) => quoted_pg_role_identifier(role)?,
            None => "PUBLIC".to_owned(),
        };
        let grantor = quoted_pg_role_identifier(&entry.grantor)?;
        let grant_option = if entry.grantable {
            " WITH GRANT OPTION"
        } else {
            ""
        };
        sqlx::query(AssertSqlSafe(format!("SET LOCAL ROLE {grantor}")))
            .execute(&mut **transaction)
            .await
            .map_err(|_| BackupError::InvalidRestore)?;
        sqlx::query(AssertSqlSafe(format!(
            "GRANT {} ON DATABASE {database} TO {grantee}{grant_option}",
            entry.privilege
        )))
        .execute(&mut **transaction)
        .await
        .map_err(|_| BackupError::InvalidRestore)?;
        sqlx::query("RESET ROLE")
            .execute(&mut **transaction)
            .await
            .map_err(|_| BackupError::InvalidRestore)?;
    }
    sqlx::query(AssertSqlSafe(format!(
        "ALTER DATABASE {database} CONNECTION LIMIT {}",
        contract.connection_limit
    )))
    .execute(&mut **transaction)
    .await
    .map_err(|_| BackupError::InvalidRestore)?;
    Ok(())
}

fn database_access_matches_while_closed(
    current: &DatabaseAccessContract,
    expected: &DatabaseAccessContract,
) -> bool {
    !current.allow_connections
        && current.owner == expected.owner
        && current.connection_limit == expected.connection_limit
        && current.semantics == expected.semantics
        && current.acl == expected.acl
}

fn create_restore_database_sql(
    database_name: &str,
    replacement_owner: &str,
    semantics: &DatabaseSemanticIdentity,
) -> Result<AssertSqlSafe<String>, BackupError> {
    validate_database_semantic_contract(semantics)?;
    let database = quoted_identifier(database_name)?;
    let owner = quoted_pg_role_identifier(replacement_owner)?;
    let encoding = quoted_literal(&semantics.encoding)?;
    let provider = match semantics.locale_provider.as_str() {
        "c" => "libc",
        "i" => "icu",
        "b" => "builtin",
        _ => return Err(BackupError::InvalidRestore),
    };
    let mut sql = format!(
        "CREATE DATABASE {database} OWNER {owner} TEMPLATE template0 ENCODING {encoding} \
         LOCALE_PROVIDER {provider} ALLOW_CONNECTIONS false CONNECTION LIMIT 0"
    );
    match semantics.locale_provider.as_str() {
        "c" => {
            sql.push_str(" LC_COLLATE ");
            sql.push_str(&quoted_literal(&semantics.collate)?);
            sql.push_str(" LC_CTYPE ");
            sql.push_str(&quoted_literal(&semantics.ctype)?);
        }
        "i" => {
            let locale = semantics
                .icu_locale
                .as_deref()
                .ok_or(BackupError::InvalidRestore)?;
            sql.push_str(" ICU_LOCALE ");
            sql.push_str(&quoted_literal(locale)?);
        }
        "b" => {
            let locale = semantics
                .icu_locale
                .as_deref()
                .ok_or(BackupError::InvalidRestore)?;
            sql.push_str(" BUILTIN_LOCALE ");
            sql.push_str(&quoted_literal(locale)?);
        }
        _ => return Err(BackupError::InvalidRestore),
    }
    if let Some(rules) = &semantics.icu_rules {
        sql.push_str(" ICU_RULES ");
        sql.push_str(&quoted_literal(rules)?);
    }
    if let Some(version) = &semantics.collation_version {
        sql.push_str(" COLLATION_VERSION ");
        sql.push_str(&quoted_literal(version)?);
    }
    Ok(AssertSqlSafe(sql))
}

async fn reset_restore_target(
    control_pool: &PgPool,
    profile: &DatabaseProcessProfile,
    marker: &RestoreMarker,
    key_provider: &Arc<dyn KeyProvider>,
) -> Result<(), BackupError> {
    let mut marker = marker.clone();
    sqlx::query("SET statement_timeout='30000'")
        .execute(control_pool)
        .await
        .map_err(|_| BackupError::InvalidRestore)?;
    sqlx::query("SET lock_timeout='5000'")
        .execute(control_pool)
        .await
        .map_err(|_| BackupError::InvalidRestore)?;
    let target_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname=$1)")
            .bind(&profile.database)
            .fetch_one(control_pool)
            .await
            .map_err(|_| BackupError::InvalidRestore)?;
    let quarantine_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname=$1)")
            .bind(&marker.quarantine)
            .fetch_one(control_pool)
            .await
            .map_err(|_| BackupError::InvalidRestore)?;
    if !quarantine_exists {
        if let Some(replacement_identity) = marker.replacement_identity_sha256.as_deref() {
            if !target_exists
                || database_identity_by_name(control_pool, &profile.database, &profile.database)
                    .await?
                    != replacement_identity
                || !database_access_matches_while_closed(
                    &database_access_contract_by_name(control_pool, &profile.database).await?,
                    &marker.access,
                )
                || database_marker(control_pool, &profile.database)
                    .await?
                    .as_deref()
                    != Some(
                        &serde_json::to_string(&marker).map_err(|_| BackupError::InvalidRestore)?,
                    )
            {
                return Err(BackupError::InvalidRestore);
            }
            finalize_restore_marker_and_role(
                control_pool,
                &profile.database,
                &marker,
                key_provider,
            )
            .await?;
            return Ok(());
        }
        if !target_exists {
            return Err(BackupError::InvalidRestore);
        }
        // A target-only phase-one marker cannot be renamed safely by name
        // against a hostile co-admin. Preserve it for manual authenticated
        // recovery instead of performing a check-then-rename operation.
        return Err(BackupError::InvalidRestore);
    }
    validate_restore_owner_role(control_pool, &marker.replacement_owner).await?;
    if role_identity(control_pool, &marker.replacement_owner).await?
        != marker.replacement_owner_identity_sha256
    {
        return Err(BackupError::InvalidRestore);
    }
    let target_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname=$1)")
            .bind(&profile.database)
            .fetch_one(control_pool)
            .await
            .map_err(|_| BackupError::InvalidRestore)?;
    let created_target = !target_exists;
    if created_target {
        sqlx::query(create_restore_database_sql(
            &profile.database,
            &marker.replacement_owner,
            &marker.access.semantics,
        )?)
        .execute(control_pool)
        .await
        .map_err(|_| BackupError::InvalidRestore)?;
    }
    let target_identity =
        database_identity_by_name(control_pool, &profile.database, &profile.database).await?;
    if let Some(expected) = marker.replacement_identity_sha256.as_deref() {
        if target_identity != expected {
            return Err(BackupError::InvalidRestore);
        }
    } else {
        // A target that appeared while only the phase-one quarantine marker was
        // durable cannot be proven operation-owned. Never promote an observed
        // database identity merely from its name, owner, or catalog flags.
        if !created_target {
            return Err(BackupError::InvalidRestore);
        }
        let target_access =
            database_access_contract_by_name(control_pool, &profile.database).await?;
        if target_access.owner != marker.replacement_owner
            || target_access.allow_connections
            || target_access.connection_limit != 0
            || target_access.semantics != marker.access.semantics
        {
            return Err(BackupError::InvalidRestore);
        }
        marker.replacement_identity_sha256 = Some(target_identity);
        let tag = key_provider
            .authenticate(
                AuthenticateRequest::new(
                    RESTORE_MARKER_PURPOSE,
                    restore_marker_bytes(
                        &marker.database,
                        &marker.target_identity_sha256,
                        &marker.application,
                        &marker.quarantine,
                        &marker.replacement_owner,
                        &marker.replacement_owner_identity_sha256,
                        marker.replacement_identity_sha256.as_deref(),
                        &marker.access,
                    )?,
                )
                .map_err(|_| BackupError::InvalidRestore)?,
            )
            .await
            .map_err(|_| BackupError::KeyUnavailable)?;
        marker.key_id = tag.key_id().to_owned();
        marker.algorithm = tag.algorithm().to_owned();
        marker.tag_hex = hex::encode(tag.bytes());
        set_database_marker(control_pool, &marker.quarantine, &marker).await?;
    }
    let marker_json = serde_json::to_string(&marker).map_err(|_| BackupError::InvalidRestore)?;
    let target_contract: (bool, i32, Option<String>) = sqlx::query_as(
        "SELECT datallowconn,datconnlimit,shobj_description(oid,'pg_database') FROM pg_database WHERE datname=$1",
    )
    .bind(&profile.database)
    .fetch_one(control_pool)
    .await
    .map_err(|_| BackupError::InvalidRestore)?;
    if !(!target_contract.0
        && (target_contract.1 == 0 || target_contract.1 == marker.access.connection_limit))
        || target_contract
            .2
            .as_deref()
            .is_some_and(|comment| comment != marker_json)
    {
        return Err(BackupError::InvalidRestore);
    }
    set_database_marker(control_pool, &profile.database, &marker).await?;
    let current_access = database_access_contract_by_name(control_pool, &profile.database).await?;
    if !database_access_matches_while_closed(&current_access, &marker.access) {
        if current_access.allow_connections || current_access.connection_limit != 0 {
            return Err(BackupError::InvalidRestore);
        }
        restore_database_access_contract(control_pool, &profile.database, &marker.access).await?;
        if !database_access_matches_while_closed(
            &database_access_contract_by_name(control_pool, &profile.database).await?,
            &marker.access,
        ) {
            return Err(BackupError::InvalidRestore);
        }
    }
    let quarantine_identity =
        database_identity_by_name(control_pool, &marker.quarantine, &profile.database).await?;
    if quarantine_identity != marker.target_identity_sha256 {
        return Err(BackupError::InvalidRestore);
    }
    finalize_restore_marker_and_role(control_pool, &profile.database, &marker, key_provider)
        .await?;
    Ok(())
}

async fn finalize_restore_marker_and_role(
    control_pool: &PgPool,
    database_name: &str,
    marker: &RestoreMarker,
    key_provider: &Arc<dyn KeyProvider>,
) -> Result<(), BackupError> {
    let database = quoted_identifier(database_name)?;
    let role = quoted_pg_role_identifier(&marker.replacement_owner)?;
    let mut transaction = control_pool
        .begin()
        .await
        .map_err(|_| BackupError::InvalidRestore)?;
    validate_final_restore_ownership(
        &mut transaction,
        database_name,
        marker,
        key_provider,
        marker
            .replacement_identity_sha256
            .as_deref()
            .ok_or(BackupError::InvalidRestore)?,
    )
    .await?;
    validate_closed_restore_access(&mut transaction, database_name, marker).await?;
    sqlx::query(AssertSqlSafe(format!(
        "ALTER DATABASE {database} CONNECTION LIMIT {}",
        marker.access.connection_limit
    )))
    .execute(&mut *transaction)
    .await
    .map_err(|_| BackupError::InvalidRestore)?;
    sqlx::query(AssertSqlSafe(format!(
        "ALTER DATABASE {database} ALLOW_CONNECTIONS {}",
        marker.access.allow_connections
    )))
    .execute(&mut *transaction)
    .await
    .map_err(|_| BackupError::InvalidRestore)?;
    sqlx::query(AssertSqlSafe(format!("DROP ROLE {role}")))
        .execute(&mut *transaction)
        .await
        .map_err(|_| BackupError::InvalidRestore)?;
    sqlx::query(AssertSqlSafe(format!(
        "COMMENT ON DATABASE {database} IS NULL"
    )))
    .execute(&mut *transaction)
    .await
    .map_err(|_| BackupError::InvalidRestore)?;
    transaction
        .commit()
        .await
        .map_err(|_| BackupError::InvalidRestore)?;
    Ok(())
}

const RESTORE_CATALOG_LOCKS: [&str; 3] = [
    "LOCK TABLE pg_catalog.pg_database IN SHARE ROW EXCLUSIVE MODE",
    "LOCK TABLE pg_catalog.pg_shdescription IN SHARE ROW EXCLUSIVE MODE",
    "LOCK TABLE pg_catalog.pg_authid IN SHARE ROW EXCLUSIVE MODE",
];

#[derive(Clone, Copy)]
enum RestoreCatalogLockScope {
    IdentityAndMarker,
    FullOwnership,
}

impl RestoreCatalogLockScope {
    const fn count(self) -> usize {
        match self {
            Self::IdentityAndMarker => 2,
            Self::FullOwnership => RESTORE_CATALOG_LOCKS.len(),
        }
    }
}

async fn lock_restore_catalogs(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    scope: RestoreCatalogLockScope,
) -> Result<(), BackupError> {
    for statement in &RESTORE_CATALOG_LOCKS[..scope.count()] {
        sqlx::query(*statement)
            .execute(&mut **transaction)
            .await
            .map_err(|_| BackupError::InvalidRestore)?;
    }
    Ok(())
}

async fn validate_restore_identity_and_marker(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    database_name: &str,
    marker: &RestoreMarker,
    expected_database_identity: &str,
) -> Result<(), BackupError> {
    let raw: Option<String> = sqlx::query_scalar(
        "SELECT shobj_description(oid,'pg_database') FROM pg_database WHERE datname=$1",
    )
    .bind(database_name)
    .fetch_one(&mut **transaction)
    .await
    .map_err(|_| BackupError::InvalidRestore)?;
    if raw.as_deref()
        != Some(&serde_json::to_string(marker).map_err(|_| BackupError::InvalidRestore)?)
    {
        return Err(BackupError::InvalidRestore);
    }
    let values: (String, String) = sqlx::query_as(
        "SELECT system_identifier::text,d.oid::text FROM pg_control_system(),pg_database d \
         WHERE d.datname=$1",
    )
    .bind(database_name)
    .fetch_one(&mut **transaction)
    .await
    .map_err(|_| BackupError::InvalidRestore)?;
    let identity = hex::encode(Sha256::digest(canonical_bytes(&(
        "graphhelm-postgres-database-identity-v1",
        values.0,
        values.1,
        database_name,
    ))?));
    if identity != expected_database_identity {
        return Err(BackupError::InvalidRestore);
    }
    Ok(())
}

async fn close_restore_target_after_minimal_validation(
    control_pool: &PgPool,
    database_name: &str,
    marker: &RestoreMarker,
    expected_database_identity: &str,
) -> Result<(), BackupError> {
    let database = quoted_identifier(database_name)?;
    let mut transaction = control_pool
        .begin()
        .await
        .map_err(|_| BackupError::InvalidRestore)?;
    lock_restore_catalogs(&mut transaction, RestoreCatalogLockScope::IdentityAndMarker).await?;
    validate_restore_identity_and_marker(
        &mut transaction,
        database_name,
        marker,
        expected_database_identity,
    )
    .await?;
    sqlx::query(AssertSqlSafe(format!(
        "ALTER DATABASE {database} ALLOW_CONNECTIONS false"
    )))
    .execute(&mut *transaction)
    .await
    .map_err(|_| BackupError::InvalidRestore)?;
    transaction
        .commit()
        .await
        .map_err(|_| BackupError::InvalidRestore)
}

async fn validate_final_restore_ownership(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    database_name: &str,
    marker: &RestoreMarker,
    key_provider: &Arc<dyn KeyProvider>,
    expected_database_identity: &str,
) -> Result<(), BackupError> {
    // Every restore transaction uses this one global order. The close-only recovery path uses
    // the same prefix, so acquisition and validation cannot form an authid/shdescription cycle.
    lock_restore_catalogs(transaction, RestoreCatalogLockScope::FullOwnership).await?;
    validate_restore_identity_and_marker(
        transaction,
        database_name,
        marker,
        expected_database_identity,
    )
    .await?;
    key_provider
        .verify(
            VerifyAuthenticationRequest::new(
                RESTORE_MARKER_PURPOSE,
                restore_marker_bytes(
                    &marker.database,
                    &marker.target_identity_sha256,
                    &marker.application,
                    &marker.quarantine,
                    &marker.replacement_owner,
                    &marker.replacement_owner_identity_sha256,
                    marker.replacement_identity_sha256.as_deref(),
                    &marker.access,
                )?,
                AuthenticationTag::new(
                    marker.key_id.clone(),
                    &marker.algorithm,
                    hex::decode(&marker.tag_hex).map_err(|_| BackupError::InvalidRestore)?,
                )
                .map_err(|_| BackupError::InvalidRestore)?,
            )
            .map_err(|_| BackupError::InvalidRestore)?,
        )
        .await
        .map_err(|_| BackupError::InvalidRestore)?;
    let state: Option<(bool, bool, bool, bool, bool, bool)> = sqlx::query_as(
        "SELECT rolcanlogin,rolsuper,rolcreatedb,rolcreaterole,rolreplication,rolbypassrls \
         FROM pg_roles WHERE rolname=$1",
    )
    .bind(&marker.replacement_owner)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(|_| BackupError::InvalidRestore)?;
    if state != Some((false, false, false, false, false, false))
        || role_identity_in(transaction, &marker.replacement_owner).await?
            != marker.replacement_owner_identity_sha256
    {
        return Err(BackupError::InvalidRestore);
    }
    let current_principals =
        database_principal_identities_in(transaction, &marker.access.owner, &marker.access.acl)
            .await?;
    if current_principals != marker.access.principal_identities {
        return Err(BackupError::InvalidRestore);
    }
    Ok(())
}

async fn validate_closed_restore_access(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    database_name: &str,
    marker: &RestoreMarker,
) -> Result<(), BackupError> {
    let access = database_access_contract_by_name_in(transaction, database_name).await?;
    if !database_access_matches_while_closed(&access, &marker.access) {
        return Err(BackupError::InvalidRestore);
    }
    Ok(())
}

async fn database_marker(
    control_pool: &PgPool,
    database_name: &str,
) -> Result<Option<String>, BackupError> {
    sqlx::query_scalar(
        "SELECT shobj_description(oid,'pg_database') FROM pg_database WHERE datname=$1",
    )
    .bind(database_name)
    .fetch_one(control_pool)
    .await
    .map_err(|_| BackupError::InvalidRestore)
}

async fn set_database_marker(
    control_pool: &PgPool,
    database_name: &str,
    marker: &RestoreMarker,
) -> Result<(), BackupError> {
    let database = quoted_identifier(database_name)?;
    let marker_json = serde_json::to_string(marker).map_err(|_| BackupError::InvalidRestore)?;
    let marker_literal = quoted_literal(&marker_json)?;
    sqlx::query(AssertSqlSafe(format!(
        "COMMENT ON DATABASE {database} IS {marker_literal}"
    )))
    .execute(control_pool)
    .await
    .map_err(|_| BackupError::InvalidRestore)?;
    Ok(())
}

fn read_profile_passfile(path: &Path) -> Result<Zeroizing<String>, BackupError> {
    use std::io::Read as _;
    let file = File::open(path).map_err(|_| BackupError::InvalidBackup)?;
    let metadata = file.metadata().map_err(|_| BackupError::InvalidBackup)?;
    if !metadata.is_file() || metadata.len() > 4096 {
        return Err(BackupError::InvalidBackup);
    }
    let capacity = usize::try_from(metadata.len()).map_err(|_| BackupError::InvalidBackup)?;
    let mut bytes = Zeroizing::new(Vec::with_capacity(capacity));
    file.take(4097)
        .read_to_end(&mut bytes)
        .map_err(|_| BackupError::InvalidBackup)?;
    if bytes.len() > 4096 {
        return Err(BackupError::InvalidBackup);
    }
    String::from_utf8(std::mem::take(&mut *bytes))
        .map(Zeroizing::new)
        .map_err(|_| BackupError::InvalidBackup)
}

async fn schema_contract_sha256_in(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<String, BackupError> {
    let contract: String = sqlx::query_scalar(SCHEMA_CONTRACT_QUERY)
        .fetch_one(&mut **transaction)
        .await
        .map_err(|_| BackupError::InvalidBackup)?;
    Ok(hex::encode(Sha256::digest(contract.as_bytes())))
}

async fn schema_contract_sha256(pool: &PgPool) -> Result<String, BackupError> {
    let contract: String = sqlx::query_scalar(SCHEMA_CONTRACT_QUERY)
        .fetch_one(pool)
        .await
        .map_err(|_| BackupError::InvalidRestore)?;
    Ok(hex::encode(Sha256::digest(contract.as_bytes())))
}

async fn privilege_contract_sha256_in(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<String, BackupError> {
    let contract: String = sqlx::query_scalar(PRIVILEGE_CONTRACT_QUERY)
        .fetch_one(&mut **transaction)
        .await
        .map_err(|_| BackupError::InvalidBackup)?;
    Ok(hex::encode(Sha256::digest(contract.as_bytes())))
}

async fn privilege_contract_sha256(pool: &PgPool) -> Result<String, BackupError> {
    let contract: String = sqlx::query_scalar(PRIVILEGE_CONTRACT_QUERY)
        .fetch_one(pool)
        .await
        .map_err(|_| BackupError::InvalidRestore)?;
    Ok(hex::encode(Sha256::digest(contract.as_bytes())))
}

async fn configured_runtime_role(pool: &PgPool) -> Result<String, BackupError> {
    let roles: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT grantee.rolname FROM pg_class c \
         JOIN pg_namespace n ON n.oid=c.relnamespace \
         CROSS JOIN LATERAL aclexplode(COALESCE(c.relacl,acldefault('r',c.relowner))) a \
         JOIN pg_roles grantee ON grantee.oid=a.grantee \
         WHERE n.nspname='public' AND c.relname LIKE 'graphhelm\\_%' ESCAPE '\\' \
           AND c.relkind IN ('r','p') AND a.grantee<>c.relowner \
         ORDER BY grantee.rolname LIMIT 2",
    )
    .fetch_all(pool)
    .await
    .map_err(|_| BackupError::InvalidRestore)?;
    match roles.as_slice() {
        [role] => Ok(role.clone()),
        _ => Err(BackupError::InvalidRestore),
    }
}

async fn configured_runtime_role_in(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<String, BackupError> {
    let roles: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT grantee.rolname FROM pg_class c \
         JOIN pg_namespace n ON n.oid=c.relnamespace \
         CROSS JOIN LATERAL aclexplode(COALESCE(c.relacl,acldefault('r',c.relowner))) a \
         JOIN pg_roles grantee ON grantee.oid=a.grantee \
         WHERE n.nspname='public' AND c.relname LIKE 'graphhelm\\_%' ESCAPE '\\' \
           AND c.relkind IN ('r','p') AND a.grantee<>c.relowner \
         ORDER BY grantee.rolname LIMIT 2",
    )
    .fetch_all(&mut **transaction)
    .await
    .map_err(|_| BackupError::InvalidBackup)?;
    match roles.as_slice() {
        [role] => Ok(role.clone()),
        _ => Err(BackupError::InvalidBackup),
    }
}

async fn privilege_contract_is_safe<'e, E>(executor: E) -> Result<bool, BackupError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query_scalar(
        r#"WITH relation_grants AS (
          SELECT c.relname,a.grantee,a.privilege_type,a.is_grantable,c.relowner
          FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
          CROSS JOIN LATERAL aclexplode(COALESCE(c.relacl,acldefault('r',c.relowner))) a
          WHERE n.nspname='public' AND c.relname LIKE 'graphhelm\_%' ESCAPE '\'
            AND c.relkind IN ('r','p')
        ), runtime_candidates AS (
          SELECT DISTINCT grantee FROM relation_grants WHERE grantee<>0 AND grantee<>relowner
        ), runtime AS (
          SELECT min(grantee) oid FROM runtime_candidates
        ), expected(relname,privilege_type) AS (VALUES
          ('graphhelm_streams','SELECT'),('graphhelm_streams','INSERT'),('graphhelm_streams','UPDATE'),
          ('graphhelm_evidence','SELECT'),('graphhelm_evidence','INSERT'),('graphhelm_evidence','UPDATE'),
          ('graphhelm_retention_operations','SELECT'),('graphhelm_retention_operations','INSERT'),('graphhelm_retention_operations','UPDATE'),
          ('graphhelm_retention_targets','SELECT'),('graphhelm_retention_targets','INSERT'),('graphhelm_retention_targets','UPDATE'),
          ('graphhelm_projection_active','SELECT'),('graphhelm_projection_active','INSERT'),('graphhelm_projection_active','UPDATE'),
          ('graphhelm_legal_holds','SELECT'),('graphhelm_legal_holds','INSERT'),
          ('graphhelm_projection_checkpoints','SELECT'),('graphhelm_projection_checkpoints','INSERT'),
          ('graphhelm_idempotency','SELECT'),('graphhelm_idempotency','INSERT'),
          ('graphhelm_events','SELECT'),('graphhelm_events','INSERT'),
          ('graphhelm_artifacts','SELECT'),('graphhelm_artifacts','INSERT'),
          ('graphhelm_evidence_refs','SELECT'),('graphhelm_evidence_refs','INSERT'),
          ('graphhelm_artifact_refs','SELECT'),('graphhelm_artifact_refs','INSERT'),
          ('graphhelm_checkpoints','SELECT'),('graphhelm_checkpoints','INSERT'),
          ('graphhelm_retention_policies','SELECT'),('graphhelm_retention_policies','INSERT'),
          ('graphhelm_evidence_tombstones','SELECT'),('graphhelm_evidence_tombstones','INSERT'),
          ('graphhelm_cleanup_receipts','SELECT'),('graphhelm_cleanup_receipts','INSERT')
        ), actual AS (
          SELECT relname,privilege_type FROM relation_grants,runtime
          WHERE grantee=runtime.oid
        ) SELECT
          (SELECT count(*)=1 FROM runtime_candidates)
          AND NOT EXISTS (SELECT * FROM expected EXCEPT SELECT * FROM actual)
          AND NOT EXISTS (SELECT * FROM actual EXCEPT SELECT * FROM expected)
          AND NOT EXISTS (SELECT 1 FROM relation_grants,runtime
             WHERE grantee<>relowner AND grantee<>runtime.oid)
          AND NOT EXISTS (SELECT 1 FROM relation_grants WHERE is_grantable)
          AND EXISTS (SELECT 1 FROM pg_roles r,runtime WHERE r.oid=runtime.oid
             AND NOT r.rolsuper AND NOT r.rolcreatedb AND NOT r.rolcreaterole
             AND NOT r.rolinherit AND NOT r.rolreplication AND NOT r.rolbypassrls)
          AND NOT EXISTS (SELECT 1 FROM pg_auth_members m,runtime
             WHERE m.member=runtime.oid OR m.roleid=runtime.oid)
          AND (SELECT count(*)=1 FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
             CROSS JOIN LATERAL aclexplode(COALESCE(p.proacl,acldefault('f',p.proowner))) a,runtime
             WHERE n.nspname='public' AND p.proname LIKE 'graphhelm\_%' ESCAPE '\'
               AND a.grantee=runtime.oid AND a.privilege_type='EXECUTE' AND NOT a.is_grantable
               AND p.proname='graphhelm_migrations_are_current')
          AND NOT EXISTS (SELECT 1 FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
             CROSS JOIN LATERAL aclexplode(COALESCE(p.proacl,acldefault('f',p.proowner))) a,runtime
             WHERE n.nspname='public' AND p.proname LIKE 'graphhelm\_%' ESCAPE '\'
               AND a.grantee<>p.proowner AND a.grantee<>runtime.oid
               AND (a.is_grantable OR a.grantee<>0 OR p.prosecdef))
          AND EXISTS (SELECT 1 FROM pg_namespace n
             CROSS JOIN LATERAL aclexplode(COALESCE(n.nspacl,acldefault('n',n.nspowner))) a,runtime
             WHERE n.nspname='public' AND a.grantee=runtime.oid
               AND a.privilege_type='USAGE' AND NOT a.is_grantable)
          AND NOT EXISTS (SELECT 1 FROM pg_namespace n
             CROSS JOIN LATERAL aclexplode(COALESCE(n.nspacl,acldefault('n',n.nspowner))) a,runtime
             WHERE n.nspname='public' AND a.grantee=runtime.oid AND a.privilege_type<>'USAGE')
          AND NOT has_schema_privilege((SELECT oid FROM runtime),'public','CREATE')
          AND NOT has_database_privilege((SELECT oid FROM runtime),current_database(),'CREATE')
          AND NOT has_database_privilege((SELECT oid FROM runtime),current_database(),'TEMP')"#,
    )
    .fetch_one(executor)
    .await
    .map_err(|_| BackupError::InvalidBackup)
}

async fn validate_restore_domain_bounds(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<(), BackupError> {
    let (
        checkpoints,
        retained_rows,
        retention_scopes,
        retention_operations,
        max_policies_per_scope,
        max_holds_per_scope,
        max_cleanup_per_scope,
        max_targets_per_operation,
    ): (i64, i64, i64, i64, i64, i64, i64, i64) =
        sqlx::query_as(
            "SELECT \
             (SELECT count(*) FROM public.graphhelm_checkpoints), \
             ((SELECT count(*) FROM public.graphhelm_retention_operations)+\
              (SELECT count(*) FROM public.graphhelm_retention_policies)+\
              (SELECT count(*) FROM public.graphhelm_retention_targets)+\
              (SELECT count(*) FROM public.graphhelm_legal_holds)+\
              (SELECT count(*) FROM public.graphhelm_evidence_tombstones)+\
              (SELECT count(*) FROM public.graphhelm_cleanup_receipts)), \
             (SELECT count(*) FROM (\
              SELECT workspace_id,project_id,execution_id FROM public.graphhelm_retention_operations \
              UNION SELECT workspace_id,project_id,execution_id FROM public.graphhelm_retention_policies \
              UNION SELECT workspace_id,project_id,execution_id FROM public.graphhelm_legal_holds \
              UNION SELECT workspace_id,project_id,execution_id FROM public.graphhelm_evidence_tombstones \
              UNION SELECT workspace_id,project_id,execution_id FROM public.graphhelm_cleanup_receipts) scopes), \
             (SELECT count(*) FROM public.graphhelm_retention_operations), \
             (SELECT COALESCE(max(rows),0) FROM (SELECT count(*) rows FROM public.graphhelm_retention_policies \
              GROUP BY workspace_id,project_id,execution_id) counts), \
             (SELECT COALESCE(max(rows),0) FROM (SELECT count(*) rows FROM public.graphhelm_legal_holds \
              GROUP BY workspace_id,project_id,execution_id) counts), \
             (SELECT COALESCE(max(rows),0) FROM (SELECT count(*) rows FROM public.graphhelm_cleanup_receipts \
              GROUP BY workspace_id,project_id,execution_id) counts), \
             (SELECT COALESCE(max(rows),0) FROM (SELECT count(*) rows FROM public.graphhelm_retention_targets \
              GROUP BY workspace_id,project_id,execution_id,operation_id) counts)",
        )
        .fetch_one(&mut **transaction)
        .await
        .map_err(|_| BackupError::InvalidBackup)?;
    if !(0..=100_000).contains(&checkpoints)
        || !(0..=1_000_000).contains(&retained_rows)
        || !(0..=10_000).contains(&retention_scopes)
        || !(0..=100_000).contains(&retention_operations)
        || !(0..=100_000).contains(&max_policies_per_scope)
        || !(0..=20_000).contains(&max_holds_per_scope)
        || !(0..=100_000).contains(&max_cleanup_per_scope)
        || !(0..=10_000).contains(&max_targets_per_operation)
    {
        return Err(BackupError::LimitExceeded);
    }
    Ok(())
}

async fn state_summary_connection(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<String, BackupError> {
    const PAGE_ROWS: usize = 4;
    const MAX_STATE_ROWS: u64 = 10_000_000;
    const MAX_STATE_ROW_BYTES: usize = 32 * 1024 * 1024;
    let relation_bytes: i64 = sqlx::query_scalar(
        "SELECT COALESCE(sum(pg_total_relation_size(c.oid)),0)::bigint FROM pg_class c \
         JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public' \
         AND c.relkind IN ('r','p') AND c.relname LIKE 'graphhelm\\_%' ESCAPE '\\'",
    )
    .fetch_one(&mut **transaction)
    .await
    .map_err(|_| BackupError::InvalidBackup)?;
    if relation_bytes < 0 || relation_bytes as u64 > MAX_BACKUP_BYTES {
        return Err(BackupError::LimitExceeded);
    }
    let mut digest = Sha256::new();
    digest.update(b"graphhelm-postgres-state-summary-v1");
    let mut total_rows = 0_u64;
    let mut total_bytes = 0_u64;
    for (index, table) in STATE_TABLES.iter().enumerate() {
        let (table_rows, largest_row, table_bytes): (i64, i64, String) =
            sqlx::query_as(AssertSqlSafe(format!(
                "SELECT count(*)::bigint, \
                 COALESCE(max(octet_length(to_jsonb(t)::text)),0)::bigint, \
                 COALESCE(sum(octet_length(to_jsonb(t)::text))::numeric,0)::text \
                 FROM public.{table} t"
            )))
            .fetch_one(&mut **transaction)
            .await
            .map_err(|_| BackupError::InvalidBackup)?;
        let table_rows = u64::try_from(table_rows).map_err(|_| BackupError::InvalidBackup)?;
        let largest_row = usize::try_from(largest_row).map_err(|_| BackupError::InvalidBackup)?;
        let table_bytes = table_bytes
            .parse::<u64>()
            .map_err(|_| BackupError::LimitExceeded)?;
        total_rows = total_rows
            .checked_add(table_rows)
            .ok_or(BackupError::LimitExceeded)?;
        total_bytes = total_bytes
            .checked_add(table_bytes)
            .ok_or(BackupError::LimitExceeded)?;
        if total_rows > MAX_STATE_ROWS
            || largest_row > MAX_STATE_ROW_BYTES
            || total_bytes > MAX_BACKUP_BYTES
        {
            return Err(BackupError::LimitExceeded);
        }
        let cursor = format!("graphhelm_backup_{index}");
        digest.update(u32::try_from(table.len()).unwrap().to_be_bytes());
        digest.update(table.as_bytes());
        sqlx::query(AssertSqlSafe(format!(
            // This digest is computed on the source cluster, recomputed on the restored cluster,
            // and the two are compared. Without an explicit collation the row order follows each
            // cluster's default, so a byte-identical restore between hosts that disagree on
            // collation would be rejected.
            "DECLARE {cursor} NO SCROLL CURSOR FOR \
             SELECT to_jsonb(t)::text FROM public.{table} t ORDER BY to_jsonb(t)::text COLLATE \"C\""
        )))
        .execute(&mut **transaction)
        .await
        .map_err(|_| BackupError::InvalidBackup)?;
        loop {
            let rows: Vec<Option<String>> = sqlx::query_scalar(AssertSqlSafe(format!(
                "FETCH FORWARD {PAGE_ROWS} FROM {cursor}"
            )))
            .fetch_all(&mut **transaction)
            .await
            .map_err(|_| BackupError::InvalidBackup)?;
            if rows.is_empty() {
                break;
            }
            for row in rows {
                let row = row.ok_or(BackupError::LimitExceeded)?;
                if row.len() > MAX_STATE_ROW_BYTES {
                    return Err(BackupError::LimitExceeded);
                }
                digest.update(u32::try_from(row.len()).unwrap().to_be_bytes());
                digest.update(row.as_bytes());
            }
        }
        sqlx::query(AssertSqlSafe(format!("CLOSE {cursor}")))
            .execute(&mut **transaction)
            .await
            .map_err(|_| BackupError::InvalidBackup)?;
    }
    Ok(hex::encode(digest.finalize()))
}

struct ProcessWatchdog {
    child: Option<std::process::Child>,
    process_id: u32,
    process_group: ProcessGroup,
    state: Arc<(Mutex<bool>, Condvar)>,
    timed_out: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
    /// #805: set when a termination sweep hit its pass limit with descendants still appearing.
    /// Shared with the watchdog thread, which cannot return a value of its own.
    swept_incomplete: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    completed: bool,
    lifecycle_probe: Option<Arc<Mutex<LifecycleProbe>>>,
}

#[derive(Default)]
struct LifecycleProbe {
    events: Vec<&'static str>,
    #[cfg(unix)]
    release_waitable: bool,
    #[cfg(unix)]
    reaped: bool,
}

impl ProcessWatchdog {
    fn start(child: std::process::Child, timeout: Duration) -> Result<Self, BackupError> {
        Self::start_with_cancellation(child, timeout, Arc::new(AtomicBool::new(false)))
    }

    fn start_with_cancellation(
        mut child: std::process::Child,
        timeout: Duration,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Self, BackupError> {
        let process_id = child.id();
        let process_group = match create_process_group(&child) {
            Ok(group) => group,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        let state = Arc::new((Mutex::new(false), Condvar::new()));
        let timed_out = Arc::new(AtomicBool::new(false));
        let watched = Arc::clone(&state);
        let timeout_flag = Arc::clone(&timed_out);
        let cancellation_flag = Arc::clone(&cancelled);
        let swept_incomplete = Arc::new(AtomicBool::new(false));
        let swept_flag = Arc::clone(&swept_incomplete);
        let group_for_timeout = process_group_for_thread(process_group);
        let thread = std::thread::spawn(move || {
            let (lock, condition) = &*watched;
            let deadline = Instant::now() + timeout;
            let mut completed = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            while !*completed {
                if cancellation_flag.load(Ordering::Acquire) {
                    if sweep_left_descendants(&terminate_process(process_id, group_for_timeout)) {
                        swept_flag.store(true, Ordering::Release);
                    }
                    return;
                }
                let now = Instant::now();
                if now >= deadline {
                    timeout_flag.store(true, Ordering::Release);
                    if sweep_left_descendants(&terminate_process(process_id, group_for_timeout)) {
                        swept_flag.store(true, Ordering::Release);
                    }
                    return;
                }
                let wait = deadline
                    .saturating_duration_since(now)
                    .min(Duration::from_millis(10));
                let (next, _) = condition
                    .wait_timeout(completed, wait)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                completed = next;
            }
        });
        Ok(Self {
            child: Some(child),
            process_id,
            process_group,
            state,
            timed_out,
            cancelled,
            swept_incomplete,
            thread: Some(thread),
            completed: false,
            lifecycle_probe: None,
        })
    }

    #[cfg(test)]
    fn with_lifecycle_probe(mut self, probe: Arc<Mutex<LifecycleProbe>>) -> Self {
        self.lifecycle_probe = Some(probe);
        self
    }

    fn record_release(&mut self) {
        if let Some(probe) = &self.lifecycle_probe {
            let mut probe = probe.lock().unwrap();
            probe.events.push("release");
            #[cfg(unix)]
            if let Some(child) = self.child.as_mut() {
                // `leader_exited` uses waitid(WNOWAIT): success proves the leader is still an
                // owned, waitable child at the exact release boundary. A pre-release wait would
                // return ECHILD here and make this regression red.
                probe.release_waitable = graphhelm_process_tree::leader_exited(child).is_ok();
            }
        }
    }

    fn record_reap(&self, child: &mut std::process::Child) {
        if let Some(probe) = &self.lifecycle_probe {
            let mut probe = probe.lock().unwrap();
            probe.events.push("reap");
            #[cfg(unix)]
            {
                // After Child::wait, the same waitid probe must report ECHILD. This observes the
                // kernel's ownership state rather than trusting a test-only label.
                probe.reaped = graphhelm_process_tree::leader_exited(child).is_err();
            }
            #[cfg(not(unix))]
            let _ = child;
        }
    }

    fn terminate(&mut self) {
        if sweep_left_descendants(&terminate_process(
            self.process_id,
            process_group_for_thread(self.process_group),
        )) {
            self.swept_incomplete.store(true, Ordering::Release);
        }
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
        }
    }

    fn leader_exited(&mut self) -> bool {
        self.child
            .as_mut()
            .and_then(|child| graphhelm_process_tree::leader_exited(child).ok())
            .unwrap_or(false)
    }

    fn finish(mut self) -> Result<std::process::ExitStatus, BackupError> {
        // On Unix this must observe the leader without reaping it. The cached pgid is the
        // leader's pid, so the anchor has to remain occupied until `finish_inner` releases the
        // group. `Child::try_wait` would give the pid back before that release and reopen #714.
        let waited = wait_for_leader(
            self.child
                .as_mut()
                .ok_or_else(|| unavailable(UnavailableStage::ProcessWait))?,
            Duration::from_secs(24 * 60 * 60),
        );
        if waited.is_err() {
            self.terminate();
        }
        // A successful leader must not be allowed to leave pipe-owning descendants behind.
        if sweep_left_descendants(&terminate_process(
            self.process_id,
            process_group_for_thread(self.process_group),
        )) {
            self.swept_incomplete.store(true, Ordering::Release);
        }
        self.finish_inner();
        // Reap only after `close_process_group`: on Unix this is the point at which the cached
        // pgid is no longer used. Windows has the same ordering through the platform-neutral
        // process-tree API, although its job handle is not invalidated by a reap.
        let mut child = self
            .child
            .take()
            .ok_or_else(|| unavailable(UnavailableStage::ProcessWait))?;
        let status = child
            .wait()
            .map_err(|_| unavailable(UnavailableStage::ProcessWait));
        self.record_reap(&mut child);
        let status = status?;
        waited?;
        // #81: these two flags were fused into one value here, and the fusion is exactly
        // the defect class this issue names — a deliberate cancel and an elapsed deadline
        // are opposite facts to whoever holds the error. Cancelled stays an availability
        // fact (someone chose to stop the work); timed_out is a TIMING fact and must say so.
        // Checked cancel-first: a cancel that also crossed the deadline was still a cancel.
        if self.cancelled.load(Ordering::Acquire) {
            Err(unavailable(UnavailableStage::Cancellation))
        } else if self.timed_out.load(Ordering::Acquire) {
            Err(BackupError::DeadlineElapsed)
        } else if self.swept_incomplete.load(Ordering::Acquire) {
            // SAFE ONLY BECAUSE `finish_inner()` ABOVE JOINS THE WATCHDOG THREAD. That join is
            // the happens-before edge for this read: the thread's `store(Release)` on the cancel
            // and timeout arms is ordered before it, so this `load(Acquire)` cannot miss a sweep
            // the watchdog recorded. Notifying the condvar is NOT enough on its own -- it wakes
            // the thread, it does not wait for it. And there is exactly ONE join in this type:
            // `Drop` reaches `finish_inner()` as well, but `finish()` has already taken the
            // handle by then, so `Drop` is NOT a backstop for this read -- do not read the
            // `self.thread.take()` below as belonging to it.
            //
            // Move the join, make it conditional, or hoist this load above `finish_inner()`, and
            // the failure is SILENT and points the WRONG WAY: a missed store reads as `false`,
            // which says the tree is gone for a sweep that actually hit its bound.
            //
            // The `Release`/`Acquire` pair is redundant GIVEN the join, and stays on purpose:
            // it is what keeps this read correct if the join is ever legitimately moved, and
            // relaxing it to `Relaxed` would remove the second of two guarantees while the
            // first is the one people edit.
            // #805: the sweep ran out of passes while descendants were still appearing, so the
            // leader's exit status does not mean the tree is gone. Reported LAST so it never
            // masks a cancel or a deadline, both of which are facts about why the work stopped;
            // this one is a fact about what the stop left behind.
            Err(unavailable(UnavailableStage::ProcessTerminate))
        } else {
            Ok(status)
        }
    }

    fn finish_inner(&mut self) {
        let (lock, condition) = &*self.state;
        if let Ok(mut completed) = lock.lock() {
            *completed = true;
            condition.notify_one();
        }
        // LOAD-BEARING, NOT CLEANUP. `finish()` reads `swept_incomplete` after calling this, and
        // this join is the happens-before edge that makes the read see the watchdog's
        // `store(Release)`. The discarded `Result` is the thread's panic payload and nothing
        // else -- discarding it does not make the join optional. Removing it, making it
        // conditional, or moving it below `close_process_group` breaks a read thirty lines away
        // in `finish()`, silently, in the direction that says the tree is gone.
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        self.record_release();
        close_process_group(&mut self.process_group);
        self.completed = true;
    }
}

impl Drop for ProcessWatchdog {
    fn drop(&mut self) {
        if !self.completed {
            self.terminate();
            self.finish_inner();
            // The Unix pgid remains valid only until the release above. Reap after it, even on
            // the drop path, so cancellation cannot leave either a reusable group identity or a
            // zombie.
            if let Some(mut child) = self.child.take() {
                let _ = child.wait();
                self.record_reap(&mut child);
            }
        }
    }
}

// Process-tree termination moved to `adapters/process-tree` (#618). The bodies that used to sit
// here are unchanged there; what stays is the CALL and the one place this crate's vocabulary is
// restated.
//
// Why it moved: `adapters/tool-host`'s spawn funnel — the path every Tier 1 execution takes — kills
// the direct child only, while this file had carried job objects and process groups since the
// evidence-store milestone. The capability existed and the place that needed it most did not call
// it. Copying it would have made platform code exist twice, which is the worst kind to have two of.
//
// The wrappers below are kept rather than inlined at the ~15 call sites, so this move changes no
// call site at all and the error mapping lives in exactly one place.
use graphhelm_process_tree::{ProcessGroup, ProcessTreeError, TerminationOutcome};

fn configure_process_group(command: &mut std::process::Command) {
    graphhelm_process_tree::configure(command);
}

/// The extracted crate cannot name `UnavailableStage`, so the mapping is here — and it is the only
/// semantic surface the extraction touches. Each variant maps to the stage the inline code returned
/// for exactly the same condition: `JobSetup` for a job object that could not be created,
/// configured or assigned, `ProcessResume` for an assigned child that could not be resumed.
fn create_process_group(child: &std::process::Child) -> Result<ProcessGroup, BackupError> {
    graphhelm_process_tree::create(child).map_err(|error| match error {
        ProcessTreeError::JobSetup => unavailable(UnavailableStage::JobSetup),
        ProcessTreeError::ProcessResume => unavailable(UnavailableStage::ProcessResume),
        // #878: a child that reached `create` unsuspended. Mapped to `JobSetup` deliberately
        // rather than given a stage of its own -- the stage is a wire-visible code
        // (`process.job.setup`), and the condition IS a job that cannot contain this child. A new
        // stage would widen a published vocabulary for a case this call site cannot reach,
        // because the spawn above configures.
        ProcessTreeError::ChildNotSuspended => unavailable(UnavailableStage::JobSetup),
    })
}

fn process_group_for_thread(group: ProcessGroup) -> ProcessGroup {
    graphhelm_process_tree::for_thread(group)
}

fn close_process_group(group: &mut ProcessGroup) {
    graphhelm_process_tree::close(group);
}

/// Kills the tree and RETURNS what the sweep managed, rather than dropping it (#805).
///
/// `BoundReached` is the only outcome this crate treats as a failure: it means descendants were
/// still appearing when the sweep ran out of passes, so "the tree is gone" would be a claim the
/// call cannot support. `SweepUnavailable` is NOT a failure -- it is the process-tree crate
/// declaring a platform limit it cannot exceed, and turning a documented limit into a backup error
/// would fail every host without `/proc`.
fn terminate_process(process_id: u32, group: ProcessGroup) -> TerminationOutcome {
    graphhelm_process_tree::terminate(process_id, group)
}

/// Whether an outcome means descendants may still be running.
///
/// AN EXHAUSTIVE `match`, NOT `matches!` (#815). The predicate used to be one `matches!` arm, which
/// means every variant that did not exist yet answered `false` -- "no descendants left" -- with no
/// compiler complaint. That is a policy taken by omission, in the direction that says the tree is
/// gone. When `NotAttempted` was split out of `SweepUnavailable`, a `matches!` here would have
/// classified a call that terminated NOTHING as a clean tree: the type would have gained the
/// distinction and the consumer would have thrown it away in the same commit.
///
/// Written out, the next variant does not compile until somebody decides what it means.
const fn sweep_left_descendants(outcome: &TerminationOutcome) -> bool {
    match outcome {
        // The sweep ran out of passes while descendants were still appearing.
        TerminationOutcome::BoundReached { .. } => true,
        // Nothing was signalled at all: the leader and every descendant are still running, which is
        // strictly worse than a bounded sweep and cannot be read as a stopped tree.
        TerminationOutcome::NotAttempted => true,
        // Signalled everything reachable and a final pass found nothing new.
        TerminationOutcome::Complete => false,
        // The platform declared a limit it cannot exceed; the group signal WAS sent. Treating a
        // documented limit as a backup failure would fail every host without `/proc`.
        TerminationOutcome::SweepUnavailable => false,
    }
}

#[cfg(test)]
fn process_is_running(process_id: u32) -> bool {
    graphhelm_process_tree::process_is_running(process_id)
}

fn wait_for_leader(child: &mut std::process::Child, timeout: Duration) -> Result<(), BackupError> {
    let deadline = Instant::now() + timeout;
    loop {
        if graphhelm_process_tree::leader_exited(child).map_err(|_| BackupError::InvalidBackup)? {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(unavailable(UnavailableStage::ProcessWait));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn validate_manifest_for_restore(
    manifest: &BackupManifest,
    pg_dump: &PinnedTool,
    pg_restore: &PinnedTool,
) -> Result<(), BackupError> {
    let expected_migrations = [
        crate::INITIAL_MIGRATION,
        crate::RETENTION_MIGRATION,
        crate::PROJECTION_MIGRATION,
    ]
    .into_iter()
    .map(|migration| hex::encode(Sha256::digest(migration.as_bytes())))
    .collect::<Vec<_>>();
    if manifest.schema_migration_version != 3
        || manifest.repository_format != "repository-v1"
        || manifest.release_catalog_sha256
            != hex::encode(Sha256::digest(include_bytes!(
                "../../../schemas/catalog.json"
            )))
        || manifest.migration_sha256 != expected_migrations
        || manifest.pg_dump_version != pg_dump.exact_version
        || manifest.pg_dump_sha256 != pg_dump.sha256_hex()
        || manifest.pg_restore_version != pg_restore.exact_version
        || manifest.pg_restore_sha256 != pg_restore.sha256_hex()
        || !valid_sha256(&manifest.state_summary_sha256)
        || !valid_sha256(&manifest.privilege_summary_sha256)
        || manifest.runtime_role.is_empty()
        || manifest.runtime_role.len() > 63
        || !manifest
            .runtime_role
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(BackupError::InvalidRestore);
    }
    Ok(())
}

async fn verify_evidence_records(
    pool: &PgPool,
    key_provider: Arc<dyn KeyProvider>,
) -> Result<(), BackupError> {
    const PAGE: i64 = 4;
    const MAX_EVIDENCE_RECORD_BYTES: i64 = 32 * 1024 * 1024;
    let relation_bytes: i64 = sqlx::query_scalar(
        "SELECT pg_total_relation_size('public.graphhelm_evidence'::regclass)::bigint",
    )
    .fetch_one(pool)
    .await
    .map_err(|_| BackupError::InvalidRestore)?;
    if relation_bytes < 0 || relation_bytes as u64 > MAX_BACKUP_BYTES {
        return Err(BackupError::LimitExceeded);
    }
    let (record_count, largest_record): (i64, i64) = sqlx::query_as(
        "SELECT count(*)::bigint,COALESCE(max(octet_length(record::text)),0)::bigint \
         FROM public.graphhelm_evidence",
    )
    .fetch_one(pool)
    .await
    .map_err(|_| BackupError::InvalidRestore)?;
    if !(0..=10_000_000).contains(&record_count)
        || !(0..=MAX_EVIDENCE_RECORD_BYTES).contains(&largest_record)
    {
        return Err(BackupError::LimitExceeded);
    }
    let mut cursor: Option<(String, String, String, String)> = None;
    let mut verified = 0_u64;
    loop {
        let (workspace, project, execution, evidence) = cursor.clone().unwrap_or_default();
        let rows: Vec<(
            String,
            String,
            String,
            String,
            String,
            Option<serde_json::Value>,
        )> = sqlx::query_as(
            "SELECT workspace_id,project_id,execution_id,evidence_id,state, \
             CASE WHEN octet_length(record::text)<=33554432 THEN record END \
             FROM public.graphhelm_evidence \
             WHERE ($1='' OR (workspace_id,project_id,execution_id,evidence_id)>($1,$2,$3,$4)) \
             ORDER BY workspace_id COLLATE \"C\",project_id COLLATE \"C\",execution_id COLLATE \"C\",evidence_id COLLATE \"C\" LIMIT $5",
        )
        .bind(&workspace)
        .bind(&project)
        .bind(&execution)
        .bind(&evidence)
        .bind(PAGE)
        .fetch_all(pool)
        .await
        .map_err(|_| BackupError::InvalidRestore)?;
        if rows.is_empty() {
            break;
        }
        verified = verified
            .checked_add(rows.len() as u64)
            .ok_or(BackupError::LimitExceeded)?;
        if verified > 10_000_000 {
            return Err(BackupError::LimitExceeded);
        }
        for (workspace, project, execution, evidence, state, record) in rows {
            let record = record.ok_or(BackupError::LimitExceeded)?;
            if state == "erased" && record == serde_json::json!({}) {
                cursor = Some((workspace, project, execution, evidence));
                continue;
            }
            let stored: crate::rows::StoredEvidence =
                serde_json::from_value(record).map_err(|_| BackupError::InvalidRestore)?;
            let sealed = stored
                .into_sealed()
                .map_err(|_| BackupError::InvalidRestore)?;
            graphhelm_events::validate_sealed_evidence(&sealed)
                .map_err(|_| BackupError::InvalidRestore)?;
            if sealed.scope().workspace_id().as_str() != workspace
                || sealed.scope().project_id().as_str() != project
                || sealed
                    .scope()
                    .execution_id()
                    .map_or("", |value| value.as_str())
                    != execution
                || sealed.reference().evidence_id().as_str() != evidence
            {
                return Err(BackupError::InvalidRestore);
            }
            match state.as_str() {
                "available" => {
                    let opener =
                        EvidenceProtector::new(BackupKeyProvider(Arc::clone(&key_provider)));
                    opener
                        .open(sealed.scope().clone(), &sealed)
                        .await
                        .map_err(|_| BackupError::InvalidRestore)?;
                }
                "expired" | "missing_key" | "integrity_failed" | "erasure_pending" | "erased" => {}
                _ => return Err(BackupError::InvalidRestore),
            }
            cursor = Some((workspace, project, execution, evidence));
        }
    }
    Ok(())
}

struct BackupKeyProvider(Arc<dyn KeyProvider>);

impl KeyProvider for BackupKeyProvider {
    fn metadata<'a>(
        &'a self,
    ) -> graphhelm_events::RepositoryFuture<
        'a,
        Result<graphhelm_events::KeyProviderMetadata, graphhelm_events::KeyError>,
    > {
        self.0.metadata()
    }

    fn wrap<'a>(
        &'a self,
        request: WrapKeyRequest,
    ) -> graphhelm_events::RepositoryFuture<'a, Result<WrappedKey, graphhelm_events::KeyError>>
    {
        self.0.wrap(request)
    }

    fn unwrap<'a>(
        &'a self,
        wrapped: WrappedKey,
    ) -> graphhelm_events::RepositoryFuture<'a, Result<SecretBytes, graphhelm_events::KeyError>>
    {
        self.0.unwrap(wrapped)
    }

    fn revoke<'a>(
        &'a self,
        request: graphhelm_events::RevokeKeyRequest,
    ) -> graphhelm_events::RepositoryFuture<
        'a,
        Result<graphhelm_events::RevocationReceipt, graphhelm_events::KeyError>,
    > {
        self.0.revoke(request)
    }

    fn authenticate<'a>(
        &'a self,
        request: AuthenticateRequest,
    ) -> graphhelm_events::RepositoryFuture<'a, Result<AuthenticationTag, graphhelm_events::KeyError>>
    {
        self.0.authenticate(request)
    }

    fn verify<'a>(
        &'a self,
        request: VerifyAuthenticationRequest,
    ) -> graphhelm_events::RepositoryFuture<'a, Result<(), graphhelm_events::KeyError>> {
        self.0.verify(request)
    }
}

struct ProcessResult {
    success: bool,
    stdout: Vec<u8>,
    stdout_truncated: bool,
    stderr_truncated: bool,
}

/// One budget for the whole operation, not a cap per step (#81 item 2). Before this,
/// eleven independent 30 s caps meant every added step silently added another chance to
/// fail under load, and the compound probability is exactly the rotating-victim gate
/// flake the issue documents. Each step now spends from ONE budget; the per-step ceiling
/// survives only as an anti-hang bound.
///
/// `step()` may return ZERO once the budget is spent: a step started after exhaustion
/// fails immediately with `DeadlineElapsed` instead of waiting out its own private cap.
#[derive(Clone, Copy)]
struct OperationDeadline {
    deadline: Instant,
}

impl OperationDeadline {
    fn new(budget: Duration) -> Self {
        Self {
            deadline: Instant::now() + budget,
        }
    }

    /// The lesser of the operation's remaining budget and a per-step anti-hang ceiling.
    fn step(&self, ceiling: Duration) -> Duration {
        self.remaining().min(ceiling)
    }

    /// Time this operation may still spend.
    fn remaining(&self) -> Duration {
        self.deadline.saturating_duration_since(Instant::now())
    }
}

/// The exclusivity poll's exit decision (#81, disclosed hazard: this loop is a HAND-ROLLED
/// deadline, not a `timeout` wrapper). Pure, so the contention/elapsed split is
/// unit-testable without a database. `Contention` outranks `Elapsed` deliberately: a
/// contended target at the deadline is still contended, and reporting it as timing would
/// tell the operator to retry a restore whose target someone else holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExclusivityPoll {
    /// Exactly our own connection: proceed.
    Proceed,
    /// A rival connection holds the target: not a timing fact.
    Contention,
    /// Nobody rivals us and the budget ran out: a timing fact.
    Elapsed,
    /// Nobody rivals us and there is budget left: poll again.
    Waiting,
}

/// Poll until the target is exclusively ours, the target is contended, or the budget is gone.
///
/// THE BOUND LIVES HERE, NOT IN THE CALLER'S QUERY (#866). The loop this replaces consulted the
/// clock once per iteration and awaited the status query with no bound of its own, so ten seconds
/// bounded HOW MANY TIMES it looked rather than how long it took. `DeadlineElapsed` was then a
/// timing fact reported for a budget that had not actually been enforced.
///
/// The remedy #796 declined -- give the innermost blocking call the remaining budget -- is the one
/// #763 has since merged in this same subsystem (`54e4e3b5`), so the argument for leaving this
/// advisory rests on a decision that was reversed.
///
/// The poll is a closure that takes NOTHING and returns the query. That is deliberate: handing the
/// closure a `remaining` to honour would leave the real caller free to ignore it, and a cell
/// exercising a well-behaved test closure would pass while production stayed unbounded -- the bound
/// would be armed in one place and fired in another. Wrapping `poll()` here makes the subject own
/// the clock, so no caller can opt out.
///
/// The precedence of [`classify_exclusivity`] is preserved exactly: the query still runs before the
/// classification, so a target that is free AT the deadline still proceeds. A deadline already gone
/// is not a short-circuit -- `timeout_at` polls once before it checks -- which is the same rule
/// [`OperationDeadline::step`] documents for a step begun after exhaustion.
///
/// The deadline is a [`tokio::time::Instant`] because that is the clock `timeout_at` enforces.
/// Measuring the bound against `std::time::Instant` while the timer runs on tokio's makes the two
/// disagree the moment a test pauses time -- and the first version of the cells below passed with
/// this bound REMOVED for exactly that reason.
async fn poll_until_exclusive<P, F>(
    deadline: tokio::time::Instant,
    mut poll: P,
) -> Result<(), BackupError>
where
    P: FnMut() -> F,
    F: std::future::Future<Output = Result<i64, sqlx::Error>>,
{
    loop {
        let connected = tokio::time::timeout_at(deadline, poll())
            .await
            // The wait itself ran out: a timing fact, and now a true one.
            .map_err(|_| BackupError::DeadlineElapsed)?
            .map_err(|_| BackupError::InvalidRestore)?;
        // Converted to the std instant the pure classifier takes. Both sides come from the SAME clock,
        // which is the whole reason the deadline changed type.
        match classify_exclusivity(
            connected,
            tokio::time::Instant::now().into_std(),
            deadline.into_std(),
        ) {
            ExclusivityPoll::Proceed => return Ok(()),
            // A rival holds the target: NOT a timing fact -- the restore target is genuinely not
            // exclusively ours, however much time remains. Folding this into "elapsed" would
            // replace one flattening with another.
            ExclusivityPoll::Contention => return Err(BackupError::InvalidRestore),
            // Nobody rivals us and the budget ran out: a timing fact, named as one.
            ExclusivityPoll::Elapsed => return Err(BackupError::DeadlineElapsed),
            ExclusivityPoll::Waiting => {}
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn classify_exclusivity(connected: i64, now: Instant, deadline: Instant) -> ExclusivityPoll {
    if connected == 1 {
        ExclusivityPoll::Proceed
    } else if connected > 1 {
        ExclusivityPoll::Contention
    } else if now >= deadline {
        ExclusivityPoll::Elapsed
    } else {
        ExclusivityPoll::Waiting
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PostMarkerAcquisition {
    ReleaseEmpty,
    VerifyCompleted,
    AwaitRestore,
    PreserveForRecovery,
}

fn classify_post_marker_acquisition(
    restore_pid_count: usize,
    other_sessions: Option<i64>,
    target_objects: Option<i64>,
    restore_completed: bool,
) -> PostMarkerAcquisition {
    if restore_pid_count == 0 && other_sessions == Some(0) {
        match target_objects {
            Some(0) if restore_completed => PostMarkerAcquisition::ReleaseEmpty,
            Some(0) => PostMarkerAcquisition::AwaitRestore,
            Some(objects) if objects > 0 => PostMarkerAcquisition::VerifyCompleted,
            _ => PostMarkerAcquisition::PreserveForRecovery,
        }
    } else {
        PostMarkerAcquisition::PreserveForRecovery
    }
}

fn run_bounded_process(
    executable: &Path,
    arguments: &[&str],
    environment: &[(&str, &str)],
    timeout: Duration,
) -> Result<ProcessResult, BackupError> {
    use std::process::{Command, Stdio};
    let mut command = Command::new(executable);
    command
        .args(arguments)
        .env_clear()
        .envs(environment.iter().copied())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    if let Some(system_root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", system_root);
    }
    configure_process_group(&mut command);
    let mut child = command.spawn().map_err(|_| BackupError::InvalidBackup)?;
    let stdout = child.stdout.take().ok_or(BackupError::InvalidBackup)?;
    let stderr = child.stderr.take().ok_or(BackupError::InvalidBackup)?;
    let stdout_reader = std::thread::spawn(move || read_bounded_output(stdout));
    let stderr_reader = std::thread::spawn(move || read_bounded_output(stderr));
    let status = ProcessWatchdog::start(child, timeout)?.finish();
    let (stdout, stdout_truncated) = stdout_reader
        .join()
        .map_err(|_| BackupError::InvalidBackup)??;
    let (_, stderr_truncated) = stderr_reader
        .join()
        .map_err(|_| BackupError::InvalidBackup)??;
    let status = status?;
    Ok(ProcessResult {
        success: status.success(),
        stdout,
        stdout_truncated,
        stderr_truncated,
    })
}

#[cfg(test)]
mod process_tests {
    use super::*;

    #[test]
    fn postgres_commands_are_direct_bounded_argument_vectors_without_secret_material() {
        let executable = std::env::current_exe().unwrap();
        let passfile = std::env::temp_dir().join("graphhelm-command-test.pgpass");
        let profile =
            DatabaseProcessProfile::new("127.0.0.1", 5432, "admin", "target_db", &passfile)
                .unwrap();
        let snapshot = "snapshot;$(not-a-shell)|& value";
        let dump = dump_command(&executable, &profile, snapshot);
        assert_eq!(dump.get_program(), executable.as_os_str());
        assert_eq!(
            dump.get_args().collect::<Vec<_>>(),
            [
                "--format=custom",
                "--no-owner",
                "--no-privileges",
                "--snapshot",
                snapshot,
            ]
            .map(std::ffi::OsStr::new)
        );
        let application = "restore;$(not-a-shell)|& value";
        let restore = restore_command(&executable, &profile, application);
        assert_eq!(restore.get_program(), executable.as_os_str());
        assert_eq!(
            restore.get_args().collect::<Vec<_>>(),
            [
                "--exit-on-error",
                "--single-transaction",
                "--no-owner",
                "--no-privileges",
                "--dbname",
                "target_db",
            ]
            .map(std::ffi::OsStr::new)
        );
        assert!(
            dump.get_envs()
                .any(|(key, value)| { key == "PGPASSFILE" && value == Some(passfile.as_os_str()) })
        );
        assert!(
            restore
                .get_envs()
                .any(|(key, value)| { key == "PGPASSFILE" && value == Some(passfile.as_os_str()) })
        );
        assert!(restore.get_envs().any(|(key, value)| {
            key == "PGAPPNAME" && value == Some(std::ffi::OsStr::new(application))
        }));
        for command in [&dump, &restore] {
            let material = command
                .get_args()
                .chain(command.get_envs().filter_map(|(_, value)| value))
                .map(|value| value.to_string_lossy())
                .collect::<Vec<_>>()
                .join("\n");
            assert!(!material.contains("secret-canary"));
        }
    }

    #[test]
    fn long_stream_integrity_is_partitioned_into_bounded_ranges() {
        assert_eq!(
            verification_ranges(200_001).collect::<Vec<_>>(),
            vec![(1, 100_000), (100_001, 100_000), (200_001, 1)]
        );
    }

    #[test]
    fn profile_passfile_is_bounded_before_reading() {
        let path = std::env::temp_dir().join(format!(
            "graphhelm-passfile-bound-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::write(&path, vec![b'x'; 4097]).unwrap();
        assert!(matches!(
            read_profile_passfile(&path),
            Err(BackupError::InvalidBackup)
        ));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn directory_sync_failure_rolls_back_the_published_link() {
        let root = owned_test_root("sync-failure");
        let destination = root.join("archive.ghb");
        let mut temporary = OwnedTemporary::for_destination(&destination).unwrap();
        temporary
            .file_mut()
            .write_all(b"authenticated archive")
            .unwrap();
        fail_next_directory_sync();
        let result = temporary.publish(&destination);
        assert!(temporary.rollback_published.is_some());
        drop(temporary);
        assert_eq!(result, Err(BackupError::Unavailable));
        assert!(!destination.exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn rollback_restores_a_concurrent_replacement_without_deleting_it() {
        let root = owned_test_root("rollback-race");
        let destination = root.join("archive.ghb");
        let displaced = root.join("owned-displaced.ghb");
        let mut temporary = OwnedTemporary::for_destination(&destination).unwrap();
        temporary.file_mut().write_all(b"owned archive").unwrap();
        fail_next_directory_sync();
        assert_eq!(
            temporary.publish(&destination),
            Err(BackupError::Unavailable)
        );
        set_before_linux_rollback_rename({
            let displaced = displaced.clone();
            move |path| {
                std::fs::rename(path, &displaced).unwrap();
                std::fs::write(path, b"concurrent replacement").unwrap();
            }
        });
        drop(temporary);
        assert_eq!(
            std::fs::read(&destination).unwrap(),
            b"concurrent replacement"
        );
        assert_eq!(std::fs::read(&displaced).unwrap(), b"owned archive");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn linux_publication_requires_an_euid_owned_non_writable_parent() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!(
            "graphhelm-backup-parent-mode-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir(&root).unwrap();
        for mode in [0o700, 0o750] {
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(mode)).unwrap();
            let destination = root.join(format!("accepted-{mode:o}.ghb"));
            assert!(OwnedTemporary::for_destination(&destination).is_ok());
        }
        for mode in [0o720, 0o702, 0o777] {
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(mode)).unwrap();
            let destination = root.join(format!("rejected-{mode:o}.ghb"));
            assert!(matches!(
                OwnedTemporary::for_destination(&destination),
                Err(BackupError::InvalidBackup)
            ));
        }
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn linux_publication_errno_decisions() {
        assert!(empty_path_link_needs_proc_fallback(Some(libc::EPERM)));
        assert!(empty_path_link_needs_proc_fallback(Some(libc::ENOENT)));
        assert!(!empty_path_link_needs_proc_fallback(Some(libc::EEXIST)));
        assert!(!empty_path_link_needs_proc_fallback(Some(libc::EXDEV)));
        assert!(!empty_path_link_needs_proc_fallback(None));
        assert!(tmpfile_is_unsupported(Some(libc::EOPNOTSUPP)));
        assert!(tmpfile_is_unsupported(Some(libc::EISDIR)));
        assert!(tmpfile_is_unsupported(Some(libc::EINVAL)));
        assert!(!tmpfile_is_unsupported(Some(libc::EACCES)));
        assert!(!tmpfile_is_unsupported(Some(libc::ENOSPC)));
        assert!(!tmpfile_is_unsupported(Some(libc::EEXIST)));
        assert!(!tmpfile_is_unsupported(None));
    }

    /// A fresh publication parent. Mode 0700 on Unix so the fixture does not depend on the
    /// caller's umask: under Ubuntu's default 0002 `create_dir` makes a group-writable parent,
    /// which the Linux publication check refuses by design.
    fn owned_test_root(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "graphhelm-backup-{label}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir(&root).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        root
    }

    #[cfg(target_os = "linux")]
    fn entries(root: &Path) -> Vec<String> {
        let mut names = std::fs::read_dir(root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect::<Vec<_>>();
        names.sort();
        names
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn tmpfile_unsupported_falls_back_to_an_exclusive_named_pending_file() {
        use std::os::unix::fs::PermissionsExt;
        let root = owned_test_root("named-publish");
        let destination = root.join("archive.ghb");
        fail_next_tmpfile(libc::EOPNOTSUPP);
        let mut temporary = OwnedTemporary::for_destination(&destination).unwrap();
        let pending = temporary.path.clone();
        let name = pending.file_name().unwrap().to_str().unwrap().to_owned();
        assert_eq!(pending.parent(), Some(root.as_path()));
        assert!(name.starts_with(".graphhelm-backup-") && name.ends_with(".pending"));
        assert_eq!(
            std::fs::symlink_metadata(&pending)
                .unwrap()
                .permissions()
                .mode()
                & 0o077,
            0
        );
        temporary.file_mut().write_all(b"named archive").unwrap();
        assert!(!destination.exists(), "nothing is published before commit");
        temporary.publish(&destination).unwrap();
        drop(temporary);
        assert_eq!(std::fs::read(&destination).unwrap(), b"named archive");
        assert_eq!(entries(&root), vec!["archive.ghb".to_owned()]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn named_pending_is_removed_when_dropped_unpublished() {
        for errno in [libc::EOPNOTSUPP, libc::EISDIR, libc::EINVAL] {
            let root = owned_test_root("named-drop");
            let destination = root.join("archive.ghb");
            fail_next_tmpfile(errno);
            let mut temporary = OwnedTemporary::for_destination(&destination).unwrap();
            assert!(!temporary.path.as_os_str().is_empty(), "errno {errno}");
            temporary.file_mut().write_all(b"partial").unwrap();
            drop(temporary);
            assert!(entries(&root).is_empty(), "errno {errno}");
            std::fs::remove_dir(root).unwrap();
        }
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn named_pending_publication_is_no_replace() {
        let root = owned_test_root("named-noreplace");
        let destination = root.join("archive.ghb");
        fail_next_tmpfile(libc::EOPNOTSUPP);
        let mut temporary = OwnedTemporary::for_destination(&destination).unwrap();
        temporary.file_mut().write_all(b"owned archive").unwrap();
        std::fs::write(&destination, b"concurrent writer").unwrap();
        assert_eq!(
            temporary.publish(&destination),
            Err(BackupError::InvalidBackup)
        );
        drop(temporary);
        assert_eq!(std::fs::read(&destination).unwrap(), b"concurrent writer");
        assert_eq!(entries(&root), vec!["archive.ghb".to_owned()]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn named_pending_directory_sync_failure_rolls_back_both_names() {
        let root = owned_test_root("named-rollback");
        let destination = root.join("archive.ghb");
        fail_next_tmpfile(libc::EOPNOTSUPP);
        let mut temporary = OwnedTemporary::for_destination(&destination).unwrap();
        temporary.file_mut().write_all(b"owned archive").unwrap();
        fail_next_directory_sync();
        assert_eq!(
            temporary.publish(&destination),
            Err(BackupError::Unavailable)
        );
        assert!(temporary.rollback_published.is_some());
        drop(temporary);
        assert!(entries(&root).is_empty());
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn named_pending_is_never_unlinked_after_a_swap() {
        let root = owned_test_root("named-swap");
        let destination = root.join("archive.ghb");
        fail_next_tmpfile(libc::EOPNOTSUPP);
        let temporary = OwnedTemporary::for_destination(&destination).unwrap();
        let pending = temporary.path.clone();
        let displaced = root.join("displaced");
        std::fs::rename(&pending, &displaced).unwrap();
        std::fs::write(&pending, b"attacker replacement").unwrap();
        drop(temporary);
        assert_eq!(std::fs::read(&pending).unwrap(), b"attacker replacement");
        assert!(displaced.exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn tmpfile_failure_other_than_unsupported_is_not_masked() {
        let root = owned_test_root("named-eacces");
        fail_next_tmpfile(libc::EACCES);
        assert!(matches!(
            OwnedTemporary::for_destination(&root.join("archive.ghb")),
            Err(BackupError::Unavailable)
        ));
        assert!(entries(&root).is_empty());
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn owned_backup_is_not_published_before_explicit_commit() {
        let root = owned_test_root("commit-test");
        let destination = root.join("archive.ghb");
        {
            let mut temporary = OwnedTemporary::for_destination(&destination).unwrap();
            temporary
                .file_mut()
                .write_all(b"complete encrypted archive")
                .unwrap();
            temporary.file().sync_all().unwrap();
        }
        assert!(!destination.exists());
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    #[cfg(windows)]
    fn publication_never_links_a_replacement_of_the_owned_pending_file() {
        let root = std::env::temp_dir().join(format!(
            "graphhelm-backup-publish-identity-test-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir(&root).unwrap();
        let destination = root.join("archive.ghb");
        let mut temporary = OwnedTemporary::for_destination(&destination).unwrap();
        temporary
            .file_mut()
            .write_all(b"owned encrypted archive")
            .unwrap();
        let displaced = root.join("displaced");
        match std::fs::rename(&temporary.path, &displaced) {
            Ok(()) => {
                std::fs::write(&temporary.path, b"attacker replacement").unwrap();
                assert_eq!(
                    temporary.publish(&destination),
                    Err(BackupError::Unavailable)
                );
                assert!(!destination.exists());
                assert_eq!(
                    std::fs::read(&displaced).unwrap(),
                    b"owned encrypted archive"
                );
                std::fs::remove_file(&temporary.path).unwrap();
                std::fs::remove_file(displaced).unwrap();
            }
            Err(error) => {
                assert!(matches!(error.raw_os_error(), Some(5 | 32)));
                temporary.publish(&destination).unwrap();
                assert_eq!(
                    std::fs::read(&destination).unwrap(),
                    b"owned encrypted archive"
                );
                drop(temporary);
                std::fs::remove_file(destination).unwrap();
            }
        }
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn plaintext_budget_accepts_exact_limit_and_rejects_limit_plus_one() {
        let mut budget = PlaintextBudget::default();
        budget.account(MAX_BACKUP_BYTES - 1).unwrap();
        budget.account(1).unwrap();
        assert_eq!(budget.account(1), Err(BackupError::LimitExceeded));
    }

    #[test]
    fn receipt_provider_epoch_must_still_match_the_authenticated_manifest() {
        assert_eq!(ensure_provider_epoch(7, 7), Ok(()));
        assert_eq!(
            ensure_provider_epoch(7, 8),
            Err(BackupError::InvalidRestore)
        );
        assert_eq!(
            ensure_provider_epoch(7, 6),
            Err(BackupError::InvalidRestore)
        );
    }

    #[test]
    fn fake_process_child() {
        match std::env::var("GRAPHHELM_FAKE_PROCESS_MODE").as_deref() {
            Ok("success") => print!("direct-process-ok"),
            Ok("noisy") => eprint!("{}", "noise-canary".repeat(7_000)),
            Ok("hang") => std::thread::sleep(Duration::from_secs(30)),
            Ok("descendant") => {
                let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "backup::process_tests::fake_process_child",
                        "--nocapture",
                    ])
                    .env_clear()
                    .env("GRAPHHELM_FAKE_PROCESS_MODE", "hang")
                    .spawn()
                    .unwrap();
                std::fs::write(
                    std::env::var_os("GRAPHHELM_FAKE_CHILD_PID").unwrap(),
                    child.id().to_string(),
                )
                .unwrap();
                std::thread::sleep(Duration::from_secs(30));
                let _ = child.kill();
                let _ = child.wait();
            }
            Ok("nonzero") => std::process::exit(17),
            _ => {}
        }
    }

    #[test]
    fn direct_process_runner_bounds_output_timeout_and_nonzero_status() {
        let executable = std::env::current_exe().unwrap();
        let arguments = [
            "--exact",
            "backup::process_tests::fake_process_child",
            "--nocapture",
        ];
        let success = run_bounded_process(
            &executable,
            &arguments,
            &[("GRAPHHELM_FAKE_PROCESS_MODE", "success")],
            Duration::from_secs(5),
        )
        .unwrap();
        assert!(success.success);
        assert!(
            success
                .stdout
                .windows(b"direct-process-ok".len())
                .any(|window| window == b"direct-process-ok")
        );

        let noisy = run_bounded_process(
            &executable,
            &arguments,
            &[("GRAPHHELM_FAKE_PROCESS_MODE", "noisy")],
            Duration::from_secs(5),
        )
        .unwrap();
        assert!(noisy.success);
        assert!(noisy.stderr_truncated);

        let nonzero = run_bounded_process(
            &executable,
            &arguments,
            &[("GRAPHHELM_FAKE_PROCESS_MODE", "nonzero")],
            Duration::from_secs(5),
        )
        .unwrap();
        assert!(!nonzero.success);

        // #81, pre-declared fix casualty (sealed P3): this asserted `Unavailable` while
        // "timed out" hid inside "backup invalid"; a bounded child running out of time is
        // a TIMING fact and now says so.
        let timed_out = run_bounded_process(
            &executable,
            &arguments,
            &[("GRAPHHELM_FAKE_PROCESS_MODE", "hang")],
            Duration::from_millis(50),
        );
        assert!(matches!(timed_out, Err(BackupError::DeadlineElapsed)));
    }

    /// #81: the exit decision of the hand-rolled exclusivity loop, pinned pure. The
    /// contention/elapsed split is the half a database test cannot cheaply reach: a rival
    /// at the deadline is STILL contention (never timing), and an empty target past the
    /// deadline is timing (never a verdict about the target).
    #[test]
    fn exclusivity_classifier_separates_contention_from_elapsed() {
        let now = Instant::now();
        let later = now + Duration::from_secs(1);
        // Proceed: exactly our own connection, regardless of the clock.
        assert_eq!(
            classify_exclusivity(1, later, now),
            ExclusivityPoll::Proceed
        );
        // Contention outranks elapsed: a rival AT the deadline is still a rival.
        assert_eq!(
            classify_exclusivity(2, later, now),
            ExclusivityPoll::Contention
        );
        // Elapsed: nobody rivals us and the budget ran out.
        assert_eq!(
            classify_exclusivity(0, later, now),
            ExclusivityPoll::Elapsed
        );
        // Waiting: nobody rivals us and there is budget left.
        assert_eq!(
            classify_exclusivity(0, now, later),
            ExclusivityPoll::Waiting
        );
    }

    /// #866: the budget bounds the WAIT, not the number of times the loop looks.
    ///
    /// The clock used to be consulted once per iteration with an unbounded query between the
    /// readings, so a status read that hung made ten seconds bound HOW MANY TIMES this looked.
    /// `DeadlineElapsed` came back either way -- which is why an assertion on the returned error
    /// cannot see this defect at all. **The measured quantity has to be elapsed time.**
    ///
    /// Time is paused, so the elapsed figure is the virtual clock and not a race with the machine
    /// this runs on: a real-time assertion here would be exactly the load-dependent flake this
    /// subsystem's own deadline work exists to remove.
    #[tokio::test(start_paused = true)]
    async fn a_poll_that_hangs_cannot_outlive_the_budget() {
        // tokio's clock, the one `start_paused` pauses and the one the subject's bound obeys.
        let started = tokio::time::Instant::now();
        let deadline = started + Duration::from_millis(100);

        let outcome = poll_until_exclusive(deadline, || async {
            // The status read that never answers. Nothing here honours any bound -- that is the
            // point: the bound belongs to the subject, and a poll cannot opt out of it.
            tokio::time::sleep(Duration::from_secs(3600)).await;
            Ok(0)
        })
        .await;
        let elapsed = started.elapsed();

        assert!(
            matches!(outcome, Err(BackupError::DeadlineElapsed)),
            "a hanging status read did not end as a timing fact: {outcome:?}"
        );
        assert!(
            elapsed < Duration::from_millis(200),
            "the wait outlived its own deadline by {elapsed:?}: a 100 ms budget that returns after \
             an hour bounds the polls rather than the wait, which is the defect and not the fix"
        );
    }

    /// CONTROL for the cell above: an ordinary poll must still spend the WHOLE budget.
    ///
    /// Without this, a "fix" that refused immediately -- or that mistook a zero remaining for a
    /// short-circuit -- would satisfy the elapsed assertion above and look correct while making
    /// every contended restore fail on its first look.
    #[tokio::test(start_paused = true)]
    async fn a_fast_poll_still_waits_out_the_whole_budget() {
        let started = tokio::time::Instant::now();
        let deadline = started + Duration::from_millis(100);

        let outcome = poll_until_exclusive(deadline, || async { Ok(0) }).await;
        let elapsed = started.elapsed();

        assert!(
            matches!(outcome, Err(BackupError::DeadlineElapsed)),
            "a target that never frees up did not end as a timing fact: {outcome:?}"
        );
        assert!(
            elapsed >= Duration::from_millis(100),
            "the budget was abandoned after {elapsed:?} of a 100 ms deadline: refusing early passes \
             the hang assertion for the wrong reason"
        );
    }

    /// CONTROL: the classifier's precedence survives the bound.
    ///
    /// `Proceed` and `Contention` outrank `Elapsed` by design, and a bound that short-circuited a
    /// spent budget would answer `DeadlineElapsed` for a target that is free, or for one a rival
    /// genuinely holds. Both are asked at a deadline that is ALREADY gone, which is the instant a
    /// short-circuit would get wrong.
    #[tokio::test(start_paused = true)]
    async fn a_spent_budget_still_reports_what_the_query_saw() {
        let spent = tokio::time::Instant::now() - Duration::from_secs(1);

        assert!(
            matches!(
                poll_until_exclusive(spent, || async { Ok(1) }).await,
                Ok(())
            ),
            "an exclusively-ours target was refused because the budget was gone: `Proceed` outranks \
             `Elapsed`, and a step begun after exhaustion still gets its one look"
        );
        assert!(
            matches!(
                poll_until_exclusive(spent, || async { Ok(2) }).await,
                Err(BackupError::InvalidRestore)
            ),
            "a contended target was reported as a timing fact: an operator told to retry a restore \
             whose target someone else holds is the flattening #81 removed"
        );
    }

    /// CONTROL: a failing query is a restore failure, not a timing one.
    ///
    /// The two error kinds arrive through the same `?` chain now, and swapping them would tell an
    /// operator to wait when the database refused.
    #[tokio::test(start_paused = true)]
    async fn a_query_error_is_not_a_deadline() {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);

        let outcome =
            poll_until_exclusive(deadline, || async { Err(sqlx::Error::PoolClosed) }).await;

        assert!(
            matches!(outcome, Err(BackupError::InvalidRestore)),
            "a refused query was reported as an elapsed deadline: {outcome:?}"
        );
    }

    #[test]
    fn disappeared_restore_with_committed_objects_requires_verification() {
        assert_eq!(
            classify_post_marker_acquisition(0, Some(0), Some(1), true),
            PostMarkerAcquisition::VerifyCompleted,
        );
        assert_eq!(
            classify_post_marker_acquisition(0, Some(0), Some(0), true),
            PostMarkerAcquisition::ReleaseEmpty,
        );
        assert_eq!(
            classify_post_marker_acquisition(0, None, Some(0), true),
            PostMarkerAcquisition::PreserveForRecovery,
        );
        assert_eq!(
            classify_post_marker_acquisition(2, Some(0), Some(0), true),
            PostMarkerAcquisition::PreserveForRecovery,
        );
        assert_eq!(
            classify_post_marker_acquisition(0, Some(0), Some(0), false),
            PostMarkerAcquisition::AwaitRestore,
            "an empty startup snapshot must wait while the restore child is still running",
        );
        assert_eq!(
            classify_post_marker_acquisition(0, Some(0), Some(1), false),
            PostMarkerAcquisition::VerifyCompleted,
            "observed objects still use the existing verification path while the child is running",
        );
    }

    #[test]
    fn restore_catalog_locks_have_one_global_order() {
        assert_eq!(
            RESTORE_CATALOG_LOCKS,
            [
                "LOCK TABLE pg_catalog.pg_database IN SHARE ROW EXCLUSIVE MODE",
                "LOCK TABLE pg_catalog.pg_shdescription IN SHARE ROW EXCLUSIVE MODE",
                "LOCK TABLE pg_catalog.pg_authid IN SHARE ROW EXCLUSIVE MODE",
            ]
        );
        assert_eq!(RestoreCatalogLockScope::IdentityAndMarker.count(), 2);
        assert_eq!(RestoreCatalogLockScope::FullOwnership.count(), 3);
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct FakeDurableRestoreState {
        datallowconn: bool,
        marker_present: bool,
        role_present: bool,
    }

    struct FakeDropCompensator(Arc<Mutex<FakeDurableRestoreState>>);

    impl RestoreDropCompensator for FakeDropCompensator {
        fn compensate_on_drop(self, ownership: Arc<RestoreOwnership>) {
            let mut durable = self.0.lock().unwrap();
            match ownership.recovery_action() {
                RestoreRecoveryAction::EnsureClosedForRecovery => {
                    durable.datallowconn = false;
                }
                RestoreRecoveryAction::ReleaseProvedState => {
                    durable.datallowconn = true;
                    durable.marker_present = false;
                    durable.role_present = false;
                }
                RestoreRecoveryAction::CleanupFailedRestore => {
                    durable.datallowconn = true;
                    durable.marker_present = false;
                    durable.role_present = false;
                }
                RestoreRecoveryAction::Inactive
                | RestoreRecoveryAction::PreserveForRecovery
                | RestoreRecoveryAction::Complete => {}
            }
        }
    }

    #[test]
    fn drop_closes_marked_target_without_releasing_marker_or_role() {
        let durable = Arc::new(Mutex::new(FakeDurableRestoreState {
            datallowconn: true,
            marker_present: true,
            role_present: true,
        }));
        let guard =
            RestoreCleanupGuard::with_compensator(FakeDropCompensator(Arc::clone(&durable)));
        guard.ownership().authorize_ensure_closed();

        drop(guard);

        assert_eq!(
            *durable.lock().unwrap(),
            FakeDurableRestoreState {
                datallowconn: false,
                marker_present: true,
                role_present: true,
            }
        );
    }

    #[test]
    fn post_marker_failpoints_select_the_only_safe_drop_action() {
        for sabotage in ["marker_to_close", "timeout_before_close"] {
            let durable = Arc::new(Mutex::new(FakeDurableRestoreState {
                datallowconn: true,
                marker_present: true,
                role_present: true,
            }));
            let guard =
                RestoreCleanupGuard::with_compensator(FakeDropCompensator(Arc::clone(&durable)));
            guard.ownership().authorize_ensure_closed();
            assert_eq!(
                guard.ownership().recovery_action(),
                RestoreRecoveryAction::EnsureClosedForRecovery,
                "{sabotage} must force Drop to close the marked target",
            );
            drop(guard);
            assert_eq!(
                *durable.lock().unwrap(),
                FakeDurableRestoreState {
                    datallowconn: false,
                    marker_present: true,
                    role_present: true,
                },
                "{sabotage} must run close-only compensation"
            );
        }

        for sabotage in ["pg_backend_pid", "restore_pids", "timeout_after_close"] {
            let durable = Arc::new(Mutex::new(FakeDurableRestoreState {
                datallowconn: false,
                marker_present: true,
                role_present: true,
            }));
            let guard =
                RestoreCleanupGuard::with_compensator(FakeDropCompensator(Arc::clone(&durable)));
            guard.ownership().authorize_ensure_closed();
            guard.ownership().mark_closed_for_recovery();
            assert_eq!(
                guard.ownership().recovery_action(),
                RestoreRecoveryAction::PreserveForRecovery,
                "{sabotage} must preserve the closed target, marker, and role",
            );
            drop(guard);
            assert_eq!(
                *durable.lock().unwrap(),
                FakeDurableRestoreState {
                    datallowconn: false,
                    marker_present: true,
                    role_present: true,
                },
                "{sabotage} must not reopen or clean unverified content"
            );
        }
    }

    #[test]
    fn automatic_compensation_requires_an_explicit_positive_transition() {
        let ownership = RestoreOwnership::default();
        assert!(
            !ownership
                .recovery_action()
                .requires_automatic_compensation()
        );

        ownership.authorize_release();
        assert!(
            ownership
                .recovery_action()
                .requires_automatic_compensation()
        );

        let ownership = RestoreOwnership::default();
        assert!(
            !ownership
                .recovery_action()
                .requires_automatic_compensation()
        );

        ownership.authorize_cleanup();
        assert!(
            ownership
                .recovery_action()
                .requires_automatic_compensation()
        );

        ownership.mark_complete();
        assert!(
            !ownership
                .recovery_action()
                .requires_automatic_compensation()
        );
    }

    /// #81 item 2: a step never receives more than the operation has left. The zero case
    /// is the property that kills the compounding — a step started after exhaustion gets
    /// ZERO budget and fails fast as `DeadlineElapsed`, instead of enjoying a private cap
    /// the operation no longer has.
    #[test]
    fn a_step_never_outlives_the_operations_budget() {
        let spent = OperationDeadline {
            deadline: Instant::now() - Duration::from_secs(1),
        };
        assert_eq!(spent.step(Duration::from_secs(30)), Duration::ZERO);
        assert_eq!(spent.remaining(), Duration::ZERO);

        let fresh = OperationDeadline::new(Duration::from_secs(600));
        let step = fresh.step(Duration::from_secs(30));
        assert!(
            step <= Duration::from_secs(30),
            "the anti-hang ceiling caps a step even when the operation is rich: {step:?}"
        );
        assert!(
            fresh.step(Duration::from_secs(3600)) <= Duration::from_secs(600),
            "a step never receives more than the whole operation's budget"
        );
    }

    /// #81: a step that ran OUT OF TIME must not be reported with a code the operator
    /// reads as "this backup is not trustworthy". "The machine was busy" and "this
    /// archive is corrupt" have opposite responses — retry later versus never retry —
    /// and today both arrive fused (the M09 gate misdiagnosis this issue exists for).
    ///
    /// Asserted at the CODE grain, where the operator reads, for two reasons: two
    /// variants already share GHB001, so a variant-grain assertion can lie about the
    /// operator surface; and the code-grain comparison against a string literal is what
    /// lets this red COMPILE before the variant it demands exists — a variant-grain red
    /// would be a compile error, which is not a result.
    ///
    /// The sibling test above is this red's harness control: it proves (green, today)
    /// that the hang arrangement genuinely elapses at this bound. If IT fails, the
    /// arrangement broke — nothing here measured anything.
    #[test]
    fn a_bounded_process_that_exceeds_its_deadline_names_elapsed_not_corruption() {
        let executable = std::env::current_exe().unwrap();
        let arguments = [
            "--exact",
            "backup::process_tests::fake_process_child",
            "--nocapture",
        ];
        let Err(elapsed) = run_bounded_process(
            &executable,
            &arguments,
            &[("GRAPHHELM_FAKE_PROCESS_MODE", "hang")],
            Duration::from_millis(50),
        ) else {
            panic!("a 50ms bound on a hanging child must not succeed");
        };
        assert_eq!(
            elapsed.code(),
            "GHB003_DEADLINE_ELAPSED",
            "a bounded child that ran out of time must name TIMING, not arrive under a \
             code the operator reads as untrustworthy-backup or unavailability"
        );
    }

    #[test]
    fn process_watchdog_owns_kills_and_reaps_the_child() {
        use std::process::{Command, Stdio};

        let executable = std::env::current_exe().unwrap();
        let mut command = Command::new(executable);
        command
            .args([
                "--exact",
                "backup::process_tests::fake_process_child",
                "--nocapture",
            ])
            .env_clear()
            .env("GRAPHHELM_FAKE_PROCESS_MODE", "hang")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        configure_process_group(&mut command);
        let child = command.spawn().unwrap();
        let process_id = child.id();
        let watchdog = ProcessWatchdog::start(child, Duration::from_millis(50)).unwrap();

        let result = watchdog.finish();

        // #81 casualty, UNSEALED (reported as such — the seal's census named only the
        // run_bounded_process pin): this test pins kill-and-reap, and its error assertion
        // rode the old fused mapping. A 50 ms watchdog on a hanging child is a deadline
        // that elapsed. The CANCELLATION test below stays `Unavailable` on purpose — a
        // deliberate cancel is an availability fact, which is the #81 split itself.
        assert_eq!(result, Err(BackupError::DeadlineElapsed));
        assert!(!process_is_running(process_id));
    }

    #[test]
    fn process_watchdog_releases_before_reaping_a_normally_finished_child() {
        use std::process::{Command, Stdio};

        let executable = std::env::current_exe().unwrap();
        let mut command = Command::new(executable);
        command
            .args([
                "--exact",
                "backup::process_tests::fake_process_child",
                "--nocapture",
            ])
            .env_clear()
            .env("GRAPHHELM_FAKE_PROCESS_MODE", "success")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        configure_process_group(&mut command);
        let child = command.spawn().unwrap();
        let process_id = child.id();
        let lifecycle = Arc::new(Mutex::new(LifecycleProbe::default()));

        let status = ProcessWatchdog::start(child, Duration::from_secs(5))
            .unwrap()
            .with_lifecycle_probe(Arc::clone(&lifecycle))
            .finish()
            .unwrap();

        assert!(status.success());
        assert!(!process_is_running(process_id));
        let lifecycle = lifecycle.lock().unwrap();
        assert_eq!(lifecycle.events, ["release", "reap"]);
        #[cfg(unix)]
        {
            assert!(lifecycle.release_waitable);
            assert!(lifecycle.reaped);
        }
    }

    #[test]
    fn abort_guard_signals_cancellation_before_aborting_the_task() {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(async {
                let cancelled = Arc::new(AtomicBool::new(false));
                let task = tokio::spawn(std::future::pending::<()>());
                let guard = AbortTaskOnDrop::new(task, Arc::clone(&cancelled));

                drop(guard);

                assert!(cancelled.load(Ordering::Acquire));
            });
    }

    #[test]
    fn cancelling_wait_aborts_the_owned_inner_task() {
        struct DropFlag(Arc<AtomicBool>);

        impl Drop for DropFlag {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Release);
            }
        }

        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(async {
                let cancelled = Arc::new(AtomicBool::new(false));
                let inner_dropped = Arc::new(AtomicBool::new(false));
                let inner_flag = Arc::clone(&inner_dropped);
                let inner = tokio::spawn(async move {
                    let _flag = DropFlag(inner_flag);
                    std::future::pending::<()>().await;
                });
                let guard = AbortTaskOnDrop::new(inner, Arc::clone(&cancelled));
                let outer = tokio::spawn(guard.wait());
                tokio::task::yield_now().await;

                outer.abort();
                let _ = outer.await;
                tokio::task::yield_now().await;

                assert!(cancelled.load(Ordering::Acquire));
                assert!(inner_dropped.load(Ordering::Acquire));
            });
    }

    #[test]
    fn cancellation_signal_kills_and_reaps_a_blocked_process_without_waiting_for_timeout() {
        use std::process::{Command, Stdio};

        let executable = std::env::current_exe().unwrap();
        let mut command = Command::new(executable);
        command
            .args([
                "--exact",
                "backup::process_tests::fake_process_child",
                "--nocapture",
            ])
            .env_clear()
            .env("GRAPHHELM_FAKE_PROCESS_MODE", "hang")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        configure_process_group(&mut command);
        let child = command.spawn().unwrap();
        let process_id = child.id();
        let cancelled = Arc::new(AtomicBool::new(false));
        let watchdog = ProcessWatchdog::start_with_cancellation(
            child,
            Duration::from_secs(30),
            Arc::clone(&cancelled),
        )
        .unwrap();

        cancelled.store(true, Ordering::Release);
        let started = Instant::now();
        let result = watchdog.finish();

        assert_eq!(result, Err(BackupError::Unavailable));
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(!process_is_running(process_id));
    }

    #[test]
    fn cancelled_watchdog_kills_the_owned_process_tree() {
        use std::process::{Command, Stdio};

        let pid_path = std::env::temp_dir().join(format!(
            "graphhelm-process-tree-{}.pid",
            uuid::Uuid::new_v4().simple()
        ));
        let executable = std::env::current_exe().unwrap();
        let mut command = Command::new(executable);
        command
            .args([
                "--exact",
                "backup::process_tests::fake_process_child",
                "--nocapture",
            ])
            .env_clear()
            .env("GRAPHHELM_FAKE_PROCESS_MODE", "descendant")
            .env("GRAPHHELM_FAKE_CHILD_PID", &pid_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        configure_process_group(&mut command);
        let child = command.spawn().unwrap();
        let parent_id = child.id();
        let lifecycle = Arc::new(Mutex::new(LifecycleProbe::default()));
        let watchdog = ProcessWatchdog::start(child, Duration::from_secs(10))
            .unwrap()
            .with_lifecycle_probe(Arc::clone(&lifecycle));
        let deadline = Instant::now() + Duration::from_secs(3);
        while !pid_path.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        let descendant_id = std::fs::read_to_string(&pid_path)
            .unwrap()
            .parse::<u32>()
            .unwrap();

        drop(watchdog);

        assert!(!process_is_running(parent_id));
        assert!(!process_is_running(descendant_id));
        let lifecycle = lifecycle.lock().unwrap();
        assert_eq!(lifecycle.events, ["release", "reap"]);
        #[cfg(unix)]
        {
            assert!(lifecycle.release_waitable);
            assert!(lifecycle.reaped);
        }
        std::fs::remove_file(pid_path).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn pinned_tool_rejects_a_reparse_path_before_execution() {
        use std::os::windows::fs::symlink_file;
        let root = std::env::temp_dir().join(format!(
            "graphhelm-tool-link-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir(&root).unwrap();
        let executable = std::env::current_exe().unwrap();
        let link = root.join("tool.exe");
        match symlink_file(&executable, &link) {
            Ok(()) => {}
            Err(error) if error.raw_os_error() == Some(1314) => {
                std::fs::remove_dir(root).unwrap();
                return;
            }
            Err(error) => panic!("unexpected symlink error: {error}"),
        }
        let digest = hex::encode(Sha256::digest(std::fs::read(&executable).unwrap()));
        let tool = PinnedTool::new(link.clone(), digest, "irrelevant").unwrap();
        assert_eq!(
            tool.verify_identity(Duration::from_secs(1)),
            Err(BackupError::InvalidBackup)
        );
        std::fs::remove_file(link).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}

fn read_bounded_output(mut reader: impl Read) -> Result<(Vec<u8>, bool), BackupError> {
    let mut output = Vec::with_capacity(MAX_PROCESS_OUTPUT_BYTES);
    let mut buffer = [0_u8; 8192];
    let mut truncated = false;
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|_| BackupError::InvalidBackup)?;
        if read == 0 {
            break;
        }
        let remaining = MAX_PROCESS_OUTPUT_BYTES.saturating_sub(output.len());
        let retained = remaining.min(read);
        output.extend_from_slice(&buffer[..retained]);
        truncated |= retained < read;
    }
    Ok((output, truncated))
}

/// Exact canonical-state counts authenticated by the backup manifest.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackupCounts {
    pub stream_heads: u64,
    pub checkpoints: u64,
    pub artifact_references: u64,
    pub evidence: u64,
    pub tombstones: u64,
    pub retention_receipts: u64,
    pub projection_generations: u64,
}

/// Validated semantic identity and summary captured from the exported snapshot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackupManifest {
    source_identity_sha256: String,
    source_database_semantics: DatabaseSemanticIdentity,
    schema_migration_version: u32,
    repository_format: String,
    release_catalog_sha256: String,
    migration_sha256: Vec<String>,
    counts: BackupCounts,
    pg_dump_version: String,
    pg_dump_sha256: String,
    pg_restore_version: String,
    pg_restore_sha256: String,
    runtime_role: String,
    provider_epoch: u64,
    state_summary_sha256: String,
    privilege_summary_sha256: String,
}

impl BackupManifest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        source_identity_sha256: String,
        source_database_semantics: DatabaseSemanticIdentity,
        schema_migration_version: u32,
        repository_format: impl Into<String>,
        release_catalog_sha256: String,
        migration_sha256: Vec<String>,
        counts: BackupCounts,
        pg_dump_version: impl Into<String>,
        pg_dump_sha256: String,
        pg_restore_version: impl Into<String>,
        pg_restore_sha256: String,
        runtime_role: impl Into<String>,
        provider_epoch: u64,
    ) -> Result<Self, BackupError> {
        let repository_format = repository_format.into();
        let pg_dump_version = pg_dump_version.into();
        let pg_restore_version = pg_restore_version.into();
        let runtime_role = runtime_role.into();
        let hashes_valid = valid_sha256(&source_identity_sha256)
            && valid_sha256(&release_catalog_sha256)
            && migration_sha256.len() <= 64
            && migration_sha256.iter().all(|value| valid_sha256(value))
            && valid_sha256(&pg_dump_sha256)
            && valid_sha256(&pg_restore_sha256);
        let repository_token = |value: &str| {
            !value.is_empty()
                && value.len() <= 64
                && value.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b' ')
                })
        };
        let tool_version = |value: &str| {
            !value.is_empty()
                && value.len() <= 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_graphic() || byte == b' ')
        };
        if !hashes_valid
            || validate_database_semantic_contract(&source_database_semantics).is_err()
            || schema_migration_version == 0
            || !repository_token(&repository_format)
            || !tool_version(&pg_dump_version)
            || !tool_version(&pg_restore_version)
            || runtime_role.is_empty()
            || runtime_role.len() > 63
            || !runtime_role
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            || provider_epoch > 9_007_199_254_740_991
        {
            return Err(BackupError::InvalidBackup);
        }
        Ok(Self {
            source_identity_sha256,
            source_database_semantics,
            schema_migration_version,
            repository_format,
            release_catalog_sha256,
            migration_sha256,
            counts,
            pg_dump_version,
            pg_dump_sha256,
            pg_restore_version,
            pg_restore_sha256,
            runtime_role,
            provider_epoch,
            state_summary_sha256: hex::encode(Sha256::digest([])),
            privilege_summary_sha256: hex::encode(Sha256::digest([])),
        })
    }

    #[must_use]
    pub fn source_identity_sha256(&self) -> &str {
        &self.source_identity_sha256
    }
}

/// Bounded receipt for an encrypted archive publication.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackupReceipt {
    chunk_count: u64,
    plaintext_bytes: u64,
    ciphertext_bytes: u64,
    archive_sha256: String,
}

impl BackupReceipt {
    #[must_use]
    pub const fn chunk_count(&self) -> u64 {
        self.chunk_count
    }
}

/// Result of complete authentication and the second-pass decryption.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedBackup {
    footer: FooterCore,
}

impl VerifiedBackup {
    #[must_use]
    pub const fn manifest(&self) -> &BackupManifest {
        &self.footer.manifest
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WrappedWire {
    key_id: String,
    handle: String,
    nonce: String,
    ciphertext: String,
    aad_sha256: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProviderWire {
    key_id: String,
    algorithm: String,
    version: String,
    epoch: u64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TagWire {
    key_id: String,
    algorithm: String,
    bytes: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HeaderCore {
    format_version: u32,
    algorithm: String,
    chunk_bytes: u32,
    nonce_prefix: String,
    wrapped_key: WrappedWire,
    provider: ProviderWire,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HeaderEnvelope {
    core: HeaderCore,
    authentication_tag: TagWire,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FooterCore {
    manifest: BackupManifest,
    header_sha256: String,
    chunk_count: u64,
    plaintext_bytes: u64,
    ciphertext_bytes: u64,
    ciphertext_sha256: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FooterEnvelope {
    core: FooterCore,
    authentication_tag: TagWire,
}

/// Streaming AEAD codec shared by backup creation and the three-pass restore boundary.
pub struct BackupCodec {
    key_provider: Arc<dyn KeyProvider>,
}

impl BackupCodec {
    #[must_use]
    pub fn new(key_provider: Arc<dyn KeyProvider>) -> Self {
        Self { key_provider }
    }

    pub async fn encrypt<R: Read, W: Write>(
        &self,
        manifest: &BackupManifest,
        mut input: R,
        mut output: W,
    ) -> Result<BackupReceipt, BackupError> {
        let metadata = self
            .key_provider
            .metadata()
            .await
            .map_err(|_| BackupError::KeyUnavailable)?;
        if metadata.current_revocation_epoch() != manifest.provider_epoch {
            return Err(BackupError::InvalidBackup);
        }
        let mut dek = Zeroizing::new([0_u8; 32]);
        getrandom::fill(dek.as_mut()).map_err(|_| unavailable(UnavailableStage::Random))?;
        let mut nonce_prefix = [0_u8; 16];
        if getrandom::fill(&mut nonce_prefix).is_err() {
            nonce_prefix.zeroize();
            return Err(unavailable(UnavailableStage::Random));
        }
        let handle = format!("backup-{}", hex::encode(nonce_prefix));
        let key_aad = canonical_bytes(&(
            "graphhelm-backup-key-v1",
            &manifest.source_identity_sha256,
            metadata.key_id(),
            metadata.algorithm(),
            metadata.version(),
            metadata.current_revocation_epoch(),
        ))?;
        let wrapped = self
            .key_provider
            .wrap(
                WrapKeyRequest::new(handle.clone(), SecretBytes::new(dek.to_vec()), key_aad)
                    .map_err(|_| BackupError::KeyUnavailable)?,
            )
            .await
            .map_err(|_| BackupError::KeyUnavailable)?;
        if wrapped.key_id() != metadata.key_id() || wrapped.handle() != handle {
            return Err(BackupError::KeyUnavailable);
        }
        let header_core = HeaderCore {
            format_version: 1,
            algorithm: "xchacha20poly1305".to_owned(),
            chunk_bytes: CHUNK_BYTES as u32,
            nonce_prefix: hex::encode(nonce_prefix),
            wrapped_key: wrapped_to_wire(&wrapped),
            provider: ProviderWire {
                key_id: metadata.key_id().to_owned(),
                algorithm: metadata.algorithm().to_owned(),
                version: metadata.version().to_owned(),
                epoch: metadata.current_revocation_epoch(),
            },
        };
        let header_core_bytes = canonical_bytes(&header_core)?;
        let header_tag = self
            .key_provider
            .authenticate(
                AuthenticateRequest::new(BACKUP_PURPOSE_HEADER, header_core_bytes.clone())
                    .map_err(|_| BackupError::InvalidBackup)?,
            )
            .await
            .map_err(|_| BackupError::KeyUnavailable)?;
        if header_tag.key_id() != metadata.key_id()
            || self
                .key_provider
                .metadata()
                .await
                .map_err(|_| BackupError::KeyUnavailable)?
                != metadata
        {
            return Err(BackupError::KeyUnavailable);
        }
        self.key_provider
            .verify(
                VerifyAuthenticationRequest::new(
                    BACKUP_PURPOSE_HEADER,
                    header_core_bytes.clone(),
                    header_tag.clone(),
                )
                .map_err(|_| BackupError::KeyUnavailable)?,
            )
            .await
            .map_err(|_| BackupError::KeyUnavailable)?;
        let header_bytes = canonical_bytes(&HeaderEnvelope {
            core: header_core,
            authentication_tag: tag_to_wire(&header_tag),
        })?;
        if header_bytes.len() > MAX_HEADER_BYTES {
            return Err(BackupError::LimitExceeded);
        }
        output
            .write_all(MAGIC)
            .map_err(|_| unavailable(UnavailableStage::ArchiveWrite))?;
        write_u32(&mut output, header_bytes.len())?;
        output
            .write_all(&header_bytes)
            .map_err(|_| unavailable(UnavailableStage::ArchiveWrite))?;

        let header_sha256 = Sha256::digest(&header_core_bytes);
        let cipher = XChaCha20Poly1305::new_from_slice(dek.as_slice())
            .map_err(|_| BackupError::InvalidBackup)?;
        let mut buffer = Zeroizing::new(vec![0_u8; CHUNK_BYTES]);
        let mut chunk_count = 0_u64;
        let mut plaintext_budget = PlaintextBudget::default();
        let mut ciphertext_bytes = 0_u64;
        let mut ciphertext_digest = Sha256::new();
        loop {
            let read = read_chunk(&mut input, buffer.as_mut_slice())
                .map_err(|_| unavailable(UnavailableStage::PipeRead))?;
            if read == 0 {
                break;
            }
            plaintext_budget.account(read as u64)?;
            let nonce = chunk_nonce(&nonce_prefix, chunk_count);
            let aad = chunk_aad(&header_sha256, chunk_count, read as u32);
            let nonce = XNonce::from(nonce);
            let ciphertext = cipher
                .encrypt(
                    &nonce,
                    Payload {
                        msg: &buffer[..read],
                        aad: &aad,
                    },
                )
                .map_err(|_| BackupError::InvalidBackup)?;
            output
                .write_all(CHUNK_MARKER)
                .map_err(|_| unavailable(UnavailableStage::ArchiveWrite))?;
            write_u64(&mut output, chunk_count)?;
            write_u32(&mut output, read)?;
            write_u32(&mut output, ciphertext.len())?;
            output
                .write_all(&ciphertext)
                .map_err(|_| unavailable(UnavailableStage::ArchiveWrite))?;
            ciphertext_digest.update(&ciphertext);
            ciphertext_bytes = ciphertext_bytes
                .checked_add(ciphertext.len() as u64)
                .ok_or(BackupError::LimitExceeded)?;
            chunk_count = chunk_count
                .checked_add(1)
                .ok_or(BackupError::LimitExceeded)?;
        }
        let footer_core = FooterCore {
            manifest: manifest.clone(),
            header_sha256: hex::encode(header_sha256),
            chunk_count,
            plaintext_bytes: plaintext_budget.total(),
            ciphertext_bytes,
            ciphertext_sha256: hex::encode(ciphertext_digest.finalize()),
        };
        let footer_core_bytes = canonical_bytes(&footer_core)?;
        let footer_tag = self
            .key_provider
            .authenticate(
                AuthenticateRequest::new(BACKUP_PURPOSE_MANIFEST, footer_core_bytes.clone())
                    .map_err(|_| BackupError::InvalidBackup)?,
            )
            .await
            .map_err(|_| BackupError::KeyUnavailable)?;
        if footer_tag.key_id() != metadata.key_id()
            || self
                .key_provider
                .metadata()
                .await
                .map_err(|_| BackupError::KeyUnavailable)?
                != metadata
        {
            return Err(BackupError::KeyUnavailable);
        }
        self.key_provider
            .verify(
                VerifyAuthenticationRequest::new(
                    BACKUP_PURPOSE_MANIFEST,
                    footer_core_bytes.clone(),
                    footer_tag.clone(),
                )
                .map_err(|_| BackupError::KeyUnavailable)?,
            )
            .await
            .map_err(|_| BackupError::KeyUnavailable)?;
        let footer_bytes = canonical_bytes(&FooterEnvelope {
            core: footer_core,
            authentication_tag: tag_to_wire(&footer_tag),
        })?;
        if footer_bytes.len() > MAX_MANIFEST_BYTES {
            return Err(BackupError::LimitExceeded);
        }
        output
            .write_all(FOOTER_MARKER)
            .map_err(|_| unavailable(UnavailableStage::ArchiveWrite))?;
        write_u32(&mut output, footer_bytes.len())?;
        output
            .write_all(&footer_bytes)
            .map_err(|_| unavailable(UnavailableStage::ArchiveWrite))?;
        output
            .flush()
            .map_err(|_| unavailable(UnavailableStage::ArchiveWrite))?;
        Ok(BackupReceipt {
            chunk_count,
            plaintext_bytes: plaintext_budget.total(),
            ciphertext_bytes,
            archive_sha256: hex::encode(Sha256::digest(footer_core_bytes)),
        })
    }

    /// Encrypts into an exclusive sibling temporary and atomically publishes without replacement.
    pub async fn encrypt_to_path<R: Read>(
        &self,
        manifest: &BackupManifest,
        input: R,
        destination: &Path,
    ) -> Result<BackupReceipt, BackupError> {
        let mut owned = OwnedTemporary::for_destination(destination)?;
        let receipt = self.encrypt(manifest, input, owned.file_mut()).await?;
        owned.publish(destination)?;
        Ok(receipt)
    }

    /// Authenticates and decrypts to a discard sink, then repeats decryption into `output`.
    pub async fn verify_then_decrypt<R: Read + Seek, W: Write>(
        &self,
        mut archive: R,
        current_provider_epoch: u64,
        mut output: W,
    ) -> Result<VerifiedBackup, BackupError> {
        let first = self
            .decrypt_pass(
                &mut archive,
                current_provider_epoch,
                &mut std::io::sink(),
                None,
            )
            .await?;
        archive
            .seek(SeekFrom::Start(0))
            .map_err(|_| BackupError::InvalidBackup)?;
        let second = self
            .decrypt_pass(&mut archive, current_provider_epoch, &mut output, None)
            .await?;
        if first != second {
            return Err(BackupError::InvalidBackup);
        }
        output.flush().map_err(|_| BackupError::InvalidRestore)?;
        Ok(VerifiedBackup { footer: second })
    }

    async fn decrypt_verified_once<R: Read, W: Write>(
        &self,
        archive: &mut R,
        current_provider_epoch: u64,
        mut output: W,
        expected: &VerifiedBackup,
    ) -> Result<VerifiedBackup, BackupError> {
        let footer = self
            .decrypt_pass(
                archive,
                current_provider_epoch,
                &mut output,
                Some(&expected.footer.header_sha256),
            )
            .await?;
        let verified = VerifiedBackup { footer };
        if &verified != expected {
            return Err(BackupError::InvalidRestore);
        }
        output.flush().map_err(|_| BackupError::InvalidRestore)?;
        Ok(verified)
    }

    async fn decrypt_pass<R: Read, W: Write>(
        &self,
        input: &mut R,
        current_provider_epoch: u64,
        output: &mut W,
        expected_header_sha256: Option<&str>,
    ) -> Result<FooterCore, BackupError> {
        let mut magic = [0_u8; 8];
        input.read_exact(&mut magic).map_err(invalid_backup)?;
        if &magic != MAGIC {
            return Err(BackupError::InvalidBackup);
        }
        let header_len = read_u32(input)? as usize;
        if header_len == 0 || header_len > MAX_HEADER_BYTES {
            return Err(BackupError::InvalidBackup);
        }
        let mut header_bytes = vec![0_u8; header_len];
        input
            .read_exact(&mut header_bytes)
            .map_err(invalid_backup)?;
        let header: HeaderEnvelope =
            serde_json::from_slice(&header_bytes).map_err(|_| BackupError::InvalidBackup)?;
        validate_header(&header.core)?;
        let header_core_bytes = canonical_bytes(&header.core)?;
        self.key_provider
            .verify(
                VerifyAuthenticationRequest::new(
                    BACKUP_PURPOSE_HEADER,
                    header_core_bytes.clone(),
                    wire_to_tag(&header.authentication_tag)?,
                )
                .map_err(|_| BackupError::InvalidBackup)?,
            )
            .await
            .map_err(|_| BackupError::InvalidBackup)?;
        let current = self
            .key_provider
            .metadata()
            .await
            .map_err(|_| BackupError::KeyUnavailable)?;
        if header.core.provider.epoch != current_provider_epoch
            || header.core.provider.epoch != current.current_revocation_epoch()
            || header.core.provider.key_id != current.key_id()
            || header.core.provider.algorithm != current.algorithm()
            || header.core.provider.version != current.version()
        {
            return Err(BackupError::InvalidRestore);
        }
        let key = self
            .key_provider
            .unwrap(wire_to_wrapped(&header.core.wrapped_key)?)
            .await
            .map_err(|_| BackupError::KeyUnavailable)?;
        let mut nonce_prefix = [0_u8; 16];
        hex::decode_to_slice(&header.core.nonce_prefix, &mut nonce_prefix)
            .map_err(|_| BackupError::InvalidBackup)?;
        let header_sha256 = Sha256::digest(&header_core_bytes);
        if expected_header_sha256.is_some_and(|expected| hex::encode(header_sha256) != expected) {
            return Err(BackupError::InvalidRestore);
        }
        let mut expected_index = 0_u64;
        let mut plaintext_budget = PlaintextBudget::default();
        let mut ciphertext_bytes = 0_u64;
        let mut ciphertext_digest = Sha256::new();
        let mut prior_plain_len: Option<usize> = None;
        let footer = key.expose(|key_bytes| -> Result<FooterEnvelope, BackupError> {
            let cipher = XChaCha20Poly1305::new_from_slice(key_bytes)
                .map_err(|_| BackupError::InvalidBackup)?;
            loop {
                let mut marker = [0_u8; 4];
                input.read_exact(&mut marker).map_err(invalid_backup)?;
                if &marker == FOOTER_MARKER {
                    if prior_plain_len.is_some_and(|length| length == 0 || length > CHUNK_BYTES) {
                        return Err(BackupError::InvalidBackup);
                    }
                    let len = read_u32(input)? as usize;
                    if len == 0 || len > MAX_MANIFEST_BYTES {
                        return Err(BackupError::InvalidBackup);
                    }
                    let mut bytes = vec![0_u8; len];
                    input.read_exact(&mut bytes).map_err(invalid_backup)?;
                    let mut trailing = [0_u8; 1];
                    if input.read(&mut trailing).map_err(invalid_backup)? != 0 {
                        return Err(BackupError::InvalidBackup);
                    }
                    return serde_json::from_slice(&bytes).map_err(|_| BackupError::InvalidBackup);
                }
                if &marker != CHUNK_MARKER {
                    return Err(BackupError::InvalidBackup);
                }
                let index = read_u64(input)?;
                let plain_len = read_u32(input)? as usize;
                let cipher_len = read_u32(input)? as usize;
                if index != expected_index
                    || expected_index >= MAX_CHUNKS
                    || plain_len == 0
                    || plain_len > CHUNK_BYTES
                    || cipher_len != plain_len + 16
                    || prior_plain_len.is_some_and(|length| length != CHUNK_BYTES)
                {
                    return Err(BackupError::InvalidBackup);
                }
                prior_plain_len = Some(plain_len);
                plaintext_budget.account(plain_len as u64)?;
                let mut ciphertext = vec![0_u8; cipher_len];
                input.read_exact(&mut ciphertext).map_err(invalid_backup)?;
                ciphertext_digest.update(&ciphertext);
                ciphertext_bytes = ciphertext_bytes
                    .checked_add(cipher_len as u64)
                    .ok_or(BackupError::LimitExceeded)?;
                let nonce = chunk_nonce(&nonce_prefix, index);
                let aad = chunk_aad(&header_sha256, index, plain_len as u32);
                let nonce = XNonce::from(nonce);
                let plaintext = Zeroizing::new(
                    cipher
                        .decrypt(
                            &nonce,
                            Payload {
                                msg: &ciphertext,
                                aad: &aad,
                            },
                        )
                        .map_err(|_| BackupError::InvalidBackup)?,
                );
                output
                    .write_all(&plaintext)
                    .map_err(|_| BackupError::InvalidRestore)?;
                expected_index += 1;
            }
        })?;
        let footer_core_bytes = canonical_bytes(&footer.core)?;
        self.key_provider
            .verify(
                VerifyAuthenticationRequest::new(
                    BACKUP_PURPOSE_MANIFEST,
                    footer_core_bytes,
                    wire_to_tag(&footer.authentication_tag)?,
                )
                .map_err(|_| BackupError::InvalidBackup)?,
            )
            .await
            .map_err(|_| BackupError::InvalidBackup)?;
        if footer.core.header_sha256 != hex::encode(header_sha256)
            || footer.core.chunk_count != expected_index
            || footer.core.plaintext_bytes != plaintext_budget.total()
            || footer.core.ciphertext_bytes != ciphertext_bytes
            || footer.core.ciphertext_sha256 != hex::encode(ciphertext_digest.finalize())
            || footer.core.manifest.provider_epoch != header.core.provider.epoch
        {
            return Err(BackupError::InvalidBackup);
        }
        Ok(footer.core)
    }
}

struct OwnedTemporary {
    path: PathBuf,
    file: Option<File>,
    parent: File,
    parent_path: PathBuf,
    destination_name: std::ffi::OsString,
    _anchors: Vec<File>,
    rollback_published: Option<File>,
}

impl OwnedTemporary {
    // On Linux the O_TMPFILE block is the tail of this function, so its early `return` reads as
    // needless there while it is load-bearing on Windows, where more code follows (#869).
    #[cfg_attr(target_os = "linux", allow(clippy::needless_return))]
    fn for_destination(destination: &Path) -> Result<Self, BackupError> {
        #[cfg(all(unix, not(target_os = "linux")))]
        return Err(unavailable(UnavailableStage::TemporaryCreate));
        if !destination.is_absolute() || destination.exists() {
            return Err(BackupError::InvalidBackup);
        }
        let parent = destination.parent().ok_or(BackupError::InvalidBackup)?;
        if !parent.is_dir() {
            return Err(BackupError::InvalidBackup);
        }
        #[cfg(windows)]
        let anchors = pin_execution_ancestors(destination)?;
        #[cfg(not(windows))]
        let anchors = Vec::new();
        let parent_handle = open_directory(parent)?;
        #[cfg(target_os = "linux")]
        validate_linux_publication_parent(&parent_handle)?;
        let destination_name = destination
            .file_name()
            .ok_or(BackupError::InvalidBackup)?
            .to_os_string();
        #[cfg(target_os = "linux")]
        {
            use std::os::fd::{AsRawFd, FromRawFd};
            let unnamed = match injected_tmpfile_failure() {
                Some(os) => Err(Some(os)),
                None => {
                    let descriptor = unsafe {
                        libc::openat(
                            parent_handle.as_raw_fd(),
                            c".".as_ptr(),
                            libc::O_TMPFILE | libc::O_RDWR | libc::O_CLOEXEC,
                            0o600,
                        )
                    };
                    if descriptor < 0 {
                        Err(std::io::Error::last_os_error().raw_os_error())
                    } else {
                        Ok(descriptor)
                    }
                }
            };
            let (path, descriptor) = match unnamed {
                Ok(descriptor) => (PathBuf::new(), descriptor),
                Err(os) if !tmpfile_is_unsupported(os) => {
                    return Err(unavailable_os(UnavailableStage::TemporaryCreate, os));
                }
                // The filesystem has no O_TMPFILE (#1306): a random-named pending file in the
                // same parent, created exclusively through the retained parent descriptor, and
                // published below by descriptor exactly like the unnamed one.
                Err(_) => create_linux_named_pending(&parent_handle, parent)?,
            };
            return Ok(Self {
                path,
                file: Some(unsafe { File::from_raw_fd(descriptor) }),
                parent: parent_handle,
                parent_path: parent.to_path_buf(),
                destination_name,
                _anchors: anchors,
                rollback_published: None,
            });
        }
        #[cfg(windows)]
        for _ in 0..16 {
            let mut random = [0_u8; 16];
            getrandom::fill(&mut random).map_err(|_| unavailable(UnavailableStage::Random))?;
            let path = parent.join(format!(".graphhelm-backup-{}.pending", hex::encode(random)));
            random.zeroize();
            match create_owned_pending(&path) {
                Ok(file) => {
                    return Self::new(
                        path,
                        file,
                        parent_handle,
                        parent.to_path_buf(),
                        destination_name,
                        anchors,
                    );
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => return Err(unavailable(UnavailableStage::TemporaryCreate)),
            }
        }
        #[cfg(windows)]
        Err(unavailable(UnavailableStage::TemporaryCreate))
    }

    #[cfg(windows)]
    fn new(
        path: PathBuf,
        file: File,
        parent: File,
        parent_path: PathBuf,
        destination_name: std::ffi::OsString,
        anchors: Vec<File>,
    ) -> Result<Self, BackupError> {
        if !same_identity(&file, &File::open(&path).map_err(io_error)?)? {
            return Err(unavailable(UnavailableStage::FileIdentity));
        }
        Ok(Self {
            path,
            file: Some(file),
            parent,
            parent_path,
            destination_name,
            _anchors: anchors,
            rollback_published: None,
        })
    }

    fn file(&self) -> &File {
        self.file.as_ref().expect("owned temporary file")
    }

    fn file_mut(&mut self) -> &mut File {
        self.file.as_mut().expect("owned temporary file")
    }

    fn publish(&mut self, destination: &Path) -> Result<(), BackupError> {
        if destination.file_name() != Some(self.destination_name.as_os_str()) {
            return Err(BackupError::InvalidBackup);
        }
        self.file().sync_all().map_err(io_error)?;
        let published = link_owned_file(self, destination)?;
        self.rollback_published = Some(published);
        if let Some(source) = self.file.as_ref()
            && !same_identity(
                source,
                self.rollback_published
                    .as_ref()
                    .expect("published rollback handle"),
            )?
        {
            return Err(unavailable(UnavailableStage::FileIdentity));
        }
        let committed = self
            .remove()
            .and_then(|()| sync_owned_directory(&self.parent, &self.parent_path));
        committed?;
        self.rollback_published.take();
        Ok(())
    }

    fn remove(&mut self) -> Result<(), BackupError> {
        if self.path.as_os_str().is_empty() {
            return Ok(());
        }
        // After publication the Linux pending name and the published name share one inode, and
        // the only handle left on it is the rollback one.
        #[cfg(target_os = "linux")]
        {
            let owned = self
                .file
                .as_ref()
                .or(self.rollback_published.as_ref())
                .ok_or_else(|| unavailable(UnavailableStage::FileIdentity))?;
            unlink_linux_named_pending(&self.parent, &self.path, owned)?;
            self.path = PathBuf::new();
            Ok(())
        }
        #[cfg(not(target_os = "linux"))]
        {
            let reopened = File::open(&self.path).map_err(io_error)?;
            if !same_identity(self.file(), &reopened)? {
                return Err(unavailable(UnavailableStage::FileIdentity));
            }
            remove_owned_path(&self.path, self.file())
        }
    }
}

#[cfg(target_os = "linux")]
fn validate_linux_publication_parent(parent: &File) -> Result<(), BackupError> {
    use std::os::unix::fs::MetadataExt;
    let metadata = parent.metadata().map_err(io_error)?;
    if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o022 != 0 {
        return Err(BackupError::InvalidBackup);
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn remove_published_link(owned: &OwnedTemporary, published: File) -> Result<(), BackupError> {
    use std::{
        ffi::CString,
        os::fd::{AsRawFd, FromRawFd},
        os::unix::ffi::OsStrExt,
    };
    let destination = CString::new(owned.destination_name.as_bytes())
        .map_err(|_| unavailable(UnavailableStage::FileIo))?;
    let current_descriptor = unsafe {
        libc::openat(
            owned.parent.as_raw_fd(),
            destination.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if current_descriptor < 0 {
        return Err(unavailable(UnavailableStage::FileIo));
    }
    let current = unsafe { File::from_raw_fd(current_descriptor) };
    if !same_identity(&published, &current)? {
        return Err(unavailable(UnavailableStage::FileIdentity));
    }
    drop(current);
    run_before_linux_rollback_rename(&owned.parent_path.join(&owned.destination_name));
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random).map_err(|_| unavailable(UnavailableStage::Random))?;
    let quarantine = CString::new(format!(
        ".graphhelm-backup-rollback-{}",
        hex::encode(random)
    ))
    .map_err(|_| unavailable(UnavailableStage::FileIo))?;
    random.zeroize();
    if unsafe {
        libc::renameat2(
            owned.parent.as_raw_fd(),
            destination.as_ptr(),
            owned.parent.as_raw_fd(),
            quarantine.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    } != 0
    {
        return Err(unavailable(UnavailableStage::FileIo));
    }
    let descriptor = unsafe {
        libc::openat(
            owned.parent.as_raw_fd(),
            quarantine.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if descriptor < 0 {
        return Err(unavailable(UnavailableStage::FileIo));
    }
    let quarantined = unsafe { File::from_raw_fd(descriptor) };
    if !same_identity(&published, &quarantined)? {
        let _ = unsafe {
            libc::renameat2(
                owned.parent.as_raw_fd(),
                quarantine.as_ptr(),
                owned.parent.as_raw_fd(),
                destination.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        return Err(unavailable(UnavailableStage::FileIdentity));
    }
    if unsafe { libc::unlinkat(owned.parent.as_raw_fd(), quarantine.as_ptr(), 0) } != 0 {
        return Err(unavailable(UnavailableStage::FileIo));
    }
    Ok(())
}

#[cfg(all(test, target_os = "linux"))]
type LinuxRollbackHook = Box<dyn FnOnce(&Path)>;

#[cfg(all(test, target_os = "linux"))]
std::thread_local! {
    static BEFORE_LINUX_ROLLBACK_RENAME: std::cell::RefCell<Option<LinuxRollbackHook>> =
        std::cell::RefCell::new(None);
}

#[cfg(all(test, target_os = "linux"))]
fn set_before_linux_rollback_rename(hook: impl FnOnce(&Path) + 'static) {
    BEFORE_LINUX_ROLLBACK_RENAME.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
}

#[cfg(all(test, target_os = "linux"))]
fn run_before_linux_rollback_rename(path: &Path) {
    BEFORE_LINUX_ROLLBACK_RENAME.with(|slot| {
        if let Some(hook) = slot.borrow_mut().take() {
            hook(path);
        }
    });
}

#[cfg(all(not(test), target_os = "linux"))]
fn run_before_linux_rollback_rename(_: &Path) {}

#[cfg(windows)]
fn remove_published_link(owned: &OwnedTemporary, published: File) -> Result<(), BackupError> {
    let _ = owned;
    remove_owned_path(Path::new(""), &published)?;
    drop(published);
    Ok(())
}

#[cfg(all(unix, not(target_os = "linux")))]
fn remove_published_link(_: &OwnedTemporary, _: File) -> Result<(), BackupError> {
    Err(unavailable(UnavailableStage::FileIo))
}

impl Drop for OwnedTemporary {
    fn drop(&mut self) {
        #[cfg(target_os = "linux")]
        if !self.path.as_os_str().is_empty()
            && let Some(file) = self.file.as_ref().or(self.rollback_published.as_ref())
        {
            let _ = unlink_linux_named_pending(&self.parent, &self.path, file);
        }
        #[cfg(not(target_os = "linux"))]
        if !self.path.as_os_str().is_empty()
            && let Some(file) = self.file.as_ref()
            && let Ok(reopened) = File::open(&self.path)
            && same_identity(file, &reopened).unwrap_or(false)
        {
            let _ = remove_owned_path(&self.path, file);
        }
        #[cfg(windows)]
        drop(self.file.take());
        if let Some(published) = self.rollback_published.take() {
            let _ = remove_published_link(self, published);
            let _ = sync_owned_directory(&self.parent, &self.parent_path);
        }
    }
}

#[cfg(unix)]
fn open_directory(path: &Path) -> Result<File, BackupError> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(io_error)
}

#[cfg(windows)]
fn open_directory(path: &Path) -> Result<File, BackupError> {
    use std::os::windows::{ffi::OsStrExt, io::FromRawHandle};
    use windows_sys::Win32::{
        Foundation::{GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE},
        Storage::FileSystem::{
            CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
            FILE_SHARE_WRITE, OPEN_EXISTING,
        },
    };
    let mut wide = path.as_os_str().encode_wide().collect::<Vec<_>>();
    wide.push(0);
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(unavailable(UnavailableStage::DirectoryOpen));
    }
    let file = unsafe { File::from_raw_handle(handle) };
    validate_windows_handle(&file, true)?;
    Ok(file)
}

#[cfg(windows)]
fn create_owned_pending(path: &Path) -> std::io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::{
        Foundation::{GENERIC_READ, GENERIC_WRITE},
        Storage::FileSystem::{DELETE, FILE_SHARE_READ},
    };
    OpenOptions::new()
        .read(true)
        .write(true)
        .access_mode(GENERIC_READ | GENERIC_WRITE | DELETE)
        .share_mode(FILE_SHARE_READ)
        .create_new(true)
        .open(path)
}

#[cfg(target_os = "linux")]
fn link_owned_file(owned: &mut OwnedTemporary, _destination: &Path) -> Result<File, BackupError> {
    use std::{
        ffi::CString,
        os::unix::{ffi::OsStrExt, io::AsRawFd},
    };
    let destination =
        CString::new(owned.destination_name.as_bytes()).map_err(|_| BackupError::InvalidBackup)?;
    let published = owned.file.take().expect("owned temporary file");
    let published_fd = published.as_raw_fd();
    let direct = unsafe {
        libc::linkat(
            published_fd,
            c"".as_ptr(),
            owned.parent.as_raw_fd(),
            destination.as_ptr(),
            libc::AT_EMPTY_PATH,
        )
    };
    let linked = if direct == 0
        || !empty_path_link_needs_proc_fallback(std::io::Error::last_os_error().raw_os_error())
    {
        direct
    } else {
        let source = CString::new(format!("/proc/self/fd/{published_fd}"))
            .map_err(|_| unavailable(UnavailableStage::FileIo))?;
        unsafe {
            libc::linkat(
                libc::AT_FDCWD,
                source.as_ptr(),
                owned.parent.as_raw_fd(),
                destination.as_ptr(),
                libc::AT_SYMLINK_FOLLOW,
            )
        }
    };
    if let Err(error) = map_link_result(linked) {
        // A named pending file stays owned, so Drop can still unlink it by identity.
        owned.file = Some(published);
        return Err(error);
    }
    Ok(published)
}

/// Whether a failed `linkat(fd, "", dir, name, AT_EMPTY_PATH)` is retried through
/// `linkat(AT_FDCWD, "/proc/self/fd/N", dir, name, AT_SYMLINK_FOLLOW)` (#1306).
///
/// Both forms link the inode the descriptor pins, never a name, and both refuse an existing
/// destination with `EEXIST`, so the retry keeps publication no-replace. `AT_EMPTY_PATH` needs
/// `CAP_DAC_READ_SEARCH`: without it the kernel answers `EPERM` on 6.10 and later but `ENOENT`
/// before 6.10 (Ubuntu 22.04/24.04 GA, Debian 12, WSL 6.6), so both retry. Any other errno,
/// `EEXIST` above all, is the real answer. With `/proc` unmounted the retry fails closed.
#[cfg(target_os = "linux")]
fn empty_path_link_needs_proc_fallback(os: Option<i32>) -> bool {
    matches!(os, Some(libc::EPERM | libc::ENOENT))
}

/// Whether `openat(parent, ".", O_TMPFILE)` failed because the filesystem cannot make unnamed
/// files (#1306): `EOPNOTSUPP` (overlayfs on older kernels, 9p, some NFS), `EISDIR` (a kernel
/// without O_TMPFILE reads the flag as a directory open) or `EINVAL`. Every other errno, such as
/// `EACCES` or `ENOSPC`, would fail the named fallback too, so it is reported instead.
#[cfg(target_os = "linux")]
fn tmpfile_is_unsupported(os: Option<i32>) -> bool {
    matches!(os, Some(libc::EOPNOTSUPP | libc::EISDIR | libc::EINVAL))
}

/// Creates `.graphhelm-backup-<hex>.pending` through the retained parent descriptor with
/// `O_CREAT | O_EXCL | O_NOFOLLOW` at 0600, so it can neither reuse nor follow an existing entry.
#[cfg(target_os = "linux")]
fn create_linux_named_pending(
    parent_handle: &File,
    parent: &Path,
) -> Result<(PathBuf, std::os::fd::RawFd), BackupError> {
    use std::{ffi::CString, os::fd::AsRawFd};
    for _ in 0..16 {
        let mut random = [0_u8; 16];
        getrandom::fill(&mut random).map_err(|_| unavailable(UnavailableStage::Random))?;
        let name = format!(".graphhelm-backup-{}.pending", hex::encode(random));
        random.zeroize();
        let child = CString::new(name.as_bytes())
            .map_err(|_| unavailable(UnavailableStage::TemporaryCreate))?;
        let descriptor = unsafe {
            libc::openat(
                parent_handle.as_raw_fd(),
                child.as_ptr(),
                libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_RDWR | libc::O_CLOEXEC,
                0o600,
            )
        };
        if descriptor >= 0 {
            return Ok((parent.join(name), descriptor));
        }
        let os = std::io::Error::last_os_error().raw_os_error();
        if os != Some(libc::EEXIST) {
            return Err(unavailable_os(UnavailableStage::TemporaryCreate, os));
        }
    }
    Err(unavailable(UnavailableStage::TemporaryCreate))
}

/// Unlinks the named pending file through the retained parent descriptor, only while the entry
/// is still the inode `owned` holds.
#[cfg(target_os = "linux")]
fn unlink_linux_named_pending(parent: &File, path: &Path, owned: &File) -> Result<(), BackupError> {
    use std::{
        ffi::CString,
        os::fd::{AsRawFd, FromRawFd},
        os::unix::ffi::OsStrExt,
    };
    let name = CString::new(
        path.file_name()
            .ok_or_else(|| unavailable(UnavailableStage::FileIo))?
            .as_bytes(),
    )
    .map_err(|_| unavailable(UnavailableStage::FileIo))?;
    let descriptor = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
        )
    };
    if descriptor < 0 {
        return Err(unavailable_os(
            UnavailableStage::FileIo,
            std::io::Error::last_os_error().raw_os_error(),
        ));
    }
    let current = unsafe { File::from_raw_fd(descriptor) };
    if !same_identity(owned, &current)? {
        return Err(unavailable(UnavailableStage::FileIdentity));
    }
    drop(current);
    if unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), 0) } != 0 {
        return Err(unavailable_os(
            UnavailableStage::FileIo,
            std::io::Error::last_os_error().raw_os_error(),
        ));
    }
    Ok(())
}

#[cfg(all(test, target_os = "linux"))]
std::thread_local! {
    static FAIL_NEXT_TMPFILE: std::cell::Cell<Option<i32>> = const { std::cell::Cell::new(None) };
}

#[cfg(all(test, target_os = "linux"))]
fn fail_next_tmpfile(os: i32) {
    FAIL_NEXT_TMPFILE.set(Some(os));
}

#[cfg(all(test, target_os = "linux"))]
fn injected_tmpfile_failure() -> Option<i32> {
    FAIL_NEXT_TMPFILE.take()
}

#[cfg(all(not(test), target_os = "linux"))]
const fn injected_tmpfile_failure() -> Option<i32> {
    None
}

#[cfg(all(unix, not(target_os = "linux")))]
fn link_owned_file(owned: &mut OwnedTemporary, destination: &Path) -> Result<File, BackupError> {
    let published = owned.file().try_clone().map_err(io_error)?;
    let named = File::open(&owned.path).map_err(io_error)?;
    if !same_identity(owned.file(), &named)? {
        return Err(unavailable(UnavailableStage::FileIdentity));
    }
    std::fs::hard_link(&owned.path, destination).map_err(map_link_error)?;
    Ok(published)
}

#[cfg(windows)]
fn link_owned_file(owned: &mut OwnedTemporary, destination: &Path) -> Result<File, BackupError> {
    use std::os::windows::{ffi::OsStrExt, io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_RENAME_INFO, FileRenameInfo, SetFileInformationByHandle,
    };
    let published = owned.file.take().expect("owned temporary file");
    let name = destination.as_os_str().encode_wide().collect::<Vec<_>>();
    let name_bytes = name
        .len()
        .checked_mul(std::mem::size_of::<u16>())
        .ok_or_else(|| unavailable(UnavailableStage::FileIo))?;
    let buffer_len = std::mem::size_of::<FILE_RENAME_INFO>()
        .checked_add(name_bytes.saturating_sub(std::mem::size_of::<u16>()))
        .ok_or_else(|| unavailable(UnavailableStage::FileIo))?;
    let mut buffer = vec![0_u8; buffer_len];
    let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    unsafe {
        (*info).Anonymous.ReplaceIfExists = false;
        (*info).RootDirectory = std::ptr::null_mut();
        (*info).FileNameLength =
            u32::try_from(name_bytes).map_err(|_| unavailable(UnavailableStage::FileIo))?;
        std::ptr::copy_nonoverlapping(name.as_ptr(), (*info).FileName.as_mut_ptr(), name.len());
    }
    let result = unsafe {
        SetFileInformationByHandle(
            published.as_raw_handle() as _,
            FileRenameInfo,
            buffer.as_ptr().cast(),
            u32::try_from(buffer.len()).map_err(|_| unavailable(UnavailableStage::FileIo))?,
        )
    };
    if result == 0 {
        let error = map_link_error(std::io::Error::last_os_error());
        owned.file = Some(published);
        return Err(error);
    }
    owned.path = PathBuf::new();
    Ok(published)
}

#[cfg(target_os = "linux")]
fn map_link_result(result: i32) -> Result<(), BackupError> {
    if result == 0 {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    if error.kind() == std::io::ErrorKind::AlreadyExists {
        return Err(BackupError::InvalidBackup);
    }
    Err(unavailable_os(
        UnavailableStage::FileIo,
        error.raw_os_error(),
    ))
}

#[cfg(not(target_os = "linux"))]
fn map_link_error(error: std::io::Error) -> BackupError {
    if error.kind() == std::io::ErrorKind::AlreadyExists {
        BackupError::InvalidBackup
    } else {
        unavailable(UnavailableStage::FileIo)
    }
}

#[cfg(unix)]
fn same_identity(left: &File, right: &File) -> Result<bool, BackupError> {
    use std::os::unix::fs::MetadataExt;
    let left = left.metadata().map_err(io_error)?;
    let right = right.metadata().map_err(io_error)?;
    Ok(left.dev() == right.dev() && left.ino() == right.ino())
}

#[cfg(all(unix, not(target_os = "linux")))]
fn remove_owned_path(path: &Path, owned: &File) -> Result<(), BackupError> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt, os::unix::fs::MetadataExt};
    let metadata = std::fs::symlink_metadata(path).map_err(io_error)?;
    let owned_metadata = owned.metadata().map_err(io_error)?;
    if metadata.dev() != owned_metadata.dev() || metadata.ino() != owned_metadata.ino() {
        return Err(unavailable(UnavailableStage::FileIdentity));
    }
    let path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| unavailable(UnavailableStage::FileIo))?;
    if unsafe { libc::unlink(path.as_ptr()) } != 0 {
        return Err(unavailable(UnavailableStage::FileIo));
    }
    Ok(())
}

#[cfg(windows)]
fn remove_owned_path(_path: &Path, owned: &File) -> Result<(), BackupError> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_DISPOSITION_FLAG_DELETE, FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE,
        FILE_DISPOSITION_FLAG_POSIX_SEMANTICS, FILE_DISPOSITION_INFO_EX, FileDispositionInfoEx,
        SetFileInformationByHandle,
    };
    let info = FILE_DISPOSITION_INFO_EX {
        Flags: FILE_DISPOSITION_FLAG_DELETE
            | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS
            | FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE,
    };
    let result = unsafe {
        SetFileInformationByHandle(
            owned.as_raw_handle() as _,
            FileDispositionInfoEx,
            std::ptr::addr_of!(info).cast(),
            std::mem::size_of::<FILE_DISPOSITION_INFO_EX>() as u32,
        )
    };
    if result == 0 {
        return Err(unavailable(UnavailableStage::FileIo));
    }
    Ok(())
}

#[cfg(unix)]
fn sync_owned_directory(directory: &File, _path: &Path) -> Result<(), BackupError> {
    if directory_sync_is_injected_failure() {
        return Err(unavailable(UnavailableStage::FileIo));
    }
    directory.sync_all().map_err(io_error)
}

#[cfg(windows)]
fn sync_owned_directory(directory: &File, _path: &Path) -> Result<(), BackupError> {
    use std::os::windows::io::AsRawHandle;
    if directory_sync_is_injected_failure() {
        return Err(unavailable(UnavailableStage::FileIo));
    }
    if unsafe {
        windows_sys::Win32::Storage::FileSystem::FlushFileBuffers(directory.as_raw_handle() as _)
    } == 0
    {
        return Err(unavailable(UnavailableStage::FileIo));
    }
    Ok(())
}

#[cfg(test)]
std::thread_local! {
    static FAIL_NEXT_DIRECTORY_SYNC: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
fn fail_next_directory_sync() {
    FAIL_NEXT_DIRECTORY_SYNC.set(true);
}

#[cfg(test)]
fn directory_sync_is_injected_failure() -> bool {
    FAIL_NEXT_DIRECTORY_SYNC.replace(false)
}

#[cfg(not(test))]
const fn directory_sync_is_injected_failure() -> bool {
    false
}

#[cfg(windows)]
fn same_identity(left: &File, right: &File) -> Result<bool, BackupError> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };
    fn information(file: &File) -> Result<BY_HANDLE_FILE_INFORMATION, BackupError> {
        let mut value = unsafe { std::mem::zeroed::<BY_HANDLE_FILE_INFORMATION>() };
        let result = unsafe {
            GetFileInformationByHandle(file.as_raw_handle() as _, std::ptr::addr_of_mut!(value))
        };
        if result == 0 {
            return Err(unavailable(UnavailableStage::FileIdentity));
        }
        Ok(value)
    }
    let left = information(left)?;
    let right = information(right)?;
    Ok(left.dwVolumeSerialNumber == right.dwVolumeSerialNumber
        && left.nFileIndexHigh == right.nFileIndexHigh
        && left.nFileIndexLow == right.nFileIndexLow)
}

fn canonical_bytes(value: &impl Serialize) -> Result<Vec<u8>, BackupError> {
    fn sort(value: serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::Array(values) => {
                serde_json::Value::Array(values.into_iter().map(sort).collect())
            }
            serde_json::Value::Object(values) => {
                let mut entries = values.into_iter().collect::<Vec<_>>();
                entries.sort_by(|left, right| left.0.cmp(&right.0));
                serde_json::Value::Object(
                    entries
                        .into_iter()
                        .map(|(key, value)| (key, sort(value)))
                        .collect(),
                )
            }
            scalar => scalar,
        }
    }
    let value = serde_json::to_value(value).map_err(|_| BackupError::InvalidBackup)?;
    serde_json::to_vec(&sort(value)).map_err(|_| BackupError::InvalidBackup)
}

fn restore_receipt_bytes(
    source_identity_sha256: &str,
    target_identity_sha256: &str,
    manifest_sha256: &str,
) -> Result<Vec<u8>, BackupError> {
    if !valid_sha256(source_identity_sha256)
        || !valid_sha256(target_identity_sha256)
        || !valid_sha256(manifest_sha256)
    {
        return Err(BackupError::InvalidRestore);
    }
    canonical_bytes(&(
        "graphhelm-restore-receipt-v1",
        source_identity_sha256,
        target_identity_sha256,
        manifest_sha256,
    ))
}

fn wrapped_to_wire(value: &WrappedKey) -> WrappedWire {
    WrappedWire {
        key_id: value.key_id().to_owned(),
        handle: value.handle().to_owned(),
        nonce: hex::encode(value.nonce()),
        ciphertext: hex::encode(value.ciphertext()),
        aad_sha256: value.aad_sha256().as_str().to_owned(),
    }
}

fn wire_to_wrapped(value: &WrappedWire) -> Result<WrappedKey, BackupError> {
    WrappedKey::new(
        value.key_id.clone(),
        value.handle.clone(),
        "xchacha20poly1305",
        hex::decode(&value.nonce).map_err(|_| BackupError::InvalidBackup)?,
        hex::decode(&value.ciphertext).map_err(|_| BackupError::InvalidBackup)?,
        RawSha256::parse(value.aad_sha256.clone()).map_err(|_| BackupError::InvalidBackup)?,
    )
    .map_err(|_| BackupError::InvalidBackup)
}

fn tag_to_wire(value: &AuthenticationTag) -> TagWire {
    TagWire {
        key_id: value.key_id().to_owned(),
        algorithm: value.algorithm().to_owned(),
        bytes: hex::encode(value.bytes()),
    }
}

fn wire_to_tag(value: &TagWire) -> Result<AuthenticationTag, BackupError> {
    AuthenticationTag::new(
        value.key_id.clone(),
        &value.algorithm,
        hex::decode(&value.bytes).map_err(|_| BackupError::InvalidBackup)?,
    )
    .map_err(|_| BackupError::InvalidBackup)
}

fn validate_header(value: &HeaderCore) -> Result<(), BackupError> {
    if value.format_version != 1
        || value.algorithm != "xchacha20poly1305"
        || value.chunk_bytes as usize != CHUNK_BYTES
        || value.nonce_prefix.len() != 32
        || value.provider.epoch > 9_007_199_254_740_991
    {
        return Err(BackupError::InvalidBackup);
    }
    Ok(())
}

fn chunk_nonce(prefix: &[u8; 16], index: u64) -> [u8; 24] {
    let mut nonce = [0_u8; 24];
    nonce[..16].copy_from_slice(prefix);
    nonce[16..].copy_from_slice(&index.to_be_bytes());
    nonce
}

fn chunk_aad(header_sha256: &[u8], index: u64, plaintext_len: u32) -> Vec<u8> {
    let mut aad = Vec::with_capacity(32 + 8 + 4);
    aad.extend_from_slice(header_sha256);
    aad.extend_from_slice(&index.to_be_bytes());
    aad.extend_from_slice(&plaintext_len.to_be_bytes());
    aad
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn read_chunk(reader: &mut impl Read, buffer: &mut [u8]) -> std::io::Result<usize> {
    let mut used = 0;
    while used < buffer.len() {
        let read = reader.read(&mut buffer[used..])?;
        if read == 0 {
            break;
        }
        used += read;
    }
    Ok(used)
}

fn write_u32(writer: &mut impl Write, value: usize) -> Result<(), BackupError> {
    let value = u32::try_from(value).map_err(|_| BackupError::LimitExceeded)?;
    writer.write_all(&value.to_be_bytes()).map_err(io_error)
}

fn write_u64(writer: &mut impl Write, value: u64) -> Result<(), BackupError> {
    writer.write_all(&value.to_be_bytes()).map_err(io_error)
}

fn read_u32(reader: &mut impl Read) -> Result<u32, BackupError> {
    let mut bytes = [0_u8; 4];
    reader.read_exact(&mut bytes).map_err(invalid_backup)?;
    Ok(u32::from_be_bytes(bytes))
}

fn read_u64(reader: &mut impl Read) -> Result<u64, BackupError> {
    let mut bytes = [0_u8; 8];
    reader.read_exact(&mut bytes).map_err(invalid_backup)?;
    Ok(u64::from_be_bytes(bytes))
}

fn io_error(_: std::io::Error) -> BackupError {
    unavailable(UnavailableStage::FileIo)
}

fn invalid_backup(_: std::io::Error) -> BackupError {
    BackupError::InvalidBackup
}

#[cfg(test)]
mod termination_outcome_policy {
    use super::sweep_left_descendants;
    use graphhelm_process_tree::TerminationOutcome;

    /// #805 gave `terminate` a `#[must_use]` outcome and this crate dropped it, which said "the
    /// tree is gone" on a sweep that had not finished. Propagating it forced a POLICY: which
    /// outcomes mean descendants may still be running.
    ///
    /// The policy is not obvious and it is not symmetric, which is why it is pinned rather than
    /// left to the one `matches!` that implements it:
    ///
    /// * `BoundReached` IS a failure -- the sweep ran out of passes while descendants were still
    ///   appearing, so the leader's exit status does not mean the tree is gone.
    /// * `SweepUnavailable` is NOT -- it is the process-tree crate declaring a platform limit
    ///   (no `/proc`, no `PR_SET_CHILD_SUBREAPER`) rather than claiming a property it cannot
    ///   deliver. Treating a documented limit as a backup failure would fail every such host,
    ///   turning an honest disclosure into an outage.
    ///
    /// Both negative cases are asserted, not just the interesting one: a predicate that answered
    /// `true` for everything would satisfy the `BoundReached` case alone and look correct.
    #[test]
    fn only_a_bounded_sweep_means_descendants_may_remain() {
        assert!(
            sweep_left_descendants(&TerminationOutcome::BoundReached {
                passes: 8,
                remaining: 3,
            }),
            "a sweep that hit its bound with descendants still appearing was read as a clean tree"
        );
        assert!(
            !sweep_left_descendants(&TerminationOutcome::Complete),
            "CONTROL: a complete sweep was read as leaving descendants, so the assertion above \
             would pass for a predicate that is simply always true"
        );
        assert!(
            !sweep_left_descendants(&TerminationOutcome::SweepUnavailable),
            "a platform that cannot sweep was read as a failed backup: the crate is declaring a \
             limit it cannot exceed, and every host without /proc would fail its backups"
        );
        assert!(
            sweep_left_descendants(&TerminationOutcome::NotAttempted),
            "a call that signalled NOTHING was read as a stopped tree: `NotAttempted` means the \
             leader and every descendant are untouched, which is strictly worse than a bounded \
             sweep -- and it is the variant it was split out of that must stay a non-failure"
        );
    }
}
