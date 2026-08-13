# Task 11 report - operator CLI, CI, documentation, and final gate

## Authority and scope

- Implemented from the Task 10 commit `649c193` under issue `#5`, following the Task 11 section of
  the plan as the authoritative brief.
- The user authorized the collation fix, which modifies the already-committed Task 10 adapter and
  recomputes a security-relevant constant.
- The CLI gained only existing workspace crates: `graphhelm-postgres-event-store`,
  `graphhelm-sealed-key-provider`, `sqlx`, and `tokio`. `Cargo.lock` grew by four lines and no new
  external crate or version entered the workspace.

## Delivered behavior

- `events verify`, `events rebuild`, `events backup`, and `events restore` are JSON-only and bounded,
  reusing the existing `Outcome` exit-code contract. Argument and configuration validation completes
  before any repository is opened or any connection is made.
- Configuration is bounded JSON from `--config` or `GRAPHHELM_EVENTS_CONFIG`: at most 64 KiB,
  symbolic links rejected, non-regular files rejected, and group or world accessible modes rejected
  on Unix. `deny_unknown_fields` applies throughout.
- The 32-byte key never appears in the configuration file. It is supplied out of band through
  `GRAPHHELM_EVENTS_KEY` as 64 lowercase hexadecimal characters, so a leaked configuration alone
  cannot unwrap Evidence.
- No public failure carries a filesystem path, DSN, credential, or backtrace. `OperatorConfig`
  renders its DSN as `[redacted]` in `Debug`, and every loader error reports a fixed message with a
  JSON Pointer.
- An unsupported repository format fails with `GHE007_UNSUPPORTED_FORMAT` and exit code 2, and the
  output never mentions a legacy path, import, downgrade, or migration, because none exists.

## Capability boundaries discovered during implementation

Two command shapes were corrected against the real API rather than assumed:

- `LocalEventRepository` implements `EventRepository`, which has no `verify_range`; only
  `AsyncEventRepository` does. `events verify --repository` therefore recognizes the stored format,
  and a range supplied against a local repository is refused rather than silently ignored.
- `ProjectionRepository` is implemented only by the PostgreSQL adapter, so `events rebuild` is
  configuration-driven. There is no local projection generation to swap.

`events restore` originally accepted `--target`, validated it, and then discarded it, because
`restore_from_path` restores into the database named by the configuration's `adminUrl` and refuses to
proceed unless that database is already empty. A flag that does not control what it names is worse
than no flag, so it was removed and a test now asserts its absence.

## RED to GREEN and adversarial evidence

- `apps/cli/tests/event_store_cli.rs` was written first and observed RED at 16 failed of 18. The two
  passing tests were correct at that point: no `events` command existed, so no import or migration
  subcommand could exist either.
- GREEN at 20/20 after implementation, including the two tests added while removing `--target`.
- Coverage: unsupported format with no fallback, absent import/migrate/upgrade/convert subcommands,
  mutually exclusive and required selectors, verify range bounds, incomplete scope, rebuild
  generation bounds, existing backup target preserved byte for byte, missing and non-regular
  archives, oversized and malformed configuration, out-of-range process timeout, relative tool path,
  non-SHA-256 tool digest, environment-supplied configuration, Unix permission and symlink
  rejection, secret/path/backtrace redaction, and single bounded envelope output.

## Continuous integration

`ci/postgres.ps1` creates a throwaway cluster from an already installed PostgreSQL on a random free
port bound to `127.0.0.1`, creates the database, exports `GRAPHHELM_TEST_ADMIN_URL`,
`GRAPHHELM_TEST_PG_DUMP` and `GRAPHHELM_TEST_PG_RESTORE`, runs the ignored tests serially, and always
stops and removes the cluster through `finally`. No Docker and no service container. The workflow
adds a `postgres` job on `ubuntu-latest` and `windows-latest`; the existing `rust` job is unchanged.

Making that script actually work exposed four defects, recorded with their mechanisms in the Task 11
ledger: a collation-dependent schema contract hash, a randomized administrative role contradicting a
test that requires the conventional `postgres` superuser, two distinct PowerShell hangs caused by the
detached server inheriting output handles, and Windows PowerShell 5.1 promoting redirected native
stderr into a terminating error.

## Fresh verification evidence

- Full ignored PostgreSQL matrix through the stock script: **41 passed, 0 failed**, serially, with
  cluster processes and temporary directories confirmed absent afterwards.
- Workspace Rustfmt and Clippy with `-D warnings`: passed.
- Workspace all-feature tests: passed with zero failures.
- `cli_smoke` 10/10, `schema_cli` 27/27, `event_store_cli` 20/20.
- `schema catalog` and `schema conformance` returned `ok`; locked metadata succeeded;
  `git diff --check` passed.

## Rollback and exclusions

- Rollback is code rollback plus restoration from a matching authenticated backup. It never
  downgrades repository bytes in place.
- The Runtime API, authentication and RBAC, scheduling, artifact byte stores, Knowledge Graph,
  Studio, hosted services, and telemetry export remain separate milestones. Key rotation and
  external KMS adapters do not exist; their absence is documented as a trust limitation rather than
  implicit authorization.
