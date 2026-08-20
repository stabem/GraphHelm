# #106 — SSH/Docker bootstrap blueprint

Read-only design deliverable. RA §3.2 (`docs/product/ROADMAP_AND_ACCEPTANCE.md:44`, Scope: "SSH/Docker
bootstrap"); acceptance step 1 (`ROADMAP_AND_ACCEPTANCE.md:79`): "user installs Runtime on a clean
VPS." No code changes in this document — every claim below is either cited to a file:line or marked
as unmeasured/aspirational (CITE-or-MARK). Built from two independent code surveys of this worktree
(`F:\github\GraphHelm\.claude\worktrees\f-agent-8dc3ee`), cross-checked against each other where they
overlap.

## The sharpest finding first

**The code's own loopback-only enforcement fights the decision register's stated install mechanism.**
`D-002` (`docs/DECISION_REGISTER.md:8`): "installation and updates via Docker." But `serve`'s `--bind`
is validated by `parse_loopback_bind` (`apps/cli/src/commands/serve/mod.rs:1094-1105`), which hard-
refuses any non-loopback address: *"--bind must name a loopback address; the Public Runtime API is
never exposed beyond localhost."* This is not a doc statement, it's an `Err` path in the binary.

A naive Docker deployment — build an image, `docker run -p 8080:8080`, done — does not work against
this binary as written: the container's `127.0.0.1` is its own network namespace's loopback, not the
host's, so a process inside the container bound to `127.0.0.1` is unreachable via a published port
regardless of the `-p` mapping. Two ways out, neither built today:

- **`--network=host`** on the container, so the container's loopback IS the host's loopback. Zero
  code changes; changes only how the container is launched.
- **A new, explicit flag** (e.g. `--bind 0.0.0.0:PORT --i-understand-this-exposes-the-api` or a
  separate `--allow-non-loopback`) that relaxes the check for containerized deployments. This is a
  real code change to `parse_loopback_bind`, not a bootstrap-script decision, and it reopens the
  "never exposed beyond localhost" guarantee the current code makes unconditionally — worth a design
  discussion on its own, not a rider on this blueprint.

Recommendation below routes around this rather than resolving it: ship the binary+systemd path first
(zero code changes, the loopback constraint is exactly what SSH-tunnel access wants), treat Docker as
a fast-follow that either adopts `--network=host` or waits for the flag.

## 1. What the Runtime actually needs on a clean host

**Binary.** `apps/cli` is the only `[[bin]]` target in the workspace (`apps/cli/Cargo.toml:8-10`,
name `graphhelm`); every other crate is a library. `serve` is a subcommand of that one binary
(`apps/cli/src/args.rs:18-38`), not a separate daemon binary.

**Startup, in the order the code runs it** (`apps/cli/src/commands/serve/mod.rs:149-167`):
1. Parse `--bind` as loopback (`mod.rs:150`, `1094-1105`) — required flag, no default, no env-var
   alternative.
2. `create_dir_all(--events)` (`mod.rs:151-152`) — required flag, no default.
3. Create-or-load a 32-byte bearer token file, sibling to `--events` (named
   `<events-dir-basename>.token`, deliberately outside the events directory so it doesn't collide
   with the event store's own closed layout allow-list — `mod.rs:1107-1133`; mode `0o600` on Unix,
   `mod.rs:1163-1179`).
4. Optionally validate the real-executor flag group (`--manifest --broker --route --staging` +
   `--keyring --key-id`) and read the route manifest if present (`mod.rs:154`, `176-261`). **No env
   var is read at startup, ever** — `GRAPHHELM_GATEWAY_KEY`/`GRAPHHELM_EVENTS_KEY` are read lazily,
   per-request, only when a drive actually needs them (`ports.rs:66-95`, `251-282`).
5. Bind the one TCP listener, print one JSON line to stdout naming the actually-bound address
   (`mod.rs:263-287`), then `axum::serve` forever — no shutdown endpoint, no config reload.

**Env vars** (full inventory, non-test code): `GRAPHHELM_GATEWAY_KEY` and `GRAPHHELM_EVENTS_KEY`
(64 lowercase-hex chars, validated at `ports.rs:288-299` / `257-268`) are read only if the real-
executor/sealing flag groups were passed AND a drive that needs them actually runs — a `serve`
launched without those flags never touches either var. `GRAPHHELM_EVENTS_CONFIG` and
`GRAPHHELM_API_TOKEN` belong to other subcommands (`events {verify,rebuild,backup,restore}` and
`mcp` respectively), never to `serve`. **No `GRAPHHELM_PG_*` connection env vars exist anywhere** —
Postgres configuration is a JSON file, not env vars (see below). Notable hardening already in place:
any env var named `GRAPHHELM_*` is structurally refused from being forwarded into a brokered tool's
child process (`adapters/tool-host/src/process.rs:80-94,87`), so secrets can't leak sideways into
sandboxed tool execution.

**Ports.** Exactly one TCP bind, from `--bind`, loopback-enforced (see above). MCP is not a second
port — `graphhelm mcp` is a separate process that speaks JSON-RPC over stdio and is itself an HTTP
*client* of `serve` (`apps/cli/src/commands/mcp/mod.rs`, `args.rs:29-31`), also loopback-restricted
on its `--url` (`mcp/mod.rs:35-40`).

**Filesystem.** `--events <dir>` — created at startup, then governed by `classify_layout`'s closed
six-entry allow-list (`core/events/src/local.rs:2093-2150`): `blobs/`, `.tmp/`, `active/`,
`journal.jsonl`, `repository.lock`, `format.json`. Nothing else may exist at that root or the store
refuses to open. The token file sits *beside* it, not inside. No log file is written anywhere; the
one stdout JSON line is the entire startup signal.

**PostgreSQL is not required for `serve`.** Confirmed two ways: the one `event_store()` helper every
command including `serve` goes through is hardcoded to `LocalEventRepository::open`
(`apps/cli/src/commands/mod.rs:282-286`), and `graphhelm-postgres-event-store` is imported only by
`commands/events/{backup,config,rebuild,restore,verify}.rs` — never by anything under
`commands/serve/`. Postgres is an entirely separate, optional surface for backup/restore/rebuild/
verify against an alternate event-store backend, configured by a JSON file (`--config` /
`GRAPHHELM_EVENTS_CONFIG`, `apps/cli/src/commands/events/config.rs:18-30`), not env vars. If that
surface IS used: PostgreSQL 16+ (`adapters/postgres-event-store/tests/backup_restore.rs:1691-1693`,
stated for a `datlocale`/`daticulocale` compatibility reason, not a formally declared minimum
elsewhere), 4 raw-SQL migrations applied programmatically via `sqlx::migrate::Migrator`
(`adapters/postgres-event-store/src/lib.rs:187-207`), zero `CREATE EXTENSION` statements required.

## 2. The smallest honest install story

**Nothing exists today.** Confirmed absent, exhaustively: no `Dockerfile`, no `docker-compose*`, no
`.dockerignore`, no install/bootstrap/setup/provision script anywhere in the tree, no `.github/`
directory. `README.md:40` says so directly under "Does not exist yet, said plainly": *"Anything
describing a VPS daemon deployment story."* `AGENTS.md:129` lists "SSH bootstrap, Docker/Podman
orchestration" as explicitly out of scope for the first milestone. This blueprint starts from zero,
not from an unfinished partial system.

**What "clean VPS" should mean, operationally, and why:** Ubuntu 22.04 or 24.04 LTS, x86_64. Nothing
in the repo names an OS (`"Ubuntu"` and `"systemd"` both have zero hits across `docs/**`, `AGENTS.md`,
`README.md`) — this is a recommendation, not a citation. It's chosen to rhyme with the owner's actual
fleet, not invented: every server in the owner's registry (`dale-main`, `multi`) already runs Ubuntu
22.04/24.04, and those services are already operated via `docker compose` commands over SSH. Picking
anything else means the bootstrap teaches a second operational pattern for no reason.

**Recommended shape, in order, matching the owner's own `deploy/local-deploy.sh` convention rather
than inventing a new one** (that convention: per-repo script, run from the local clone, never writes
the server's `.env` — the server's own `.env` is the secrets source of truth, `--dry-run` shows
intent, `DEPLOY_REF=<sha>` pins/rolls back):

1. **Phase 1 (ships now, zero code changes): binary + systemd + SSH tunnel.** A `deploy/vps-
   bootstrap.sh`, run from the local clone against a target host over SSH — same shape as the
   owner's existing `local-deploy.sh` scripts. It: checks the target is Ubuntu 22.04/24.04 x86_64,
   builds `graphhelm` (either cross-compiled locally and `scp`'d, or built on the VPS from a
   `git pull` — the repo already depends on `sqlx` with `tls-rustls-ring-native-roots`, pure-Rust
   TLS, which is favorable for a static/musl build, but nothing in the repo configures that path
   today — MARK: unmeasured, would need its own spike), creates the events directory, writes a
   systemd unit running `graphhelm serve --bind 127.0.0.1:<port> --events <dir>` with
   `Restart=on-failure`, and prints the SSH-tunnel command the operator (or Studio, once it exists)
   uses to reach it: `ssh -L <local-port>:127.0.0.1:<port> <host>`. This satisfies the code's
   loopback-only constraint *by construction* — it's the constraint the code already enforces, not a
   workaround — and rhymes exactly with `ADR-002` (`docs/reference/REFERENCE_STACK_AND_ADRS.md:62-
   74`: "SSH only for bootstrap/maintenance").
2. **Phase 2 (fast-follow, needs a decision first): Docker.** Matches `D-002`'s literal text and the
   owner's actual day-to-day (`docker compose logs` is already how the owner's other services get
   operated). Blocked on the loopback-vs-container conflict named above — pick `--network=host` (no
   code change, less idiomatic Docker) or land the new bind-relaxation flag (real code change,
   reopens a security guarantee) before writing the compose file. Do not build this phase without
   that decision; building it around `--network=host` silently and revisiting later duplicates work
   and risks shipping the more permissive flag by accident.

`PRODUCT_REQUIREMENTS.md:9-13`'s FR-001 acceptance text ("validates architecture, disk, memory,
Docker/Podman, ports, Git, and persistence; shows a plan before altering the VPS; allows updating and
uninstalling") is the target shape for the bootstrap script's own preflight — MARK: aspirational,
no code implements any of it today.

## 3. The verification story

**`graphhelm doctor` does not exist.** One hit for the string "doctor" in the entire repo, and it's
in a planning document, not a spec or implementation: `docs/superpowers/plans/2026-08-19-arming-the-
alarm.md:124-127`, naming it as a future idea with no enumerated checks. `apps/cli/src/args.rs:18-38`
confirms the full subcommand list has no `Doctor` variant. **Do not design this blueprint's
verification step around an existing diagnostic — there is nothing to invoke.**

What exists instead, usable today as the install-worked signal:
- The one stdout JSON line `serve` prints on successful bind, naming the actual address
  (`mod.rs:271-281`) — the existing mechanism for a caller to confirm what got bound, already used
  by test harnesses for exactly this purpose.
- `GET /health` (`apps/cli/src/commands/serve/mod.rs:291`), exempted from bearer-token auth
  (`mod.rs:362-364`, checked before the token comparison) so the bootstrap script's postflight poll
  needs no credential. Confirmed by reading the handler itself (`mod.rs:334-339`): it is a **pure
  liveness check** — unconditionally returns `200 {}`, touches no event store, no disk, no
  persistence. It answers "is the HTTP server up," nothing about whether the install is actually
  sound.

**Recommendation:** the bootstrap script should implement its own thin preflight + postflight rather
than wait for `doctor` to exist — confirm disk space, confirm the target port is free, run the
install, then poll `/health` over the tunnel as a liveness gate only. `/health`'s liveness-only shape
is exactly why it can't substitute for `doctor`: a bootstrap that stops at "the server answered" has
not confirmed persistence, disk, or the events directory's layout is sound. Propose `graphhelm
doctor` as its own follow-up issue: FR-001's acceptance text already specifies what it should check,
and PRODUCT_REQUIREMENTS.md's own criteria (architecture, disk, memory, Docker/Podman, ports, Git,
persistence) would give the bootstrap script something durable to call instead of hand-rolling checks
that rot.

## 4. Explicitly out, named

- **TLS on the HTTP surface.** Not designed, not needed for Phase 1 — the SSH tunnel already
  encrypts the transport, and the binary refuses to bind anywhere the tunnel doesn't reach. Becomes
  mandatory and is currently wholly unaddressed the moment Phase 2 (Docker, non-loopback exposure)
  ships without a tunnel in front of it. `sqlx`'s `tls-rustls-ring-native-roots` feature secures the
  Postgres connection only, not the HTTP server — do not conflate the two.
- **Multi-tenant.** The Postgres migrations carry `workspace_id`/`project_id`-scoped row-level
  security (`adapters/postgres-event-store/migrations/0001_event_evidence.sql`,
  `0004_scope_guard.sql`) — so multi-tenancy was considered for that backend — but `serve`'s default
  local-filesystem event store shows no per-tenant scoping anywhere the surveys found. Out of scope
  for a single-operator VPS bootstrap regardless; named so a future design doesn't assume it's
  covered.
- **Updates and rollback.** `docs/operations/QUALITY_GATES_AND_DEPLOYMENT.md:32` describes
  `--dry-run` and `DEPLOY_REF=<sha>` as "mandatory affordances of the deploy step" — but this is
  spec prose; no such flags exist in `apps/cli` today (confirmed: no matching CLI args found). The
  owner's own `local-deploy.sh` pattern already implements exactly this shape elsewhere in the
  fleet — that's prior art to copy for `vps-bootstrap.sh`'s update path, not a gap to design from
  scratch, but it's still unbuilt and out of scope for this document.
- **`graphhelm doctor` itself.** Named above — worth its own issue, not bundled into this one.
- **Non-Ubuntu targets, firewall/hardening beyond the loopback bind, anything beyond a plain
  `Restart=on-failure` systemd unit** (health-triggered restart, log rotation, alerting). First cut
  only.

## Feeds M11

This is a design deliverable, not a task list — the two phases above (systemd/SSH now, Docker after
the bind-relaxation decision) and the two named follow-ups (`graphhelm doctor`, the `--bind`
relaxation flag) are the concrete items for M11 planning to pick up.
