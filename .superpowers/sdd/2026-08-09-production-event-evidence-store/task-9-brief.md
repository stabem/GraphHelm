# Task 9 — safe projections and executable materialization

## Authority and exact scope

- Base: `53aa94dd722db214179fb86b7df9aa6a7f31ff65`; issue `#5`; local worktree; no push/PR.
- User authorization on 2026-08-11: because no Task 9 brief existed, use only the Task 9 section of `docs/superpowers/plans/2026-08-09-production-event-evidence-store.md` as the official brief.
- Exact production/test scope from that section:
  - modify `core/events/src/lib.rs` and `core/events/src/projection.rs`;
  - create `core/events/tests/projection_rebuild.rs`;
  - create `core/governor/src/materialize.rs` and `core/governor/tests/materialization.rs`;
  - create `adapters/postgres-event-store/migrations/0003_projections.sql`, `adapters/postgres-event-store/src/projection.rs`, and `adapters/postgres-event-store/tests/projection.rs`;
  - modify `adapters/postgres-event-store/src/lib.rs`;
  - create this substitute brief and the Task 9 evidence report/ignored-test ledger required by the authorized review/gate workflow.
- No dependency/version/feature, protocol/schema/catalog/golden, Task 10 backup/restore, Task 11 CLI/configuration, provider SDK, legacy compatibility, fake success, push or PR.
- User-authorized compatibility exception on 2026-08-11: update only the legacy PostgreSQL migration test's table count from 14 to 16 and its synthetic unknown migration version from 3 to 4, because Task 9 legitimately adds two scoped tables and migration version 3.

## Required behavior

- Pure, bounded handlers cover all 16 safe event variants without Evidence plaintext.
- Generation watermarks bind exact scope, stream, last sequence/hash, projection name/version and generation.
- Rebuild writes a fresh disposable generation, resumes safely, catches up to an observed authenticated source head, rechecks an unchanged head before atomic swap, and leaves the old generation active on every failure.
- Executable materialization resolves exact same-scope Evidence ID/digest bindings, decrypts only through `KeyProvider`, reconstructs ephemeral authoring/executable content in memory and zeroizes transient buffers.
- Required unavailable content returns `GHE008_CONTENT_UNAVAILABLE` before model/tool/shell/network/deploy effects. Optional unavailable content remains explicitly unavailable. No cache, fallback or legacy reader.
- PostgreSQL migration/storage uses composite scope, bounded pages, immutable generations/watermarks, FORCE RLS, least privilege, exact migration ledger/checksum and safe concurrent swap semantics.

## TDD and completion gates

- Strict RED → GREEN → REFACTOR for each behavior and mutation.
- Cover empty/all-event replay, page boundaries and limits, interrupted resume, head advance, corrupt events/watermarks, unsupported projection version, failed generation swap, required/optional unavailable content, digest/scope/owner/ordinal mismatch, decrypt/authentication failure, successful reconstruction, RLS/concurrency/immutability and no-echo scans.
- Independent spec review must reach C0/I0 before independent quality/security review; quality/security must reach C0/I0 before commit.
- Run focused/core/PostgreSQL gates, real disposable PostgreSQL serial suite, Windows workspace gates, Linux-target Clippy or record the exact external blocker, schema/catalog/conformance and canonical CLI smokes.
- Commit locally as `feat(events): add safe replay projections`, `Refs #5`, repository co-author; no push/PR; remove disposable infrastructure and confirm a clean worktree.
