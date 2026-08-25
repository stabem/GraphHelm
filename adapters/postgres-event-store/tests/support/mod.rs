#![allow(dead_code)]

use std::{str::FromStr, sync::Arc};

use graphhelm_events::{
    AuthenticateRequest, AuthenticationTag, KeyError, KeyProvider, KeyProviderMetadata,
    PreparedAppend, RepositoryFuture, RevocationReceipt, RevokeKeyRequest, SealedEvidence,
    SecretBytes, VerifyAuthenticationRequest, WrapKeyRequest, WrappedKey,
};
use graphhelm_postgres_event_store::{PostgresEventStore, migrate};
use graphhelm_protocols::{
    ActorId, ContentSlot, EventKind, EvidenceId, EvidenceReference, ExecutionId, GraphImported,
    GraphSourceKind, GraphVersionPublished, NewEvent, OpaqueId, PersistedActor, PersistedActorType,
    PersistedGraphVersion, PersistedGraphVersionRef, ProjectId, RawSha256, RepositoryScope,
    Sensitivity, WorkspaceId,
};
use sha2::{Digest, Sha256};
use sqlx::{
    AssertSqlSafe, PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};

pub fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

pub struct TestDatabase {
    root_options: PgConnectOptions,
    database: String,
    role: String,
    admin_pool: PgPool,
    pub runtime_pool: PgPool,
    pub repository: Arc<PostgresEventStore>,
}

impl TestDatabase {
    pub async fn new() -> Self {
        Self::new_with_max_connections(1).await
    }

    pub async fn new_with_max_connections(max_connections: u32) -> Self {
        let admin_url =
            std::env::var("GRAPHHELM_TEST_ADMIN_URL").expect("GRAPHHELM_TEST_ADMIN_URL");
        let root_options = PgConnectOptions::from_str(&admin_url).unwrap();
        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let database = format!("graphhelm_t7_{suffix}");
        let role = format!("graphhelm_rt_{suffix}");
        let password = format!("pw_{suffix}");
        let root_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect_with(root_options.clone())
            .await
            .unwrap();
        sqlx::query(AssertSqlSafe(format!("CREATE ROLE {role} LOGIN PASSWORD '{password}' NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS"))).execute(&root_pool).await.unwrap();
        sqlx::query(AssertSqlSafe(format!("CREATE DATABASE {database}")))
            .execute(&root_pool)
            .await
            .unwrap();
        root_pool.close().await;

        let admin_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect_with(root_options.clone().database(&database))
            .await
            .unwrap();
        migrate(&admin_pool).await.unwrap();
        sqlx::query(AssertSqlSafe(format!(
            "GRANT CONNECT ON DATABASE {database} TO {role}"
        )))
        .execute(&admin_pool)
        .await
        .unwrap();
        sqlx::query("SELECT graphhelm_configure_runtime_role($1::name)")
            .bind(&role)
            .execute(&admin_pool)
            .await
            .unwrap();
        let runtime_pool = PgPoolOptions::new()
            .max_connections(max_connections)
            .connect_with(
                root_options
                    .clone()
                    .database(&database)
                    .username(&role)
                    .password(&password),
            )
            .await
            .unwrap();
        let repository = Arc::new(
            PostgresEventStore::from_pool(runtime_pool.clone(), Arc::new(TestKeyProvider))
                .await
                .unwrap(),
        );
        Self {
            root_options,
            database,
            role,
            admin_pool,
            runtime_pool,
            repository,
        }
    }

    pub fn admin_pool(&self) -> &PgPool {
        &self.admin_pool
    }

    pub fn role_name(&self) -> &str {
        &self.role
    }

    pub async fn cleanup(self) {
        self.runtime_pool.close().await;
        self.admin_pool.close().await;
        let root = PgPoolOptions::new()
            .max_connections(1)
            .connect_with(self.root_options.clone())
            .await
            .unwrap();
        sqlx::query(AssertSqlSafe(format!("DROP DATABASE {}", self.database)))
            .execute(&root)
            .await
            .unwrap();
        sqlx::query(AssertSqlSafe(format!("DROP ROLE {}", self.role)))
            .execute(&root)
            .await
            .unwrap();
        root.close().await;
    }
}

pub struct TestKeyProvider;

fn tag(bytes: &[u8]) -> Vec<u8> {
    let mut digest = Sha256::new();
    digest.update(b"graphhelm-test-key");
    digest.update(bytes);
    digest.finalize().to_vec()
}

impl KeyProvider for TestKeyProvider {
    fn metadata<'a>(&'a self) -> RepositoryFuture<'a, Result<KeyProviderMetadata, KeyError>> {
        Box::pin(async { KeyProviderMetadata::new("test-key", "test", "1", 0) })
    }
    fn wrap<'a>(&'a self, _: WrapKeyRequest) -> RepositoryFuture<'a, Result<WrappedKey, KeyError>> {
        Box::pin(async { Err(KeyError::Unavailable) })
    }
    fn unwrap<'a>(&'a self, _: WrappedKey) -> RepositoryFuture<'a, Result<SecretBytes, KeyError>> {
        Box::pin(async { Err(KeyError::Unavailable) })
    }
    fn revoke<'a>(
        &'a self,
        _: RevokeKeyRequest,
    ) -> RepositoryFuture<'a, Result<RevocationReceipt, KeyError>> {
        Box::pin(async { Err(KeyError::Unavailable) })
    }
    fn authenticate<'a>(
        &'a self,
        request: AuthenticateRequest,
    ) -> RepositoryFuture<'a, Result<AuthenticationTag, KeyError>> {
        Box::pin(
            async move { AuthenticationTag::new("test-key", "hmac-sha256", tag(request.bytes())) },
        )
    }
    fn verify<'a>(
        &'a self,
        request: VerifyAuthenticationRequest,
    ) -> RepositoryFuture<'a, Result<(), KeyError>> {
        Box::pin(async move {
            if request.tag().bytes() == tag(request.bytes()) {
                Ok(())
            } else {
                Err(KeyError::Integrity)
            }
        })
    }
}

pub fn scope(name: &str) -> RepositoryScope {
    RepositoryScope::new(
        WorkspaceId::parse(format!("ws-{name}")).unwrap(),
        ProjectId::parse(format!("prj-{name}")).unwrap(),
        Some(ExecutionId::parse(format!("exec-{name}")).unwrap()),
    )
}

pub fn event(key: &str) -> NewEvent {
    NewEvent::new(
        OpaqueId::parse(key).unwrap(),
        PersistedActor::new(
            PersistedActorType::System,
            ActorId::parse("system.test").unwrap(),
        ),
        Sensitivity::Internal,
        EventKind::GraphImported(GraphImported {
            source_sha256: RawSha256::parse("11".repeat(32)).unwrap(),
            source_kind: GraphSourceKind::Generated,
        }),
        Vec::new(),
        Vec::new(),
    )
}

pub fn valid_graph_publication(
    scope: &RepositoryScope,
    key: &str,
) -> (NewEvent, Vec<SealedEvidence>) {
    valid_graph_publication_for(scope, key, 1, None)
}

pub fn valid_graph_publication_for(
    scope: &RepositoryScope,
    key: &str,
    number: u64,
    predecessor: Option<PersistedGraphVersionRef>,
) -> (NewEvent, Vec<SealedEvidence>) {
    fn push(output: &mut Vec<u8>, value: &[u8]) {
        output.extend_from_slice(&u32::try_from(value.len()).unwrap().to_be_bytes());
        output.extend_from_slice(value);
    }
    let original: PersistedGraphVersion = serde_json::from_str(include_str!(
        "../../../../conformance/schemas/valid/persisted-graph-version.json"
    ))
    .unwrap();
    let slots = original
        .content_slots()
        .iter()
        .map(|slot| {
            ContentSlot::new(
                slot.slot_id().clone(),
                slot.owner_kind(),
                slot.owner_id().clone(),
                slot.field_kind(),
                slot.ordinal(),
                graphhelm_graph::derive_publication_evidence_id(
                    scope,
                    number,
                    original.semantic_hash(),
                    slot,
                )
                .unwrap(),
                slot.content_sha256().clone(),
                slot.sensitivity(),
                slot.required_for_execution(),
            )
        })
        .collect::<Vec<_>>();
    let version = PersistedGraphVersion::new(
        number,
        predecessor,
        original.topology().clone(),
        original.topology_hash().clone(),
        original.semantic_hash().clone(),
        slots,
        original.created_by().clone(),
        original.created_at().clone(),
    )
    .unwrap();
    let evidence = version
        .content_slots()
        .iter()
        .enumerate()
        .map(|(index, slot)| {
            let ciphertext = vec![u8::try_from(index + 1).unwrap(); 16];
            let reference = EvidenceReference::new(
                slot.evidence_id().clone(),
                slot.content_sha256().clone(),
                RawSha256::parse(hex::encode(Sha256::digest(&ciphertext))).unwrap(),
            );
            let mut aad = Vec::new();
            push(&mut aad, b"graphhelm-evidence-aad-v1");
            push(&mut aad, scope.workspace_id().as_str().as_bytes());
            push(&mut aad, scope.project_id().as_str().as_bytes());
            aad.push(1);
            push(&mut aad, scope.execution_id().unwrap().as_str().as_bytes());
            for value in [
                slot.evidence_id().as_str(),
                "1.0.0",
                "application/json",
                match slot.sensitivity() {
                    Sensitivity::Public => "public",
                    Sensitivity::Internal => "internal",
                    Sensitivity::Confidential => "confidential",
                    Sensitivity::Restricted => "restricted",
                },
                "standard",
                slot.content_sha256().as_str(),
            ] {
                push(&mut aad, value.as_bytes());
            }
            let wrapped = WrappedKey::new(
                "key-1",
                slot.evidence_id().as_str(),
                "xchacha20poly1305",
                vec![1; 24],
                vec![2; 48],
                RawSha256::parse(hex::encode(Sha256::digest(&aad))).unwrap(),
            )
            .unwrap();
            SealedEvidence::new(
                reference,
                scope.clone(),
                "application/json",
                slot.sensitivity(),
                "standard",
                "xchacha20poly1305",
                vec![3; 24],
                ciphertext,
                wrapped,
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    (
        NewEvent::new(
            OpaqueId::parse(key).unwrap(),
            version.created_by().clone(),
            Sensitivity::Internal,
            EventKind::GraphVersionPublished(Box::new(GraphVersionPublished { version })),
            evidence
                .iter()
                .map(|value| value.reference().clone())
                .collect(),
            vec![],
        ),
        evidence,
    )
}

pub fn prepared(scope: RepositoryScope, stream: &str, keys: &[&str]) -> PreparedAppend {
    PreparedAppend::new(
        scope,
        OpaqueId::parse(stream).unwrap(),
        1,
        keys.iter().map(|key| event(key)).collect(),
        Vec::new(),
        Vec::new(),
    )
    .unwrap()
}

pub fn sealed(scope: RepositoryScope, id: &str) -> SealedEvidence {
    let zero = RawSha256::parse("00".repeat(32)).unwrap();
    let ciphertext_hash = RawSha256::parse(hex::encode(Sha256::digest([0_u8; 16]))).unwrap();
    let reference = EvidenceReference::new(
        EvidenceId::parse(id).unwrap(),
        zero.clone(),
        ciphertext_hash,
    );
    let mut aad = Vec::new();
    for field in [
        "graphhelm-evidence-aad-v1",
        scope.workspace_id().as_str(),
        scope.project_id().as_str(),
    ] {
        aad.extend_from_slice(&u32::try_from(field.len()).unwrap().to_be_bytes());
        aad.extend_from_slice(field.as_bytes());
    }
    aad.push(1);
    let execution = scope.execution_id().unwrap().as_str();
    aad.extend_from_slice(&u32::try_from(execution.len()).unwrap().to_be_bytes());
    aad.extend_from_slice(execution.as_bytes());
    for field in [
        id,
        "1.0.0",
        "text/plain",
        "restricted",
        "standard",
        zero.as_str(),
    ] {
        aad.extend_from_slice(&u32::try_from(field.len()).unwrap().to_be_bytes());
        aad.extend_from_slice(field.as_bytes());
    }
    let aad_hash = RawSha256::parse(hex::encode(Sha256::digest(aad))).unwrap();
    let wrapped = WrappedKey::new(
        id,
        id,
        "xchacha20poly1305",
        vec![0; 24],
        vec![0; 48],
        aad_hash,
    )
    .unwrap();
    SealedEvidence::new(
        reference,
        scope,
        "text/plain",
        Sensitivity::Restricted,
        "standard",
        "xchacha20poly1305",
        vec![0; 24],
        vec![0; 16],
        wrapped,
    )
    .unwrap()
}

pub fn canonical_bytes(value: &impl serde::Serialize) -> Vec<u8> {
    fn canonical(value: serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::Array(values) => {
                serde_json::Value::Array(values.into_iter().map(canonical).collect())
            }
            serde_json::Value::Object(values) => {
                let mut entries = values.into_iter().collect::<Vec<_>>();
                entries.sort_by(|a, b| a.0.cmp(&b.0));
                serde_json::Value::Object(
                    entries
                        .into_iter()
                        .map(|(key, value)| (key, canonical(value)))
                        .collect(),
                )
            }
            scalar => scalar,
        }
    }
    serde_json::to_vec(&canonical(serde_json::to_value(value).unwrap())).unwrap()
}
