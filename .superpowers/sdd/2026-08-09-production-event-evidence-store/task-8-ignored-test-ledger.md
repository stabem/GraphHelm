# Task 8 ignored PostgreSQL test ledger

These tests require a disposable PostgreSQL 17 administrator URL and are ignored in ordinary credential-free runs.

Canonical execution:

```powershell
$env:GRAPHHELM_TEST_ADMIN_URL='<disposable-local-admin-url>'
cargo +1.97.1 test -p graphhelm-postgres-event-store --all-features --locked -- --ignored --test-threads=1
```

The 2026-08-11 disposable run passed 35/35:

- concurrency (3): same-stream winner, concurrent exact retry, repeatable read snapshot;
- isolation (3): cross-scope identical IDs, absent/wrong-scope SQL visibility, pooled scope reset;
- migration (3): FORCE RLS and role separation, exact two-migration ledger plus immutable history, unknown/failed version rejection;
- repository conformance (16): the complete Task 7 atomicity, integrity, cursor, checkpoint, bound and tamper matrix;
- retention (10): migration/RLS/least-privilege completeness; prepare-before-revoke, exact finalize and tamper-resistant retry; provider failure plus restart reconciliation and late-hold rejection; authenticated append-only hold placement/release; forged prepared-receipt rejection before pending state; an orchestrated REPEATABLE READ row-lock race proving a hold committed before prepare prevents erasure; concurrent prepare/finalize with one durable outcome; recreation after provider revoke but before finalization without a second logical revoke; deterministic rejection before a nonzero cleanup delay; and bounded rejection of 20,001 hold-history rows before authentication work.

Each test creates a random database and runtime role, applies both exact migrations, runs with transaction-local scope, and removes its owned resources. Tests run serially. The containing PostgreSQL container is disposable test infrastructure and is removed after the final gate.
