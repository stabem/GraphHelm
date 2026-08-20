# #116 — export-without-credentials: investigation verdict

**Verdict: PARTIAL.** Do not close. The events backup/restore/verify trio genuinely
satisfies "export without credentials" for its own (different) scope — the artifact
itself is credential-free, verified by reading the code, not assumed. But it does not
satisfy PRD §4.10 as written, and "re-execute with equivalent routes" has no
implementation at all, anywhere in the tree. Recommend rescoping #116 to name exactly
what's missing rather than closing it.

## The promise, exact text

`MASTER_PRD.md:89`, §4.10 Reproducibility:

> Graphs, contracts, versions, policies, agents, context, and artifacts can be exported
> without credentials and re-executed with equivalent routes.

RA §3.2 (`docs/product/ROADMAP_AND_ACCEPTANCE.md:63`) lists `export/replay` as one Phase-1
scope bullet — no further detail there; it's a scope list, not a spec.

## What exists: the events backup/restore/verify trio

`apps/cli/src/commands/events/{backup.rs,restore.rs,verify.rs}` wrapping
`adapters/postgres-event-store/src/backup.rs`'s `PostgresBackupOperator`. This is a
whole-event-store, Postgres-superuser-gated disaster-recovery feature: `backup_to_path`
requires `is_superuser` on the connected role (`backup.rs:956-963`), pins exact
`pg_dump`/`pg_restore` executables by SHA-256 (`PinnedTool::verified_for_use`), and dumps
every `graphhelm_*` table via `pg_dump` piped through an encryption codec.

### Credential check on the artifact itself — verified, not assumed

I read the actual field lists, not just the doc comments claiming redaction:

- `DatabaseProcessProfile` (`backup.rs:530-536`) holds `host`/`port`/`user`/`database`/
  `passfile` — its `Debug` impl (`backup.rs:538-549`) redacts all but `port`. This
  profile is never serialized into the backup output; it's a runtime-only struct used to
  spawn `pg_dump`/`pg_restore` as child processes on the machine running the command.
- `BackupManifest` (`backup.rs:4975-4991`) — the thing that actually travels inside the
  archive — has exactly these fields: `source_identity_sha256`,
  `source_database_semantics`, `schema_migration_version`, `repository_format`,
  `release_catalog_sha256`, `migration_sha256`, `counts`, `pg_dump_version`,
  `pg_dump_sha256`, `pg_restore_version`, `pg_restore_sha256`, `runtime_role` (a role
  *name*, not a secret), `provider_epoch`, `state_summary_sha256`,
  `privilege_summary_sha256`. Every field is a hash, a count, a version string, or an
  identity digest. Nothing here is a password, DSN, or key.
- The dump payload itself (`pg_dump`'s stdout) is piped straight into
  `BackupCodec::encrypt` (`backup.rs:1154-1156`) keyed by `KeyProvider` — a
  project-level encryption key, not the database's own credentials. The DB password
  lives only in the `passfile` path handed to `pg_dump`'s process, which is never read
  back into anything that gets written to `destination`.
- `restore_from_path` (`backup.rs:1187` on) takes the *caller's own* fresh
  `admin_pool`/`profile` for the **target** database — credentials for the second
  machine are supplied by whoever runs `restore`, never extracted from the archive. This
  is what makes the round-trip genuinely credential-free: the artifact carries identity
  proofs to *verify* it's landing somewhere new (`restore_from_path_owned:1247-1250`
  refuses a restore where target identity already equals source identity), not
  credentials to *authenticate* anywhere.

So: for this specific artifact, "exported without credentials" is TRUE, and I'd stake
the verdict on having read the fields rather than trusted the module doc comment.

### Where it falls short of the promise as written

1. **Wrong scope, wrong persona.** §4.10 lists "graphs, contracts, versions, policies,
   agents, context, and artifacts" — a Graph Engineer's per-graph portability unit. What
   ships is an admin-only, whole-database snapshot requiring Postgres `rolsuper`. There
   is no way to export *one graph's* context/policies/versions without superuser access
   to the entire event store, and no way to get less than everything.
2. **No CLI/API surface named `export` exists at all.** `grep -rn "\"export\"|fn export"
   apps/cli/src` returns zero matches. `docs/DECISION_REGISTER.md` has zero mentions of
   "export" anywhere — no §8 acceptance clause covers this promise, confirming it was
   never grounded as a decided, tracked surface.
3. **`export.include_secrets`** (`docs/graph-engineer/GRAPH_ENGINEER_GUIDE.md:376`) is
   an illustrative string inside a *Policy DSL example* teaching engineers how to write
   `deny:` rules — not a real, implemented affordance. It shows the naming vocabulary
   anticipates an export feature; it is not evidence one exists.
4. **"Re-execute with equivalent routes" has no implementation anywhere.** The only
   candidate is `graph replay` (`apps/cli/src/commands/replay.rs:34`), which calls
   `graphhelm_events::replay(&scope, &stream_id, &events)` — a pure fold of the recorded
   event stream into a read-only `projection`. It reconstructs *state*; it does not
   re-run anything against a tool broker, model gateway, or any route at all, equivalent
   or otherwise. There is no code path in this tree that takes an exported bundle and
   dispatches live work from it on a second deployment. Corroborates memory note
   `disagreeing-measurements`/the earlier session's own withdrawn "replay byte-identical"
   claim: replay's job is audit/reconstruction, not execution.

## Recommendation

Not SATISFIED (the promise as written is broader than what ships) and not fully ABSENT
(the backup trio is real, working, and genuinely credential-free within its own scope).
**PARTIAL.** Rescope #116 rather than close it, naming precisely two missing surfaces so
future work has a real target instead of a vague promise:

- **(a)** A graph/context-level export manifest — bundle one graph's declared
  contracts/versions/policies/agent config/context without requiring Postgres superuser
  access to the whole store. Distinct feature from `events backup`, smaller scope, aimed
  at the Graph Engineer persona §4.10 actually describes.
- **(b)** Re-execution against equivalent (i.e., differently-configured, same-shape)
  tool/model routes from an exported bundle. `graph replay` does not do this today and
  was never meant to — it's a state-fold, not a driver re-run. This needs its own design,
  likely sitting closer to `core/runtime`'s driver than to the event store.

Neither (a) nor (b) exists in any form — draft, partial, or stubbed — anywhere I found in
this tree. Both would need their own issues if the orchestrator wants to carry them
forward; #116 as titled ("surface unverified/absent") is now answered: verified, and
absent at the scope the PRD promises, present only at a different, narrower scope.

## Evidence index

- `MASTER_PRD.md:87-90` — §4.10 exact text.
- `docs/product/ROADMAP_AND_ACCEPTANCE.md:41-64` — RA §3.2 scope list.
- `apps/cli/src/commands/events/backup.rs:1-55` — CLI wrapper, `require_absent_file`,
  redacted `operator_error`.
- `adapters/postgres-event-store/src/backup.rs:530-549` (`DatabaseProcessProfile` +
  redacted `Debug`), `:742-751` (`PostgresBackupOperator` fields, also redacted),
  `:875-985` (`new`/`new_bounded`, superuser check at `:956-963`), `:1010-1174`
  (`backup_to_path`/`backup_to_path_owned`, encryption at `:1154-1156`), `:1187-1250`
  (`restore_from_path`/`restore_from_path_owned`, fresh-target-credential requirement,
  identity-mismatch refusal), `:4975-4991` (`BackupManifest` field list).
- `apps/cli/src/commands/replay.rs:8-49` — `graph replay`, fold-only via
  `graphhelm_events::replay`.
- `docs/graph-engineer/GRAPH_ENGINEER_GUIDE.md:357-380` — `export.include_secrets`
  policy-DSL example.
- `docs/DECISION_REGISTER.md` — grepped for "export", zero matches.
- `apps/cli/src` — grepped for `"export"`/`fn export`, zero matches.
- `.factory/h-agent-mvp-gap-analysis.md:38,58,96-97` — origin classification (PARTIAL,
  replay shipped / export surface unverified), issue #116 traced back to item 21.

---

## Disposition (orchestrator, 2026-08-20): split, two drafts below

Accepted as PARTIAL, not closing. Split per packaging precedent — different homes, a
merged issue neither fix could close alone. #116 itself gets rescoped to the
export-manifest surface; the re-execution half becomes a new issue. Orchestrator posts
both; drafted here so the text is reviewable before it goes anywhere public.

### Draft A — comment to post on #116, plus retitle

**New title:** `Export-without-credentials: per-graph portability manifest — surface
does not exist (rescoped from broader #116)`

**Comment body:**

> Investigated (see `.factory/e-agent-116-verdict.md` for full citations). Verdict:
> **PARTIAL**, not closing — rescoping instead, and splitting the promise's two halves
> into separate issues since they live in different parts of the codebase and neither
> fix could close a merged issue alone.
>
> **What's settled, so nobody re-investigates it:** the `events backup`/`restore`/
> `verify` trio (`apps/cli/src/commands/events/`, backed by
> `adapters/postgres-event-store/src/backup.rs`) genuinely satisfies "exported without
> credentials" — for its own artifact. Verified by reading the actual field lists, not
> by trusting the module's doc comments: `DatabaseProcessProfile` (host/user/passfile)
> is never serialized into the output (`backup.rs:530-549`); `BackupManifest`
> (`backup.rs:4975-4991`), the thing that actually travels inside the archive, holds
> only hashes, counts, versions, and identity digests — no field carries a password,
> DSN, or key; `restore_from_path` takes the *operator's own fresh* target credentials
> and never extracts source credentials from the archive (`backup.rs:1187-1250`). That
> half of §4.10 holds today, for that artifact.
>
> **What doesn't hold, and why this issue survives rescoped rather than closing:** §4.10
> promises "graphs, contracts, versions, policies, agents, context, and artifacts can be
> exported" — a per-graph portability unit for the Graph Engineer persona (§5.2). What
> ships is a whole-event-store, Postgres-superuser-gated disaster-recovery backup: no way
> to export less than everything, no way to do it without `rolsuper`. No `export` CLI/API
> surface exists anywhere (`apps/cli/src` greps clean), and no `docs/DECISION_REGISTER.md`
> §8 clause covers this promise — it was never grounded as a tracked, decided surface.
>
> **This issue, rescoped:** build a credential-free-by-construction per-graph/context
> export manifest — same no-credentials property the DR backup already proves is
> achievable in this codebase, but scoped to one graph's declared contracts/versions/
> policies/agent config/context, not the entire store, and reachable without database
> superuser access.
>
> The other half of §4.10 — "re-executed with equivalent routes" — is split out to
> **#\<NEW_ISSUE_NUMBER\>**, since it belongs near `core/runtime`'s driver, not here.

### Draft B — new issue body

**Title:** `Re-execution against equivalent tool/model routes — no implementation
(split from #116)`

**Body:**

> Second half of PRD §4.10 (`MASTER_PRD.md:89`): "...and re-executed with equivalent
> routes." Split out of #116 (export-without-credentials) because this half lives near
> `core/runtime`'s driver, not the event store, and neither half's fix could close a
> merged issue alone.
>
> **Opening fact, confirmed by reading the code, not assumed:** the only existing
> candidate is `graph replay` (`apps/cli/src/commands/replay.rs:34`), which calls
> `graphhelm_events::replay(&scope, &stream_id, &events)` — a pure fold of the recorded
> event stream into a read-only `projection`. It reconstructs state for audit; it does
> not dispatch anything against a tool broker, model gateway, or any route, equivalent or
> otherwise. There is no code path anywhere in this tree that takes an exported bundle
> and runs live work from it against a *different* (but equivalently-shaped) set of
> tool/model routes on a second deployment. (This also confirms, independently, an
> earlier session's own withdrawn "replay byte-identical" claim: replay's job was always
> audit/reconstruction, never re-execution — the two commands share a name-adjacent
> vocabulary but not a mechanism.)
>
> **Scope:** given an exported graph bundle (see #116's rescoped export-manifest work),
> re-execute it end to end against a target environment's own tool/model routes —
> different credentials, different endpoints, same contract shape — and reach an
> equivalent outcome. This is a driver-level concern: it needs `core/runtime`'s dispatch
> path to accept an externally-supplied route configuration for an execution reconstructed
> from an export, not the event store's replay/fold path.
>
> **Dependency note:** meaningfully blocked on #116's rescoped export-manifest existing
> first, or at minimum on its shape being drafted — there's nothing to "re-execute" from
> until an export format exists to re-execute.
>
> ---
> *Split from #116's investigation, 2026-08-20. Source: `.factory/e-agent-116-verdict.md`.*
