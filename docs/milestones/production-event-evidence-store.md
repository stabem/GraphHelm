# Production Event and Evidence Store

Status: implemented on milestone branch. Toolchain: Rust 1.97.1, edition 2024. Schema baseline: `1.0.0` with 15 contracts. Repository format: `1.0.0`.

This milestone makes GraphHelm persistence production-safe. The Governor deterministically converts an authoring graph into a `PersistedGraphVersion` before publication: bounded safe topology stays inline and every free-form value becomes an ordered content slot backed by encrypted Evidence. The local JSONL repository and the PostgreSQL adapter accept only that one envelope. Replay never requires plaintext, authorized erasure remains auditable, and encrypted backup plus verified restore is the recovery path. The provisional `https://p50.dev/...` identifiers remain unchanged.

## Crate boundaries

| Crate | Responsibility |
|---|---|
| `graphhelm-protocols` | Strict persistence identities, `RepositoryScope`, sensitivity, content slots, safe diagnostics, and event payloads. |
| `graphhelm-graph` | Safe projection construction, content-slot profiles, derived slot/Evidence identities, and the two canonical hashes. |
| `graphhelm-events` | Repository interfaces, bounded limits, envelope and chain integrity, Evidence sealing, artifacts, `KeyProvider`, retention contracts, projection rebuild, and the local JSONL repository. |
| `graphhelm-governor` | Externalization, safe publication, and fail-closed executable materialization. |
| `graphhelm-sealed-key-provider` | Local sealed keyring, subkey derivation, DEK wrapping, HMAC authentication, and the authenticated revocation journal. |
| `graphhelm-postgres-event-store` | Scoped SQL journal, Evidence, artifacts, integrity checkpoints, retention saga, projection generations, and encrypted backup/restore. |
| `graphhelm-cli` | Bounded operator commands and JSON presentation only. |

Dependency direction remains inward toward protocols. `core/events` and `core/protocols` never depend on SQLx, the Tokio runtime, PostgreSQL, or process execution. Adapters do not depend on each other, and `graph`, `policy`, `governor`, and `simulation` never depend on a concrete adapter.

## Trust boundaries

The full model is [docs/security/EVENT_EVIDENCE_STORE_THREAT_MODEL.md](../security/EVENT_EVIDENCE_STORE_THREAT_MODEL.md). The boundaries this implementation enforces are:

- caller-controlled scope and payloads to typed repository contracts;
- core repository interfaces to the PostgreSQL adapter and pooled connections;
- the least-privilege runtime role to forced-RLS tables, and the administrative role to migrations and dump/restore;
- ciphertext and wrapped DEKs to the `KeyProvider` and zeroizing plaintext buffers;
- the committed append transaction to post-commit key revocation and the retention reconciler;
- the canonical journal to disposable projection generations;
- schema-valid authoring graphs to Governor-produced Evidence plus safe persisted topology;
- database state to the encrypted backup stream, its authenticated manifest, and a fresh restore target;
- operator configuration to directly spawned dump/restore processes and bounded JSON diagnostics.

Two limits are explicit. RLS is defense in depth and does not authenticate a caller-selected scope; a caller free to choose an arbitrary scope can select another tenant until the future Runtime authorizes it. Authenticated checkpoints detect database-only tampering only while the authentication key is trustworthy; simultaneous compromise of the database and the active key provider defeats them.

## Safe persistence projection

Publication is the only path that writes an operational graph version. `prepare_draft_publication` clones the base graph, applies draft operations, sets `version` and `basedOn`, revalidates the candidate against the schema, lints it, evaluates obligations, and only then externalizes. Nothing is appended or activated during preparation.

Externalization validates lineage, preflights every authoring value for bounds and durable-content safety, collects registered content, and rejects duplicate positions. A position is the tuple `(ownerKind, ownerId, fieldKind, ordinal)`. Owner kinds are `graph`, `node`, `agent`, `edge`, `policy`, and `diagnostic`. The eleven field kinds are `display_name`, `description`, `objective`, `purpose`, `instructions`, `completion_contract`, `policy_text`, `diagnostic_detail`, `context_path`, `permission_path`, and `isolation_path`. The last three are the authoring path scopes required by D-037; the persisted projection stores them as ordered `restricted` Evidence rather than copying plaintext.

Slots carry `slotId`, position, `evidenceId`, `contentSha256`, sensitivity, and `requiredForExecution`. Slot identity is derived from the position under the domain tag `graphhelm-content-slot-position-v1`; publication Evidence identity is derived from scope, version number, semantic hash, and position under `graphhelm-publication-evidence-identity-v1`. Sensitivity and execution requirement are derived from the position, not supplied: only graph and node `display_name`/`description` are `internal` and optional; every other registered position is `restricted` and required. An unregistered position is an invalid projection.

Sealing runs after the hashes are fixed, one Evidence record per slot in ascending position order, and each sealed record is revalidated against its slot. A slot/reference bijection check requires an exact ordered one-to-one match before an event can be constructed. Externalization failures are `GHE009_EXTERNALIZATION_FAILED`.

## Hash semantics

Both hashes are lowercase `sha256:` plus SHA-256 of compact canonical JSON with recursively sorted object keys.

`topologyHash` covers the serialized `PersistedTopology` plus the ordered list of content positions. `semanticHash` covers the same topology identity plus the ordered per-slot `contentSha256` digests. Evidence ID, ciphertext, nonce, wrapped key, and key handle affect neither. That independence is proved at runtime: the projection is hashed with provisional Evidence IDs, the IDs are rebound to their derived publication values, the hashes are recomputed, and a difference is an integrity failure.

`contentSha256`, `ciphertextSha256`, and `aadSha256` are bare 64-character lowercase hex without a prefix.

## Local repository format

The local repository is a directory with exactly six entries: `format.json`, `journal.jsonl`, `blobs/`, `.tmp/`, `active/`, and `repository.lock`. Any other layout is refused. `format.json` must be the exact bytes `{"formatVersion":"1.0.0"}` followed by a newline.

Each `journal.jsonl` line is one physical batch with stream ID, starting sequence, complete envelopes, and a checksum. Evidence blobs live under `blobs/{objectKey}.json`, where the object key is the SHA-256 of canonical scope bytes, a zero byte, and the Evidence ID. Active-version markers live under `active/{stream}/{sequence}.json`.

Append holds an exclusive lock and orders its effects: validate the request, stage blobs in `.tmp/`, `sync_all` each staged file and the staging directory, hardlink into `blobs/` with byte-equality on collision, sync `blobs/`, append the journal line, `sync_data` the journal and sync the root, and publish the active marker last. A crash can therefore leave unreachable ciphertext, but never a committed reference to missing Evidence. Opening the repository revalidates anchors, syncs the loaded journal, deletes orphan blobs and every staging file, and republishes the active marker from replayed `GraphVersionPublished` events.

Stream genesis is `sha256:35c8ab0717bef1684ad07efcf3bedd4648c778a2c944cbd2c7e6a4802e2237b3`, the SHA-256 of UTF-8 `graphhelm:event-chain:v1:genesis`. A corrupt committed line fails closed. The local JSONL repository also fails closed on an
interrupted non-newline tail rather than truncating it, so a crash between the batch write and its
newline leaves the repository unopenable and requires manual recovery. The sealed key provider's
revocation journal does recover such a tail, because it can distinguish an interrupted write, whose
tail cannot deserialize, from a stripped terminator, whose tail still parses as a complete record;
the JSONL repository has no equivalent discriminator today.

## PostgreSQL format

Three embedded migrations create sixteen tables. `0001_event_evidence.sql` creates `graphhelm_streams`, `graphhelm_idempotency`, `graphhelm_events`, `graphhelm_evidence`, `graphhelm_artifacts`, `graphhelm_evidence_refs`, `graphhelm_artifact_refs`, and `graphhelm_checkpoints`. `0002_retention.sql` creates `graphhelm_retention_policies`, `graphhelm_retention_operations`, `graphhelm_retention_targets`, `graphhelm_legal_holds`, `graphhelm_evidence_tombstones`, and `graphhelm_cleanup_receipts`. `0003_projections.sql` creates `graphhelm_projection_checkpoints` and `graphhelm_projection_active`.

The wire contract is identical to the local repository; only storage differs. Every scoped foreign key and unique key includes workspace and project identity plus the optional execution identity. Reads bind the physical tail to a provider-authenticated stream head that is refreshed on every append, and bind a bounded suffix to a provider-authenticated checkpoint whose canonical bytes include the key version and provider epoch. A database-only attacker therefore cannot rehash a suffix and replace the head without a fresh provider tag. Event JSON is fetched in bounded chunks behind a conservative SQL-side transport guard and each envelope is rechecked against the 1 MiB canonical contract before accumulation.

Startup verifies the migration ledger by exact checksum for versions 1, 2, and 3; an unknown, missing, failed, or altered row fails closed. Immutable history tables reject `UPDATE` and `DELETE` through triggers that raise SQLSTATE `55000`, and stream heads accept only monotonic append transitions.

## Scope isolation and database roles

Every scoped transaction sets `graphhelm.workspace_id`, `graphhelm.project_id`, and `graphhelm.execution_id` transaction-locally with a single `set_config` statement. No session-persistent scope state exists, so a pooled connection cannot leak a scope.

Every scoped table has row-level security enabled and forced, with one policy named `graphhelm_scope` whose `USING` and `WITH CHECK` predicates both compare the three columns against those settings; the absent execution identity is normalized to the empty string.

`graphhelm_configure_runtime_role(name)` is the only grant path. It refuses a role that does not exist, that has `SUPERUSER`, `BYPASSRLS`, `CREATEROLE`, `CREATEDB`, or `REPLICATION`, that holds any role membership, or that owns the database or holds `CREATE` on it. It then revokes all table privileges, database `TEMPORARY`, and `CREATE` on `public`, and grants only `USAGE` on the schema, `EXECUTE` on the migration-currency function, `SELECT, INSERT, UPDATE` on `graphhelm_streams`, `graphhelm_evidence`, `graphhelm_retention_operations`, `graphhelm_retention_targets`, and `graphhelm_projection_active`, and `SELECT, INSERT` on the remaining tables. No `DELETE`, `TRUNCATE`, `REFERENCES`, or `TRIGGER` privilege is granted anywhere.

The adapter repeats the check at startup from the runtime session itself: dangerous role attributes, ownership of any `graphhelm_%` relation, `CREATE` on the schema or database, `TRUNCATE`/`REFERENCES`/`TRIGGER` on any table, write privileges on immutable-history tables, and any role membership each fail construction. Migration, backup, and restore use a separate administrative role.

## Evidence confidentiality and keys

Each Evidence record is sealed with a fresh 256-bit DEK and a fresh 192-bit nonce from the OS CSPRNG under XChaCha20-Poly1305. The AAD is a length-prefixed concatenation of the domain tag `graphhelm-evidence-aad-v1`, workspace, project, the present-or-absent execution identity, the Evidence ID, the Evidence schema version, the media type, the sensitivity token, the retention class, and the content digest. The same AAD binds the key wrap, so metadata cannot be swapped under a valid record. Retention classes are exactly `ephemeral`, `standard`, and `legal_hold`. Plaintext is limited to 16 MiB per item and 64 MiB per append, lives only in zeroizing buffers, and never appears in `Debug` output.

The `KeyProvider` interface is `metadata`, `wrap`, `unwrap`, `revoke`, `authenticate`, and `verify`. A key handle is the opaque identifier of exactly one wrapped DEK; it is bound into the wrapping AAD, so a wrapped key cannot be replayed under another handle, and it is the unit of revocation.

The reference implementation is a local sealed keyring in an owner-only anchored directory holding `keyring.v1.json`, `revocations.v1.jsonl`, and an exclusive lock file. The 32-byte root key is supplied out of band and never written to disk; wrapping and authentication subkeys are derived from it by domain-separated HMAC-SHA-256. The revocation journal is an authenticated hash chain: an authenticated header, then one record per revocation carrying epoch, handle, idempotency key, previous tag, receipt tag, and record tag. Loading re-verifies every tag and rejects duplicate idempotency keys, chain breaks, and epoch gaps.

The provider epoch is the monotonic count of revocation records and is persisted alongside stream heads and checkpoints. Live provider instances that authenticate the same key identity under the same storage root share the greatest authenticated epoch and journal head observed by any of them, which rejects rollback within a process; cooperative file locks serialize honest writers across processes but are not trusted monotonic custody.

There is no key rotation in this milestone. A keyring directory is single-key for life: publication refuses to overwrite an existing keyring, the key identity is compared for equality on every unwrap and verify, and a mismatch fails rather than falling back to an older key. The provider version is recorded on every checkpoint and a checkpoint whose key version differs from the live provider is rejected. Key lifecycle is therefore per-handle cryptographic erasure, not re-wrapping. Adopting a new key identity requires a new keyring plus re-sealing, and neither an external KMS adapter nor a rotation procedure exists yet.

## Retention, legal holds, and erasure

A retention request binds exact scope, versioned policy, authenticated authority, a safe reason code, ordered targets, and idempotency. Prepared and finalized outcomes, provider revocation receipts, and legal-hold changes are all authenticated, and provider revoke identities are domain-separated by full scope, operation, and Evidence ID so multi-target and cross-scope operations cannot collide.

Erasure is a crash-consistent saga. Prepare locks and revalidates Evidence identity, ciphertext digest, classification, retention class, minimum age, prior availability, and the authenticated policy and authority, then atomically records the operation and events and makes every target unreadable as `erasure_pending`. Key revocation happens after that commit and is idempotent; a provider failure leaves the operation pending, and reconciliation resumes from authenticated durable state. Finalize verifies the prepared receipt, every per-target provider receipt, the provider epoch, and the aggregate receipt before atomically publishing tombstones and completion events and marking Evidence `erased`. Physical ciphertext cleanup uses PostgreSQL transaction time, runs only after durable finalization and the configured cleanup delay, and returns the original receipt on exact retry.

Legal holds are authenticated append-only place and release records. Every eligibility decision reconstructs and verifies hold history first: a hold committed before prepare blocks erasure, and a hold placed later cannot resurrect pending or erased content. Tombstones preserve only approved identifiers, digests, classification, retention class, prior state, policy, authority, and timestamps. Event and Evidence history remains append-only throughout.

## Executable materialization

`ExecutableGraphMaterializer` is the exact-scope inverse of externalization. It revalidates the projection and the derived publication Evidence IDs, reads each slot's sealed record in scope, revalidates scope, Evidence ID, content digest, sensitivity, and media type, decrypts only through `EvidenceOpener` and `KeyProvider`, accumulates against a 64 MiB budget, parses into a buffer that zeroes its strings and keys on drop, and re-verifies the plaintext digest against the slot.

A required slot that is unavailable for any reason fails with `GHE012_CONTENT_UNAVAILABLE`. An optional slot yields an explicitly typed unavailability instead. There is no cache, fallback, or plaintext substitute: erased required content blocks execution.

## Projection rebuild

Projections are disposable. A rebuild creates a fresh generation with an exact scope, stream, name, version, and generation watermark, consumes bounded verified pages, and transactionally advances a hash-bound watermark. It resumes across persisted page boundaries, catches up to a source head that advanced during the rebuild, rechecks the head before the swap, and activates only a complete generation; a failed swap leaves the previous active generation unchanged. Checkpoints are insert-only, active pointers are guarded, and a stored generation is replayed deterministically before it is trusted. A cursor, hash, or version that cannot safely resume is `GHE005_INTEGRITY_FAILURE`.

Checkpoint intervals are domain-owned rather than caller-controlled, which bounds restart work without quadratic replay.

## Encrypted backup and verified restore

Backup streams custom-format `pg_dump` bytes through ordered 1 MiB XChaCha20-Poly1305 chunks with an authenticated header and footer, zeroizing key and plaintext buffers, and exact provider-epoch equality. The authenticated manifest binds source identity, pinned tool versions and SHA-256 digests, the migration, schema, and privilege contracts, the catalog, provider metadata, counts, an authoritative state summary, the ciphertext digest, and chunk and byte totals.

Both executables are opened and hashed before use, invoked directly with an argument array and no shell and no password in argv, drained through bounded output readers, and owned by a cancellation-safe watchdog; Unix process groups and Windows kill-on-close job objects terminate descendants. Publication is no-replace on both platforms and an existing destination is never overwritten.
Durability is not symmetric: on Unix the containing directory is fsynced, while NTFS exposes no
directory fsync, so on Windows the directory entry itself is not guaranteed durable after a power
loss even though the file contents are. Treat Windows publication as durable for the bytes and
best-effort for the directory entry.

Restore authenticates and fully decrypts the same retained archive handle before streaming to `pg_restore`, accepts only a fresh distinct target, compares both authenticated passes exactly, and rejects an observable provider-epoch rollback. Exclusivity uses a durable provider-authenticated ownership marker and a temporary connection limit without changing the target ACL; success and recovery restore the exact prior owner, effective ACL, and limit. Post-restore verification checks the exact migration, schema, RLS, policy, trigger, function, and grant contracts, all bounded event chains and checkpoints, references and Evidence state and ciphertext, provider-authenticated retention authorities, holds, receipts, tombstones and cleanup, and an independent projection rebuild. Cleanup receipts reconstruct their canonical request digest and must map bijectively to the matching `EvidenceCiphertextDeleted` events. Only then is a serializable restore receipt returned.

A failed restore leaves its partial target disabled behind an authenticated marker for manual recovery. No failed target or quarantine is automatically renamed or dropped by database name.

## Enforced limits

| Resource | Maximum |
|---|---:|
| One event | 1 MiB |
| One serialized batch | 16 MiB |
| Events in one batch | 10,000 |
| Local journal | 64 MiB |
| Evidence plaintext, one item | 16 MiB |
| Evidence plaintext, one append | 64 MiB |
| Evidence items in one append | 10,000 |
| Artifacts in one append | 64 |
| Content items in one projection | 8,192 |
| Materialized executable content | 64 MiB |
| Events in one read page | 1,000 |
| Events in one integrity request | 100,000 |
| Serialized cursor | 4 KiB |
| Retention targets in one operation | 10,000 |
| Key-wrap AAD | 4 KiB |
| Authenticated message | 1 MiB |
| Encrypted backup chunk | 1 MiB |
| Encrypted backup plaintext | 64 GiB |
| Captured tool output | 64 KiB |
| Operator configuration file | 64 KiB |

Any wire integer is additionally bounded by 9,007,199,254,740,991 so JSON consumers cannot lose precision.

## Stable diagnostic catalog

| Code | Meaning |
|---|---|
| `GHE001_SEQUENCE_CONFLICT` | Expected sequence differs from the locked stream head. |
| `GHE003_IDEMPOTENCY_CONFLICT` | A key exists with a different canonical request digest. |
| `GHE004_INVALID_EVENT` | A request, envelope, identifier, or reference is invalid. |
| `GHE011_SCOPE_VIOLATION` | Retention scope is absent, invalid, foreign, or inconsistent. |
| `GHE005_INTEGRITY_FAILURE` | Hash chain, checkpoint, cursor, watermark, Evidence, or artifact digest fails. |
| `GHE006_LIMIT_EXCEEDED` | A deterministic repository, Evidence, page, or range bound is exceeded. Includes a physical batch the schema validator declines to process: `MAX_BATCH_EVENTS` and `MAX_BATCH_BYTES` are not the only bounds a batch can cross, and a batch too complex to validate is refused at append rather than written and then unreadable (#744). |
| `GHE007_UNSUPPORTED_FORMAT` | Repository bytes are not the single supported baseline format. |
| `GHE012_CONTENT_UNAVAILABLE` | A required content slot cannot be materialized. |
| `GHE008_STORAGE_FAILURE` | The underlying store failed without a more specific classification. |
| `GHE009_EXTERNALIZATION_FAILED` | Authoring content cannot be safely projected and sealed. |
| `GHE010_STREAM_SELECTION_REQUIRED` | The operation needs an explicit single stream. |
| `GHE013_READ_BUDGET_EXCEEDED` | A read that declared a wall-clock budget outlived it mid-walk. |
| `GHEV001_EVIDENCE_UNAVAILABLE` | Evidence is pending erasure, erased, expired, missing-key, or corrupt. |
| `GHEV002_LEGAL_HOLD` | A legal hold blocks erasure or cleanup. |
| `GHEV003_RETENTION_INELIGIBLE` | Policy, age, scope, or state does not permit the action. |
| `GHEV004_EVIDENCE_INVALID` | Evidence metadata, ciphertext, AAD, or registration is invalid. |
| `GHK001_KEY_UNAVAILABLE` | Key handle, root key, tag, or provider state is unavailable or invalid. |
| `GHB001_BACKUP_INVALID` | Backup header, manifest, ciphertext, tool version, or identity is invalid. |
| `GHB002_RESTORE_INVALID` | Restore target or post-restore verification is invalid. |
| `GHCLI001_ARGUMENT_INVALID` | An operator argument is missing, out of range, or mutually exclusive. |
| `GHCLI002_CONFIG_INVALID` | Operator configuration or an operator-supplied endpoint is unusable. |

Each code names exactly one meaning. `GHE002_CORRUPT_BATCH` distinguishes a batch that failed its own checksum from a broken hash chain, and `GHPROJ001_WATERMARK_MISMATCH` distinguishes a non-resumable projection generation. `GHE013_READ_BUDGET_EXCEEDED` (issue #750) is separate from `GHE006_LIMIT_EXCEEDED` for the same kind of reason: a deterministic bound refuses the same input on every machine, while a wall-clock budget refuses a walk that would have completed given longer, and the operator's next move differs. `GHE011_SCOPE_VIOLATION` and `GHE012_CONTENT_UNAVAILABLE` were renumbered off `GHE004` and `GHE008`, which the repository already used for invalid input and storage failure: repository, retention, Evidence, materialization, projection, and backup errors share the numeric families where the classification is the same. Public diagnostics carry a stable code, a fixed message, and a registered JSON Pointer. Adapter internals retain their causes without exposing them through `Display` or CLI JSON.

## Operator CLI

```text
graphhelm events verify --repository PATH
graphhelm events backup --repository PATH --output FILE
graphhelm events restore --repository EMPTY_PATH --archive FILE
graphhelm events verify --config PATH --workspace ID --project ID [--execution ID] --stream NAME [--start N] [--max-events N]
graphhelm events rebuild --config PATH --workspace ID --project ID [--execution ID] --stream NAME [--generation N] [--page-size N]
graphhelm events backup --config PATH --output FILE
graphhelm events restore --config PATH --archive FILE
```

`--repository` and `--config` are mutually exclusive. `events verify` requires exactly one selector. Backup and restore select the local path when `--repository` is present; otherwise they use `--config` or `GRAPHHELM_EVENTS_CONFIG` for PostgreSQL.

Local verification recognizes the stored format and refuses range arguments. Local backup writes archive version `1.0.0` with `journal.jsonl` and blob content; it excludes layout files such as `format.json` and `repository.lock`, and refuses a blob entry that is not a regular file. Local restore requires an empty destination and lets the repository adapter create those layout files before it writes the journal and blobs. It rejects malformed archives, unsupported archive versions, and blob names that are not plain file names.

Range verification and rebuild remain PostgreSQL-only operations. `--start` defaults to 1, `--max-events` defaults to 1,000 and is capped at 100,000, `--generation` defaults to 1, and `--page-size` defaults to 1,000. `--output` must not exist when the command looks, and `--archive` must be an existing regular file. For a LOCAL `--repository` backup that is a check and not a guarantee: `execute_local` verifies the path is absent and later publishes with `std::fs::write`, so a path created between those two operations is truncated by the write. The stronger claim -- publication that cannot replace an existing file -- is what the PostgreSQL path enforces and what the local path would need an atomic create-new publication to make true. PostgreSQL restore has no target flag: the destination is the database named by the configuration's `adminUrl`, and the operator refuses to proceed unless that database is already empty.

Configuration is a bounded JSON document supplied by `--config` or `GRAPHHELM_EVENTS_CONFIG`, limited to 64 KiB, rejected if it is a symbolic link, not a regular file, or group- or world-accessible on Unix. It declares `adminUrl`, an absolute `passfile`, a `keyring` directory and key ID, absolute `pgDump` and `pgRestore` paths with pinned SHA-256 digests and versions, and `processTimeoutSeconds` between 1 and 86,400. The 32-byte root key is never in the configuration file: it is supplied as 64 lowercase hexadecimal characters in `GRAPHHELM_EVENTS_KEY`, so a leaked configuration alone cannot unwrap Evidence.

Every command prints exactly one JSON document; `--pretty` changes whitespace only. Success exits `0` and every argument, configuration, repository, key, or verification failure exits `2`, matching the established domain-failure contract of the graph and schema commands. Output and errors never contain Evidence plaintext, keys, SQL, DSNs, process arguments, absolute paths, temporary names, or backtraces, and every configuration failure reports the same generic message so a host, user, or embedded password cannot be reconstructed.

## Pre-release reset and the absence of a compatibility layer

D-036 and D-037 accept this correction as a pre-release replacement. The product has not shipped, so there is no deployed data and no external consumer to protect. Carrying a compatibility layer would mean maintaining a dual reader, a dual writer, an importer, and heuristic format detection for bytes that only ever existed on developer machines, and every one of those paths would be an attack surface for accepting a superseded, unsafe envelope.

Accordingly the following were removed rather than supported: `LegacyEventEnvelope`, `LegacyStoredBatch`, `LegacyEventImporter` and its import context, commands, events, fixtures and conformance cases; dual readers, dual writers, automatic migration, and fallback branches; and the intermediate schema release `1.1.0`. The single reviewed baseline `1.0.0` was corrected in place with 15 contracts and no alias, translation, or legacy branch. There is no `events import` command and no repository-format migration flag. Milestone 02's generic `schema migrate` command and its conformance fixtures are unaffected. Unsupported repository bytes fail with `GHE007_UNSUPPORTED_FORMAT` and are never interpreted heuristically. Git history is the audit trail for the replacement; runtime compatibility code is not.

The developer-data reset follows from that. Existing local repositories, PostgreSQL databases, and golden hashes produced before this milestone must be deleted and recreated from the safe projection. Nothing translates them, and no error message will suggest that anything could.

## Rollback

Rollback is a code rollback of this milestone plus restoration from an authenticated backup taken by the matching code. It never downgrades repository bytes in place. Because there is no compatibility reader, an older binary cannot open a repository written by this milestone, and this milestone cannot open anything written before it; the archive and the binary must match.

Restoration follows the documented procedure in [docs/operations/OBSERVABILITY_AND_RECOVERY.md](../operations/OBSERVABILITY_AND_RECOVERY.md): restore into a fresh, distinct database, let post-restore verification complete, and select the verified target only after its receipt is returned. A restore that fails leaves the partial target disabled behind its authenticated marker and requires manual recovery; the source database is never modified.

## Acceptance evidence

Local Windows gates pass formatting, warning-free workspace Clippy with `-D warnings`, all-feature locked workspace tests, CLI smoke tests, schema catalog and conformance suites, all five canonical graph commands with an unchanged canonical digest, a secret and no-echo scan, and `git diff --check`. The PostgreSQL suites are ignored in ordinary runs and executed serially against a disposable cluster with `--ignored --test-threads=1` and `GRAPHHELM_TEST_ADMIN_URL`; CI fails if PostgreSQL or the client tools are absent. That serial suite covers concurrency, isolation, migration, projection, repository conformance, retention, and backup/restore.

Each task also killed and restored a prescribed mutation matrix: forced RLS, transaction-local scope, idempotency ordering, the stream lock, atomic commit, event-hash verification, cursor binding, checkpoint authentication, authenticated-head verification, authority verification, hold enforcement, pending-before-revoke ordering, provider idempotency, receipt authentication and epoch, finalize atomicity, cleanup ordering, projection swap safety, and the restored cleanup-receipt-to-event correlation. Tests use no Docker, internet, production credentials, provider accounts, or production infrastructure.

## Explicitly out of scope

The Runtime API, authentication and RBAC, scheduler, jobs and leases, full Graph Engine recovery, artifact bytes and object storage, Knowledge Graph, Studio, multi-node replication, external buses, models, tools, sandboxes, Dreams, billing, hosted services, and telemetry export remain separate milestones. Cloud KMS and Vault key adapters, key rotation, external monotonic key custody, backup scheduling, remote object storage for archives, and key-handle garbage collection are not implemented here. Their absence is a trust limitation, not implicit authorization.
