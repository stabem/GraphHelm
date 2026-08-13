# Task 9 report — safe replay projections

## Authority and scope

- Implemented from clean Task 8 commit `53aa94dd722db214179fb86b7df9aa6a7f31ff65` under issue `#5`.
- The user explicitly authorized the Task 9 plan section as the substitute brief because no standalone brief existed.
- The user separately authorized only two mechanical legacy-test updates in `adapters/postgres-event-store/tests/migration.rs`: table count `14 -> 16` and synthetic unknown migration `3 -> 4`.
- No dependencies, protocol/schema/catalog/golden contracts, Task 10/11 code, push, or PR were added.

## Delivered behavior

- Generation-based disposable execution projections with an exact scope/stream/name/version/generation watermark and bounded incremental replay.
- Resume across persisted page boundaries, source-head catch-up, source-head recheck, and active-generation swap that preserves the old active generation on failure.
- PostgreSQL insert-only checkpoints, guarded active pointers, FORCE RLS, least-privilege grants, migration checksum verification, bounded JSON transport, and deterministic replay before any stored generation is trusted.
- `ExecutableGraphMaterializer` resolves exact scoped Evidence bindings, validates canonical content digests, decrypts only through `EvidenceOpener`/`KeyProvider`, keeps executable JSON in zeroizing buffers, redacts debug output, fails required unavailable content with `GHE008_CONTENT_UNAVAILABLE`, and preserves optional unavailability explicitly.

## RED -> GREEN and behavioral mutations

- Missing projection generation/watermark/rebuild contracts: compile RED `E0432` -> checked constructors and object-safe rebuild/storage interfaces.
- Invalid page resume and duplicate page: sequence/hash/idempotency mutation rejected as `GHE005_INTEGRITY_FAILURE`.
- Empty source generation was not saved before activation: behavioral RED -> initial checkpoint is durably saved before swap.
- Source head advancing between observed head and swap: interleaved RED fixture -> rebuild catches up to the new head before swap.
- Swap failure mutation: old active generation remains unchanged.
- Projection version/generation overflow mutation: constructors now reject values outside PostgreSQL/wire-safe bounds.
- Required erased Evidence incorrectly treated as executable: RED -> `GHE008_CONTENT_UNAVAILABLE`; optional erased Evidence remains typed unavailable.
- Decrypted plaintext digest mutation: rejected as integrity failure.
- Plaintext Debug mutation: RED exposed an objective string -> debug surfaces now emit `[redacted]`.
- Caller-controlled page size caused four full-prefix checkpoint validations in the two-page catch-up fixture: RED -> domain-owned 10,000-event checkpoint intervals plus one exact final save reduce the fixture to two durable saves and bound restart work without quadratic replay.
- Parsed JSON strings used ordinary stores during cleanup: security review -> every transient string/key byte is overwritten with volatile stores plus a compiler fence before deallocation.
- PostgreSQL direct caller forged valid JSON with a real watermark/head: RED -> provider-authenticated source verification plus deterministic bounded replay on save/swap/read.
- Divergent duplicate checkpoint, reverse checkpoint, stale source head, direct SQL active-pointer mutation, RLS bypass, immutable-row mutation, corrupt JSON, and old format mutations all fail closed.

## Fresh verification evidence

- `graphhelm-events` full all-features suite: GREEN; focused Task 9 projection rebuild is 11/11.
- `graphhelm-governor` full all-features suite: GREEN; focused Task 9 materialization is 7/7.
- PostgreSQL ignored serial suite: 39/39 GREEN (3 concurrency, 3 isolation, 3 migration, 4 projection, 16 repository conformance, 10 retention).
- Workspace tests with all features, workspace Clippy with `-D warnings`, Rustfmt, CLI smoke 10/10, locked metadata, all five canonical CLI commands, secret/no-echo scan, and `git diff --check`: GREEN.
- Independent specification review: `C0/I0/M0`.
- Independent quality/security review: `C0/I0/M1`. The residual Minor is the table-wide `SHARE` lock used to close the absent-stream activation race; a correct per-stream replacement also requires changing the append lock protocol outside the authorized Task 9 scope.
- Linux-target cross-Clippy was attempted with the installed `x86_64-unknown-linux-gnu` Rust target and stopped in `ring` only because this Windows host lacks the external `x86_64-linux-gnu-gcc` linker; native workspace Clippy is GREEN.

## Scope exclusions and rollback

- No backup/restore, CLI/operator commands, CI orchestration, provider SDK, compatibility reader, cache, or plaintext fallback.
- Rollback is the single local Task 9 commit; migration v3 adds only disposable projection tables and does not change authoritative events or Evidence.
