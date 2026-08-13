# Production Event and Evidence Store Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver one pre-release-safe `1.0.0` persistence contract in which the Governor externalizes free-form execution content to encrypted Evidence, the Event Journal stores only deterministic topology and references, JSONL and PostgreSQL share the same wire format, and replay remains truthful after erasure.

**Architecture:** Foundation Graph DSL remains an authoring format. Before publication, the Governor deterministically converts it into a `PersistedGraphVersion`: bounded inline topology plus ordered content slots that reference encrypted Evidence. The local JSONL repository and PostgreSQL adapter accept only this new envelope; all earlier event formats, importers, fixtures, intermediate releases, and compatibility branches are removed because the product has not shipped.

**Tech Stack:** Rust 1.97.1 (edition 2024), Serde/JSON, JSON Schema draft 2020-12, SHA-256, SQLx 0.9.0, Tokio 1.53.1, PostgreSQL 16+, XChaCha20-Poly1305 0.11.0, HMAC-SHA-256, zeroize 1.9.0, Clap, proptest, ephemeral host PostgreSQL, `pg_dump`/`pg_restore`, and GitHub Actions on Windows/Linux.

## Global Constraints

- Track all work in issue `#5`, branch `issue-5-production-event-evidence-store`, and worktree `F:\github\GraphHelm\.worktrees\issue-5-production-event-evidence-store`.
- The approved corrective specification is `docs/superpowers/specs/2026-08-09-safe-persistence-projection-design.md`; Task 1 must accept its ADR/decision-register changes before any further schema or code task.
- Rust is pinned to `1.97.1`; every dependency is exact-pinned and every Cargo command uses `--locked` after lockfile changes.
- Exact workspace dependencies are: `sqlx = 0.9.0` with `default-features = false` and features `runtime-tokio`, `tls-rustls-ring-native-roots`, `postgres`, `json`, `chrono`, `uuid`, `migrate`; `tokio = 1.53.1`; `chacha20poly1305 = 0.11.0` with `zeroize`; `zeroize = 1.9.0` with `derive`; `getrandom = 0.4.3`; `hmac = 0.13.0`; `base64 = 0.23.1`; test-only `static_assertions = 1.1.0`.
- Rebuild one pre-release schema baseline at `schemas/releases/1.0.0`; delete `schemas/releases/1.1.0`; do not create migration resources or compatibility support for the superseded internal formats.
- Preserve provisional `https://p50.dev/...` identifiers for existing documents. New document IDs use the same host and naming convention.
- Remove every runtime/source/conformance surface named `LegacyEventEnvelope`, `LegacyStoredBatch`, `LegacyEventImporter`, `LegacyImportContext`, `LegacyEventsImported`, or equivalent. Decision documentation may name removed types only to explain the pre-release correction.
- Core crates depend on interfaces. `core/events` and `core/protocols` never depend on SQLx, Tokio runtime, PostgreSQL, or process execution. Concrete database and sealed-key implementations live in adapters.
- The Policy Engine gains no persistence, network, model, provider, key, database, or browser dependency.
- Raw Foundation `GraphVersionRecord`, `Diagnostic`, and `PolicyWaiver` are not persistence aliases. Translation to persistence types is explicit and fallible.
- Event payloads contain registered deterministic fields only. Prompts, instructions, objectives, purpose text, model output, raw tool results, logs, credentials, authorization headers, environment values, arbitrary user blobs, dynamic diagnostic prose, and filesystem paths never enter journal bytes.
- The Governor externalizes content by registered typed paths without an LLM. Publication cannot construct an event until every required content slot has a same-scope sealed Evidence reference.
- Evidence plaintext is limited to 16 MiB per item and 64 MiB per append. Secret buffers have no `Debug`, `Clone`, `Serialize`, or public ownership escape and are zeroized on drop.
- Preserve repository bounds: 1 MiB per event, 16 MiB serialized batch, 10,000 events per batch, 64 MiB local journal, 128 ASCII bytes per opaque ID, 256 bytes per actor ID, 1,000 events per read page, 100,000 events per integrity request, 10,000 retention targets, 4 KiB cursors, 64 KiB captured tool stderr, 1 MiB encrypted backup chunks, and 64 GiB encrypted backup maximum.
- `topologyHash` hashes canonical safe topology plus typed slot positions. `semanticHash` additionally hashes ordered slot plaintext digests. Evidence ID, ciphertext, nonce, wrapped key, and key handle never affect graph identity.
- Exact retries are resolved before expected-sequence checks. Exact retry returns the originally committed envelopes byte-for-byte; divergent idempotency reuse fails closed.
- Local publication makes encrypted blobs durable before the journal append and publishes the active-version marker last. A crash may leave unreachable ciphertext but never a committed reference to missing Evidence.
- Every PostgreSQL scoped transaction calls `set_config('graphhelm.workspace_id', value, true)` and `set_config('graphhelm.project_id', value, true)` before queries. No session-persistent scope state is allowed.
- All scoped PostgreSQL canonical/projection tables use composite scope keys plus enabled and forced RLS. Runtime roles have neither ownership nor `BYPASSRLS`; migration/backup roles are separate.
- Evidence encryption uses a fresh 256-bit DEK and 192-bit XChaCha nonce from the OS CSPRNG. AAD binds repository scope, Evidence ID, schema version, media type, sensitivity, retention class, and content digest.
- Stream genesis is `sha256:35c8ab0717bef1684ad07efcf3bedd4648c778a2c944cbd2c7e6a4802e2237b3`, the SHA-256 of UTF-8 `graphhelm:event-chain:v1:genesis`.
- Erasure is prepared/revoked/finalized. Evidence is unreadable at prepare; external key revocation is idempotent; finalization requires an authenticated provider receipt; reconciliation resumes interruption.
- Replay never requires plaintext. Executable materialization fails closed when any required content slot is unavailable.
- No unbounded repository read, selector, verification, rebuild, process output, file read, or backup stream is public.
- PostgreSQL integration tests are ignored in ordinary runs and explicitly executed in CI with `--ignored --test-threads=1` and `GRAPHHELM_TEST_ADMIN_URL`; CI fails if PostgreSQL/client tools are absent.
- Tests use no Docker, internet, production credentials, provider accounts, browser sessions, or production infrastructure.
- CLI stdout is exactly one bounded JSON document and never exposes Evidence plaintext, keys, SQL, URLs, process arguments, absolute paths, temp names, backtraces, or raw tool/database errors. Removing event-format migration does not remove Milestone 02's generic `schema migrate` command or its conformance fixtures.
- Runtime API, scheduler, jobs/leases, full Graph Engine recovery, artifact bytes/S3, Knowledge Graph, Studio, RBAC UI, multi-node replication, external bus, models/tools/sandboxes/Dreams, billing, hosted services, and telemetry export remain out of scope.

---

## Locked File and Dependency Map

```text
Cargo.toml
Cargo.lock
.github/workflows/ci.yml
ci/postgres.ps1

core/protocols/
  Cargo.toml
  src/{lib,event,persistence,projection}.rs
  tests/{wire_roundtrip,persistence_wire}.rs

core/graph/
  src/{lib,canonical,persistence}.rs
  tests/{canonical_hash,persistence_projection}.rs

core/events/
  Cargo.toml
  src/{lib,limits,canonical,store,jsonl,repository,evidence,artifact,key,retention,projection,local}.rs
  tests/{append_only,replay,repository_conformance,evidence_crypto,local_atomicity,retention,projection_rebuild}.rs

core/governor/
  Cargo.toml
  src/{lib,apply,candidate,externalize,publish}.rs
  tests/{draft_application,safe_publication}.rs

adapters/sealed-key-provider/
  Cargo.toml
  src/{lib,keyring,journal}.rs
  tests/sealed_provider.rs

adapters/postgres-event-store/
  Cargo.toml
  migrations/{0001_event_evidence.sql,0002_retention.sql,0003_projections.sql}
  src/{lib,scope,rows,journal,evidence,artifact,integrity,retention,projection,backup,error}.rs
  tests/{migration,repository_conformance,concurrency,isolation,retention,projection,backup_restore}.rs
  tests/support/mod.rs

schemas/
  {repository-scope,sensitivity,event-envelope,evidence-record,artifact-reference,persisted-graph-version}.schema.json
  catalog.json
  CHANGELOG.md
  releases/1.0.0/{catalog.json,*.schema.json}

conformance/
  manifest.json
  schemas/{valid,invalid}/{repository-scope,sensitivity,event-envelope,evidence-record,artifact-reference,persisted-graph-version}.json

apps/cli/
  Cargo.toml
  src/{args,commands/mod}.rs
  src/commands/events/{mod,config,verify,rebuild,backup,restore}.rs
  tests/{cli_smoke,event_store_cli}.rs

docs/
  DECISION_REGISTER.md
  reference/REFERENCE_STACK_AND_ADRS.md
  security/EVENT_EVIDENCE_STORE_THREAT_MODEL.md
  milestones/production-event-evidence-store.md
  operations/OBSERVABILITY_AND_RECOVERY.md
  superpowers/specs/{2026-08-09-production-event-evidence-store-design,2026-08-09-safe-persistence-projection-design}.md
README.md
```

Dependency direction is fixed:

```text
protocols <- graph
protocols <- events <- sealed-key-provider
protocols <- events <- postgres-event-store
protocols + graph + events <- governor
protocols + graph + events + governor + adapters <- cli
schema + schema-evolution <- cli
```

Adapters do not depend on each other. `graph`, `policy`, `governor`, and `simulation` never depend on concrete adapters.

## Public Interfaces Fixed by This Plan

`core/protocols/src/persistence.rs` owns strict IDs, scope, sensitivity, references, and safe diagnostics:

```rust
pub struct WorkspaceId(String);
pub struct ProjectId(String);
pub struct ExecutionId(String);
pub struct EvidenceId(String);
pub struct ArtifactId(String);
pub struct EventHash(String);

#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RepositoryScope {
    pub workspace_id: WorkspaceId,
    pub project_id: ProjectId,
    pub execution_id: Option<ExecutionId>,
}

#[serde(rename_all = "snake_case")]
pub enum Sensitivity { Public, Internal, Confidential, Restricted }

#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvidenceReference {
    pub evidence_id: EvidenceId,
    pub content_sha256: String,
    pub ciphertext_sha256: String,
}

#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtifactReference {
    pub artifact_id: ArtifactId,
    pub locator: String,
    pub content_sha256: String,
    pub media_type: String,
    pub byte_length: u64,
    pub sensitivity: Sensitivity,
    pub metadata_version: semver::Version,
}

#[serde(transparent)]
pub struct DiagnosticDomainPath(String);

#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersistedDiagnostic {
    pub code: String,
    pub severity: Severity,
    pub path: DiagnosticDomainPath,
    pub component: DiagnosticComponent,
    pub source_content_sha256: Option<String>,
    pub detail_evidence_id: Option<EvidenceId>,
}
```

`core/protocols/src/projection.rs` owns the safe graph projection:

```rust
#[serde(rename_all = "snake_case")]
pub enum ContentOwnerKind { Graph, Node, Agent, Edge, Policy, Diagnostic }

#[serde(rename_all = "snake_case")]
pub enum ContentFieldKind {
    DisplayName,
    Description,
    Objective,
    Purpose,
    Instructions,
    CompletionContract,
    PolicyText,
    DiagnosticDetail,
    ContextPath,
    PermissionPath,
    IsolationPath,
}

#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContentSlot {
    pub slot_id: String,
    pub owner_kind: ContentOwnerKind,
    pub owner_id: String,
    pub field_kind: ContentFieldKind,
    pub ordinal: u32,
    pub evidence_id: EvidenceId,
    pub content_sha256: String,
    pub sensitivity: Sensitivity,
    pub required_for_execution: bool,
}

#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersistedGraphVersion {
    pub number: u64,
    pub predecessor: Option<PersistedGraphVersionRef>,
    pub topology: PersistedTopology,
    pub topology_hash: String,
    pub semantic_hash: String,
    pub content_slots: Vec<ContentSlot>,
    pub created_by: Actor,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersistedGraphVersionRef {
    pub number: u64,
    pub semantic_hash: String,
}

#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersistedTopology {
    pub api_version: String,
    pub kind: String,
    pub graph_id: String,
    pub execution_id: ExecutionId,
    pub labels: BTreeMap<String, String>,
    pub entrypoints: Vec<String>,
    pub nodes: BTreeMap<String, PersistedNode>,
    pub edges: Vec<PersistedEdge>,
    pub budgets: GraphBudgets,
    pub policies: Vec<PersistedControl>,
    pub completion: PersistedControl,
}

#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersistedNode {
    pub node_type: NodeType,
    pub optionality: Optionality,
    pub controls: Vec<PersistedControl>,
    pub content_slot_ids: Vec<String>,
}

#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersistedEdge {
    pub id: String,
    pub from: String,
    pub to: String,
    pub edge_type: EdgeType,
    pub priority: Option<i64>,
    pub bindings: BTreeMap<String, String>,
    pub condition: Option<PersistedControl>,
}

#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersistedControl {
    pub control_type: String,
    pub identifiers: BTreeMap<String, String>,
    pub digests: BTreeMap<String, String>,
    pub integers: BTreeMap<String, i64>,
    pub flags: BTreeMap<String, bool>,
}
```

The original eight-kind interface is superseded by D-037/ADR-023. `ContextPath` owns path strings from registered context source-scope entries, `PermissionPath` owns path strings from registered permission scopes, and `IsolationPath` owns registered isolation filesystem writable paths. All three are ordered, Evidence-backed, `restricted`, required for execution, and included in slot topology/semantic hashing; their plaintext never enters topology, events, logs, diagnostics, or errors. This pre-release correction updates only the single `1.0.0` baseline and retains no alias or compatibility branch.

The wire shapes above are exact; implementation fields are private and exposed through validated constructors/getters. They contain IDs, enums, booleans, bounded integers, structural schema digests, bindings, budgets, and registered deterministic controls only. They expose no arbitrary `serde_json::Value` constructor.

`EventEnvelope` is the only writable event format:

```rust
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewEvent {
    pub idempotency_key: String,
    pub actor: Actor,
    pub sensitivity: Sensitivity,
    pub kind: EventKind,
    pub evidence_refs: Vec<EvidenceReference>,
    pub artifact_refs: Vec<ArtifactReference>,
}

#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EventEnvelope {
    pub schema_version: semver::Version,
    pub event_id: String,
    pub scope: RepositoryScope,
    pub stream_id: String,
    pub sequence: u64,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
    pub idempotency_key: String,
    pub actor: Actor,
    pub sensitivity: Sensitivity,
    pub kind: EventKind,
    pub evidence_refs: Vec<EvidenceReference>,
    pub artifact_refs: Vec<ArtifactReference>,
    pub previous_hash: EventHash,
    pub event_hash: EventHash,
}
```

`EventKind` contains the 11 Foundation state-transition names converted to safe payloads plus exactly five new production variants: `IntegrityCheckpointCreated`, `EvidenceErasureRequested`, `EvidenceErasureCompleted`, `EvidenceCiphertextDeleted`, and `EvidenceLegalHoldChanged`. `GraphVersionPublished` carries `PersistedGraphVersion`; `GraphValidationFailed` carries `PersistedDiagnostic`; `PolicyWaiverCreated` carries the corrected bounded `PolicyWaiver`. `GraphImported.sourceKind` is closed to `graph_document | generated`. No legacy/import-receipt variant exists.

The 16 payload shapes are fixed as follows:

| Event kind | Persisted fields |
|---|---|
| `graph_imported` | `sourceSha256`, `sourceKind` |
| `graph_validation_failed` | ordered `PersistedDiagnostic[]` |
| `graph_version_published` | `PersistedGraphVersion` |
| `draft_proposed` | draft ID, expected version/hash, operation count |
| `draft_rejected` | draft ID, closed/bounded `reasonCode`, ordered safe diagnostics, optional detail Evidence ID |
| `draft_applied` | draft ID, graph version/hash |
| `policy_obligation_evaluated` | draft ID, requirement ID, status enum, Evidence IDs, bounded `reasonCode`, overrideable flag |
| `policy_waiver_created` | corrected bounded `PolicyWaiver` |
| `simulation_started` | simulation ID and graph version/hash |
| `node_state_changed` | simulation ID, node ID, previous/next state |
| `simulation_completed` | simulation ID and status |
| `integrity_checkpoint_created` | stream, sequence, event hash, repository format, authentication tag metadata |
| `evidence_erasure_requested` | operation/evidence/key-handle/policy/authority/reason/state IDs and timestamp |
| `evidence_erasure_completed` | the full request correlation plus ciphertext digest, provider receipt ID/epoch, final state and timestamp |
| `evidence_ciphertext_deleted` | operation/evidence IDs, ciphertext digest and deletion timestamp |
| `evidence_legal_hold_changed` | hold/evidence/authority/reason IDs, placed/released state and timestamp |

Any human explanation behind a `reasonCode` belongs in referenced Evidence, not the event payload.

`core/events` exposes owned object-safe repositories:

```rust
pub type RepositoryFuture<'a, T> =
    Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub struct AppendRequest {
    pub scope: RepositoryScope,
    pub stream_id: String,
    pub expected_next_sequence: u64,
    pub events: Vec<NewEvent>,
}

pub struct PreparedAppend {
    pub journal: AppendRequest,
    pub evidence: Vec<SealedEvidence>,
    pub artifacts: Vec<ArtifactRegistration>,
}

pub trait EventStore: Send + Sync {
    fn append_atomic(&self, request: &PreparedAppend)
        -> Result<Vec<EventEnvelope>, RepositoryError>;
    fn read_stream(&self, request: &ReadStreamRequest)
        -> Result<EventPage, RepositoryError>;
}

pub trait AsyncEventRepository: Send + Sync {
    fn append_atomic<'a>(&'a self, request: PreparedAppend)
        -> RepositoryFuture<'a, Result<Vec<EventEnvelope>, RepositoryError>>;
    fn read_stream<'a>(&'a self, request: ReadStreamRequest)
        -> RepositoryFuture<'a, Result<EventPage, RepositoryError>>;
    fn stream_head<'a>(&'a self, scope: RepositoryScope, stream_id: String)
        -> RepositoryFuture<'a, Result<Option<StreamHead>, RepositoryError>>;
    fn verify_range<'a>(&'a self, request: VerifyRangeRequest)
        -> RepositoryFuture<'a, Result<IntegrityReport, RepositoryError>>;
    fn append_checkpoint<'a>(&'a self, checkpoint: AuthenticatedCheckpoint)
        -> RepositoryFuture<'a, Result<AuthenticatedCheckpoint, RepositoryError>>;
    fn latest_checkpoint<'a>(&'a self, scope: RepositoryScope, stream_id: String)
        -> RepositoryFuture<'a, Result<Option<AuthenticatedCheckpoint>, RepositoryError>>;
}

pub struct ReadStreamRequest {
    pub scope: RepositoryScope,
    pub stream_id: String,
    pub start: ReadStart,
    pub limit: u32,
}

pub enum ReadStart { Beginning, After { sequence: u64, event_hash: EventHash }, Cursor(String) }
pub struct EventPage { pub events: Vec<EventEnvelope>, pub next_cursor: Option<String>, pub head: Option<StreamHead> }

pub trait EvidenceRepository: Send + Sync {
    fn get_sealed<'a>(&'a self, scope: RepositoryScope, evidence_id: EvidenceId)
        -> RepositoryFuture<'a, Result<EvidenceRead, RepositoryError>>;
}

pub trait ArtifactCatalog: Send + Sync {
    fn resolve<'a>(&'a self, scope: RepositoryScope, artifact_id: ArtifactId)
        -> RepositoryFuture<'a, Result<Option<ArtifactReference>, RepositoryError>>;
}

pub struct StreamHead { pub next_sequence: u64, pub last_event_hash: EventHash }
pub struct VerifyRangeRequest { pub scope: RepositoryScope, pub stream_id: String, pub start_sequence: u64, pub max_events: u32 }
pub struct IntegrityReport { pub verified_events: u32, pub verified_through: Option<u64>, pub head: Option<StreamHead> }
pub struct AuthenticatedCheckpoint {
    pub scope: RepositoryScope,
    pub stream_id: String,
    pub sequence: u64,
    pub event_hash: EventHash,
    pub repository_format_version: u32,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub tag: AuthenticationTag,
}

pub enum EvidenceRead { Available(SealedEvidence), Unavailable(EvidenceUnavailableReason) }
pub enum EvidenceUnavailableReason { ErasurePending, Erased, Expired, MissingKey, IntegrityFailed }
pub struct ArtifactRegistration { pub reference: ArtifactReference, pub producer_idempotency_key: String }
```

Evidence/key interfaces remain adapter-neutral:

```rust
pub struct SecretBytes(Zeroizing<Vec<u8>>);

pub struct EvidenceInput {
    pub local_ref: String,
    pub media_type: String,
    pub sensitivity: Sensitivity,
    pub retention_class: String,
    pub plaintext: SecretBytes,
}

pub struct SealedEvidence {
    pub reference: EvidenceReference,
    pub scope: RepositoryScope,
    pub media_type: String,
    pub sensitivity: Sensitivity,
    pub retention_class: String,
    pub algorithm: String,
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
    pub wrapped_key: WrappedKey,
}

pub trait EvidenceSealer: Send + Sync {
    fn seal<'a>(&'a self, scope: RepositoryScope, input: EvidenceInput)
        -> RepositoryFuture<'a, Result<SealedEvidence, EvidenceError>>;
}

pub trait EvidenceOpener: Send + Sync {
    fn open<'a>(&'a self, scope: RepositoryScope, evidence: &'a SealedEvidence)
        -> RepositoryFuture<'a, Result<SecretBytes, EvidenceError>>;
}

pub trait KeyProvider: Send + Sync {
    fn wrap<'a>(&'a self, request: WrapKeyRequest)
        -> RepositoryFuture<'a, Result<WrappedKey, KeyError>>;
    fn unwrap<'a>(&'a self, wrapped: WrappedKey)
        -> RepositoryFuture<'a, Result<SecretBytes, KeyError>>;
    fn revoke<'a>(&'a self, request: RevokeKeyRequest)
        -> RepositoryFuture<'a, Result<RevocationReceipt, KeyError>>;
    fn authenticate<'a>(&'a self, request: AuthenticateRequest)
        -> RepositoryFuture<'a, Result<AuthenticationTag, KeyError>>;
    fn verify<'a>(&'a self, request: VerifyAuthenticationRequest)
        -> RepositoryFuture<'a, Result<(), KeyError>>;
}

pub struct WrappedKey { pub key_id: String, pub handle: String, pub algorithm: String, pub nonce: Vec<u8>, pub ciphertext: Vec<u8> }
pub struct WrapKeyRequest { pub handle: String, pub plaintext_key: SecretBytes, pub aad: Vec<u8> }
pub struct RevokeKeyRequest { pub handle: String, pub idempotency_key: String }
pub struct RevocationReceipt { pub handle: String, pub idempotency_key: String, pub epoch: u64, pub authentication_tag: AuthenticationTag }
pub struct AuthenticateRequest { pub purpose: String, pub bytes: Vec<u8> }
pub struct AuthenticationTag { pub key_id: String, pub algorithm: String, pub bytes: Vec<u8> }
pub struct VerifyAuthenticationRequest { pub purpose: String, pub bytes: Vec<u8>, pub tag: AuthenticationTag }
```

`EvidenceProtector<K: KeyProvider>` implements both `EvidenceSealer` and `EvidenceOpener`; callers depend on those traits rather than concrete adapters.

`core/governor/src/externalize.rs` fixes the translation boundary:

```rust
pub struct ProjectionPreparation {
    pub version: PersistedGraphVersion,
    pub evidence: Vec<SealedEvidence>,
    pub evidence_refs: Vec<EvidenceReference>,
}

pub trait GraphExternalizer: Send + Sync {
    fn prepare<'a>(
        &'a self,
        scope: RepositoryScope,
        version: &'a GraphVersionRecord,
    ) -> RepositoryFuture<'a, Result<ProjectionPreparation, GovernorError>>;
}
```

Only the Governor calls this boundary. Event constructors for `GraphVersionPublished` accept `ProjectionPreparation`, not raw `GraphVersionRecord`.

## Stable Diagnostic Catalog

| Code | Meaning |
|---|---|
| `GHE001_SEQUENCE_CONFLICT` | Expected sequence differs from the locked stream head. |
| `GHE002_CORRUPT_BATCH` | Physical batch, strict envelope, checksum, or format is corrupt. |
| `GHE003_IDEMPOTENCY_CONFLICT` | A key exists with a different canonical request digest. |
| `GHE004_SCOPE_VIOLATION` | Scope is absent, invalid, foreign, or inconsistent. |
| `GHE005_INTEGRITY_FAILURE` | Hash chain, checkpoint, cursor, Evidence, or artifact digest fails. |
| `GHE006_LIMIT_EXCEEDED` | A deterministic repository/evidence/page/range bound is exceeded. |
| `GHE007_UNSUPPORTED_FORMAT` | Repository bytes are not the single supported baseline format. |
| `GHE008_CONTENT_UNAVAILABLE` | A required content slot cannot be materialized. |
| `GHE009_EXTERNALIZATION_FAILED` | Authoring content cannot be safely projected and sealed. |
| `GHEV001_EVIDENCE_UNAVAILABLE` | Evidence is pending erasure, erased, expired, missing-key, or corrupt. |
| `GHEV002_LEGAL_HOLD` | A legal hold blocks erasure or cleanup. |
| `GHEV003_RETENTION_INELIGIBLE` | Policy, age, scope, or state does not permit the action. |
| `GHEV004_EVIDENCE_INVALID` | Evidence metadata, ciphertext, AAD, or registration is invalid. |
| `GHK001_KEY_UNAVAILABLE` | Key handle, KEK, tag, or provider state is unavailable/invalid. |
| `GHB001_BACKUP_INVALID` | Backup header, manifest, ciphertext, tool version, or identity is invalid. |
| `GHB002_RESTORE_INVALID` | Restore target or post-restore verification is invalid. |
| `GHPROJ001_WATERMARK_MISMATCH` | Projection cursor/hash/version cannot safely resume. |

Public diagnostics use stable JSON Pointers and generic component IDs. Adapter internals retain causes without exposing them through `Display` or CLI JSON.

---

### Task 1: Accept the safe projection ADR and pre-release reset

**Files:**
- Modify: `docs/DECISION_REGISTER.md`
- Modify: `docs/reference/REFERENCE_STACK_AND_ADRS.md`
- Modify: `docs/security/EVENT_EVIDENCE_STORE_THREAT_MODEL.md`
- Modify: `docs/superpowers/specs/2026-08-09-production-event-evidence-store-design.md`
- Modify: `docs/superpowers/specs/2026-08-09-safe-persistence-projection-design.md`

**Interfaces:** Produces accepted D-036/ADR-022 language and removes the normative legacy/raw-parity assumptions that gate every later task.

- [ ] **Step 1: Write the failing documentation gate**

```powershell
$required = @(
  'D-036',
  'ADR-022',
  'PersistedGraphVersion',
  'no legacy compatibility layer',
  'Evidence criptografada'
)
foreach ($term in $required) {
  if (-not (rg -n --fixed-strings $term docs)) { throw "missing $term" }
}
```

- [ ] **Step 2: Run the gate and observe RED**

Run the PowerShell block above. Expected: FAIL because D-036 and ADR-022 do not exist.

- [ ] **Step 3: Record the accepted decision**

Add D-036 and ADR-022 with these exact decisions: authoring/persistence representations are distinct; Governor externalization is mandatory; free-form content becomes Evidence; safe topology remains inline; semantic hash uses content digests; erased required content blocks execution; JSONL adopts the new format immediately; pre-release legacy/runtime compatibility and release `1.1.0` are removed; baseline `1.0.0` is rebuilt; safe diagnostics contain no filesystem/source path or dynamic prose and allow only a registered domain JSON Pointer; bounded persistence waiver supersedes the internal draft.

- [ ] **Step 4: Update threat scenarios and supersession text**

```text
AT-14: raw authoring record reaches Event Journal
AT-15: content slot and evidenceRefs diverge
AT-16: plaintext appears in journal, diagnostics, temp names, or crash output
AT-17: erased required content is executed from cache/fallback
AT-18: superseded internal format is heuristically accepted
```

Mark the raw `GraphVersionRecord` persistence statements in the earlier design as superseded by ADR-022; do not delete historical rationale.

- [ ] **Step 5: Re-run documentation gates**

```powershell
rg -n "D-036|ADR-022|PersistedGraphVersion|AT-14|AT-18" docs
rg -n "TBD|TODO|unimplemented|LegacyEventImporter" docs/DECISION_REGISTER.md docs/reference/REFERENCE_STACK_AND_ADRS.md docs/security/EVENT_EVIDENCE_STORE_THREAT_MODEL.md docs/superpowers/specs
git diff --check
```

Expected: required terms found; placeholder/legacy runtime symbol scan has no unintended matches; diff check passes.

- [ ] **Step 6: Commit**

```powershell
git add docs/DECISION_REGISTER.md docs/reference/REFERENCE_STACK_AND_ADRS.md docs/security/EVENT_EVIDENCE_STORE_THREAT_MODEL.md docs/superpowers/specs
git commit -m "docs(architecture): accept safe persistence projection" -m "Refs #5" -m "Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 2: Rebuild the single safe schema baseline

**Files:**
- Modify: `schemas/policy-waiver.schema.json`
- Create/replace: `schemas/{repository-scope,sensitivity,event-envelope,evidence-record,artifact-reference,persisted-graph-version}.schema.json`
- Modify: `schemas/catalog.json`, `schemas/CHANGELOG.md`
- Replace: `schemas/releases/1.0.0/**`
- Delete: `schemas/releases/1.1.0/**`
- Modify: `conformance/manifest.json`
- Create/replace: `conformance/schemas/{valid,invalid}/{repository-scope,sensitivity,event-envelope,evidence-record,artifact-reference,persisted-graph-version}.json`
- Modify: `conformance/schemas/{valid,invalid}/policy-waiver.json`
- Modify: `core/schema-evolution/tests/{catalog_integrity,conformance}.rs`
- Modify: `apps/cli/tests/schema_cli.rs`

**Interfaces:** Produces exactly 15 schema documents in aggregate release `1.0.0`, 50 public conformance cases and 52 declared resources, with no persistence-format migration or legacy resources. The generic schema-evolution migration conformance suite from Milestone 02 remains unchanged. Task 3 implements these wire contracts exactly.

- [ ] **Step 1: Write RED inventory and security tests**

```rust
#[test]
fn repository_has_one_safe_initial_release() {
    let catalog = load_root_catalog();
    assert_eq!(catalog.release_version, Version::new(1, 0, 0));
    assert_eq!(catalog.schemas.len(), 15);
    assert!(!root().join("schemas/releases/1.1.0").exists());
    assert_eq!(public_case_count(), 50);
    assert_eq!(declared_resource_count(), 52);
}

#[test]
fn event_schema_rejects_authoring_plaintext_and_paths() {
    for pointer in forbidden_projection_pointers() {
        assert_schema_rejects("event-envelope", event_with(pointer, "secret text"));
    }
}
```

- [ ] **Step 2: Run RED**

```powershell
cargo +1.97.1 test -p graphhelm-schema-evolution --test catalog_integrity --locked -- --nocapture
cargo +1.97.1 test -p graphhelm-schema-evolution --test conformance --locked -- --nocapture
```

Expected: FAIL because `1.1.0` exists, catalog is not the reset baseline, and the safe projection schema is absent.

- [ ] **Step 3: Publish exact safe contracts**

`persisted-graph-version.schema.json` defines bounded topology and content slots. `event-envelope.schema.json` contains 16 event variants and references the safe projection. All four persistence timestamp definitions use one canonical RFC 3339 UTC wire form: uppercase `T`, terminal uppercase `Z`, four-digit year `0000..9999`, valid end-of-day leap seconds, and at most nine fractional digits. Offset forms are rejected because UTC normalization at the year boundaries is not bijective with this profile. `policy-waiver.schema.json` requires bounded IDs, 1..64 bounded risks, safe integer graph version, these canonical timestamps, optional non-null bounded reason, and no unknown fields. The existing graph/agent/node/edge documents remain authoring contracts that allow inline content; they are not reused as persistence projections. `Diagnostic.source` is replaced by closed component/source-digest fields. No schema property named `instructions`, `prompt`, `objective`, `purpose`, `output`, `log`, `credential`, `environment`, filesystem `path`, or arbitrary `message` is accepted in a persistence payload.

- [ ] **Step 4: Reset catalog, snapshot, fixtures, and changelog**

```text
releaseVersion = 1.0.0
schemaCount = 15
publicCases = 50
declaredResources = 52
persistenceReleaseMigrations = none
previousRelease = none
```

Root and `releases/1.0.0` copies must be raw-byte identical. Generate SHA-256 values through `graphhelm_schema_evolution::schema_digest`, not shell text hashing.

- [ ] **Step 5: Run focused GREEN gates**

```powershell
cargo +1.97.1 test -p graphhelm-schema-evolution --all-features --locked
cargo +1.97.1 test -p graphhelm-cli --test schema_cli --locked
cargo +1.97.1 run --locked -p graphhelm-cli -- schema catalog
cargo +1.97.1 run --locked -p graphhelm-cli -- schema conformance
```

Expected: 15 schemas at `1.0.0`; 50/50 cases pass; no migration is reported.

- [ ] **Step 6: Prove removal and immutable parity**

```powershell
if (Test-Path schemas/releases/1.1.0) { throw '1.1.0 still exists' }
rg -n "LegacyEvent|legacy_import|legacy-import|GHIMP" schemas conformance core/schema-evolution apps/cli/tests/schema_cli.rs
git diff --check
```

Expected: no legacy matches in runtime/schema/conformance surfaces and clean diff.

- [ ] **Step 7: Commit**

```powershell
git add schemas conformance core/schema-evolution/tests apps/cli/tests/schema_cli.rs
git commit -m "feat(schema): publish safe persistence baseline" -m "Refs #5" -m "Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 3: Implement persistence primitives and safe projection types

**Files:**
- Modify: `Cargo.toml`, `Cargo.lock`
- Modify: `core/protocols/Cargo.toml`
- Create: `core/protocols/src/{persistence,projection}.rs`
- Modify: `core/protocols/src/{lib,policy,diagnostic}.rs`
- Create: `core/protocols/tests/persistence_wire.rs`
- Modify: `core/protocols/tests/wire_roundtrip.rs`

**Interfaces:** Produces strict IDs, scope, sensitivity, references, `PersistedGraphVersion`, topology/content-slot types, safe `PersistedDiagnostic`, and corrected `PolicyWaiver`. It does not replace the active event module yet, so the commit remains buildable; Task 6 switches the complete workspace to the single new envelope atomically.

- [ ] **Step 1: Write RED wire/schema tests**

```rust
#[test]
fn safe_projection_round_trips_and_validates() {
    let version = fixture_persisted_graph_version();
    let json = serde_json::to_value(&version).unwrap();
    assert_schema_valid("persisted-graph-version", &json);
    assert_eq!(serde_json::from_value::<PersistedGraphVersion>(json).unwrap(), version);
}

#[test]
fn no_raw_foundation_record_can_construct_graph_published() {
    assert_not_impl_any!(GraphVersionRecord: Into<GraphVersionPublished>);
}
```

Add tests for ID grammar, closed sensitivity, 256-byte actor IDs, hashes, ordered content slots, optional execution scope, `reason: None` omission, unknown-field rejection, private validated topology constructors, and the absence of raw-Foundation conversion shortcuts.

- [ ] **Step 2: Run RED**

```powershell
cargo +1.97.1 test -p graphhelm-protocols --test persistence_wire --locked
```

Expected: FAIL because persistence/projection modules and types do not exist.

- [ ] **Step 3: Implement typed IDs and safe projections**

Use private-string newtypes with `parse`, `as_str`, `Display`, exact ASCII/length validation, and Serde validation on deserialize. Implement private fields plus validated constructors for topology/control types. No constructor accepts arbitrary `Value` or raw Foundation records.

- [ ] **Step 4: Implement safe diagnostics and corrected waiver serialization**

`PersistedDiagnostic` has stable code, severity, validated `DiagnosticDomainPath`, component enum, optional source-content digest, and optional nominal `EvidenceId` only. The path type allows the empty root or a JSON Pointer whose complete decoded token sequence matches the closed structural grammar of the authoring graph and durable GraphHelm contracts. Structural fields are position-specific, scalars reject descendants, arrays use canonical bounded decimal indices, and map keys reuse the exact owning nominal grammar (`OpaqueId`, `SafeKey`, or another typed rule); correct RFC 6901 `~0`/`~1` decoding precedes validation. This positive grammar rejects impossible source/filesystem/URI/traversal shapes without an arbitrary filename or extension denylist, and future fields require explicit registration. Conversion from Foundation diagnostics is later, explicit, and fallible: no automatic `From<Diagnostic>` or copying of `Diagnostic.path`/`source`. `PolicyWaiver.reason` uses `#[serde(default, skip_serializing_if = "Option::is_none")]`. Both types validate on deserialization and contain no filesystem-path/free-form persistence alias.

- [ ] **Step 5: Prove schema parity and forbidden-field rejection**

```powershell
cargo +1.97.1 test -p graphhelm-protocols --test persistence_wire --locked
cargo +1.97.1 test --workspace --all-features --locked
cargo +1.97.1 clippy -p graphhelm-protocols --all-targets --locked -- -D warnings
```

Expected: projection tests and the unchanged workspace consumers all pass.

- [ ] **Step 6: Commit**

```powershell
git add Cargo.toml Cargo.lock core/protocols
git commit -m "feat(protocols): add safe persistence wire types" -m "Refs #5" -m "Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 4: Add encrypted Evidence, artifacts, and sealed keys

**Files:**
- Modify: `Cargo.toml`, `Cargo.lock`, `core/events/Cargo.toml`, `core/events/src/lib.rs`
- Create: `core/events/src/{evidence,artifact,key}.rs`
- Create: `core/events/tests/evidence_crypto.rs`
- Create: `adapters/sealed-key-provider/{Cargo.toml,src/lib.rs,src/keyring.rs,src/journal.rs,tests/sealed_provider.rs}`

**Interfaces:** Produces `SecretBytes`, `EvidenceInput`, `SealedEvidence`, `EvidenceProtector`, `EvidenceSealer`, `ArtifactRegistration`, `KeyProvider`, and `SealedKeyProvider`. Task 5 uses sealing; Tasks 6–8 persist/revoke it.

- [ ] **Step 1: Write RED crypto and trait-surface tests**

```rust
#[tokio::test]
async fn sealed_evidence_round_trips_without_trait_leaks() {
    assert_not_impl_any!(SecretBytes: Clone, Debug, Serialize);
    let sealed = fixed_protector().seal(scope(), input(b"instructions")).await.unwrap();
    assert_ne!(sealed.ciphertext, b"instructions");
    assert_eq!(fixed_protector().open(scope(), &sealed).await.unwrap().expose(Vec::from), b"instructions");
}
```

Add AAD mismatch, tamper, 16 MiB/item, 64 MiB/append, nonce uniqueness, key-revocation, exact retry, divergent retry, corrupt keyring, symlink/no-overwrite, and zeroization tests.

- [ ] **Step 2: Run RED**

```powershell
cargo +1.97.1 test -p graphhelm-events --test evidence_crypto --locked
```

Expected: FAIL because Evidence/key modules are absent.

- [ ] **Step 3: Implement secret and cryptographic core**

Use `Zeroizing<Vec<u8>>`, XChaCha20-Poly1305, `getrandom::fill`, canonical AAD, one fresh DEK/nonce per item, ciphertext SHA-256, and content SHA-256 supplied from the Governor's canonical content. Validate sizes before allocation/encryption. Redact all crypto errors.

- [ ] **Step 4: Implement sealed key provider**

The adapter stores an atomic versioned keyring plus append-only revocation journal, both authenticated. It supports wrap/unwrap/revoke/authenticate/verify and idempotent receipt recovery. It never exposes KEK bytes through public APIs or formatting.

- [ ] **Step 5: Run GREEN and dependency audit**

```powershell
cargo +1.97.1 test -p graphhelm-events --test evidence_crypto --locked
cargo +1.97.1 test -p graphhelm-sealed-key-provider --locked
cargo +1.97.1 clippy -p graphhelm-events -p graphhelm-sealed-key-provider --all-targets --locked -- -D warnings
cargo +1.97.1 metadata --locked --no-deps --format-version 1
```

- [ ] **Step 6: Commit**

```powershell
git add Cargo.toml Cargo.lock core/events adapters/sealed-key-provider
git commit -m "feat(evidence): add encryption and sealed keys" -m "Refs #5" -m "Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 5: Externalize authoring content in the Governor

**Files:**
- Modify: `core/graph/src/{lib,canonical}.rs`, `core/graph/tests/canonical_hash.rs`
- Create: `core/graph/src/persistence.rs`, `core/graph/tests/persistence_projection.rs`
- Modify: `core/governor/Cargo.toml`, `core/governor/src/{lib,apply}.rs`
- Create: `core/governor/src/{externalize,publish}.rs`
- Create: `core/governor/tests/safe_publication.rs`

**Interfaces:** Produces `GraphExternalizer`, `ProjectionPreparation`, and deterministic topology/semantic hashing. It does not replace event writers yet; Task 6 consumes the preparation and switches every writer atomically.

- [ ] **Step 1: Write RED official-example publication tests**

```rust
#[tokio::test]
async fn official_graphs_publish_without_plaintext_in_journal_payload() {
    for graph in official_graphs() {
        let version = publish(graph);
        let prepared = fixed_externalizer().prepare(scope(), &version).await.unwrap();
        let bytes = serde_json::to_vec(&prepared.version).unwrap();
        for plaintext in authored_content_values(graph) {
            assert!(!bytes.windows(plaintext.len()).any(|window| window == plaintext));
        }
        assert_slot_ref_bijection(&prepared);
    }
}
```

Add RED tests for graph names, objective, purpose, instructions, textual completion, policy text, nested forbidden keys, structural-schema annotation stripping, slot ordering, duplicate/missing refs, and no LLM/network/tool calls.

- [ ] **Step 2: Run RED**

```powershell
cargo +1.97.1 test -p graphhelm-governor --test safe_publication --locked
cargo +1.97.1 test -p graphhelm-graph --test persistence_projection --locked
```

Expected: FAIL because the externalizer and safe projection do not exist.

- [ ] **Step 3: Implement deterministic extraction and safe topology**

Register typed extractors for every content-bearing Foundation field. Canonicalize extracted JSON/string bytes, calculate content digest, create deterministic slot positions, replace the field with a typed binding, strip schema annotations, and reject unregistered arbitrary values. Never infer through key-name heuristics alone.

- [ ] **Step 4: Implement dual hashes**

```rust
topology_hash = sha256(canonical_safe_topology_with_slot_positions);
semantic_hash = sha256(canonical_safe_topology_and_ordered_slot_content_digests);
```

Property tests must prove insertion-order independence, re-encryption independence, UI/layout exclusion, and content-change sensitivity.

- [ ] **Step 5: Integrate Governor transaction boundary**

Add a new Governor preparation phase that constructs and validates the authoring candidate, then calls the externalizer. It returns `ProjectionPreparation` without changing active version or journal. Task 6 wires this phase into publication. Failed sealing/projection/schema/hash/reference checks leave all existing behavior unchanged in this independently buildable commit.

- [ ] **Step 6: Run adversarial GREEN gates**

```powershell
cargo +1.97.1 test -p graphhelm-graph --locked
cargo +1.97.1 test -p graphhelm-governor --locked
cargo +1.97.1 clippy -p graphhelm-graph -p graphhelm-governor --all-targets --locked -- -D warnings
```

- [ ] **Step 7: Commit**

```powershell
git add core/graph core/governor Cargo.lock
git commit -m "feat(governor): externalize graph content before publish" -m "Refs #5" -m "Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 6: Replace the local JSONL repository atomically

**Files:**
- Replace: `core/protocols/src/event.rs`
- Modify: `core/protocols/src/lib.rs`, `core/protocols/tests/{wire_roundtrip,persistence_wire}.rs`
- Modify: `core/events/src/{lib,store,jsonl,projection}.rs`
- Create: `core/events/src/{limits,canonical,repository,local}.rs`
- Modify: `core/events/tests/{append_only,replay}.rs`
- Create: `core/events/tests/{repository_conformance,local_atomicity}.rs`
- Modify: `core/governor/src/{apply,publish}.rs`, `core/governor/tests/draft_application.rs`
- Modify: `core/simulation/src/{engine,fixtures}.rs`, `core/simulation/tests/deterministic_simulation.rs`
- Modify: `apps/cli/src/commands/{simulate,replay,draft}.rs`
- Modify: `apps/cli/tests/cli_smoke.rs`

**Interfaces:** Atomically replaces every workspace event producer/consumer with the 16-variant strict envelope, wires Governor `ProjectionPreparation` into publication, and produces the single synchronous local `EventStore`, safe repository format v1, crash reconciliation, hash chains, and bounded reads.

- [ ] **Step 1: Write RED local repository tests**

```rust
#[test]
fn committed_event_never_references_missing_evidence() {
    for failpoint in LocalFailpoint::all() {
        let repo = repo_with_failpoint(failpoint);
        let _ = repo.append_atomic(&prepared_graph_append());
        reopen_and_assert_no_dangling_committed_refs(repo.path());
    }
}
```

Add exact retry before sequence, divergent retry, two-writer contention, partial/truncated batch, checksum, 1 MiB event, 16 MiB batch, 64 MiB journal, ordered reads, unsupported old-format bytes, orphan cleanup, temp/symlink attack, and no-plaintext byte scan.

Also add strict schema/Serde round-trip tests for all 16 variants and compile-time/API tests proving no writer can pass raw `GraphVersionRecord` or Foundation `Diagnostic` into the new envelope.

- [ ] **Step 2: Run RED**

```powershell
cargo +1.97.1 test -p graphhelm-events --test local_atomicity --locked
cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
```

Expected: FAIL because the old JSONL format cannot append `PreparedAppend` or manage encrypted blobs.

- [ ] **Step 3: Implement canonical event chains and repository format**

Replace `core/protocols/src/event.rs` with the 16 variants fixed in Public Interfaces. Canonical request digest includes scope, stream, expected sequence, safe events, Evidence metadata/digests, and artifact registrations. Event hash includes previous hash and canonical envelope without `eventHash`. Validate strict schema before serialization and again on read.

- [ ] **Step 4: Implement local crash-consistent publication**

Under one cross-platform repository lock: validate all inputs; stage encrypted blobs; fsync; atomic no-replace publish; append one checksummed JSONL physical batch; fsync journal; publish active marker. Reconciler removes only blobs not reachable from a committed batch and never guesses an unknown format.

- [ ] **Step 5: Update CLI flows without compatibility reader**

Update Governor, simulation, replay projection, and CLI as one workspace migration. `graph simulate`, draft application, and replay create/use the new repository directory. Unsupported earlier bytes return `GHE007_UNSUPPORTED_FORMAT`; no `events import` or repository-format migration flag exists. The unrelated `schema migrate` command remains.

- [ ] **Step 6: Run GREEN gates**

```powershell
cargo +1.97.1 test -p graphhelm-events --locked
cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
cargo +1.97.1 clippy -p graphhelm-events -p graphhelm-cli --all-targets --locked -- -D warnings
rg -n "LegacyEvent|legacy_import|legacy-import|import-events" core/events apps/cli
```

- [ ] **Step 7: Commit**

```powershell
git add core/events apps/cli Cargo.lock
git commit -m "feat(events): replace local journal with safe format" -m "Refs #5" -m "Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 7: Implement PostgreSQL journal, Evidence, RLS, and integrity

**Files:**
- Modify: `Cargo.toml`, `Cargo.lock`
- Create: `adapters/postgres-event-store/Cargo.toml`
- Create: `adapters/postgres-event-store/migrations/0001_event_evidence.sql`
- Create: `adapters/postgres-event-store/src/{lib,scope,rows,journal,evidence,artifact,integrity,error}.rs`
- Create: `adapters/postgres-event-store/tests/{migration,repository_conformance,concurrency,isolation}.rs`
- Create: `adapters/postgres-event-store/tests/support/mod.rs`

**Interfaces:** Implements `AsyncEventRepository`, `EvidenceRepository`, and `ArtifactCatalog` with atomic `PreparedAppend`, composite scoped keys, forced RLS, idempotency, chain verification, cursors, and authenticated checkpoints.

- [ ] **Step 1: Write ignored RED integration tests**

```rust
#[tokio::test]
#[ignore = "requires GRAPHHELM_TEST_ADMIN_URL"]
async fn append_is_atomic_across_event_evidence_and_refs() {
    let db = TestDatabase::new().await;
    db.inject_failure(Failpoint::AfterEvidenceBeforeEvent);
    assert!(db.repo.append_atomic(prepared_graph_append()).await.is_err());
    assert_eq!(db.count_all_rows().await, 0);
}
```

Add same-stream concurrency, cross-scope ID collisions, RLS direct SQL, pool scope leakage, exact/divergent retry, foreign/missing ref, rollback, cursor tamper/scope mismatch, chain corruption, and 100,000-event range bound.

- [ ] **Step 2: Run RED against isolated PostgreSQL**

```powershell
$env:GRAPHHELM_TEST_ADMIN_URL = 'postgresql://postgres:postgres@127.0.0.1:5432/postgres'
cargo +1.97.1 test -p graphhelm-postgres-event-store --test migration --locked -- --ignored --test-threads=1
```

Expected: FAIL because adapter/migration do not exist.

- [ ] **Step 3: Implement migration and role/RLS invariants**

Create scope, streams, idempotency, events, Evidence, artifact, reference, and checkpoint tables. Every scoped FK/unique key includes workspace/project and optional execution identity. Enable and force RLS. Migration self-check fails if runtime role owns a table or has `BYPASSRLS`.

- [ ] **Step 4: Implement scoped transactions and atomic append**

Begin transaction, `set_config` scope locally, resolve idempotency before sequence, lock stream row, validate all same-scope refs/digests, insert encrypted Evidence/artifacts/events/refs, update head, commit. Map SQL errors to stable redacted diagnostics.

- [ ] **Step 5: Implement reads, cursors, integrity, and checkpoints**

Read ordered bounded pages and verify contiguity/hash before returning. Cursor binds scope, stream, sequence, head hash, format version, and checksum. Checkpoint canonical bytes are authenticated by `KeyProvider`; database stores tag/key version, not KEK.

- [ ] **Step 6: Run all database gates**

```powershell
cargo +1.97.1 test -p graphhelm-postgres-event-store --all-features --locked -- --ignored --test-threads=1
cargo +1.97.1 clippy -p graphhelm-postgres-event-store --all-targets --all-features --locked -- -D warnings
```

- [ ] **Step 7: Commit**

```powershell
git add Cargo.toml Cargo.lock adapters/postgres-event-store
git commit -m "feat(events): add scoped PostgreSQL repository" -m "Refs #5" -m "Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 8: Implement legal holds and cryptographic erasure

**Files:**
- Create: `core/events/src/retention.rs`, `core/events/tests/retention.rs`
- Modify: `core/events/src/lib.rs`
- Create: `adapters/postgres-event-store/migrations/0002_retention.sql`
- Create: `adapters/postgres-event-store/src/retention.rs`
- Modify: `adapters/postgres-event-store/src/lib.rs`
- Create: `adapters/postgres-event-store/tests/retention.rs`

**Interfaces:** Produces `RetentionPolicy`, `RetentionAuthority`, `RetentionRequest`, ordered dry-run plans, legal holds, prepared/finalized receipts, `RetentionService`, `RetentionRepository`, reconciliation, and cleanup eligibility.

- [ ] **Step 1: Write RED deterministic state-machine tests**

```rust
#[tokio::test]
async fn erased_required_slot_blocks_materialization_without_rewriting_history() {
    let services = fixed_retention_services();
    services.execute(request_for_required_slot()).await.unwrap();
    assert_eq!(services.evidence_read(), EvidenceRead::Unavailable(Erased));
    assert_eq!(services.journal_event_count(), 2);
    assert_eq!(services.materialize().unwrap_err().code(), "GHE008_CONTENT_UNAVAILABLE");
}
```

Add dry-run ordering, policy/version mismatch, authority, 10,001 targets, exact/divergent retry, hold-before-prepare, hold-after-pending rejection, provider failure, crash after prepare/revoke, finalize retry, ciphertext cleanup ordering, and no plaintext in receipts/tombstones.

- [ ] **Step 2: Run RED**

```powershell
cargo +1.97.1 test -p graphhelm-events --test retention --locked
```

Expected: FAIL because retention service/contracts are absent.

- [ ] **Step 3: Implement pure policy and saga**

Prepare transaction verifies scope/authority/idempotency/holds, appends requested receipt, and marks `erasure_pending`. Provider revoke uses operation idempotency. Finalize verifies authenticated receipt/epoch, appends completed receipt/tombstone, marks `erased`, and enables later cleanup. Failure remains retryable without restoring readability.

- [ ] **Step 4: Implement PostgreSQL state and failpoint tests**

Migration adds policies, operations, targets, holds, tombstones, and cleanup receipts with composite scope FKs and forced RLS. Tests recreate service instances at every crash point and assert one provider revoke and one completion event.

- [ ] **Step 5: Run GREEN gates**

```powershell
cargo +1.97.1 test -p graphhelm-events --test retention --locked
cargo +1.97.1 test -p graphhelm-postgres-event-store --test retention --locked -- --ignored --test-threads=1
```

- [ ] **Step 6: Commit**

```powershell
git add core/events adapters/postgres-event-store
git commit -m "feat(events): add retention and cryptographic erasure" -m "Refs #5" -m "Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 9: Add projections and executable materialization

**Files:**
- Modify: `core/events/src/{lib,projection}.rs`
- Create: `core/events/tests/projection_rebuild.rs`
- Create: `core/governor/src/materialize.rs`, `core/governor/tests/materialization.rs`
- Create: `adapters/postgres-event-store/migrations/0003_projections.sql`
- Create: `adapters/postgres-event-store/src/projection.rs`
- Modify: `adapters/postgres-event-store/src/lib.rs`
- Create: `adapters/postgres-event-store/tests/projection.rs`

**Interfaces:** Produces generation-based disposable projections, watermarks, handlers for all 16 events, content availability state, rebuild/resume/swap, and `ExecutableGraphMaterializer` that fails closed when required Evidence is unavailable.

- [ ] **Step 1: Write RED replay/materialization tests**

```rust
#[test]
fn replay_never_needs_plaintext_but_execution_does() {
    let projection = rebuild(events_with_erased_required_slot()).unwrap();
    assert_eq!(projection.graphs[&version_id()].topology_hash, expected_topology_hash());
    assert_eq!(projection.content_slots[&slot_id()].availability, EvidenceAvailability::Erased);
    assert_eq!(materialize(&projection, &empty_evidence()).unwrap_err().code(), "GHE008_CONTENT_UNAVAILABLE");
}
```

Add every event kind, empty stream, page boundaries, interrupted resume, source-head advance, corrupted event, old projection format, generation swap failure, optional unavailable slot, digest mismatch, and successful decrypt/materialize tests.

- [ ] **Step 2: Run RED**

```powershell
cargo +1.97.1 test -p graphhelm-events --test projection_rebuild --locked
cargo +1.97.1 test -p graphhelm-governor --test materialization --locked
```

- [ ] **Step 3: Implement pure handlers and watermarks**

Handlers consume safe event fields only. Watermark binds scope, stream, last sequence/hash, projection name/version, and generation. Rebuild writes a fresh generation, catches up to an observed head, verifies unchanged head before swap, and leaves old generation active on failure.

- [ ] **Step 4: Implement materializer**

Resolve every required slot by same scope/evidence ID/digest, decrypt through `KeyProvider`, reconstruct ephemeral authoring/executable fields in memory, and zeroize buffers after use. Optional unavailable slots remain typed unavailable. No fallback/cache can supply erased content.

- [ ] **Step 5: Implement PostgreSQL generation storage and run GREEN**

```powershell
cargo +1.97.1 test -p graphhelm-events --test projection_rebuild --locked
cargo +1.97.1 test -p graphhelm-governor --test materialization --locked
cargo +1.97.1 test -p graphhelm-postgres-event-store --test projection --locked -- --ignored --test-threads=1
```

- [ ] **Step 6: Commit**

```powershell
git add core/events core/governor adapters/postgres-event-store
git commit -m "feat(events): add safe replay projections" -m "Refs #5" -m "Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 10: Stream encrypted backup and verify restore

**Files:**
- Create: `adapters/postgres-event-store/src/backup.rs`
- Modify: `adapters/postgres-event-store/src/lib.rs`
- Create: `adapters/postgres-event-store/tests/backup_restore.rs`

**Interfaces:** Produces encrypted streaming backup/restore with authenticated manifest, fresh-database enforcement, pinned tool discovery, bounded stderr, cleanup, and semantic post-restore verification.

- [ ] **Step 1: Write RED backup tests**

```rust
#[tokio::test]
#[ignore = "requires PostgreSQL client tools"]
async fn restored_database_preserves_events_evidence_and_unavailability() {
    let backup = backup_fixture_database().await.unwrap();
    let restored = restore_into_fresh_database(backup).await.unwrap();
    assert_eq!(restored.verify_all().await.unwrap(), source_manifest());
    assert_eq!(restored.erased_slot_state().await.unwrap(), EvidenceAvailability::Erased);
}
```

Add wrong key/tag, tampered/reordered/truncated chunks, 64 GiB limit, non-empty target, version mismatch, timeout, 64 KiB stderr truncation/redaction, temp cleanup, failed restore cleanup, and scope/projection/keyring metadata checks.

- [ ] **Step 2: Run RED**

```powershell
cargo +1.97.1 test -p graphhelm-postgres-event-store --test backup_restore --locked -- --ignored --test-threads=1
```

- [ ] **Step 3: Implement encrypted stream and manifest**

Stream custom-format `pg_dump` stdout through 1 MiB AEAD chunks. Authenticate header and final manifest containing format/schema versions, chunk count/order, plaintext/ciphertext sizes, database identity, dump/restore versions, migration hashes, release catalog digest, stream heads/checkpoints, Evidence/tombstone/projection counts, and sealed-key metadata digest. No command contains a password.

- [ ] **Step 4: Implement fresh restore and semantic verification**

Require empty target, run `pg_restore --exit-on-error --single-transaction`, apply migrations as required by manifest, and verify RLS policies, stream chains/checkpoints, references, Evidence ciphertext digests, tombstones, projections/watermarks, and key metadata before success.

- [ ] **Step 5: Run GREEN and commit**

```powershell
cargo +1.97.1 test -p graphhelm-postgres-event-store --test backup_restore --locked -- --ignored --test-threads=1
git add adapters/postgres-event-store
git commit -m "feat(events): add verified encrypted backup restore" -m "Refs #5" -m "Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 11: Add operator CLI, CI, documentation, and final gate

**Files:**
- Modify: `apps/cli/Cargo.toml`, `apps/cli/src/{args,commands/mod}.rs`
- Create: `apps/cli/src/commands/events/{mod,config,verify,rebuild,backup,restore}.rs`
- Create: `apps/cli/tests/event_store_cli.rs`
- Modify: `.github/workflows/ci.yml`
- Create: `ci/postgres.ps1`
- Create: `docs/milestones/production-event-evidence-store.md`
- Modify: `docs/operations/OBSERVABILITY_AND_RECOVERY.md`, `README.md`, `CHANGELOG.md`

**Interfaces:** Produces bounded JSON-only operator commands, ephemeral PostgreSQL CI on Windows/Linux, complete milestone documentation, rollback instructions, and the final evidence package. No event import or repository-format migration command exists; generic schema evolution commands remain.

- [ ] **Step 1: Write RED CLI tests**

```rust
#[test]
fn unsupported_repository_format_fails_without_import_fallback() {
    graphhelm().args(["events", "verify", "--repository", unsupported_format_path()])
        .assert()
        .code(2)
        .stdout(predicate::str::contains("GHE007_UNSUPPORTED_FORMAT"))
        .stdout(predicate::str::contains("legacy").not());
}
```

`unsupported_format_path()` creates a temporary repository containing only `{"formatVersion":0}` during the test; no superseded-format fixture is checked in.

Add config permission/symlink, verify range, rebuild generation, backup/restore target, no plaintext/path/backtrace, bounded output, mutually exclusive flags, signal timeout, and no legacy command tests.

- [ ] **Step 2: Run RED**

```powershell
cargo +1.97.1 test -p graphhelm-cli --test event_store_cli --locked
```

- [ ] **Step 3: Implement commands and safe config**

Commands are `events verify`, `events rebuild`, `events backup`, and `events restore`. Config loads bounded JSON from explicit path or environment, rejects symlinks/insecure permissions where supported, and redacts DSNs/paths. Subprocess handling uses argument arrays, no shell, bounded stderr, timeout, kill/wait, and cleanup.

- [ ] **Step 4: Add isolated PostgreSQL CI**

Windows and Ubuntu jobs discover the runner's installed PostgreSQL, create a random cluster/database/roles on a random port, set `GRAPHHELM_TEST_ADMIN_URL`, run ignored tests serially, and always stop/remove the cluster. No Docker or service container is used.

- [ ] **Step 5: Document the pre-release reset and operations**

Document architecture, trust boundaries, local/PostgreSQL formats, externalization, hash semantics, erasure/materialization behavior, RLS roles, key rotation, retention, projection rebuild, backup/restore, limits, diagnostics, developer-data reset, and why no compatibility layer exists. Rollback is code rollback plus restoration from a matching authenticated backup; it never downgrades repository bytes in place.

- [ ] **Step 6: Run the complete clean-state gate**

```powershell
cargo +1.97.1 fmt --all -- --check
cargo +1.97.1 clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo +1.97.1 test --workspace --all-features --locked
cargo +1.97.1 test -p graphhelm-cli --test cli_smoke --locked
cargo +1.97.1 test -p graphhelm-cli --test schema_cli --locked
cargo +1.97.1 test -p graphhelm-cli --test event_store_cli --locked
cargo +1.97.1 run --locked -p graphhelm-cli -- schema catalog
cargo +1.97.1 run --locked -p graphhelm-cli -- schema conformance
cargo +1.97.1 metadata --locked --no-deps --format-version 1
git diff --check
```

Then run all PostgreSQL ignored tests with `--test-threads=1`, inspect the full branch diff, scan runtime/source/conformance for removed legacy symbols, scan journal fixtures for official authored plaintext, and verify the worktree is clean after the final commit.

- [ ] **Step 7: Commit**

```powershell
git add apps/cli .github/workflows/ci.yml ci docs README.md CHANGELOG.md
git commit -m "feat(cli): complete safe persistence milestone" -m "Closes #5" -m "Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

## Acceptance Traceability

| Requirement | Automated proof |
|---|---|
| Raw authoring content never enters journal | Task 5 official-example byte scans; Tasks 6/7 adapter scans |
| Governor is the only graph-publication path | Task 5 preparation API; Task 6 event-constructor and workspace migration tests |
| Safe topology and slot/reference bijection | Tasks 2, 3, 5 schema/property tests |
| Deterministic hashes across re-encryption | Task 5 hash property tests |
| JSONL and PostgreSQL share one wire contract | Tasks 6, 7, 11 repository conformance |
| No legacy runtime compatibility | Tasks 1, 2, 3, 6, 11 scans and CLI rejection |
| Single initial schema baseline | Task 2 exact 15-schema/50-case/52-resource inventory |
| Append atomicity and retry order | Tasks 6 and 7 failpoint/concurrency tests |
| Scope/RLS isolation | Task 7 direct SQL and pool leakage tests |
| Evidence confidentiality/integrity | Task 4 crypto/trait/tamper tests |
| Erasure remains auditable | Task 8 saga/crash/tombstone tests |
| Replay survives unavailable Evidence | Task 9 rebuild tests |
| Erased required content blocks execution | Tasks 8 and 9 materialization tests |
| Backup/restore is encrypted and verified | Task 10 fresh-database tests |
| Windows/Linux, offline, no Docker | Task 11 CI and clean-state gate |

## Required Final Review

After Task 11, dispatch independent whole-branch reviews for specification compliance, correctness, security/secret leakage, cross-platform behavior, PostgreSQL/RLS, cryptographic use, crash consistency, schema/release integrity, and legacy-removal completeness. Critical/Important findings require focused RED-to-GREEN correction and a fresh re-review. Minor findings are recorded in the SDD ledger and adjudicated before merge.
