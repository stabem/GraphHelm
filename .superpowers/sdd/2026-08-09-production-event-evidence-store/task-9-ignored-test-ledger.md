# Task 9 ignored PostgreSQL test ledger

All ignored tests require only the explicitly supplied disposable `GRAPHHELM_TEST_ADMIN_URL`. They are executed serially against a temporary local PostgreSQL container and the container is removed before completion.

| Suite | Count | Task 9 relevance |
|---|---:|---|
| concurrency | 3 | source append/read serialization baseline |
| isolation | 3 | exact composite scope and RLS baseline |
| migration | 3 | 16 scoped tables, migration v3 ledger/checksum, runtime immutability |
| projection | 4 | generation resume/swap, stale-head preservation, RLS/immutability/corruption, unsupported format, forged-state rejection |
| repository_conformance | 16 | authenticated source events and bounded replay source |
| retention | 10 | availability projection inputs and prior Task 8 regression |
| **Total** | **39** | all GREEN with `--ignored --test-threads=1` |

The only legacy-test expectation changes were explicitly authorized: table count `14 -> 16` and synthetic unknown migration version `3 -> 4`.
