# Task 11 brief - operator CLI, CI, documentation, and final gate

## Authority

The Task 11 section of `docs/superpowers/plans/2026-08-09-production-event-evidence-store.md`
(lines 1174-1244) is the authoritative brief. The user directed continuation to Task 11 immediately
after the Task 10 commit `649c193`. This is the final task of issue `#5`.

## Exact scope

Files named by the plan:

- Modify: `apps/cli/Cargo.toml`, `apps/cli/src/args.rs`, `apps/cli/src/commands/mod.rs`.
- Create: `apps/cli/src/commands/events/{mod,config,verify,rebuild,backup,restore}.rs`.
- Create: `apps/cli/tests/event_store_cli.rs`.
- Modify: `.github/workflows/ci.yml`.
- Create: `ci/postgres.ps1`.
- Create: `docs/milestones/production-event-evidence-store.md`.
- Modify: `docs/operations/OBSERVABILITY_AND_RECOVERY.md`, `README.md`, `CHANGELOG.md`.
- Create this brief, the Task 11 report, and the ignored-test ledger.

Mechanically necessary and considered inside the plan's explicit `apps/cli/Cargo.toml` allowance:
the CLI gains the existing workspace crates `graphhelm-postgres-event-store` and `tokio` so the
operator commands can reach the repository and backup operator. No new crate and no new version
enters the workspace; both already exist at pinned workspace versions.

Out of scope: any event import or repository-format migration command, compatibility readers,
schema/migration or core protocol change, production credentials, deployment, hosted
infrastructure, and push or PR. Generic schema evolution commands remain unchanged.

## Required behavior

- Operator commands are `events verify`, `events rebuild`, `events backup`, and `events restore`,
  JSON-only and bounded, consistent with the existing `Outcome` exit-code contract.
- Config loads bounded JSON from an explicit path or environment, rejects symlinks and insecure
  permissions where the platform supports it, and redacts DSNs and filesystem paths from every
  public error, diagnostic, and debug surface.
- An unsupported repository format fails with `GHE007_UNSUPPORTED_FORMAT` and exit code 2, and the
  output must never mention a legacy or import fallback, because none exists.
- Subprocess handling uses argument arrays with no shell, bounded stderr, a timeout, kill/wait, and
  cleanup.
- Mutually exclusive flags, verify ranges, rebuild generations, and backup/restore targets are
  validated before any expensive or effectful work.
- No plaintext, filesystem path, DSN, or backtrace appears in normal output.
- CI runs Windows and Ubuntu jobs that discover the runner's installed PostgreSQL, create a random
  cluster, database, and roles on a random port, export `GRAPHHELM_TEST_ADMIN_URL`, run the ignored
  tests serially, and always stop and remove the cluster. No Docker and no service container.
- Documentation covers architecture, trust boundaries, local and PostgreSQL formats,
  externalization, hash semantics, erasure and materialization, RLS roles, key rotation, retention,
  projection rebuild, backup and restore, limits, diagnostics, the developer-data reset, and why no
  compatibility layer exists. Rollback is code rollback plus restore from a matching authenticated
  backup; it never downgrades repository bytes in place.
- All documentation is written in English, per the repository policy in `AGENTS.md`.

## Carried-over obligation from Task 10

The 41 ignored PostgreSQL tests were not re-run against commit `649c193` because the disposable
container had been removed and the Docker daemon would not start on that host. Task 11 Step 6
requires running all ignored tests with `--test-threads=1`, and Step 4's Docker-free cluster script
is the mechanism that discharges it. The pinned EDB 16.14 distribution under `target/task10-tools`
includes `initdb`, `pg_ctl`, and `postgres`, so a local cluster can be created without Docker. This
obligation is closed only by an observed passing run recorded in the Task 11 ledger.

## Process and acceptance

Every behavioral change follows an observed RED -> GREEN cycle. Independent review order is
specification C0/I0, then quality/security C0/I0, followed by the whole-branch reviews required by
the plan's "Required Final Review" section. Final delivery is one local commit carrying `Closes #5`,
with the complete clean-state gate observed and the worktree clean afterwards. No push and no PR.
