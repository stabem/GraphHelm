# Required final review - whole-branch findings

> **Correction rounds 1 to 6 applied.** Closed: C1, C2, I1-I6, I8, I10-I16.
> Still open: I7 and I9 only, both platform-blocked on this host. I9 was attempted and reverted in
> round 4; see that entry before retrying it.

Nine independent read-only reviews were dispatched against `8ee8f49..HEAD` (72 commits, 187 files),
one per dimension named in the plan's "Required Final Review" section. This file records every
finding for adjudication before merge.

Counts: **2 Critical, 16 Important, 27 Minor.**

Per the plan, Critical and Important findings require focused RED-to-GREEN correction and a fresh
re-review. Minor findings are adjudicated here.

## Verification status of this record

Findings marked **[verified]** were re-checked directly against the code by the orchestrator before
being recorded. Findings marked **[reported]** rest on the reviewing agent's evidence only. One
finding was **downgraded** on re-check and is recorded with the reason.

---

## Critical

### C1. Torn revocation journal permanently bricks the key provider [verified]

`adapters/sealed-key-provider/src/journal.rs:146`, `lib.rs:222`, `lib.rs:377`

`journal::load` rejects any non-newline-terminated tail with `KeyError::Integrity`.
`with_locked_state` calls `load` unconditionally on every operation, including `unwrap`. The only
`set_len` in the crate is at `lib.rs:1673`, inside the test module beginning at line 1624, so no
production repair path exists.

A crash between `write_all` and `sync_all` during `append_revocation` therefore leaves a partial
line and every subsequent `unwrap` fails, including for keys that were never revoked. Blast radius
is all sealed Evidence and every backup archive, permanently undecryptable.

The correction mirrors the event journal's documented behavior: a record that never received its
terminating newline was never durable, so truncating to the last newline discards nothing that was
ever acknowledged, and the surviving prefix remains HMAC-chain authenticated. Requires a RED test
for the recoverable torn tail and a second proving adversarial mid-file corruption still fails
closed.

### C2. Cleanup pairs evidence ids with foreign digests under any non-C collation [verified]

`adapters/postgres-event-store/src/retention.rs:537` and `:597`, `core/events/src/retention.rs:696`

`cleanup()` pairs `ids` with `digests` positionally via `ids.iter().zip(&digests)`. `ids` is sorted
in Rust byte order by `evidence_ids.sort()`. `digests` arrives from `ORDER BY e.evidence_id` with no
`COLLATE`, so it uses the database default collation. Under any non-C collation the two orders
diverge.

Each evidence id is then written with a different item's `ciphertext_sha256`, into both the cleanup
receipt and the `EvidenceCiphertextDeleted` event. Nothing rechecks after the per-row validation
loop, so it commits silently and surfaces much later as an `Integrity` failure during restore. This
corrupts the cryptographic-erasure audit trail.

---

## Important

### I1. The collation defect class was treated as instances, not a class [verified]

The `COLLATE "C"` correction applied during Task 11 covered only `SCHEMA_CONTRACT_QUERY` and
`PRIVILEGE_CONTRACT_QUERY`. Remaining uncollated text orderings in
`adapters/postgres-event-store/src/backup.rs`: line 3791 (`ORDER BY to_jsonb(t)::text`, hashed into
the manifest on the source and recomputed on the restored cluster, so a byte-identical restore is
rejected whenever the two clusters disagree on collation), plus lines 1865, 2124 and 4244 ordering
`text` scope columns. Lines 2628, 2679, 3577 and 3598 order `name`-typed `rolname`, which is
implicitly C-collated; the `COALESCE(...,'')` cases still need confirmation. C2 above is the same
class.

### I2. CI cannot observe the collation class at all [verified]

`ci/postgres.ps1:257` passes `--locale=C` to `initdb`. In C locale text ordering already equals
`COLLATE "C"`, so C2 and I1 pass green on both runners and fail only on a real-locale cluster. The
41/41 ignored-suite run recorded in the Task 11 ledger was structurally incapable of revealing the
defect class it was meant to confirm. A non-C-locale job is required for the guard to exist.

### I3. RLS predicate lacks an empty-scope guard [reported, mechanism verified]

`migrations/0001_event_evidence.sql:141-148`, identical at `0002:155` and `0003:54-61`

Policies compare against `current_setting('graphhelm.<scope>', true)` with no `NULLIF`, and no CHECK
constraint forbids empty ids. A transaction-local GUC reverts to `''` rather than unset, so on a
pooled connection an unscoped query degrades from matching nothing to matching `''`.
`is_opaque_id` (`core/protocols/src/persistence.rs:63`) rejects empty strings, so no such row can be
written through the typed API — the fail-closed property rests entirely on application validation.
`NULLIF(current_setting(...), '')` plus a length CHECK is the canonical fix.

### I4. `configure_runtime_role` revokes tables but never functions [reported]

`migrations/0003_projections.sql:151-159`. An operator who configured after `0001` and then upgraded
retains EXECUTE on superseded `SECURITY DEFINER` functions over `_sqlx_migrations`. Detected only at
backup time by `privilege_contract_is_safe`.

### I5. Retention serialization failures surface as opaque `Storage` with no retry [reported]

`adapters/postgres-event-store/src/retention.rs:632-638`, `:880-881`. Retention operations for
different executions of one project contend on a single `graphhelm_streams` row under
`FOR UPDATE` at REPEATABLE READ. The loser aborts with `40001`, which `storage()` maps to
`RetentionError::Storage`. No retry loop exists anywhere in the crate.

### I6. Local journal fails closed where the milestone doc promises truncation [verified]

`core/events/src/local.rs:1015` returns `Integrity` for any non-newline tail, while
`docs/milestones/production-event-evidence-store.md:63` states "only an interrupted non-newline tail
is truncated". `core/events/tests/local_atomicity.rs:872` codifies the fail-closed behavior. One of
the two is wrong and must be reconciled deliberately rather than by editing the doc to match.

### I7. Unix delete helpers are no-ops, so repositories accumulate orphans until unopenable [reported]

`core/events/src/local.rs:2814` and `:2830` validate inode identity and return `Ok(())` without
unlinking. Every append leaks a `.tmp` staging file, `reconcile_orphans` deletes nothing, and the
shared `DirectoryBudget` of `MAX_REPOSITORY_ENTRIES` (100,000) eventually makes `open` fail
`LimitExceeded` permanently. The Windows counterparts do delete, so this is a silent platform
asymmetry, and it also disables crash recovery of orphaned blobs.

### I8. Windows silently drops directory-entry durability [reported]

`core/events/src/local.rs:2758` swallows `InvalidInput`/`PermissionDenied` from `sync_all` on a
directory handle and returns `Ok(())`. `docs/milestones/production-event-evidence-store.md:119`
asserts publication is durable on both platforms.

### I9. Operator-config permission check is a no-op on Windows [verified]

`apps/cli/src/commands/events/config.rs:204`. The config names a PostgreSQL passfile and the keyring
directory. Unix rejects `mode & 0o077 != 0`; Windows accepts a world-readable config. The codebase
already contains a working ACL validator, `validate_windows_handle_dacl`
(`adapters/sealed-key-provider/src/lib.rs:829`), which is simply not called.

### I10. Projection size limit is asymmetric between write and read [reported]

`adapters/postgres-event-store/src/projection.rs:60` gates on compact JSON length; every read gates
on `octet_length(state::text)`, which `jsonb::text` renders longer. A state that inserts
successfully can become permanently unreadable and unsaveable.

### I11. Dangling reference can be created after erasure begins [reported]

`adapters/postgres-event-store/src/journal.rs:470` validates evidence relations from `record` without
inspecting `state`. An event referencing evidence already in `erasure_pending` is accepted and
committed, but `evidence::get` returns `Unavailable(ErasurePending)` forever.

### I12. Two plan-mandated diagnostic codes were never implemented [reported]

`GHE002_CORRUPT_BATCH` and `GHPROJ001_WATERMARK_MISMATCH` (plan lines 496 and 511) have zero `.rs`
occurrences. Both collapse into `GHE005_INTEGRITY_FAILURE`, so an operator cannot distinguish a
corrupt batch or a non-resumable projection from a hash-chain break.

### I13. `GHE004` and `GHE008` each carry two conflicting meanings [reported]

`core/events/src/store.rs:35` and `:38` versus `retention.rs:49` and
`core/governor/src/materialize.rs:32`. `docs/milestones/production-event-evidence-store.md:178`
declares this deliberate, but no ADR or decision-register entry records the narrowing. `GHE008`
conflates a transient storage fault with erased required content — opposite retry semantics.

### I14. Mandated pinned crypto dependencies replaced with bespoke code [reported]

The plan fixes `hmac = 0.13.0` and `base64 = 0.23.1`; neither appears in `Cargo.toml`. Both are
hand-rolled (`adapters/sealed-key-provider/src/lib.rs:1564`, `core/graph/src/persistence.rs:830`).
No defect was found in either, but substituting unaudited crypto for a mandated pinned crate is
precisely the deviation that requires a recorded decision. Related Minor: the hand-rolled HMAC
rejects keys longer than 64 bytes instead of hashing them as RFC 2104 section 2 prescribes; no
current caller passes one.

### I15. A Task 11 test cannot fail on the property it names [verified]

`apps/cli/tests/event_store_cli.rs:156`. `verify_rejects_out_of_range_sequence_windows` passes
`--repository`, so `verify_local` refuses any range with the same `GHCLI001_ARGUMENT_INVALID` and
exit code that the bounds check produces. Deleting the bounds checks at `verify.rs:64-72` leaves the
test green.

### I16. Threat model asserts a control that does not exist [reported]

`docs/security/EVENT_EVIDENCE_STORE_THREAT_MODEL.md:202` claims "Static inventory gates assert no
legacy importer, dual reader/writer, migration fixture, fallback branch, or runtime compatibility
symbol remains." No such gate exists; `source_invariants.rs` scans only for RLS and transaction
ordering.

---

## Downgraded on re-check

### Post-restore verification without `set_scope` [downgraded: Important -> Minor]

Reported as allowing restore to report success over an empty or tampered archive when the admin is
the table owner but not a superuser. `PostgresBackupOperator::new` checks `rolsuper` at
`adapters/postgres-event-store/src/backup.rs:949` and returns `InvalidBackup` for a non-superuser,
before `reconcile_interrupted_restore` and all verification. The premise cannot occur. What survives
is the reviewer's closing suggestion: assert explicitly that the verifying connection sees rows, so
a future refactor removing the superuser gate cannot silently reintroduce this.

---

## Minor

Recorded for adjudication; none block merge on their own.

1. `RawConfig` derives `Debug` while holding a DSN that may embed a password
   (`apps/cli/src/commands/events/config.rs:21`). Verified latent — never debug-formatted. The
   derive is gratuitous and should be dropped from the three raw deserialization structs.
2. Chunk plaintext is written before footer authentication inside `decrypt_pass`
   (`backup.rs:5533`). Neutralized by `verify_then_decrypt`, which completes a full authenticating
   pass to `std::io::sink()` before the second pass writes; verified at `backup.rs:5370`.
3. Hand-rolled HMAC rejects keys over 64 bytes instead of hashing them (RFC 2104 section 2).
4. The schema contract hash covers `pg_get_functiondef`, `pg_get_indexdef`, `pg_get_constraintdef`
   and `format_type` output, whose text rendering is not stable across PostgreSQL major versions. It
   fails closed, which is correct, but an upgrade will block backup and restore until the constant
   is recomputed. Not currently documented.
5. Backups are not portable across OS families: `create_restore_database_sql` replays the source
   `LC_COLLATE`/`LC_CTYPE` verbatim, so a Windows-taken backup names a locale no Linux cluster can
   create. Surfaces as a generic `InvalidRestore`.
6. `migrations/0002_retention.sql` was edited in place after being authored; any environment
   migrated at the intermediate commit is unstartable without manual ledger surgery. Acceptable
   pre-release, recorded because it contradicts append-only migration discipline.
7. `event_key` truncates SHA-256 to 96 bits for a UNIQUE idempotency key (~2^48 birthday bound).
8. Retention timestamps are `text` and `timestamp_wire` uses `SecondsFormat::AutoSi`, so
   `...:00.500Z` sorts before `...:00Z` and `pending`'s `ORDER BY requested_at` is not FIFO whenever
   fractional precision varies. Several timestamp columns also lack the `~ '^[0-9]{4}-.*Z$'` CHECK
   that sibling columns carry.
9. `env_clear()` preserves `SystemRoot` on Windows but nothing on Unix, so `pg_dump` from a
   non-default prefix cannot load `libpq.so.5`.
10. `pg_isready` is invoked by `ci/postgres.ps1` but omitted from its `$required` discovery set.
11. `ci/postgres.ps1` sorts version directories as strings, so `9.6` outranks `16` on a
    multi-version host.
12. `tests/run_mutations.ps1:13` uses a Windows-only path literal.
13. Duplicate-slot guard bypassed on the non-required `Unavailable` arm
    (`core/governor/src/materialize.rs:169`). Reachability of duplicate `slot_id`s is unverified.
14. `[published, applied]` recovery never exact-matches `published.evidence_refs`
    (`core/governor/src/apply.rs:663`).
15. Dead condition: `sequence < CHECKPOINT_INTERVAL` can never decide the branch
    (`integrity.rs:660`). Unreachable footer guard (`backup.rs:5492`).
16. `events verify --repository R --stream S` reports the wrong reason: `plan_range` runs
    `super::scope(...)` before the selector branch, so it demands `--workspace`/`--project` rather
    than saying range verification requires `--config` (`apps/cli/src/commands/events/verify.rs:61`).
17. Orphaned public accessors added by this milestone and never called:
    `CleanupReceipt::deleted_evidence_ids` (`core/events/src/retention.rs:773`) and
    `metadata_version` (`core/protocols/src/persistence.rs:359`). Invisible to `dead_code` because
    they are `pub` in library crates.
18. Pre-existing dead public methods `allows_transition` (`core/protocols/src/policy.rs:70`) and
    `is_io` (`core/governor/src/apply.rs:89`), both predating the merge base.
19. Six Portuguese strings survived the translation, all inside YAML code fences:
    `docs/graph-engineer/GRAPH_DSL_SPEC.md:16,131`,
    `docs/graph-engineer/GRAPH_ENGINEER_GUIDE.md:131,280`, and three `examples/graphs/*.yaml`. Three
    successive narrower regex sweeps missed them because the lines contain no `ção`, `não` or `é `.
20. Plan interface names diverge from the code: `EventStore`/`EventRepository`,
    `RepositoryError`/`EventRepositoryError`, `AppendRequest` flattened into `PreparedAppend`.
21. The plan's own Task 1 acceptance gate is permanently unsatisfiable: it requires the literal
    `'Evidence criptografada'` in `docs`, which the English-only rule removed. That string now
    survives only inside the plan.
22. Task 2 fix-round reports still describe `1.1.0` as published with no supersession notice,
    unlike the design specs which carry explicit banners.
23. CI embeds the random administrative password in `GRAPHHELM_TEST_ADMIN_URL`; disposable and
    loopback-only, masked in logs.

---

## Dimensions returning no Critical or Important

- **Schema and release integrity: clean.** All 30 catalog digests independently recomputed; 15
  schema files raw-byte identical between `schemas/` and `schemas/releases/1.0.0/`; 193 `$ref`
  occurrences all resolvable with no network retrieval; conformance 50/50; no `1.1.0` residue; all
  `p50.dev` identifiers preserved.
- **Legacy-removal: substantially clean.** Zero legacy symbols in code; zero `TODO`/`FIXME`/
  `unimplemented!`/`todo!` in source; every `#[ignore]` carries a documented infrastructure reason;
  fixed-string sweep for leaked tool-call scaffolding returns zero hits tree-wide.
- **Security: no exploitable path found.** Every `AssertSqlSafe` traced to a compile-time constant,
  a loop counter, or a validated allowlist; subprocesses use `env_clear()` with credentials only via
  `PGPASSFILE` and never argv; `SecretBytes` has no `Debug`, `Clone` or `Serialize`.
- **Cryptography: no break found.** Per-item random nonces with no reuse path; distinct domain
  separation strings per purpose; constant-time tag comparison; CSPRNG throughout; HMAC rather than
  bare `hash(key || data)`; authenticated append-only revocation chain.

## Explicitly cleared on request

`correlate_restored_cleanup` and its request-digest canary were audited by two reviewers. The
extraction is self-consistent, every receipt consumes exactly one event, leftovers are rejected, the
group digest is recomputed from persisted identity, and the canary fails if and only if the
`request_digest` comparison is weakened. The deliberate project-scope widening in
`verify_restored_cleanup` was verified sound: it relaxes only `execution_id`, keeps workspace and
project, runs a single further-constrained read, and restores the narrow scope before any subsequent
work. No defect found.

The Task 10 wrap-path admission classified Minor was verified honest: `SealedKeyProvider::wrap`
persists nothing, so a cancellation there exposes no plaintext, no usable target and no published
archive.

---

## Late addition: SDD report claim verification

A delegated sweep of all 11 task reports plus the four `task-2-fix-round-*` reports returned after
the specification-compliance review had already reported (which is why that review recorded it as
unverified). The large majority of concrete claims checked out exactly. Five discrepancies:

1. **Minor.** `task-2-fix-round-{3,4,5,6}` assert an entire world that no longer exists: catalog
   `14@1.1.0`, 17 event variants, a `LegacyEventsImported` receipt, conformance `48/48`, and nine
   named GREEN tests that appear nowhere in the tree. All of it was erased by the final Task 2
   reset, which is historically explainable, but none of those four files carries a supersession
   notice. Reality is `1.0.0`, 15 schemas, 16 event variants, 50 conformance cases, zero legacy
   symbols.
2. **Minor.** `task-6-report.md:74` claims Unix uses "identity-checked `unlinkat`". No `unlink`
   family symbol exists in `core/events/src/`. Independently corroborates I7. The report retracts
   this at line 110 but leaves the round-1 prose standing, so it contradicts itself 36 lines apart.
3. **Minor.** `task-4-report.md:46` and `:114` present `hmac 0.13.0` as a direct feature-controlled
   dependency. It exists only transitively via `hkdf` and `sqlx-postgres`. Corroborates I14. The
   report self-corrects at line 184, but the dependency-audit section remains false.
4. **Minor.** `task-9-report.md:46` says migration v3 "adds only disposable projection tables". It
   also replaces `graphhelm_configure_runtime_role` and issues `REVOKE CREATE ON SCHEMA public`.
   Corroborates I4.
5. **Minor.** `task-7-report.md:19` says failpoints are "absent from the production API unless the
   `test-support` feature is enabled". Only the setters are gated; the `AtomicU8`/`AtomicBool` state
   and the runtime branches compile unconditionally. The safety property holds because the branches
   are unreachable without the feature; the wording does not.

Claims resting on transient state (cargo pass/fail lines, mutation kills, container runs) could not
be verified statically and are recorded as such rather than accepted.

---

## Corrections applied

### C1 closed - torn revocation journal

RED: `interrupted_revocation_tail_is_discarded_and_the_provider_stays_usable` failed while both
guards passed. GREEN after the fix, with the guards still passing: 11 unit and 14 integration tests.

The first attempt truncated any unterminated tail and **broke an existing security guarantee** —
`corrupt_truncated_and_reordered_journal_fail_closed` began failing, because stripping a committed
record's trailing newline would have silently un-revoked a key. That attempt was discarded rather
than shipped.

The delivered fix discriminates by parseability: an interrupted write leaves truncated JSON that
cannot deserialize and is safe to discard, while a tail that still parses as a complete record is a
stripped terminator, which is tampering and fails closed. Both properties now hold simultaneously.

### C2 closed - collation-dependent digest pairing

The pairing was extracted into the pure `align_digests`, first implemented positionally to preserve
existing behavior. RED: `cleanup_digests_align_by_evidence_id_not_by_row_order` and
`cleanup_digests_reject_a_row_set_that_does_not_cover_every_requested_id` both failed. GREEN after
keying the lookup by evidence id: 20/20. Because `digests` is now aligned to `ids`, the second
positional zip that builds the deletion events is corrected by the same change.

### I1 and I2 closed - the class, not the instances

Every collation-sensitive text ordering now pins `COLLATE "C"` across `backup.rs`, `retention.rs`
and `integrity.rs`, including the `state_summary` cursor that is hashed on the source cluster and
recomputed on the restored one. Orderings on integer columns and on `name`-typed `rolname` were
verified collation-independent and left alone.

Two guards were added so the class cannot silently return:

- `tests/source_invariants.rs::text_orderings_pin_the_c_collation` fails if any `ORDER BY` over a
  collation-sensitive column omits `COLLATE "C"`. It found real violations in `retention.rs` and
  `integrity.rs` beyond those identified by review, and one flaw in its own clause extractor.
- `ci/postgres.ps1` accepts `GRAPHHELM_PG_LOCALE`, and a new `postgres-collation` CI job runs the
  suite under `en_US.UTF-8`. Verified locally against a real `English_United States.1252` cluster:
  `backup_restore` 2/2 and `retention` 10/10, exit 0.

### Verification of the correction round

- Full ignored PostgreSQL matrix through the stock script: **41 passed, 0 failed**, exit 0, with
  `backup_restore` 2/2 confirming `EXPECTED_SCHEMA_CONTRACT_SHA256` still matches.
- Same suite under a non-C collation: passing.
- Rustfmt, workspace Clippy `-D warnings`, and full workspace tests: clean.

---

## Corrections applied - round 2

### I15 closed - a test that could not fail

`verify_rejects_out_of_range_sequence_windows` now drives `--config` instead of `--repository`,
because against a local repository `verify_local` refuses any range with the same code the bounds
check produces. Proven by mutation: with the bounds checks deleted the revised test fails, and it
passes once they are restored. The `--repository` form stayed green under the same mutation.

### I10 closed - projection admission now uses the read's own measure

`save_generation` gated on compact JSON length while every read gates on
`octet_length(state::text)`, which PostgreSQL renders with separators and is therefore strictly
longer. A state between the two bounds inserted successfully and then became permanently
unreadable, which also blocked every later checkpoint for that generation. Admission now applies the
identical expression the reads use. Because the guard is a `WHERE` clause, `rows_affected` is
checked explicitly so a rejected state surfaces as `LimitExceeded` rather than a silent success.

### I11 closed - no new reference to Evidence under erasure

`validate_relations` selected only `record`, which survives the transition into `erasure_pending`,
so an event referencing Evidence already scheduled for deletion was accepted and committed. The
reference could never resolve afterwards. The lookup now also reads `state` and rejects any
reference to Evidence that is not `available`.

### I6, I8 and I16 closed - documentation reconciled to the code

In all three the code was correct and the documentation overclaimed, so the documentation was
corrected rather than the behavior changed.

- The milestone doc claimed the local JSONL repository truncates an interrupted non-newline tail. It
  fails closed instead, and now says so, with the contrast against the revocation journal made
  explicit: the journal can distinguish an interrupted write from a stripped terminator, and the
  JSONL repository has no equivalent discriminator today.
- The milestone doc asserted publication is durable on both platforms. NTFS exposes no directory
  fsync, so Windows guarantees the bytes but not the directory entry. The asymmetry is now stated.
- The threat model claimed static inventory gates assert the absence of legacy symbols. No such gate
  exists. The text now names the gates that do exist, both behavioral and schema-level, and records
  that source-level absence is maintained by review rather than enforced.

### Verification of round 2

Full ignored PostgreSQL matrix through the stock script: **41 passed, 0 failed**, exit 0. Rustfmt,
workspace Clippy `-D warnings`, and full workspace tests clean.

### Still open after round 2

I3 (RLS empty-scope guard, needs a new migration), I4 (`configure_runtime_role` function grants),
I5 (retention `40001` retry), I7 (Unix orphan reclamation), I9 (Windows config ACL), I12 and I13
(diagnostic code catalog), I14 (mandated pinned crypto dependencies).

I7 and I9 are platform-specific. I7 requires an identity-checked `unlinkat` on Unix and cannot be
exercised on this Windows host, so it is deliberately left for an environment that can verify it
rather than shipped unverified.

---

## Corrections applied - round 3

### I3 and I4 closed - migration `0004_scope_guard.sql`

Adding a fourth migration required recreating the arity-versioned ledger function, which made it the
natural place to close both findings at once.

I3. Every `graphhelm_scope` policy now compares through `NULLIF(current_setting(...), '')`. A
transaction-local GUC reverts to the empty string rather than becoming unset, so on a reused pooled
connection the old predicate degraded from matching nothing to matching every empty-scope row, and
its `WITH CHECK` twin would have permitted writing one. A `graphhelm_scope_not_empty` CHECK on each
of the 16 scoped tables makes the invariant structural as well. `execution_id` is deliberately
excluded from `NULLIF`, because the empty string is its legitimate value for a scope with no
execution and wrapping it would reject every such row.

I4. `graphhelm_configure_runtime_role` now revokes `ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA public`
before granting, so it is idempotent across migration levels. The superseded three-argument ledger
function is dropped rather than left as an executable `SECURITY DEFINER` reader of
`_sqlx_migrations`.

`EXPECTED_SCHEMA_CONTRACT_SHA256` moved from `9d582be2...` to
`48ef7a4253e4d2c2d9c5502c683cfc0e559183e0770c28211d0b64a550b5ebd2`, recomputed against a real
cluster by two independent constructions of the four-migration schema that agreed exactly.

A new isolation test asserts the structural half from an identity that bypasses RLS entirely, which
is the only way an empty-scope row could otherwise be created. The pre-existing isolation tests do
not cover this: on a fresh connection the GUC is genuinely unset, so `= NULL` already matches
nothing and they pass with or without the fix.

### Four integration defects the compiler could not catch

Each was found only by running the migration end to end, and each would have shipped silently:

1. `migrate()` builds its `Migrator` from an explicit vec rather than discovering the directory, so
   `0004` was never applied while `verify_migration` already required it.
2. `tests/migration.rs` injected a bogus `_sqlx_migrations` row at version 4 to simulate unknown
   history. Version 4 is now real, so the insert collided; it moved to version 5, preserving intent.
3. `backup.rs` both calls and revokes the ledger function by signature on the restore path, still
   pointing at the dropped three-argument form. Left unfixed, migration `0004` would have silently
   broken restore.
4. `tests/backup_restore.rs` deliberately drifts the `graphhelm_events` policy to assert rejection
   and then restores it, but restored the pre-`0004` definition. That left the schema contract
   permanently drifted for the remainder of the test, and a hardcoded `migration_count == 3`
   assertion in the same file also had to become 4.

### Verification of round 3

Full ignored PostgreSQL matrix through the stock script: **42 passed, 0 failed**, exit 0 - the
previous 41 plus the new empty-scope guard test. Rustfmt, workspace Clippy `-D warnings`, and full
workspace tests clean.

### Still open after round 3

I5 (retention `40001` retry), I7 (Unix orphan reclamation), I9 (Windows config ACL), I12 and I13
(diagnostic code catalog), I14 (mandated pinned crypto dependencies).

I7 remains deliberately deferred: it needs an identity-checked `unlinkat` on Unix and cannot be
exercised on this Windows host, and shipping unverified deletion logic into the crash-recovery path
is the wrong trade.

---

## Corrections applied - round 4

### I5 closed - serialization failures are no longer terminal

`storage()` discarded the sqlx error and always returned `RetentionError::Storage`. Retention runs at
REPEATABLE READ and takes `FOR UPDATE` on a single per-project stream row, so two operations against
different executions of one project genuinely contend; the loser aborts with SQLSTATE `40001`, and a
deadlock aborts with `40P01`. Both were flattened into a terminal storage fault, so a caller had no
way to know a retry was the correct response.

Both are now classified as `RetentionError::Conflict`, which already carries exactly that meaning
for durable-state contention and is how `constraint()` already classifies unique violations. The
decision is isolated in the pure `retryable_sqlstate` and unit tested against both retryable codes
and seven terminal ones.

`committed_hold_wins_a_prepare_snapshot_race` asserted `Storage` for the losing side and had to be
updated. That test encoded the defect rather than a property worth keeping: its own next line
retries and reaches `LegalHold`, the correct terminal answer, which is the proof that the race
outcome was retryable contention all along.

### I9 attempted and reverted - the reported fix does not work

The finding observed that `validate_windows_handle_dacl` already exists and is not called by the
operator-config loader. Wiring it in - through a new shared `require_owner_only_access` primitive
covering both platforms - made every valid configuration fail on Windows.

The cause is that the validator requires a protected, owner-only DACL. The sealed keyring satisfies
it because it creates its own files that way. A configuration authored by an operator in a text
editor inherits Administrator, SYSTEM and user entries and can never satisfy it, so the change was a
denial of service dressed as a hardening. It was reverted rather than shipped or worked around.

I9 therefore is not a wiring gap. Closing it needs a different and weaker rule for operator-authored
files: reject broad principals such as `Everyone`, `Authenticated Users` and `Users`, while
tolerating the owner, `Administrators` and `SYSTEM`. That is real ACL enumeration work.

Worth recording for whoever picks it up: the targeted test for this finding passes either way. It
grants `Everyone:R` and asserts rejection, and a broken implementation rejects everything. The
defect surfaced only because unrelated tests exercising the ordinary path went red.

### Verification of round 4

Full ignored PostgreSQL matrix: **42 passed, 0 failed**, exit 0. Rustfmt, workspace Clippy
`-D warnings`, and full workspace tests clean.

### Still open after round 4

I7 (Unix orphan reclamation), I9 (rescoped above), I12 and I13 (diagnostic code catalog), I14
(mandated pinned crypto dependencies). I7 and I9 both require a platform this host cannot verify
against. I14 is a decision about whether to adopt the pinned crates or record the deviation in an
ADR, and belongs to the owner rather than to a correction round.

---

## Corrections applied - round 5

### I14 closed - deviation recorded, and the real defect fixed

Two halves.

The defect: the hand-rolled `hmac_sha256` rejected keys longer than the 64-byte block instead of
hashing them down as RFC 2104 section 2 requires. Every current caller passes a 32-byte derived key
so nothing failed, but a future caller with a longer key would have received `Invalid` rather than a
correct tag. It now hashes the key first. The function had no test at all; it is now pinned to RFC
4231 case 2 and case 6, the latter being exactly the over-long-key path.

The deviation: recorded as **D-038** rather than reversed. The plan pins `hmac = 0.13.0` and
`base64 = 0.23.1` and neither is declared. Adopting them now would change the byte output that
authenticated receipts, checkpoints and the revocation journal already depend on, and the in-tree
HMAC keeps its pads, subkeys and intermediate digests in `Zeroizing` in a way a crate boundary would
not guarantee. Both implementations are small, closed, and now standard-pinned. The decision records
that adopting the crates stays open but must be a deliberate, separately verified change rather than
a silent substitution in either direction.

### Not attempted in this round

I7, I9, I12 and I13 remain open. I7 needs an identity-checked `unlinkat` and I9 needs Windows ACL
enumeration; neither can be exercised on this host, and round 4 already demonstrated the cost of
shipping a platform fix that cannot be run - the I9 attempt passed its own targeted test while
breaking every ordinary path. I12 and I13 add and disambiguate public diagnostic codes, which is a
wire-contract change to `EventRepositoryError` and should be adjudicated before it is made.

---

## Corrections applied - round 6

### I12 closed - the two missing codes now exist and are reachable

`GHE002_CORRUPT_BATCH` and `GHPROJ001_WATERMARK_MISMATCH` were mandated by the plan and never
implemented; both collapsed into `GHE005_INTEGRITY_FAILURE`.

`EventRepositoryError` gains `CorruptBatch` and `WatermarkMismatch`. The local repository now
separates a batch that fails its own checksum from a line that fails canonical or format validation,
and the projection rebuilder reports a stored generation that does not describe the requested one as
a watermark mismatch rather than a chain break. Those are different failures with different recovery
paths, which is the discrimination the catalog exists to provide.

`GHE002_CORRUPT_BATCH` is covered by a new test that tampers with a committed batch's checksum and
asserts the code on reopen. That test was written because the change initially had none: the
workspace suite passed with the new variants in place, which proved only that nothing exercised
them. A code no path produces is the same defect class as a test that cannot fail.

`GHPROJ001_WATERMARK_MISMATCH` is **not** covered by a test. Reaching it requires a stored projection
generation that disagrees with the request, which the PostgreSQL projection suite does not currently
construct. It is recorded here as unproven rather than presented as verified.

### I13 closed - one code, one meaning

`GHE004` named both `INVALID_EVENT` (repository) and `SCOPE_VIOLATION` (retention); `GHE008` named
both `STORAGE_FAILURE` (repository) and `CONTENT_UNAVAILABLE` (materialization). A caller
prefix-matching `GHE008` could not distinguish a transient storage fault from erased required
content, which have opposite retry semantics.

The repository codes have twenty consumers and each colliding code had exactly one other definition
site, so the single-site ones moved: `GHE011_SCOPE_VIOLATION` and `GHE012_CONTENT_UNAVAILABLE`. This
deviates from the plan's specific numbering, which had assigned `GHE004` and `GHE008` to those two
meanings, but the plan's own catalog already conflicted with the established repository codes. Fewer
consumers move, and no number now carries two meanings.

A sweep over every `GH*` code in `core/`, `adapters/` and `apps/` confirms no remaining duplicate,
with one pre-existing exception outside this finding's scope: `GHP001` names both `GHP001_SAFE` and
`GHP001_STRUCTURAL_IMPOSSIBILITY` in the policy family. That predates this milestone and was not
raised by review; it is recorded here so it is not lost.

The milestone doc's diagnostic catalog was updated, including removing its claim that codes are
"stable but not uniquely owned by one error type", which is no longer true.

### Verification of round 6

Rustfmt, workspace Clippy `-D warnings`, and full workspace tests clean. The ignored PostgreSQL
matrix was not re-run for this round; the change is to error classification in `core/events` and the
milestone doc, and no SQL was touched.

---

## Correction to the merged commit message

The squash commit `d1d8b24` carries, from branch commit `96b7a89`, the sentence:

> The gate has not yet completed a full green run; that is the next thing to establish.

**That statement is false as of the merge.** It was true when written, and was superseded minutes
later by the very thing it asked for.

`ci/gate.ps1` completed **three consecutive green runs** on `96b7a89`, the exact commit merged. Each
reached `[gate] GREEN - every stage passed`, executed both PostgreSQL passes - the C locale and
`English_United States.1252` - for **84 ignored tests per run**, with zero test failures.

The earlier red that prompted the sentence was not a flaky test. It was the `$IsWindows` defect: a
PowerShell Core-only variable that, under `Set-StrictMode` on Windows PowerShell 5.1, raises a
terminating error and aborted the run at the non-C collation stage. The PostgreSQL failure observed
alongside it was collateral from that teardown, not a genuine defect in
`admin_operator_binds_pool_profile_and_source_identity`, which has since passed six consecutive
times.

The commit message is left unedited. Rewriting a published merge commit on the default branch would
require a force push, would invalidate every existing clone and worktree, and would destroy the
audit trail this record exists to preserve. An append-only correction is the correct remedy for a
project whose own event store is append-only.
