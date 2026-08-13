# Task 10 ignored PostgreSQL/tool test ledger

All ignored tests require explicitly supplied disposable infrastructure. Task 10 additionally
requires exact pinned `pg_dump` and `pg_restore` paths. The final local run used PostgreSQL 16.9 in
the disposable `graphhelm-task10-pg` container and EDB PostgreSQL client tools 16.14.

| Suite | Count | Task 10 relevance |
|---|---:|---|
| backup_restore | 2 | encrypted real dump/restore plus bounded constructor reconciliation under catalog lock |
| concurrency | 3 | append/read snapshot and exact retry baseline |
| isolation | 3 | composite scope and RLS baseline |
| migration | 3 | migration ledger and runtime-role baseline |
| projection | 4 | authenticated disposable projection baseline |
| repository_conformance | 16 | chains, checkpoints, Evidence, references, bounds, and retries |
| retention | 10 | holds, revocation, receipts, tombstones, cleanup, and reconciliation |
| **Total** | **41** | all passed serially with `--ignored --test-threads=1` |

Pinned tool evidence:

- `pg_dump (PostgreSQL) 16.14`, SHA-256
  `c2765fc559bdc8e2f71a6d408ed024a984c5140e448f797a6f15168355b62133`.
- `pg_restore (PostgreSQL) 16.14`, SHA-256
  `cd865f0d32fe5da3de51fcd6012faa75f3f4a8a17c6bbbdca960f5dad8e3d4a4`.

The container, temporary databases/roles, and portable client-tool directory are test-only and are
removed before Task 10 completion.

## Status against the final diff

The 41/41 run above predates the `correlate_restored_cleanup` extraction and its request-digest
canary. Those 41 tests were not re-run afterwards: the disposable container was already removed and
the Docker daemon could not be started on this host in that session, so they stayed correctly
ignored rather than silently passing. The pinned 16.14 client tools are still present under
`target/task10-tools`, so the suite can be reproduced by recreating the `graphhelm-task10-pg`
container and re-exporting `GRAPHHELM_TEST_ADMIN_URL`, `GRAPHHELM_TEST_PG_DUMP` and
`GRAPHHELM_TEST_PG_RESTORE`. This re-run is required before Task 10 is declared closed.
