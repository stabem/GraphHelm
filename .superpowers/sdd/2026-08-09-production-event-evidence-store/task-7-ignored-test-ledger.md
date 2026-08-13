# Task 7 ignored PostgreSQL test ledger

These tests are ignored in ordinary runs because they require an isolated PostgreSQL administrator URL. They have no Docker dependency; any disposable PostgreSQL 17 runtime is sufficient.

Canonical execution:

```powershell
$env:GRAPHHELM_TEST_ADMIN_URL='postgresql://postgres:postgres@127.0.0.1:55432/postgres'
cargo +1.97.1 test -p graphhelm-postgres-event-store --all-features --locked -- --ignored --test-threads=1
```

The 2026-08-11 disposable PostgreSQL 17 run passed 25/25:

- concurrency (3): single same-stream winner; concurrent identical retry returns original envelopes; one repeatable snapshot while an append commits;
- isolation (3): cross-scope identical IDs; absent/wrong-scope direct SQL invisibility; pool commit/rollback/error/cancel scope reset;
- migration (3): FORCE RLS/non-owning runtime role; idempotent ledger plus least privilege and immutable-history trigger; startup rejection of unknown/failed migration history;
- repository conformance (16): atomic Event/Evidence rollback; exact/divergent retry; missing/foreign reference rollback; cursor authentication/binding; chain corruption and retry rejection; deleted tail/head mismatch; empty stream plus authenticated head tamper rejection; authenticated checkpoint plus rehashed-head rejection; checkpoint rejection of a corrupt prefix; authenticated-head rejection of a fully rehashed checkpoint suffix plus database head; SQL-side oversized-envelope rejection before client JSON materialization; full-page preservation across multiple bounded internal chunks; bounded `activeGraph` checkpoint round trip, authenticated successor lineage, half-null constraint and tamper rejection; 100,001 pre-SQL bound; orphan prepared Evidence rejection; persisted Evidence/artifact read and reuse tamper rejection.

Every test creates a random database and login role, migrates it, configures exact runtime grants, and removes both on success. Tests run serially to keep administrator lifecycle and mutation evidence deterministic.

`adapters/postgres-event-store/tests/run_mutations.ps1` is the reproducible mutation gate. It requires the same administrator URL, executes the eight normative removals (FORCE RLS, transaction-local scope, idempotency-first ordering, stream lock, atomic commit, event-hash verification, cursor binding and checkpoint authentication), rejects any survivor, and restores the exact original bytes even when a command fails.

Authenticated-head verification was also neutralized independently. The rehashed-suffix fixture then returned the forged `EventPage` instead of `GHE005`, killing the mutation; the exact implementation was restored and the focused test returned GREEN.

Artifact locator/content-digest revalidation was neutralized independently. The locator-only tamper then returned an invalid `ArtifactReference`, killing the mutation; the shared core validator was restored and the focused test returned GREEN. SQL source invariants additionally assert every multi-row Event JSON path uses bounded internal chunks and a SQL-side per-row transport guard before client JSON materialization.
