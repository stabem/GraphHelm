# Task 10 report - encrypted backup and verified restore

## Authority and scope

- Implemented from clean Task 9 commit `9bc2d9b3d5f9006c6b48494a144af476bbfc587e` under issue `#5`.
- The user authorized the Task 10 plan section as a substitute brief, the five exact pinned
  workspace dependencies, the minimum checkpoint/retention owner-module verification reuse, and
  the cleanup receipt `requested_at` migration field needed for canonical event correlation.
- No other migration/schema or core protocol, CLI/Task 11, compatibility reader, production
  credential, push, or PR was added.

## Delivered behavior

- `GHBAK001` streams custom-format `pg_dump` bytes through ordered 1 MiB
  XChaCha20-Poly1305 chunks, with checked 64 GiB plaintext/archive/chunk bounds, zeroizing key and
  plaintext buffers, authenticated header/footer, and exact provider-epoch equality.
- The authenticated manifest binds source identity, exact tool versions and SHA-256 digests,
  migration/schema/privilege contracts, catalog, provider metadata, counts, authoritative state
  summary, ciphertext digest, and chunk/byte totals.
- Executables are opened and hashed before use, invoked directly without a shell or password in
  argv, drained through bounded output readers, and owned by a cancellation-safe watchdog. Unix
  process groups and Windows kill-on-close Job Objects terminate descendants and reap the child.
- Backup publication is no-replace and durable. Linux publishes an anonymous retained inode with
  `linkat` relative to a pinned immediate-parent descriptor; the parent must be effective-uid owned
  and not group/other writable. Same-effective-uid processes are an explicit trusted deployment
  boundary. Windows denies source replacement, checks destination file identity, deletes by handle,
  and flushes the directory. Existing destinations are never overwritten.
- Restore authenticates and fully decrypts the same retained archive handle before streaming to
  `pg_restore`, accepts only a fresh distinct target, compares both authenticated passes exactly,
  and preserves any failed partial target disabled behind its authenticated marker. Exclusivity uses a durable
  provider-authenticated ownership marker and a temporary connection limit without changing the
  target ACL; success and recovery restore the exact prior owner, effective ACL and limit. A
  preexisting database comment is rejected unchanged because that field is reserved for the
  authenticated recovery marker. ACL replay validates the grant graph before mutation, restores
  grant-option authorities before their delegated grants, and supports quoted PostgreSQL role names.
  The constructor requires a PostgreSQL superuser administrative identity before reconciliation;
  a `CREATEDB` owner without that capability is rejected with its database unchanged.
  No failed target or quarantine is automatically renamed or dropped by database name. A failed
  target stays disabled for manual authenticated recovery. If an authenticated quarantine phase is
  already present, recovery can finalize only the identity-bound disabled replacement, restores its
  exact access contract, clears its marker and removes the ephemeral role, while preserving the
  disabled authenticated quarantine for manual garbage collection. A later constructor recognizes
  that exact terminal pair read-only. Quarantine phase one plus an existing target without an
  authenticated replacement identity is preserved for manual recovery: automatically adopting its
  observed OID would let a hostile PostgreSQL superuser substitute a database in the crash interval.
  Copied markers and unproven same-name targets fail without touching either database.
- Post-restore selection verifies the exact migration, schema/RLS/policy/trigger/function and grant
  contracts; all bounded event chains and checkpoints; references and Evidence state/ciphertext;
  provider-authenticated retention authorities, holds, receipts, tombstones and cleanup; and an
  independent projection rebuild. Cleanup receipts reconstruct their canonical request digest from
  persisted scope/operation/idempotency/request time/evidence IDs and must map bijectively to the
  matching `EvidenceCiphertextDeleted` events. Only then is a serializable, independently
  verifiable restore receipt returned.

## RED -> GREEN and adversarial evidence

- Missing pinned-tool/profile/codec/operator contracts first failed at compile time, then gained
  closed bounded constructors and redacted error/debug surfaces.
- Tamper, reorder, truncate, extend, tiny/noncanonical chunks, header/footer swap, wrong key/tool,
  provider epoch rollback/advance, archive replacement between passes, and manifest divergence all
  fail before a receipt.
- Fake process tests cover direct invocation, metacharacter arguments, nonzero/premature exit,
  noisy output truncation, timeout, future cancellation, descendant survival, and exact pinned
  executable replacement.
- A real catalog-lock regression holds `pg_database` exclusively and proves constructor endpoint
  verification/reconciliation returns within its configured timeout instead of hanging.
- Filesystem tests cover existing-destination preservation, operation-owned temporary cleanup,
  source-name replacement, destination identity, crash-stage publication, directory durability,
  and Linux acceptance of owner-only/owner-group-readable parents while rejecting group/other
  writable parents.
- Real PostgreSQL tests cover a nonempty source with available and erased Evidence, finalized
  retention/tombstone/cleanup, authenticated checkpoints, active projections, exact grants/RLS, and
  receipt transport/reverification. Authenticated corrupt schema, checkpoint, projection, retention,
  same-count state, and post-restore receipt-provider failures fail closed with a disabled,
  authenticated partial target that requires manual recovery.
- An adversarial cleanup archive changes receipt operation, idempotency and request digest while
  retaining the original event; restore rejects it before selection. PostgreSQL 17 also applies the
  updated retention migration ledger successfully.
- Isolating the receipt/event binding required a dedicated mutation. Neutralizing only the physical
  `graphhelm_events.request_digest` comparison left the end-to-end adversarial archive scenario
  green, because that tampered archive is already refused by a different post-restore invariant.
  Global safety therefore held, but the new binding was not independently proven. The correlation
  was extracted into the pure, database-free `correlate_restored_cleanup`, and a unit canary now
  covers exactly that comparison: a receipt whose ciphertext digest, deletion timestamp, Evidence
  state, tombstone and recomputed canonical digest are all valid, paired one-to-one with an event
  persisted under a foreign `request_digest`. With the comparison neutralized the canary observed
  `Ok(())` and failed; with the comparison restored it returns `Integrity` and passes, alongside a
  positive control proving consistent data still correlates. `request_digest` is the right subject
  for this canary because it is the one field outside the envelope hash chain, so it is the field an
  archive can rewrite.
- The key-provider wrap operation may leave an unreachable wrapped-key handle if the process is
  cancelled after custody registration but before publication. This is a bounded Minor custody
  garbage-collection concern; no plaintext, usable target, or published archive is exposed.

## Fresh verification evidence

Recorded before the correlation extraction and canary:

- PostgreSQL ignored serial suite: 41/41 passed (2 backup/restore, 3 concurrency, 3 isolation,
  3 migration, 4 projection, 16 repository conformance, 10 retention).
- Linux-target Clippy was attempted and stopped in `ring` only because this Windows host lacks the
  external `x86_64-linux-gnu-gcc` compiler; native Windows workspace Clippy passed.

Re-run on the exact final diff, after restoring the neutralized comparison:

- New `retention::tests` canary pair: RED with the comparison neutralized
  (`called Result::unwrap_err() on an Ok value`), GREEN once restored; 2/2 passed.
- Workspace Rustfmt and Clippy with `-D warnings`: passed.
- Workspace all-feature tests and doc tests: passed with zero failures; the 41 PostgreSQL tests
  remained correctly ignored without supplied infrastructure.
- CLI smoke: 10/10 passed; locked metadata succeeded.
- Canonical validate/lint/hash/simulate/replay commands passed; canonical graph hash remained
  `sha256:ca0484b161029b3cdded19d872665a319a608e240bb66964864ee2ef33bf92c6` and replay completed.
- `git diff --check` passed.

Outstanding verification gap:

- The 41 ignored PostgreSQL tests were **not** re-run against this final diff. The disposable
  container was removed at the earlier Task 10 completion and the Docker daemon could not be
  brought up on this host in this session; the pinned 16.14 client tools remain in
  `target/task10-tools`. The extraction is behavior-preserving and the restored comparison is
  strictly stricter, and the earlier 41/41 run is known not to discriminate this binding either way
  — that is precisely why the canary exists — but an end-to-end re-run is still owed before Task 10
  is declared closed.
- Final independent specification and quality/security verdicts are recorded after their read-only
  re-review of this exact final diff.

## Rollback and exclusions

- Rollback is the single local Task 10 commit. No authoritative source row, schema, migration, or
  prior wire contract is changed by the implementation.
- Operator CLI, scheduling, remote object storage, key-handle reconciliation, compatibility
  readers, hosted infrastructure, and deployment remain out of scope.
