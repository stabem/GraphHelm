# Event/Evidence Store threat model

## 1. Status and scope

**Status:** Accepted for Milestone 03 implementation and security review.

This focused model implements the documentation gate required by [D-035 through D-037](../DECISION_REGISTER.md), [ADR-021 through ADR-023](../reference/REFERENCE_STACK_AND_ADRS.md), and the [accepted safe persistence projection design](../specs/2026-08-09-safe-persistence-projection-design.md). It covers Governor externalization, `PersistedGraphVersion`, the Event Journal, encrypted Evidence Store, artifact-reference catalog, PostgreSQL adapter, `KeyProvider`, retention and legal-hold service, projection rebuild, integrity checkpoints, and encrypted backup/restore boundary.

Runtime authentication/RBAC, public APIs, artifact bytes, cloud KMS/Vault adapters, scheduler recovery, Studio, models, tools, sandboxes, and hosted services are outside this milestone. Their absence is a trust limitation, not implicit authorization.

## 2. Security objectives and protected assets

The required properties are confidentiality of evidence and keys; integrity, ordering, scope and provenance of canonical events; availability through bounded operations and recoverable state; deterministic replay without evidence plaintext; auditable authorized erasure; and fail-closed backup/restore.

Protected assets are:

- replay-safe event envelopes, sequence numbers, idempotency records, stream heads, hash chains, checkpoints, and receipt events;
- evidence ciphertext, nonces, wrapped DEKs, ciphertext digests, metadata, availability states, and minimal tombstones;
- KEKs, unwrapped DEKs, authentication keys, revocation handles, revocation receipts, and the monotonic provider epoch;
- repository scope, actor identity, retention authority, policy identity/version, reasons, legal holds, and operation idempotency;
- immutable artifact metadata and opaque locators;
- projection generations and watermarks;
- authoring graphs, prepared plaintext, safe topology, typed content slots, content digests, and executable-materialization availability;
- database roles, migrations, RLS policies, connection-pool state, and database identity;
- encrypted dumps, authenticated manifests, restore-verification receipts, configured executable identity, and owned temporary files;
- logs, diagnostics, traces, metrics, fixtures, command output, and process metadata that must remain free of secrets and plaintext.

## 3. Actors and trust assumptions

- **Repository caller:** trusted to supply only a scope it is authorized to use. It is untrusted for payload content, identifiers, cursors, ordering, sequence, and idempotency claims.
- **Event actor:** recorded provenance. Actor identity does not grant retention authority or database administration.
- **Retention authority:** a separate trusted capability allowed to request a policy-bound action; each request is still validated for scope, policy, reason, legal hold, idempotency, and state.
- **Runtime database role:** least-privilege role without `BYPASSRLS`, table ownership, schema mutation, or backup/restore capability.
- **Migration/backup operator:** separately authorized to use the administrative role and configured database/key profiles. Operator inputs and environment remain untrusted.
- **PostgreSQL:** trusted for transaction and locking semantics while healthy, but rows, errors, backups, and restored state are treated as potentially corrupted or attacker-controlled.
- **KeyProvider:** trusted to generate/wrap/unwrap/revoke keys and authenticate material, preserve the revocation journal, and report a monotonic epoch. Handles, receipts, metadata, and failures are validated.
- **Governor externalization and projection code:** trusted deterministic code operating on untrusted authoring graphs, events, repository bytes, cursors, content slots, Evidence references, and watermarks.
- **External processes:** explicitly configured `pg_dump` and `pg_restore` executables; their output, exit status, timing, and failure behavior are untrusted.
- **Attacker:** may control repository inputs, Graph DSL-derived payloads, unsupported repository formats, a project scope, cursors, backup files, process output, timing/concurrency, and crash points. A stronger attacker may read or modify PostgreSQL, obtain an obsolete keyring snapshot, or compromise the active `KeyProvider`.

All live local sealed-provider instances in one process that authenticate the same key identity under the same stable storage-root object share the greatest authenticated revocation epoch and journal head observed by any of them. Cooperative filesystem locks still serialize honest writers across processes, but they are not trusted monotonic custody: independent processes with write access to the provider root do not share the in-memory floor. Rollback by such a process is within the active-provider or malicious-host compromise boundary and requires future external monotonic custody or an HSM for a stronger guarantee. The restore boundary must still compare the backup provider epoch with the current provider epoch before releasing plaintext.

## 4. Trust boundaries

1. Future authenticated Runtime to trusted repository caller. Authentication and RBAC are not implemented here.
2. Caller-controlled scope and data to typed repository contracts.
3. Core repository interfaces to the PostgreSQL adapter and pooled connections.
4. Runtime database role to forced-RLS tables; administrative role to migrations and dump/restore.
5. PostgreSQL ciphertext/wrapped keys to the external `KeyProvider` and zeroizing plaintext buffers.
6. Event append transaction to post-commit provider revocation and the retention reconciler.
7. Canonical Event Journal to disposable projection generations.
8. Schema-valid authoring graphs and bounded prepared plaintext to Governor-produced Evidence plus safe persisted topology.
9. Database state to encrypted backup stream, authenticated manifest, and fresh restore target.
10. CLI configuration to directly spawned dump/restore processes and bounded JSON diagnostics.

RLS is defense in depth and **does not authenticate caller-selected scope**. A caller allowed to choose arbitrary session scope can select another tenant unless the future Runtime authenticates the caller and authorizes that scope before repository entry.

Authenticated checkpoints detect database-only tampering only while their authentication key remains trustworthy. **Simultaneous compromise of the database plus the active key provider defeats authenticated checkpoints**: such an attacker can rewrite events and produce matching anchors. This milestone makes no stronger transparency or public-signature claim.

## 5. Data flows

1. **Governor publication:** validate authoring schema, lint, policy, scope, bounds and secret/content policy; externalize registered content paths; seal Evidence through `KeyProvider`; build safe topology and ordered slots; verify slot/reference bijection and semantic hashes; append Evidence, references, artifacts and events atomically; activate the graph version last.
2. **Read/verify:** bind a bounded cursor to exact scope and stream; apply RLS and composite constraints; verify order, continuity, hashes and a trusted head/checkpoint; return replay-safe events or typed evidence unavailability.
3. **Evidence read:** authorize exact scope; load ciphertext and wrapped DEK; unwrap and AEAD-decrypt using bound metadata as AAD; expose plaintext only through a bounded zeroizing buffer.
4. **Retention/erasure:** validate authority, policy, scope, reason, idempotency and holds; commit `erasure_pending`; revoke the key handle after commit; verify the provider receipt; finalize tombstone and receipt events; reconcile interrupted operations.
5. **Projection:** create a fresh generation; consume bounded verified pages; transactionally advance a hash-bound watermark; activate only a complete generation.
6. **Local publication:** use the same safe wire contract as PostgreSQL; publish durably synced encrypted Evidence blobs before the locked journal batch and activate the version marker last. Unknown or superseded formats fail generically and are never interpreted heuristically.
7. **Backup:** directly spawn configured `pg_dump`; stream the dump through authenticated chunked encryption; authenticate the canonical manifest; durably publish with atomic no-replace.
8. **Restore:** authenticate all backup material before trusting metadata; reject observable provider-epoch rollback; directly stream plaintext to configured `pg_restore` for a new empty database; validate migrations, RLS, chains, references, receipts and rebuilt projections; emit a verification receipt before selection.

## 6. Attacker capabilities and excluded guarantees

The design assumes adversarial identifiers, JSON/YAML/JSONL, event data, evidence metadata, artifact references, selectors, cursors, manifests, ciphertext, database rows, SQL error text, process output, and operation timing. Attackers may retry, race, cancel, exhaust limits, reuse pooled connections, submit shell metacharacters, inject crash/failure points, alter backups, or substitute an older database/key snapshot.

The design does not guarantee availability against loss of both database and backups, confidentiality after active-key-provider compromise, checkpoint authenticity after simultaneous database/key-provider compromise, authorization before the future Runtime boundary exists, or irreversible erasure if an attacker restores an obsolete keyring whose monotonic revocation state was not preserved. Public transparency, per-event signatures, HSM guarantees, and protection from a malicious host kernel are future controls.

## 7. STRIDE and OWASP threat/control matrix

| ID | Threat and attacker action | STRIDE / OWASP | Required controls | Residual risk |
|---|---|---|---|---|
| T-01 | Inject SQL through scope, IDs, cursor, metadata, profile, or selectors | Tampering, Disclosure, Elevation / A03 Injection | Parameterized SQL only; typed bounded identifiers; no dynamic identifiers from callers; least-privilege roles; redacted errors | Vulnerability in database/client library or unsafe future query code |
| T-02 | Select foreign rows by changing scope or bypassing RLS | Spoofing, Disclosure, Elevation / A01 Broken Access Control | Exact typed scope; forced RLS; runtime role without bypass/ownership; composite keys; uniform not-found result | RLS cannot authorize caller-selected scope; future Runtime auth/RBAC is required |
| T-03 | Reuse a pooled connection carrying another scope | Disclosure, Elevation / A01 Broken Access Control | Transaction-only `SET LOCAL`; omitted scope fails closed; no session scope; rollback before pool return; adversarial reuse tests | PostgreSQL/driver defects or later raw-connection escape hatch |
| T-04 | Reuse/collide an idempotency key to suppress or substitute an append/retention action | Spoofing, Tampering, Repudiation / A04 Insecure Design | Resolve keys before sequence checks; bind canonical request digest and exact scope/stream; exact retry returns original result; divergent reuse fails closed | Deliberate denial within an already authorized scope until keys rotate |
| T-05 | Race writers to duplicate sequence, fork a stream, or leave gaps | Tampering, Repudiation / A04 Insecure Design | Stream-head row lock; database uniqueness; expected-next sequence; whole-batch transaction; hash-chain validation | Database outage reduces availability but cannot be guessed around |
| T-06 | Persist secrets in replay payloads or expose them through output | Disclosure / A02 Cryptographic Failures, A09 Logging and Monitoring Failures | Schema-first allowlist; forbid free-form sensitive classes; deterministic secret scan; evidence references; redacted structured errors; no plaintext in logs/fixtures/manifests/CLI | Pattern scanner cannot recognize every secret; allowlisted schemas remain primary control |
| T-07 | Read database, steal wrapped keys, compromise key material, or restore revoked handles | Disclosure, Tampering / A02 Cryptographic Failures | Per-record AEAD; fresh OS-CSPRNG nonce; scope/metadata AAD; KEK outside database; zeroization; durable revocation journal and monotonic epoch; restore rollback check | Active `KeyProvider` compromise exposes authorized decryptions; obsolete unobserved keyring restore may resurrect handles |
| T-08 | Alter/reorder/delete events, heads, checkpoints, or watermarks | Tampering, Repudiation / A08 Data and Software Integrity Failures | Canonical hashes; contiguous chain; immutable sequences; authenticated append-only checkpoints; hash-bound cursors/watermarks; first-corruption diagnostics | Database plus active-key-provider compromise defeats authenticated checkpoints |
| T-09 | Race erasure with legal hold, replay provider revocation, or partially erase evidence | Tampering, Repudiation, Elevation / A01 Broken Access Control, A04 Insecure Design | Separate authority; policy/version/reason/scope; shared row locks; revalidation; pending state blocks reads; idempotent provider revocation; authenticated receipt; reconciler; physical deletion only after committed erasure and no hold | Revocation is irreversible once pending; a hold arriving after prepare is rejected by design |
| T-10 | Substitute, roll back, disclose, truncate, or overwrite a backup | Spoofing, Tampering, Disclosure / A02 Cryptographic Failures, A08 Data Integrity Failures | Authenticated chunked encryption; authenticated canonical manifest bound to database/schema/repository identity; complete pre-restore verification; provider epoch check; new empty target; atomic no-replace publication | Loss/compromise of separate key custody or malicious host can deny recovery |
| T-11 | Turn dump/restore configuration into shell execution or leak secrets through process metadata | Elevation, Disclosure / A03 Injection, A05 Security Misconfiguration | Direct process spawn without shell; explicit executable; fixed arguments; opaque profiles/keys; no connection secret in argv; bounded/redacted output | Malicious replacement of an explicitly trusted executable remains host/supply-chain risk |
| T-12 | Exhaust memory, CPU, storage, locks, database work, crypto, or process output | Denial of Service / A04 Insecure Design | Bounds before expensive work on files, nesting, externalizable content, batches, payloads, evidence, selectors, pages, cursors, verification ranges, process output and temporary files; timeouts/cancellation | Authorized traffic within limits can still exhaust finite host capacity; quotas are future Runtime work |
| T-13 | Crash/cancel between externalization, Evidence or event writes, key revocation, file publication, or restore | Tampering, Repudiation, DoS / A04 Insecure Design | Database transactions; journal-last local publication; active-marker-last activation; explicit retention state machine; idempotent side effects; bounded reconciler; durable flush/sync; owned temp cleanup; atomic no-replace; restore verification receipt | Crash after irreversible revocation leaves evidence unavailable pending reconciliation, intentionally failing closed |
| T-14 | Persist a raw authoring record or `GraphVersionRecord` in the Event Journal | Disclosure, Tampering / A02 Cryptographic Failures, A04 Insecure Design | Governor-only publication; distinct non-aliasing types; explicit fallible conversion; schema allowlist; forbidden-content scan; no bypass constructor | A future registered safe field can be misclassified unless schema and adversarial tests evolve together |
| T-15 | Make `contentSlots` and envelope `evidenceRefs` diverge by omission, duplication, order, scope, or digest | Tampering, Repudiation / A04 Insecure Design, A08 Data Integrity Failures | Deterministic slot IDs and ordering; exact one-to-one ordered bijection; composite scope binding; digest verification before append and during replay | Implementation defects shared by producer and verifier require independent conformance vectors |
| T-16 | Leak plaintext through journal bytes, diagnostics, temporary names, logs, or crash output | Disclosure / A02 Cryptographic Failures, A09 Logging and Monitoring Failures | Safe projection; `PersistedDiagnostic` without dynamic prose/filesystem path; opaque owned temp names; redacted errors; canary scans across success, rejection, and injected crash paths | Host-level memory or active debugger access remains outside this persistence guarantee |
| T-17 | Execute erased or unavailable required content from cache, stale materialization, or fallback | Tampering, Elevation / A01 Broken Access Control, A04 Insecure Design | Availability checked at executable materialization; required slots fail closed before model/tool calls; no fallback/legacy reader; cache bound to Evidence state and authorization | A compromised executor outside the Governor/materialization boundary remains future Runtime risk |
| T-18 | Heuristically accept a superseded internal format as the safe baseline | Spoofing, Tampering / A04 Insecure Design, A08 Data Integrity Failures | One strict `1.0.0` inventory; no dual readers/importer/fallback; exact format/schema dispatch; generic unsupported-format diagnostic | Developer data in removed formats must be discarded and recreated |
| T-19 | Copy a context, permission, or isolation writable path into durable topology or mislabel it as another content kind | Disclosure, Tampering / A02 Cryptographic Failures, A04 Insecure Design | Closed `ContextPath`/`PermissionPath`/`IsolationPath` positions; exact owner/ordinal binding; `restricted` execution-required Evidence; slot hashing; canary scans over journal, logs, diagnostics and errors | A future authoring path surface requires a new registered typed position and adversarial coverage |

## 8. Exact acceptance tests

All tests use fixed clocks/IDs and deterministic fake failure points where applicable. PostgreSQL cases run against a pinned ephemeral isolated instance with separate runtime and administrative roles. Public results assert stable diagnostic codes/paths and absence of foreign-data or secret echoes, not database prose.

### AT-01 — SQL injection

- For every string-bearing repository input, submit quotes, comments, statement separators, Unicode confusables, backslashes, and SQL keywords while a foreign-scope sentinel row exists.
- Assert each value is either treated as opaque data or rejected by typed bounds; no additional statement executes, schema/data remain unchanged, and neither sentinel existence nor SQL error text is returned.
- A static gate rejects non-parameterized runtime SQL and caller-derived SQL identifiers in the adapter.

### AT-02 — RLS bypass and foreign-scope indistinguishability

- Create two workspaces/projects with events, evidence, artifacts, idempotency records, checkpoints, retention rows, and projections using the administrative fixture role.
- Through the runtime role, set scope A and attempt direct/select/join/subquery access to every scoped table using identifiers from B; also attempt to disable RLS, set role, mutate scope columns, and use omitted/malformed scope.
- Assert zero foreign rows and no foreign existence signal; repository reads for foreign and nonexistent identifiers have the same public shape/code. Assert the runtime role lacks `BYPASSRLS`, ownership, schema mutation, and admin capabilities, and forced RLS is present after migration and restore.

### AT-03 — Pool scope leakage

- Use a pool size of one. Commit, roll back, cancel, and error transactions under scope A, then reacquire the same physical connection for scope B and for a request with no scope.
- Assert B sees only B, the unscoped transaction fails closed, `SET LOCAL` state does not survive commit/rollback, and retry/evidence/artifact/projection queries reveal nothing from A.

### AT-04 — Idempotency collisions

- Append a bounded batch, then retry byte-for-byte with the same scope, stream, keys, and canonical request digest after the stream head has advanced.
- Assert the exact original envelopes, IDs, timestamps, sequences, and hashes return without new rows.
- Reuse any event/import/retention idempotency key with changed data, ordering, target, policy, scope, or stream; assert a stable idempotency-conflict rejection, no sequence-conflict masking, and no state change.

### AT-05 — Sequence races

- Synchronize at least two transactions against one stream and the same expected next sequence, then release them concurrently; repeat for a newly created stream and a multi-event batch.
- Assert exactly one whole batch commits and every loser reports sequence conflict. Assert unique contiguous sequences, one correct head, one hash chain, no gaps, duplicates, partial evidence/artifacts, or orphan references.

### AT-06 — Payload secrets and output redaction

- Submit each forbidden replay field class (prompt, output, log, credential, authorization header, environment, raw tool result), known secret patterns, nested/oversized variants, and secret-bearing authoring records at every registered and unregistered content position.
- Assert rejection occurs before database/evidence/artifact mutation. Search captured stdout, stderr, structured diagnostics, logs, traces, metrics, fixtures, manifests, database event JSON, and debug output for exact canary values; assert zero matches.
- Store the same canaries through the Evidence boundary and assert PostgreSQL contains only ciphertext, wrapped keys and approved metadata, never plaintext or a public plaintext digest.

### AT-07 — Database and key compromise

- With a complete database snapshot but no `KeyProvider`, assert evidence plaintext and KEK cannot be recovered and tampering with ciphertext, nonce, wrapped key, scope, ID, schema version, media type, or sensitivity produces typed integrity/unavailable failure.
- Revoke a handle, restore an older database containing the wrapped DEK, and assert `unwrap` still rejects it using the provider revocation journal. Present a backup provider epoch older than current and assert restore fails before plaintext is sent to `pg_restore`.
- Record the residual test conclusion explicitly: a harness controlling both PostgreSQL and the active `KeyProvider` can forge a new chain and authenticated checkpoint, so the system must not claim detection under simultaneous compromise.

### AT-08 — Event, checkpoint, cursor, and watermark tampering

- Independently alter, delete, duplicate, reorder, or splice each event field, sequence, previous hash, event hash, head, checkpoint tag, cursor scope/stream/sequence/version, and projection watermark.
- Assert bounded verification rejects at the first corrupt sequence with a redacted integrity diagnostic; foreign cursors fail without existence leakage; rebuild never activates a generation whose watermark/chain mismatches; no historical row or checkpoint is repaired or guessed.

### AT-09 — Retention and legal-hold races

- Interleave legal-hold placement immediately before the prepare lock, while prepare holds the row, after prepare commits, during provider revocation, and before physical deletion.
- Assert a hold committed before prepare always blocks erasure; once `erasure_pending` commits, reads are unavailable and a later hold is rejected; provider failure leaves pending state; exact retries resume/return the original operation; divergent retries fail; authenticated revocation finalizes one tombstone and one logical completed receipt; physical cleanup requires completed erasure and no hold.
- Crash after every state transition and resume with the bounded reconciler; assert plaintext never becomes available from pending/erased state and no receipt, tombstone, policy/authority/reason, or canonical event is removed.

### AT-10 — Backup substitution and rollback

- Alter, truncate, extend, reorder, or swap encrypted chunks, header, wrapped key, manifest, authentication tag, database identity, schema version, repository version, checkpoint summary, provider epoch, and required tool version.
- Assert restore rejects all variants before marking or selecting a target and never changes the source. Restore valid material only into a new empty database, rebuild projections, compare all recorded summaries, and require a restore-verification receipt.
- Precreate the backup destination and simulate interruption at every publication stage; assert no overwrite, no plaintext dump file, durable atomic publication only on success, and removal limited to the operation-owned temporary file.

### AT-11 — Process execution boundary

- Use instrumented fake `pg_dump`/`pg_restore` executables and configuration values containing spaces, quotes, shell metacharacters, substitutions, option prefixes, and secret canaries.
- Assert exactly one configured executable is spawned directly without a shell; metacharacters remain inert data; fixed required flags cannot be overridden; database/key profiles are opaque; secrets do not appear in argv, process listings, diagnostics, or captured bounded output.
- Missing, replaced, version-incompatible, hanging, noisy, nonzero, or prematurely exiting processes fail closed, leave no usable restore target, and never publish a partial backup.

### AT-12 — Denial-of-service bounds

- For every published limit on file bytes, nesting, externalizable fields/content, batch count, payload/evidence bytes, selectors, pages, cursors, verification range, projection page, process output, and temporary output, test `limit - 1`, `limit`, and `limit + 1`, plus arithmetic-overflow and decompression/expansion-shaped inputs where applicable.
- Assert accepted boundary values remain deterministic; over-limit input returns the stable limit diagnostic before database locks, allocation proportional to claimed size, cryptography, full-file parsing, or process launch. Cancellation/timeouts release locks, connections, buffers, and owned temporary resources.

### AT-13 — Crash consistency

- Inject failure/cancellation before and after each externalization, sealing, append, and activation stage; assert either the complete committed batch plus Evidence/references/head exists and the active marker advances, or the prior active version remains unchanged.
- Inject crashes before retention prepare commit, after prepare, before/after provider revocation, and before/after finalize; assert the documented pending/completed state, idempotent reconciliation, one provider side effect, and no plaintext availability after prepare.
- Inject failures during backup encryption, flush, durable sync, no-replace publication, restore streaming, validation, projection rebuild, and receipt emission; assert no plaintext file, no overwritten destination, no selected partial restore, and cleanup only of owned temporary resources.

### AT-14 — Raw authoring record reaches Event Journal

- Publish every official authoring graph plus adversarial inline objectives, instructions, purposes, textual completion contracts, diagnostic details, schema annotations, paths, and unknown free-form values through the real Governor/externalizer.
- Assert journal events deserialize only as `PersistedGraphVersion`, contain bounded safe topology and ordered slots, and contain none of the exact authoring plaintext. Attempt direct/raw `GraphVersionRecord` construction at each public persistence boundary and assert it is impossible by type or rejected before append with no state change.

### AT-15 — Content slot and `evidenceRefs` diverge

- Starting from a valid publication, independently remove, duplicate, reorder, cross-scope, change the Evidence ID, and change the content/ciphertext digest in slots and envelope references.
- Assert every mismatch fails before append or at the first corrupt replay record with a stable safe-path diagnostic; no partial Evidence, event, head, or active-version change occurs. Re-encryption with the same plaintext may change Evidence metadata while preserving the slot content digest and semantic hash.

### AT-16 — Plaintext appears in journal, diagnostics, temp names, or crash output

- Seed unique canaries in every externalized field and in relative/absolute source paths, then exercise success, every validation rejection, sealing failure, local crash point, PostgreSQL rollback, debug formatting, logs, stdout/stderr, metrics, manifests, and owned temporary-file enumeration.
- Search serialized journal/database rows and all captured surfaces for the exact canaries and path components; assert zero matches. Assert persisted diagnostics expose only stable code, severity, bounded domain path, closed component, and authorized digest/reference fields.

### AT-17 — Erased required content is executed from cache/fallback

- Materialize once, then erase, expire, remove, corrupt, or revoke one execution-required Evidence slot and retry through fresh and deliberately stale cache paths.
- Assert availability is revalidated, materialization returns the stable required-content-unavailable diagnostic, and zero model, tool, shell, network, or deploy side effects occur. Optional unavailable slots remain explicitly unavailable and never borrow content from another version or scope.

### AT-18 — Superseded internal format is heuristically accepted

- Present prior Foundation envelopes, internal `1.1.0` artifacts, legacy command-shaped input, structurally similar unknown JSON, and mixed old/new repositories to every JSONL, PostgreSQL, CLI, restore, and replay entry boundary.
- Assert exact safe `1.0.0` input is the only accepted contract; every other format fails with the same generic unsupported-format family before interpretation or mutation. The gates that exist are behavioral and schema-level: `apps/cli/tests/event_store_cli.rs` asserts no import, migrate, upgrade or convert subcommand exists and that unsupported-format output never names a legacy or migration path, and `core/schema-evolution/tests/catalog_integrity.rs` asserts no superseded release directory remains. There is no static source-inventory gate asserting the absence of legacy importer, dual reader/writer or runtime compatibility symbols; that absence is currently maintained by review rather than enforced by a test.

### AT-19 — Authoring path scopes cross the durable boundary

- Publish graphs containing distinct canaries at every ordered context source-scope path, permission-scope path and isolation filesystem writable path, including duplicates and multiple owners.
- Assert each value becomes exactly one correctly ordered `ContextPath`, `PermissionPath` or `IsolationPath` slot with `restricted` sensitivity and `requiredForExecution=true`; topology and semantic identities include the typed positions/content digests while Evidence IDs and encryption metadata do not.
- Search topology, event bytes, logs, diagnostics and errors for every path canary and component; assert zero matches. Remove or erase one required path Evidence and assert materialization fails before any model, tool, shell, network or deploy effect. A foreign path field kind and any owner/ordinal mismatch fail closed.

## 9. Security review gates

Task 11 review must map implementation evidence to every `AT-*` case and every threat row. Acceptance also requires parameterized-query review, migration/RLS inspection, dependency audit, secret-canary search, direct-process-spawn inspection, full locked test commands, and independent OWASP-oriented review. A missing test, silent skip, production credential, Docker dependency, unresolved critical/important finding, or claim stronger than the limitations above blocks acceptance.

Changes to the Event/Evidence split, erasure semantics, trust assumptions, key custody, authenticated checkpoints, caller-scope authorization, or backup identity binding require an accepted superseding ADR and an updated threat/control/test matrix before implementation changes.
