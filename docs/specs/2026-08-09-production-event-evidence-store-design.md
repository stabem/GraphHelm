# Production Event and Evidence Store Design

**Status:** Historical baseline; persistence projection and pre-release compatibility portions superseded by D-036/D-037 and ADR-022/ADR-023
**Milestone:** 03
**Original tracking issue:** #5 in the private development archive.
**Repository base:** `8ee8f49ca36bd1b292eda841c84cf0da18c9c4ce`
**Normative documentation baseline:** `72c376499e4fc92f7a1097432c703d73c1b2f6b0`
**Normative decisions:** [D-035 through D-037](../DECISION_REGISTER.md), [ADR-021 through ADR-023](../reference/REFERENCE_STACK_AND_ADRS.md)
**Focused threat model:** [Event/Evidence Store threat model](../security/EVENT_EVIDENCE_STORE_THREAT_MODEL.md)

> **Supersession notice:** This document preserves the original Milestone 03 rationale, threat controls, PostgreSQL, retention, projection, backup, and restore design. ADR-022 supersedes every statement here that permits raw `GraphVersionRecord`/Foundation payload persistence, direct reuse of Foundation `Diagnostic` or the internal waiver shape, `LocalExecutionContext::foundation`, legacy envelopes/import/context/commands/fixtures, dual-format runtime behavior, immutable `1.1.0`, or migration compatibility with those pre-release formats. ADR-023 additionally supersedes the former eight-kind content-position list with the exact eleven-kind pre-release baseline required by D-037. Those passages are historical and non-normative; they do not authorize an implementation. The accepted [safe persistence projection design](2026-08-09-safe-persistence-projection-design.md) governs authoring externalization, `PersistedGraphVersion`, content slots, safe diagnostics, the bounded waiver, the single rebuilt `1.0.0` baseline, and immediate JSONL/PostgreSQL adoption.

## 1. Purpose

This milestone turns the Foundation Graph Kernel's local event log into a production-grade persistence boundary for the later durable Graph Engine and Runtime. It delivers an adapter-neutral repository contract, a transactional PostgreSQL adapter, encrypted and erasable evidence, immutable artifact references, rebuildable projections, explicit legacy import, and encrypted database backup with tested restore. The explicit legacy-import portion of this historical summary is superseded by ADR-022; no legacy compatibility layer is implemented.

The design resolves the tension between append-only replay and authorized erasure by separating the replay-safe Event Journal from sensitive Evidence storage. Events retain the minimum deterministic state required to rebuild projections. Prompts, outputs, logs, and other sensitive or large content are referenced rather than embedded and can become unavailable without falsifying history.

## 2. Dependency and design gate

Milestone 02 is complete and merged. Its immutable `schemas/releases/1.0.0` package remains the compatibility baseline.

Before production code begins, the first delivery wave must record the decisions in this specification as an accepted Event/Evidence Store ADR and a repository-grounded threat model. No schema, Rust, SQL, CLI, or migration implementation may begin until that documentation gate is accepted.

## 3. Normative decisions

The following decisions are approved for this milestone:

1. **Replay-safe history and erasable evidence are separate.** The immutable event envelope and deterministic projection payload remain. Sensitive evidence may be cryptographically erased and later physically expired.
2. **Erasure is itself auditable history.** Authorized erasure appends a receipt event and leaves a minimal tombstone. Reads return an explicit unavailable result; they never fabricate content or success.
3. **Keys are externally mediated.** Core repositories depend on a `KeyProvider` interface. PostgreSQL stores ciphertext and wrapped data-encryption keys, never the key-encryption key.
4. **Local and production repositories have distinct execution models.** The complete JSONL `EventStore` remains synchronous for offline CLI use. PostgreSQL implements a new object-safe asynchronous repository. Both use shared domain types, errors, and conformance behavior.
5. **Legacy records never acquire guessed context.** The old Foundation envelope was proposed for acceptance only by `LegacyEventImporter` with an explicit `LegacyImportContext`. **Superseded by ADR-022:** neither the importer nor the legacy envelope is implemented.
6. **Tamper evidence uses chained events and authenticated anchors.** Streams form hash chains. Checkpoints and backup manifests are authenticated through `KeyProvider`. Public per-event signing remains a later compatibility decision.
7. **Scope is enforced at three layers.** Typed scope, composite relational constraints, and PostgreSQL row-level security all apply. Runtime and administrative database roles are separate.
8. **Events contain only replay-critical data.** Prompts, outputs, logs, raw tool results, and sensitive blobs become encrypted evidence or artifact references. Replay remains complete when referenced evidence is unavailable.
9. **PostgreSQL acceptance is real and isolated.** Concurrency, isolation, migration, backup, and restore tests run against an ephemeral PostgreSQL instance without Docker, production credentials, or production infrastructure.
10. **Retention requires explicit authority.** Every expiry or erasure operation carries a versioned policy, typed scope, authority, reason, idempotency key, and dry-run result. A legal hold blocks key destruction and physical deletion.

## 4. Scope

The milestone delivers:

- an accepted ADR and threat model;
- versioned event, evidence, artifact-reference, repository-scope, and sensitivity schemas;
- strict production event envelopes and append requests;
- adapter-neutral repository, key-provider, retention, integrity, and projection contracts;
- preservation of the complete local JSONL adapter and offline CLI behavior;
- transactional ordered and idempotent PostgreSQL append;
- typed workspace/project/execution isolation plus PostgreSQL RLS;
- per-stream hash chains and authenticated integrity checkpoints;
- encrypted evidence with wrapped per-record keys;
- legal holds, retention evaluation, tombstones, cryptographic erasure, and physical-expiry eligibility;
- immutable content-addressed artifact metadata and opaque references, not artifact bytes;
- disposable projections with watermarked full and incremental rebuild;
- atomic import of Foundation JSONL records with explicit context;
- encrypted PostgreSQL dump, authenticated manifest, and tested restore;
- operator CLI commands needed to verify, import, rebuild, back up, and restore;
- Windows and Linux gates plus isolated PostgreSQL acceptance on supported CI hosts.

## 5. Non-goals

This milestone does not implement:

- Runtime HTTP/gRPC APIs, streaming, mTLS, authentication, or product RBAC;
- scheduler jobs, leases, multi-node replication, or an external message bus;
- durable Graph Engine recovery or execution re-scheduling;
- artifact bytes, filesystem/S3 artifact adapters, or artifact garbage collection;
- full project export, re-execution, or branch replay;
- Credential Broker, Vault, KMS, or cloud-key adapters;
- Studio, Knowledge Graph, Living Docs, Dreams, model, tool, or sandbox integrations;
- hosted services, external telemetry, or public release signing;
- multiuser authorization UI or billing.

## 6. Architecture

The Event Journal and Evidence Store are separate persistence concepts joined by typed references.

```text
core/protocols
  |-- production event/evidence/artifact/scope wire types
  `-- stable diagnostics and sensitivity types

core/events
  |-- synchronous EventStore and complete JSONL adapter
  |-- object-safe AsyncEventRepository contract
  |-- EvidenceRepository, ArtifactCatalog, KeyProvider contracts
  |-- retention, integrity, import, and projection domain services
  `-- shared repository conformance suite

adapters/postgres-event-store
  |-- PostgreSQL repository and RLS-aware transactions
  |-- forward-only embedded SQL migrations
  `-- database dump/restore orchestration

adapters/sealed-key-provider
  `-- local sealed keyring and durable revocation journal

apps/cli
  `-- bounded filesystem/process adapters and JSON presentation
```

Core crates depend only on interfaces. The PostgreSQL crate may depend on `core/events` and `core/protocols`; neither core crate may depend on the adapter. The adapter uses SQLx with runtime queries and Rustls. It does not require a live database during ordinary compilation.

The asynchronous repository remains object-safe: public trait methods return `Send` futures without generic methods. The later Runtime can hold the repository behind an `Arc<dyn AsyncEventRepository>`.

## 7. Domain contracts

### 7.1 Repository scope

Every production operation carries a `RepositoryScope`:

- `workspaceId` is mandatory;
- `projectId` is mandatory and belongs to the workspace;
- `executionId` is mandatory for execution events and explicitly absent only for registered project-level event kinds;
- identifiers are bounded opaque values, not paths, URLs, SQL fragments, or display names.

Scope equality is exact and case-sensitive. No repository API accepts unscoped reads or wildcard scope. Administrative backup and migration operations use a distinct capability and database role rather than weakening the scoped repository contract.

### 7.2 Sensitivity

`Sensitivity` is a closed, versioned enum:

- `public`;
- `internal`;
- `confidential`;
- `restricted`.

The replay-safe event payload may contain only fields allowed by its registered event schema. Free-form prompt, output, log, credential, authorization header, environment, or raw tool-result fields are forbidden regardless of sensitivity label. Deterministic secret-pattern scanning is defense in depth, not permission to store unknown content.

### 7.3 Production event envelope

The strict envelope contains:

- `schemaVersion`;
- `eventId`;
- `scope`;
- `streamId` and `sequence`;
- `occurredAt` from an injected clock;
- `actor`;
- `idempotencyKey`;
- `sensitivity`;
- registered event `kind` and replay-safe `data`;
- ordered `evidenceRefs` and `artifactRefs`;
- `previousHash` and `eventHash`.

Canonical JSON bytes define hashes. Object keys are recursively sorted, arrays preserve semantic order, and the `eventHash` field itself is excluded while hashing. The first event uses the documented genesis hash. Existing provisional `p50.dev` identifiers remain unchanged.

An `AppendRequest` contains caller-supplied scope, stream, expected next sequence, and one bounded batch of new events. Store-assigned fields are never accepted from an untrusted caller.

### 7.4 Evidence

An evidence record contains an opaque `evidenceId`, scope, media type, sensitivity, ciphertext algorithm/version, ciphertext digest, nonce, wrapped-key metadata, bounded size, creation metadata, retention classification, and availability state.

The database never stores evidence plaintext or a public plaintext digest. An event references evidence by opaque ID and ciphertext digest. `EvidenceUnavailable` is a typed read outcome with a stable reason such as erased, expired, missing-key, or integrity-failed; it is not an empty payload.

### 7.5 Artifact references

An artifact reference contains an opaque locator, content digest, media type, byte length, sensitivity, and immutable metadata version. The scoped catalog binds it relationally to its producer event during atomic append. Neither representation contains artifact bytes, absolute filesystem paths, presigned URLs, access tokens, or credentials.

Artifact registration is content-addressed and immutable. A repeated identical registration is idempotent; a locator or digest collision with different metadata fails closed. Cross-scope resolution returns the same not-found-shaped public result as a missing reference.

### 7.6 Actor and authority

Event actors remain the typed protocol actors. Retention adds a separate trusted `RetentionAuthority` capability so an event actor cannot grant itself deletion rights. The repository validates request structure, scope, policy version, legal hold, and idempotency. User authentication and product RBAC remain the responsibility of the future Runtime boundary.

## 8. Repository contracts

### 8.1 Synchronous local store

The existing synchronous `EventStore` remains the local/offline boundary and retains exclusive file locking, ordered sequences, idempotency, flush, durable sync, bounded reads, and corrupt-record rejection.

New local writes use the strict envelope. Existing Foundation JSONL bytes are legacy input and are not silently deserialized as production events. To preserve the canonical Foundation CLI command shape, the CLI constructs `LocalExecutionContext::foundation` explicitly: its documented workspace is `local-cli`, its project identity is `graph-` plus the graph semantic-hash hex, its execution ID is the graph's validated `metadata.executionId`, its actor is `system`, and its sensitivity is `internal`. This convenience constructor is valid only for new local CLI events; legacy file import still requires caller-provided context.

### 8.2 Asynchronous production repository

`AsyncEventRepository` provides bounded operations for:

- atomic batch append;
- exact scoped stream reads with pagination and continuation cursor;
- stream-head lookup;
- integrity verification over an explicit bounded range;
- authenticated checkpoint creation and lookup.

There is no unbounded `read_all` operation. Cursors bind scope, stream, last sequence, and repository format version; a cursor cannot be reused in another scope.

### 8.3 Evidence repository

`EvidenceRepository` provides:

- encrypted put tied to an append transaction;
- authorized scoped get;
- availability lookup without payload exposure;
- legal-hold placement and release;
- retention dry run;
- idempotent cryptographic erasure;
- physical-expiry eligibility query.

Repository consumers receive plaintext only in a bounded zeroizing buffer. Normal errors, debug output, tracing, metrics, fixtures, and manifests never include evidence bytes.

### 8.4 Artifact catalog

`ArtifactCatalog` registers and resolves immutable metadata. Registration can participate in the event append transaction. Resolution is always scoped and never dereferences the opaque locator.

### 8.5 Key provider

`KeyProvider` supports:

- generating a per-evidence data-encryption key;
- wrapping and unwrapping that key under a named key-encryption key;
- durably and idempotently revoking a wrapped-key handle;
- authenticating and verifying checkpoint or backup-manifest bytes;
- returning non-secret key version metadata.

The first local implementation uses an explicitly configured sealed keyring or secret mount with restricted permissions. It contains the KEK and a durable append-only revocation journal with a monotonic epoch. `unwrap` must reject a revoked handle even when an older database backup still contains its wrapped DEK. The KEK and revocation contents never enter PostgreSQL, repository JSON, logs, fixtures, command arguments, or backup manifests. The future Credential Broker implements the same interface.

Evidence uses authenticated encryption with a fresh random nonce and binds scope, evidence ID, schema version, media type, and sensitivity as additional authenticated data. Algorithm and key versions are explicit so rotation is possible without guessing.

## 9. PostgreSQL model and transactions

The adapter uses responsibility-focused tables for workspaces/projects, stream heads, events, evidence, evidence tombstones, legal holds, artifact references, integrity checkpoints, retention operations, and projection watermarks. Projection tables are separate from canonical history.

Every scoped table carries `workspace_id` and `project_id`. Composite primary/foreign keys prevent references from crossing scope. Key constraints include:

- unique `(workspace_id, project_id, stream_id, sequence)`;
- unique `(workspace_id, project_id, stream_id, idempotency_key)`;
- composite evidence and artifact references bound to the event's scope;
- one immutable event hash per scoped sequence;
- one current stream head consistent with its last committed event;
- unique retention-operation idempotency within scope.

### 9.1 Atomic append algorithm

One database transaction performs:

1. set the typed workspace and project through `SET LOCAL`;
2. resolve all requested idempotency keys before sequence comparison;
3. return the original envelopes when the complete request is an exact retry;
4. reject a reused idempotency key whose canonical request digest differs;
5. lock the stream-head row using `SELECT ... FOR UPDATE`, creating it safely for a new stream;
6. compare the expected next sequence;
7. validate and encrypt evidence, then insert evidence and artifact metadata;
8. assign event IDs, times, sequences, previous hashes, and event hashes;
9. insert the whole batch and update the head;
10. commit only after every constraint and integrity check succeeds.

A concurrent append using the same expected sequence yields exactly one commit and one sequence conflict. A batch is all-or-nothing and cannot create gaps. Cancellation, serialization failure, process termination before commit, or any injected stage failure leaves no visible partial event, evidence, artifact reference, or stream-head change.

### 9.2 Isolation and roles

PostgreSQL RLS is enabled and forced on every scoped canonical and projection table. Runtime connections use a role without `BYPASSRLS`, table ownership, schema mutation, or administrative backup permission. Every pooled transaction sets scope with `SET LOCAL`; no session-level scope state is permitted.

A separate migration/backup role owns schema changes and dump/restore operations. Tests prove that omitted scope, changed scope, pooled-connection reuse, retry lookup, evidence lookup, artifact resolution, and projection reads cannot reveal whether foreign data exists.

RLS is defense in depth against repository/query defects; it does not authenticate a caller that is itself allowed to choose arbitrary session scope. Until Runtime authentication and RBAC exist, `AsyncEventRepository` is a trusted-caller boundary and its caller is responsible for supplying only an authorized scope.

## 10. Integrity model

Each stream is a SHA-256 hash chain over canonical envelope bytes and the previous event hash. Reads verify ordering, contiguity, stored hashes, and the requested bounded range's connection to a trusted stream head or checkpoint.

Checkpoints contain scope, stream, sequence, event hash, repository format version, creation metadata, and a `KeyProvider` authentication tag. Checkpoints are append-only. A superseding checkpoint never mutates an earlier one.

This design detects accidental corruption and database-only tampering when the authentication key remains trustworthy. It does not claim to defeat an attacker who controls both PostgreSQL and the KEK or `KeyProvider`. Public signatures, transparency logs, and external release signing remain future work.

## 11. Retention, legal holds, and erasure

A versioned `RetentionPolicy` maps an evidence classification to a minimum retention interval and physical-expiry rule. Policy evaluation is deterministic and uses an injected clock.

Every requested retention action includes scope, evidence IDs or a bounded selector, policy ID and version, authority, reason, idempotency key, requested time, and `dryRun` mode. The service first produces an ordered dry-run plan containing only identifiers, classifications, eligibility, and blocked reasons.

Because `KeyProvider` is outside PostgreSQL, erasure is a crash-consistent state machine rather than a false distributed transaction:

1. a **prepare transaction** confirms authority and scope, resolves idempotency, locks evidence and legal-hold rows, revalidates eligibility, appends an erasure-requested receipt, and changes the evidence state to `erasure_pending`;
2. reads of `erasure_pending` evidence immediately return `EvidenceUnavailable`;
3. after the prepare commit, the service asks `KeyProvider` to revoke each opaque key handle using the retention-operation idempotency key;
4. a **finalize transaction** verifies the provider's authenticated revocation receipt, appends the minimal tombstone and erasure-completed receipt, records the provider epoch, changes the state to `erased`, and marks ciphertext eligible for later cleanup;
5. a bounded reconciler or an explicit CLI retry resumes prepared operations after a crash and never repeats a completed side effect.

Legal-hold placement locks the same evidence row and is rejected once erasure has entered `erasure_pending`, because revocation may already be irreversible. A hold committed before the prepare transaction always blocks erasure. If provider revocation temporarily fails, the operation remains pending, plaintext remains inaccessible, and retry is safe.

The tombstone preserves evidence ID, scope, ciphertext digest, classification, policy/version, authority, reason code, timestamps, receipt events, provider revocation epoch, and prior availability state. It contains no plaintext, key, secret, or user payload. Repeating the exact operation returns the current or completed original operation. Reusing its idempotency key for different targets or policy fails closed.

Cryptographic erasure is the compliance boundary. Physical ciphertext deletion is a separate idempotent maintenance operation that may run only after erasure is committed and no legal hold exists. It never removes event envelopes, tombstones, receipts, or replay-critical fields.

## 12. Replay and projections

Projection handlers consume only registered replay-safe event fields. A handler may surface evidence availability but may not require evidence plaintext to reconstruct canonical state.

Each projection declares a name and format version. Its watermark binds scope, stream, last sequence, last event hash, and projection version. Rebuild behavior is:

- create a fresh projection generation;
- read bounded ordered pages from the Event Journal;
- verify the hash chain while applying pure handlers;
- persist the watermark transactionally with each page;
- resume an interrupted rebuild only when the watermark hash still matches;
- atomically mark the fresh generation active after the complete target range succeeds;
- discard the old generation later without touching canonical history.

An unavailable evidence reference produces an explicit projected availability state. It does not stop replay unless the event itself is corrupt or its registered replay-safe data is invalid.

## 13. Legacy JSONL import

> **Superseded by ADR-022:** this section records rejected pre-release migration rationale only. `LegacyEventImporter`, `LegacyImportContext`, import receipts, commands, fixtures, and runtime branches are removed; unknown formats fail generically and are never interpreted heuristically.

`LegacyEventImporter` is the only boundary that accepts the Foundation envelope lacking production scope, actor, sensitivity, schema version, and hash links.

The caller supplies a `LegacyImportContext` containing:

- target workspace and project;
- execution mapping or explicit project-level classification;
- actor;
- sensitivity;
- source format version;
- destination stream policy;
- deterministic import ID and idempotency key.

Import performs two phases. First it bounds and validates the entire file, verifies every physical JSONL batch/checksum/sequence, rejects secret-bearing or ambiguous legacy fields, maps every event, computes a source digest, and validates the complete candidate stream without writes. Second it commits the converted stream and an import receipt atomically.

No default actor, workspace, project, execution, sensitivity, or source version exists. A repeated identical import returns the original result. A changed file or context under the same import idempotency key fails closed. Source bytes and absolute paths never enter events or diagnostics.

## 14. Schemas and compatibility release

> **Superseded by ADR-022:** the `1.0.0` authoring/persistence schema baseline is rebuilt before first public release. No `1.1.0` release or predecessor migration package is published.

Add versioned schemas for:

1. `repository-scope.schema.json`;
2. `sensitivity.schema.json`;
3. `event-envelope.schema.json`;
4. `evidence-record.schema.json`;
5. `artifact-reference.schema.json`.

Each document starts at `1.0.0` and retains provisional `p50.dev` wire identifiers. The aggregate schema catalog advances compatibly from `1.0.0` to `1.1.0`; the nine existing `1.0.0` documents and their immutable release snapshot do not change. Publish a new immutable `schemas/releases/1.1.0` package containing the complete catalog snapshot required by the Milestone 02 contract.

The public conformance suite gains valid/invalid instances, bounded failure cases, redaction cases, catalog checks, and deterministic views for every new schema. Schema validation always precedes typed deserialization and remains offline.

## 15. Database migrations

SQL migrations are ordered, embedded, checksummed, and forward-only. An applied migration is never edited. Startup validates the migration history before opening the repository.

Tests apply the complete chain to an empty database and each new migration to a database at the immediately previous version. Migrations use explicit locks and transactions where PostgreSQL permits them. A migration that requires a non-transactional statement must be isolated, documented with recovery steps, and approved separately; none is expected in this milestone.

Rollback is roll-forward or restore, never down migration. Before an operator applies a production migration, the CLI requires a verified encrypted backup receipt for the exact database identity and current schema version unless an explicit future emergency procedure supersedes this policy.

## 16. Encrypted backup and restore

Backup is an encrypted PostgreSQL database dump as required by the recovery specification. The operator adapter launches an explicitly configured `pg_dump` executable directly, never through a shell. It requires version compatibility, custom format, no owner/privilege restoration, and a consistent database snapshot.

Dump bytes stream directly through authenticated chunked encryption under a fresh backup DEK wrapped by `KeyProvider`; no plaintext dump file is created. A small authenticated header carries only the algorithm/version, opaque key handle, wrapped backup DEK, nonces, and manifest length. The CLI writes to an exclusive sibling temporary file, flushes and durably syncs it, then publishes with an atomic no-replace operation. Failure removes only its owned temporary file and never overwrites an existing destination. An authenticated canonical manifest records backup format, database identity, schema migration version, repository format, creation time, ciphertext digest and size, stream-head/checkpoint summary, artifact-reference count, key version, provider revocation epoch, and required tool versions. It contains no plaintext key, connection secret, or evidence plaintext.

Restore always targets a newly created empty database. The operator adapter:

1. authenticates the manifest and complete ciphertext before trusting metadata and rejects a provider revocation epoch older than the current key-provider state;
2. unwraps the DEK and streams decrypted bytes directly to an explicitly configured `pg_restore` process without a shell or plaintext file;
3. validates migration history, RLS policy presence, schemas, stream heads, hash chains, checkpoints, evidence/artifact references, and retention receipts;
4. rebuilds every production projection from canonical events;
5. compares rebuilt watermarks and recorded manifest summaries;
6. emits a restore-verification receipt before the database may be selected by an operator.

Failed restore never modifies the source database and never marks the destination usable. Artifact bytes are not part of this milestone's database dump; the manifest preserves and verifies their immutable references so a later artifact snapshot can be reconciled.

## 17. CLI contract

> **Superseded in part by ADR-022:** the import command and `GHIMP001_LEGACY_CONTEXT_REQUIRED` diagnostic below are historical only and are not implemented. The remaining bounded JSON operator contracts continue under the safe wire format.

Add JSON-only operator commands with explicit configuration:

```text
graphhelm events verify --database <profile> --scope <file> --stream <id>
graphhelm events import-jsonl --database <profile> --context <file> --input <file>
graphhelm events rebuild-projections --database <profile> --scope <file>
graphhelm events backup --database <profile> --output <file> --key <key-id>
graphhelm events restore --backup <file> --target-database <profile>
```

Database profiles and key IDs are opaque configuration references, not connection strings or key bytes. Secrets are supplied only through the configured secret boundary. Normal standard output contains one bounded JSON document with identifiers, digests, counts, watermarks, and stable diagnostics. It never contains SQL errors, evidence payloads, connection strings, keys, absolute paths, process command lines, or backtraces.

Exit codes retain the established contract: `0` success, `2` domain rejection, and `4` bounded adapter/internal failure.

Stable diagnostic families include:

- `GHE001_SEQUENCE_CONFLICT`;
- `GHE002_CORRUPT_BATCH`;
- `GHE003_IDEMPOTENCY_CONFLICT`;
- `GHE004_SCOPE_VIOLATION`;
- `GHE005_INTEGRITY_FAILURE`;
- `GHE006_LIMIT_EXCEEDED`;
- `GHEV001_EVIDENCE_UNAVAILABLE`;
- `GHEV002_LEGAL_HOLD`;
- `GHEV003_RETENTION_INELIGIBLE`;
- `GHK001_KEY_UNAVAILABLE`;
- `GHB001_BACKUP_INVALID`;
- `GHB002_RESTORE_INVALID`;
- `GHIMP001_LEGACY_CONTEXT_REQUIRED`;
- `GHPROJ001_WATERMARK_MISMATCH`.

## 18. Security and threat assessment

Repository inputs, event payloads, evidence, artifact metadata, database rows, JSONL files, manifests, backups, cursors, and external process output are untrusted.

Required controls include:

- parameterized SQL only;
- bounded files, batches, payloads, evidence, selectors, pages, cursors, verification ranges, and process output;
- schema-first validation and allowlisted replay-safe event kinds;
- deterministic secret scanning and payload redaction in every error path;
- AEAD nonces from the operating-system CSPRNG with no nonce reuse;
- additional authenticated data binding ciphertext to exact scope and metadata;
- zeroizing plaintext and key buffers;
- no secrets in fixtures, CLI arguments, process listings, logs, traces, metrics, dumps, or manifests;
- forced RLS, least-privilege roles, `SET LOCAL`, and composite scope constraints;
- idempotency lookup before sequence checks and request-digest comparison;
- row locks and database uniqueness for sequence races;
- canonical hash chains and authenticated checkpoints;
- legal-hold enforcement before erasure and again before physical deletion;
- no shell invocation and exact executable/configuration selection for dump and restore;
- atomic publication, crash-consistent receipts, and fail-closed restore verification;
- uniform public responses for foreign and nonexistent scoped objects;
- dependency audit and independent OWASP-oriented review.

Primary threats and required proofs are:

| Threat | Required proof |
|---|---|
| Cross-project data exposure | RLS and pooled-connection adversarial tests return no foreign existence or data |
| Concurrent sequence race | Exactly one writer commits and one conflicts, without gaps |
| Idempotency collision | Exact retry returns original bytes; divergent reuse fails closed |
| Event or checkpoint tampering | Verification identifies the first corrupt sequence with a redacted diagnostic |
| Secret-bearing replay payload | Schema/classifier rejects it before persistence and CLI output does not echo it |
| Evidence/key compromise | Database alone yields only ciphertext and wrapped keys |
| Unauthorized or partial erasure | Authority, policy, legal hold, locking, pending-state, provider-revocation, and reconciliation tests fail closed |
| Backup disclosure or substitution | Ciphertext authentication and manifest binding reject alteration or mismatch |
| Resource exhaustion | Every public boundary has tested deterministic limits before expensive work |
| Crash during append/import/retention | Recovery observes either the complete transaction or no change |

The trust boundary limitations are explicit: authenticated checkpoints do not protect against an attacker who simultaneously controls PostgreSQL and the active `KeyProvider`/KEK. Restoring an obsolete keyring snapshot could also resurrect revoked handles, so key custody must preserve its monotonic revocation epoch and be backed up separately under the recovery policy; the database restore command rejects an epoch rollback it can observe.

## 19. Testing strategy

Every behavior change follows observed RED -> GREEN -> REFACTOR. Clocks, IDs, nonces, and key providers are injected in deterministic unit tests; production randomness is tested through properties and invariants rather than golden bytes.

Test layers are:

- unit tests for canonical envelopes, scopes, sensitivity, errors, retention, encryption metadata, cursor binding, and projection handlers;
- property tests for canonical hashing, idempotency, ordered diagnostics, retention determinism, and replay equivalence;
- a shared repository conformance suite exercised by JSONL and PostgreSQL where their capabilities overlap;
- PostgreSQL integration tests for transactions, RLS, pooled scope reuse, uniqueness, concurrent writers, injected failures, and migration history;
- schema catalog, compatibility, migration, and public conformance gates from Milestone 02;
- CLI acceptance for JSON-only output, exit codes, bounded input, redaction, atomic files, and no-overwrite behavior;
- legacy import tests covering whole-source validation, missing context, corruption, duplicates, secrets, atomic commit, and retry;
- retention tests covering dry run, authority, policy version, legal hold races, erasure, tombstone, physical cleanup eligibility, and repeated operations;
- projection tests covering full rebuild, incremental resume, corrupt watermark, version replacement, evidence unavailable, and generation activation;
- backup tests covering altered manifest/ciphertext, wrong key, interrupted dump/restore, tool mismatch, foreign database identity, complete restore, hash verification, and projection rebuild.

The PostgreSQL acceptance job installs or provisions a pinned ephemeral PostgreSQL service directly on the CI host, creates a random isolated database/roles, and removes only those owned resources after the run. It uses no Docker, network retrieval during tests, production credentials, or production infrastructure. Linux runs the complete database/backup/restore suite. Windows compiles all adapter paths and runs the full suite when the pinned PostgreSQL service and client tools are provisioned by the workflow; otherwise the workflow must fail rather than silently mark database acceptance successful.

## 20. Delivery waves and file ownership

> **Superseded in part by ADR-022:** waves for immutable `1.1.0` and legacy import are replaced by the rebuilt safe `1.0.0` baseline and Governor externalization/projection verification. The retained delivery concerns remain historical planning input to the revised plan.

Implementation planning decomposes the milestone into disjoint waves:

0. accepted ADR, threat model, compliance boundary, and runbook design;
1. versioned schemas and immutable `1.1.0` release;
2. shared production contracts, JSONL evolution, and conformance;
3. PostgreSQL migrations, repository, concurrency, and RLS;
4. evidence encryption, retention, tombstones, and erasure;
5. projection generations and rebuild;
6. legacy JSONL import;
7. encrypted dump/restore and recovery runbook;
8. CLI, CI, documentation, and independent final review.

The detailed implementation plan assigns exact non-overlapping files to parallel tasks. A later wave may depend on an earlier wave's committed public contract, but two concurrent tasks may not edit the same file.

## 21. Rollback and recovery

Rust and CLI behavior can be reverted before release when no production migration has run. Schema releases and applied SQL migrations are immutable and roll-forward only.

Before a production schema migration, create and verify an encrypted backup. Recovery restores into a fresh database, validates manifest authentication, migration history, RLS, canonical schemas, stream heads, hash chains, checkpoints, evidence/artifact references, retention receipts, and rebuilt projections, then allows an operator to switch the connection. Never edit committed events, tombstones, receipts, applied migrations, or an immutable schema release.

## 22. Acceptance criteria

> **Superseded in part by ADR-022:** acceptance statements requiring `1.1.0`, explicit legacy import, or raw Foundation parity are replaced by a single strict safe `1.0.0`, absence of compatibility runtime surfaces, and real Governor-to-`PersistedGraphVersion` publication tests.

The milestone is complete only when:

- the ADR and threat model are accepted before implementation code;
- the five new schemas are published in a complete immutable `1.1.0` release and pass compatibility/conformance gates;
- JSONL remains a complete durable offline adapter using the strict envelope;
- legacy Foundation records require explicit import context and import atomically;
- concurrent PostgreSQL writers using one expected sequence yield one commit and one conflict, without gaps or partial batches;
- exact retries return the original envelopes and divergent idempotency reuse fails closed;
- scoped reads, retries, evidence, artifacts, cursors, and projections expose no foreign existence or data;
- event replay and projection rebuild succeed after evidence erasure;
- legal holds block erasure and physical cleanup;
- authorized erasure is auditable, idempotent, crash-consistent, and leaves only the approved tombstone data;
- integrity verification detects altered events, chains, checkpoints, evidence, and backup material;
- restored PostgreSQL reproduces stream heads, integrity anchors, evidence state, artifact references, retention receipts, and projection watermarks;
- no secret or evidence plaintext appears in persisted event data, artifacts, fixtures, logs, diagnostics, manifests, or CLI output;
- formatting, warning-free Clippy, locked workspace tests, CLI smoke, schema checks, Windows/Linux CI, and isolated PostgreSQL acceptance pass;
- independent security and code review report no unresolved critical or important finding.
