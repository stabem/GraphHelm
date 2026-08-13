//! Scoped PostgreSQL Event/Evidence repository.

mod artifact;
pub mod backup;
mod error;
mod evidence;
mod integrity;
mod journal;
mod projection;
mod retention;
mod rows;
mod scope;

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering},
};

use graphhelm_events::{
    ArtifactCatalog, AsyncEventRepository, AuthenticatedCheckpoint, EventPage,
    EventRepositoryError, EvidenceRead, EvidenceRepository, IntegrityReport, KeyProvider,
    PreparedAppend, ProjectionGeneration, ProjectionRebuildRequest, ProjectionRepository,
    ReadStreamRequest, RepositoryFuture, RetentionRepository, StreamHead, VerifyRangeRequest,
};
use graphhelm_protocols::{
    ArtifactId, ArtifactReference, EventEnvelope, EvidenceId, RepositoryScope,
};
use sqlx::{
    PgPool, SqlSafeStr,
    migrate::{Migration, MigrationType, Migrator},
    postgres::PgPoolOptions,
};

const INITIAL_MIGRATION: &str = include_str!("../migrations/0001_event_evidence.sql");
const RETENTION_MIGRATION: &str = include_str!("../migrations/0002_retention.sql");
const PROJECTION_MIGRATION: &str = include_str!("../migrations/0003_projections.sql");
const SCOPE_GUARD_MIGRATION: &str = include_str!("../migrations/0004_scope_guard.sql");

#[cfg(feature = "test-support")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum PostgresFailpoint {
    #[default]
    None = 0,
    AfterEvidenceBeforeEvent = 1,
}

pub struct PostgresEventStore {
    pool: PgPool,
    key_provider: Arc<dyn KeyProvider>,
    verification_role: Option<String>,
    failpoint: AtomicU8,
    pause_after_head: AtomicBool,
    pause_reached: tokio::sync::Notify,
    pause_release: tokio::sync::Notify,
    head_reads: AtomicUsize,
    head_read: tokio::sync::Notify,
}

impl PostgresEventStore {
    pub(crate) fn for_admin_verification(
        pool: PgPool,
        key_provider: Arc<dyn KeyProvider>,
        verification_role: String,
    ) -> Self {
        Self {
            pool,
            key_provider,
            verification_role: Some(verification_role),
            failpoint: AtomicU8::new(0),
            pause_after_head: AtomicBool::new(false),
            pause_reached: tokio::sync::Notify::new(),
            pause_release: tokio::sync::Notify::new(),
            head_reads: AtomicUsize::new(0),
            head_read: tokio::sync::Notify::new(),
        }
    }

    pub async fn connect(
        database_url: &str,
        max_connections: u32,
        key_provider: Arc<dyn KeyProvider>,
    ) -> Result<Self, EventRepositoryError> {
        if database_url.is_empty() || max_connections == 0 || max_connections > 64 {
            return Err(EventRepositoryError::Invalid);
        }
        let pool = PgPoolOptions::new()
            .max_connections(max_connections)
            .connect(database_url)
            .await
            .map_err(error::storage)?;
        Self::from_pool(pool, key_provider).await
    }

    pub async fn from_pool(
        pool: PgPool,
        key_provider: Arc<dyn KeyProvider>,
    ) -> Result<Self, EventRepositoryError> {
        verify_migration(&pool).await?;
        verify_runtime_role(&pool).await?;
        Ok(Self {
            pool,
            key_provider,
            verification_role: None,
            failpoint: AtomicU8::new(0),
            pause_after_head: AtomicBool::new(false),
            pause_reached: tokio::sync::Notify::new(),
            pause_release: tokio::sync::Notify::new(),
            head_reads: AtomicUsize::new(0),
            head_read: tokio::sync::Notify::new(),
        })
    }

    #[must_use]
    pub(crate) fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub(crate) async fn set_scope(
        &self,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        scope_value: &graphhelm_protocols::RepositoryScope,
    ) -> Result<(), EventRepositoryError> {
        if let Some(role) = &self.verification_role {
            sqlx::query("SELECT set_config('role',$1,true)")
                .bind(role)
                .execute(&mut **transaction)
                .await
                .map_err(error::storage)?;
        }
        scope::set_local(transaction, scope_value).await
    }

    #[must_use]
    pub fn key_provider(&self) -> &dyn KeyProvider {
        self.key_provider.as_ref()
    }

    #[cfg(feature = "test-support")]
    pub fn set_failpoint_for_testing(&self, failpoint: PostgresFailpoint) {
        self.failpoint.store(failpoint as u8, Ordering::SeqCst);
    }

    pub(crate) fn failpoint_after_evidence(&self) -> bool {
        self.failpoint.load(Ordering::SeqCst) == 1
    }

    #[cfg(feature = "test-support")]
    #[doc(hidden)]
    pub fn pause_first_append_after_head_read_for_testing(&self) {
        self.head_reads.store(0, Ordering::SeqCst);
        self.pause_after_head.store(true, Ordering::SeqCst);
    }

    #[cfg(feature = "test-support")]
    #[doc(hidden)]
    pub async fn wait_for_paused_head_read_for_testing(&self) {
        while self.pause_after_head.load(Ordering::SeqCst) {
            self.pause_reached.notified().await;
        }
    }

    #[cfg(feature = "test-support")]
    #[doc(hidden)]
    pub async fn wait_for_head_reads_for_testing(&self, target: usize) {
        while self.head_reads.load(Ordering::SeqCst) < target {
            self.head_read.notified().await;
        }
    }

    #[cfg(feature = "test-support")]
    #[doc(hidden)]
    pub fn release_paused_head_read_for_testing(&self) {
        self.pause_release.notify_one();
    }

    pub(crate) async fn after_head_read(&self) {
        self.head_reads.fetch_add(1, Ordering::SeqCst);
        self.head_read.notify_one();
        if self.pause_after_head.swap(false, Ordering::SeqCst) {
            self.pause_reached.notify_one();
            self.pause_release.notified().await;
        }
    }
}

pub async fn migrate(admin_pool: &PgPool) -> Result<(), EventRepositoryError> {
    let migrator = Migrator::with_migrations(vec![
        initial_migration(),
        retention_migration(),
        projection_migration(),
        scope_guard_migration(),
    ]);
    let mut connection = admin_pool.acquire().await.map_err(error::storage)?;
    sqlx::query("DISCARD TEMP")
        .execute(&mut *connection)
        .await
        .map_err(error::storage)?;
    sqlx::query("SET search_path = public, pg_catalog")
        .execute(&mut *connection)
        .await
        .map_err(error::storage)?;
    migrator
        .run(&mut *connection)
        .await
        .map_err(|_| EventRepositoryError::Storage)
}

fn initial_migration() -> Migration {
    Migration::new(
        1,
        "event evidence store".into(),
        MigrationType::Simple,
        INITIAL_MIGRATION.into_sql_str(),
        false,
    )
}

fn retention_migration() -> Migration {
    Migration::new(
        2,
        "retention and cryptographic erasure".into(),
        MigrationType::Simple,
        RETENTION_MIGRATION.into_sql_str(),
        false,
    )
}

fn projection_migration() -> Migration {
    Migration::new(
        3,
        "disposable projection generations".into(),
        MigrationType::Simple,
        PROJECTION_MIGRATION.into_sql_str(),
        false,
    )
}

fn scope_guard_migration() -> Migration {
    Migration::new(
        4,
        "scope guard and function privilege revocation".into(),
        MigrationType::Simple,
        SCOPE_GUARD_MIGRATION.into_sql_str(),
        false,
    )
}

async fn verify_migration(pool: &PgPool) -> Result<(), EventRepositoryError> {
    let first = initial_migration();
    let second = retention_migration();
    let third = projection_migration();
    let fourth = scope_guard_migration();
    let current: bool =
        sqlx::query_scalar("SELECT public.graphhelm_migrations_are_current($1,$2,$3,$4)")
            .bind(first.checksum.as_ref())
            .bind(second.checksum.as_ref())
            .bind(third.checksum.as_ref())
            .bind(fourth.checksum.as_ref())
            .fetch_one(pool)
            .await
            .map_err(error::storage)?;
    if !current {
        return Err(EventRepositoryError::Integrity);
    }
    Ok(())
}

pub async fn verify_runtime_role(pool: &PgPool) -> Result<(), EventRepositoryError> {
    let dangerous_role: bool = sqlx::query_scalar(
        "SELECT rolsuper OR rolbypassrls OR rolcreaterole OR rolcreatedb OR rolreplication \
         FROM pg_roles WHERE rolname=current_user",
    )
    .fetch_one(pool)
    .await
    .map_err(error::storage)?;
    let owns_scoped_table: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace \
         WHERE n.nspname='public' AND c.relname LIKE 'graphhelm_%' \
         AND pg_get_userbyid(c.relowner)=current_user)",
    )
    .fetch_one(pool)
    .await
    .map_err(error::storage)?;
    let schema_create: bool =
        sqlx::query_scalar("SELECT has_schema_privilege(current_user, 'public', 'CREATE')")
            .fetch_one(pool)
            .await
            .map_err(error::storage)?;
    let dangerous_database_privilege: bool = sqlx::query_scalar(
        "SELECT has_database_privilege(current_user,current_database(),'CREATE') OR \
         has_database_privilege(current_user,current_database(),'TEMPORARY') OR \
         (SELECT pg_get_userbyid(datdba)=current_user FROM pg_database \
          WHERE datname=current_database())",
    )
    .fetch_one(pool)
    .await
    .map_err(error::storage)?;
    let dangerous_table_privilege: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace \
         WHERE n.nspname='public' AND c.relname LIKE 'graphhelm_%' AND \
         (has_table_privilege(current_user,c.oid,'TRUNCATE') OR \
          has_table_privilege(current_user,c.oid,'REFERENCES') OR \
          has_table_privilege(current_user,c.oid,'TRIGGER')))",
    )
    .fetch_one(pool)
    .await
    .map_err(error::storage)?;
    let mutable_history_privilege: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace \
         WHERE n.nspname='public' AND c.relname IN \
         ('graphhelm_idempotency','graphhelm_events','graphhelm_artifacts', \
          'graphhelm_evidence_refs','graphhelm_artifact_refs','graphhelm_checkpoints', \
          'graphhelm_projection_checkpoints') \
         AND (has_table_privilege(current_user,c.oid,'UPDATE') OR \
              has_table_privilege(current_user,c.oid,'DELETE'))) OR \
         has_table_privilege(current_user,'public.graphhelm_streams','DELETE') OR \
         has_table_privilege(current_user,'public.graphhelm_projection_active','DELETE')",
    )
    .fetch_one(pool)
    .await
    .map_err(error::storage)?;
    let has_role_membership: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM pg_auth_members m \
         JOIN pg_roles member ON member.oid=m.member WHERE member.rolname=current_user)",
    )
    .fetch_one(pool)
    .await
    .map_err(error::storage)?;
    if dangerous_role
        || owns_scoped_table
        || schema_create
        || dangerous_database_privilege
        || dangerous_table_privilege
        || mutable_history_privilege
        || has_role_membership
    {
        return Err(EventRepositoryError::Invalid);
    }
    Ok(())
}

impl AsyncEventRepository for PostgresEventStore {
    fn append_atomic<'a>(
        &'a self,
        request: PreparedAppend,
    ) -> RepositoryFuture<'a, Result<Vec<EventEnvelope>, EventRepositoryError>> {
        Box::pin(journal::append(self, request))
    }
    fn read_stream<'a>(
        &'a self,
        request: ReadStreamRequest,
    ) -> RepositoryFuture<'a, Result<EventPage, EventRepositoryError>> {
        Box::pin(integrity::read(self, request))
    }
    fn stream_head<'a>(
        &'a self,
        scope: RepositoryScope,
        stream_id: String,
    ) -> RepositoryFuture<'a, Result<Option<StreamHead>, EventRepositoryError>> {
        Box::pin(integrity::stream_head(self, scope, stream_id))
    }
    fn verify_range<'a>(
        &'a self,
        request: VerifyRangeRequest,
    ) -> RepositoryFuture<'a, Result<IntegrityReport, EventRepositoryError>> {
        Box::pin(integrity::verify_range(self, request))
    }
    fn append_checkpoint<'a>(
        &'a self,
        checkpoint: AuthenticatedCheckpoint,
    ) -> RepositoryFuture<'a, Result<AuthenticatedCheckpoint, EventRepositoryError>> {
        Box::pin(integrity::append_checkpoint(self, checkpoint))
    }
    fn latest_checkpoint<'a>(
        &'a self,
        scope: RepositoryScope,
        stream_id: String,
    ) -> RepositoryFuture<'a, Result<Option<AuthenticatedCheckpoint>, EventRepositoryError>> {
        Box::pin(integrity::latest_checkpoint(self, scope, stream_id))
    }
}

impl EvidenceRepository for PostgresEventStore {
    fn get_sealed<'a>(
        &'a self,
        scope: RepositoryScope,
        evidence_id: EvidenceId,
    ) -> RepositoryFuture<'a, Result<EvidenceRead, EventRepositoryError>> {
        Box::pin(evidence::get(self, scope, evidence_id))
    }
}

impl ProjectionRepository for PostgresEventStore {
    fn load_generation<'a>(
        &'a self,
        request: &'a ProjectionRebuildRequest,
    ) -> RepositoryFuture<'a, Result<Option<ProjectionGeneration>, EventRepositoryError>> {
        Box::pin(projection::load_generation(self, request))
    }

    fn save_generation<'a>(
        &'a self,
        generation: ProjectionGeneration,
    ) -> RepositoryFuture<'a, Result<(), EventRepositoryError>> {
        Box::pin(projection::save_generation(self, generation))
    }

    fn load_active<'a>(
        &'a self,
        scope: RepositoryScope,
        stream_id: String,
        projection_name: String,
        projection_version: u32,
    ) -> RepositoryFuture<'a, Result<Option<ProjectionGeneration>, EventRepositoryError>> {
        Box::pin(projection::load_active(
            self,
            scope,
            stream_id,
            projection_name,
            projection_version,
        ))
    }

    fn swap_active<'a>(
        &'a self,
        generation: ProjectionGeneration,
        expected_source_head: Option<StreamHead>,
    ) -> RepositoryFuture<'a, Result<(), EventRepositoryError>> {
        Box::pin(projection::swap_active(
            self,
            generation,
            expected_source_head,
        ))
    }
}

impl ArtifactCatalog for PostgresEventStore {
    fn resolve<'a>(
        &'a self,
        scope: RepositoryScope,
        artifact_id: ArtifactId,
    ) -> RepositoryFuture<'a, Result<Option<ArtifactReference>, EventRepositoryError>> {
        Box::pin(artifact::resolve(self, scope, artifact_id))
    }
}

impl RetentionRepository for PostgresEventStore {
    fn dry_run<'a>(
        &'a self,
        request: graphhelm_events::RetentionRequest,
        evaluated_at: graphhelm_protocols::PersistedTimestamp,
    ) -> RepositoryFuture<
        'a,
        Result<graphhelm_events::RetentionPlan, graphhelm_events::RetentionError>,
    > {
        Box::pin(retention::dry_run(self, request, evaluated_at))
    }
    fn prepare<'a>(
        &'a self,
        prepared: graphhelm_events::PreparedRetention,
    ) -> RepositoryFuture<
        'a,
        Result<graphhelm_events::RetentionPrepareOutcome, graphhelm_events::RetentionError>,
    > {
        Box::pin(retention::prepare(self, prepared))
    }
    fn finalize<'a>(
        &'a self,
        prepared: graphhelm_events::PreparedRetention,
        receipts: Vec<graphhelm_events::RevocationReceipt>,
        completed_at: graphhelm_protocols::PersistedTimestamp,
        authentication_tag: graphhelm_events::AuthenticationTag,
    ) -> RepositoryFuture<
        'a,
        Result<graphhelm_events::FinalizedRetention, graphhelm_events::RetentionError>,
    > {
        Box::pin(retention::finalize(
            self,
            prepared,
            receipts,
            completed_at,
            authentication_tag,
        ))
    }
    fn pending<'a>(
        &'a self,
        scope: RepositoryScope,
        limit: u32,
    ) -> RepositoryFuture<
        'a,
        Result<Vec<graphhelm_events::PreparedRetention>, graphhelm_events::RetentionError>,
    > {
        Box::pin(retention::pending(self, scope, limit))
    }
    fn change_legal_hold<'a>(
        &'a self,
        change: graphhelm_events::LegalHoldChange,
    ) -> RepositoryFuture<
        'a,
        Result<graphhelm_events::LegalHoldReceipt, graphhelm_events::RetentionError>,
    > {
        Box::pin(retention::change_legal_hold(self, change))
    }
    fn cleanup<'a>(
        &'a self,
        request: graphhelm_events::CleanupRequest,
    ) -> RepositoryFuture<
        'a,
        Result<graphhelm_events::CleanupReceipt, graphhelm_events::RetentionError>,
    > {
        Box::pin(retention::cleanup(self, request))
    }
}
