# Task 11 ignored PostgreSQL/tool test ledger

All ignored tests require explicitly supplied disposable infrastructure. Task 11 replaces the
container used in Task 10 with `ci/postgres.ps1`, which creates a throwaway cluster from an already
installed PostgreSQL. No Docker and no service container is involved.

The final local run used the portable EDB PostgreSQL 16.14 distribution (server and client tools)
via `GRAPHHELM_PG_BIN`, on Windows.

| Suite | Count | Task 11 relevance |
|---|---:|---|
| backup_restore | 2 | encrypted dump/restore and bounded constructor reconciliation under catalog lock |
| concurrency | 3 | append/read snapshot and exact retry baseline |
| isolation | 3 | composite scope and RLS baseline |
| migration | 3 | migration ledger and runtime-role baseline |
| projection | 4 | authenticated disposable projection baseline |
| repository_conformance | 16 | chains, checkpoints, Evidence, references, bounds, and retries |
| retention | 10 | holds, revocation, receipts, tombstones, cleanup, and reconciliation |
| **Total** | **41** | all passed serially with `--ignored --test-threads=1` |

Observed result of the complete run through the stock script:

```text
test result: ok. 2 passed; 0 failed; ... finished in 86.52s
test result: ok. 3 passed; 0 failed; ... finished in 2.78s
test result: ok. 3 passed; 0 failed; ... finished in 2.33s
test result: ok. 3 passed; 0 failed; ... finished in 2.36s
test result: ok. 4 passed; 0 failed; ... finished in 3.03s
test result: ok. 16 passed; 0 failed; ... finished in 12.14s
test result: ok. 10 passed; 0 failed; ... finished in 8.38s
[ci/postgres] Test command exited with code 0.
```

41 passed, 0 failed. The cluster processes and the temporary directory were confirmed absent
afterwards.

## Task 10 carry-over discharged

The Task 10 ledger recorded that its 41/41 run predated the `correlate_restored_cleanup` extraction
and its request-digest canary, and that the suite had not been re-run against the final diff. That
obligation is now closed: the run above executes the complete ignored matrix against the current
tree, including the restored three-way receipt/event comparison, and it was observed directly rather
than reported by a delegated run.

## Defects found while making the suite reproducible

The Task 10 constant and the first draft of the CI script could never have passed together. Four
distinct problems were found and fixed:

1. `SCHEMA_CONTRACT_QUERY` and `PRIVILEGE_CONTRACT_QUERY` sorted text keys under the database
   default collation. Object names are full of `_` and `.`, which collate differently across
   platforms, so `EXPECTED_SCHEMA_CONTRACT_SHA256` was pinned to whichever server computed it.
   Every text sort key now carries `COLLATE "C"` and the constant was recomputed to
   `9d582be267da4a77e20bbace4a68981c041c4221554f9037299ac85e8bdeb934`.
2. The script created a random administrative role, but the hostile-superuser recovery test asserts
   the conventional `postgres` superuser owns a database created through the admin connection. The
   administrative role is now `postgres`; isolation comes from the private data directory and the
   random port.
3. `pg_ctl start` invoked through the PowerShell native pipeline hung after the server was already
   accepting connections: the detached server inherits stdout and stderr, so the stream never
   reaches EOF. `Start-Process -Wait` hangs for the same reason at one remove, because it waits on
   descendants and the server never exits. The script now launches without waiting and polls
   `pg_isready`.
4. `cargo` writes all progress to stderr. Under Windows PowerShell 5.1 a redirected native stderr
   line becomes a `NativeCommandError`, which the script-wide `$ErrorActionPreference = 'Stop'`
   promotes to a terminating error, so redirecting the script's output aborted the run before the
   first test executed. The test invocation now runs under `Continue` and is judged by its exit
   code.

Reproduce with `GRAPHHELM_PG_BIN` pointed at a PostgreSQL `bin` directory containing `initdb`,
`pg_ctl`, `postgres`, `pg_isready`, `psql`, `pg_dump`, and `pg_restore`:

```powershell
./ci/postgres.ps1
```
