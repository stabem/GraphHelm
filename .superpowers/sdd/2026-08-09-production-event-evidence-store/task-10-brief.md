# Task 10 substitute brief - encrypted backup and verified restore

## Authority

The user explicitly authorized the Task 10 section of
`docs/superpowers/plans/2026-08-09-production-event-evidence-store.md` as the official substitute
brief because no standalone Task 10 brief existed at clean base
`9bc2d9b3d5f9006c6b48494a144af476bbfc587e`.

## Exact scope

- Create `adapters/postgres-event-store/src/backup.rs`.
- Modify `adapters/postgres-event-store/src/lib.rs`.
- Create `adapters/postgres-event-store/tests/backup_restore.rs`.
- Create this brief, the Task 10 report, and the ignored-test ledger.
- The user subsequently authorized the minimum owner-module expansion needed to avoid duplicating
  security rules in the backup adapter: modify
  `adapters/postgres-event-store/src/integrity.rs` for aggregate checkpoint authentication and
  `adapters/postgres-event-store/src/retention.rs` for aggregate retention, receipt, hold, and
  tombstone verification. During independent review, the user authorized continuing the fixes
  needed for the administrative verifier to execute through the runtime role rather than the
  BYPASSRLS admin identity. That mechanically requires the shared `set_scope` call in the four
  remaining repository consumers:
  `adapters/postgres-event-store/src/artifact.rs`,
  `adapters/postgres-event-store/src/evidence.rs`,
  `adapters/postgres-event-store/src/journal.rs`, and
  `adapters/postgres-event-store/src/projection.rs`. These changes only route existing scoped SQL
  through the central role-aware scope setter; they add no new public behavior.
- The user explicitly authorized the mechanically necessary direct workspace dependencies in
  `adapters/postgres-event-store/Cargo.toml` and corresponding `Cargo.lock` entry only:
  `chacha20poly1305`, `getrandom`, `zeroize`, `windows-sys`, and `libc`. No new crate or version is
  permitted.
- After independent security review exposed a cleanup receipt/event correlation gap, the user
  authorized the minimum `0002_retention.sql` delta and matching retention adapter/test changes:
  persist cleanup `requested_at` so restore can reconstruct the canonical cleanup request digest
  and prove a one-to-one mapping to deleted events.

No other migration/schema, core contract, CLI/Task 11, compatibility reader, production credential,
push, or PR is authorized.

## Required behavior

- Stream custom-format `pg_dump` output through ordered 1 MiB XChaCha20-Poly1305 chunks with a
  64 GiB inclusive maximum and zeroizing keys/plaintext.
- Authenticate a bounded canonical header/final manifest binding database identity, exact tool,
  format/schema/repository/catalog/migration state, provider metadata, heads/checkpoints,
  Evidence/tombstone/retention/projection summaries, chunk order/count, and byte counts.
- Spawn only exact pinned `pg_dump`/`pg_restore` executables directly, without shell or password in
  argv; bound stderr to 64 KiB, time out with kill/wait, and redact every public error/debug path.
- Publish backup output durably, atomically, and without replacement; cleanup only the exact
  operation-owned temporary object after verifying its identity.
- On Linux, backup destinations must have an immediate parent owned by the effective uid and not
  writable by group or others (`mode & 0o022 == 0`). The parent is opened with
  `O_DIRECTORY|O_NOFOLLOW`, and namespace operations remain relative to that pinned descriptor.
  Processes running under the same effective uid are trusted operators and are outside the AT-13
  hostile-writer model; this is an explicit deployment precondition, not an inferred guarantee.
- Authenticate and fully decrypt/validate the complete backup before sending plaintext to
  `pg_restore`; require a distinct fresh empty target and reject provider-epoch rollback first.
- Restore with exit-on-error/single-transaction semantics, then verify migration/RLS state,
  streams/hash chains/checkpoints/references, Evidence ciphertext/state, tombstones/receipts,
  projection state/watermarks, and provider metadata before returning an authenticated receipt.
- A crash or failure never overwrites an existing backup, selects a partial target, creates a
  plaintext dump file, changes the source, or returns fake success.
- Restore exclusivity must preserve the target database owner, effective ACL (including grantor and
  grant option), and exact prior connection limit. A provider-authenticated durable database marker
  binds operation ownership and that complete access contract so construction after interruption can
  recognize only a GraphHelm-owned partial target. The database comment is reserved for this marker; a
  preexisting non-GraphHelm comment is rejected unchanged. Delegated grant chains are validated and
  replayed in authority order, and valid quoted PostgreSQL role names are preserved. The
  administrative profile must be a PostgreSQL superuser so every authenticated grantor can be
  assumed during exact replay; lesser `CREATEDB` identities fail before marker reconciliation.
  A failed restore remains disabled and authenticated instead of being automatically renamed or
  dropped by database name; this leaves no usable partial target and requires manual authenticated
  recovery. If an authenticated quarantine phase already exists, recovery may create and finalize
  the disabled replacement only from authenticated identities, but it never automatically drops
  the quarantine. The final target becomes operational only after exact access restoration, marker
  removal, and ephemeral-role removal in one locked transaction. Later construction recognizes the
  exact finalized target plus residual disabled quarantine read-only. A copied marker on a
  replacement OID is rejected without deleting either database. If only the phase-one quarantine
  marker is durable and a target of the same name already exists without an authenticated
  replacement OID, the operator likewise preserves both databases and requires manual authenticated
  recovery. Under the stated hostile-superuser database model, PostgreSQL-local name/owner/flags
  cannot prove whether a same-name database was created by GraphHelm or forged after a crash, and
  check-then-DROP cannot prove the named quarantine is still the authenticated OID. Availability and
  automatic garbage collection are therefore subordinate to cleanup-only-owned safety.

## Process and acceptance

Every behavioral change follows an observed RED -> GREEN cycle. Required adversarial coverage is
the Task 10 matrix plus threat-model AT-10 through AT-13. PostgreSQL/tool tests are ignored and run
serially only with explicitly supplied disposable test infrastructure. Independent review order is
specification C0/I0, then quality/security C0/I0. Final delivery is one local commit with full gates,
no push/PR, and complete disposable-infrastructure cleanup.
